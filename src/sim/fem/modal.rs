#![allow(clippy::needless_range_loop)]

use super::elastic::{Support, assemble_bonded, projectors};
use super::solve::{Blocks, Reduced, TwoLevel};
use super::{Quadratic, map, shape};
use anyhow::{Result, anyhow};
use monstertruck::modeling::*;
use rayon::prelude::*;

pub const DIRECT_LIMIT: usize = 250_000;

pub const MASS_RULE: [([f64; 4], f64); 14] = {
    const A: f64 = 0.092_735_250_310_891_2;
    const B: f64 = 0.310_885_919_263_300_6;
    const C: f64 = 0.454_496_295_874_350_4;
    const D: f64 = 0.5 - C;
    const WA: f64 = 0.012_248_840_519_393_66;
    const WB: f64 = 0.018_781_320_953_002_64;
    const WC: f64 = 0.007_091_003_462_846_911;
    const AA: f64 = 1.0 - 3.0 * A;
    const BB: f64 = 1.0 - 3.0 * B;
    [
        ([AA, A, A, A], WA),
        ([A, AA, A, A], WA),
        ([A, A, AA, A], WA),
        ([A, A, A, AA], WA),
        ([BB, B, B, B], WB),
        ([B, BB, B, B], WB),
        ([B, B, BB, B], WB),
        ([B, B, B, BB], WB),
        ([C, C, D, D], WC),
        ([C, D, C, D], WC),
        ([C, D, D, C], WC),
        ([D, C, C, D], WC),
        ([D, C, D, C], WC),
        ([D, D, C, C], WC),
    ]
};

pub fn mass(mesh: &Quadratic, densities: &[f64]) -> Blocks {
    let groups: Vec<[u32; 7]> = mesh.ties.iter().map(|t| t.group()).collect();
    let mut blocks = Blocks::pattern_with(mesh.nodes.len(), &mesh.tets, &groups);
    let locals: Vec<[[f64; 10]; 10]> = (0..mesh.tets.len())
        .into_par_iter()
        .map(|t| {
            let density = densities[mesh.body[t] as usize];
            let points = mesh.points(t);
            let mut local = [[0.0; 10]; 10];
            for (l, w) in MASS_RULE {
                if let Some(m) = map(&points, l) {
                    let n = shape(l);
                    for (row, a) in local.iter_mut().zip(n) {
                        for (cell, b) in row.iter_mut().zip(n) {
                            *cell += density * a * b * m.det * w;
                        }
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

pub struct Mode {
    pub frequency: f64,
    pub shape: Vec<f64>,
    pub effective: [f64; 3],
}

pub struct Modes {
    pub modes: Vec<Mode>,
    pub total_mass: f64,
    pub iterations: usize,
}

pub fn jacobi_eigen(a: &mut [Vec<f64>]) -> (Vec<f64>, Vec<Vec<f64>>) {
    let n = a.len();
    let mut v: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect())
        .collect();
    for _ in 0..100 {
        let off: f64 = (0..n)
            .flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| a[i][j] * a[i][j])
            .sum();
        let diag: f64 = (0..n).map(|i| a[i][i] * a[i][i]).sum();
        if off <= diag * 1.0e-30 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if a[p][q].abs() < 1.0e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a[k][p], a[k][q]);
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a[p][k], a[q][k]);
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for row in v.iter_mut() {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = c * vp - s * vq;
                    row[q] = s * vp + c * vq;
                }
            }
        }
    }
    ((0..n).map(|i| a[i][i]).collect(), v)
}

fn cholesky(m: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = m.len();
    let mut l = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let sum: f64 = (0..j).map(|k| l[i][k] * l[j][k]).sum();
            if i == j {
                let d = m[i][i] - sum;
                if d <= 0.0 {
                    return None;
                }
                l[i][i] = d.sqrt();
            } else {
                l[i][j] = (m[i][j] - sum) / l[j][j];
            }
        }
    }
    Some(l)
}

fn lower_solve(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let n = l.len();
    let mut x = vec![0.0; n];
    for i in 0..n {
        let s: f64 = (0..i).map(|k| l[i][k] * x[k]).sum();
        x[i] = (b[i] - s) / l[i][i];
    }
    x
}

fn upper_solve(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let n = l.len();
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let s: f64 = (i + 1..n).map(|k| l[k][i] * x[k]).sum();
        x[i] = (b[i] - s) / l[i][i];
    }
    x
}

