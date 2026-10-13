use super::solve::{Block, Blocks, Factored, IDENTITY};

const DIRECT_LIMIT: usize = 100_000;
pub const DIRECT_FALLBACK: usize = 600_000;
pub const TIE: f64 = 10.0;
use super::{
    GAUSS, NODE_COORDS, Quadratic, map, shape, triangle_area_vector, triangle_gauss, triangle_shape,
};
use anyhow::{Result, bail};
use monstertruck::modeling::*;
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hold {
    Fixed,
    Slide,
    Pin {
        point: Point3,
        axis: Vector3,
    },
    Bearing {
        point: Point3,
        axis: Vector3,
        axial: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Support {
    pub label: String,
    pub faces: Vec<usize>,
    pub hold: Hold,
}

#[derive(Clone, Copy, Debug)]
pub enum Push {
    Force(Vector3),
    Pressure(f64),
    Torque {
        point: Point3,
        axis: Vector3,
        moment: f64,
    },
}

fn off_axis(p: Point3, point: Point3, axis: Vector3) -> Vector3 {
    let r = p - point;
    r - axis * r.dot(axis)
}

fn polar(mesh: &Quadratic, faces: &[usize], point: Point3, axis: Vector3) -> f64 {
    faces_of(mesh, faces)
        .map(|f| {
            let p = six(mesh, f);
            triangle_gauss()
                .iter()
                .map(|(l, w)| {
                    let shape = triangle_shape(*l);
                    let at = Point3::from_vec(
                        p.iter()
                            .zip(shape)
                            .map(|(q, n)| q.to_vec() * n)
                            .sum::<Vector3>(),
                    );
                    off_axis(at, point, axis).magnitude2()
                        * triangle_area_vector(&p, *l).magnitude()
                        * w
                })
                .sum::<f64>()
        })
        .sum()
}

#[derive(Clone, Debug)]
pub struct Load {
    pub label: String,
    pub faces: Vec<usize>,
    pub push: Push,
}

pub struct SurfacePressure {
    reach: f64,
    buckets: std::collections::HashMap<[i64; 3], Vec<(Point3, f64)>>,
}

impl SurfacePressure {
    pub fn new(samples: impl IntoIterator<Item = (Point3, f64)>, reach: f64) -> SurfacePressure {
        let mut buckets: std::collections::HashMap<[i64; 3], Vec<(Point3, f64)>> =
            Default::default();
        for (p, v) in samples {
            buckets
                .entry([p.x, p.y, p.z].map(|c| (c / reach).floor() as i64))
                .or_default()
                .push((p, v));
        }
        SurfacePressure { reach, buckets }
    }

    pub fn at(&self, p: Point3) -> Option<f64> {
        let base = [p.x, p.y, p.z].map(|c| (c / self.reach).floor() as i64);
        let mut best = (self.reach * self.reach, None);
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let key = [base[0] + dx, base[1] + dy, base[2] + dz];
                    for (q, v) in self.buckets.get(&key).into_iter().flatten() {
                        let d = (q - p).magnitude2();
                        if d < best.0 {
                            best = (d, Some(*v));
                        }
                    }
                }
            }
        }
        best.1
    }
}

pub struct Problem<'a> {
    pub mesh: &'a Quadratic,
    pub materials: &'a [(f64, f64)],
    pub supports: &'a [Support],
    pub loads: &'a [Load],
    pub body_force: Option<Vec<Vector3>>,
    pub expansion: Option<Vec<f64>>,
    pub wind: Option<&'a SurfacePressure>,
}

pub struct Answer {
    pub displacement: Vec<Vector3>,
    pub von_mises: Vec<f64>,
    pub reactions: Vec<(String, Vector3, Vector3)>,
    pub scale: f64,
    pub contact: Option<(usize, usize, usize)>,
    pub iterations: usize,
    pub residual: f64,
    pub applied: Vector3,
}

pub fn lame(young: f64, poisson: f64) -> (f64, f64) {
    let lambda = young * poisson / ((1.0 + poisson) * (1.0 - 2.0 * poisson));
    let mu = young / (2.0 * (1.0 + poisson));
    (lambda, mu)
}

fn element_matrix(points: &[Point3; 10], lambda: f64, mu: f64) -> Option<Vec<Block>> {
    let mut k = vec![[0.0; 9]; 100];
    for (l, w) in GAUSS {
        let m = map(points, l)?;
        let weight = m.det * w;
        let g = &m.gradients;
        for a in 0..10 {
            for b in 0..10 {
                let block = &mut k[a * 10 + b];
                let dot = g[a].dot(g[b]) * mu;
                for i in 0..3 {
                    for j in 0..3 {
                        let mut v = lambda * g[a][i] * g[b][j] + mu * g[a][j] * g[b][i];
                        if i == j {
                            v += dot;
                        }
                        block[i * 3 + j] += v * weight;
                    }
                }
            }
        }
    }
    Some(k)
}

