use super::Flight;
use super::slab::{inside, slab_layers};
use crate::Model;
use crate::parse::{Line, eval};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::P2;
use gcad_mesh::arch::layout::{add, left, scale, sub, unit};
use gcad_mesh::arch::stair::{blade_stations, even_split};

const BALCONY_POST: (f64, f64) = (50.0, 1500.0);
const BALCONY_BALUSTER: (f64, f64) = (12.0, 120.0);
const DOOR_LEAF: f64 = 50.0;

fn axis(a: P2, b: P2, z0: f64) -> Result<(Flight, f64)> {
    let d = sub(b, a);
    let length = d.0.hypot(d.1);
    if length < 1.0e-6 {
        bail!("a facade run needs two different points");
    }
    Ok((
        Flight {
            start: a,
            dir: unit(d),
            z0,
            treads: 0,
        },
        length,
    ))
}

fn rect(s0: f64, s1: f64, z0: f64, z1: f64) -> Vec<P2> {
    vec![(s0, z0), (s1, z0), (s1, z1), (s0, z1)]
}

fn pair(text: &str, scope: &crate::parse::Scope, key: &str) -> Result<(f64, f64)> {
    let (a, b) = text
        .split_once('x')
        .ok_or_else(|| anyhow!("`{key}=` is two sizes like 1500x900"))?;
    let (a, b) = (eval(a, scope)?, eval(b, scope)?);
    if a <= 0.0 || b <= 0.0 {
        bail!("`{key}=` sizes must be above zero");
    }
    Ok((a, b))
}

fn glass_of(text: &str, scope: &crate::parse::Scope) -> Result<f64> {
    Ok(match text {
        "single" => 6.0,
        "double" => 24.0,
        "triple" => 44.0,
        other => eval(other, scope).with_context(|| {
            format!("`glass=` is single, double, triple or a thickness, not `{other}`")
        })?,
    })
}

impl Model {
    fn put_part(&mut self, owner: &str, part: &str, solid: monstertruck::modeling::Solid, material: &str) -> Result<String> {
        let used = self.body_names();
        let body = (1..)
            .map(|k| format!("{owner}.{part}.{k}"))
            .find(|b| !used.contains(b))
            .expect("a free name");
        self.put_body(body.clone(), solid, material)?;
        self.note_built(owner, &body);
        Ok(body)
    }

    fn plan_points(&self, words: &[String]) -> Result<Vec<P2>> {
        let points = words
            .iter()
            .map(|w| self.plan_point(w))
            .collect::<Result<Vec<_>>>()?;
        if points.len() < 2 {
            bail!("a facade run needs two or more points");
        }
        Ok(points)
    }

