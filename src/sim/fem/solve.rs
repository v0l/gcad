use rayon::prelude::*;

pub type Block = [f64; 9];

pub const IDENTITY: Block = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

pub struct Blocks {
    pub starts: Vec<usize>,
    pub columns: Vec<u32>,
    pub values: Vec<Block>,
}

fn times(m: &Block, v: [f64; 3]) -> [f64; 3] {
    [
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    ]
}

pub fn invert(m: &Block) -> Block {
    let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6]);
    if det.abs() < 1.0e-300 {
        return [0.0; 9];
    }
    let d = 1.0 / det;
    [
        (m[4] * m[8] - m[5] * m[7]) * d,
        (m[2] * m[7] - m[1] * m[8]) * d,
        (m[1] * m[5] - m[2] * m[4]) * d,
        (m[5] * m[6] - m[3] * m[8]) * d,
        (m[0] * m[8] - m[2] * m[6]) * d,
        (m[2] * m[3] - m[0] * m[5]) * d,
        (m[3] * m[7] - m[4] * m[6]) * d,
        (m[1] * m[6] - m[0] * m[7]) * d,
        (m[0] * m[4] - m[1] * m[3]) * d,
    ]
}

impl Blocks {
    pub fn pattern<const K: usize>(nodes: usize, elements: &[[u32; K]]) -> Blocks {
        let mut neighbours: Vec<Vec<u32>> = vec![Vec::new(); nodes];
        for element in elements {
            for &a in element {
                neighbours[a as usize].extend_from_slice(element);
            }
        }
        let mut starts = Vec::with_capacity(nodes + 1);
        let mut columns = Vec::new();
        starts.push(0);
        for list in &mut neighbours {
            list.sort_unstable();
            list.dedup();
            columns.extend_from_slice(list);
            starts.push(columns.len());
        }
        let values = vec![[0.0; 9]; columns.len()];
        Blocks {
            starts,
            columns,
            values,
        }
    }

    pub fn nodes(&self) -> usize {
        self.starts.len() - 1
    }

    pub fn find(&self, row: u32, column: u32) -> usize {
        let range = self.starts[row as usize]..self.starts[row as usize + 1];
        let at = self.columns[range.clone()]
            .binary_search(&column)
            .expect("the pattern holds every element pair");
        range.start + at
    }

    pub fn diagonal(&self, row: usize) -> Block {
        self.values[self.find(row as u32, row as u32)]
    }