pub fn spacing(mesh: &Quadratic) -> f64 {
    (mesh.volume() / mesh.tets.len().max(1) as f64 * 6.0).cbrt()
}

pub fn assemble(mesh: &Quadratic, materials: &[(f64, f64)]) -> Result<Blocks> {
    assemble_with(mesh, materials, false)
}

pub fn assemble_bonded(mesh: &Quadratic, materials: &[(f64, f64)]) -> Result<Blocks> {
    assemble_with(mesh, materials, true)
}

fn assemble_with(mesh: &Quadratic, materials: &[(f64, f64)], all: bool) -> Result<Blocks> {
    let constants: Vec<(f64, f64)> = materials.iter().map(|&(e, nu)| lame(e, nu)).collect();
    let groups: Vec<[u32; 7]> = mesh.ties.iter().map(|t| t.group()).collect();
    let mut blocks = Blocks::pattern_with(mesh.nodes.len(), &mesh.tets, &groups);
    let flat = spacing(mesh).powi(3) * 1.0e-6;
    let batch = 4096;
    let stage = crate::progress::stage(
        "assembling the stiffness",
        Some(mesh.tets.len()),
        "elements",
    );
    for start in (0..mesh.tets.len()).step_by(batch) {
        let end = (start + batch).min(mesh.tets.len());
        stage.set(start as f64);
        let matrices: Vec<Option<Vec<Block>>> = (start..end)
            .into_par_iter()
            .map(|t| {
                let (lambda, mu) = constants[mesh.body[t] as usize];
                element_matrix(&mesh.points(t), lambda, mu)
            })
            .collect();
        for (t, matrix) in (start..end).zip(matrices) {
            let Some(matrix) = matrix else {
                let p = mesh.points(t);
                let volume = (p[1] - p[0]).dot((p[2] - p[0]).cross(p[3] - p[0])) / 6.0;
                if volume.abs() <= flat {
                    continue;
                }
                let centre = (p[0].to_vec() + p[1].to_vec() + p[2].to_vec() + p[3].to_vec()) / 4.0;
                let body = mesh
                    .bodies
                    .get(mesh.body[t] as usize)
                    .map_or(String::new(), |b| format!(" in `{b}`"));
                bail!(
                    "an element{body} near {:.2},{:.2},{:.2} is inverted; try another `mesh` size",
                    centre.x,
                    centre.y,
                    centre.z
                );
            };
            let tet = mesh.tets[t];
            for a in 0..10 {
                for b in 0..10 {
                    let at = blocks.find(tet[a], tet[b]);
                    let target = &mut blocks.values[at];
                    for (x, y) in target.iter_mut().zip(&matrix[a * 10 + b]) {
                        *x += y;
                    }
                }
            }
        }
    }
    if !mesh.ties.is_empty() {
        let stiffest = materials.iter().map(|m| m.0).fold(0.0, f64::max);
        let penalty = stiffest * spacing(mesh) * TIE;
        for tie in mesh.ties.iter().filter(|t| all || t.normal.is_none()) {
            let middle = tie
                .along
                .map_or(IDENTITY, |n| std::array::from_fn(|k| n[k / 3] * n[k % 3]));
            add_tie(&mut blocks, tie, tie.spring.unwrap_or(penalty), &middle);
        }
    }
    stage.set(mesh.tets.len() as f64);
    Ok(blocks)
}

pub fn contact_penalty(mesh: &Quadratic, materials: &[(f64, f64)]) -> f64 {
    materials.iter().map(|m| m.0).fold(0.0, f64::max) * spacing(mesh) * TIE
}

fn add_contacts(blocks: &mut Blocks, mesh: &Quadratic, penalty: f64, touches: &[Touch]) {
    for (tie, t) in mesh.ties.iter().zip(touches) {
        let Some(n) = tie.normal else {
            continue;
        };
        if !t.closed && !t.sticking && !t.slipping {
            continue;
        }
        let along = |i: usize, j: usize| {
            let identity = if i == j { 1.0 } else { 0.0 };
            let d = t.along;
            let pressing = if t.closed { n[i] * n[j] } else { 0.0 };
            let sideways = if t.sticking {
                identity - n[i] * n[j]
            } else if t.slipping {
                t.ratio * (identity - n[i] * n[j] - d[i] * d[j]) + SLIDE * d[i] * d[j]
            } else {
                0.0
            };
            pressing + sideways
        };
        let middle: Block = std::array::from_fn(|k| along(k / 3, k % 3));
        add_tie(blocks, tie, penalty, &middle);
    }
}

