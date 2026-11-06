use super::{Mesh, P3, Selection, add, cross, dot, length, normalized, scale, sub};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

type Quadric = [f64; 10];
type Candidate = Reverse<(u64, u32, u32, u32, u32)>;

fn plane_quadric(n: P3, d: f64, w: f64) -> Quadric {
    let [a, b, c] = n;
    [
        a * a * w,
        a * b * w,
        a * c * w,
        a * d * w,
        b * b * w,
        b * c * w,
        b * d * w,
        c * c * w,
        c * d * w,
        d * d * w,
    ]
}

fn plus(a: &Quadric, b: &Quadric) -> Quadric {
    std::array::from_fn(|k| a[k] + b[k])
}

fn error(q: &Quadric, p: P3) -> f64 {
    let [x, y, z] = p;
    q[0] * x * x
        + 2.0 * q[1] * x * y
        + 2.0 * q[2] * x * z
        + 2.0 * q[3] * x
        + q[4] * y * y
        + 2.0 * q[5] * y * z
        + 2.0 * q[6] * y
        + q[7] * z * z
        + 2.0 * q[8] * z
        + q[9]
}

fn optimum(q: &Quadric, a: P3, b: P3) -> P3 {
    let m = [[q[0], q[1], q[2]], [q[1], q[4], q[5]], [q[2], q[5], q[7]]];
    let rhs = [-q[3], -q[6], -q[8]];
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let scale_of = m.iter().flatten().fold(0.0f64, |s, v| s.max(v.abs()));
    if det.abs() > 1.0e-9 * scale_of.powi(3).max(1.0e-30) {
        let solve = |col: usize| {
            let mut r = m;
            for row in 0..3 {
                r[row][col] = rhs[row];
            }
            (r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
                - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
                + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]))
                / det
        };
        let p = [solve(0), solve(1), solve(2)];
        let span = length(sub(a, b));
        let mid = scale(add(a, b), 0.5);
        if length(sub(p, mid)) <= 2.0 * span {
            return p;
        }
    }
    [a, scale(add(a, b), 0.5), b]
        .into_iter()
        .min_by(|x, y| error(q, *x).total_cmp(&error(q, *y)))
        .expect("three candidates")
}

impl Mesh {
    pub fn error_from(&self, reference: &Mesh) -> (f64, f64) {
        let there = super::grid::Surface::new(reference);
        let here = super::grid::Surface::new(self);
        let one = |from: &Mesh, to: &super::grid::Surface| {
            from.positions
                .iter()
                .filter_map(|&p| to.closest(p))
                .fold((0.0f64, 0.0f64, 0usize), |(m, s, n), h| {
                    (m.max(h.distance), s + h.distance, n + 1)
                })
        };
        let (a, sa, na) = one(self, &there);
        let (b, sb, nb) = one(reference, &here);
        (a.max(b), (sa + sb) / (na + nb).max(1) as f64)
    }

    pub fn finish(
        &mut self,
        reference: &Mesh,
        target: Option<usize>,
        max_error: Option<f64>,
    ) -> String {
        self.finish_weighted(reference, target, max_error, &[])
    }

    pub fn finish_weighted(
        &mut self,
        reference: &Mesh,
        target: Option<usize>,
        max_error: Option<f64>,
        importance: &[f64],
    ) -> String {
        let before = self.triangles.len();
        let all =
            Selection::new(self, vec![super::Region::All], 0.0, None).expect("a mesh has vertices");
        let unmasked = Selection {
            weights: vec![1.0; self.positions.len()],
            ..all
        };
        match (target, max_error) {
            (Some(target), None) => {
                self.decimate_weighted(&unmasked, target, importance);
            }
            (target, Some(limit)) => {
                let floor = target.unwrap_or(4).max(4);
                let (mut lo, mut hi) = (floor, before);
                let mut best = reference.clone();
                while hi > lo + (before / 200).max(16) {
                    let mid = (lo + hi) / 2;
                    let mut trial = reference.clone();
                    let every = Selection {
                        weights: vec![1.0; trial.positions.len()],
                        ..unmasked.clone()
                    };
                    trial.decimate_weighted(&every, mid, importance);
                    if trial.error_from(reference).0 <= limit {
                        hi = trial.triangles.len().min(mid);
                        best = trial;
                    } else {
                        lo = mid;
                    }
                }
                *self = best;
            }
            (None, None) => {}
        }
        let (worst, mean) = self.error_from(reference);
        let (lo, hi) = reference.bounds().unwrap_or_default();
        let size = (0..3)
            .map(|k| hi[k] - lo[k])
            .fold(0.0, f64::max)
            .max(1.0e-12);
        let open = match self.open_edges() {
            0 => String::new(),
            n => format!(", {n} open edges"),
        };
        format!(
            "{} triangles (was {before}); surface moved {worst:.4} at most ({:.3}% of the size), {mean:.4} on average; volume {:.3} (was {:.3}){open}",
            self.triangles.len(),
            worst / size * 100.0,
            self.volume(),
            reference.volume()
        )
    }

