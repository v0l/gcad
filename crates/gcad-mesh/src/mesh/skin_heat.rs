use super::rig::{Rig, Weights, trim};
use super::skin_smooth::{PRUNE, SmoothOptions, core_bone, dense, finish, soften_steps, untangle};
use super::{Mesh, cross, dot, length, sub};

pub const HEAT: f64 = 1.0;
pub const CUT: f64 = 0.05;
pub const SNAP: f64 = 0.8;
const TIE: f64 = 1.0001;
const TOLERANCE: f64 = 1.0e-3;
const MAX_ITERATIONS: usize = 4000;

pub struct Laplacian {
    pub diagonal: Vec<f64>,
    pub rows: Vec<Vec<(u32, f64)>>,
    pub area: Vec<f64>,
}

fn cot(a: super::P3, b: super::P3, c: super::P3) -> f64 {
    let u = sub(b, a);
    let v = sub(c, a);
    let s = length(cross(u, v));
    if s < 1.0e-18 { 0.0 } else { dot(u, v) / s }
}

pub fn laplacian(mesh: &Mesh) -> Laplacian {
    let n = mesh.positions.len();
    let mut rows: Vec<Vec<(u32, f64)>> = vec![Vec::new(); n];
    let mut area = vec![0.0; n];
    for t in &mesh.triangles {
        let [a, b, c] = mesh.corners(*t);
        let third = length(cross(sub(b, a), sub(c, a))) / 6.0;
        for k in 0..3 {
            area[t[k] as usize] += third;
            let (i, j, o) = (t[(k + 1) % 3], t[(k + 2) % 3], t[k]);
            let w = 0.5
                * cot(
                    mesh.positions[o as usize],
                    mesh.positions[i as usize],
                    mesh.positions[j as usize],
                );
            rows[i as usize].push((j, w));
            rows[j as usize].push((i, w));
        }
    }
    let mut diagonal = vec![0.0; n];
    for (i, row) in rows.iter_mut().enumerate() {
        row.sort_unstable_by_key(|(j, _)| *j);
        let mut merged: Vec<(u32, f64)> = Vec::with_capacity(row.len());
        for &(j, w) in row.iter() {
            match merged.last_mut() {
                Some((k, s)) if *k == j => *s += w,
                _ => merged.push((j, w)),
            }
        }
        for (_, w) in &mut merged {
            *w = w.max(0.0);
        }
        diagonal[i] = merged.iter().map(|(_, w)| w).sum();
        *row = merged;
    }
    Laplacian {
        diagonal,
        rows,
        area,
    }
}

struct Csr {
    start: Vec<u32>,
    column: Vec<u32>,
    weight: Vec<f64>,
    diagonal: Vec<f64>,
}

impl Csr {
    fn of(l: &Laplacian, h: &[f64]) -> Csr {
        let mut start = Vec::with_capacity(l.rows.len() + 1);
        let mut column = Vec::new();
        let mut weight = Vec::new();
        start.push(0);
        for row in &l.rows {
            for &(j, w) in row {
                column.push(j);
                weight.push(w);
            }
            start.push(column.len() as u32);
        }
        Csr {
            start,
            column,
            weight,
            diagonal: l.diagonal.iter().zip(h).map(|(d, h)| d + h).collect(),
        }
    }

    fn multiply(&self, k: usize, x: &[f64], out: &mut [f64]) {
        let row = |(i, o): (usize, &mut [f64])| {
            let d = self.diagonal[i];
            for c in 0..k {
                o[c] = d * x[i * k + c];
            }
            for e in self.start[i] as usize..self.start[i + 1] as usize {
                let j = self.column[e] as usize;
                let w = self.weight[e];
                for c in 0..k {
                    o[c] -= w * x[j * k + c];
                }
            }
        };
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            out.par_chunks_mut(k).enumerate().for_each(row);
        }
        #[cfg(not(feature = "parallel"))]
        out.chunks_mut(k).enumerate().for_each(row);
    }
}