fn add_tie(blocks: &mut Blocks, tie: &super::Tie, penalty: f64, middle: &Block) {
    let nodes = tie.group();
    let matrices = tie.matrices();
    let left: Vec<Block> = matrices
        .iter()
        .map(|m| super::product(&super::transposed(m), middle))
        .collect();
    for a in 0..7 {
        for b in 0..7 {
            let at = blocks.find(nodes[a], nodes[b]);
            let term = super::product(&left[a], &matrices[b]);
            for (x, y) in blocks.values[at].iter_mut().zip(term) {
                *x += penalty * y;
            }
        }
    }
}

const SLIDE: f64 = 1.0e-8;

fn gap(tie: &super::Tie, u: &[f64]) -> Vector3 {
    tie.gap(u)
}

#[derive(Clone)]
struct Touch {
    closed: bool,
    sticking: bool,
    slipping: bool,
    pull: Vector3,
    along: Vector3,
    ratio: f64,
}

fn touch(tie: &super::Tie, u: &[f64], scale: f64, limit: f64) -> (Touch, f64) {
    let mut t = Touch {
        closed: false,
        sticking: false,
        slipping: false,
        pull: Vector3::zero(),
        along: Vector3::zero(),
        ratio: 0.0,
    };
    let Some(n) = tie.normal else {
        return (t, 0.0);
    };
    let g = gap(tie, u);
    let normal = g.dot(n);
    let mut stored = 0.0;
    if normal <= scale * 1.0e-9 {
        t.closed = true;
        t.pull = n * normal;
        stored += 0.5 * normal * normal;
    }
    if tie.friction == 0.0 || limit <= 0.0 {
        return (t, stored);
    }
    let sideways = g - n * normal;
    let length = sideways.magnitude();
    if length <= limit {
        t.sticking = true;
        t.pull += sideways;
        stored += 0.5 * length * length;
    } else {
        t.slipping = true;
        t.pull += sideways * (limit / length);
        t.along = sideways / length;
        t.ratio = limit / length;
        stored += limit * (length - 0.5 * limit);
    }
    (t, stored)
}

fn add_pulls(out: &mut [f64], mesh: &Quadratic, penalty: f64, touches: &[Touch]) {
    for (tie, t) in mesh.ties.iter().zip(touches) {
        for (&v, m) in tie.group().iter().zip(tie.matrices()) {
            let i = 3 * v as usize;
            let p = t.pull;
            out[i] += penalty * (m[0] * p.x + m[3] * p.y + m[6] * p.z);
            out[i + 1] += penalty * (m[1] * p.x + m[4] * p.y + m[7] * p.z);
            out[i + 2] += penalty * (m[2] * p.x + m[5] * p.y + m[8] * p.z);
        }
    }
}

fn faces_of<'a>(mesh: &'a Quadratic, faces: &'a [usize]) -> impl Iterator<Item = [u32; 6]> + 'a {
    mesh.boundary
        .iter()
        .filter(move |(_, tag)| faces.contains(tag))
        .map(|(f, _)| *f)
}

fn six(mesh: &Quadratic, f: [u32; 6]) -> [Point3; 6] {
    f.map(|v| mesh.nodes[v as usize])
}

pub fn area(mesh: &Quadratic, faces: &[usize]) -> f64 {
    faces_of(mesh, faces)
        .map(|f| {
            let p = six(mesh, f);
            triangle_gauss()
                .iter()
                .map(|(l, w)| triangle_area_vector(&p, *l).magnitude() * w)
                .sum::<f64>()
        })
        .sum()
}

pub fn wind_force(mesh: &Quadratic, wind: &SurfacePressure) -> Vector3 {
    let mut total = Vector3::zero();
    for (face, _) in &mesh.boundary {
        let p = six(mesh, *face);
        let middle = Point3::from_vec((p[0].to_vec() + p[1].to_vec() + p[2].to_vec()) / 3.0);
        if let Some(pressure) = wind.at(middle) {
            for (l, w) in triangle_gauss() {
                total -= triangle_area_vector(&p, l) * (pressure * w);
            }
        }
    }
    total
}