    pub fn decimate(&mut self, selection: &Selection, target: usize) -> usize {
        self.decimate_weighted(selection, target, &[])
    }

    pub fn decimate_weighted(
        &mut self,
        selection: &Selection,
        target: usize,
        importance: &[f64],
    ) -> usize {
        let importance_of = |i: u32| importance.get(i as usize).copied().unwrap_or(1.0);
        let start = self.triangles.len();
        if target >= start {
            return 0;
        }
        let n = self.positions.len();
        let mut quadric = vec![[0.0; 10]; n];
        for &t in &self.triangles {
            let [a, b, c] = self.corners(t);
            let raw = cross(sub(b, a), sub(c, a));
            let area = length(raw) / 2.0;
            if area <= 0.0 {
                continue;
            }
            let normal = normalized(raw);
            let q = plane_quadric(normal, -dot(normal, a), area);
            for i in t {
                let k = importance_of(i);
                let scaled = q.map(|v| v * k);
                quadric[i as usize] = plus(&quadric[i as usize], &scaled);
            }
        }
        for &t in &self.triangles {
            for k in 0..3 {
                let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
                if self.labels[a as usize] == self.labels[b as usize] {
                    continue;
                }
                let [pa, pb, pc] = [a, b, c].map(|i| self.positions[i as usize]);
                let face = cross(sub(pb, pa), sub(pc, pa));
                let edge = sub(pb, pa);
                let side = normalized(cross(edge, face));
                let weight = dot(edge, edge) * 4.0;
                let q = plane_quadric(side, -dot(side, pa), weight);
                for i in [a, b] {
                    quadric[i as usize] = plus(&quadric[i as usize], &q);
                }
            }
        }
        let mut around: Vec<HashSet<u32>> = vec![HashSet::new(); n];
        for (f, t) in self.triangles.iter().enumerate() {
            for &i in t {
                around[i as usize].insert(f as u32);
            }
        }
        let mut alive = vec![true; self.triangles.len()];
        let mut gone = vec![false; n];
        let mut stamp = vec![0u32; n];
        let free = |i: u32| selection.weights[i as usize] > 0.0;
        let mut heap: BinaryHeap<Candidate> = BinaryHeap::new();
        let push = |heap: &mut BinaryHeap<_>,
                    quadric: &[Quadric],
                    positions: &[P3],
                    stamp: &[u32],
                    a: u32,
                    b: u32| {
            let q = plus(&quadric[a as usize], &quadric[b as usize]);
            let p = optimum(&q, positions[a as usize], positions[b as usize]);
            let cost = error(&q, p).max(0.0);
            heap.push(Reverse((
                cost.to_bits(),
                a,
                b,
                stamp[a as usize],
                stamp[b as usize],
            )));
        };
        let mut seen = HashSet::new();
        for t in &self.triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if free(a) && free(b) && seen.insert((a.min(b), a.max(b))) {
                    push(
                        &mut heap,
                        &quadric,
                        &self.positions,
                        &stamp,
                        a.min(b),
                        a.max(b),
                    );
                }
            }
        }
        let mut count = start;
        while count > target {
            let Some(Reverse((_, a, b, sa, sb))) = heap.pop() else {
                break;
            };
            if gone[a as usize]
                || gone[b as usize]
                || stamp[a as usize] != sa
                || stamp[b as usize] != sb
            {
                continue;
            }
            let ring = |v: u32, around: &[HashSet<u32>], tris: &[[u32; 3]]| -> HashSet<u32> {
                around[v as usize]
                    .iter()
                    .flat_map(|&f| tris[f as usize])
                    .filter(|&x| x != v)
                    .collect()
            };
            let (ra, rb) = (
                ring(a, &around, &self.triangles),
                ring(b, &around, &self.triangles),
            );
            let shared: Vec<u32> = ra.intersection(&rb).copied().collect();
            let both: Vec<u32> = around[a as usize]
                .intersection(&around[b as usize])
                .copied()
                .collect();
            if shared.len() != 2 || both.len() != 2 {
                continue;
            }
            let q = plus(&quadric[a as usize], &quadric[b as usize]);
            if self.labels[a as usize] != self.labels[b as usize] {
                continue;
            }
            let p = optimum(&q, self.positions[a as usize], self.positions[b as usize]);
            let flips = [a, b].iter().any(|&v| {
                around[v as usize].iter().any(|&f| {
                    let t = self.triangles[f as usize];
                    if t.contains(&a) && t.contains(&b) {
                        return false;
                    }
                    let before = cross(
                        sub(self.positions[t[1] as usize], self.positions[t[0] as usize]),
                        sub(self.positions[t[2] as usize], self.positions[t[0] as usize]),
                    );
                    let moved = t.map(|i| {
                        if i == v {
                            p
                        } else {
                            self.positions[i as usize]
                        }
                    });
                    let after = cross(sub(moved[1], moved[0]), sub(moved[2], moved[0]));
                    dot(normalized(before), normalized(after)) < 0.2 || length(after) < 1.0e-14
                })
            });
            if flips {
                continue;
            }
            for &f in &both {
                alive[f as usize] = false;
                for i in self.triangles[f as usize] {
                    around[i as usize].remove(&f);
                }
                count -= 1;
            }
            let moving: Vec<u32> = around[b as usize].iter().copied().collect();
            for f in moving {
                for i in self.triangles[f as usize].iter_mut() {
                    if *i == b {
                        *i = a;
                    }
                }
                around[a as usize].insert(f);
            }
            around[b as usize].clear();
            gone[b as usize] = true;
            self.moved_to(a as usize, p);
            quadric[a as usize] = q;
            stamp[a as usize] += 1;
            let neighbours: HashSet<u32> = ring(a, &around, &self.triangles);
            for v in neighbours {
                if free(a) && free(v) {
                    push(
                        &mut heap,
                        &quadric,
                        &self.positions,
                        &stamp,
                        a.min(v),
                        a.max(v),
                    );
                }
            }
        }
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let mut keep = Vec::new();
        let mut triangles = Vec::new();
        for (f, t) in self.triangles.iter().enumerate() {
            if !alive[f] {
                continue;
            }
            triangles.push(t.map(|i| {
                *remap.entry(i).or_insert_with(|| {
                    keep.push(i);
                    (keep.len() - 1) as u32
                })
            }));
        }
        self.keep_vertices(&keep);
        self.triangles = triangles;
        start - self.triangles.len()
    }
}