pub fn solve_block(
    l: &Laplacian,
    h: &[f64],
    k: usize,
    rhs: &[f64],
    start: Vec<f64>,
) -> (Vec<f64>, usize) {
    let a = Csr::of(l, h);
    let n = a.diagonal.len();
    let pre: Vec<f64> = a
        .diagonal
        .iter()
        .map(|&d| if d > 0.0 { 1.0 / d } else { 1.0 })
        .collect();
    let mut x = start;
    let mut ax = vec![0.0; n * k];
    a.multiply(k, &x, &mut ax);
    let mut r: Vec<f64> = rhs.iter().zip(&ax).map(|(b, a)| b - a).collect();
    let mut z = vec![0.0; n * k];
    for i in 0..n {
        for c in 0..k {
            z[i * k + c] = r[i * k + c] * pre[i];
        }
    }
    let mut p = z.clone();
    let column_dot = |u: &[f64], v: &[f64]| -> Vec<f64> {
        let mut s = vec![0.0; k];
        for (a, b) in u.chunks_exact(k).zip(v.chunks_exact(k)) {
            for c in 0..k {
                s[c] += a[c] * b[c];
            }
        }
        s
    };
    let mut rz = column_dot(&r, &z);
    let target: Vec<f64> = column_dot(rhs, rhs)
        .iter()
        .map(|b| TOLERANCE * b.sqrt().max(1.0e-300))
        .collect();
    let mut done = vec![false; k];
    let mut ap = ax;
    for it in 0..MAX_ITERATIONS {
        let rr = column_dot(&r, &r);
        for c in 0..k {
            done[c] = done[c] || rr[c].sqrt() <= target[c];
        }
        if done.iter().all(|d| *d) {
            return (x, it);
        }
        a.multiply(k, &p, &mut ap);
        let pap = column_dot(&p, &ap);
        let alpha: Vec<f64> = (0..k)
            .map(|c| {
                if done[c] || pap[c] <= 0.0 {
                    0.0
                } else {
                    rz[c] / pap[c]
                }
            })
            .collect();
        for i in 0..n {
            for c in 0..k {
                let e = i * k + c;
                x[e] += alpha[c] * p[e];
                r[e] -= alpha[c] * ap[e];
                z[e] = r[e] * pre[i];
            }
        }
        let next = column_dot(&r, &z);
        let beta: Vec<f64> = (0..k)
            .map(|c| if rz[c] > 0.0 { next[c] / rz[c] } else { 0.0 })
            .collect();
        rz = next;
        for (pp, zz) in p.chunks_exact_mut(k).zip(z.chunks_exact(k)) {
            for c in 0..k {
                pp[c] = zz[c] + beta[c] * pp[c];
            }
        }
    }
    (x, MAX_ITERATIONS)
}

pub struct Sources {
    pub heat: Vec<f64>,
    pub nearest: Vec<Vec<usize>>,
}

pub fn sources(distances: &[Vec<f64>], area: &[f64], strength: f64) -> Sources {
    let mut heat = vec![0.0; distances.len()];
    let mut nearest = vec![Vec::new(); distances.len()];
    for (v, d) in distances.iter().enumerate() {
        let best = d
            .iter()
            .copied()
            .filter(|x| x.is_finite())
            .fold(f64::INFINITY, f64::min);
        if !best.is_finite() {
            continue;
        }
        nearest[v] = (0..d.len()).filter(|&b| d[b] <= best * TIE).collect();
        heat[v] = area[v] * strength / (best * best);
    }
    Sources { heat, nearest }
}

pub fn heat_weights(rig: &Rig, mesh: &Mesh, strength: f64) -> Vec<Vec<f64>> {
    heat_from(rig, mesh, &rig.bone_distances(mesh), strength)
}

pub fn heat_from(rig: &Rig, mesh: &Mesh, distances: &[Vec<f64>], strength: f64) -> Vec<Vec<f64>> {
    let l = laplacian(mesh);
    let s = sources(distances, &l.area, strength);
    let k = rig.bones.len();
    let n = mesh.positions.len();
    let mut rhs = vec![0.0; n * k];
    for v in 0..n {
        let near = &s.nearest[v];
        for &b in near {
            rhs[v * k + b] = s.heat[v] / near.len() as f64;
        }
    }
    let mut start = vec![0.0; n * k];
    for (v, d) in distances.iter().enumerate() {
        let near = d.iter().copied().fold(f64::INFINITY, f64::min);
        if !near.is_finite() {
            continue;
        }
        let row: Vec<f64> = d.iter().map(|&x| (near / x).powi(4)).collect();
        let sum: f64 = row.iter().sum();
        for b in 0..k {
            start[v * k + b] = row[b] / sum;
        }
    }
    let (x, _) = solve_block(&l, &s.heat, k, &rhs, start);
    (0..k)
        .map(|b| (0..n).map(|v| x[v * k + b]).collect())
        .collect()
}

pub fn sharpen(row: &mut [f64], cut: f64) {
    if cut <= 0.0 {
        return;
    }
    let span = (1.0 - 2.0 * cut).max(1.0e-9);
    for w in row.iter_mut() {
        *w = ((*w - cut) / span).clamp(0.0, 1.0);
    }
    let sum: f64 = row.iter().sum();
    if sum > 0.0 {
        row.iter_mut().for_each(|w| *w /= sum);
    }
}