pub fn loads(problem: &Problem) -> Result<Vec<f64>> {
    let mesh = problem.mesh;
    let mut f = vec![0.0; mesh.nodes.len() * 3];
    let mut add = |node: u32, v: Vector3| {
        let i = node as usize * 3;
        f[i] += v.x;
        f[i + 1] += v.y;
        f[i + 2] += v.z;
    };
    for load in problem.loads {
        let total = area(mesh, &load.faces);
        if total <= 0.0 {
            bail!("a load's faces have no area");
        }
        let polar_moment = match load.push {
            Push::Torque { point, axis, .. } => {
                let j = polar(mesh, &load.faces, point, axis);
                if j <= 0.0 {
                    bail!("the torque's faces all lie on its axis");
                }
                j
            }
            _ => 1.0,
        };
        for face in faces_of(mesh, &load.faces) {
            let p = six(mesh, face);
            for (l, w) in triangle_gauss() {
                let normal = triangle_area_vector(&p, l);
                let traction = match load.push {
                    Push::Force(force) => force * (normal.magnitude() / total),
                    Push::Pressure(pressure) => -normal * pressure,
                    Push::Torque {
                        point,
                        axis,
                        moment,
                    } => {
                        let at = Point3::from_vec(
                            p.iter()
                                .zip(triangle_shape(l))
                                .map(|(q, n)| q.to_vec() * n)
                                .sum::<Vector3>(),
                        );
                        axis.cross(off_axis(at, point, axis))
                            * (moment / polar_moment * normal.magnitude())
                    }
                };
                for (k, n) in triangle_shape(l).iter().enumerate() {
                    add(face[k], traction * (n * w));
                }
            }
        }
    }
    if let Some(wind) = problem.wind {
        for (face, _) in &mesh.boundary {
            let p = six(mesh, *face);
            let middle = Point3::from_vec((p[0].to_vec() + p[1].to_vec() + p[2].to_vec()) / 3.0);
            let Some(pressure) = wind.at(middle) else {
                continue;
            };
            for (l, w) in triangle_gauss() {
                let traction = -triangle_area_vector(&p, l) * pressure;
                for (k, n) in triangle_shape(l).iter().enumerate() {
                    add(face[k], traction * (n * w));
                }
            }
        }
    }
    if let Some(strains) = &problem.expansion {
        for t in 0..mesh.tets.len() {
            let body = mesh.body[t] as usize;
            let (lambda, mu) = lame(problem.materials[body].0, problem.materials[body].1);
            let stiffness = 3.0 * lambda + 2.0 * mu;
            let points = mesh.points(t);
            for (l, w) in GAUSS {
                let free: f64 = shape(l)
                    .iter()
                    .zip(mesh.tets[t])
                    .map(|(n, v)| n * strains[v as usize])
                    .sum();
                let push = stiffness * free;
                if let Some(m) = map(&points, l) {
                    for (k, g) in m.gradients.iter().enumerate() {
                        add(mesh.tets[t][k], g * (push * m.det * w));
                    }
                }
            }
        }
    }
    if let Some(forces) = &problem.body_force {
        for t in 0..mesh.tets.len() {
            let b = forces[mesh.body[t] as usize];
            let points = mesh.points(t);
            for (l, w) in GAUSS {
                if let Some(m) = map(&points, l) {
                    for (k, n) in shape(l).iter().enumerate() {
                        add(mesh.tets[t][k], b * (n * m.det * w));
                    }
                }
            }
        }
    }
    Ok(f)
}

pub fn projectors(mesh: &Quadratic, supports: &[Support]) -> (Vec<Block>, Vec<Option<usize>>) {
    let mut normals: Vec<Vec<Vector3>> = vec![Vec::new(); mesh.nodes.len()];
    let mut fixed = vec![false; mesh.nodes.len()];
    let mut owner: Vec<Option<usize>> = vec![None; mesh.nodes.len()];
    for (s, support) in supports.iter().enumerate() {
        if let Hold::Bearing { point, axis, axial } = support.hold {
            let ring = bearing_ring(mesh, &support.faces, point, axis);
            for &node in &ring {
                owner[node as usize].get_or_insert(s);
                let r = off_axis(mesh.nodes[node as usize], point, axis);
                if r.magnitude() > 0.0 {
                    normals[node as usize].push(r.normalize());
                }
            }
            if let Some(&node) = ring.first().filter(|_| axial) {
                normals[node as usize].push(axis);
            }
            continue;
        }
        for face in faces_of(mesh, &support.faces) {
            let p = six(mesh, face);
            for (k, &node) in face.iter().enumerate() {
                owner[node as usize].get_or_insert(s);
                match support.hold {
                    Hold::Fixed => fixed[node as usize] = true,
                    Hold::Bearing { .. } => {}
                    Hold::Pin { point, axis } => {
                        normals[node as usize].push(axis);
                        let r = off_axis(mesh.nodes[node as usize], point, axis);
                        if r.magnitude() > 0.0 {
                            normals[node as usize].push(r.normalize());
                        }
                    }
                    Hold::Slide => {
                        let l = match k {
                            0 => [1.0, 0.0, 0.0],
                            1 => [0.0, 1.0, 0.0],
                            2 => [0.0, 0.0, 1.0],
                            3 => [0.5, 0.5, 0.0],
                            4 => [0.0, 0.5, 0.5],
                            _ => [0.5, 0.0, 0.5],
                        };
                        let n = triangle_area_vector(&p, l);
                        if n.magnitude() > 0.0 {
                            normals[node as usize].push(n.normalize());
                        }
                    }
                }
            }
        }
    }
    let blocks = (0..mesh.nodes.len())
        .map(|v| {
            if fixed[v] {
                return [0.0; 9];
            }
            let mut basis: Vec<Vector3> = Vec::new();
            for n in &normals[v] {
                let mut r = *n;
                for b in &basis {
                    r -= *b * r.dot(*b);
                }
                if r.magnitude() > 0.05 {
                    basis.push(r.normalize());
                }
            }
            let mut p = IDENTITY;
            for b in basis {
                for i in 0..3 {
                    for j in 0..3 {
                        p[i * 3 + j] -= b[i] * b[j];
                    }
                }
            }
            p
        })
        .collect();
    (blocks, owner)
}

