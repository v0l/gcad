use super::Mesh;
use super::rig::{Rig, Weights, trim};
use super::{dot, sub};

pub const PRUNE: f64 = 0.01;
const PIN_MARGIN: f64 = 0.85;
const GAP_EDGES: f64 = 5.0;
const SOFT_TARGET: f64 = 0.8;
const SOFT_ROUNDS: usize = 30;
const SOFT_RINGS: usize = 1;
const SOFT_PASSES: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmoothOptions {
    pub power: f64,
    pub passes: usize,
    pub step: f64,
    pub core: f64,
    pub zone: f64,
    pub joint: f64,
    pub unrelated: bool,
}

impl Default for SmoothOptions {
    fn default() -> Self {
        SmoothOptions {
            power: 4.0,
            passes: 300,
            step: 1.0,
            core: 0.5,
            zone: 0.25,
            joint: super::skin_check::CORE_JOINT,
            unrelated: false,
        }
    }
}

pub fn family(rig: &Rig, bone: usize) -> Vec<usize> {
    let mut out = vec![bone];
    out.extend(rig.bones[bone].parent);
    out.extend(rig.children(bone));
    out
}

fn segment_t(p: super::P3, a: super::P3, b: super::P3) -> f64 {
    let ab = sub(b, a);
    let len2 = dot(ab, ab);
    if len2 < 1.0e-18 {
        return 0.0;
    }
    (dot(sub(p, a), ab) / len2).clamp(0.0, 1.0)
}

fn nearest(d: &[f64]) -> Option<usize> {
    d.iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .min_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, _)| i)
}

pub fn core_bone(
    rig: &Rig,
    mesh: &Mesh,
    distances: &[Vec<f64>],
    vertex: usize,
    core: f64,
    joint: f64,
) -> Option<usize> {
    if core <= 0.0 {
        return None;
    }
    let bone = nearest(&distances[vertex])?;
    let d = &distances[vertex];
    if d.iter().enumerate().any(|(i, &x)| {
        !super::skin_check::related(rig, i, bone) && d[bone] > super::skin_check::CORE_CLEAR * x
    }) {
        return None;
    }
    let b = &rig.bones[bone];
    let p = mesh.positions[vertex];
    let t = segment_t(p, b.head, b.tail);
    let len = super::length(sub(b.tail, b.head));
    let radius = d[bone];
    let clear = |along: f64| along >= joint * radius * PIN_MARGIN;
    let lo = if b.parent.is_none() {
        0.0
    } else {
        0.5 - core / 2.0
    };
    let hi = if rig.children(bone).is_empty() {
        1.0
    } else {
        0.5 + core / 2.0
    };
    let head_ok = b.parent.is_none() || clear(t * len);
    let tail_ok = rig.children(bone).is_empty() || clear((1.0 - t) * len);
    (t >= lo && t <= hi && head_ok && tail_ok).then_some(bone)
}

fn start_weights(rig: &Rig, d: &[f64], power: f64, unrelated: bool) -> Vec<(usize, f64)> {
    let Some(n) = nearest(d) else {
        return vec![(0, 1.0)];
    };
    let allowed = family(rig, n);
    let near = d[n];
    trim(
        d.iter()
            .enumerate()
            .filter(|(i, _)| unrelated || allowed.contains(i))
            .map(|(i, &v)| (i, (near / v).powf(power)))
            .filter(|(_, w)| *w > 1.0e-4)
            .collect(),
    )
}

pub fn dense(skin: &Weights, bones: usize) -> Vec<f64> {
    let mut out = vec![0.0; skin.len() * bones];
    for (v, w) in skin.iter().enumerate() {
        for &(b, x) in w {
            out[v * bones + b] += x;
        }
    }
    out
}

