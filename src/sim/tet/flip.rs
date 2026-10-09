use super::delaunay::{face_key, face_of, orient};
use super::fixed::{HashMap, HashSet};
use super::{TetMesh, mean_ratio};

fn edge(a: u32, b: u32) -> [u32; 2] {
    if a < b { [a, b] } else { [b, a] }
}

struct Topology {
    tets: Vec<[u32; 4]>,
    alive: Vec<bool>,
    faces: HashMap<[u32; 3], Vec<usize>>,
    edges: HashMap<[u32; 2], Vec<usize>>,
}

impl Topology {
    fn add(&mut self, tet: [u32; 4]) -> usize {
        let t = self.tets.len();
        self.tets.push(tet);
        self.alive.push(true);
        for i in 0..4 {
            self.faces
                .entry(face_key(face_of(tet, i)))
                .or_default()
                .push(t);
            for j in i + 1..4 {
                self.edges.entry(edge(tet[i], tet[j])).or_default().push(t);
            }
        }
        t
    }

    fn remove(&mut self, t: usize) {
        let tet = self.tets[t];
        self.alive[t] = false;
        for i in 0..4 {
            if let Some(list) = self.faces.get_mut(&face_key(face_of(tet, i))) {
                list.retain(|&x| x != t);
            }
            for j in i + 1..4 {
                if let Some(list) = self.edges.get_mut(&edge(tet[i], tet[j])) {
                    list.retain(|&x| x != t);
                }
            }
        }
    }
}

impl TetMesh {
    fn coords(&self, tet: [u32; 4]) -> [[f64; 3]; 4] {
        tet.map(|v| {
            let p = self.nodes[v as usize];
            [p.x, p.y, p.z]
        })
    }

    fn oriented(&self, mut tet: [u32; 4]) -> ([u32; 4], f64) {
        let q = self.coords(tet);
        let six = orient(q[0], q[1], q[2], q[3]);
        if six < 0.0 {
            tet.swap(0, 1);
        }
        (tet, six.abs())
    }

    fn quality_of(&self, tet: [u32; 4]) -> f64 {
        mean_ratio(self.coords(tet))
    }

    fn volume_of(&self, tet: [u32; 4]) -> f64 {
        let q = self.coords(tet);
        orient(q[0], q[1], q[2], q[3]).abs()
    }

    fn replace(&self, old: &[[u32; 4]], new: Vec<[u32; 4]>) -> Option<Vec<[u32; 4]>> {
        let before: f64 = old.iter().map(|&t| self.volume_of(t)).sum();
        let worst_before = old
            .iter()
            .map(|&t| self.quality_of(t))
            .fold(f64::INFINITY, f64::min);
        let mut made = Vec::with_capacity(new.len());
        let mut after = 0.0;
        for tet in new {
            let (tet, six) = self.oriented(tet);
            if six <= before * 1.0e-12 {
                return None;
            }
            after += six;
            made.push(tet);
        }
        if (after - before).abs() > before * 1.0e-9 {
            return None;
        }
        let worst_after = made
            .iter()
            .map(|&t| self.quality_of(t))
            .fold(f64::INFINITY, f64::min);
        (worst_after > worst_before + 1.0e-6).then_some(made)
    }

    pub fn flip(&mut self, passes: usize) {
        let skin: HashSet<[u32; 3]> = self.boundary.iter().map(|(f, _)| face_key(*f)).collect();
        let mut topo = Topology {
            tets: Vec::new(),
            alive: Vec::new(),
            faces: HashMap::default(),
            edges: HashMap::default(),
        };
        for &tet in &self.tets {
            topo.add(tet);
        }
        for _ in 0..passes {
            let mut flipped = 0;
            let candidates: Vec<usize> = (0..topo.tets.len())
                .filter(|&t| topo.alive[t] && self.quality_of(topo.tets[t]) < 0.3)
                .collect();
            for t in candidates {
                if !topo.alive[t] {
                    continue;
                }
                let tet = topo.tets[t];
                let mut done = false;
                for i in 0..4 {
                    for j in i + 1..4 {
                        if done {
                            break;
                        }
                        let (d, e) = (tet[i], tet[j]);
                        let around = topo.edges.get(&edge(d, e)).cloned().unwrap_or_default();
                        if around.len() != 3 {
                            continue;
                        }
                        let ring: Vec<u32> = {
                            let mut r: Vec<u32> = around
                                .iter()
                                .flat_map(|&x| topo.tets[x])
                                .filter(|&v| v != d && v != e)
                                .collect();
                            r.sort_unstable();
                            r.dedup();
                            r
                        };
                        let [a, b, c] = ring[..] else { continue };
                        let closed = [a, b, c].iter().all(|&x| {
                            let f = face_key([d, e, x]);
                            !skin.contains(&f) && topo.faces.get(&f).is_some_and(|l| l.len() == 2)
                        });
                        if !closed {
                            continue;
                        }
                        let old: Vec<[u32; 4]> = around.iter().map(|&x| topo.tets[x]).collect();
                        if let Some(made) = self.replace(&old, vec![[a, b, c, d], [a, b, c, e]]) {
                            around.iter().for_each(|&x| topo.remove(x));
                            made.into_iter().for_each(|m| {
                                topo.add(m);
                            });
                            flipped += 1;
                            done = true;
                        }
                    }
                }
                if done {
                    continue;
                }
                for i in 0..4 {
                    let face = face_key(face_of(tet, i));
                    if skin.contains(&face) {
                        continue;
                    }
                    let Some(pair) = topo.faces.get(&face).cloned() else {
                        continue;
                    };
                    let [t1, t2] = pair[..] else { continue };
                    let apex = |x: usize| {
                        *topo.tets[x]
                            .iter()
                            .find(|v| !face.contains(v))
                            .expect("an apex")
                    };
                    let (d, e) = (apex(t1), apex(t2));
                    if topo.edges.get(&edge(d, e)).is_some_and(|l| !l.is_empty()) {
                        continue;
                    }
                    let [a, b, c] = face;
                    let old = [topo.tets[t1], topo.tets[t2]];
                    if let Some(made) =
                        self.replace(&old, vec![[a, b, d, e], [b, c, d, e], [c, a, d, e]])
                    {
                        topo.remove(t1);
                        topo.remove(t2);
                        made.into_iter().for_each(|m| {
                            topo.add(m);
                        });
                        flipped += 1;
                        break;
                    }
                }
            }
            if flipped == 0 {
                break;
            }
        }
        self.tets = (0..topo.tets.len())
            .filter(|&t| topo.alive[t])
            .map(|t| topo.tets[t])
            .collect();
    }
}
