use super::Terrain;
use super::wall::planar_solid;
use crate::Model;
use crate::parse::{Line, eval};
use anyhow::{Context, Result, anyhow, bail};
use monstertruck::modeling::*;

type P3 = [f64; 3];

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
                other => bail!("`terrain` takes depth=, not `{other}=`"),
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
            _ => bail!("write `t1: terrain points site.csv depth=1000`"),
        };
        let tris = triangulate(&points);
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
