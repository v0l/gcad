use super::Flight;
use super::wall::piece_of;
use crate::Model;
use crate::member::straight_member;
use crate::parse::{Line, eval};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::stair::{P3, infill_stations, path_length, point_along, post_stations};
use monstertruck::modeling::*;

const BALUSTER_GAP: f64 = 100.0;
const BRACKET: (f64, f64) = (16.0, 60.0);

#[derive(Clone, Debug)]
pub(crate) struct Railing {
    pub height: f64,
    pub post: Option<(f64, f64)>,
    pub baluster: Option<(f64, f64)>,
    pub top: Option<(String, f64)>,
    pub glass: Option<f64>,
    pub material: String,
}

impl Default for Railing {
    fn default() -> Self {
        Railing {
            height: 900.0,
            post: Some((50.0, 1500.0)),
            baluster: None,
            top: Some(("round".into(), 40.0)),
            glass: None,
            material: "steel".into(),
        }
    }
}

fn size_at(text: &str, scope: &crate::parse::Scope, key: &str) -> Result<Option<(f64, f64)>> {
    if text == "none" {
        return Ok(None);
    }
    let (size, spacing) = text
        .split_once('@')
        .ok_or_else(|| anyhow!("`{key}=` is a size and spacing like `50@1500`, or none"))?;
    let (size, spacing) = (eval(size, scope)?, eval(spacing, scope)?);
    if size <= 0.0 || spacing <= 0.0 {
        bail!("`{key}=` size and spacing must be above zero");
    }
    Ok(Some((size, spacing)))
}

fn point(p: P3) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}

fn level_dir(a: P3, b: P3) -> Vector3 {
    let d = Vector3::new(b[0] - a[0], b[1] - a[1], 0.0);
    if d.magnitude() < 1.0e-9 {
        Vector3::unit_x()
    } else {
        d.normalize()
    }
}

fn rail_offset(a: P3, b: P3, height: f64, radius: f64) -> f64 {
    let run = (b[0] - a[0]).hypot(b[1] - a[1]);
    let cos = if run < 1.0e-9 {
        1.0
    } else {
        run / run.hypot(b[2] - a[2])
    };
    height - radius * cos
}

impl Model {
    pub(crate) fn railing_options(&self, line: &Line, extra: &[&str]) -> Result<Railing> {
        let mut r = Railing::default();
        let mut top_given = false;
        for (key, text) in &line.named {
            match key.as_str() {
                "height" => {
                    r.height = eval(text, &self.scope).with_context(|| format!("`height={text}`"))?
                }
                "post" => r.post = size_at(text, &self.scope, "post")?,
                "baluster" => r.baluster = size_at(text, &self.scope, "baluster")?,
                "top" => {
                    top_given = true;
                    r.top = if text == "none" {
                        None
                    } else {
                        let (kind, size) = text
                            .split_once(':')
                            .filter(|(k, _)| *k == "round" || *k == "rect")
                            .ok_or_else(|| {
                                anyhow!("`top=` is round:D, rect:S or none, not `{text}`")
                            })?;
                        let size = eval(size, &self.scope)?;
                        if size <= 0.0 {
                            bail!("the top rail size must be above zero");
                        }
                        Some((kind.to_string(), size))
                    }
                }
                "glass" => {
                    let t = eval(text, &self.scope).with_context(|| format!("`glass={text}`"))?;
                    if t <= 0.0 {
                        bail!("glass= is the pane thickness, above zero");
                    }
                    r.glass = Some(t);
                }
                "material" => r.material = text.clone(),
                other if extra.contains(&other) => {}
                other => bail!(
                    "`railing` takes height=, post=, baluster=, top=, glass=, material=, along=, not `{other}=`"
                ),
            }
        }
        if r.glass.is_some() && !top_given {
            r.top = None;
        }
        if r.height <= 0.0 {
            bail!("a railing height must be above zero");
        }
        Ok(r)
    }