fn bearing_ring(mesh: &Quadratic, faces: &[usize], point: Point3, axis: Vector3) -> Vec<u32> {
    let mut nodes: Vec<u32> = faces_of(mesh, faces).flatten().collect();
    nodes.sort_unstable();
    nodes.dedup();
    if nodes.is_empty() {
        return Vec::new();
    }
    let along = |v: u32| (mesh.nodes[v as usize] - point).dot(axis);
    let radius = nodes
        .iter()
        .map(|&v| off_axis(mesh.nodes[v as usize], point, axis).magnitude())
        .fold(0.0, f64::max)
        .max(1.0e-9);
    let step = radius * 1.0e-6;
    let mut stations: std::collections::BTreeMap<i64, Vec<u32>> = Default::default();
    for &v in &nodes {
        stations
            .entry((along(v) / step).round() as i64)
            .or_default()
            .push(v);
    }
    let body = mesh
        .tets
        .iter()
        .zip(&mesh.body)
        .find_map(|(tet, &b)| tet.contains(&nodes[0]).then_some(b));
    let (sum, count) = mesh
        .tets
        .iter()
        .zip(&mesh.body)
        .filter(|(_, b)| Some(**b) == body)
        .flat_map(|(tet, _)| &tet[..4])
        .fold((0.0, 0usize), |(s, n), &v| (s + along(v), n + 1));
    let middle = sum / count.max(1) as f64;
    let most = stations.values().map(Vec::len).max().unwrap_or(0);
    stations
        .into_iter()
        .filter(|(_, ring)| 2 * ring.len() >= most)
        .min_by(|a, b| {
            (a.0 as f64 * step - middle)
                .abs()
                .total_cmp(&(b.0 as f64 * step - middle).abs())
        })
        .map(|(_, ring)| ring)
        .unwrap_or_default()
}

fn loose(mesh: &Quadratic, projectors: &[Block]) -> bool {
    let n = mesh.nodes.len().max(1) as f64;
    let centre = mesh
        .nodes
        .iter()
        .fold(Vector3::zero(), |a, p| a + p.to_vec())
        / n;
    let size = mesh
        .nodes
        .iter()
        .map(|p| (p.to_vec() - centre).magnitude())
        .fold(1.0e-9, f64::max);
    let mut gram = vec![vec![0.0; 6]; 6];
    for (p, block) in mesh.nodes.iter().zip(projectors) {
        let r = (p.to_vec() - centre) / size;
        let modes = [
            Vector3::unit_x(),
            Vector3::unit_y(),
            Vector3::unit_z(),
            Vector3::unit_x().cross(r),
            Vector3::unit_y().cross(r),
            Vector3::unit_z().cross(r),
        ];
        let held: Vec<Vector3> = modes
            .iter()
            .map(|m| {
                let kept = Vector3::new(
                    block[0] * m.x + block[1] * m.y + block[2] * m.z,
                    block[3] * m.x + block[4] * m.y + block[5] * m.z,
                    block[6] * m.x + block[7] * m.y + block[8] * m.z,
                );
                *m - kept
            })
            .collect();
        for i in 0..6 {
            for j in 0..6 {
                gram[i][j] += held[i].dot(held[j]);
            }
        }
    }
    let (values, _) = super::modal::jacobi_eigen(&mut gram);
    let top = values.iter().copied().fold(0.0, f64::max);
    values.iter().any(|v| *v <= top * 1.0e-9)
}

