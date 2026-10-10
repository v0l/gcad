use super::solve::{Block, Blocks, block_jacobi, project, times};
use rayon::prelude::*;

pub struct Rigid {
    a: Blocks,
    projectors: Vec<Block>,
    inverse: Vec<Block>,
    weight: f64,
    owner: Vec<u32>,
    modes: Vec<[f64; 18]>,
    count: usize,
    llt: faer::sparse::linalg::solvers::Llt<usize, f64>,
}

fn groups(a: &Blocks) -> (Vec<u32>, usize) {
    let n = a.nodes();
    let mut owner = vec![u32::MAX; n];
    let mut count = 0u32;
    for v in 0..n {
        if owner[v] != u32::MAX {
            continue;
        }
        let range = a.starts[v]..a.starts[v + 1];
        if a.columns[range.clone()]
            .iter()
            .any(|&c| owner[c as usize] != u32::MAX)
        {
            continue;
        }
        for &c in &a.columns[range] {
            owner[c as usize] = count;
        }
        count += 1;
    }
    for v in 0..n {
        if owner[v] == u32::MAX {
            let near = (a.starts[v]..a.starts[v + 1])
                .map(|k| owner[a.columns[k] as usize])
                .find(|&o| o != u32::MAX);
            owner[v] = match near {
                Some(o) => o,
                None => {
                    count += 1;
                    count - 1
                }
            };
        }
    }
    (owner, count as usize)
}