impl Mesh {
    pub fn cluster(&self, target: usize) -> (Mesh, Vec<u32>) {
        if self.triangles.len() <= target || self.positions.is_empty() {
            let same = (0..self.positions.len() as u32).collect();
            return (self.clone(), same);
        }
        let (lo, hi) = self.bounds().expect("vertices");
        let extent = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f64, f64::max);
        let mut cell = (self.area() * 2.0 / target.max(1) as f64)
            .sqrt()
            .max(extent * 1.0e-6);
        let mut result = self.clustered(cell, lo);
        for _ in 0..6 {
            let count = result.0.triangles.len();
            if count <= target + target / 10 {
                break;
            }
            cell *= (count as f64 / target as f64).sqrt().max(1.05);
            result = self.clustered(cell, lo);
        }
        result
    }

    fn clustered(&self, cell: f64, origin: P3) -> (Mesh, Vec<u32>) {
        let mut index: HashMap<(i64, i64, i64, u16), u32> = HashMap::new();
        let mut sums: Vec<(P3, f64)> = Vec::new();
        let mut representative: Vec<u32> = Vec::new();
        let mut labels: Vec<u16> = Vec::new();
        let mut owner = vec![u32::MAX; self.positions.len()];
        let mut assign = |i: u32| -> u32 {
            if owner[i as usize] != u32::MAX {
                return owner[i as usize];
            }
            let p = self.positions[i as usize];
            let label = self.labels[i as usize];
            let key = (
                ((p[0] - origin[0]) / cell).floor() as i64,
                ((p[1] - origin[1]) / cell).floor() as i64,
                ((p[2] - origin[2]) / cell).floor() as i64,
                label,
            );
            let id = *index.entry(key).or_insert_with(|| {
                sums.push(([0.0; 3], 0.0));
                representative.push(i);
                labels.push(label);
                (sums.len() - 1) as u32
            });
            let slot = &mut sums[id as usize];
            slot.0 = add(slot.0, p);
            slot.1 += 1.0;
            owner[i as usize] = id;
            id
        };
        let mut triangles = Vec::new();
        for &t in &self.triangles {
            let [a, b, c] = t.map(&mut assign);
            if a != b && b != c && a != c {
                triangles.push([a, b, c]);
            }
        }
        let positions = sums.iter().map(|(s, n)| scale(*s, 1.0 / n)).collect();
        let mesh = Mesh {
            positions,
            triangles,
            labels,
            parts: self.parts.clone(),
            mask: if self.mask.len() == self.positions.len() {
                representative
                    .iter()
                    .map(|&i| self.mask[i as usize])
                    .collect()
            } else {
                Vec::new()
            },
            hidden: self.hidden.clone(),
            layers: Vec::new(),
        };
        (mesh, representative)
    }
}