pub fn solve(problem: &Problem) -> Result<Answer> {
    let mesh = problem.mesh;
    if problem.supports.is_empty() {
        bail!("add a support, like `fix <X`, or the part is free to fly away");
    }
    let stiffness = assemble(mesh, problem.materials)?;
    let f = loads(problem)?;
    let (projectors, owner) = projectors(mesh, problem.supports);
    if loose(mesh, &projectors) {
        bail!("the supports leave the part free to move or turn; add one that holds it");
    }
    let penalty = contact_penalty(mesh, problem.materials);
    let contacts = mesh.ties.iter().any(|t| t.normal.is_some());
    let friction = mesh.ties.iter().any(|t| t.friction > 0.0);
    let coarse = mesh.coarse();
    let largest = if friction {
        DIRECT_FALLBACK
    } else {
        DIRECT_LIMIT
    };
    let project = |r: &mut [f64]| {
        for (c, p) in r.chunks_mut(3).zip(&projectors) {
            let v = [c[0], c[1], c[2]];
            for (i, out) in c.iter_mut().enumerate() {
                *out = p[i * 3] * v[0] + p[i * 3 + 1] * v[1] + p[i * 3 + 2] * v[2];
            }
        }
    };
    let residual = |u: &[f64], touches: &[Touch]| -> Vec<f64> {
        let mut inside = vec![0.0; u.len()];
        stiffness.multiply(u, &mut inside);
        add_pulls(&mut inside, mesh, penalty, touches);
        let mut r: Vec<f64> = f.iter().zip(&inside).map(|(a, b)| a - b).collect();
        project(&mut r);
        r
    };
    let norm = |r: &[f64]| r.iter().map(|x| x * x).sum::<f64>().sqrt();
    let reference = {
        let mut pf = f.clone();
        project(&mut pf);
        norm(&pf).max(1.0e-300)
    };
    let scale_of = |u: &[f64]| u.iter().fold(0.0f64, |a, b| a.max(b.abs())).max(1.0e-300);
    let energy = |u: &[f64], tie_energy: f64| -> f64 {
        let mut ku = vec![0.0; u.len()];
        stiffness.multiply(u, &mut ku);
        0.5 * u.iter().zip(&ku).map(|(a, b)| a * b).sum::<f64>()
            - u.iter().zip(&f).map(|(a, b)| a * b).sum::<f64>()
            + penalty * tie_energy
    };
    let state = |u: &[f64], limits: &[f64]| -> (Vec<Touch>, f64) {
        let scale = scale_of(u);
        let mut stored = 0.0;
        let touches = mesh
            .ties
            .iter()
            .zip(limits)
            .map(|(tie, &limit)| {
                let (t, e) = touch(tie, u, scale, limit);
                stored += e;
                t
            })
            .collect();
        (touches, stored)
    };
    let mut limits = vec![f64::INFINITY; mesh.ties.len()];
    let mut touches: Vec<Touch> = mesh
        .ties
        .iter()
        .map(|t| Touch {
            closed: t.normal.is_some(),
            sticking: t.normal.is_some() && t.friction > 0.0,
            slipping: false,
            pull: Vector3::zero(),
            along: Vector3::zero(),
            ratio: 0.0,
        })
        .collect();
    let mut u = vec![0.0; f.len()];
    let mut stored = 0.0;
    let mut r = residual(&u, &touches);
    let mut rounds = 0;
    let mut iterations = 0;
    let mut solved_to;
    let mut bonded_scale = 0.0;
    let mut inner = 0;
    let settling = if contacts {
        crate::progress::stage("settling contacts", None, "rounds")
    } else {
        crate::progress::Stage::silent()
    };
    loop {
        rounds += 1;
        inner += 1;
        settling.progress(rounds as f64, || {
            let closed = touches.iter().filter(|t| t.closed).count();
            let slipping = touches.iter().filter(|t| t.closed && t.slipping).count();
            format!(
                "{closed} of {} node pairs touching, {slipping} slipping",
                touches.len()
            )
        });
        let mut tangent = stiffness.clone();
        add_contacts(&mut tangent, mesh, penalty, &touches);
        let step = Factored::new(&tangent, &projectors, &coarse, largest)
            .map(|solver| solver.solve(&r))
            .ok()
            .filter(|step| step.residual <= 1.0e-6);
        let Some(step) = step else {
            if friction {
                bail!(
                    "the parts in contact slide: friction cannot hold the sideways load, so nothing stops them"
                );
            }
            bail!("the solver did not converge; check the supports hold the part");
        };
        iterations += step.iterations;
        solved_to = step.residual;
        if !contacts {
            u = step.x;
            break;
        }
        let before = energy(&u, stored);
        let slope: f64 = -r.iter().zip(&step.x).map(|(a, b)| a * b).sum::<f64>();
        let mut alpha = 1.0;
        loop {
            let trial: Vec<f64> = u.iter().zip(&step.x).map(|(a, b)| a + alpha * b).collect();
            let (next, e) = state(&trial, &limits);
            let after = energy(&trial, e);
            if after <= before + 1.0e-4 * alpha * slope || alpha < 1.0e-4 {
                r = residual(&trial, &next);
                u = trial;
                touches = next;
                stored = e;
                break;
            }
            alpha *= 0.5;
        }
        if bonded_scale == 0.0 {
            bonded_scale = scale_of(&u);
        }
        if scale_of(&u) > bonded_scale * 1.0e3 {
            bail!(
                "the parts in contact slide: friction cannot hold the sideways load, so nothing stops them"
            );
        }
        if norm(&r) <= reference * if friction { 1.0e-5 } else { 1.0e-9 } {
            if !friction {
                break;
            }
            inner = 0;
            let next: Vec<f64> = mesh
                .ties
                .iter()
                .map(|tie| match tie.normal {
                    Some(n) if tie.friction > 0.0 => tie.friction * (-gap(tie, &u).dot(n)).max(0.0),
                    _ => f64::INFINITY,
                })
                .collect();
            let top = next
                .iter()
                .filter(|v| v.is_finite())
                .fold(0.0f64, |a, b| a.max(*b))
                .max(1.0e-300);
            let change = limits
                .iter()
                .zip(&next)
                .filter(|(_, b)| b.is_finite())
                .map(|(a, b)| {
                    if a.is_finite() {
                        (a - b).abs()
                    } else {
                        f64::INFINITY
                    }
                })
                .fold(0.0, f64::max);
            limits = next;
            if change <= top * 1.0e-3 {
                break;
            }
            let (t, e) = state(&u, &limits);
            touches = t;
            stored = e;
            r = residual(&u, &touches);
        }
        if rounds >= 400 || inner > 120 {
            if friction {
                bail!(
                    "the parts in contact did not settle in {rounds} steps; friction may not hold the sideways load, so they slide"
                );
            }
            bail!("the contact did not settle in {rounds} steps; try a different `mesh` size");
        }
    }
    if contacts {
        solved_to = norm(&r) / reference;
    }
    drop(settling);
    let _recovering = crate::progress::stage("working out stresses and reactions", None, "");
    let touching = touches.iter().filter(|t| t.closed).count();
    let slipping = touches.iter().filter(|t| t.slipping && t.closed).count();
    let mut ku = vec![0.0; u.len()];
    stiffness.multiply(&u, &mut ku);
    add_pulls(&mut ku, mesh, penalty, &touches);
    let solution = super::solve::Solution {
        x: u,
        iterations,
        residual: solved_to,
    };
    let u = solution.x;
    let mut reactions = vec![Vector3::zero(); problem.supports.len()];
    let mut centres = vec![(Vector3::zero(), 0.0); problem.supports.len()];
    for (v, o) in owner.iter().enumerate() {
        if let Some(s) = o {
            let i = 3 * v;
            reactions[*s] += Vector3::new(ku[i] - f[i], ku[i + 1] - f[i + 1], ku[i + 2] - f[i + 2]);
            centres[*s].0 += mesh.nodes[v].to_vec();
            centres[*s].1 += 1.0;
        }
    }
    let mut moments = vec![Vector3::zero(); problem.supports.len()];
    for (v, o) in owner.iter().enumerate() {
        if let Some(s) = o {
            let i = 3 * v;
            let r = Vector3::new(ku[i] - f[i], ku[i + 1] - f[i + 1], ku[i + 2] - f[i + 2]);
            let mut centre = centres[*s].0 / centres[*s].1;
            if let Hold::Bearing { point, axis, .. } = problem.supports[*s].hold {
                centre = point.to_vec() + axis * (centre - point.to_vec()).dot(axis);
            }
            moments[*s] += (mesh.nodes[v].to_vec() - centre).cross(r);
        }
    }
    let scale: f64 = f
        .chunks(3)
        .map(|c| Vector3::new(c[0], c[1], c[2]).magnitude())
        .sum();
    let applied = f
        .chunks(3)
        .fold(Vector3::zero(), |a, c| a + Vector3::new(c[0], c[1], c[2]));
    let displacement: Vec<Vector3> = u
        .chunks(3)
        .map(|c| Vector3::new(c[0], c[1], c[2]))
        .collect();
    let von_mises = stresses(
        mesh,
        &displacement,
        problem.materials,
        problem.expansion.as_deref(),
    );
    Ok(Answer {
        displacement,
        von_mises,
        reactions: problem
            .supports
            .iter()
            .zip(reactions)
            .zip(moments)
            .map(|((s, r), m)| (s.label.clone(), r, m * 1.0e-3))
            .collect(),
        scale,
        iterations: solution.iterations,
        residual: solution.residual,
        contact: contacts.then_some((touching, slipping, rounds)),
        applied,
    })
}