impl Rigid {
    pub fn new(a: Blocks, projectors: Vec<Block>, positions: &[[f64; 3]]) -> Result<Rigid, String> {
        let (owner, count) = groups(&a);
        let mut centres = vec![([0.0; 3], 0.0); count];
        for (v, p) in positions.iter().enumerate() {
            let c = &mut centres[owner[v] as usize];
            for (sum, q) in c.0.iter_mut().zip(p) {
                *sum += q;
            }
            c.1 += 1.0;
        }
        let size = {
            let (lo, hi) = positions.iter().fold(
                ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
                |(lo, hi), p| {
                    (
                        [lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])],
                        [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])],
                    )
                },
            );
            ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2))
                .sqrt()
                .max(1.0e-9)
        };
        let modes: Vec<[f64; 18]> = positions
            .iter()
            .enumerate()
            .map(|(v, p)| {
                let (sum, n) = centres[owner[v] as usize];
                let r = [0, 1, 2].map(|d| (p[d] - sum[d] / n) / size);
                let raw = [
                    [1.0, 0.0, 0.0, 0.0, r[2], -r[1]],
                    [0.0, 1.0, 0.0, -r[2], 0.0, r[0]],
                    [0.0, 0.0, 1.0, r[1], -r[0], 0.0],
                ];
                let q = &projectors[v];
                let mut out = [0.0; 18];
                for i in 0..3 {
                    for j in 0..6 {
                        out[i * 6 + j] = (0..3).map(|k| q[i * 3 + k] * raw[k][j]).sum();
                    }
                }
                out
            })
            .collect();
        let mut entries: std::collections::HashMap<(usize, usize), f64> =
            std::collections::HashMap::new();
        for v in 0..a.nodes() {
            let (gv, mv) = (owner[v] as usize, &modes[v]);
            for k in a.starts[v]..a.starts[v + 1] {
                let w = a.columns[k] as usize;
                let (gw, mw) = (owner[w] as usize, &modes[w]);
                let block = &a.values[k];
                let mut am = [0.0; 18];
                for i in 0..3 {
                    for j in 0..6 {
                        am[i * 6 + j] = (0..3).map(|m| block[i * 3 + m] * mw[m * 6 + j]).sum();
                    }
                }
                for i in 0..6 {
                    for j in 0..6 {
                        let value: f64 = (0..3).map(|m| mv[m * 6 + i] * am[m * 6 + j]).sum();
                        let (r, c) = (gv * 6 + i, gw * 6 + j);
                        if c <= r && value != 0.0 {
                            *entries.entry((r, c)).or_insert(0.0) += value;
                        }
                    }
                }
            }
        }
        let largest = entries
            .iter()
            .filter(|((r, c), _)| r == c)
            .map(|(_, v)| v.abs())
            .fold(0.0, f64::max);
        for d in 0..count * 6 {
            *entries.entry((d, d)).or_insert(0.0) += largest * 1.0e-10;
        }
        let triplets: Vec<faer::sparse::Triplet<usize, usize, f64>> = entries
            .into_iter()
            .map(|((r, c), v)| faer::sparse::Triplet::new(r, c, v))
            .collect();
        let matrix = faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(
            count * 6,
            count * 6,
            &triplets,
        )
        .map_err(|e| format!("{e:?}"))?;
        faer::set_global_parallelism(faer::Par::Seq);
        let llt = matrix
            .sp_cholesky(faer::Side::Lower)
            .map_err(|_| "the aggregated coarse problem is singular".to_string())?;
        let inverse = block_jacobi(&a);
        let mut rigid = Rigid {
            a,
            projectors,
            inverse,
            weight: 0.6,
            owner,
            modes,
            count,
            llt,
        };
        rigid.weight = (1.2 / rigid.spread()).min(0.6);
        Ok(rigid)
    }

    fn smooth(&self, b: &[f64], x: &mut [f64], ax: &mut [f64]) {
        self.a.multiply(x, ax);
        project(&self.projectors, ax);
        let w = self.weight;
        x.par_chunks_mut(3).enumerate().for_each(|(i, out)| {
            let r = [
                b[3 * i] - ax[3 * i],
                b[3 * i + 1] - ax[3 * i + 1],
                b[3 * i + 2] - ax[3 * i + 2],
            ];
            let d = times(&self.inverse[i], r);
            for k in 0..3 {
                out[k] += w * d[k];
            }
        });
        project(&self.projectors, x);
    }

    fn spread(&self) -> f64 {
        let n = self.a.nodes() * 3;
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut x: Vec<f64> = (0..n)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
            })
            .collect();
        project(&self.projectors, &mut x);
        let mut largest = 1.0;
        let mut ax = vec![0.0; n];
        for _ in 0..12 {
            let norm = x.iter().map(|v| v * v).sum::<f64>().sqrt().max(1.0e-300);
            x.iter_mut().for_each(|v| *v /= norm);
            self.a.multiply(&x, &mut ax);
            project(&self.projectors, &mut ax);
            let next: Vec<f64> = ax
                .chunks(3)
                .enumerate()
                .flat_map(|(i, c)| times(&self.inverse[i], [c[0], c[1], c[2]]))
                .collect();
            largest = next.iter().map(|v| v * v).sum::<f64>().sqrt();
            x = next;
            project(&self.projectors, &mut x);
        }
        largest.max(1.0e-12)
    }

    pub fn apply(&self, b: &[f64]) -> Vec<f64> {
        use faer::linalg::solvers::Solve;
        let n = b.len();
        let mut b = b.to_vec();
        project(&self.projectors, &mut b);
        let mut x = vec![0.0; n];
        let mut ax = vec![0.0; n];
        for _ in 0..2 {
            self.smooth(&b, &mut x, &mut ax);
        }
        self.a.multiply(&x, &mut ax);
        project(&self.projectors, &mut ax);
        let mut coarse = vec![0.0; self.count * 6];
        for (v, m) in self.modes.iter().enumerate() {
            let g = self.owner[v] as usize;
            for j in 0..6 {
                coarse[g * 6 + j] += (0..3)
                    .map(|i| m[i * 6 + j] * (b[3 * v + i] - ax[3 * v + i]))
                    .sum::<f64>();
            }
        }
        let solved = self
            .llt
            .solve(faer::Mat::<f64>::from_fn(self.count * 6, 1, |i, _| {
                coarse[i]
            }));
        x.par_chunks_mut(3).enumerate().for_each(|(v, out)| {
            let g = self.owner[v] as usize;
            let m = &self.modes[v];
            for i in 0..3 {
                out[i] += (0..6)
                    .map(|j| m[i * 6 + j] * solved[(g * 6 + j, 0)])
                    .sum::<f64>();
            }
        });
        project(&self.projectors, &mut x);
        for _ in 0..2 {
            self.smooth(&b, &mut x, &mut ax);
        }
        x
    }
}