pub fn ritz(k: &[Vec<f64>], m: &[Vec<f64>]) -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
    let n = k.len();
    let l = cholesky(m)?;
    let mut c = vec![vec![0.0; n]; n];
    let columns: Vec<Vec<f64>> = (0..n)
        .map(|j| lower_solve(&l, &(0..n).map(|i| k[i][j]).collect::<Vec<_>>()))
        .collect();
    for j in 0..n {
        let row: Vec<f64> = (0..n).map(|i| columns[i][j]).collect();
        let solved = lower_solve(&l, &row);
        c[j][..n].copy_from_slice(&solved[..n]);
    }
    for i in 0..n {
        for j in i + 1..n {
            let avg = (c[i][j] + c[j][i]) / 2.0;
            c[i][j] = avg;
            c[j][i] = avg;
        }
    }
    let (values, vectors) = jacobi_eigen(&mut c);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let sorted_values = order.iter().map(|&i| values[i]).collect();
    let sorted_vectors = order
        .iter()
        .map(|&j| upper_solve(&l, &(0..n).map(|i| vectors[i][j]).collect::<Vec<_>>()))
        .collect();
    Some((sorted_values, sorted_vectors))
}

pub fn solve(
    mesh: &Quadratic,
    materials: &[(f64, f64)],
    densities: &[f64],
    supports: &[Support],
    wanted: usize,
) -> Result<Modes> {
    let stiffness = assemble_bonded(mesh, materials)?;
    let mass = mass(mesh, densities);
    let (projectors, _) = projectors(mesh, supports);
    let reduced = Reduced::new(&projectors);
    let n = reduced.count;
    if n > 6 * super::elastic::DIRECT_FALLBACK {
        anyhow::bail!(
            "{n} unknowns is too many for the frequency solver; use a larger `mesh` size"
        );
    }
    let free = supports.is_empty();
    let count = (wanted + if free { 6 } else { 0 }).min(n);
    let ratio: f64 = {
        let (k, m): (f64, f64) = (0..mesh.nodes.len()).fold((0.0, 0.0), |(k, m), i| {
            (k + stiffness.diagonal(i)[0], m + mass.diagonal(i)[0])
        });
        k / m.max(1.0e-300)
    };
    let shift = ratio * 1.0e-8;
    let apply = |blocks: &Blocks, x: &[f64]| -> Vec<f64> {
        let full = reduced.expand(x);
        let mut out = vec![0.0; full.len()];
        blocks.multiply(&full, &mut out);
        reduced.restrict(&out)
    };
    let dot = |a: &[f64], b: &[f64]| a.par_iter().zip(b).map(|(p, q)| p * q).sum::<f64>();
    let (values, x, iterations) = if n <= DIRECT_LIMIT {
        shift_invert(&reduced, &stiffness, &mass, shift, count)?
    } else {
        let rigid: Vec<Vec<f64>> = if free {
            let centre = mesh.nodes.iter().map(|p| p.to_vec()).sum::<Vector3>()
                / mesh.nodes.len().max(1) as f64;
            (0..6)
                .map(|k| {
                    let mut full = vec![0.0; mesh.nodes.len() * 3];
                    for (i, p) in mesh.nodes.iter().enumerate() {
                        let axis = [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()][k % 3];
                        let d = if k < 3 {
                            axis
                        } else {
                            axis.cross(p.to_vec() - centre)
                        };
                        full[3 * i..3 * i + 3].copy_from_slice(&[d.x, d.y, d.z]);
                    }
                    reduced.restrict(&full)
                })
                .collect()
        } else {
            Vec::new()
        };
        let count = if free { wanted.min(n) } else { count };
        let mut shifted = stiffness.clone();
        for row in 0..mass.nodes() {
            for k in mass.starts[row]..mass.starts[row + 1] {
                let at = shifted.find(row as u32, mass.columns[k]);
                for (a, b) in shifted.values[at].iter_mut().zip(&mass.values[k]) {
                    *a += shift * b;
                }
            }
        }
        let coarse = mesh.coarse();
        let level = TwoLevel::new(&shifted, &projectors, &coarse).map_err(|e| anyhow!(e))?;
        let eigen = super::lobpcg::smallest(
            n,
            count,
            rigid,
            &|v: &[f64]| apply(&shifted, v),
            &|v: &[f64]| apply(&mass, v),
            &|r: &[f64]| {
                let full = reduced.expand(r);
                let mut z = vec![0.0; full.len()];
                level.apply(&full, &mut z);
                reduced.restrict(&z)
            },
            1.0e-6,
            400,
        );
        if eigen.residual > 1.0e-4 {
            anyhow::bail!(
                "the frequency solver did not settle (residual {:.1e} after {} iterations); try a different `mesh` size",
                eigen.residual,
                eigen.iterations
            );
        }
        (eigen.values, eigen.vectors, eigen.iterations)
    };
    let total_mass = {
        let mut full = vec![0.0; mesh.nodes.len() * 3];
        for i in 0..mesh.nodes.len() {
            full[3 * i] = 1.0;
        }
        let mut out = vec![0.0; full.len()];
        mass.multiply(&full, &mut out);
        out.iter().step_by(3).sum::<f64>()
    };
    let directions: Vec<Vec<f64>> = (0..3)
        .map(|d| {
            let mut full = vec![0.0; mesh.nodes.len() * 3];
            for i in 0..mesh.nodes.len() {
                full[3 * i + d] = 1.0;
            }
            let mut out = vec![0.0; full.len()];
            mass.multiply(&full, &mut out);
            reduced.restrict(&out)
        })
        .collect();
    let mut modes = Vec::new();
    for k in 0..count.min(values.len()) {
        let lambda = values[k] - shift;
        let frequency = lambda.max(0.0).sqrt() / (2.0 * std::f64::consts::PI);
        let v = &x[k];
        let norm = dot(v, &apply(&mass, v)).sqrt().max(1.0e-300);
        let phi: Vec<f64> = v.iter().map(|a| a / norm).collect();
        let effective = [0, 1, 2].map(|d| {
            let g = dot(&phi, &directions[d]);
            g * g / total_mass.max(1.0e-300)
        });
        modes.push(Mode {
            frequency,
            shape: reduced.expand(&phi),
            effective,
        });
    }
    if free {
        let top = modes.iter().map(|m| m.frequency).fold(0.0, f64::max);
        modes.retain(|m| m.frequency > top * 1.0e-3);
    }
    modes.truncate(wanted);
    Ok(Modes {
        modes,
        total_mass,
        iterations,
    })
}

