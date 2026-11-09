use super::Roof;
use super::wall::planar_solid;
use crate::Model;
use crate::parse::{Line, eval};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::P2;
use gcad_mesh::arch::layout::signed_area;
use gcad_mesh::arch::roof::{
    Plane, ROOF_KINDS, RoofEdge, ccw_ring, extruded_profile, joined_shell, longest_edge,
    offset_ring, roof_edges, roof_planes,
};
use monstertruck::modeling::*;

const EDGE_KINDS: &[&str] = &["ridge", "hip", "valley", "eave", "verge", "curb"];

pub(crate) fn shell_solid(shell: &gcad_mesh::arch::roof::Shell) -> Result<Solid> {
    let polygons: Vec<Vec<Vec<Point3>>> = shell
        .faces
        .iter()
        .map(|f| {
            vec![
                f.iter()
                    .map(|&i| {
                        let p = shell.points[i];
                        Point3::new(p[0], p[1], p[2])
                    })
                    .collect(),
            ]
        })
        .collect();
    Ok(crate::solids::oriented(planar_solid(&polygons)?))
}

impl Roof {
    pub(crate) fn planes(&self) -> Result<Vec<Plane>> {
        roof_planes(
            &self.kind,
            &self.outline,
            self.base,
            &self.pitches,
            self.ridge,
        )
        .map_err(|e| anyhow!(e))
    }

    pub(crate) fn eaves(&self) -> Vec<P2> {
        offset_ring(&self.outline, self.overhang)
    }

    pub(crate) fn edges(&self) -> Result<Vec<RoofEdge>> {
        Ok(roof_edges(&self.eaves(), &self.planes()?, self.thickness))
    }

    pub(crate) fn edges_named(&self, rest: &[&str]) -> Result<Vec<RoofEdge>> {
        let [kind, number @ ..] = rest else {
            bail!(
                "a roof edge selector is `{}.KIND` or `{}.KIND.N`, KIND one of {}",
                self.name,
                self.name,
                EDGE_KINDS.join(", ")
            );
        };
        if !EDGE_KINDS.contains(kind) {
            bail!("roof edges are {}, not `{kind}`", EDGE_KINDS.join(", "));
        }
        let all: Vec<RoofEdge> = self
            .edges()?
            .into_iter()
            .filter(|e| e.kind == *kind)
            .collect();
        match number {
            [] if all.is_empty() => bail!("`{}` has no {kind} edges", self.name),
            [] => Ok(all),
            [n] => {
                let k: usize = n
                    .parse()
                    .ok()
                    .filter(|k| *k >= 1)
                    .ok_or_else(|| anyhow!("`{n}` is not an edge number from 1"))?;
                all.get(k - 1).cloned().map(|e| vec![e]).ok_or_else(|| {
                    anyhow!("`{}` has {} {kind} edges, not {k}", self.name, all.len())
                })
            }
            _ => bail!("write `{}.{kind}.N`", self.name),
        }
    }

    fn solid(&self) -> Result<Solid> {
        let shell = gcad_mesh::arch::roof::holed_shell(
            &self.eaves(),
            &self.planes()?,
            self.thickness,
            &self.holes,
        )
        .map_err(|e| anyhow!(e))?;
        shell_solid(&shell)
    }
}

impl Model {
    fn roof_outline(&self, words: &[String]) -> Result<Vec<P2>> {
        match words {
            [host] if self.arch.wall(host).is_some() => {
                let wall = self.arch.wall(host).expect("a wall").clone();
                if !wall.line.closed {
                    bail!("`{host}` is not a closed loop of walls, so a roof cannot sit on it");
                }
                let (lo, hi) = wall.outer_span();
                let ring = |d: f64| -> Vec<P2> {
                    wall.line
                        .offset(d, None, None)
                        .into_iter()
                        .map(|(p, _)| p)
                        .collect()
                };
                let (a, b) = (ring(lo), ring(hi));
                Ok(if signed_area(&a).abs() > signed_area(&b).abs() {
                    a
                } else {
                    b
                })
            }
            [points @ .., last] if last == "closed" => self.plan_ring(points),
            [_, _, ..] => bail!(
                "a roof outline must be closed: end the points with `closed`, or name a closed wall"
            ),
            _ => bail!(
                "write `r1: roof outer type=gable pitch=45` or `r1: roof x,y x,y x,y closed ...`"
            ),
        }
    }

