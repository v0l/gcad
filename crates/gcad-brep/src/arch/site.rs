use super::slab::{inside, prism};
use super::wall::{loft, planar_solid};
use super::{Pad, SiteBoundary, Terrain};
use crate::Model;
use crate::parse::{Line, eval, eval_point, label};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::P2;
use gcad_mesh::arch::layout::{length, signed_area, sub, unit};
use monstertruck::modeling::*;

type P3 = [f64; 3];

fn clip_half(poly: &[P2], keep: impl Fn(P2) -> f64) -> Vec<P2> {
    let mut out = Vec::new();
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        let (fa, fb) = (keep(a), keep(b));
        if fa >= 0.0 {
            out.push(a);
        }
        if (fa >= 0.0) != (fb >= 0.0) {
            let t = fa / (fa - fb);
            out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
    }
    out
}

fn centroid_area(poly: &[P2]) -> Option<(P2, f64)> {
    let area = signed_area(poly);
    if area.abs() < 1.0e-9 || poly.len() < 3 {
        return None;
    }
    let mut c = (0.0, 0.0);
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        let w = a.0 * b.1 - b.0 * a.1;
        c = (c.0 + (a.0 + b.0) * w, c.1 + (a.1 + b.1) * w);
    }
    Some(((c.0 / (6.0 * area), c.1 / (6.0 * area)), area.abs()))
}

pub(crate) fn cut_and_fill(tris: &[[P3; 3]], pad: &[P2], level: f64) -> (f64, f64) {
    let mut pad = pad.to_vec();
    if signed_area(&pad) < 0.0 {
        pad.reverse();
    }
    let (mut cut, mut fill) = (0.0, 0.0);
    for t in tris {
        let mut piece = pad.clone();
        let sign = orient(t[0], t[1], t[2]).signum();
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            piece = clip_half(&piece, |p| {
                sign * ((b[0] - a[0]) * (p.1 - a[1]) - (b[1] - a[1]) * (p.0 - a[0]))
            });
            if piece.len() < 3 {
                break;
            }
        }
        if piece.len() < 3 {
            continue;
        }
        let area = orient(t[0], t[1], t[2]);
        let height = |p: P2| {
            let w0 = orient([p.0, p.1, 0.0], t[1], t[2]) / area;
            let w1 = orient(t[0], [p.0, p.1, 0.0], t[2]) / area;
            w0 * t[0][2] + w1 * t[1][2] + (1.0 - w0 - w1) * t[2][2] - level
        };
        let up = clip_half(&piece, &height);
        let down = clip_half(&piece, |p| -height(p));
        if let Some((c, a)) = centroid_area(&up) {
            cut += a * height(c);
        }
        if let Some((c, a)) = centroid_area(&down) {
            fill -= a * height(c);
        }
    }
    (cut, fill)
}

fn segment_distance(p: P2, a: P2, b: P2) -> f64 {
    let ab = sub(b, a);
    let t = (((p.0 - a.0) * ab.0 + (p.1 - a.1) * ab.1) / (ab.0 * ab.0 + ab.1 * ab.1)).clamp(0.0, 1.0);
    length(sub(p, (a.0 + ab.0 * t, a.1 + ab.1 * t)))
}

pub(crate) fn setback_of(ring: &[P2], p: P2) -> Option<f64> {
    if !inside(p, ring) {
        return None;
    }
    (0..ring.len())
        .map(|k| segment_distance(p, ring[k], ring[(k + 1) % ring.len()]))
        .min_by(f64::total_cmp)
}

fn orient(a: P3, b: P3, c: P3) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn in_circle(a: P3, b: P3, c: P3, p: P3) -> bool {
    let (ax, ay) = (a[0] - p[0], a[1] - p[1]);
    let (bx, by) = (b[0] - p[0], b[1] - p[1]);
    let (cx, cy) = (c[0] - p[0], c[1] - p[1]);
    let det = (ax * ax + ay * ay) * (bx * cy - cx * by)
        - (bx * bx + by * by) * (ax * cy - cx * ay)
        + (cx * cx + cy * cy) * (ax * by - bx * ay);
    let sign = orient(a, b, c).signum();
    det * sign > 1.0e-9
}