    pub fn multiply(&self, x: &[f64], y: &mut [f64]) {
        y.par_chunks_mut(3).enumerate().for_each(|(row, out)| {
            let mut sum = [0.0; 3];
            for k in self.starts[row]..self.starts[row + 1] {
                let c = self.columns[k] as usize;
                let v = times(&self.values[k], [x[3 * c], x[3 * c + 1], x[3 * c + 2]]);
                sum[0] += v[0];
                sum[1] += v[1];
                sum[2] += v[2];
            }
            out.copy_from_slice(&sum);
        });
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.par_iter().zip(b).map(|(x, y)| x * y).sum()
}

fn project(projectors: &[Block], v: &mut [f64]) {
    v.par_chunks_mut(3).zip(projectors).for_each(|(c, p)| {
        let r = times(p, [c[0], c[1], c[2]]);
        c.copy_from_slice(&r);
    });
}

pub struct Solution {
    pub x: Vec<f64>,
    pub iterations: usize,
    pub residual: f64,
}

pub fn conjugate_gradient(
    a: &Blocks,
    b: &[f64],
    projectors: &[Block],
    tolerance: f64,
    limit: usize,
) -> Solution {
    let n = b.len();
    let inverse: Vec<Block> = (0..a.nodes())
        .into_par_iter()
        .map(|i| invert(&a.diagonal(i)))
        .collect();
    let precondition = |r: &[f64], z: &mut [f64]| {
        z.par_chunks_mut(3).enumerate().for_each(|(i, out)| {
            let v = times(&inverse[i], [r[3 * i], r[3 * i + 1], r[3 * i + 2]]);
            out.copy_from_slice(&v);
        });
        project(projectors, z);
    };
    let mut x = vec![0.0; n];
    let mut r = b.to_vec();
    project(projectors, &mut r);
    let norm = dot(&r, &r).sqrt();
    if norm == 0.0 {
        return Solution {
            x,
            iterations: 0,
            residual: 0.0,
        };
    }
    let mut z = vec![0.0; n];
    precondition(&r, &mut z);
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    let mut q = vec![0.0; n];
    let mut iterations = 0;
    let mut residual = 1.0;
    while iterations < limit {
        a.multiply(&p, &mut q);
        project(projectors, &mut q);
        let pq = dot(&p, &q);
        if pq <= 0.0 {
            break;
        }
        let alpha = rz / pq;
        x.par_iter_mut()
            .zip(&p)
            .for_each(|(xi, pi)| *xi += alpha * pi);
        r.par_iter_mut()
            .zip(&q)
            .for_each(|(ri, qi)| *ri -= alpha * qi);
        iterations += 1;
        residual = dot(&r, &r).sqrt() / norm;
        if residual < tolerance {
            break;
        }
        precondition(&r, &mut z);
        let next = dot(&r, &z);
        let beta = next / rz;
        rz = next;
        p.par_iter_mut()
            .zip(&z)
            .for_each(|(pi, zi)| *pi = zi + beta * *pi);
    }
    Solution {
        x,
        iterations,
        residual,
    }
}

fn basis(p: &Block) -> Vec<[f64; 3]> {
    let mut kept: Vec<[f64; 3]> = Vec::new();
    for axis in 0..3 {
        let mut v = [p[axis], p[3 + axis], p[6 + axis]];
        for k in &kept {
            let d = v[0] * k[0] + v[1] * k[1] + v[2] * k[2];
            for i in 0..3 {
                v[i] -= d * k[i];
            }
        }
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if n > 0.5 {
            kept.push(v.map(|x| x / n));
        }
    }
    kept
}

pub fn direct(a: &Blocks, b: &[f64], projectors: &[Block]) -> Result<Solution, String> {
    use faer::linalg::solvers::Solve;
    let bases: Vec<Vec<[f64; 3]>> = projectors.iter().map(basis).collect();
    let mut first = Vec::with_capacity(bases.len());
    let mut count = 0;
    for basis in &bases {
        first.push(count);
        count += basis.len();
    }
    if count == 0 {
        return Ok(Solution {
            x: vec![0.0; b.len()],
            iterations: 0,
            residual: 0.0,
        });
    }
    let triplets: Vec<faer::sparse::Triplet<usize, usize, f64>> = (0..a.nodes())
        .into_par_iter()
        .flat_map_iter(|row| {
            let mut out = Vec::new();
            let ti = &bases[row];
            for k in a.starts[row]..a.starts[row + 1] {
                let col = a.columns[k] as usize;
                if col > row {
                    continue;
                }
                let tj = &bases[col];
                let m = &a.values[k];
                for (p, u) in ti.iter().enumerate() {
                    for (q, w) in tj.iter().enumerate() {
                        let r = first[row] + p;
                        let c = first[col] + q;
                        if c > r {
                            continue;
                        }
                        let mw = times(m, *w);
                        let v = u[0] * mw[0] + u[1] * mw[1] + u[2] * mw[2];
                        out.push(faer::sparse::Triplet::new(r, c, v));
                    }
                }
            }
            out
        })
        .collect();
    let matrix =
        faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(count, count, &triplets)
            .map_err(|e| format!("{e:?}"))?;
    faer::set_global_parallelism(faer::Par::Seq);
    let llt = matrix
        .sp_cholesky(faer::Side::Lower)
        .map_err(|_| "the part can still move or turn; add supports that hold it".to_string())?;
    let rhs = faer::Mat::<f64>::from_fn(count, 1, |i, _| {
        let node = first.partition_point(|&f| f <= i) - 1;
        let u = bases[node][i - first[node]];
        u[0] * b[3 * node] + u[1] * b[3 * node + 1] + u[2] * b[3 * node + 2]
    });
    let reduced = llt.solve(&rhs);
    let mut x = vec![0.0; b.len()];
    for (node, basis) in bases.iter().enumerate() {
        for (p, u) in basis.iter().enumerate() {
            let v = reduced[(first[node] + p, 0)];
            for i in 0..3 {
                x[3 * node + i] += u[i] * v;
            }
        }
    }
    let mut ax = vec![0.0; b.len()];
    a.multiply(&x, &mut ax);
    let mut r: Vec<f64> = ax.iter().zip(b).map(|(p, q)| q - p).collect();
    project(projectors, &mut r);
    let mut pb = b.to_vec();
    project(projectors, &mut pb);
    let norm = dot(&pb, &pb).sqrt().max(1.0e-300);
    Ok(Solution {
        x,
        iterations: 1,
        residual: dot(&r, &r).sqrt() / norm,
    })
}
