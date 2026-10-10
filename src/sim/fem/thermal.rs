use super::scalar::{Csr, direct, two_level};
use super::{GAUSS, Quadratic, map, shape, triangle_area_vector, triangle_gauss, triangle_shape};
use anyhow::{Result, anyhow, bail};

pub const DIRECT_LIMIT: usize = 150_000;
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
        label: String,
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
    pub iterations: Option<usize>,
}

fn faces_of<'a>(mesh: &'a Quadratic, faces: &'a [usize]) -> impl Iterator<Item = [u32; 6]> + 'a {
    mesh.boundary
        .iter()
        .filter(move |(_, tag)| faces.contains(tag))
        .map(|(f, _)| *f)
}

pub fn solve(
    mesh: &Quadratic,
    conductivity: &[f64],
    boundaries: &[Boundary],
    source: f64,
) -> Result<Answer> {
    let n = mesh.nodes.len();
    let mut entries: Vec<(u32, u32, f64)> = (0..mesh.tets.len())
        .into_par_iter()
        .flat_map_iter(|t| {
            let k = conductivity[mesh.body[t] as usize] * 1.0e-3;
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
    if !mesh.ties.is_empty() {
        let best = conductivity.iter().copied().fold(0.0, f64::max) * 1.0e-3;
        let penalty = best * super::elastic::spacing(mesh) * super::elastic::TIE;
        for tie in &mesh.ties {
            let (nodes, c) = (tie.group(), tie.coefficients());
            for a in 0..7 {
                for b in 0..7 {
                    entries.push((nodes[a], nodes[b], penalty * c[a] * c[b]));
                }
            }
        }
    }
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
            Boundary::Flux { faces, watts, .. } => {
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
    if count > 3 * super::elastic::DIRECT_FALLBACK {
        bail!("{count} unknowns is too many for the heat solver; use a larger `mesh` size");
    }
    let mut reduced_entries = Vec::with_capacity(entries.len());
    for &(r, c, value) in &entries {
        let (r, c) = (r as usize, c as usize);
        match (fixed[r], fixed[c]) {
            (None, None) => reduced_entries.push((index[r] as u32, index[c] as u32, value)),
            (None, Some(t)) => reduced_rhs[index[r]] -= value * t,
            _ => {}
        }
    }
    let mut temperature: Vec<f64> = fixed.iter().map(|f| f.unwrap_or(0.0)).collect();
    let mut iterations = None;
    if count > 0 {
        let matrix = Csr::from_triplets(count, &reduced_entries);
        drop(reduced_entries);
        let x = if count <= DIRECT_LIMIT {
            direct(&matrix, &reduced_rhs).map_err(|_| {
                anyhow!("the heat problem has no single answer; check its boundaries")
            })?
        } else {
            let coarse = mesh.coarse();
            let parents: Vec<[(u32, f64); 2]> = (0..n)
                .filter(|&v| fixed[v].is_none())
                .map(|v| coarse.parents[v])
                .collect();
            let clusters: Vec<Vec<u32>> = coarse
                .clusters
                .iter()
                .map(|c| {
                    c.iter()
                        .filter(|&&v| fixed[v as usize].is_none())
                        .map(|&v| index[v as usize] as u32)
                        .collect::<Vec<u32>>()
                })
                .filter(|c| c.len() > 1)
                .collect();
            let solved = two_level(
                &matrix,
                &reduced_rhs,
                &parents,
                coarse.nodes,
                &clusters,
                1.0e-10,
                2000,
            )
            .map_err(|_| anyhow!("the heat problem has no single answer; check its boundaries"))?;
            if solved.residual > 1.0e-8 {
                bail!(
                    "the heat solver did not settle (residual {:.1e} after {} iterations); try a different `mesh` size",
                    solved.residual,
                    solved.iterations
                );
            }
            iterations = Some(solved.iterations);
            solved.x
        };
        for v in 0..n {
            if fixed[v].is_none() {
                temperature[v] = x[index[v]];
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
        iterations,
    })
}
