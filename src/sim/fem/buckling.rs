use super::elastic::{Problem, assemble, lame, projectors, solve as solve_static};
use super::modal::ritz;
use super::solve::{Blocks, Reduced};
use super::{GAUSS, Quadratic, map};
use anyhow::{Result, anyhow, bail};
use rayon::prelude::*;

pub struct Buckle {
    pub factor: f64,
    pub shape: Vec<f64>,
}

fn geometric(
    mesh: &Quadratic,
    materials: &[(f64, f64)],
    u: &[monstertruck::modeling::Vector3],
    expansion: Option<&[f64]>,
) -> Blocks {
    let groups: Vec<[u32; 7]> = mesh.ties.iter().map(|t| t.group()).collect();
    let mut blocks = Blocks::pattern_with(mesh.nodes.len(), &mesh.tets, &groups);
    let locals: Vec<[[f64; 10]; 10]> = (0..mesh.tets.len())
        .into_par_iter()
        .map(|t| {
            let (e, nu) = materials[mesh.body[t] as usize];
            let (lambda, mu) = lame(e, nu);
            let points = mesh.points(t);
            let tet = mesh.tets[t];
            let mut local = [[0.0; 10]; 10];
            for (l, w) in GAUSS {
                let Some(m) = map(&points, l) else { continue };
                let mut grad = [[0.0; 3]; 3];
                for (node, g) in tet.iter().zip(&m.gradients) {
                    let d = u[*node as usize];
                    for (i, row) in grad.iter_mut().enumerate() {
                        for (j, cell) in row.iter_mut().enumerate() {
                            *cell += d[i] * g[j];
                        }
                    }
                }
                let free = expansion.map_or(0.0, |e| e[mesh.body[t] as usize]);
                for (i, row) in grad.iter_mut().enumerate() {
                    row[i] -= free;
                }
                let strain = |i: usize, j: usize| 0.5 * (grad[i][j] + grad[j][i]);
                let trace = strain(0, 0) + strain(1, 1) + strain(2, 2);
                let mut sigma = [[0.0; 3]; 3];
                for (i, row) in sigma.iter_mut().enumerate() {
                    for (j, cell) in row.iter_mut().enumerate() {
                        *cell = 2.0 * mu * strain(i, j) + if i == j { lambda * trace } else { 0.0 };
                    }
                }
                let weight = m.det * w;
                for (row, ga) in local.iter_mut().zip(&m.gradients) {
                    let s = [0, 1, 2].map(|i| (0..3).map(|j| sigma[i][j] * ga[j]).sum::<f64>());
                    for (cell, gb) in row.iter_mut().zip(&m.gradients) {
                        *cell += (s[0] * gb[0] + s[1] * gb[1] + s[2] * gb[2]) * weight;
                    }
                }
            }
            local
        })
        .collect();
    for (t, local) in locals.iter().enumerate() {
        let tet = mesh.tets[t];
        for a in 0..10 {
            for b in 0..10 {
                let at = blocks.find(tet[a], tet[b]);
                let v = local[a][b];
                let block = &mut blocks.values[at];
                block[0] += v;
                block[4] += v;
                block[8] += v;
            }
        }
    }
    blocks
}

pub fn solve(problem: &Problem, wanted: usize) -> Result<Vec<Buckle>> {
    use faer::linalg::solvers::Solve;
    let mesh = problem.mesh;
    let answer = solve_static(problem)?;
    let stiffness = assemble(mesh, problem.materials)?;
    let geometry = geometric(
        mesh,
        problem.materials,
        &answer.displacement,
        problem.expansion.as_deref(),
    );
    let (projectors, _) = projectors(mesh, problem.supports);
    let reduced = Reduced::new(&projectors);
    let n = reduced.count;
    if n > super::elastic::DIRECT_FALLBACK {
        bail!(
            "{n} unknowns is too many for the buckling solver, which factors the whole matrix; use a larger `mesh` size"
        );
    }
    let llt = reduced
        .factor(&reduced.lower(&stiffness, None))
        .map_err(|e| anyhow!(e))?;
    let apply = |blocks: &Blocks, x: &[f64]| -> Vec<f64> {
        let full = reduced.expand(x);
        let mut out = vec![0.0; full.len()];
        blocks.multiply(&full, &mut out);
        reduced.restrict(&out)
    };
    let width = (wanted * 2).max(wanted + 8).min(n);
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut x: Vec<Vec<f64>> = (0..width)
        .map(|_| {
            (0..n)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
                })
                .collect()
        })
        .collect();
    let dot = |a: &[f64], b: &[f64]| a.par_iter().zip(b).map(|(p, q)| p * q).sum::<f64>();
    let mut values = vec![0.0; width];
    for round in 0..300 {
        let y: Vec<Vec<f64>> = x
            .par_iter()
            .map(|v| apply(&geometry, v).iter().map(|g| -g).collect())
            .collect();
        let rhs = faer::Mat::<f64>::from_fn(n, width, |i, j| y[j][i]);
        let solved = llt.solve(&rhs);
        let z: Vec<Vec<f64>> = (0..width)
            .map(|j| (0..n).map(|i| solved[(i, j)]).collect())
            .collect();
        let gz: Vec<Vec<f64>> = z
            .par_iter()
            .map(|v| apply(&geometry, v).iter().map(|g| -g).collect())
            .collect();
        let kr: Vec<Vec<f64>> = (0..width)
            .map(|i| (0..width).map(|j| dot(&z[i], &y[j])).collect())
            .collect();
        let gr: Vec<Vec<f64>> = (0..width)
            .map(|i| (0..width).map(|j| dot(&z[i], &gz[j])).collect())
            .collect();
        let Some((theta, q)) = ritz(&gr, &kr) else {
            break;
        };
        let mut order: Vec<usize> = (0..width).collect();
        order.sort_by(|&a, &b| theta[b].total_cmp(&theta[a]));
        x = order
            .iter()
            .map(|&k| {
                let mut v = vec![0.0; n];
                for (j, zj) in z.iter().enumerate() {
                    let c = q[k][j];
                    v.iter_mut().zip(zj).for_each(|(a, b)| *a += c * b);
                }
                v
            })
            .collect();
        let next: Vec<f64> = order.iter().map(|&k| theta[k]).collect();
        let settled =
            (0..wanted.min(width)).all(|k| (next[k] - values[k]).abs() <= next[k].abs() * 1.0e-9);
        values = next;
        if settled && round > 2 {
            break;
        }
    }
    let buckles = values
        .iter()
        .zip(&x)
        .filter(|(mu, _)| **mu > 0.0)
        .take(wanted)
        .map(|(mu, v)| {
            let shape = reduced.expand(v);
            let top = shape
                .iter()
                .fold(0.0f64, |a, b| a.max(b.abs()))
                .max(1.0e-300);
            Buckle {
                factor: 1.0 / mu,
                shape: shape.iter().map(|s| s / top).collect(),
            }
        })
        .collect();
    Ok(buckles)
}
