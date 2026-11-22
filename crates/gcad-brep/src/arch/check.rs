use super::{Fill, Opening, Tier, Wall};
use crate::Model;
use crate::cloud_inspect::CheckRecord;
use crate::parse::{Line, eval, label};
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
        let warn = named_value(line, "warn").is_some();
        let text = if passed { ok } else { problems.join("; ") };
        self.record_check(line, passed || warn, text.clone());
        Ok(match (passed, warn) {
            (true, _) => text,
            (false, true) => format!("warn: {text}"),
            (false, false) => format!("FAIL: {text}"),
        })
    }

    pub(crate) fn arch_check(&mut self, line: &Line) -> Option<Result<String>> {
        let kind = line.positional.first()?.as_str();
        if !CHECKS.contains(&kind) {
            return None;
        }
        let mut line = line.clone();
        if line.positional.iter().skip(1).any(|w| w == "warn") {
            line.positional.retain(|w| w != "warn");
            line.named.push(("warn".to_string(), "1".to_string()));
        }
        let line = &line;
        Some(match kind {
            "uvalue" => self.check_uvalue(line),
            "heatloss" => self.check_heatloss(line),
            "bridges" => self.check_bridges(line),
            "shadow" => self.check_shadow(line),
            _ => self.check_more(line, kind),
        })
    }

    fn check_more(&mut self, line: &Line, kind: &str) -> Result<String> {
        let number = |model: &Model, key: &str| -> Result<Option<f64>> {
            named_value(line, key).map(|v| eval(&v, &model.scope)).transpose()
        };
        let mut problems: Vec<String> = Vec::new();
        let ok = match kind {
            "stairs" => {
                let min_width = number(self, "min_width")?.unwrap_or(900.0);
                for s in &self.arch.stairs {
                    if s.width < min_width {
                        problems.push(format!("{} width {} under {}", s.name, label(s.width), label(min_width)));
                    }
                    let sum = 2.0 * s.steps.rise + s.steps.tread;
                    if !(550.0..=700.0).contains(&sum) {
                        problems.push(format!("{} 2R+T {sum:.0} outside 550 to 700", s.name));
                    }
                }
                format!("{} stairs checked", self.arch.stairs.len())
            }
            "ramps" => {
                let max = number(self, "max")?.unwrap_or(1.0 / 12.0);
                for (name, slope) in &self.arch.ramps {
                    if *slope > max + 1.0e-9 {
                        problems.push(format!("{name} slope 1:{:.1} over 1:{:.1}", 1.0 / slope, 1.0 / max));
                    }
                }
                format!("{} ramps checked", self.arch.ramps.len())
            }
            "doors" => {
                let clear = number(self, "clear")?.unwrap_or(775.0);
                let turning = number(self, "turning")?;
                for o in &self.arch.openings {
                    if let Fill::Door { frame, .. } = &o.fill {
                        let width = o.w - 2.0 * frame.0;
                        if width < clear {
                            let mark = o.mark.clone().unwrap_or_else(|| o.name.clone());
                            problems.push(format!("{mark} clear {} under {}", label(width), label(clear)));
                        }
                    }
                }
                if let Some(circle) = turning {
                    for r in &self.arch.rooms {
                        let small = (r.rect[2] - r.rect[0]).min(r.rect[3] - r.rect[1]);
                        if small < circle {
                            problems.push(format!("{} turning circle {} under {}", r.name, label(small), label(circle)));
                        }
                    }
                }
                "doors checked".to_string()
            }
            "daylight" => {
                let min = number(self, "min")?.unwrap_or(0.1);
                for r in self.arch.rooms.clone() {
                    let glazing: f64 = self
                        .arch
                        .openings
                        .iter()
                        .filter(|o| matches!(o.fill, Fill::Window { .. }))
                        .filter(|o| self.opening_in(o, &r))
                        .map(|o| o.w * o.h)
                        .sum();
                    let floor = (r.rect[2] - r.rect[0]) * (r.rect[3] - r.rect[1]);
                    let ratio = glazing / floor;
                    if ratio < min {
                        problems.push(format!("{} {ratio:.3} under {}", r.name, label(min)));
                    }
                }
                "daylight checked".to_string()
            }
            "egress" => {
                let max = number(self, "max")?.unwrap_or(18000.0);
                for r in self.arch.rooms.clone() {
                    let centre = ((r.rect[0] + r.rect[2]) / 2.0, (r.rect[1] + r.rect[3]) / 2.0);
                    let nearest = self
                        .arch
                        .openings
                        .iter()
                        .filter(|o| matches!(o.fill, Fill::Door { .. }))
                        .filter(|o| self.opening_in(o, &r))
                        .filter_map(|o| self.opening_point(o))
                        .map(|p| (p.0 - centre.0).hypot(p.1 - centre.1))
                        .min_by(f64::total_cmp);
                    match nearest {
                        None => problems.push(format!("{} egress no door", r.name)),
                        Some(d) if d > max => problems.push(format!("{} egress {d:.0} over {}", r.name, label(max))),
                        _ => {}
                    }
                }
                "egress checked".to_string()
            }
            "rooms" => {
                let min_area = number(self, "min_area")?;
                let min_height = number(self, "min_height")?;
                for r in &self.arch.rooms {
                    if let Some(min) = min_area.filter(|m| r.area() < *m) {
                        problems.push(format!("{} area {:.2} m² under {}", r.name, r.area(), label(min)));
                    }
                    if let Some(min) = min_height.filter(|m| r.height < *m) {
                        problems.push(format!("{} height {} under {}", r.name, label(r.height), label(min)));
                    }
                }
                "rooms checked".to_string()
            }
            "compartments" => {
                let max = number(self, "max")?.unwrap_or(500.0);
                let mut levels: Vec<(f64, f64)> = Vec::new();
                for r in &self.arch.rooms {
                    match levels.iter_mut().find(|l| l.0 == r.level) {
                        Some(l) => l.1 += r.area(),
                        None => levels.push((r.level, r.area())),
                    }
                }
                for (z, area) in levels {
                    if area > max {
                        problems.push(format!("level {} compartment {area:.2} m² over {}", label(z), label(max)));
                    }
                }
                problems.extend(
                    self.checks.iter().filter(|c| !c.passed).map(|c| c.detail.clone()),
                );
                "compartments checked".to_string()
            }
            other => bail!("`check {other}` is not available in {}", line.text),
        };
        self.finish_check(line, problems, ok)
    }

    pub(crate) fn opening_point(&self, o: &Opening) -> Option<(f64, f64)> {
        let wall = self.arch.wall(&o.host)?;
        Some(wall.line.locate(o.at).1)
    }

    fn opening_in(&self, o: &Opening, r: &super::Room) -> bool {
        let Some(p) = self.opening_point(o) else { return false };
        let slack = 400.0;
        p.0 >= r.rect[0] - slack && p.0 <= r.rect[2] + slack && p.1 >= r.rect[1] - slack && p.1 <= r.rect[3] + slack
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