pub(crate) fn triangulate(points: &[P3]) -> Vec<[P3; 3]> {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in points {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0) * 20.0;
    let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let sup = [
        [mid[0] - span, mid[1] - span, 0.0],
        [mid[0] + span, mid[1] - span, 0.0],
        [mid[0], mid[1] + span, 0.0],
    ];
    let mut tris: Vec<[P3; 3]> = vec![sup];
    for &p in points {
        let (bad, keep): (Vec<_>, Vec<_>) = tris
            .into_iter()
            .partition(|t| in_circle(t[0], t[1], t[2], p));
        let mut edges: Vec<(P3, P3)> = Vec::new();
        for t in &bad {
            for k in 0..3 {
                let e = (t[k], t[(k + 1) % 3]);
                if let Some(i) = edges.iter().position(|f| f.0 == e.1 && f.1 == e.0) {
                    edges.remove(i);
                } else {
                    edges.push(e);
                }
            }
        }
        tris = keep;
        for (a, b) in edges {
            if orient(a, b, p).abs() > 1.0e-9 {
                tris.push(if orient(a, b, p) > 0.0 { [a, b, p] } else { [b, a, p] });
            }
        }
    }
    tris.into_iter()
        .filter(|t| t.iter().all(|v| !sup.contains(v)))
        .collect()
}

pub(crate) fn height_at(tris: &[[P3; 3]], x: f64, y: f64) -> Option<f64> {
    tris.iter().find_map(|t| {
        let area = orient(t[0], t[1], t[2]);
        let w0 = orient([x, y, 0.0], t[1], t[2]) / area;
        let w1 = orient(t[0], [x, y, 0.0], t[2]) / area;
        let w2 = 1.0 - w0 - w1;
        let e = -1.0e-9;
        (w0 >= e && w1 >= e && w2 >= e).then(|| w0 * t[0][2] + w1 * t[1][2] + w2 * t[2][2])
    })
}

fn boundary_loop(tris: &[[P3; 3]]) -> Vec<P3> {
    let mut edges: Vec<(P3, P3)> = Vec::new();
    for t in tris {
        for k in 0..3 {
            let e = (t[k], t[(k + 1) % 3]);
            if let Some(i) = edges.iter().position(|f| f.0 == e.1 && f.1 == e.0) {
                edges.remove(i);
            } else {
                edges.push(e);
            }
        }
    }
    let mut ring = vec![edges[0].0];
    let mut at = edges[0].1;
    while at != ring[0] {
        ring.push(at);
        match edges.iter().find(|e| e.0 == at) {
            Some(e) => at = e.1,
            None => break,
        }
    }
    ring
}

pub(crate) fn terrain_solid(tris: &[[P3; 3]], depth: f64) -> Result<Solid> {
    let floor = tris
        .iter()
        .flatten()
        .map(|p| p[2])
        .fold(f64::INFINITY, f64::min)
        - depth;
    let point = |p: P3| Point3::new(p[0], p[1], p[2]);
    let ring = boundary_loop(tris);
    let mut faces: Vec<Vec<Vec<Point3>>> = tris
        .iter()
        .map(|t| vec![t.iter().map(|&p| point(p)).collect()])
        .collect();
    let n = ring.len();
    for k in 0..n {
        let (a, b) = (ring[k], ring[(k + 1) % n]);
        let (a0, b0) = ([a[0], a[1], floor], [b[0], b[1], floor]);
        faces.push(vec![vec![point(a0), point(b0), point(b), point(a)]]);
    }
    faces.push(vec![
        ring.iter()
            .rev()
            .map(|p| Point3::new(p[0], p[1], floor))
            .collect(),
    ]);
    planar_solid(&faces)
}

fn named<'a>(line: &'a Line, key: &str) -> Option<&'a str> {
    line.named.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

pub(crate) fn contour_points(text: &str) -> Result<Vec<P3>> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter(|p| p.len() == 2).map(|p| (p[0], p[1])).collect();
    let mut out: Vec<P3> = Vec::new();
    let mut kind = "";
    let mut elevation = 0.0;
    let mut pending: Vec<(f64, f64)> = Vec::new();
    let mut vertex: (Option<f64>, Option<f64>) = (None, None);
    let flush = |kind: &str, elevation: f64, pending: &mut Vec<(f64, f64)>, out: &mut Vec<P3>| {
        if kind == "LWPOLYLINE" {
            out.extend(pending.iter().map(|p| [p.0, p.1, elevation]));
        }
        pending.clear();
    };
    for (code, value) in pairs {
        let number = value.parse::<f64>().ok();
        match (code, number) {
            ("0", _) => {
                flush(kind, elevation, &mut pending, &mut out);
                kind = match value {
                    "LWPOLYLINE" => "LWPOLYLINE",
                    "VERTEX" => "VERTEX",
                    _ => "",
                };
                elevation = 0.0;
                vertex = (None, None);
            }
            ("38", Some(z)) if kind == "LWPOLYLINE" => elevation = z,
            ("10", Some(x)) if kind == "LWPOLYLINE" => pending.push((x, 0.0)),
            ("20", Some(y)) if kind == "LWPOLYLINE" => {
                if let Some(last) = pending.last_mut() {
                    last.1 = y;
                }
            }
            ("10", Some(x)) if kind == "VERTEX" => vertex.0 = Some(x),
            ("20", Some(y)) if kind == "VERTEX" => vertex.1 = Some(y),
            ("30", Some(z)) if kind == "VERTEX" => {
                if let (Some(x), Some(y)) = vertex {
                    out.push([x, y, z]);
                }
            }
            _ => {}
        }
    }
    flush(kind, elevation, &mut pending, &mut out);
    out.dedup();
    if out.len() < 3 {
        bail!("the DXF has no contour polylines with elevations (LWPOLYLINE with group 38, or POLYLINE vertices with z)");
    }
    Ok(out)
}

