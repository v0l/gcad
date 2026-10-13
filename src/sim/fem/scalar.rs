use rayon::prelude::*;

pub struct Csr {
    pub starts: Vec<usize>,
    pub columns: Vec<u32>,
    pub values: Vec<f64>,
}

fn merged(mut row: Vec<(u32, f64)>) -> Vec<(u32, f64)> {
    row.sort_unstable_by_key(|e| e.0);
    let mut out: Vec<(u32, f64)> = Vec::with_capacity(row.len());
    for (c, v) in row {
        match out.last_mut() {
            Some(last) if last.0 == c => last.1 += v,
            _ => out.push((c, v)),
        }
    }
    out
}

impl Csr {
    fn from_rows(rows: Vec<Vec<(u32, f64)>>) -> Csr {
        let mut starts = Vec::with_capacity(rows.len() + 1);
        starts.push(0);
        let total = rows.iter().map(Vec::len).sum();
        let mut columns = Vec::with_capacity(total);
        let mut values = Vec::with_capacity(total);
        for row in rows {
            for (c, v) in row {
                columns.push(c);
                values.push(v);
            }
            starts.push(columns.len());
        }
        Csr {
            starts,
            columns,
            values,
        }
    }

    pub fn from_triplets(n: usize, triplets: &[(u32, u32, f64)]) -> Csr {
        let mut rows: Vec<Vec<(u32, f64)>> = vec![Vec::new(); n];
        for &(r, c, v) in triplets {
            rows[r as usize].push((c, v));
        }
        Csr::from_rows(rows.into_par_iter().map(merged).collect())
    }

    pub fn rows(&self) -> usize {
        self.starts.len() - 1
    }

    pub fn row(&self, i: usize) -> impl Iterator<Item = (usize, f64)> + '_ {
        (self.starts[i]..self.starts[i + 1]).map(|k| (self.columns[k] as usize, self.values[k]))
    }

    pub fn multiply(&self, x: &[f64], y: &mut [f64]) {
        y.par_iter_mut().enumerate().for_each(|(i, out)| {
            *out = self.row(i).map(|(c, v)| v * x[c]).sum();
        });
    }

    fn diagonal(&self) -> Vec<f64> {
        (0..self.rows())
            .into_par_iter()
            .map(|i| self.row(i).find(|(c, _)| *c == i).map_or(0.0, |(_, v)| v))
            .collect()
    }

    fn lower(&self) -> Vec<faer::sparse::Triplet<usize, usize, f64>> {
        (0..self.rows())
            .flat_map(|i| {
                self.row(i)
                    .filter(move |(c, _)| *c <= i)
                    .map(move |(c, v)| faer::sparse::Triplet::new(i, c, v))
            })
            .collect()
    }
}

pub struct Solved {
    pub x: Vec<f64>,
    pub iterations: usize,
    pub residual: f64,
}

pub fn factor(a: &Csr) -> Result<faer::sparse::linalg::solvers::Llt<usize, f64>, String> {
    let n = a.rows();
    let matrix = faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(n, n, &a.lower())
        .map_err(|e| format!("{e:?}"))?;
    faer::set_global_parallelism(faer::Par::Seq);
    matrix
        .sp_cholesky(faer::Side::Lower)
        .map_err(|_| "the problem has no single answer".to_string())
}

pub fn direct(a: &Csr, b: &[f64]) -> Result<Vec<f64>, String> {
    use faer::linalg::solvers::Solve;
    let _stage = crate::progress::stage("factoring", None, "");
    let llt = factor(a)?;
    let x = llt.solve(faer::Mat::<f64>::from_fn(b.len(), 1, |i, _| b[i]));
    Ok((0..b.len()).map(|i| x[(i, 0)]).collect())
}

fn galerkin(a: &Csr, parents: &[[(u32, f64); 2]], count: usize) -> Csr {
    let ap: Vec<Vec<(u32, f64)>> = (0..a.rows())
        .into_par_iter()
        .map(|i| {
            merged(
                a.row(i)
                    .flat_map(|(j, v)| {
                        parents[j]
                            .iter()
                            .filter(|p| p.1 != 0.0)
                            .map(move |&(p, w)| (p, v * w))
                    })
                    .collect(),
            )
        })
        .collect();
    let mut children: Vec<Vec<(u32, f64)>> = vec![Vec::new(); count];
    for (i, list) in parents.iter().enumerate() {
        for &(p, w) in list.iter().filter(|p| p.1 != 0.0) {
            children[p as usize].push((i as u32, w));
        }
    }
    let rows: Vec<Vec<(u32, f64)>> = children
        .into_par_iter()
        .enumerate()
        .map(|(p, kids)| {
            let mut row: Vec<(u32, f64)> = kids
                .iter()
                .flat_map(|&(i, w)| ap[i as usize].iter().map(move |&(c, v)| (c, v * w)))
                .collect();
            if row.is_empty() {
                row.push((p as u32, 1.0));
            }
            merged(row)
        })
        .collect();
    Csr::from_rows(rows)
}

