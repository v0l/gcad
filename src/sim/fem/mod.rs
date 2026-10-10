pub mod buckling;
pub mod elastic;
pub mod lobpcg;
pub mod modal;
pub mod scalar;
pub mod solve;
pub mod thermal;

use super::tet::{HashMap, TetMesh};
use monstertruck::modeling::*;

pub const EDGES: [(usize, usize); 6] = [(0, 1), (1, 2), (0, 2), (0, 3), (1, 3), (2, 3)];

pub const STRIDE: usize = 1 << 20;

#[derive(Clone, Copy, Debug)]
pub struct Tie {
    pub slave: u32,
    pub masters: [u32; 6],
    pub weights: [f64; 6],
    pub normal: Option<Vector3>,
    pub friction: f64,
}

impl Tie {
    pub fn group(&self) -> [u32; 7] {
        let m = self.masters;
        [self.slave, m[0], m[1], m[2], m[3], m[4], m[5]]
    }

    pub fn coefficients(&self) -> [f64; 7] {
        let w = self.weights;
        [1.0, -w[0], -w[1], -w[2], -w[3], -w[4], -w[5]]
    }
}

pub struct Quadratic {
    pub nodes: Vec<Point3>,
    pub corners: usize,
    pub tets: Vec<[u32; 10]>,
    pub boundary: Vec<([u32; 6], usize)>,
    pub faces: usize,
    pub body: Vec<u16>,
    pub bodies: Vec<String>,
    pub ties: Vec<Tie>,
    pub bonds: Vec<(usize, usize, usize)>,
}

pub fn shape(l: [f64; 4]) -> [f64; 10] {
    let mut n = [0.0; 10];
    for i in 0..4 {
        n[i] = l[i] * (2.0 * l[i] - 1.0);
    }
    for (k, &(i, j)) in EDGES.iter().enumerate() {
        n[4 + k] = 4.0 * l[i] * l[j];
    }
    n
}

pub fn shape_gradient(l: [f64; 4]) -> [[f64; 3]; 10] {
    let mut dl = [[0.0; 4]; 10];
    for i in 0..4 {
        dl[i][i] = 4.0 * l[i] - 1.0;
    }
    for (k, &(i, j)) in EDGES.iter().enumerate() {
        dl[4 + k][i] = 4.0 * l[j];
        dl[4 + k][j] = 4.0 * l[i];
    }
    dl.map(|d| [d[1] - d[0], d[2] - d[0], d[3] - d[0]])
}

pub const GAUSS: [([f64; 4], f64); 4] = {
    const A: f64 = 0.585_410_196_624_968_5;
    const B: f64 = 0.138_196_601_125_010_5;
    const W: f64 = 1.0 / 24.0;
    [
        ([A, B, B, B], W),
        ([B, A, B, B], W),
        ([B, B, A, B], W),
        ([B, B, B, A], W),
    ]
};

pub const NODE_COORDS: [[f64; 4]; 10] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
    [0.5, 0.5, 0.0, 0.0],
    [0.0, 0.5, 0.5, 0.0],
    [0.5, 0.0, 0.5, 0.0],
    [0.5, 0.0, 0.0, 0.5],
    [0.0, 0.5, 0.0, 0.5],
    [0.0, 0.0, 0.5, 0.5],
];

pub struct Map {
    pub gradients: [Vector3; 10],
    pub det: f64,
}

pub fn map(points: &[Point3; 10], l: [f64; 4]) -> Option<Map> {
    let dn = shape_gradient(l);
    let mut j = Matrix3::zero();
    for (p, d) in points.iter().zip(&dn) {
        for col in 0..3 {
            j[col] += p.to_vec() * d[col];
        }
    }
    let det = j.determinant();
    if det <= 0.0 {
        return None;
    }
    let inverse = j.invert()?;
    let inverse_t = inverse.transpose();
    let gradients = dn.map(|d| inverse_t * Vector3::new(d[0], d[1], d[2]));
    Some(Map { gradients, det })
}

