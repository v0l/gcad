use super::{Mesh, P3, add, cross, dot, length, normalized, scale, sub};
use std::collections::HashMap;

const CHART_COS: f64 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub corners: Vec<[[f64; 2]; 3]>,
    pub charts: Vec<u32>,
    pub chart_count: usize,
}

fn face_normal(mesh: &Mesh, t: [u32; 3]) -> P3 {
    let [a, b, c] = mesh.corners(t);
    normalized(cross(sub(b, a), sub(c, a)))
}

fn face_area(mesh: &Mesh, t: [u32; 3]) -> f64 {
    let [a, b, c] = mesh.corners(t);
    length(cross(sub(b, a), sub(c, a))) / 2.0
}

pub fn edge_triangles(mesh: &Mesh) -> HashMap<(u32, u32), Vec<u32>> {
    let mut edges: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    for (f, t) in mesh.triangles.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            edges
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push(f as u32);
        }
    }
    edges
}

pub fn charts(mesh: &Mesh) -> (Vec<u32>, Vec<P3>) {
    let normals: Vec<P3> = mesh
        .triangles
        .iter()
        .map(|&t| face_normal(mesh, t))
        .collect();
    let edges = edge_triangles(mesh);
    let mut next: Vec<Vec<u32>> = vec![Vec::new(); mesh.triangles.len()];
    for tris in edges.values() {
        for &a in tris {
            for &b in tris {
                if a != b {
                    next[a as usize].push(b);
                }
            }
        }
    }
    let mut order: Vec<usize> = (0..mesh.triangles.len()).collect();
    order.sort_by(|&a, &b| {
        face_area(mesh, mesh.triangles[b]).total_cmp(&face_area(mesh, mesh.triangles[a]))
    });
    let mut chart = vec![u32::MAX; mesh.triangles.len()];
    let mut axes = Vec::new();
    for seed in order {
        if chart[seed] != u32::MAX {
            continue;
        }
        let id = axes.len() as u32;
        let axis = normals[seed];
        axes.push(axis);
        chart[seed] = id;
        let mut stack = vec![seed as u32];
        while let Some(f) = stack.pop() {
            for &g in &next[f as usize] {
                if chart[g as usize] == u32::MAX && dot(normals[g as usize], axis) > CHART_COS {
                    chart[g as usize] = id;
                    stack.push(g);
                }
            }
        }
    }
    (chart, axes)
}