#[allow(clippy::too_many_arguments)]
pub fn two_level(
    a: &Csr,
    b: &[f64],
    parents: &[[(u32, f64); 2]],
    count: usize,
    clusters: &[Vec<u32>],
    tolerance: f64,
    limit: usize,
) -> Result<Solved, String> {
    use faer::linalg::solvers::Solve;
    let n = b.len();
    let mut inverse: Vec<f64> = a
        .diagonal()
        .iter()
        .map(|d| if *d != 0.0 { 1.0 / d } else { 0.0 })
        .collect();
    let groups: Vec<(Vec<u32>, Vec<f64>)> = clusters
        .par_iter()
        .filter_map(|nodes| {
            let k = nodes.len();
            let mut dense = vec![0.0; k * k];
            for (i, &u) in nodes.iter().enumerate() {
                for (c, v) in a.row(u as usize) {
                    if let Some(j) = nodes.iter().position(|&w| w as usize == c) {
                        dense[i * k + j] = v;
                    }
                }
            }
            Some((nodes.clone(), super::solve::dense_inverse(dense, k)?))
        })
        .collect();
    for (nodes, _) in &groups {
        for &v in nodes {
            inverse[v as usize] = 0.0;
        }
    }
    let llt = factor(&galerkin(a, parents, count))?;
    let weight = 0.6;
    let smooth = |r: &[f64], z: &mut [f64], az: &mut [f64]| {
        a.multiply(z, az);
        let moves: Vec<Vec<f64>> = groups
            .par_iter()
            .map(|(nodes, inv)| {
                let k = nodes.len();
                (0..k)
                    .map(|i| {
                        (0..k)
                            .map(|j| {
                                let v = nodes[j] as usize;
                                inv[i * k + j] * (r[v] - az[v])
                            })
                            .sum()
                    })
                    .collect()
            })
            .collect();
        z.par_iter_mut()
            .enumerate()
            .for_each(|(i, zi)| *zi += weight * inverse[i] * (r[i] - az[i]));
        for ((nodes, _), delta) in groups.iter().zip(moves) {
            for (&v, d) in nodes.iter().zip(delta) {
                z[v as usize] += weight * d;
            }
        }
    };
    let precondition = |r: &[f64], z: &mut [f64]| {
        z.iter_mut().for_each(|v| *v = 0.0);
        let mut az = vec![0.0; n];
        for _ in 0..2 {
            smooth(r, z, &mut az);
        }
        a.multiply(z, &mut az);
        let mut coarse = vec![0.0; count];
        for (i, list) in parents.iter().enumerate() {
            for &(p, w) in list {
                coarse[p as usize] += w * (r[i] - az[i]);
            }
        }
        let solved = llt.solve(faer::Mat::<f64>::from_fn(count, 1, |i, _| coarse[i]));
        z.par_iter_mut().enumerate().for_each(|(i, zi)| {
            *zi += parents[i]
                .iter()
                .map(|&(p, w)| w * solved[(p as usize, 0)])
                .sum::<f64>();
        });
        for _ in 0..2 {
            smooth(r, z, &mut az);
        }
    };
    let dot = |x: &[f64], y: &[f64]| x.par_iter().zip(y).map(|(p, q)| p * q).sum::<f64>();
    let mut x = vec![0.0; n];
    let mut r = b.to_vec();
    let norm = dot(&r, &r).sqrt();
    if norm == 0.0 {
        return Ok(Solved {
            x,
            iterations: 0,
            residual: 0.0,
        });
    }
    let mut z = vec![0.0; n];
    precondition(&r, &mut z);
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    let mut q = vec![0.0; n];
    let mut iterations = 0;
    let mut residual = 1.0;
    let mut best = 1.0f64;
    let stage = crate::progress::stage("solving", None, "iterations");
    while iterations < limit {
        a.multiply(&p, &mut q);
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
        best = best.min(residual);
        stage.fraction(crate::progress::residual_fraction(1.0, best, tolerance));
        stage.progress(iterations as f64, || {
            format!("residual {residual:.1e}, aiming for {tolerance:.0e}")
        });
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
    Ok(Solved {
        x,
        iterations,
        residual,
    })
}
