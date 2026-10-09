use super::solve::{Block, Blocks, IDENTITY, direct, two_level};

const DIRECT_LIMIT: usize = 100_000;
pub const DIRECT_FALLBACK: usize = 600_000;
pub const TIE: f64 = 100.0;
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
    Pin { point: Point3, axis: Vector3 },
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
    pub faces: Vec<usize>,
    pub push: Push,
}

pub struct Problem<'a> {
    pub mesh: &'a Quadratic,
    pub materials: &'a [(f64, f64)],
    pub supports: &'a [Support],
    pub loads: &'a [Load],
    pub body_force: Option<Vec<Vector3>>,
    pub expansion: Option<Vec<f64>>,
}

pub struct Answer {
    pub displacement: Vec<Vector3>,
    pub von_mises: Vec<f64>,
    pub reactions: Vec<(String, Vector3, Vector3)>,
    pub scale: f64,
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
    let constants: Vec<(f64, f64)> = materials.iter().map(|&(e, nu)| lame(e, nu)).collect();
    let groups: Vec<[u32; 7]> = mesh.ties.iter().map(|t| t.group()).collect();
    let mut blocks = Blocks::pattern_with(mesh.nodes.len(), &mesh.tets, &groups);
    let batch = 4096;
    for start in (0..mesh.tets.len()).step_by(batch) {
        let end = (start + batch).min(mesh.tets.len());
        let matrices: Vec<Option<Vec<Block>>> = (start..end)
            .into_par_iter()
            .map(|t| {
                let (lambda, mu) = constants[mesh.body[t] as usize];
                element_matrix(&mesh.points(t), lambda, mu)
            })
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
    if !mesh.ties.is_empty() {
        let stiffest = materials.iter().map(|m| m.0).fold(0.0, f64::max);
        let penalty = stiffest * spacing(mesh) * TIE;
        for tie in &mesh.ties {
            let (nodes, c) = (tie.group(), tie.coefficients());
            for a in 0..7 {
                for b in 0..7 {
                    let at = blocks.find(nodes[a], nodes[b]);
                    let v = penalty * c[a] * c[b];
                    let block = &mut blocks.values[at];
                    block[0] += v;
                    block[4] += v;
                    block[8] += v;
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
        for face in faces_of(mesh, &support.faces) {
            let p = six(mesh, face);
            for (k, &node) in face.iter().enumerate() {
                owner[node as usize].get_or_insert(s);
                match support.hold {
                    Hold::Fixed => fixed[node as usize] = true,
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
    let solution = if mesh.nodes.len() * 3 <= DIRECT_LIMIT {
        direct(&stiffness, &f, &projectors).map_err(|e| anyhow::anyhow!(e))?
    } else {
        let iterative = two_level(&stiffness, &f, &projectors, &mesh.coarse(), 1.0e-10, 3000)
            .map_err(|e| anyhow::anyhow!(e))?;
        if iterative.residual > 1.0e-8 && mesh.nodes.len() * 3 <= DIRECT_FALLBACK {
            direct(&stiffness, &f, &projectors).map_err(|e| anyhow::anyhow!(e))?
        } else {
            iterative
        }
    };
    let u = solution.x;
    let mut ku = vec![0.0; u.len()];
    stiffness.multiply(&u, &mut ku);
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
            let centre = centres[*s].0 / centres[*s].1;
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
