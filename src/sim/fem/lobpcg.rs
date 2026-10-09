use super::modal::ritz;
use rayon::prelude::*;

pub type Operator<'a> = &'a (dyn Fn(&[f64]) -> Vec<f64> + Sync);

pub struct Eigen {
    pub values: Vec<f64>,
    pub vectors: Vec<Vec<f64>>,
    pub iterations: usize,
    pub residual: f64,
}

#[derive(Clone, Default)]
struct Block {
    v: Vec<Vec<f64>>,
    m: Vec<Vec<f64>>,
    k: Vec<Vec<f64>>,
}

impl Block {
    fn len(&self) -> usize {
        self.v.len()
    }

    fn extend(&mut self, other: Block) {
        self.v.extend(other.v);
        self.m.extend(other.m);
        self.k.extend(other.k);
    }

    fn combine(&self, coefficients: &[Vec<f64>]) -> Block {
        let mix = |set: &[Vec<f64>]| -> Vec<Vec<f64>> {
            coefficients
                .par_iter()
                .map(|c| {
                    let n = set[0].len();
                    let mut out = vec![0.0; n];
                    out.par_iter_mut().enumerate().for_each(|(i, o)| {
                        *o = set.iter().zip(c).map(|(s, w)| w * s[i]).sum();
                    });
                    out
                })
                .collect()
        };
        Block {
            v: mix(&self.v),
            m: mix(&self.m),
            k: mix(&self.k),
        }
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.par_iter().zip(b).map(|(x, y)| x * y).sum()
}

fn axpy(y: &mut [f64], a: f64, x: &[f64]) {
    y.par_iter_mut().zip(x).for_each(|(p, q)| *p += a * q);
}

fn orthonormal(mut block: Block, against: &Block) -> Block {
    for _ in 0..2 {
        for i in 0..block.len() {
            for j in 0..against.len() {
                let c = dot(&against.v[j], &block.m[i]);
                axpy(&mut block.v[i], -c, &against.v[j]);
                axpy(&mut block.m[i], -c, &against.m[j]);
                axpy(&mut block.k[i], -c, &against.k[j]);
            }
        }
    }
    let count = block.len();
    if count == 0 {
        return block;
    }
    let gram: Vec<Vec<f64>> = (0..count)
        .map(|i| (0..count).map(|j| dot(&block.v[i], &block.m[j])).collect())
        .collect();
    let identity: Vec<Vec<f64>> = (0..count)
        .map(|i| (0..count).map(|j| if i == j { 1.0 } else { 0.0 }).collect())
        .collect();
    let Some((values, vectors)) = ritz(&gram, &identity) else {
        return Block::default();
    };
    let top = values.iter().copied().fold(0.0, f64::max);
    let coefficients: Vec<Vec<f64>> = values
        .iter()
        .zip(&vectors)
        .filter(|(d, _)| **d > top * 1.0e-10 && **d > 0.0)
        .map(|(d, u)| u.iter().map(|x| x / d.sqrt()).collect())
        .collect();
    block.combine(&coefficients)
}

#[allow(clippy::too_many_arguments)]
pub fn smallest(
    n: usize,
    count: usize,
    excluded: Vec<Vec<f64>>,
    stiffness: Operator,
    mass: Operator,
    precondition: Operator,
    tolerance: f64,
    limit: usize,
) -> Eigen {
    let size = (count + 3).max(count * 5 / 4).min(n);
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let start: Vec<Vec<f64>> = (0..size)
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
    let block_of = |v: Vec<Vec<f64>>| -> Block {
        let m = v.par_iter().map(|x| mass(x)).collect();
        let k = v.par_iter().map(|x| stiffness(x)).collect();
        Block { v, m, k }
    };
    let locked = orthonormal(block_of(excluded), &Block::default());
    let mut x = orthonormal(block_of(start), &locked);
    let mut p = Block::default();
    let mut values = vec![0.0; x.len()];
    let mut iterations = 0;
    let mut worst = f64::INFINITY;
    for round in 0..=limit {
        let mut basis = x.clone();
        basis.extend(p.clone());
        let width = basis.len();
        let projected: Vec<Vec<f64>> = (0..width)
            .map(|i| (0..width).map(|j| dot(&basis.v[i], &basis.k[j])).collect())
            .collect();
        let symmetric: Vec<Vec<f64>> = (0..width)
            .map(|i| {
                (0..width)
                    .map(|j| 0.5 * (projected[i][j] + projected[j][i]))
                    .collect()
            })
            .collect();
        let identity: Vec<Vec<f64>> = (0..width)
            .map(|i| (0..width).map(|j| if i == j { 1.0 } else { 0.0 }).collect())
            .collect();
        let Some((theta, vectors)) = ritz(&symmetric, &identity) else {
            break;
        };
        let keep = size.min(width);
        let chosen: Vec<Vec<f64>> = vectors[..keep].to_vec();
        let next = basis.combine(&chosen);
        if round > 0 {
            let tail: Vec<Vec<f64>> = chosen
                .iter()
                .map(|c| {
                    c.iter()
                        .enumerate()
                        .map(|(j, w)| if j < x.len() { 0.0 } else { *w })
                        .collect()
                })
                .collect();
            let moved = basis.combine(&tail);
            p = moved;
        }
        x = next;
        values = theta[..keep].to_vec();
        iterations = round;
        let residuals: Vec<Vec<f64>> = (0..x.len())
            .into_par_iter()
            .map(|i| {
                x.k[i]
                    .iter()
                    .zip(&x.m[i])
                    .map(|(k, m)| k - values[i] * m)
                    .collect()
            })
            .collect();
        let errors: Vec<f64> = residuals
            .iter()
            .zip(&x.k)
            .map(|(r, k)| dot(r, r).sqrt() / dot(k, k).sqrt().max(1.0e-300))
            .collect();
        worst = errors[..count.min(errors.len())]
            .iter()
            .copied()
            .fold(0.0, f64::max);
        if worst < tolerance || round == limit {
            break;
        }
        let active: Vec<Vec<f64>> = residuals
            .iter()
            .zip(&errors)
            .filter(|(_, e)| **e >= tolerance)
            .map(|(r, _)| precondition(r))
            .collect();
        let mut known = locked.clone();
        known.extend(x.clone());
        p = orthonormal(p, &known);
        known.extend(p.clone());
        let w = orthonormal(block_of(active), &known);
        p.extend(w);
    }
    Eigen {
        values,
        vectors: x.v,
        iterations,
        residual: worst,
    }
}