pub fn heat_skin(rig: &mut Rig, mesh: &Mesh, strength: f64, cut: f64, unrelated: bool) {
    rig.skin = Some(heat_rows(
        rig,
        mesh,
        &heat_weights(rig, mesh, strength),
        cut,
        unrelated,
    ));
}

pub fn heat_rows(
    rig: &Rig,
    mesh: &Mesh,
    columns: &[Vec<f64>],
    cut: f64,
    unrelated: bool,
) -> Weights {
    let bones = rig.bones.len();
    (0..mesh.positions.len())
        .map(|v| {
            let mut row: Vec<f64> = (0..bones).map(|b| columns[b][v].max(0.0)).collect();
            if !unrelated {
                untangle(rig, &mut row);
            }
            sharpen(&mut row, cut);
            let mut w = trim(row.into_iter().enumerate().collect());
            w.retain(|(_, x)| *x >= PRUNE);
            let w = trim(w);
            if w.is_empty() { vec![(0, 1.0)] } else { w }
        })
        .collect()
}

pub fn heat_core_skin(
    rig: &mut Rig,
    mesh: &Mesh,
    strength: f64,
    cut: f64,
    snap: f64,
    options: SmoothOptions,
) {
    let distances = rig.bone_distances(mesh);
    let columns = heat_from(rig, mesh, &distances, strength);
    let skin = heat_rows(rig, mesh, &columns, cut, options.unrelated);
    let bones = rig.bones.len();
    let mut weights = dense(&skin, bones);
    let mut pinned = vec![false; skin.len()];
    for (v, w) in skin.iter().enumerate() {
        let Some(bone) = core_bone(rig, mesh, &distances, v, options.core, options.joint) else {
            continue;
        };
        if w.iter().any(|&(b, x)| b == bone && x >= snap) {
            pinned[v] = true;
            weights[v * bones..(v + 1) * bones].fill(0.0);
            weights[v * bones + bone] = 1.0;
        }
    }
    let rule = (!options.unrelated).then_some(&*rig);
    soften_steps(rule, &mut weights, bones, &mesh.neighbours(), &mut pinned);
    rig.skin = Some(finish(rig, &weights, bones, options.unrelated));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::rig::Bone;

    fn tube() -> Mesh {
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        let ring = 12u32;
        let rows = 41u32;
        for i in 0..rows {
            for k in 0..ring {
                let a = k as f64 / ring as f64 * std::f64::consts::TAU;
                positions.push([a.cos(), a.sin(), i as f64 * 0.5]);
            }
        }
        for i in 0..rows - 1 {
            for k in 0..ring {
                let a = i * ring + k;
                let b = i * ring + (k + 1) % ring;
                triangles.push([a, b, b + ring]);
                triangles.push([a, b + ring, a + ring]);
            }
        }
        Mesh {
            labels: vec![0; positions.len()],
            positions,
            triangles,
            parts: vec!["tube".into()],
            ..Default::default()
        }
    }

    #[test]
    fn the_laplacian_of_a_constant_is_zero() {
        let mesh = tube();
        let l = laplacian(&mesh);
        let ones = vec![1.0; mesh.positions.len()];
        let mut out = vec![0.0; ones.len()];
        Csr::of(&l, &vec![0.0; ones.len()]).multiply(1, &ones, &mut out);
        assert!(out.iter().all(|v| v.abs() < 1.0e-9));
        let total: f64 = l.area.iter().sum();
        assert!((total - mesh.area()).abs() < 1.0e-9);
    }

    #[test]
    fn heat_from_two_bones_sums_to_one_and_ramps_over_the_joint() {
        let mesh = tube();
        let rig = Rig {
            bones: vec![
                Bone {
                    name: "a".into(),
                    parent: None,
                    head: [0.0, 0.0, 0.0],
                    tail: [0.0, 0.0, 10.0],
                },
                Bone {
                    name: "b".into(),
                    parent: Some(0),
                    head: [0.0, 0.0, 10.0],
                    tail: [0.0, 0.0, 20.0],
                },
            ],
            ..Default::default()
        };
        let w = heat_weights(&rig, &mesh, HEAT);
        for v in 0..mesh.positions.len() {
            assert!((w[0][v] + w[1][v] - 1.0).abs() < 1.0e-4, "{v}");
        }
        let b_at = |row: usize| w[1][row * 12];
        assert!(b_at(2) < 0.01 && b_at(38) > 0.99);
        let ramp: Vec<f64> = (10..=30).map(b_at).collect();
        assert!(ramp.windows(2).all(|p| p[1] >= p[0] - 1.0e-6), "{ramp:?}");
        assert!((b_at(20) - 0.5).abs() < 0.05, "{}", b_at(20));
    }
}