    pub(crate) fn op_roof_extrude(&mut self, line: &Line, name: String) -> Result<String> {
        let [_, length] = line.positional.as_slice() else {
            bail!(
                "write `poly` with the roof's top line, then `r1: roof extrude LENGTH thickness=200`"
            );
        };
        let length = eval(length, &self.scope).with_context(|| format!("the length `{length}`"))?;
        if !(length > 0.0) {
            bail!("a roof extrusion's length must be above zero");
        }
        let mut thickness = 200.0;
        let mut material = "clay-tile".to_string();
        for (key, text) in &line.named {
            match key.as_str() {
                "thickness" => {
                    thickness =
                        eval(text, &self.scope).with_context(|| format!("`thickness={text}`"))?
                }
                "material" => material = text.clone(),
                other => bail!("`roof extrude` takes thickness= and material=, not `{other}=`"),
            }
        }
        if !(thickness > 0.0) {
            bail!("a roof's thickness must be above zero");
        }
        let (frame, profiles) = self.take_sketch("roof extrude")?;
        let [crate::geometry::Profile::Polygon { points }] = profiles.as_slice() else {
            bail!("`roof extrude` takes one `poly`, the points along the top of the roof");
        };
        let section = extruded_profile(points, thickness).map_err(|e| anyhow!(e))?;
        let ring = |d: f64| -> Vec<Point3> {
            section
                .iter()
                .map(|&(u, v)| frame.at(u, v) + frame.normal * d)
                .collect()
        };
        let solid = super::wall::loft(&[ring(0.0)], &[ring(length)])?;
        self.ensure_library()?;
        let top = crate::geometry::bounds(&solid).max().z;
        self.put_body(name.clone(), solid, &material)?;
        self.note_built(&name, &name);
        Ok(format!(
            "roof {name} extruded {}, top at {}",
            crate::parse::label(length),
            crate::parse::label(top)
        ))
    }