fn basis(axis: P3) -> (P3, P3) {
    let helper = if axis[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalized(cross(axis, helper));
    (u, cross(axis, u))
}

struct Placed {
    min: [f64; 2],
    size: [f64; 2],
}

fn shelves(sizes: &[[f64; 2]], order: &[usize], s: f64, gutter: f64) -> Option<Vec<[f64; 2]>> {
    let limit = 1.0 + 1.0e-12;
    let mut at = vec![[0.0; 2]; sizes.len()];
    let (mut x, mut y, mut row) = (0.0f64, 0.0f64, 0.0f64);
    for &c in order {
        let w = sizes[c][0] * s + 2.0 * gutter;
        let h = sizes[c][1] * s + 2.0 * gutter;
        if x + w > limit {
            x = 0.0;
            y += row;
            row = 0.0;
        }
        if x + w > limit || y + h > limit {
            return None;
        }
        at[c] = [x, y];
        x += w;
        row = row.max(h);
    }
    Some(at)
}

fn rotated(q: [f64; 2], angle: f64) -> [f64; 2] {
    let (s, c) = angle.sin_cos();
    [q[0] * c - q[1] * s, q[0] * s + q[1] * c]
}

fn tightest_turn(points: &[[f64; 2]]) -> f64 {
    let mut best = (f64::INFINITY, 0.0);
    for step in 0..90 {
        let angle = (step as f64 * 2.0).to_radians();
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for &p in points {
            let q = rotated(p, angle);
            for k in 0..2 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
        let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
        let area = w * h;
        if area < best.0 - 1.0e-12 {
            best = (
                area,
                if w >= h {
                    angle
                } else {
                    angle + std::f64::consts::FRAC_PI_2
                },
            );
        }
    }
    best.1
}

pub fn unwrap(mesh: &Mesh, size: usize) -> Layout {
    let (chart, axes) = charts(mesh);
    let count = axes.len();
    let mut placed: Vec<Placed> = (0..count)
        .map(|_| Placed {
            min: [f64::INFINITY; 2],
            size: [0.0; 2],
        })
        .collect();
    let mut points: Vec<Vec<[f64; 2]>> = vec![Vec::new(); count];
    for (f, t) in mesh.triangles.iter().enumerate() {
        let c = chart[f] as usize;
        let (u, v) = basis(axes[c]);
        for p in mesh.corners(*t) {
            points[c].push([dot(p, u), dot(p, v)]);
        }
    }
    let turn: Vec<f64> = points.iter().map(|pts| tightest_turn(pts)).collect();
    let flat = |c: usize, p: P3| {
        let (u, v) = basis(axes[c]);
        let q = [dot(p, u), dot(p, v)];
        rotated(q, turn[c])
    };
    let mut max = vec![[f64::NEG_INFINITY; 2]; count];
    for c in 0..count {
        for &q in &points[c] {
            let q = rotated(q, turn[c]);
            for k in 0..2 {
                placed[c].min[k] = placed[c].min[k].min(q[k]);
                max[c][k] = max[c][k].max(q[k]);
            }
        }
    }
    for c in 0..count {
        placed[c].size = [max[c][0] - placed[c].min[0], max[c][1] - placed[c].min[1]];
    }
    let sizes: Vec<[f64; 2]> = placed.iter().map(|p| p.size).collect();
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| sizes[b][1].total_cmp(&sizes[a][1]).then(a.cmp(&b)));
    let gutter = 3.0 / size.max(1) as f64;
    let widest = sizes.iter().flatten().fold(1.0e-12f64, |m, v| m.max(*v));
    let (mut lo, mut hi) = (0.0, (1.0 - 2.0 * gutter).max(1.0e-6) / widest);
    let mut best = None;
    if let Some(at) = shelves(&sizes, &order, hi, gutter) {
        lo = hi;
        best = Some(at);
    } else {
        for _ in 0..48 {
            let mid = (lo + hi) / 2.0;
            match shelves(&sizes, &order, mid, gutter) {
                Some(at) => {
                    lo = mid;
                    best = Some(at);
                }
                None => hi = mid,
            }
        }
    }
    let s = lo;
    let at = best.unwrap_or_else(|| vec![[0.0; 2]; count]);
    let corners = mesh
        .triangles
        .iter()
        .enumerate()
        .map(|(f, t)| {
            let c = chart[f] as usize;
            t.map(|i| {
                let q = flat(c, mesh.positions[i as usize]);
                [
                    at[c][0] + gutter + (q[0] - placed[c].min[0]) * s,
                    at[c][1] + gutter + (q[1] - placed[c].min[1]) * s,
                ]
            })
        })
        .collect();
    Layout {
        corners,
        charts: chart,
        chart_count: count,
    }
}

#[derive(Clone, Debug, Default)]
pub struct Expanded {
    pub source: Vec<u32>,
    pub positions: Vec<P3>,
    pub normals: Vec<P3>,
    pub uvs: Vec<[f64; 2]>,
    pub tangents: Vec<[f64; 4]>,
    pub triangles: Vec<[u32; 3]>,
}

pub fn expand(mesh: &Mesh, layout: &Layout) -> Expanded {
    let smooth = mesh.vertex_normals();
    let mut out = Expanded::default();
    let mut seen: HashMap<(u32, u64, u64), u32> = HashMap::new();
    for (f, t) in mesh.triangles.iter().enumerate() {
        let mut tri = [0u32; 3];
        for k in 0..3 {
            let uv = layout.corners[f][k];
            let key = (t[k], uv[0].to_bits(), uv[1].to_bits());
            tri[k] = *seen.entry(key).or_insert_with(|| {
                out.source.push(t[k]);
                out.positions.push(mesh.positions[t[k] as usize]);
                out.normals.push(smooth[t[k] as usize]);
                out.uvs.push(uv);
                (out.source.len() - 1) as u32
            });
        }
        out.triangles.push(tri);
    }
    let n = out.source.len();
    let mut tan = vec![[0.0; 3]; n];
    let mut bit = vec![[0.0; 3]; n];
    for t in &out.triangles {
        let [p0, p1, p2] = t.map(|i| out.positions[i as usize]);
        let [a, b, c] = t.map(|i| {
            let uv = out.uvs[i as usize];
            [uv[0], 1.0 - uv[1]]
        });
        let (e1, e2) = (sub(p1, p0), sub(p2, p0));
        let (du1, dv1, du2, dv2) = (b[0] - a[0], b[1] - a[1], c[0] - a[0], c[1] - a[1]);
        let r = du1 * dv2 - du2 * dv1;
        if r.abs() < 1.0e-30 {
            continue;
        }
        let tangent = scale(sub(scale(e1, dv2), scale(e2, dv1)), 1.0 / r);
        let bitangent = scale(sub(scale(e2, du1), scale(e1, du2)), 1.0 / r);
        for &i in t {
            tan[i as usize] = add(tan[i as usize], tangent);
            bit[i as usize] = add(bit[i as usize], bitangent);
        }
    }
    out.tangents = (0..n)
        .map(|i| {
            let normal = out.normals[i];
            let mut t = sub(tan[i], scale(normal, dot(normal, tan[i])));
            if length(t) < 1.0e-12 {
                t = basis(normal).0;
            }
            let t = normalized(t);
            let w = if dot(cross(normal, t), bit[i]) < 0.0 {
                -1.0
            } else {
                1.0
            };
            [t[0], t[1], t[2], w]
        })
        .collect();
    out
}
