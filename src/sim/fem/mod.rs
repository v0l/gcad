pub mod elastic;
pub mod solve;

use super::tet::{HashMap, TetMesh};
use monstertruck::modeling::*;

pub const EDGES: [(usize, usize); 6] = [(0, 1), (1, 2), (0, 2), (0, 3), (1, 3), (2, 3)];

pub struct Quadratic {
    pub nodes: Vec<Point3>,
    pub corners: usize,
    pub tets: Vec<[u32; 10]>,
    pub boundary: Vec<([u32; 6], usize)>,
    pub faces: usize,
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
        let mut quadratic = Quadratic {
            nodes,
            corners,
            tets,
            boundary,
            faces: mesh.faces,
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