type Pairs = (Vec<f64>, Vec<Vec<f64>>, usize);

fn shift_invert(
    reduced: &Reduced,
    stiffness: &Blocks,
    mass: &Blocks,
    shift: f64,
    count: usize,
) -> Result<Pairs> {
    use faer::linalg::solvers::Solve;
    let n = reduced.count;
    let width = (count * 2).max(count + 8).min(n);
    let llt = reduced
        .factor(&reduced.lower(stiffness, Some((mass, shift))))
        .map_err(|e| anyhow!(e))?;
    let apply = |blocks: &Blocks, x: &[f64]| -> Vec<f64> {
        let full = reduced.expand(x);
        let mut out = vec![0.0; full.len()];
        blocks.multiply(&full, &mut out);
        reduced.restrict(&out)
    };
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
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
    let mut iterations = 0;
    for round in 0..200 {
        iterations = round + 1;
        let y: Vec<Vec<f64>> = x.par_iter().map(|v| apply(mass, v)).collect();
        let rhs = faer::Mat::<f64>::from_fn(n, width, |i, j| y[j][i]);
        let solved = llt.solve(&rhs);
        let z: Vec<Vec<f64>> = (0..width)
            .map(|j| (0..n).map(|i| solved[(i, j)]).collect())
            .collect();
        let mz: Vec<Vec<f64>> = z.par_iter().map(|v| apply(mass, v)).collect();
        let kr: Vec<Vec<f64>> = (0..width)
            .map(|i| (0..width).map(|j| dot(&z[i], &y[j])).collect())
            .collect();
        let mr: Vec<Vec<f64>> = (0..width)
            .map(|i| (0..width).map(|j| dot(&z[i], &mz[j])).collect())
            .collect();
        let Some((theta, q)) = ritz(&kr, &mr) else {
            break;
        };
        x = (0..width)
            .map(|k| {
                let mut v = vec![0.0; n];
                for (j, zj) in z.iter().enumerate() {
                    let c = q[k][j];
                    v.iter_mut().zip(zj).for_each(|(a, b)| *a += c * b);
                }
                v
            })
            .collect();
        let settled = (0..count).all(|k| (theta[k] - values[k]).abs() <= theta[k].abs() * 1.0e-10);
        values = theta;
        if settled && round > 2 {
            break;
        }
    }
    Ok((values, x, iterations))
}