fn read_points(text: &str) -> Result<Vec<P3>> {
    let mut out: Vec<P3> = Vec::new();
    for line in text.lines() {
        let numbers: Vec<f64> = line
            .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
            .filter(|w| !w.is_empty())
            .filter_map(|w| w.parse().ok())
            .collect();
        if let [x, y, z] = numbers[..] {
            out.push([x, y, z]);
        }
    }
    out.dedup();
    if out.len() < 3 {
        bail!("a terrain needs three or more points x,y,z");
    }
    Ok(out)
}

impl Model {
    pub fn contour_segments(&self, interval: f64) -> Vec<(f64, (f64, f64), (f64, f64))> {
        let mut out = Vec::new();
        for terrain in &self.arch.terrains {
            let zs: Vec<f64> = terrain.tris.iter().flatten().map(|p| p[2]).collect();
            let (lo, hi) = zs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, z| (a.0.min(*z), a.1.max(*z)));
            let mut level = (lo / interval).ceil() * interval;
            while level <= hi + 1.0e-9 {
                for t in &terrain.tris {
                    let mut hits: Vec<(f64, f64)> = Vec::new();
                    for k in 0..3 {
                        let (a, b) = (t[k], t[(k + 1) % 3]);
                        if (a[2] - level) * (b[2] - level) < 0.0 {
                            let s = (level - a[2]) / (b[2] - a[2]);
                            hits.push((a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s));
                        }
                    }
                    if let [p, q] = hits[..] {
                        out.push((level, p, q));
                    }
                }
                level += interval;
            }
        }
        out
    }
    pub(crate) fn check_setbacks(&self, wall: &super::Wall) -> Result<()> {
        for site in &self.arch.sites {
            for p in &wall.line.points {
                match setback_of(&site.ring, *p) {
                    Some(d) if d >= site.setback - 1.0e-6 => {}
                    _ => bail!(
                        "wall `{}` is within the {} setback of the site boundary",
                        wall.name,
                        label(site.setback)
                    ),
                }
            }
        }
        Ok(())
    }

    pub(crate) fn op_site_boundary(&mut self, line: &Line) -> Result<String> {
        let mut setback = 0.0;
        for (key, text) in &line.named {
            match key.as_str() {
                "setback" => setback = eval(text, &self.scope).with_context(|| format!("`setback={text}`"))?,
                other => bail!("`site_boundary` takes setback=, not `{other}=`"),
            }
        }
        let ring = self.plan_ring(&line.positional)?;
        let area = signed_area(&ring).abs() / 1.0e6;
        self.arch.sites.push(SiteBoundary { ring, setback });
        Ok(format!("site boundary: area {area:.2} m², setback {}", label(setback)))
    }

    pub(crate) fn op_pad(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| anyhow!("a pad needs a label, like `p1: pad building level=L0`"))?;
        let (mut level, mut offset) = (None, 0.0);
        for (key, text) in &line.named {
            let value = || eval(text, &self.scope).with_context(|| format!("`{key}={text}`"));
            match key.as_str() {
                "level" => level = Some(value()?),
                "offset" => offset = value()?,
                other => bail!("`pad` takes level= and offset=, not `{other}=`"),
            }
        }
        let level = level.ok_or_else(|| anyhow!("a pad needs `level=`"))?;
        let ring = match line.positional.as_slice() {
            [wall] if self.arch.wall(wall).is_some() => {
                let wall = self.arch.wall(wall).cloned().expect("a wall");
                let (lo, hi) = wall.outer_span();
                let ring = |d: f64| -> Vec<P2> { wall.line.offset(d, None, None).into_iter().map(|(p, _)| p).collect() };
                let (a, b) = (ring(lo - offset), ring(hi + offset));
                if signed_area(&a).abs() > signed_area(&b).abs() { a } else { b }
            }
            points => self.plan_ring(points)?,
        };
        let area = signed_area(&ring).abs() / 1.0e6;
        self.arch.pads.retain(|p| p.name != name);
        self.arch.pads.push(Pad { name: name.clone(), ring, level });
        Ok(format!("pad {name}: {area:.2} m² at {}", label(level)))
    }

    pub(crate) fn op_cut_fill(&mut self, line: &Line) -> Result<String> {
        let [terrain, pad] = line.positional.as_slice() else {
            bail!("write `cut_fill t1 p1`");
        };
        let terrain = self.arch.terrains.iter().find(|t| t.name == *terrain).ok_or_else(|| anyhow!("no terrain `{terrain}`"))?;
        let pad = self.arch.pads.iter().find(|p| p.name == *pad).ok_or_else(|| anyhow!("no pad `{pad}`"))?;
        let (cut, fill) = cut_and_fill(&terrain.tris, &pad.ring, pad.level);
        Ok(format!("cut {:.2} m³, fill {:.2} m³", cut / 1.0e9, fill / 1.0e9))
    }

    fn segment_ring(&self, line: &Line, width: f64) -> Result<Vec<P2>> {
        let [a, b] = line.positional.as_slice() else {
            bail!("`{}` takes two points", line.op);
        };
        let (a, b) = (self.plan_point(a)?, self.plan_point(b)?);
        let d = unit(sub(b, a));
        let n = (-d.1 * width / 2.0, d.0 * width / 2.0);
        Ok(vec![
            (a.0 - n.0, a.1 - n.1),
            (b.0 - n.0, b.1 - n.1),
            (b.0 + n.0, b.1 + n.1),
            (a.0 + n.0, a.1 + n.1),
        ])
    }

    pub(crate) fn op_road(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| anyhow!("a road needs a label"))?;
        self.check_free_name(&name)?;
        let (mut width, mut layers) = (3500.0, None);
        for (key, text) in &line.named {
            match key.as_str() {
                "width" => width = eval(text, &self.scope)?,
                "type" => layers = Some(super::slab::slab_layers(self, text)?),
                other => bail!("`road` takes width= and type=, not `{other}=`"),
            }
        }
        let layers = layers.ok_or_else(|| anyhow!("a road needs `type=`, like `asphalt:100`"))?;
        let ring = self.segment_ring(line, width)?;
        let ends = (self.plan_point(&line.positional[0])?, self.plan_point(&line.positional[1])?);
        self.arch.roads.push((name.clone(), ends.0, ends.1));
        let mut z = 0.0;
        for (k, l) in layers.iter().enumerate().rev() {
            let body = if layers.len() == 1 { name.clone() } else { format!("{name}.{}", k + 1) };
            self.put_body(body.clone(), prism(&ring, &[], z - l.thickness, z)?, &l.material)?;
            self.note_built(&name, &body);
            z -= l.thickness;
        }
        Ok(format!("road {name}: {} wide", label(width)))
    }

    pub(crate) fn check_driveway(&mut self, line: &Line) -> Result<String> {
        let max = match super::check::named_value(line, "max") {
            Some(m) => eval(&m, &self.scope)?,
            None => 0.1,
        };
        let mut problems = Vec::new();
        for (name, a, b) in self.arch.roads.clone() {
            let heights: Vec<Option<f64>> = self
                .arch
                .terrains
                .iter()
                .map(|t| height_at(&t.tris, a.0, a.1).zip(height_at(&t.tris, b.0, b.1)).map(|(za, zb)| zb - za))
                .collect();
            let Some(Some(rise)) = heights.first().copied() else { continue };
            let slope = rise.abs() / length(sub(b, a));
            if slope > max + 1.0e-9 {
                problems.push(format!("{name} slope {:.1} percent over {:.1}", slope * 100.0, max * 100.0));
            }
        }
        self.finish_check(line, problems, "driveways checked".to_string())
    }

    pub(crate) fn op_retaining(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| anyhow!("a retaining wall needs a label"))?;
        self.check_free_name(&name)?;
        let (mut h, mut t) = (None, None);
        for (key, text) in &line.named {
            match key.as_str() {
                "h" => h = Some(eval(text, &self.scope)?),
                "t" => t = Some(eval(text, &self.scope)?),
                other => bail!("`retaining` takes h= and t=, not `{other}=`"),
            }
        }
        let (h, t) = h.zip(t).ok_or_else(|| anyhow!("a retaining wall needs `h=` and `t=`"))?;
        let ring = self.segment_ring(line, t)?;
        self.put_body(name.clone(), prism(&ring, &[], 0.0, h)?, "concrete")?;
        self.note_built(&name, &name);
        Ok(format!("retaining {name}: {} high", label(h)))
    }

    pub(crate) fn op_tree(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| anyhow!("a tree needs a label"))?;
        self.check_free_name(&name)?;
        let [at] = line.positional.as_slice() else {
            bail!("write `tr: tree x,y h=6000 spread=4000`");
        };
        let c = self.plan_point(at)?;
        let (mut h, mut spread) = (6000.0, 4000.0);
        for (key, text) in &line.named {
            match key.as_str() {
                "h" => h = eval(text, &self.scope)?,
                "spread" => spread = eval(text, &self.scope)?,
                other => bail!("`tree` takes h= and spread=, not `{other}=`"),
            }
        }
        if h <= 0.0 || spread <= 0.0 {
            bail!("a tree's h= and spread= must be above zero");
        }
        let ring = |r: f64, z: f64| -> Vec<Vec<Point3>> {
            vec![(0..8).map(|k| {
                let a = std::f64::consts::TAU * k as f64 / 8.0;
                Point3::new(c.0 + r * a.cos(), c.1 + r * a.sin(), z)
            }).collect()]
        };
        let solid = loft(&ring(spread / 2.0, h * 0.3), &ring(spread / 8.0, h))?;
        self.put_body(name.clone(), solid, "soil")?;
        self.note_built(&name, &name);
        Ok(format!("tree {name}: {} high, {} spread", label(h), label(spread)))
    }

    pub(crate) fn op_terrain(&mut self, line: &Line) -> Result<String> {
        let name = line
            .label
            .clone()
            .ok_or_else(|| anyhow!("a terrain needs a label, like `t1: terrain points site.csv`"))?;
        self.check_free_name(&name)?;
        let mut depth = 1000.0;
        for (key, text) in &line.named {
            match key.as_str() {
                "depth" => depth = eval(text, &self.scope).with_context(|| format!("`depth={text}`"))?,
                "slope" | "at" => {}
                other => bail!("`terrain` takes depth=, slope= and at=, not `{other}=`"),
            }
        }
        if depth <= 0.0 {
            bail!("a terrain's depth= must be above zero");
        }
        let points = match line.positional.as_slice() {
            [kind, file] if kind == "points" => {
                let path = self.relative(file);
                let text = crate::host::read_to_string(&path)
                    .map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
                read_points(&text)?
            }
            [kind, file] if kind == "contour" => {
                let path = self.relative(file);
                let text = crate::host::read_to_string(&path)
                    .map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
                contour_points(&text)?
            }
            [kind, size] if kind == "grid" => {
                let (w, d) = size
                    .split_once('x')
                    .ok_or_else(|| anyhow!("write the grid size as 20000x20000"))?;
                let (w, d) = (eval(w, &self.scope)?, eval(d, &self.scope)?);
                let slope = match named(line, "slope") {
                    Some(s) => eval_point(s, &self.scope)?,
                    None => (0.0, 0.0),
                };
                let at = match named(line, "at") {
                    Some(s) => eval_point(s, &self.scope)?,
                    None => (0.0, 0.0),
                };
                let z = |x: f64, y: f64| slope.0 * (x - at.0) + slope.1 * (y - at.1);
                let c = |dx: f64, dy: f64| [at.0 + dx, at.1 + dy, z(at.0 + dx, at.1 + dy)];
                vec![c(0.0, 0.0), c(w, 0.0), c(w, d), c(0.0, d)]
            }
            _ => bail!("write `t1: terrain points site.csv depth=1000` or `terrain grid 20000x20000 slope=0.1,0`"),
        };
        let tris = if points.len() == 4 && line.positional[0] == "grid" {
            vec![[points[0], points[1], points[2]], [points[0], points[2], points[3]]]
        } else {
            triangulate(&points)
        };
        if tris.is_empty() {
            bail!("the terrain points are in a line");
        }
        let solid = terrain_solid(&tris, depth)?;
        self.ensure_library()?;
        self.put_body(name.clone(), solid, "soil")?;
        self.note_built(&name, &name);
        let count = tris.len();
        self.arch.terrains.push(Terrain {
            name: name.clone(),
            tris,
            depth,
        });
        Ok(format!("terrain {name}: {} points, {count} triangles", points.len()))
    }
}