    fn railing_path(&self, line: &Line) -> Result<(Vec<P3>, String)> {
        let along = line.named.iter().find(|(k, _)| k == "along").map(|(_, v)| v);
        match (line.positional.as_slice(), along) {
            ([kind, points @ ..], None) if kind == "path" => {
                let path = points
                    .iter()
                    .map(|p| {
                        crate::args::point3(p, &self.scope).map(|q| [q.x, q.y, q.z])
                    })
                    .collect::<Result<Vec<_>>>()?;
                if path.len() < 2 || path_length(&path) < 1.0e-6 {
                    bail!("a railing path needs two or more different points");
                }
                Ok((path, "a path".into()))
            }
            ([], Some(target)) => {
                let host = target.strip_suffix(".edge").ok_or_else(|| {
                    anyhow!("`along=` names a stair or slab edge, like `along=st1.edge`")
                })?;
                if let Some(stair) = self.arch.stairs.iter().find(|s| s.name == host) {
                    return Ok((stair.edge.clone(), format!("stair {host}")));
                }
                if let Some(slab) = self.arch.slabs.iter().find(|s| s.name == host) {
                    let mut path: Vec<P3> =
                        slab.outline.iter().map(|p| [p.0, p.1, slab.top]).collect();
                    path.push(path[0]);
                    return Ok((path, format!("slab {host}")));
                }
                bail!("no stair or slab `{host}` to run a railing along")
            }
            _ => bail!("write `railing path x,y,z x,y,z ...` or `railing along=st1.edge`"),
        }
    }

    pub(crate) fn build_railing(&mut self, name: &str, path: &[P3], r: &Railing) -> Result<usize> {
        let mut made = Vec::new();
        let mut put = |model: &mut Model, part: &str, solid: Solid, material: &str| -> Result<()> {
            let body = piece_of(&format!("{name}.{part}"), &made);
            made.push(body.clone());
            model.put_body(body.clone(), solid, material)?;
            model.note_built(name, &body);
            Ok(())
        };
        let total = path_length(path);
        let top_size = r.top.as_ref().map_or(0.0, |t| t.1);
        let stations = match r.post {
            Some((_, spacing)) => post_stations(total, spacing),
            None => vec![0.0, total],
        };
        let seg_dir = |s: f64| {
            let a = point_along(path, (s - 1.0).max(0.0));
            let b = point_along(path, (s + 1.0).min(total));
            level_dir(a, b)
        };
        if let Some((size, _)) = r.post {
            for (k, &s) in stations.iter().enumerate() {
                let p = point_along(path, s);
                let solid = straight_member(
                    "bar",
                    &format!("{size}x{size}"),
                    point(p),
                    point(p) + Vector3::unit_z() * (r.height - top_size),
                    seg_dir(s),
                    false,
                )?;
                put(self, &format!("post.{}", k + 1), solid, &r.material)?;
            }
        }
        if let Some((size, spacing)) = r.baluster {
            for (k, s) in infill_stations(&stations, spacing).into_iter().enumerate() {
                let p = point_along(path, s);
                let solid = straight_member(
                    "rod",
                    &crate::parse::label(size),
                    point(p) + Vector3::unit_z() * BALUSTER_GAP,
                    point(p) + Vector3::unit_z() * (r.height - top_size),
                    seg_dir(s),
                    false,
                )?;
                put(self, &format!("baluster.{}", k + 1), solid, &r.material)?;
            }
        }
        if let Some(t) = r.glass {
            let mut pane = 0;
            for w in path.windows(2) {
                let run = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
                if run < 1.0e-6 {
                    continue;
                }
                let axis = Flight {
                    start: (w[0][0], w[0][1]),
                    dir: ((w[1][0] - w[0][0]) / run, (w[1][1] - w[0][1]) / run),
                    z0: w[0][2],
                    treads: 0,
                };
                let rise = w[1][2] - w[0][2];
                let profile = [
                    (0.0, 0.0),
                    (run, rise),
                    (run, rise + r.height - top_size),
                    (0.0, r.height - top_size),
                ];
                let solid = axis.slab(&profile, -t / 2.0, t / 2.0)?;
                pane += 1;
                put(self, &format!("glass.{pane}"), solid, "glass")?;
            }
        }
        if let Some((kind, size)) = &r.top {
            let radius = size / 2.0;
            for w in path.windows(2) {
                if path_length(w) < 1.0e-6 {
                    continue;
                }
                let lift = rail_offset(w[0], w[1], r.height, radius);
                let up = Vector3::unit_z();
                let (from, to) = (point(w[0]) + up * lift, point(w[1]) + up * lift);
                let (section, dims) = if kind == "round" {
                    ("rod", crate::parse::label(*size))
                } else {
                    ("bar", format!("{size}x{size}"))
                };
                let solid = straight_member(section, &dims, from, to, up, false)?;
                put(self, "top", solid, &r.material)?;
            }
        }
        Ok(stations.len())
    }