    pub(crate) fn op_roof(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a roof needs a label, like `r1: roof outer type=gable pitch=45 base=L1`")
        })?;
        self.check_free_name(&name)?;
        if line.positional.first().is_some_and(|w| w == "extrude") {
            return self.op_roof_extrude(line, name);
        }
        let outline = ccw_ring(self.roof_outline(&line.positional)?);
        let mut roof = Roof {
            name: name.clone(),
            kind: String::new(),
            outline,
            overhang: 0.0,
            pitch: f64::NAN,
            pitches: Vec::new(),
            base: f64::NAN,
            ridge: (0.0, 0.0),
            thickness: 200.0,
            material: "clay-tile".into(),
            joins: None,
            holes: Vec::new(),
        };
        let mut ridge = None;
        let mut trims: Vec<String> = Vec::new();
        for (key, text) in &line.named {
            let number =
                |model: &Model| eval(text, &model.scope).with_context(|| format!("`{key}={text}`"));
            match key.as_str() {
                "type" => roof.kind = text.clone(),
                "pitch" => roof.pitch = number(self)?,
                "overhang" => roof.overhang = number(self)?,
                "thickness" => roof.thickness = number(self)?,
                "base" => roof.base = number(self)?,
                "material" => roof.material = text.clone(),
                "walls" => trims = text.split(',').map(str::to_string).collect(),
                "ridge" => {
                    ridge = Some(match text.as_str() {
                        "x" => (1.0, 0.0),
                        "y" => (0.0, 1.0),
                        other => bail!("ridge is x or y, not `{other}`"),
                    })
                }
                other => bail!(
                    "`roof` takes type=, pitch=, overhang=, thickness=, base=, ridge=, walls= and material=, not `{other}=`"
                ),
            }
        }
        if !ROOF_KINDS.contains(&roof.kind.as_str()) {
            bail!("a roof needs `type=`, one of {}", ROOF_KINDS.join(", "));
        }
        if roof.base.is_nan() {
            bail!("a roof needs `base=`, the level of its eaves like `base=L1`");
        }
        if roof.kind == "flat" && roof.pitch.is_nan() {
            roof.pitch = 0.0;
        } else if !(roof.pitch > 0.0 && roof.pitch < 90.0) {
            bail!(
                "a {} roof needs `pitch=` in degrees above 0 and under 90",
                roof.kind
            );
        }
        if !(roof.thickness > 0.0) || roof.overhang < 0.0 {
            bail!("a roof's thickness must be above zero and its overhang not below zero");
        }
        roof.ridge = ridge.unwrap_or_else(|| longest_edge(&roof.outline));
        roof.pitches = vec![roof.pitch; roof.outline.len()];
        self.ensure_library()?;
        let kind = roof.kind.clone();
        self.arch.roofs.push(roof);
        let top = self.build_roof(&name)?;
        let mut trimmed = String::new();
        if !trims.is_empty() {
            let planes = self.roof(&name).expect("a roof").planes()?;
            for wall in &trims {
                let slot = self
                    .arch
                    .walls
                    .iter_mut()
                    .find(|w| w.name == *wall)
                    .ok_or_else(|| anyhow!("`walls={wall}`: there is no wall `{wall}`"))?;
                slot.under = planes.clone();
            }
            self.rebuild_walls()
                .with_context(|| format!("trimming {} to `{name}`", trims.join(", ")))?;
            trimmed = format!(", {} built up to its underside", trims.join(", "));
        }
        Ok(format!(
            "{kind} roof {name}, top at {}{trimmed}",
            crate::parse::label(top)
        ))
    }

    pub(crate) fn roof(&self, name: &str) -> Option<&Roof> {
        self.arch.roofs.iter().find(|r| r.name == name)
    }

    pub(crate) fn build_roof(&mut self, name: &str) -> Result<f64> {
        let roof = self.roof(name).cloned().expect("a roof");
        let solid = match &roof.joins {
            None => roof.solid(),
            Some(host) => {
                let host = self
                    .roof(host)
                    .cloned()
                    .ok_or_else(|| anyhow!("no roof `{host}` to join"))?;
                let shell = joined_shell(
                    (&roof.eaves(), &roof.planes()?, roof.ridge),
                    (&host.eaves(), &host.planes()?),
                    roof.thickness,
                )
                .map_err(|e| anyhow!(e))?;
                shell_solid(&shell)
            }
        }
        .with_context(|| format!("building `{name}`"))?;
        let top = crate::geometry::bounds(&solid).max().z;
        self.put_body(name.to_string(), solid, &roof.material)?;
        self.note_built(name, name);
        Ok(top)
    }

    pub(crate) fn roof_edge_selection(&self, selector: &str) -> Option<Result<Vec<RoofEdge>>> {
        let mut parts = selector.split('.');
        let roof = self.roof(parts.next()?)?;
        let rest: Vec<&str> = parts.collect();
        if rest.is_empty() {
            return None;
        }
        Some(roof.edges_named(&rest))
    }

    pub(crate) fn op_roof_join(&mut self, line: &Line) -> Result<String> {
        let [own, host] = line.positional.as_slice() else {
            bail!("write `roof_join r2 r1`: r2 runs on until it meets r1");
        };
        if own == host {
            bail!("a roof cannot join itself");
        }
        if self.roof(host).is_none() {
            bail!("no roof `{host}`");
        }
        let index = self
            .arch
            .roofs
            .iter()
            .position(|r| r.name == *own)
            .ok_or_else(|| anyhow!("no roof `{own}`"))?;
        self.arch.roofs[index].joins = Some(host.clone());
        let top = self.build_roof(own)?;
        Ok(format!(
            "{own} runs on into {host}, top at {}",
            crate::parse::label(top)
        ))
    }

    pub(crate) fn op_pitch_edge(&mut self, line: &Line) -> Result<String> {
        let [selector, angle] = line.positional.as_slice() else {
            bail!("write `pitch_edge r1.eave.1 60`");
        };
        let pitch = eval(angle, &self.scope).with_context(|| format!("the pitch `{angle}`"))?;
        if !(pitch > 0.0 && pitch < 90.0) {
            bail!("a pitch is in degrees above 0 and under 90");
        }
        let edges = self
            .roof_edge_selection(selector)
            .ok_or_else(|| anyhow!("`{selector}` is not a roof edge like `r1.eave.1`"))??;
        let name = selector.split('.').next().expect("a name").to_string();
        let index = self
            .arch
            .roofs
            .iter()
            .position(|r| r.name == name)
            .expect("a roof");
        for edge in &edges {
            let Some(e) = edge.outline_edge else {
                bail!("`pitch_edge` sets the pitch of eaves and verges, not ridges or hips");
            };
            self.arch.roofs[index].pitches[e] = pitch;
        }
        let top = self.build_roof(&name)?;
        Ok(format!(
            "{selector} at {} degrees, {name} top at {}",
            crate::parse::label(pitch),
            crate::parse::label(top)
        ))
    }
}