pub fn von_mises_of(grad: [[f64; 3]; 3], lambda: f64, mu: f64) -> f64 {
    let strain = |i: usize, j: usize| 0.5 * (grad[i][j] + grad[j][i]);
    let trace = strain(0, 0) + strain(1, 1) + strain(2, 2);
    let s =
        |i: usize, j: usize| 2.0 * mu * strain(i, j) + if i == j { lambda * trace } else { 0.0 };
    let (xx, yy, zz) = (s(0, 0), s(1, 1), s(2, 2));
    let (xy, yz, zx) = (s(0, 1), s(1, 2), s(2, 0));
    (0.5 * ((xx - yy).powi(2) + (yy - zz).powi(2) + (zz - xx).powi(2))
        + 3.0 * (xy * xy + yz * yz + zx * zx))
        .sqrt()
}

pub fn stresses(
    mesh: &Quadratic,
    u: &[Vector3],
    materials: &[(f64, f64)],
    expansion: Option<&[f64]>,
) -> Vec<f64> {
    let per: Vec<[f64; 10]> = (0..mesh.tets.len())
        .into_par_iter()
        .map(|t| {
            let (lambda, mu) = lame(
                materials[mesh.body[t] as usize].0,
                materials[mesh.body[t] as usize].1,
            );
            let points = mesh.points(t);
            let tet = mesh.tets[t];
            NODE_COORDS.map(|l| {
                let Some(m) = map(&points, l) else { return 0.0 };
                let mut grad = [[0.0; 3]; 3];
                for (node, g) in tet.iter().zip(&m.gradients) {
                    let d = u[*node as usize];
                    for (i, row) in grad.iter_mut().enumerate() {
                        for (j, cell) in row.iter_mut().enumerate() {
                            *cell += d[i] * g[j];
                        }
                    }
                }
                let free = expansion.map_or(0.0, |e| {
                    shape(l)
                        .iter()
                        .zip(tet)
                        .map(|(n, v)| n * e[v as usize])
                        .sum()
                });
                for (i, row) in grad.iter_mut().enumerate() {
                    row[i] -= free;
                }
                von_mises_of(grad, lambda, mu)
            })
        })
        .collect();
    let weights: Vec<f64> = (0..mesh.tets.len())
        .into_par_iter()
        .map(|t| {
            let p = mesh.points(t);
            let corners = [p[0], p[1], p[2], p[3]].map(|q| [q.x, q.y, q.z]);
            let quality = crate::sim::tet::mean_ratio(corners).max(0.0);
            let volume = (p[1] - p[0]).dot((p[2] - p[0]).cross(p[3] - p[0])).abs() / 6.0;
            volume * quality * quality
        })
        .collect();
    let mut sum = vec![0.0; mesh.nodes.len()];
    let mut total = vec![0.0; mesh.nodes.len()];
    for (t, values) in per.iter().enumerate() {
        let w = weights[t].max(1.0e-300);
        for (k, v) in values.iter().enumerate() {
            let node = mesh.tets[t][k] as usize;
            sum[node] += w * v;
            total[node] += w;
        }
    }
    sum.iter()
        .zip(&total)
        .map(|(s, &w)| if w > 0.0 { s / w } else { 0.0 })
        .collect()
}

