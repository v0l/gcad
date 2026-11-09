use super::roof::shell_solid;
use crate::Model;
use crate::parse::{Line, eval, eval_point};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::P2;
use gcad_mesh::arch::layout::{add, left, scale, unit};
use gcad_mesh::arch::roof::{Plane, dormer_shell, lowest};

fn plan_rect(at: P2, along: P2, w: f64, h: f64) -> Vec<P2> {
    let across = left(along);
    let (hw, hh) = (w / 2.0, h / 2.0);
    [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
        .iter()
        .map(|&(a, b)| add(at, add(scale(across, a), scale(along, b))))
        .collect()
}

impl Model {
    fn host_roof(&self, line: &Line, op: &str) -> Result<(String, P2, Plane)> {
        let mut on = None;
        let mut at = None;
        for (key, text) in &line.named {
            match key.as_str() {
                "on" => on = Some(text.clone()),
                "at" => at = Some(eval_point(text, &self.scope)?),
                _ => {}
            }
        }
        let on = on.ok_or_else(|| anyhow!("a {op} needs `on=`, the roof it sits on"))?;
        let at = at.ok_or_else(|| anyhow!("a {op} needs `at=x,y`, its centre in plan"))?;
        let roof = self
            .roof(&on)
            .ok_or_else(|| anyhow!("`on={on}`: there is no roof `{on}`"))?;
        let plane = lowest(&roof.planes()?, at);
        if plane.g.0.hypot(plane.g.1) < 1.0e-9 {
            bail!(
                "a {op} sits on a sloped roof plane, and `{on}` is flat at {},{}",
                at.0,
                at.1
            );
        }
        Ok((on, at, plane))
    }

    fn number(&self, line: &Line, key: &str) -> Result<Option<f64>> {
        line.named
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, text)| eval(text, &self.scope).with_context(|| format!("`{key}={text}`")))
            .transpose()
    }

    pub(crate) fn op_skylight(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a skylight needs a label, like `sk1: skylight on=r1 at=4000,1500 w=780 h=980`")
        })?;
        self.check_free_name(&name)?;
        for (key, _) in &line.named {
            if !["on", "at", "w", "h"].contains(&key.as_str()) {
                bail!("`skylight` takes on=, at=, w= and h=, not `{key}=`");
            }
        }
        let (on, at, plane) = self.host_roof(line, "skylight")?;
        let w = self
            .number(line, "w")?
            .ok_or_else(|| anyhow!("a skylight needs `w=`"))?;
        let h = self
            .number(line, "h")?
            .ok_or_else(|| anyhow!("a skylight needs `h=`"))?;
        if !(w > 0.0 && h > 0.0) {
            bail!("a skylight's w= and h= must be above zero");
        }
        let along = unit(plane.g);
        let slope = (1.0 + plane.g.0 * plane.g.0 + plane.g.1 * plane.g.1).sqrt();
        let hole = plan_rect(at, along, w, h / slope);
        let index = self
            .arch
            .roofs
            .iter()
            .position(|r| r.name == on)
            .expect("a roof");
        self.arch.roofs[index].holes.push(hole.clone());
        if let Err(e) = self.build_roof(&on) {
            self.arch.roofs[index].holes.pop();
            return Err(e.context(format!("cutting `{name}` into `{on}`")));
        }
        let roof = self.arch.roofs[index].clone();
        let glass = 20.0;
        let top = plane.lifted(roof.thickness);
        let pane = plane.lifted(roof.thickness - glass);
        let section = |p: P2| (p, pane.at(p), top.at(p));
        let polygons: Vec<Vec<Vec<monstertruck::modeling::Point3>>> = {
            use monstertruck::modeling::Point3;
            let ring: Vec<(P2, f64, f64)> = hole.iter().map(|&p| section(p)).collect();
            let n = ring.len();
            let mut faces = vec![
                vec![
                    ring.iter()
                        .rev()
                        .map(|(p, lo, _)| Point3::new(p.0, p.1, *lo))
                        .collect(),
                ],
                vec![
                    ring.iter()
                        .map(|(p, _, hi)| Point3::new(p.0, p.1, *hi))
                        .collect(),
                ],
            ];
            for k in 0..n {
                let (a, b) = (ring[k], ring[(k + 1) % n]);
                faces.push(vec![vec![
                    Point3::new(a.0.0, a.0.1, a.1),
                    Point3::new(b.0.0, b.0.1, b.1),
                    Point3::new(b.0.0, b.0.1, b.2),
                    Point3::new(a.0.0, a.0.1, a.2),
                ]]);
            }
            faces
        };
        let solid = crate::solids::oriented(super::wall::planar_solid(&polygons)?);
        self.put_body(name.clone(), solid, "glass")?;
        self.note_built(&name, &name);
        Ok(format!(
            "skylight {name} in {on}: {} x {} on the slope, glazed flush with the roof",
            crate::parse::label(w),
            crate::parse::label(h)
        ))
    }

    pub(crate) fn op_dormer(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!(
                "a dormer needs a label, like `dm1: dormer on=r1 at=4000,1500 w=1200 type=gable`"
            )
        })?;
        self.check_free_name(&name)?;
        let mut material = None;
        for (key, text) in &line.named {
            match key.as_str() {
                "on" | "at" | "w" | "front" | "pitch" => {}
                "type" if text == "gable" => {}
                "type" => bail!("dormers are `type=gable`, not `{text}`"),
                "material" => material = Some(text.clone()),
                other => bail!(
                    "`dormer` takes on=, at=, w=, front=, pitch=, type= and material=, not `{other}=`"
                ),
            }
        }
        let (on, at, _) = self.host_roof(line, "dormer")?;
        let roof = self.roof(&on).cloned().expect("a roof");
        let w = self
            .number(line, "w")?
            .ok_or_else(|| anyhow!("a dormer needs `w=`, its width"))?;
        let pitch = self.number(line, "pitch")?.unwrap_or(roof.pitch.max(30.0));
        let outer_w = w + roof.thickness;
        let tops: Vec<Plane> = roof
            .planes()?
            .iter()
            .map(|p| p.lifted(roof.thickness))
            .collect();
        let crest = roof
            .edges()?
            .iter()
            .map(|e| e.a[2].max(e.b[2]))
            .fold(f64::MIN, f64::max);
        let base = gcad_mesh::arch::roof::envelope(&tops, at);
        let rise =
            outer_w / 2.0 * pitch.to_radians().tan() + roof.thickness / pitch.to_radians().cos();
        let room = crest - base - rise;
        let front = match self.number(line, "front")? {
            Some(front) if front > room => bail!(
                "a dormer front of {} puts its ridge above the main roof; at most {}",
                crate::parse::label(front),
                crate::parse::label(room.max(0.0))
            ),
            Some(front) => front,
            None if room <= 0.0 => {
                bail!("there is no room under the main ridge for a dormer this wide at this pitch")
            }
            None => room.min(1200.0),
        };
        if !(w > 0.0 && front > 0.0) || !(pitch > 0.0 && pitch < 90.0) {
            bail!("a dormer's w= and front= must be above zero and its pitch under 90");
        }
        let outer = outer_w;
        let shell = dormer_shell(
            (&roof.eaves(), &roof.planes()?),
            roof.thickness,
            at,
            outer,
            front,
            pitch,
        )
        .map_err(|e| anyhow!(e))
        .with_context(|| format!("building `{name}` on `{on}`"))?;
        let solid = shell_solid(&shell)?;
        let top = crate::geometry::bounds(&solid).max().z;
        let material = material.unwrap_or(roof.material.clone());
        self.put_body(name.clone(), solid, &material)?;
        self.note_built(&name, &name);
        Ok(format!(
            "dormer {name} on {on}: {} wide, front {} tall, top at {}",
            crate::parse::label(outer),
            crate::parse::label(front),
            crate::parse::label(top)
        ))
    }
}
