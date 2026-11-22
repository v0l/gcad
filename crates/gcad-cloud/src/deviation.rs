use crate::cloud::{Cloud, P3};
use anyhow::{Result, bail};

const LEAF: usize = 4;
const TIE: f64 = 1.0e-9;

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn closest_on_triangle(p: P3, [a, b, c]: [P3; 3]) -> P3 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return std::array::from_fn(|k| a[k] + v * ab[k]);
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return std::array::from_fn(|k| a[k] + w * ac[k]);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return std::array::from_fn(|k| b[k] + w * (c[k] - b[k]));
    }
    let denominator = 1.0 / (va + vb + vc);
    let v = vb * denominator;
    let w = vc * denominator;
    std::array::from_fn(|k| a[k] + ab[k] * v + ac[k] * w)
}

#[derive(Clone, Debug)]
struct Node {
    lo: P3,
    hi: P3,
    kind: Kind,
}

#[derive(Clone, Debug)]
enum Kind {
    Leaf { from: u32, to: u32 },
    Split { left: u32, right: u32 },
}

fn box_distance_squared(p: P3, lo: P3, hi: P3) -> f64 {
    (0..3)
        .map(|k| {
            let d = (lo[k] - p[k]).max(0.0).max(p[k] - hi[k]);
            d * d
        })
        .sum()
}

#[derive(Clone, Debug)]
pub struct Surface {
    triangles: Vec<[P3; 3]>,
    normals: Vec<P3>,
    nodes: Vec<Node>,
}

impl Surface {
    pub fn new(triangles: Vec<[P3; 3]>) -> Result<Surface> {
        if triangles.is_empty() {
            bail!("the surface has no triangles");
        }
        let mut order: Vec<u32> = (0..triangles.len() as u32).collect();
        let centres: Vec<P3> = triangles
            .iter()
            .map(|t| std::array::from_fn(|k| (t[0][k] + t[1][k] + t[2][k]) / 3.0))
            .collect();
        let mut nodes = Vec::new();
        let count = order.len();
        grow(&triangles, &centres, &mut order, 0, count, &mut nodes);
        let sorted: Vec<[P3; 3]> = order.iter().map(|&i| triangles[i as usize]).collect();
        let normals = sorted
            .iter()
            .map(|[a, b, c]| {
                let n = cross(sub(*b, *a), sub(*c, *a));
                let length = dot(n, n).sqrt();
                if length > 0.0 { n.map(|v| v / length) } else { [0.0; 3] }
            })
            .collect();
        Ok(Surface {
            triangles: sorted,
            normals,
            nodes,
        })
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    pub fn signed_distance(&self, p: P3) -> f64 {
        let mut best = f64::INFINITY;
        let mut direction = [0.0; 3];
        let mut pseudo = [0.0; 3];
        let mut stack = vec![0u32];
        while let Some(at) = stack.pop() {
            let node = &self.nodes[at as usize];
            if box_distance_squared(p, node.lo, node.hi) > best * (1.0 + TIE) {
                continue;
            }
            match node.kind {
                Kind::Leaf { from, to } => {
                    for i in from as usize..to as usize {
                        let q = closest_on_triangle(p, self.triangles[i]);
                        let d = sub(p, q);
                        let squared = dot(d, d);
                        if squared < best * (1.0 - TIE) {
                            best = squared;
                            direction = d;
                            pseudo = self.normals[i];
                        } else if squared <= best * (1.0 + TIE) {
                            (0..3).for_each(|k| pseudo[k] += self.normals[i][k]);
                        }
                    }
                }
                Kind::Split { left, right } => {
                    let near = |n: u32| {
                        let node = &self.nodes[n as usize];
                        box_distance_squared(p, node.lo, node.hi)
                    };
                    if near(left) < near(right) {
                        stack.push(right);
                        stack.push(left);
                    } else {
                        stack.push(left);
                        stack.push(right);
                    }
                }
            }
        }
        let sign = if dot(direction, pseudo) < 0.0 { -1.0 } else { 1.0 };
        sign * best.sqrt()
    }
}

fn grow(
    triangles: &[[P3; 3]],
    centres: &[P3],
    order: &mut [u32],
    from: usize,
    to: usize,
    nodes: &mut Vec<Node>,
) -> u32 {
    let id = nodes.len() as u32;
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    let (mut clo, mut chi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for &i in &order[from..to] {
        for corner in triangles[i as usize] {
            for k in 0..3 {
                lo[k] = lo[k].min(corner[k]);
                hi[k] = hi[k].max(corner[k]);
            }
        }
        let c = centres[i as usize];
        for k in 0..3 {
            clo[k] = clo[k].min(c[k]);
            chi[k] = chi[k].max(c[k]);
        }
    }
    nodes.push(Node {
        lo,
        hi,
        kind: Kind::Leaf {
            from: from as u32,
            to: to as u32,
        },
    });
    if to - from <= LEAF {
        return id;
    }
    let axis = (0..3)
        .max_by(|&a, &b| (chi[a] - clo[a]).total_cmp(&(chi[b] - clo[b])))
        .unwrap_or(0);
    let middle = (from + to) / 2;
    order[from..to].select_nth_unstable_by(middle - from, |&a, &b| {
        centres[a as usize][axis].total_cmp(&centres[b as usize][axis])
    });
    let left = grow(triangles, centres, order, from, middle, nodes);
    let right = grow(triangles, centres, order, middle, to, nodes);
    nodes[id as usize].kind = Kind::Split { left, right };
    id
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub count: usize,
    pub mean: f64,
    pub std: f64,
    pub min: f64,
    pub max: f64,
    pub rms: f64,
    pub within: f64,
}

pub fn measure(cloud: &Cloud, surface: &Surface) -> Vec<f32> {
    cloud
        .positions
        .iter()
        .map(|&p| surface.signed_distance(p) as f32)
        .collect()
}

pub fn stats(values: &[f32], tolerance: f64) -> Stats {
    let n = values.len().max(1) as f64;
    let mean = values.iter().map(|&v| v as f64).sum::<f64>() / n;
    let variance = values.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n;
    let rms = (values.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / n).sqrt();
    let inside = values.iter().filter(|&&v| (v as f64).abs() <= tolerance).count();
    Stats {
        count: values.len(),
        mean,
        std: variance.sqrt(),
        min: values.iter().fold(f64::INFINITY, |m, &v| m.min(v as f64)),
        max: values.iter().fold(f64::NEG_INFINITY, |m, &v| m.max(v as f64)),
        rms,
        within: 100.0 * inside as f64 / n,
    }
}
