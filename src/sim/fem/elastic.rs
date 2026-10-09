use super::solve::{Block, Blocks, IDENTITY, conjugate_gradient, direct};

const DIRECT_LIMIT: usize = 600_000;
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
}

#[derive(Clone, Debug)]
pub struct Load {
    pub faces: Vec<usize>,
    pub push: Push,
}

pub struct Problem<'a> {
    pub mesh: &'a Quadratic,
    pub young: f64,
    pub poisson: f64,
    pub supports: &'a [Support],
    pub loads: &'a [Load],
    pub body_force: Option<Vector3>,
}

pub struct Answer {
    pub displacement: Vec<Vector3>,
    pub von_mises: Vec<f64>,
    pub reactions: Vec<(String, Vector3)>,
    pub iterations: usize,
    pub residual: f64,
    pub applied: Vector3,
}

fn lame(young: f64, poisson: f64) -> (f64, f64) {
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

pub fn assemble(mesh: &Quadratic, young: f64, poisson: f64) -> Result<Blocks> {
    let (lambda, mu) = lame(young, poisson);
    let mut blocks = Blocks::pattern(mesh.nodes.len(), &mesh.tets);
    let batch = 4096;
    for start in (0..mesh.tets.len()).step_by(batch) {
        let end = (start + batch).min(mesh.tets.len());
        let matrices: Vec<Option<Vec<Block>>> = (start..end)
            .into_par_iter()
            .map(|t| element_matrix(&mesh.points(t), lambda, mu))
            .collect();
        for (t, matrix) in (start..end).zip(matrices) {
            let Some(matrix) = matrix else {
                bail!("element {t} is inverted; try another `mesh` size");
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
    Ok(blocks)
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
        for face in faces_of(mesh, &load.faces) {
            let p = six(mesh, face);
            for (l, w) in triangle_gauss() {
                let normal = triangle_area_vector(&p, l);
                let traction = match load.push {
                    Push::Force(force) => force * (normal.magnitude() / total),
                    Push::Pressure(pressure) => -normal * pressure,
                };
                for (k, n) in triangle_shape(l).iter().enumerate() {
                    add(face[k], traction * (n * w));
                }
            }
        }
    }
    if let Some(b) = problem.body_force {
        for t in 0..mesh.tets.len() {
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
        for face in faces_of(mesh, &support.faces) {
            let p = six(mesh, face);
            for (k, &node) in face.iter().enumerate() {
                owner[node as usize].get_or_insert(s);
                match support.hold {
                    Hold::Fixed => fixed[node as usize] = true,
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

pub fn solve(problem: &Problem) -> Result<Answer> {
    let mesh = problem.mesh;
    if problem.supports.is_empty() {
        bail!("add a support, like `fix <X`, or the part is free to fly away");
    }
    let stiffness = assemble(mesh, problem.young, problem.poisson)?;
    let f = loads(problem)?;
    let (projectors, owner) = projectors(mesh, problem.supports);
    let solution = if mesh.nodes.len() * 3 <= DIRECT_LIMIT {
        direct(&stiffness, &f, &projectors).map_err(|e| anyhow::anyhow!(e))?
    } else {
        conjugate_gradient(
            &stiffness,
            &f,
            &projectors,
            1.0e-10,
            20 * mesh.nodes.len().max(1000),
        )
    };
    let u = solution.x;
    let mut ku = vec![0.0; u.len()];
    stiffness.multiply(&u, &mut ku);
    let mut reactions = vec![Vector3::zero(); problem.supports.len()];
    for (v, o) in owner.iter().enumerate() {
        if let Some(s) = o {
            let i = 3 * v;
            reactions[*s] += Vector3::new(ku[i] - f[i], ku[i + 1] - f[i + 1], ku[i + 2] - f[i + 2]);
        }
    }
    let applied = f
        .chunks(3)
        .fold(Vector3::zero(), |a, c| a + Vector3::new(c[0], c[1], c[2]));
    let displacement: Vec<Vector3> = u
        .chunks(3)
        .map(|c| Vector3::new(c[0], c[1], c[2]))
        .collect();
    let von_mises = stresses(mesh, &displacement, problem.young, problem.poisson);
    Ok(Answer {
        displacement,
        von_mises,
        reactions: problem
            .supports
            .iter()
            .zip(reactions)
            .map(|(s, r)| (s.label.clone(), r))
            .collect(),
        iterations: solution.iterations,
        residual: solution.residual,
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

pub fn stresses(mesh: &Quadratic, u: &[Vector3], young: f64, poisson: f64) -> Vec<f64> {
    let (lambda, mu) = lame(young, poisson);
    let per: Vec<[f64; 10]> = (0..mesh.tets.len())
        .into_par_iter()
        .map(|t| {
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
                von_mises_of(grad, lambda, mu)
            })
        })
        .collect();
    let mut sum = vec![0.0; mesh.nodes.len()];
    let mut count = vec![0u32; mesh.nodes.len()];
    for (t, values) in per.iter().enumerate() {
        for (k, v) in values.iter().enumerate() {
            let node = mesh.tets[t][k] as usize;
            sum[node] += v;
            count[node] += 1;
        }
    }
    sum.iter()
        .zip(&count)
        .map(|(s, &c)| if c > 0 { s / c as f64 } else { 0.0 })
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

pub fn sample(mesh: &Quadratic, values: &[f64], at: (usize, [f64; 4])) -> f64 {
    let (t, l) = at;
    shape(l)
        .iter()
        .zip(mesh.tets[t])
        .map(|(n, v)| n * values[v as usize])
        .sum()
}
