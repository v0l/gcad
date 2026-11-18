use super::{Mesh, P3, add, cross, length, scale, sub};
use std::collections::{HashMap, HashSet};

pub struct Cleaned {
    pub welded: usize,
    pub degenerate: usize,
    pub non_manifold: usize,
    pub islands: usize,
}

fn weld_key(p: P3, step: f64) -> [i64; 3] {
    p.map(|v| (v / step).round() as i64)
}

impl Mesh {
    pub fn compact(&mut self) {
        let mut remap = vec![u32::MAX; self.positions.len()];
        let mut keep: Vec<u32> = Vec::new();
        for t in self.triangles.iter_mut() {
            for i in t.iter_mut() {
                if remap[*i as usize] == u32::MAX {
                    remap[*i as usize] = keep.len() as u32;
                    keep.push(*i);
                }
                *i = remap[*i as usize];
            }
        }
        self.keep_vertices(&keep);
    }

    fn triangle_area(&self, t: [u32; 3]) -> f64 {
        let [a, b, c] = self.corners(t);
        length(cross(sub(b, a), sub(c, a))) / 2.0
    }

    pub fn clean(&mut self, island_share: f64) -> Cleaned {
        let (lo, hi) = self.bounds().unwrap_or_default();
        let extent = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max);
        let step = (extent * 1.0e-9).max(1.0e-12);
        let mut first: HashMap<[i64; 3], u32> = HashMap::new();
        let mut canonical: Vec<u32> = Vec::with_capacity(self.positions.len());
        for (i, &p) in self.positions.iter().enumerate() {
            canonical.push(*first.entry(weld_key(p, step)).or_insert(i as u32));
        }
        let welded = canonical
            .iter()
            .enumerate()
            .filter(|(i, c)| **c != *i as u32)
            .count();
        let mut seen: HashSet<[u32; 3]> = HashSet::new();
        let before = self.triangles.len();
        let mut triangles: Vec<[u32; 3]> = Vec::new();
        for &t in &self.triangles {
            let t = t.map(|i| canonical[i as usize]);
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                continue;
            }
            let mut key = t;
            key.sort_unstable();
            if seen.insert(key) {
                triangles.push(t);
            }
        }
        self.triangles = triangles;
        let degenerate = before - self.triangles.len();
        let mut non_manifold = 0;
        loop {
            let mut users: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
            for (f, t) in self.triangles.iter().enumerate() {
                for k in 0..3 {
                    let (a, b) = (t[k], t[(k + 1) % 3]);
                    users.entry((a.min(b), a.max(b))).or_default().push(f);
                }
            }
            let mut drop: HashSet<usize> = HashSet::new();
            for faces in users.values().filter(|f| f.len() > 2) {
                let mut ranked: Vec<usize> = faces.clone();
                ranked.sort_by(|&a, &b| {
                    self.triangle_area(self.triangles[b])
                        .total_cmp(&self.triangle_area(self.triangles[a]))
                });
                drop.extend(ranked.into_iter().skip(2));
            }
            if drop.is_empty() {
                break;
            }
            non_manifold += drop.len();
            let mut f = 0;
            self.triangles.retain(|_| {
                f += 1;
                !drop.contains(&(f - 1))
            });
        }
        non_manifold += self.drop_pinched_fans();
        let islands = self.drop_islands(island_share);
        self.compact();
        Cleaned {
            welded,
            degenerate,
            non_manifold,
            islands,
        }
    }

    fn drop_pinched_fans(&mut self) -> usize {
        let mut dropped = 0;
        loop {
            let mut fans: HashMap<u32, Vec<usize>> = HashMap::new();
            for (f, t) in self.triangles.iter().enumerate() {
                for &v in t {
                    fans.entry(v).or_default().push(f);
                }
            }
            let mut drop: HashSet<usize> = HashSet::new();
            for (&v, faces) in &fans {
                let mut group: Vec<usize> = (0..faces.len()).collect();
                fn root(group: &mut [usize], mut i: usize) -> usize {
                    while group[i] != i {
                        group[i] = group[group[i]];
                        i = group[i];
                    }
                    i
                }
                for x in 0..faces.len() {
                    for y in x + 1..faces.len() {
                        let (a, b) = (self.triangles[faces[x]], self.triangles[faces[y]]);
                        let shared = a.iter().filter(|&&i| i != v && b.contains(&i)).count();
                        if shared > 0 {
                            let (rx, ry) = (root(&mut group, x), root(&mut group, y));
                            group[rx] = ry;
                        }
                    }
                }
                let mut sizes: HashMap<usize, usize> = HashMap::new();
                for x in 0..faces.len() {
                    *sizes.entry(root(&mut group, x)).or_default() += 1;
                }
                if sizes.len() < 2 {
                    continue;
                }
                let best = sizes.iter().max_by_key(|(r, n)| (**n, **r)).map(|(r, _)| *r);
                for x in 0..faces.len() {
                    if Some(root(&mut group, x)) != best {
                        drop.insert(faces[x]);
                    }
                }
            }
            if drop.is_empty() {
                return dropped;
            }
            dropped += drop.len();
            let mut f = 0;
            self.triangles.retain(|_| {
                f += 1;
                !drop.contains(&(f - 1))
            });
        }
    }

    fn drop_islands(&mut self, share: f64) -> usize {
        let n = self.positions.len();
        let mut parent: Vec<u32> = (0..n as u32).collect();
        fn find(parent: &mut [u32], mut i: u32) -> u32 {
            while parent[i as usize] != i {
                parent[i as usize] = parent[parent[i as usize] as usize];
                i = parent[i as usize];
            }
            i
        }
        for t in &self.triangles {
            for k in 1..3 {
                let (a, b) = (find(&mut parent, t[0]), find(&mut parent, t[k]));
                if a != b {
                    parent[a as usize] = b;
                }
            }
        }
        let mut sizes: HashMap<u32, usize> = HashMap::new();
        for t in &self.triangles {
            *sizes.entry(find(&mut parent, t[0])).or_default() += 1;
        }
        let biggest = sizes.values().copied().max().unwrap_or(0);
        let floor = (biggest as f64 * share).ceil() as usize;
        let before = self.triangles.len();
        let mut roots: Vec<u32> = Vec::new();
        for t in &self.triangles {
            roots.push(find(&mut parent, t[0]));
        }
        let mut f = 0;
        self.triangles.retain(|_| {
            f += 1;
            sizes[&roots[f - 1]] >= floor
        });
        before - self.triangles.len()
    }

    pub fn boundary_loops(&self) -> Vec<Vec<u32>> {
        let mut count: HashMap<(u32, u32), i32> = HashMap::new();
        let mut direction: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
        for t in &self.triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = (a.min(b), a.max(b));
                *count.entry(key).or_default() += 1;
                direction.insert(key, (a, b));
            }
        }
        let mut next: HashMap<u32, Vec<u32>> = HashMap::new();
        for (&(a, b), &n) in &count {
            if n == 1 {
                next.entry(a).or_default().push(b);
                next.entry(b).or_default().push(a);
            }
        }
        let mut starts: Vec<u32> = next.keys().copied().collect();
        starts.sort_unstable();
        let mut loops = Vec::new();
        for start in starts {
            if next[&start].len() != 2 {
                continue;
            }
            let mut ring = vec![start];
            let mut previous = start;
            let mut here = next[&start][0];
            let mut closed = false;
            while next[&here].len() == 2 {
                if here == start {
                    closed = true;
                    break;
                }
                ring.push(here);
                let ahead = &next[&here];
                let to = if ahead[0] == previous { ahead[1] } else { ahead[0] };
                previous = here;
                here = to;
            }
            if !closed || ring.len() < 3 {
                continue;
            }
            let mut sorted = ring.clone();
            sorted.sort_unstable();
            for v in &sorted {
                next.get_mut(v).expect("on the ring").clear();
            }
            let (a, b) = (ring[0], ring[1]);
            if direction[&(a.min(b), a.max(b))] != (a, b) {
                ring.reverse();
            }
            loops.push(ring);
        }
        loops
    }

    pub fn fill_holes(&mut self, max_edges: usize) -> usize {
        let mut filled = 0;
        for ring in self.boundary_loops() {
            if ring.len() > max_edges || ring.len() < 3 {
                continue;
            }
            let mut centre = [0.0; 3];
            for &i in &ring {
                centre = add(centre, self.positions[i as usize]);
            }
            let centre = scale(centre, 1.0 / ring.len() as f64);
            let label = self.labels[ring[0] as usize];
            let colour = self.vertex_colours.get(ring[0] as usize).copied();
            let c = self.positions.len() as u32;
            self.positions.push(centre);
            self.labels.push(label);
            if let Some(colour) = colour {
                self.vertex_colours.push(colour);
            }
            if !self.mask.is_empty() {
                self.mask.push(0.0);
            }
            for layer in &mut self.layers {
                match &mut layer.origin {
                    Some(origin) => origin.push(centre),
                    None => layer.delta.push([0.0; 3]),
                }
            }
            for k in 0..ring.len() {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                self.triangles.push([b, a, c]);
            }
            filled += 1;
        }
        filled
    }

    pub fn open_hole(&mut self, at: P3, radius: f64) -> usize {
        let before = self.triangles.len();
        let positions = &self.positions;
        self.triangles.retain(|t| {
            let m = scale(
                add(add(positions[t[0] as usize], positions[t[1] as usize]), positions[t[2] as usize]),
                1.0 / 3.0,
            );
            length(sub(m, at)) > radius
        });
        let removed = before - self.triangles.len();
        self.compact();
        removed
    }

    pub fn taubin(&mut self, iterations: usize, lambda: f64, mu: f64) {
        let around = self.neighbours();
        let n = self.positions.len();
        for _ in 0..iterations {
            for factor in [lambda, mu] {
                let moved: Vec<P3> = (0..n)
                    .map(|i| {
                        let ring = &around[i];
                        if ring.is_empty() {
                            return self.positions[i];
                        }
                        let mean = scale(
                            ring.iter()
                                .fold([0.0; 3], |s, &j| add(s, self.positions[j as usize])),
                            1.0 / ring.len() as f64,
                        );
                        add(self.positions[i], scale(sub(mean, self.positions[i]), factor))
                    })
                    .collect();
                for (i, p) in moved.into_iter().enumerate() {
                    self.moved_to(i, p);
                }
            }
        }
    }
}