pub fn relax(
    rig: Option<&Rig>,
    dense: &mut [f64],
    bones: usize,
    around: &[Vec<u32>],
    fixed: &[bool],
    passes: usize,
    step: f64,
) {
    let mut mean = vec![0.0; bones];
    for _ in 0..passes {
        for (v, ring) in around.iter().enumerate() {
            if fixed[v] || ring.is_empty() {
                continue;
            }
            mean.fill(0.0);
            for &u in ring {
                let row = &dense[u as usize * bones..(u as usize + 1) * bones];
                mean.iter_mut().zip(row).for_each(|(m, x)| *m += x);
            }
            let k = 1.0 / ring.len() as f64;
            let own = &mut dense[v * bones..(v + 1) * bones];
            own.iter_mut()
                .zip(&mean)
                .for_each(|(x, m)| *x += step * (m * k - *x));
            if let Some(rig) = rig {
                untangle(rig, own);
            }
        }
    }
}

fn row_jump(dense: &[f64], bones: usize, a: usize, b: usize) -> f64 {
    (0..bones)
        .map(|k| (dense[a * bones + k] - dense[b * bones + k]).abs())
        .fold(0.0, f64::max)
}

pub fn soften_steps(
    rig: Option<&Rig>,
    dense: &mut [f64],
    bones: usize,
    around: &[Vec<u32>],
    pinned: &mut [bool],
) {
    let target = super::skin_check::JUMP_LIMIT * SOFT_TARGET;
    for round in 0..SOFT_ROUNDS {
        let mut free = vec![false; around.len()];
        let mut any = false;
        for (v, ring) in around.iter().enumerate() {
            if ring
                .iter()
                .any(|&u| row_jump(dense, bones, v, u as usize) > target)
            {
                free[v] = true;
                any = true;
            }
        }
        if !any {
            return;
        }
        for _ in 0..SOFT_RINGS {
            let grown: Vec<bool> = around
                .iter()
                .enumerate()
                .map(|(v, ring)| free[v] || ring.iter().any(|&u| free[u as usize]))
                .collect();
            free = grown;
        }
        if round >= SOFT_ROUNDS / 2 {
            for (p, f) in pinned.iter_mut().zip(&free) {
                if *f {
                    *p = false;
                }
            }
        }
        for _ in 0..SOFT_RINGS {
            let grown: Vec<bool> = around
                .iter()
                .enumerate()
                .map(|(v, ring)| free[v] || ring.iter().any(|&u| free[u as usize]))
                .collect();
            free = grown;
        }
        let fixed: Vec<bool> = free
            .iter()
            .zip(pinned.iter())
            .map(|(f, p)| !f || *p)
            .collect();
        relax(rig, dense, bones, around, &fixed, SOFT_PASSES, 1.0);
    }
}

fn ancestors(rig: &Rig, bone: usize) -> Vec<usize> {
    std::iter::successors(Some(bone), |&b| rig.bones[b].parent).collect()
}

fn link(rig: &Rig, a: usize, b: usize) -> usize {
    let up_a = ancestors(rig, a);
    let up_b = ancestors(rig, b);
    match up_a.iter().find(|x| up_b.contains(x)) {
        Some(&c) if c != a && c != b => c,
        Some(&c) => {
            let (top, path) = if c == a { (a, &up_b) } else { (b, &up_a) };
            let at = path.iter().position(|&x| x == top).unwrap_or(1);
            path[at.saturating_sub(1)]
        }
        None => {
            if up_a.len() <= up_b.len() {
                a
            } else {
                b
            }
        }
    }
}

pub fn untangle(rig: &Rig, row: &mut [f64]) {
    let mut live: Vec<usize> = Vec::with_capacity(8);
    for _ in 0..64 {
        live.clear();
        live.extend((0..row.len()).filter(|&b| row[b] > 1.0e-9));
        let mut worst: Option<(usize, usize, f64)> = None;
        for (i, &a) in live.iter().enumerate() {
            for &b in &live[i + 1..] {
                let m = row[a].min(row[b]);
                if !super::skin_check::related(rig, a, b) && worst.is_none_or(|w| m > w.2) {
                    worst = Some((a, b, m));
                }
            }
        }
        let Some((a, b, m)) = worst else {
            return;
        };
        let to = link(rig, a, b);
        row[a] -= m;
        row[b] -= m;
        row[to] += 2.0 * m;
    }
}

