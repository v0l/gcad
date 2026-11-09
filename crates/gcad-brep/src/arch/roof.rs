use super::Roof;
use super::wall::planar_solid;
use crate::Model;
use crate::parse::{Line, eval};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::P2;
use gcad_mesh::arch::layout::signed_area;
use gcad_mesh::arch::roof::{Plane, ccw_ring, gable_planes, longest_edge, offset_ring, roof_shell};
use monstertruck::modeling::*;

const KINDS: &[&str] = &["gable"];

impl Roof {
    fn planes(&self) -> Vec<Plane> {
        gable_planes(&self.outline, self.base, self.pitch, self.ridge)
    }

    fn solid(&self) -> Result<Solid> {
        let eaves = offset_ring(&self.outline, self.overhang);
        let shell = roof_shell(&eaves, &self.planes(), self.thickness).map_err(|e| anyhow!(e))?;
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

    pub(crate) fn op_roof(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a roof needs a label, like `r1: roof outer type=gable pitch=45 base=L1`")
        })?;
        self.check_free_name(&name)?;
        let outline = ccw_ring(self.roof_outline(&line.positional)?);
        let mut roof = Roof {
            name: name.clone(),
            kind: String::new(),
            outline,
            overhang: 0.0,
            pitch: f64::NAN,
            base: f64::NAN,
            ridge: (0.0, 0.0),
            thickness: 200.0,
            material: "clay-tile".into(),
        };
        let mut ridge = None;
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
                "ridge" => {
                    ridge = Some(match text.as_str() {
                        "x" => (1.0, 0.0),
                        "y" => (0.0, 1.0),
                        other => bail!("ridge is x or y, not `{other}`"),
                    })
                }
                other => bail!(
                    "`roof` takes type=, pitch=, overhang=, thickness=, base=, ridge= and material=, not `{other}=`"
                ),
            }
        }
        if !KINDS.contains(&roof.kind.as_str()) {
            bail!("a roof needs `type=`, one of {}", KINDS.join(", "));
        }
        if roof.base.is_nan() {
            bail!("a roof needs `base=`, the level of its eaves like `base=L1`");
        }
        if !(roof.pitch > 0.0 && roof.pitch < 90.0) {
            bail!(
                "a {} roof needs `pitch=` in degrees above 0 and under 90",
                roof.kind
            );
        }
        if !(roof.thickness > 0.0) || roof.overhang < 0.0 {
            bail!("a roof's thickness must be above zero and its overhang not below zero");
        }
        roof.ridge = ridge.unwrap_or_else(|| longest_edge(&roof.outline));
        self.ensure_library()?;
        let solid = roof.solid().with_context(|| format!("building `{name}`"))?;
        let top = crate::geometry::bounds(&solid).max().z;
        let material = roof.material.clone();
        let kind = roof.kind.clone();
        self.arch.roofs.push(roof);
        self.put_body(name.clone(), solid, &material)?;
        self.note_built(&name, &name);
        Ok(format!(
            "{kind} roof {name}, top at {}",
            crate::parse::label(top)
        ))
    }
}