fn edge_key(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

impl Quadratic {
    pub fn coarse(&self) -> solve::Coarse {
        let mut index: Vec<Option<u32>> = vec![None; self.nodes.len()];
        let mut count = 0u32;
        for tet in &self.tets {
            for &v in &tet[..4] {
                if index[v as usize].is_none() {
                    index[v as usize] = Some(count);
                    count += 1;
                }
            }
        }
        let mut parents = vec![[(0u32, 0.0), (0u32, 0.0)]; self.nodes.len()];
        for tet in &self.tets {
            for (k, &v) in tet.iter().enumerate() {
                if k < 4 {
                    parents[v as usize] = [(index[v as usize].unwrap_or(0), 1.0), (0, 0.0)];
                } else {
                    let (i, j) = EDGES[k - 4];
                    let (a, b) = (tet[i] as usize, tet[j] as usize);
                    parents[v as usize] =
                        [(index[a].unwrap_or(0), 0.5), (index[b].unwrap_or(0), 0.5)];
                }
            }
        }
        solve::Coarse {
            parents,
            nodes: count as usize,
            clusters: self.clusters(),
        }
    }

    fn clusters(&self) -> Vec<Vec<u32>> {
        const LARGEST: usize = 8;
        let mut parent: Vec<u32> = (0..self.nodes.len() as u32).collect();
        let mut size = vec![1usize; self.nodes.len()];
        fn root(parent: &mut [u32], mut v: u32) -> u32 {
            while parent[v as usize] != v {
                parent[v as usize] = parent[parent[v as usize] as usize];
                v = parent[v as usize];
            }
            v
        }
        for tie in &self.ties {
            let (k, _) =
                tie.weights
                    .iter()
                    .enumerate()
                    .fold(
                        (0, f64::NEG_INFINITY),
                        |a, (k, &w)| if w > a.1 { (k, w) } else { a },
                    );
            let (a, b) = (
                root(&mut parent, tie.slave),
                root(&mut parent, tie.masters[k]),
            );
            if a != b && size[a as usize] + size[b as usize] <= LARGEST {
                parent[b as usize] = a;
                size[a as usize] += size[b as usize];
            }
        }
        let mut groups: HashMap<u32, Vec<u32>> = HashMap::default();
        for v in 0..self.nodes.len() as u32 {
            let r = root(&mut parent, v);
            if size[r as usize] > 1 {
                groups.entry(r).or_default().push(v);
            }
        }
        let mut out: Vec<Vec<u32>> = groups.into_values().collect();
        out.sort_unstable();
        out
    }

    pub fn points(&self, t: usize) -> [Point3; 10] {
        self.tets[t].map(|v| self.nodes[v as usize])
    }

    pub fn element_ok(&self, t: usize) -> bool {
        let points = self.points(t);
        GAUSS
            .iter()
            .map(|(l, _)| *l)
            .chain(NODE_COORDS.iter().take(4).copied())
            .chain(std::iter::once([0.25; 4]))
            .all(|l| map(&points, l).is_some())
    }

    pub fn from(mesh: &TetMesh) -> Quadratic {
        let mut nodes = mesh.nodes.clone();
        let corners = nodes.len();
        let mut mids: HashMap<(u32, u32), u32> = HashMap::default();
        let mut tets = Vec::with_capacity(mesh.tets.len());
        for raw in &mesh.tets {
            let tet = [raw[1], raw[0], raw[2], raw[3]];
            let mut q = [0u32; 10];
            q[..4].copy_from_slice(&tet);
            for (k, &(i, j)) in EDGES.iter().enumerate() {
                let key = edge_key(tet[i], tet[j]);
                q[4 + k] = *mids.entry(key).or_insert_with(|| {
                    let (a, b) = (mesh.nodes[key.0 as usize], mesh.nodes[key.1 as usize]);
                    nodes.push(a.midpoint(b));
                    (nodes.len() - 1) as u32
                });
            }
            tets.push(q);
        }
        let mut owners: HashMap<(u32, u32), Vec<usize>> = HashMap::default();
        for (f, tag) in &mesh.boundary {
            for k in 0..3 {
                owners
                    .entry(edge_key(f[k], f[(k + 1) % 3]))
                    .or_default()
                    .push(*tag);
            }
        }
        let mut curved: Vec<(u32, Point3)> = Vec::new();
        for (&(a, b), tags) in &owners {
            let chord = mesh.nodes[a as usize].midpoint(mesh.nodes[b as usize]);
            let length = (mesh.nodes[a as usize] - mesh.nodes[b as usize]).magnitude();
            let mut tags = tags.clone();
            tags.sort_unstable();
            tags.dedup();
            let best = tags
                .iter()
                .filter_map(|&tag| {
                    let uv = |v: u32| {
                        mesh.uvs
                            .get(&v)?
                            .iter()
                            .find(|(f, _)| *f == tag)
                            .map(|(_, uv)| *uv)
                    };
                    let (ua, ub) = (uv(a)?, uv(b)?);
                    let mid = (ua + ub) / 2.0;
                    let p = mesh.surfaces[tag].surface().subs(mid.x, mid.y);
                    let off = (p - chord).magnitude();
                    (off < length * 0.25).then_some((off, p))
                })
                .max_by(|x, y| x.0.total_cmp(&y.0));
            if let Some((off, p)) = best
                && off > length * 1.0e-9
            {
                curved.push((mids[&(a, b)], p));
            }
        }
        for (v, p) in &curved {
            nodes[*v as usize] = *p;
        }
        let boundary = mesh
            .boundary
            .iter()
            .map(|(f, tag)| {
                let m = |i: usize, j: usize| mids[&edge_key(f[i], f[j])];
                ([f[0], f[1], f[2], m(0, 1), m(1, 2), m(2, 0)], *tag)
            })
            .collect();
        let body = vec![0; tets.len()];
        let mut quadratic = Quadratic {
            nodes,
            corners,
            tets,
            boundary,
            faces: mesh.faces,
            body,
            bodies: Vec::new(),
            ties: Vec::new(),
            bonds: Vec::new(),
        };
        quadratic.straighten_bad(mesh);
        quadratic
    }

    fn straighten_bad(&mut self, mesh: &TetMesh) {
        for _ in 0..4 {
            let bad: Vec<usize> = (0..self.tets.len())
                .filter(|&t| !self.element_ok(t))
                .collect();
            if bad.is_empty() {
                return;
            }
            for t in bad {
                let tet = self.tets[t];
                for (k, &(i, j)) in EDGES.iter().enumerate() {
                    let (a, b) = (tet[i] as usize, tet[j] as usize);
                    self.nodes[tet[4 + k] as usize] = mesh.nodes[a].midpoint(mesh.nodes[b]);
                }
            }
        }
    }

    pub fn volume(&self) -> f64 {
        (0..self.tets.len())
            .map(|t| {
                let p = self.points(t);
                GAUSS
                    .iter()
                    .filter_map(|(l, w)| map(&p, *l).map(|m| m.det * w))
                    .sum::<f64>()
            })
            .sum()
    }
}

pub fn triangle_gauss() -> [([f64; 3], f64); 3] {
    [
        ([2.0 / 3.0, 1.0 / 6.0, 1.0 / 6.0], 1.0 / 6.0),
        ([1.0 / 6.0, 2.0 / 3.0, 1.0 / 6.0], 1.0 / 6.0),
        ([1.0 / 6.0, 1.0 / 6.0, 2.0 / 3.0], 1.0 / 6.0),
    ]
}

pub fn triangle_shape(l: [f64; 3]) -> [f64; 6] {
    [
        l[0] * (2.0 * l[0] - 1.0),
        l[1] * (2.0 * l[1] - 1.0),
        l[2] * (2.0 * l[2] - 1.0),
        4.0 * l[0] * l[1],
        4.0 * l[1] * l[2],
        4.0 * l[2] * l[0],
    ]
}

pub fn triangle_area_vector(points: &[Point3; 6], l: [f64; 3]) -> Vector3 {
    let d = [
        [4.0 * l[0] - 1.0, 0.0, 0.0, 4.0 * l[1], 0.0, 4.0 * l[2]],
        [0.0, 4.0 * l[1] - 1.0, 0.0, 4.0 * l[0], 4.0 * l[2], 0.0],
        [0.0, 0.0, 4.0 * l[2] - 1.0, 0.0, 4.0 * l[1], 4.0 * l[0]],
    ];
    let mut du = Vector3::zero();
    let mut dv = Vector3::zero();
    for k in 0..6 {
        let p = points[k].to_vec();
        du += p * (d[1][k] - d[0][k]);
        dv += p * (d[2][k] - d[0][k]);
    }
    du.cross(dv)
}

pub fn closest_on_triangle(p: Point3, [a, b, c]: [Point3; 3]) -> (f64, [f64; 3]) {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return ((p - a).magnitude(), [1.0, 0.0, 0.0]);
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return ((p - b).magnitude(), [0.0, 1.0, 0.0]);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return ((p - (a + ab * v)).magnitude(), [1.0 - v, v, 0.0]);
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return ((p - c).magnitude(), [0.0, 0.0, 1.0]);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return ((p - (a + ac * w)).magnitude(), [1.0 - w, 0.0, w]);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return ((p - (b + (c - b) * w)).magnitude(), [0.0, 1.0 - w, w]);
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    ((p - (a + ab * v + ac * w)).magnitude(), [1.0 - v - w, v, w])
}

pub fn merge(
    parts: Vec<(String, Quadratic)>,
    gap: f64,
    touching: &[(usize, usize, f64)],
) -> Quadratic {
    let mut out = Quadratic {
        nodes: Vec::new(),
        corners: 0,
        tets: Vec::new(),
        boundary: Vec::new(),
        faces: 0,
        body: Vec::new(),
        bodies: Vec::new(),
        ties: Vec::new(),
        bonds: Vec::new(),
    };
    let mut skins: Vec<Vec<[u32; 6]>> = Vec::new();
    for (index, (name, part)) in parts.into_iter().enumerate() {
        let offset = out.nodes.len() as u32;
        out.nodes.extend(part.nodes);
        out.corners += part.corners;
        out.tets
            .extend(part.tets.iter().map(|t| t.map(|v| v + offset)));
        out.body
            .extend(std::iter::repeat_n(index as u16, part.tets.len()));
        let skin: Vec<[u32; 6]> = part
            .boundary
            .iter()
            .map(|(f, _)| f.map(|v| v + offset))
            .collect();
        out.boundary.extend(
            part.boundary
                .iter()
                .map(|(f, tag)| (f.map(|v| v + offset), index * STRIDE + tag)),
        );
        out.faces = out.faces.max(index * STRIDE + part.faces);
        out.bodies.push(name);
        skins.push(skin);
    }
    for master in 0..skins.len() {
        let cell = gap.max(1.0e-9) * 8.0;
        let key = |p: Point3| [p.x, p.y, p.z].map(|v| (v / cell).floor() as i64);
        let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::default();
        for (t, f) in skins[master].iter().enumerate() {
            let corners = [f[0], f[1], f[2]].map(|v| out.nodes[v as usize]);
            let lo = corners.iter().fold(corners[0], |m, p| {
                Point3::new(m.x.min(p.x), m.y.min(p.y), m.z.min(p.z))
            });
            let hi = corners.iter().fold(corners[0], |m, p| {
                Point3::new(m.x.max(p.x), m.y.max(p.y), m.z.max(p.z))
            });
            let (a, b) = (
                key(lo - Vector3::new(gap, gap, gap)),
                key(hi + Vector3::new(gap, gap, gap)),
            );
            for x in a[0]..=b[0] {
                for y in a[1]..=b[1] {
                    for z in a[2]..=b[2] {
                        grid.entry([x, y, z]).or_default().push(t);
                    }
                }
            }
        }
        for slave in (0..skins.len()).filter(|&s| s != master) {
            let pair = (master.min(slave), master.max(slave));
            let contact = touching.iter().find(|t| (t.0, t.1) == pair);
            let mut facing: HashMap<u32, Vec<Vector3>> = HashMap::default();
            for f in &skins[slave] {
                let [a, b, c] = [f[0], f[1], f[2]].map(|v| out.nodes[v as usize]);
                let n = (b - a).cross(c - a);
                if n.magnitude() > 0.0 {
                    for &v in f {
                        facing.entry(v).or_default().push(n.normalize());
                    }
                }
            }
            let mut seen = std::collections::BTreeSet::new();
            let mut tied = 0;
            for f in &skins[slave] {
                for &v in f {
                    if !seen.insert(v) {
                        continue;
                    }
                    let p = out.nodes[v as usize];
                    let Some(list) = grid.get(&key(p)) else {
                        continue;
                    };
                    let best = list
                        .iter()
                        .filter(|&&t| {
                            let m = skins[master][t];
                            let [a, b, c] = [m[0], m[1], m[2]].map(|n| out.nodes[n as usize]);
                            let n = (b - a).cross(c - a);
                            facing.get(&v).is_some_and(|list| {
                                list.iter().any(|s| s.dot(n) < -0.7 * n.magnitude())
                            })
                        })
                        .map(|&t| {
                            let m = skins[master][t];
                            let corners = [m[0], m[1], m[2]].map(|n| out.nodes[n as usize]);
                            let (d, l) = closest_on_triangle(p, corners);
                            (d, t, l)
                        })
                        .min_by(|a, b| a.0.total_cmp(&b.0));
                    if let Some((d, t, l)) = best
                        && d <= gap
                    {
                        let shape = triangle_shape(l);
                        let m = skins[master][t];
                        let [a, b, c] = [m[0], m[1], m[2]].map(|n| out.nodes[n as usize]);
                        out.ties.push(Tie {
                            slave: v,
                            masters: m,
                            weights: shape,
                            normal: contact.map(|_| (b - a).cross(c - a).normalize()),
                            friction: contact.map_or(0.0, |t| t.2),
                        });
                        tied += 1;
                    }
                }
            }
            if tied > 0 && master < slave {
                out.bonds.push((master, slave, tied));
            }
        }
    }
    out
}