    pub(crate) fn op_railing(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a railing needs a label, like `rl1: railing path 0,0,0 3000,0,0 height=900`")
        })?;
        self.check_free_name(&name)?;
        let r = self.railing_options(line, &["along"])?;
        let (path, what) = self.railing_path(line)?;
        self.ensure_library()?;
        let posts = self
            .build_railing(&name, &path, &r)
            .with_context(|| format!("building `{name}`"))?;
        Ok(format!(
            "railing {name} along {what}: {} long, {} high, {}",
            crate::parse::label(path_length(&path)),
            crate::parse::label(r.height),
            match (r.glass, r.post) {
                (Some(t), _) => format!("{} glass", crate::parse::label(t)),
                (None, Some(_)) => format!("{posts} posts"),
                (None, None) => "no posts".into(),
            }
        ))
    }

    pub(crate) fn op_handrail(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a handrail needs a label, like `hr: handrail path 0,0,0 3000,0,0 d=40`")
        })?;
        self.check_free_name(&name)?;
        let (mut height, mut d, mut spacing) = (900.0, 40.0, 1000.0);
        let mut material = "steel".to_string();
        for (key, text) in &line.named {
            let value = || eval(text, &self.scope).with_context(|| format!("`{key}={text}`"));
            match key.as_str() {
                "height" => height = value()?,
                "d" => d = value()?,
                "bracket" => {
                    spacing = eval(
                        text.strip_prefix('@')
                            .ok_or_else(|| anyhow!("`bracket=` is a spacing like `@1000`"))?,
                        &self.scope,
                    )?
                }
                "material" => material = text.clone(),
                "along" => {}
                other => bail!(
                    "`handrail` takes height=, d=, bracket=, material= and along=, not `{other}=`"
                ),
            }
        }
        if height <= 0.0 || d <= 0.0 || spacing <= 0.0 {
            bail!("a handrail's height=, d= and bracket= must be above zero");
        }
        let (path, what) = self.railing_path(line)?;
        self.ensure_library()?;
        let mut made = Vec::new();
        for w in path.windows(2) {
            if path_length(w) < 1.0e-6 {
                continue;
            }
            let lift = rail_offset(w[0], w[1], height, d / 2.0);
            let up = Vector3::unit_z();
            let solid = straight_member(
                "rod",
                &crate::parse::label(d),
                point(w[0]) + up * lift,
                point(w[1]) + up * lift,
                up,
                false,
            )
            .with_context(|| format!("building `{name}`"))?;
            let body = piece_of(&format!("{name}.rail"), &made);
            made.push(body.clone());
            self.put_body(body.clone(), solid, &material)?;
            self.note_built(&name, &body);
        }
        let total = path_length(&path);
        let stations = post_stations(total, spacing);
        let (bd, drop) = BRACKET;
        for (k, &s) in stations.iter().enumerate() {
            let p = point(point_along(&path, s));
            let top = p + Vector3::unit_z() * (height - d / 2.0);
            let solid = straight_member(
                "rod",
                &crate::parse::label(bd),
                top - Vector3::unit_z() * (d / 2.0 + drop),
                top,
                Vector3::unit_x(),
                false,
            )
            .with_context(|| format!("building `{name}`"))?;
            let body = format!("{name}.bracket.{}", k + 1);
            self.put_body(body.clone(), solid, &material)?;
            self.note_built(&name, &body);
        }
        Ok(format!(
            "handrail {name} along {what}: {} long, {} round at {}, {} brackets",
            crate::parse::label(total),
            crate::parse::label(d),
            crate::parse::label(height),
            stations.len()
        ))
    }
}
