use rayon::prelude::*;

pub type Block = [f64; 9];

pub const IDENTITY: Block = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

#[derive(Clone)]
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
        Blocks::pattern_with(nodes, elements, &[])
    }

    pub fn pattern_with<const K: usize>(
        nodes: usize,
        elements: &[[u32; K]],
        groups: &[[u32; 7]],
    ) -> Blocks {
        let mut neighbours: Vec<Vec<u32>> = vec![Vec::new(); nodes];
        for element in elements {
            for &a in element {
                neighbours[a as usize].extend_from_slice(element);
            }
        }
        for group in groups {
            for &a in group {
                neighbours[a as usize].extend_from_slice(group);
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

pub fn block_jacobi(a: &Blocks) -> Vec<Block> {
    (0..a.nodes())
        .into_par_iter()
        .map(|i| invert(&a.diagonal(i)))
        .collect()
}

fn smooth(inverse: &[Block], r: &[f64], z: &mut [f64], weight: f64) {
    z.par_chunks_mut(3).enumerate().for_each(|(i, out)| {
        let v = times(&inverse[i], [r[3 * i], r[3 * i + 1], r[3 * i + 2]]);
        out[0] += weight * v[0];
        out[1] += weight * v[1];
        out[2] += weight * v[2];
    });
}

pub struct Coarse {
    pub parents: Vec<[(u32, f64); 2]>,
    pub nodes: usize,
}

fn coarse_pattern(a: &Blocks, coarse: &Coarse) -> Blocks {
    let lists: Vec<Vec<u32>> = {
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); coarse.nodes];
        for i in 0..a.nodes() {
            for &(pa, wa) in &coarse.parents[i] {
                if wa == 0.0 {
                    continue;
                }
                for k in a.starts[i]..a.starts[i + 1] {
                    for &(pb, wb) in &coarse.parents[a.columns[k] as usize] {
                        if wb != 0.0 {
                            lists[pa as usize].push(pb);
                        }
                    }
                }
            }
        }
        lists
            .into_par_iter()
            .map(|mut l| {
                l.sort_unstable();
                l.dedup();
                l
            })
            .collect()
    };
    let mut starts = Vec::with_capacity(coarse.nodes + 1);
    let mut columns = Vec::new();
    starts.push(0);
    for list in &lists {
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

pub struct TwoLevel<'a> {
    a: &'a Blocks,
    projectors: &'a [Block],
    coarse: &'a Coarse,
    inverse: Vec<Block>,
    reduced: Reduced,
    llt: faer::sparse::linalg::solvers::Llt<usize, f64>,
}

impl<'a> TwoLevel<'a> {
    pub fn new(a: &'a Blocks, projectors: &'a [Block], coarse: &'a Coarse) -> Result<Self, String> {
        let inverse = block_jacobi(a);
        let mut small = coarse_pattern(a, coarse);
        let projected = |i: usize, m: &Block, j: usize| -> Block {
            let (pi, pj) = (&projectors[i], &projectors[j]);
            let mut left = [0.0; 9];
            for r in 0..3 {
                for c in 0..3 {
                    left[r * 3 + c] = (0..3).map(|k| pi[r * 3 + k] * m[k * 3 + c]).sum();
                }
            }
            let mut out = [0.0; 9];
            for r in 0..3 {
                for c in 0..3 {
                    out[r * 3 + c] = (0..3).map(|k| left[r * 3 + k] * pj[k * 3 + c]).sum();
                }
            }
            out
        };
        for i in 0..a.nodes() {
            for k in a.starts[i]..a.starts[i + 1] {
                let j = a.columns[k] as usize;
                let block = projected(i, &a.values[k], j);
                for &(pa, wa) in &coarse.parents[i] {
                    if wa == 0.0 {
                        continue;
                    }
                    for &(pb, wb) in &coarse.parents[j] {
                        if wb == 0.0 {
                            continue;
                        }
                        let at = small.find(pa, pb);
                        let target = &mut small.values[at];
                        for (t, v) in target.iter_mut().zip(&block) {
                            *t += wa * wb * v;
                        }
                    }
                }
            }
        }
        let mut coarse_projectors = vec![IDENTITY; coarse.nodes];
        for (i, parents) in coarse.parents.iter().enumerate() {
            if parents[1].1 == 0.0 && parents[0].1 == 1.0 {
                coarse_projectors[parents[0].0 as usize] = projectors[i];
            }
        }
        let reduced = Reduced::new(&coarse_projectors);
        let llt = reduced.factor(&reduced.lower(&small, None))?;
        Ok(TwoLevel {
            a,
            projectors,
            coarse,
            inverse,
            reduced,
            llt,
        })
    }

    fn restrict(&self, r: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; self.coarse.nodes * 3];
        for (i, parents) in self.coarse.parents.iter().enumerate() {
            for &(p, w) in parents {
                if w != 0.0 {
                    for d in 0..3 {
                        out[3 * p as usize + d] += w * r[3 * i + d];
                    }
                }
            }
        }
        out
    }

    fn prolong(&self, c: &[f64], fine: &mut [f64]) {
        fine.par_chunks_mut(3).enumerate().for_each(|(i, out)| {
            for &(p, w) in &self.coarse.parents[i] {
                if w != 0.0 {
                    for d in 0..3 {
                        out[d] += w * c[3 * p as usize + d];
                    }
                }
            }
        });
    }

    fn sweep(&self, rr: &[f64], z: &mut [f64], ax: &mut [f64], residual: &mut [f64]) {
        self.a.multiply(z, ax);
        project(self.projectors, ax);
        residual
            .par_iter_mut()
            .zip(rr)
            .zip(ax.par_iter())
            .for_each(|((o, a), b)| *o = a - b);
    }

    pub fn apply(&self, r: &[f64], z: &mut [f64]) {
        use faer::linalg::solvers::Solve;
        let n = r.len();
        let weight = 0.6;
        z.iter_mut().for_each(|v| *v = 0.0);
        let mut rr = r.to_vec();
        project(self.projectors, &mut rr);
        let mut ax = vec![0.0; n];
        let mut residual = vec![0.0; n];
        for _ in 0..2 {
            self.sweep(&rr, z, &mut ax, &mut residual);
            smooth(&self.inverse, &residual, z, weight);
            project(self.projectors, z);
        }
        self.sweep(&rr, z, &mut ax, &mut residual);
        let reduced = &self.reduced;
        let coarse_rhs = reduced.restrict(&self.restrict(&residual));
        let solved = self
            .llt
            .solve(faer::Mat::<f64>::from_fn(reduced.count, 1, |i, _| {
                coarse_rhs[i]
            }));
        let correction = reduced.expand(
            &(0..reduced.count)
                .map(|i| solved[(i, 0)])
                .collect::<Vec<_>>(),
        );
        self.prolong(&correction, z);
        project(self.projectors, z);
        for _ in 0..2 {
            self.sweep(&rr, z, &mut ax, &mut residual);
            smooth(&self.inverse, &residual, z, weight);
            project(self.projectors, z);
        }
    }
}

pub fn two_level(
    a: &Blocks,
    b: &[f64],
    projectors: &[Block],
    coarse: &Coarse,
    tolerance: f64,
    limit: usize,
) -> Result<Solution, String> {
    let level = TwoLevel::new(a, projectors, coarse)?;
    Ok(preconditioned(
        a,
        b,
        projectors,
        tolerance,
        limit,
        &|r: &[f64], z: &mut [f64]| level.apply(r, z),
    ))
}

fn preconditioned(
    a: &Blocks,
    b: &[f64],
    projectors: &[Block],
    tolerance: f64,
    limit: usize,
    precondition: &(dyn Fn(&[f64], &mut [f64]) + Sync),
) -> Solution {
    let n = b.len();
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

pub struct Reduced {
    pub bases: Vec<Vec<[f64; 3]>>,
    pub first: Vec<usize>,
    pub count: usize,
}

impl Reduced {
    pub fn new(projectors: &[Block]) -> Reduced {
        let bases: Vec<Vec<[f64; 3]>> = projectors.iter().map(basis).collect();
        let mut first = Vec::with_capacity(bases.len());
        let mut count = 0;
        for basis in &bases {
            first.push(count);
            count += basis.len();
        }
        Reduced {
            bases,
            first,
            count,
        }
    }

    pub fn lower(
        &self,
        a: &Blocks,
        extra: Option<(&Blocks, f64)>,
    ) -> Vec<faer::sparse::Triplet<usize, usize, f64>> {
        let part = |m: &Blocks, scale: f64| -> Vec<faer::sparse::Triplet<usize, usize, f64>> {
            (0..m.nodes())
                .into_par_iter()
                .flat_map_iter(|row| {
                    let mut out = Vec::new();
                    let ti = &self.bases[row];
                    for k in m.starts[row]..m.starts[row + 1] {
                        let col = m.columns[k] as usize;
                        if col > row {
                            continue;
                        }
                        let tj = &self.bases[col];
                        let block = &m.values[k];
                        for (p, u) in ti.iter().enumerate() {
                            for (q, w) in tj.iter().enumerate() {
                                let r = self.first[row] + p;
                                let c = self.first[col] + q;
                                if c > r {
                                    continue;
                                }
                                let mw = times(block, *w);
                                let v = u[0] * mw[0] + u[1] * mw[1] + u[2] * mw[2];
                                out.push(faer::sparse::Triplet::new(r, c, v * scale));
                            }
                        }
                    }
                    out
                })
                .collect()
        };
        let mut triplets = part(a, 1.0);
        if let Some((m, scale)) = extra {
            triplets.extend(part(m, scale));
        }
        triplets
    }

    pub fn restrict(&self, full: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; self.count];
        for (node, basis) in self.bases.iter().enumerate() {
            for (p, u) in basis.iter().enumerate() {
                out[self.first[node] + p] =
                    u[0] * full[3 * node] + u[1] * full[3 * node + 1] + u[2] * full[3 * node + 2];
            }
        }
        out
    }

    pub fn expand(&self, reduced: &[f64]) -> Vec<f64> {
        let mut x = vec![0.0; self.bases.len() * 3];
        for (node, basis) in self.bases.iter().enumerate() {
            for (p, u) in basis.iter().enumerate() {
                let v = reduced[self.first[node] + p];
                for i in 0..3 {
                    x[3 * node + i] += u[i] * v;
                }
            }
        }
        x
    }

    pub fn factor(
        &self,
        triplets: &[faer::sparse::Triplet<usize, usize, f64>],
    ) -> Result<faer::sparse::linalg::solvers::Llt<usize, f64>, String> {
        let matrix = faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(
            self.count, self.count, triplets,
        )
        .map_err(|e| format!("{e:?}"))?;
        faer::set_global_parallelism(faer::Par::Seq);
        matrix
            .sp_cholesky(faer::Side::Lower)
            .map_err(|_| "the part can still move or turn; add supports that hold it".to_string())
    }
}

pub fn direct(a: &Blocks, b: &[f64], projectors: &[Block]) -> Result<Solution, String> {
    use faer::linalg::solvers::Solve;
    let reduced = Reduced::new(projectors);
    if reduced.count == 0 {
        return Ok(Solution {
            x: vec![0.0; b.len()],
            iterations: 0,
            residual: 0.0,
        });
    }
    let llt = reduced.factor(&reduced.lower(a, None))?;
    let rhs = reduced.restrict(b);
    let rhs = faer::Mat::<f64>::from_fn(reduced.count, 1, |i, _| rhs[i]);
    let solved = llt.solve(&rhs);
    let x = reduced.expand(
        &(0..reduced.count)
            .map(|i| solved[(i, 0)])
            .collect::<Vec<_>>(),
    );
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