pub fn finish(rig: &Rig, dense: &[f64], bones: usize, unrelated: bool) -> Weights {
    dense
        .chunks(bones)
        .map(|row| {
            let mut row = row.to_vec();
            if !unrelated {
                untangle(rig, &mut row);
            }
            let mut w: Vec<(usize, f64)> = row.into_iter().enumerate().collect();
            w = trim(w);
            w.retain(|(_, x)| *x >= PRUNE);
            trim(w)
        })
        .collect()
}

fn erode(rig: &Rig, mesh: &Mesh, around: &[Vec<u32>], labels: &mut [Option<usize>], zone: f64) {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let edge = mean_edge(mesh, around);
    let mut clash = vec![false; labels.len()];
    for bone in 0..rig.bones.len() {
        let b = &rig.bones[bone];
        let reach = (zone * super::length(sub(b.tail, b.head))).max(GAP_EDGES * edge);
        let mut dist = vec![f64::INFINITY; labels.len()];
        let mut heap = BinaryHeap::new();
        for (v, l) in labels.iter().enumerate() {
            if *l == Some(bone) {
                dist[v] = 0.0;
                heap.push(Reverse((0u64, v)));
            }
        }
        while let Some(Reverse((key, v))) = heap.pop() {
            let d = dist[v];
            if key != d.to_bits() {
                continue;
            }
            if labels[v].is_some_and(|l| !super::skin_check::related(rig, l, bone)) {
                clash[v] = true;
            }
            for &u in &around[v] {
                let u = u as usize;
                let nd = d + super::length(sub(mesh.positions[u], mesh.positions[v]));
                if nd < dist[u] && nd <= reach {
                    dist[u] = nd;
                    heap.push(Reverse((nd.to_bits(), u)));
                }
            }
        }
    }
    for (l, c) in labels.iter_mut().zip(clash) {
        if c {
            *l = None;
        }
    }
}

fn mean_edge(mesh: &Mesh, around: &[Vec<u32>]) -> f64 {
    let (sum, n) = around
        .iter()
        .enumerate()
        .fold((0.0, 0usize), |(s, n), (v, ring)| {
            let here: f64 = ring
                .iter()
                .map(|&u| super::length(sub(mesh.positions[u as usize], mesh.positions[v])))
                .sum();
            (s + here, n + ring.len())
        });
    if n == 0 { 0.0 } else { sum / n as f64 }
}

pub fn smooth_skin(rig: &mut Rig, mesh: &Mesh, options: SmoothOptions) {
    let distances = rig.bone_distances(mesh);
    let start: Weights = distances
        .iter()
        .map(|d| start_weights(rig, d, options.power, options.unrelated))
        .collect();
    pin_and_relax(rig, mesh, &distances, start, options);
}

pub fn pin_and_relax(
    rig: &mut Rig,
    mesh: &Mesh,
    distances: &[Vec<f64>],
    start: Weights,
    options: SmoothOptions,
) {
    let bones = rig.bones.len();
    let around = mesh.neighbours();
    let mut labels: Vec<Option<usize>> = (0..start.len())
        .map(|v| core_bone(rig, mesh, distances, v, options.core, options.joint))
        .collect();
    erode(rig, mesh, &around, &mut labels, options.zone);
    let mut fixed = vec![false; start.len()];
    let mut weights = dense(&start, bones);
    for (v, label) in labels.iter().enumerate() {
        if let Some(b) = *label {
            fixed[v] = true;
            weights[v * bones..(v + 1) * bones].fill(0.0);
            weights[v * bones + b] = 1.0;
        }
    }
    relax(
        (!options.unrelated).then_some(&*rig),
        &mut weights,
        bones,
        &around,
        &fixed,
        options.passes,
        options.step,
    );
    soften_steps(
        (!options.unrelated).then_some(&*rig),
        &mut weights,
        bones,
        &around,
        &mut fixed,
    );
    rig.skin = Some(finish(rig, &weights, bones, options.unrelated));
}