#[cfg(test)]
mod cluster_tests {
    use super::*;

    fn grid(n: u32) -> Mesh {
        let mut mesh = Mesh::default();
        mesh.parts.push("a".into());
        for y in 0..=n {
            for x in 0..=n {
                mesh.positions.push([x as f64, y as f64, 0.0]);
                mesh.labels.push(0);
            }
        }
        for y in 0..n {
            for x in 0..n {
                let i = y * (n + 1) + x;
                mesh.triangles.push([i, i + 1, i + n + 2]);
                mesh.triangles.push([i, i + n + 2, i + n + 1]);
            }
        }
        mesh
    }

    #[test]
    fn cluster_reaches_the_budget_and_keeps_the_extent() {
        let mesh = grid(200);
        let (lod, rep) = mesh.cluster(2000);
        assert!(lod.triangles.len() <= 2500, "{}", lod.triangles.len());
        assert!(lod.triangles.len() >= 500, "{}", lod.triangles.len());
        assert_eq!(rep.len(), lod.positions.len());
        assert!(
            lod.triangles
                .iter()
                .flatten()
                .all(|&i| (i as usize) < lod.positions.len())
        );
        let (lo, hi) = lod.bounds().unwrap();
        assert!(lo[0] < 8.0 && hi[0] > 192.0);
    }

    #[test]
    fn cluster_keeps_part_labels_and_colours_at_two_million_triangles() {
        let mut mesh = grid(1000);
        mesh.parts.push("b".into());
        let width = 1001;
        for (i, label) in mesh.labels.iter_mut().enumerate() {
            *label = u16::from(i % width >= width / 2);
        }
        let colours: Vec<[f64; 3]> = mesh
            .labels
            .iter()
            .map(|&l| {
                if l == 0 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                }
            })
            .collect();
        assert!(mesh.triangles.len() >= 2_000_000);
        let started = std::time::Instant::now();
        let (lod, source) = mesh.cluster(20_000);
        eprintln!(
            "cluster 2M -> {} in {:?}",
            lod.triangles.len(),
            started.elapsed()
        );
        assert!(lod.triangles.len() <= 22_000);
        assert!(lod.triangles.len() >= 5_000);
        assert_eq!(lod.parts, mesh.parts);
        assert_eq!(lod.labels.len(), lod.positions.len());
        for (i, &label) in lod.labels.iter().enumerate() {
            assert_eq!(label, mesh.labels[source[i] as usize]);
            assert_eq!(colours[source[i] as usize][0] == 1.0, label == 0);
            let x = lod.positions[i][0];
            assert_eq!(label == 1, x > 499.5, "vertex {i} at x={x}");
        }
        assert!(lod.labels.contains(&0) && lod.labels.contains(&1));
    }

    #[test]
    fn cluster_leaves_a_small_mesh_alone() {
        let mesh = grid(4);
        assert_eq!(mesh.cluster(1000).0, mesh);
    }
}
