use super::{GAUSS, Quadratic, map, shape, triangle_area_vector, triangle_gauss, triangle_shape};
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;
use rayon::prelude::*;

#[derive(Clone, Debug)]
pub enum Boundary {
    Temperature {
        label: String,
        faces: Vec<usize>,
        value: f64,
    },
    Flux {
        faces: Vec<usize>,
        watts: f64,
    },
    Convect {
        label: String,
        faces: Vec<usize>,
        film: f64,
        ambient: f64,
    },
}

pub struct Answer {
    pub temperature: Vec<f64>,
    pub leaving: Vec<(String, f64)>,
}

fn faces_of<'a>(mesh: &'a Quadratic, faces: &'a [usize]) -> impl Iterator<Item = [u32; 6]> + 'a {
    mesh.boundary
        .iter()
        .filter(move |(_, tag)| faces.contains(tag))
        .map(|(f, _)| *f)
}

pub fn solve(
    mesh: &Quadratic,
    conductivity: f64,
    boundaries: &[Boundary],
    source: f64,
) -> Result<Answer> {
    let n = mesh.nodes.len();
    let k = conductivity * 1.0e-3;
    let mut entries: Vec<(u32, u32, f64)> = (0..mesh.tets.len())
        .into_par_iter()
        .flat_map_iter(|t| {
            let points = mesh.points(t);
            let tet = mesh.tets[t];
            let mut local = [[0.0; 10]; 10];
            for (l, w) in GAUSS {
                if let Some(m) = map(&points, l) {
                    for (row, ga) in local.iter_mut().zip(&m.gradients) {
                        for (cell, gb) in row.iter_mut().zip(&m.gradients) {
                            *cell += k * ga.dot(*gb) * m.det * w;
                        }
                    }
                }
            }
            (0..100).map(move |i| (tet[i / 10], tet[i % 10], local[i / 10][i % 10]))
        })
        .collect();
    let mut rhs = vec![0.0; n];
    let volume = mesh.volume();
    if source != 0.0 {
        for t in 0..mesh.tets.len() {
            let points = mesh.points(t);
            for (l, w) in GAUSS {
                if let Some(m) = map(&points, l) {
                    for (a, s) in shape(l).iter().enumerate() {
                        rhs[mesh.tets[t][a] as usize] += source / volume * s * m.det * w;
                    }
                }
            }
        }
    }
    let mut fixed: Vec<Option<f64>> = vec![None; n];
    let mut owner: Vec<Option<usize>> = vec![None; n];
    let mut held = false;
    for (b, boundary) in boundaries.iter().enumerate() {
        match boundary {
            Boundary::Temperature { faces, value, .. } => {
                for f in faces_of(mesh, faces) {
                    for v in f {
                        fixed[v as usize] = Some(*value);
                        owner[v as usize].get_or_insert(b);
                    }
                }
                held = true;
            }
            Boundary::Flux { faces, watts } => {
                let area = super::elastic::area(mesh, faces);
                if area <= 0.0 {
                    bail!("the heated faces have no area");
                }
                for f in faces_of(mesh, faces) {
                    let p = f.map(|v| mesh.nodes[v as usize]);
                    for (l, w) in triangle_gauss() {
                        let da = triangle_area_vector(&p, l).magnitude() * w;
                        for (a, s) in triangle_shape(l).iter().enumerate() {
                            rhs[f[a] as usize] += watts / area * s * da;
                        }
                    }
                }
            }
            Boundary::Convect {
                faces,
                film,
                ambient,
                ..
            } => {
                let h = film * 1.0e-6;
                for f in faces_of(mesh, faces) {
                    let p = f.map(|v| mesh.nodes[v as usize]);
                    for (l, w) in triangle_gauss() {
                        let da = triangle_area_vector(&p, l).magnitude() * w;
                        let s = triangle_shape(l);
                        for a in 0..6 {
                            rhs[f[a] as usize] += h * ambient * s[a] * da;
                            for b in 0..6 {
                                entries.push((f[a], f[b], h * s[a] * s[b] * da));
                            }
                        }
                    }
                }
                held = true;
            }
        }
    }
    if !held {
        bail!(
            "give the part a `temperature` or `convect` boundary, or its temperature is not fixed"
        );
    }
    let mut index = vec![usize::MAX; n];
    let mut count = 0;
    for v in 0..n {
        if fixed[v].is_none() {
            index[v] = count;
            count += 1;
        }
    }
    let mut reduced_rhs: Vec<f64> = (0..n)
        .filter(|&v| fixed[v].is_none())
        .map(|v| rhs[v])
        .collect();
    let mut triplets = Vec::new();
    for &(r, c, value) in &entries {
        let (r, c) = (r as usize, c as usize);
        match (fixed[r], fixed[c]) {
            (None, None) if index[c] <= index[r] => {
                triplets.push(faer::sparse::Triplet::new(index[r], index[c], value))
            }
            (None, Some(t)) => reduced_rhs[index[r]] -= value * t,
            _ => {}
        }
    }
    let mut temperature: Vec<f64> = fixed.iter().map(|f| f.unwrap_or(0.0)).collect();
    if count > 0 {
        use faer::linalg::solvers::Solve;
        let matrix = faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(
            count, count, &triplets,
        )
        .map_err(|e| anyhow!("{e:?}"))?;
        faer::set_global_parallelism(faer::Par::Seq);
        let llt = matrix
            .sp_cholesky(faer::Side::Lower)
            .map_err(|_| anyhow!("the heat problem has no single answer; check its boundaries"))?;
        let b = faer::Mat::<f64>::from_fn(count, 1, |i, _| reduced_rhs[i]);
        let x = llt.solve(&b);
        for v in 0..n {
            if fixed[v].is_none() {
                temperature[v] = x[(index[v], 0)];
            }
        }
    }
    let mut flow = vec![0.0; n];
    for &(r, c, value) in &entries {
        flow[r as usize] += value * temperature[c as usize];
    }
    let mut leaving: Vec<(String, f64)> = Vec::new();
    for (b, boundary) in boundaries.iter().enumerate() {
        match boundary {
            Boundary::Temperature { label, .. } => {
                let total: f64 = (0..n)
                    .filter(|&v| owner[v] == Some(b))
                    .map(|v| rhs[v] - flow[v])
                    .sum();
                leaving.push((label.clone(), total));
            }
            Boundary::Convect {
                label,
                faces,
                film,
                ambient,
            } => {
                let h = film * 1.0e-6;
                let mut total = 0.0;
                for f in faces_of(mesh, faces) {
                    let p = f.map(|v| mesh.nodes[v as usize]);
                    for (l, w) in triangle_gauss() {
                        let da = triangle_area_vector(&p, l).magnitude() * w;
                        let t: f64 = triangle_shape(l)
                            .iter()
                            .zip(f)
                            .map(|(s, v)| s * temperature[v as usize])
                            .sum();
                        total += h * (t - ambient) * da;
                    }
                }
                leaving.push((label.clone(), total));
            }
            Boundary::Flux { .. } => {}
        }
    }
    Ok(Answer {
        temperature,
        leaving,
    })
}