pub fn smooth_existing(
    rig: &mut Rig,
    mesh: &Mesh,
    region: &[bool],
    passes: usize,
    step: f64,
) -> usize {
    let bones = rig.bones.len();
    let skin = rig.ensure_skin(mesh.positions.len()).clone();
    let mut weights = dense(&skin, bones);
    let fixed: Vec<bool> = region.iter().map(|r| !r).collect();
    relax(
        None,
        &mut weights,
        bones,
        &mesh.neighbours(),
        &fixed,
        passes,
        step,
    );
    let mut out = skin;
    let mut count = 0;
    for (v, w) in out.iter_mut().enumerate() {
        if !region[v] {
            continue;
        }
        let mut next: Vec<(usize, f64)> = weights[v * bones..(v + 1) * bones]
            .iter()
            .enumerate()
            .map(|(i, &x)| (i, x))
            .collect();
        next = trim(next);
        next.retain(|(_, x)| *x >= PRUNE);
        *w = trim(next);
        count += 1;
    }
    rig.skin = Some(out);
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::rig::Bone;

    fn tube() -> Mesh {
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        let ring = 8u32;
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

    fn arm() -> Rig {
        Rig {
            bones: vec![
                Bone {
                    name: "up".into(),
                    parent: None,
                    head: [0.0, 0.0, 0.0],
                    tail: [0.0, 0.0, 10.0],
                },
                Bone {
                    name: "low".into(),
                    parent: Some(0),
                    head: [0.0, 0.0, 10.0],
                    tail: [0.0, 0.0, 20.0],
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn smoothing_ramps_weights_over_the_joint_and_keeps_cores_rigid() {
        let mesh = tube();
        let mut rig = arm();
        smooth_skin(&mut rig, &mesh, SmoothOptions::default());
        let skin = rig.skin.as_ref().unwrap();
        let low = |v: usize| {
            skin[v]
                .iter()
                .find(|(b, _)| *b == 1)
                .map_or(0.0, |(_, w)| *w)
        };
        let at = |z: f64| (z / 0.5) as usize * 8;
        assert_eq!(low(at(2.0)), 0.0);
        assert_eq!(low(at(18.0)), 1.0);
        let ramp: Vec<f64> = (14..=26).map(|i| low(i * 8)).collect();
        assert!(ramp.windows(2).all(|w| w[1] >= w[0] - 1.0e-9), "{ramp:?}");
        assert!(ramp.windows(2).all(|w| w[1] - w[0] < 0.2), "{ramp:?}");
        let r = crate::mesh::skin_check::check(&mesh, &rig).unwrap();
        assert!(r.max_jump < 0.2, "{}", r.max_jump);
        assert_eq!(r.min_core, 1.0);
    }

    #[test]
    fn a_vertex_keeps_only_its_bone_family_unless_asked() {
        let mut rig = arm();
        rig.bones.push(Bone {
            name: "far".into(),
            parent: None,
            head: [3.0, 0.0, 9.0],
            tail: [3.0, 0.0, 11.0],
        });
        let d = vec![1.0, 1.1, 1.05];
        let kept = start_weights(&rig, &d, 4.0, false);
        assert!(kept.iter().all(|(b, _)| *b != 2));
        let all = start_weights(&rig, &d, 4.0, true);
        assert!(all.iter().any(|(b, _)| *b == 2));
    }

    #[test]
    fn smoothing_existing_weights_leaves_the_outside_alone() {
        let mesh = tube();
        let mut rig = arm();
        let n = mesh.positions.len();
        rig.skin = Some(
            (0..n)
                .map(|v| vec![(usize::from(v >= n / 2), 1.0)])
                .collect(),
        );
        let region: Vec<bool> = (0..n)
            .map(|v| (n / 2 - 24..n / 2 + 24).contains(&v))
            .collect();
        let touched = smooth_existing(&mut rig, &mesh, &region, 20, 0.5);
        assert_eq!(touched, 48);
        let skin = rig.skin.as_ref().unwrap();
        assert_eq!(skin[0], vec![(0, 1.0)]);
        assert!(skin[n / 2].len() == 2);
    }
}