    pub(crate) fn op_curtainwall(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a curtain wall needs a label, like `cw1: curtainwall 0,0 6000,0 height=L1`")
        })?;
        self.check_free_name(&name)?;
        let points = self.plan_points(&line.positional)?;
        let (mut base, mut height) = (0.0, None);
        let (mut grid, mut mullion) = ((1500.0, 900.0), (50.0, 100.0));
        let (mut glass, mut door) = (24.0, None);
        let mut material = "aluminium".to_string();
        for (key, text) in &line.named {
            let value = || eval(text, &self.scope).with_context(|| format!("`{key}={text}`"));
            match key.as_str() {
                "base" => base = value()?,
                "height" => height = Some(value()?),
                "grid" => grid = pair(text, &self.scope, "grid")?,
                "mullion" => mullion = pair(text, &self.scope, "mullion")?,
                "glass" => glass = glass_of(text, &self.scope)?,
                "door" => {
                    let cell = text
                        .strip_prefix("cell:")
                        .and_then(|c| c.split_once(','))
                        .and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)))
                        .filter(|(a, b)| *a >= 1 && *b >= 1)
                        .ok_or_else(|| anyhow!("`door=` is a cell like `cell:2,1`, bay then row from 1"))?;
                    door = Some(cell);
                }
                "material" => material = text.clone(),
                other => bail!(
                    "`curtainwall` takes base=, height=, grid=, mullion=, glass=, door= and material=, not `{other}=`"
                ),
            }
        }
        let top = height.ok_or_else(|| anyhow!("a curtain wall needs `height=`, its top"))?;
        if top <= base {
            bail!("the curtain wall's top is not above its base");
        }
        if glass <= 0.0 || glass >= mullion.1 {
            bail!("the glass must be thinner than the mullion depth {}", mullion.1);
        }
        let (mw, md) = mullion;
        let rows = even_split(top - base, grid.1);
        let row_h = (top - base) / rows as f64;
        self.ensure_library()?;
        let (mut bay_index, mut panes, mut doors) = (0, 0, 0);
        for w in points.windows(2) {
            let (axis, length) = axis(w[0], w[1], base)?;
            let bays = even_split(length, grid.0);
            let bay_w = length / bays as f64;
            for k in 0..=bays {
                let s = (k as f64 * bay_w).clamp(mw / 2.0, length - mw / 2.0);
                let solid = axis.slab(&rect(s - mw / 2.0, s + mw / 2.0, 0.0, top - base), -md / 2.0, md / 2.0)?;
                self.put_part(&name, "mullion", solid, &material)?;
            }
            for b in 0..bays {
                bay_index += 1;
                let (s0, s1) = (b as f64 * bay_w + mw / 2.0, (b + 1) as f64 * bay_w - mw / 2.0);
                let (s0, s1) = (s0.max(mw), s1.min(length - mw));
                for j in 0..=rows {
                    let z = (j as f64 * row_h).clamp(mw / 2.0, top - base - mw / 2.0);
                    let solid = axis.slab(&rect(s0, s1, z - mw / 2.0, z + mw / 2.0), -md / 2.0, md / 2.0)?;
                    self.put_part(&name, "transom", solid, &material)?;
                }
                for j in 0..rows {
                    let z0 = (j as f64 * row_h).max(0.0) + mw / 2.0 + if j == 0 { mw / 2.0 } else { 0.0 };
                    let z1 = ((j + 1) as f64 * row_h) - mw / 2.0 - if j + 1 == rows { mw / 2.0 } else { 0.0 };
                    let profile = rect(s0, s1, z0, z1);
                    if door == Some((bay_index, j + 1)) {
                        doors += 1;
                        let solid = axis.slab(&profile, -DOOR_LEAF / 2.0, DOOR_LEAF / 2.0)?;
                        self.put_part(&name, "door", solid, &material)?;
                    } else {
                        panes += 1;
                        let solid = axis.slab(&profile, -glass / 2.0, glass / 2.0)?;
                        self.put_part(&name, "glass", solid, "glass")?;
                    }
                }
            }
        }
        if door.is_some() && doors == 0 {
            bail!("the door cell is outside the {bay_index} bays and {rows} rows");
        }
        Ok(format!(
            "curtain wall {name}: {bay_index} bays by {rows} rows, {panes} panes, {doors} door{}",
            if doors == 1 { "" } else { "s" }
        ))
    }

    pub(crate) fn op_louvre(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a louvre needs a label, like `lv: louvre 0,0 2000,0 z=900..2100 blade=150x20@100`")
        })?;
        self.check_free_name(&name)?;
        let [a, b] = line.positional.as_slice() else {
            bail!("write `lv: louvre x,y x,y z=900..2100 blade=150x20@100 angle=45`");
        };
        let (a, b) = (self.plan_point(a)?, self.plan_point(b)?);
        let (mut span, mut blade, mut angle) = (None, None, 45.0);
        let mut material = "aluminium".to_string();
        for (key, text) in &line.named {
            match key.as_str() {
                "z" => {
                    let (lo, hi) = text
                        .split_once("..")
                        .ok_or_else(|| anyhow!("`z=` is a range like 900..2100"))?;
                    span = Some((eval(lo, &self.scope)?, eval(hi, &self.scope)?));
                }
                "blade" => {
                    let (size, spacing) = text
                        .split_once('@')
                        .ok_or_else(|| anyhow!("`blade=` is DxT@spacing like 150x20@100"))?;
                    let (d, t) = pair(size, &self.scope, "blade")?;
                    let spacing = eval(spacing, &self.scope)?;
                    if spacing <= 0.0 {
                        bail!("the blade spacing must be above zero");
                    }
                    blade = Some((d, t, spacing));
                }
                "angle" => angle = eval(text, &self.scope)?,
                "material" => material = text.clone(),
                other => bail!("`louvre` takes z=, blade=, angle= and material=, not `{other}=`"),
            }
        }
        let (lo, hi) = span.ok_or_else(|| anyhow!("a louvre needs `z=lo..hi`"))?;
        if hi <= lo {
            bail!("the louvre's z range is empty");
        }
        let (d, t, spacing) = blade.ok_or_else(|| anyhow!("a louvre needs `blade=DxT@spacing`"))?;
        let (line_axis, length) = axis(a, b, 0.0)?;
        let across = Flight {
            start: a,
            dir: left(line_axis.dir),
            z0: 0.0,
            treads: 0,
        };
        let (sin, cos) = angle.to_radians().sin_cos();
        self.ensure_library()?;
        let stations = blade_stations(lo, hi, spacing);
        for (k, z) in stations.iter().enumerate() {
            let profile: Vec<P2> = [(-d / 2.0, -t / 2.0), (d / 2.0, -t / 2.0), (d / 2.0, t / 2.0), (-d / 2.0, t / 2.0)]
                .map(|(x, y)| (x * cos - y * sin, z + x * sin + y * cos))
                .to_vec();
            let solid = across.slab(&profile, 0.0, length)?;
            let body = format!("{name}.{}", k + 1);
            self.put_body(body.clone(), solid, &material)?;
            self.note_built(&name, &body);
        }
        Ok(format!(
            "louvre {name}: {} blades {}x{} at {}, {}°",
            stations.len(),
            crate::parse::label(d),
            crate::parse::label(t),
            crate::parse::label(spacing),
            crate::parse::label(angle)
        ))
    }

    pub(crate) fn op_shading_fin(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("shading fins need a label, like `sf: shading_fin 0,0 4000,0 depth=400 t=20@500 height=L1`")
        })?;
        self.check_free_name(&name)?;
        let [a, b] = line.positional.as_slice() else {
            bail!("write `sf: shading_fin x,y x,y depth=400 t=20@500 height=L1`");
        };
        let (a, b) = (self.plan_point(a)?, self.plan_point(b)?);
        let (mut depth, mut fin, mut base, mut height) = (400.0, None, 0.0, None);
        let mut material = "aluminium".to_string();
        for (key, text) in &line.named {
            let value = || eval(text, &self.scope).with_context(|| format!("`{key}={text}`"));
            match key.as_str() {
                "depth" => depth = value()?,
                "t" => {
                    let (t, spacing) = text
                        .split_once('@')
                        .ok_or_else(|| anyhow!("`t=` is a thickness and spacing like 20@500"))?;
                    fin = Some((eval(t, &self.scope)?, eval(spacing, &self.scope)?));
                }
                "base" => base = value()?,
                "height" => height = Some(value()?),
                "material" => material = text.clone(),
                other => bail!(
                    "`shading_fin` takes depth=, t=, base=, height= and material=, not `{other}=`"
                ),
            }
        }
        let (t, spacing) = fin.ok_or_else(|| anyhow!("shading fins need `t=T@spacing`"))?;
        let top = height.ok_or_else(|| anyhow!("shading fins need `height=`, their top"))?;
        if t <= 0.0 || spacing <= 0.0 || depth <= 0.0 || top <= base {
            bail!("shading fins need a thickness, spacing, depth and height above zero");
        }
        let (axis, length) = axis(a, b, base)?;
        self.ensure_library()?;
        let stations = blade_stations(0.0, length, spacing);
        for (k, s) in stations.iter().enumerate() {
            let s = s.clamp(t / 2.0, length - t / 2.0);
            let solid = axis.slab(&rect(s - t / 2.0, s + t / 2.0, 0.0, top - base), 0.0, depth)?;
            let body = format!("{name}.{}", k + 1);
            self.put_body(body.clone(), solid, &material)?;
            self.note_built(&name, &body);
        }
        Ok(format!(
            "shading fins {name}: {} fins {} thick, {} deep",
            stations.len(),
            crate::parse::label(t),
            crate::parse::label(depth)
        ))
    }

    pub(crate) fn op_balcony(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a balcony needs a label, like `bal: balcony on=outer at=4000 level=L1 w=3000 d=1200`")
        })?;
        self.check_free_name(&name)?;
        if !line.positional.is_empty() {
            bail!("`balcony` takes only named values, like `on=outer at=4000 level=L1`");
        }
        let (mut host, mut at, mut level, mut w, mut d) = (None, None, None, None, None);
        let (mut slab, mut railing) = (None, Some(1100.0));
        for (key, text) in &line.named {
            let value = || eval(text, &self.scope).with_context(|| format!("`{key}={text}`"));
            match key.as_str() {
                "on" => host = Some(text.clone()),
                "at" => at = Some(value()?),
                "level" => level = Some(value()?),
                "w" => w = Some(value()?),
                "d" => d = Some(value()?),
                "slab" => slab = Some(text.clone()),
                "railing" if text == "none" => railing = None,
                "railing" => railing = Some(value()?),
                other => bail!(
                    "`balcony` takes on=, at=, level=, w=, d=, slab= and railing=, not `{other}=`"
                ),
            }
        }
        let need = |v: Option<f64>, what: &str| v.ok_or_else(|| anyhow!("a balcony needs `{what}=`"));
        let (at, level, w, d) = (need(at, "at")?, need(level, "level")?, need(w, "w")?, need(d, "d")?);
        if w <= 0.0 || d <= 0.0 {
            bail!("a balcony's w= and d= must be above zero");
        }
        let host = host.ok_or_else(|| anyhow!("a balcony needs `on=`, the wall it hangs off"))?;
        let wall = self
            .arch
            .wall(&host)
            .cloned()
            .ok_or_else(|| anyhow!("no wall `{host}` for the balcony"))?;
        if at - w / 2.0 < -1.0e-6 || at + w / 2.0 > wall.line.length() + 1.0e-6 {
            bail!("the balcony runs off the end of `{host}`");
        }
        let (i, p, dir) = wall.line.locate(at);
        let starts = wall.line.starts();
        if at - w / 2.0 < starts[i] - 1.0e-6 || at + w / 2.0 > starts[i + 1] + 1.0e-6 {
            bail!("the balcony crosses a corner of `{host}`; it sits on one straight run");
        }
        let (lo, hi) = wall.outer_span();
        let side = if wall.line.closed && inside(add(p, scale(left(dir), 1.0)), &wall.line.points) {
            -1.0
        } else {
            1.0
        };
        let face = if side > 0.0 { hi } else { lo };
        let out = scale(left(dir), side);
        let corner = |s: f64, k: f64| add(add(p, scale(dir, s - at)), scale(out, side * face + k));
        let ring = vec![
            corner(at - w / 2.0, 0.0),
            corner(at + w / 2.0, 0.0),
            corner(at + w / 2.0, d),
            corner(at - w / 2.0, d),
        ];
        let layers = slab_layers(self, slab.as_deref().unwrap_or("concrete:200"))?;
        let total: f64 = layers.iter().map(|l| l.thickness).sum();
        let mut z = level - total;
        for layer in &layers {
            let solid = super::slab::prism(&ring, &[], z, z + layer.thickness)
                .with_context(|| format!("building `{name}`"))?;
            z += layer.thickness;
            if layer.material != "air" {
                let body = format!("{name}.slab.{}", layer.material);
                let body = if self.body_names().contains(&body) { format!("{body}.2") } else { body };
                self.put_body(body.clone(), solid, &layer.material)?;
                self.note_built(&name, &body);
            }
        }
        if let Some(height) = railing {
            let inset = BALCONY_POST.0 / 2.0;
            let edge = |s: f64, k: f64| {
                let q = corner(s, k);
                [q.0, q.1, level]
            };
            let path = vec![
                edge(at - w / 2.0 + inset, 0.0),
                edge(at - w / 2.0 + inset, d - inset),
                edge(at + w / 2.0 - inset, d - inset),
                edge(at + w / 2.0 - inset, 0.0),
            ];
            let r = super::railing::Railing {
                height,
                post: Some(BALCONY_POST),
                baluster: Some(BALCONY_BALUSTER),
                ..Default::default()
            };
            let owner = format!("{name}.railing");
            self.build_railing(&owner, &path, &r)
                .with_context(|| format!("building `{name}`"))?;
        }
        Ok(format!(
            "balcony {name} on {host}: {} x {}, {} thick, top at {}{}",
            crate::parse::label(w),
            crate::parse::label(d),
            crate::parse::label(total),
            crate::parse::label(level),
            railing.map_or(String::new(), |h| format!(", railing {}", crate::parse::label(h)))
        ))
    }
}
