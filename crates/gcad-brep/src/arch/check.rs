use super::{Tier, Wall};
use crate::Model;
use crate::cloud_inspect::CheckRecord;
use crate::parse::{Line, eval};
use anyhow::{Result, anyhow, bail};
use gcad_mesh::arch::Layer;

const RSI: f64 = 0.13;
const RSE: f64 = 0.04;
const AIR_GAP: f64 = 0.18;
const INSULATING: f64 = 0.1;

pub(crate) const CHECKS: &[&str] = &[
    "uvalue",
    "heatloss",
    "bridges",
    "daylight",
    "stairs",
    "ramps",
    "doors",
    "egress",
    "rooms",
    "compartments",
    "shadow",
];

pub(crate) fn named_value(line: &Line, key: &str) -> Option<String> {
    line.named.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

impl Model {
    pub(crate) fn conductivity(&mut self, material: &str) -> Result<f64> {
        self.ensure_library()?;
        self.library
            .get(&crate::materials::slug(material))
            .and_then(|e| e.mechanics.conductivity)
            .ok_or_else(|| anyhow!("no conductivity known for `{material}`"))
    }

    pub(crate) fn resistance(&mut self, layers: &[Layer]) -> Result<f64> {
        let mut r = RSI + RSE;
        for layer in layers {
            r += if layer.material == "air" {
                AIR_GAP
            } else {
                layer.thickness / 1000.0 / self.conductivity(&layer.material)?
            };
        }
        Ok(r)
    }

    pub(crate) fn wall_u(&mut self, wall: &Wall) -> Result<f64> {
        let tier: &Tier = wall
            .tiers
            .iter()
            .max_by(|a, b| (a.to - a.from).total_cmp(&(b.to - b.from)))
            .ok_or_else(|| anyhow!("wall has no layers"))?;
        let layers = tier.layers.clone();
        Ok(1.0 / self.resistance(&layers)?)
    }

    pub(crate) fn record_check(&mut self, line: &Line, passed: bool, detail: String) {
        self.checks.push(CheckRecord {
            line: line.number,
            text: line.text.clone(),
            passed,
            detail,
        });
    }

    pub(crate) fn finish_check(&mut self, line: &Line, problems: Vec<String>, ok: String) -> Result<String> {
        let passed = problems.is_empty();
        let text = if passed { ok } else { problems.join("; ") };
        self.record_check(line, passed, text.clone());
        if !passed && named_value(line, "strict").is_some_and(|v| v != "0") {
            bail!("check failed: {text}");
        }
        Ok(text)
    }

    pub(crate) fn arch_check(&mut self, line: &Line) -> Option<Result<String>> {
        let kind = line.positional.first()?.as_str();
        if !CHECKS.contains(&kind) {
            return None;
        }
        Some(match kind {
            "uvalue" => self.check_uvalue(line),
            "heatloss" => self.check_heatloss(line),
            "bridges" => self.check_bridges(line),
            _ => self.check_more(line, kind),
        })
    }

    fn check_more(&mut self, line: &Line, kind: &str) -> Result<String> {
        bail!("`check {kind}` is not available yet in {}", line.text)
    }

    fn wall_named(&self, line: &Line, at: usize) -> Result<Wall> {
        let name = line
            .positional
            .get(at)
            .ok_or_else(|| anyhow!("name a wall, like `check {} w1`", line.positional[0]))?;
        self.arch
            .wall(name)
            .cloned()
            .ok_or_else(|| anyhow!("no wall `{name}`"))
    }

    fn check_uvalue(&mut self, line: &Line) -> Result<String> {
        let wall = self.wall_named(line, 1)?;
        let u = self.wall_u(&wall)?;
        let mut problems = Vec::new();
        if let Some(max) = named_value(line, "max") {
            let max = eval(&max, &self.scope)?;
            if u > max {
                problems.push(format!("{} U {u:.3} over {max}", wall.name));
            }
        }
        self.finish_check(line, problems, format!("{} U {u:.3} W/m²K", wall.name))
    }

    fn check_heatloss(&mut self, line: &Line) -> Result<String> {
        let dt = match named_value(line, "dt") {
            Some(t) => eval(&t, &self.scope)?,
            None => 20.0,
        };
        let walls: Vec<Wall> = match line.positional.get(1) {
            Some(_) => vec![self.wall_named(line, 1)?],
            None => self.arch.walls.clone(),
        };
        let mut total = 0.0;
        let mut names = Vec::new();
        for wall in &walls {
            let gross = wall.line.length() * (wall.top - wall.base) / 1.0e6;
            let holes: f64 = self
                .arch
                .openings
                .iter()
                .filter(|o| o.host == wall.name)
                .map(|o| o.w * o.h / 1.0e6)
                .sum();
            let u = self.wall_u(wall)?;
            total += (gross - holes) * u * dt;
            names.push(wall.name.clone());
        }
        let label = if walls.len() == 1 { names[0].clone() } else { "building".to_string() };
        self.finish_check(
            line,
            Vec::new(),
            format!("heat loss {label} {total:.1} W at dt {dt}"),
        )
    }

    fn check_bridges(&mut self, line: &Line) -> Result<String> {
        let mut flags = Vec::new();
        let slabs = self.arch.slabs.clone();
        let walls = self.arch.walls.clone();
        for slab in &slabs {
            let total: f64 = slab.layers.iter().map(|l| l.thickness).sum();
            let (z0, z1) = (slab.top - total, slab.top);
            for wall in &walls {
                if z1 <= wall.base || z0 >= wall.top {
                    continue;
                }
                let reach = wall.thickness() + 1.0;
                let near = slab.outline.iter().any(|&p| {
                    wall.line
                        .project(p)
                        .is_some_and(|(_, _, across)| across.abs() <= reach + wall.offset.abs())
                });
                if !near {
                    continue;
                }
                for (body, t, l) in wall.layer_names() {
                    let material = &wall.tiers[t].layers[l].material;
                    if self.conductivity(material).is_ok_and(|k| k < INSULATING) {
                        flags.push(format!("{} crosses {body}", slab.name));
                    }
                }
            }
        }
        let ok = "no thermal bridges at slab edges".to_string();
        self.finish_check(line, flags, ok)
    }
}
