use super::roof::shell_solid;
use crate::Model;
use crate::parse::{Line, eval, eval_point};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::P2;
use gcad_mesh::arch::layout::{add, left, scale, sub, unit};
use gcad_mesh::arch::roof::{Plane, dormer_shell, envelope, lowest};

fn plan_rect(at: P2, along: P2, w: f64, h: f64) -> Vec<P2> {
    let across = left(along);
    let (hw, hh) = (w / 2.0, h / 2.0);
    [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
        .iter()
        .map(|&(a, b)| add(at, add(scale(across, a), scale(along, b))))
        .collect()
}

fn highest_over(ring: &[P2], planes: &[Plane]) -> f64 {
    let n = ring.len();
    let mut points = ring.to_vec();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        for p in planes {
            for q in planes {
                let (fa, fb) = (p.at(a) - q.at(a), p.at(b) - q.at(b));
                if fa * fb < 0.0 {
                    points.push(add(a, scale(sub(b, a), fa / (fa - fb))));
                }
            }
        }
    }
    points
        .iter()
        .map(|&p| envelope(planes, p))
        .fold(f64::MIN, f64::max)
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

    pub(crate) fn op_chimney(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a chimney needs a label, like `ch: chimney 6000,3000 w=600 d=600 up_to=r1 over=900`")
        })?;
        self.check_free_name(&name)?;
        let [at] = line.positional.as_slice() else {
            bail!("write `ch: chimney x,y w=600 d=600 up_to=r1 over=900`");
        };
        let at = self.plan_point(at)?;
        let mut up_to = None;
        let mut material = "brick".to_string();
        for (key, text) in &line.named {
            match key.as_str() {
                "w" | "d" | "over" | "base" => {}
                "up_to" => up_to = Some(text.clone()),
                "material" => material = text.clone(),
                other => bail!(
                    "`chimney` takes w=, d=, up_to=, over=, base= and material=, not `{other}=`"
                ),
            }
        }
        let w = self
            .number(line, "w")?
            .ok_or_else(|| anyhow!("a chimney needs `w=`"))?;
        let d = self.number(line, "d")?.unwrap_or(w);
        let over = self.number(line, "over")?.unwrap_or(900.0);
        let base = self.number(line, "base")?.unwrap_or(0.0);
        if !(w > 0.0 && d > 0.0) || over < 0.0 {
            bail!("a chimney's w= and d= must be above zero and over= not below zero");
        }
        let stack = plan_rect(at, (0.0, 1.0), w, d);
        let (top, note) = match &up_to {
            Some(on) => {
                let roof = self
                    .roof(on)
                    .cloned()
                    .ok_or_else(|| anyhow!("`up_to={on}`: there is no roof `{on}`"))?;
                let tops: Vec<Plane> = roof
                    .planes()?
                    .iter()
                    .map(|p| p.lifted(roof.thickness))
                    .collect();
                let highest = highest_over(&stack, &tops);
                let index = self
                    .arch
                    .roofs
                    .iter()
                    .position(|r| r.name == *on)
                    .expect("a roof");
                self.arch.roofs[index].holes.push(stack.clone());
                if let Err(e) = self.build_roof(on) {
                    self.arch.roofs[index].holes.pop();
                    return Err(e.context(format!("cutting `{name}` through `{on}`")));
                }
                (highest + over, format!(" through {on}"))
            }
            None => (base + over, String::new()),
        };
        if top <= base {
            bail!("the chimney's top {} is not above its base {}", top, base);
        }
        let solid = super::slab::prism(&stack, &[], base, top)?;
        self.ensure_library()?;
        self.put_body(name.clone(), solid, &material)?;
        self.note_built(&name, &name);
        Ok(format!(
            "chimney {name}{note}, {} x {}, top at {}",
            crate::parse::label(w),
            crate::parse::label(d),
            crate::parse::label(top)
        ))
    }

    pub(crate) fn op_gutter(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a gutter needs a label, like `gu: gutter along=r1.eave.1 w=100 slope=0.005`")
        })?;
        self.check_free_name(&name)?;
        let mut along = None;
        let mut material = "pvc".to_string();
        for (key, text) in &line.named {
            match key.as_str() {
                "along" => along = Some(text.clone()),
                "w" | "slope" => {}
                "material" => material = text.clone(),
                other => bail!("`gutter` takes along=, w=, slope= and material=, not `{other}=`"),
            }
        }
        let along =
            along.ok_or_else(|| anyhow!("a gutter needs `along=`, an eave like `r1.eave.1`"))?;
        let edges = self
            .roof_edge_selection(&along)
            .ok_or_else(|| anyhow!("`along={along}` is not a roof edge like `r1.eave.1`"))??;
        let [edge] = edges.as_slice() else {
            bail!("`along=` names one eave, like `r1.eave.1`");
        };
        if edge.kind != "eave" {
            bail!(
                "a gutter runs along an eave, and `{along}` is a {}",
                edge.kind
            );
        }
        let roof = self
            .roof(along.split('.').next().expect("a name"))
            .cloned()
            .expect("a roof");
        let w = self.number(line, "w")?.unwrap_or(100.0);
        let slope = self.number(line, "slope")?.unwrap_or(0.0);
        if !(w > 0.0) || !(0.0..0.1).contains(&slope) {
            bail!("a gutter's w= must be above zero and its slope= between 0 and 0.1");
        }
        let e = edge.outline_edge.expect("an outline edge");
        let eaves = roof.eaves();
        let n = eaves.len();
        let (a, b) = (eaves[e], eaves[(e + 1) % n]);
        let dir = unit(sub(b, a));
        let out = scale(left(dir), -1.0);
        let z = roof
            .planes()?
            .iter()
            .map(|p| p.at(a))
            .fold(f64::INFINITY, f64::min);
        let run = gcad_mesh::arch::layout::length(sub(b, a));
        let fall = run * slope;
        let at = |s: f64, d: f64, dz: f64| {
            let p = add(add(a, scale(dir, s)), scale(out, d));
            monstertruck::modeling::Point3::new(p.0, p.1, z + dz - fall * s / run)
        };
        let depth = w * 0.75;
        let section = [(0.0, 0.0), (w, 0.0), (w, -depth), (0.0, -depth)];
        let ring = |s: f64| -> Vec<monstertruck::modeling::Point3> {
            section.iter().map(|&(d, dz)| at(s, d, dz)).collect()
        };
        let solid = super::wall::loft(&[ring(0.0)], &[ring(run)])?;
        self.ensure_library()?;
        self.put_body(name.clone(), solid, &material)?;
        self.note_built(&name, &name);
        let end = add(b, scale(out, w / 2.0));
        self.scope.insert(format!("{name}.end.x"), end.0);
        self.scope.insert(format!("{name}.end.y"), end.1);
        self.scope.insert(format!("{name}.end.z"), z - depth - fall);
        Ok(format!(
            "gutter {name} along {along}: {} long, falls {} to its end",
            crate::parse::label(run),
            crate::parse::label(fall)
        ))
    }

    pub(crate) fn op_downpipe(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a downpipe needs a label, like `dp: downpipe from=gu.end d=68 to=L0`")
        })?;
        self.check_free_name(&name)?;
        let mut from = None;
        let mut material = "pvc".to_string();
        for (key, text) in &line.named {
            match key.as_str() {
                "from" => from = Some(text.clone()),
                "d" | "to" => {}
                "material" => material = text.clone(),
                other => bail!("`downpipe` takes from=, d=, to= and material=, not `{other}=`"),
            }
        }
        let from = from.ok_or_else(|| anyhow!("a downpipe needs `from=`, like `from=gu.end`"))?;
        let get = |axis: &str| self.scope.get(&format!("{from}.{axis}")).copied();
        let (Some(x), Some(y), Some(top)) = (get("x"), get("y"), get("z")) else {
            bail!("`from={from}` is not a gutter end like `gu.end`");
        };
        let d = self.number(line, "d")?.unwrap_or(68.0);
        let bottom = self.number(line, "to")?.unwrap_or(0.0);
        if !(d > 0.0) || bottom >= top {
            bail!("a downpipe needs d= above zero and to= below the gutter");
        }
        let ring: Vec<P2> = (0..32)
            .map(|k| {
                let t = std::f64::consts::TAU * k as f64 / 32.0;
                (x + d / 2.0 * t.cos(), y + d / 2.0 * t.sin())
            })
            .collect();
        let solid = super::slab::prism(&ring, &[], bottom, top)?;
        self.ensure_library()?;
        self.put_body(name.clone(), solid, &material)?;
        self.note_built(&name, &name);
        Ok(format!(
            "downpipe {name} from {from}: {} long",
            crate::parse::label(top - bottom)
        ))
    }

    pub(crate) fn op_parapet(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a parapet needs a label, like `pa: parapet outer h=600 coping=50`")
        })?;
        self.check_free_name(&name)?;
        let [host] = line.positional.as_slice() else {
            bail!("write `pa: parapet WALL h=600 coping=50`");
        };
        let mut material = None;
        let mut coping_material = None;
        for (key, text) in &line.named {
            match key.as_str() {
                "h" | "coping" | "overhang" => {}
                "material" => material = Some(text.clone()),
                "coping_material" => coping_material = Some(text.clone()),
                other => bail!(
                    "`parapet` takes h=, coping=, overhang=, material= and coping_material=, not `{other}=`"
                ),
            }
        }
        let wall =
            self.arch.wall(host).cloned().ok_or_else(|| {
                anyhow!("a parapet stands on a wall, and there is no wall `{host}`")
            })?;
        let h = self
            .number(line, "h")?
            .ok_or_else(|| anyhow!("a parapet needs `h=`, its height"))?;
        let coping = self.number(line, "coping")?.unwrap_or(0.0);
        let overhang = self.number(line, "overhang")?.unwrap_or(0.0);
        if !(h > 0.0) || coping < 0.0 || overhang < 0.0 {
            bail!("a parapet's h= must be above zero and coping= and overhang= not below zero");
        }
        let top = wall.top;
        let (lo, hi) = wall.outer_span();
        let material = material.unwrap_or_else(|| {
            let tier = wall.tiers.last().expect("a tier");
            tier.layers
                .iter()
                .find(|l| l.material != "air")
                .map(|l| l.material.clone())
                .unwrap_or_else(|| "brick".into())
        });
        let band = |lo: f64, hi: f64| -> (Vec<P2>, Vec<Vec<P2>>) {
            let (outer, hole) = wall.line.band(lo, hi, None, None, &[]);
            let flat = |r: Vec<(P2, f64)>| r.into_iter().map(|(p, _)| p).collect::<Vec<P2>>();
            (flat(outer), hole.map(flat).into_iter().collect())
        };
        let (outer, holes) = band(lo, hi);
        self.ensure_library()?;
        let inner = self.inner_ring(&wall).ok();
        let over = |r: &super::Roof| {
            let n = r.outline.len() as f64;
            let c = r
                .outline
                .iter()
                .fold((0.0, 0.0), |s, p| (s.0 + p.0 / n, s.1 + p.1 / n));
            inner
                .as_ref()
                .is_some_and(|ring| gcad_mesh::arch::roof::inside(c, ring))
        };
        let flats: Vec<String> = self
            .arch
            .roofs
            .iter()
            .filter(|r| r.kind == "flat" && r.joins.is_none() && (r.base - top).abs() < 1.0e-6)
            .filter(|r| over(r))
            .map(|r| r.name.clone())
            .collect();
        if let Some(inner) = inner {
            for roof in &flats {
                let index = self
                    .arch
                    .roofs
                    .iter()
                    .position(|r| r.name == *roof)
                    .expect("a roof");
                let previous = self.arch.roofs[index].clone();
                let slot = &mut self.arch.roofs[index];
                slot.outline = gcad_mesh::arch::roof::ccw_ring(inner.clone());
                slot.overhang = 0.0;
                slot.pitches = vec![slot.pitch; slot.outline.len()];
                if let Err(e) = self.build_roof(roof) {
                    self.arch.roofs[index] = previous;
                    return Err(e.context(format!("fitting `{roof}` inside `{name}`")));
                }
            }
        }
        let upstand = if overhang > 0.0 { h } else { h + coping };
        let body = super::slab::prism(&outer, &holes, top, top + upstand)?;
        self.put_body(name.clone(), body, &material)?;
        self.note_built(&name, &name);
        if coping > 0.0 && overhang > 0.0 {
            let (outer, holes) = band(lo - overhang, hi + overhang);
            let cap = super::slab::prism(&outer, &holes, top + h, top + h + coping)?;
            let cap_name = format!("{name}.coping");
            self.put_body(
                cap_name.clone(),
                cap,
                coping_material.as_deref().unwrap_or("limestone"),
            )?;
            self.note_built(&name, &cap_name);
        }
        Ok(format!(
            "parapet {name} on {host}: {} high, top at {}",
            crate::parse::label(h + coping),
            crate::parse::label(top + h + coping)
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