pub fn locate(mesh: &Quadratic, p: Point3) -> Option<(usize, [f64; 4])> {
    let mut best: Option<(f64, usize, [f64; 4])> = None;
    for t in 0..mesh.tets.len() {
        let q = mesh.tets[t].map(|v| mesh.nodes[v as usize]);
        let m = Matrix3::from_cols(q[1] - q[0], q[2] - q[0], q[3] - q[0]);
        let Some(inv) = m.invert() else { continue };
        let x = inv * (p - q[0]);
        let l = [1.0 - x.x - x.y - x.z, x.x, x.y, x.z];
        let worst = l.iter().fold(f64::INFINITY, |a, b| a.min(*b));
        if best.as_ref().is_none_or(|b| worst > b.0) {
            best = Some((worst, t, l));
        }
        if worst >= 0.0 {
            break;
        }
    }
    best.filter(|b| b.0 > -0.05).map(|(_, t, l)| (t, l))
}

pub fn locate_near(mesh: &Quadratic, p: Point3) -> Option<((usize, [f64; 4]), f64)> {
    if let Some(found) = locate(mesh, p) {
        return Some((found, 0.0));
    }
    let (gap, nearest, edge) = mesh
        .boundary
        .iter()
        .map(|(f, _)| {
            let corners = [f[0], f[1], f[2]].map(|v| mesh.nodes[v as usize]);
            let (d, l) = super::closest_on_triangle(p, corners);
            let q = corners
                .iter()
                .zip(l)
                .fold(Vector3::zero(), |acc, (c, w)| acc + c.to_vec() * w);
            let edge = [(0, 1), (1, 2), (2, 0)]
                .iter()
                .map(|&(i, j)| (corners[i] - corners[j]).magnitude())
                .fold(0.0, f64::max);
            (d, Point3::from_vec(q), edge)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    if gap > 0.15 * edge {
        return None;
    }
    locate(mesh, nearest).map(|found| (found, gap))
}

pub fn sample(mesh: &Quadratic, values: &[f64], at: (usize, [f64; 4])) -> f64 {
    let (t, l) = at;
    shape(l)
        .iter()
        .zip(mesh.tets[t])
        .map(|(n, v)| n * values[v as usize])
        .sum()
}
