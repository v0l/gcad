use crate::cloud::{Cloud, P3};
use crate::kdtree::KdTree;
use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: P3, s: f64) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: P3) -> f64 {
    dot(a, a).sqrt()
}

fn undirected(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

fn ball_centre(a: P3, b: P3, c: P3, radius: f64) -> Option<P3> {
    let u = sub(b, a);
    let v = sub(c, a);
    let w = cross(u, v);
    let w2 = dot(w, w);
    if w2 < 1.0e-24 {
        return None;
    }
    let to_circumcentre = scale(
        add(
            scale(cross(w, u), dot(v, v)),
            scale(cross(v, w), dot(u, u)),
        ),
        1.0 / (2.0 * w2),
    );
    let squared = dot(to_circumcentre, to_circumcentre);
    if squared > radius * radius {
        return None;
    }
    let height = (radius * radius - squared).sqrt();
    let circumcentre = add(a, to_circumcentre);
    Some(add(circumcentre, scale(w, height / w2.sqrt())))
}

struct Front {
    from: u32,
    to: u32,
    opposite: u32,
    centre: P3,
}

struct Mesher<'a> {
    points: &'a [P3],
    normals: Vec<P3>,
    tree: &'a KdTree,
    radius: f64,
    triangles: Vec<[u32; 3]>,
    opposite: HashMap<(u32, u32), u32>,
    edges: HashMap<(u32, u32), u8>,
    open_edges: Vec<u32>,
    used: Vec<bool>,
    stack: Vec<Front>,
}

impl Mesher<'_> {
    fn consistent(&self, tri: [u32; 3], normal: P3) -> bool {
        let sum = tri
            .iter()
            .fold([0.0; 3], |s, &i| add(s, self.normals[i as usize]));
        dot(normal, sum) > 0.0
    }

    fn inner(&self, v: u32) -> bool {
        self.used[v as usize] && self.open_edges[v as usize] == 0
    }

    fn can_add(&self, tri: [u32; 3]) -> bool {
        let sides = [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])];
        sides.iter().all(|&(a, b)| {
            !self.opposite.contains_key(&(a, b))
                && self.edges.get(&undirected(a, b)).copied().unwrap_or(0) < 2
        })
    }

    fn add_triangle(&mut self, tri: [u32; 3], centre: P3) {
        let sides = [
            (tri[0], tri[1], tri[2]),
            (tri[1], tri[2], tri[0]),
            (tri[2], tri[0], tri[1]),
        ];
        for &(a, b, o) in &sides {
            self.opposite.insert((a, b), o);
            let count = self.edges.entry(undirected(a, b)).or_insert(0);
            *count += 1;
            if *count == 1 {
                self.open_edges[a as usize] += 1;
                self.open_edges[b as usize] += 1;
            } else {
                self.open_edges[a as usize] -= 1;
                self.open_edges[b as usize] -= 1;
            }
        }
        for v in tri {
            self.used[v as usize] = true;
        }
        self.triangles.push(tri);
        for &(a, b, o) in &sides {
            if self.edges[&undirected(a, b)] == 1 {
                self.stack.push(Front {
                    from: a,
                    to: b,
                    opposite: o,
                    centre,
                });
            }
        }
    }

    fn empty_ball(&self, centre: P3, tri: [u32; 3]) -> bool {
        self.tree
            .within(centre, self.radius * (1.0 - 1.0e-7))
            .into_iter()
            .all(|i| tri.contains(&(i as u32)))
    }

    fn seed(&mut self, p: u32) -> bool {
        let here = self.points[p as usize];
        let mut near: Vec<(f64, u32)> = self
            .tree
            .within(here, 2.0 * self.radius)
            .into_iter()
            .map(|i| i as u32)
            .filter(|&i| i != p && !self.used[i as usize])
            .map(|i| (length(sub(self.points[i as usize], here)), i))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.truncate(12);
        for x in 0..near.len() {
            for y in x + 1..near.len() {
                let (q, s) = (near[x].1, near[y].1);
                let mut tri = [p, q, s];
                let pts = tri.map(|i| self.points[i as usize]);
                let mut normal = cross(sub(pts[1], pts[0]), sub(pts[2], pts[0]));
                if !self.consistent(tri, normal) {
                    tri = [p, s, q];
                    normal = scale(normal, -1.0);
                    if !self.consistent(tri, normal) {
                        continue;
                    }
                }
                let pts = tri.map(|i| self.points[i as usize]);
                let Some(centre) = ball_centre(pts[0], pts[1], pts[2], self.radius) else {
                    continue;
                };
                if self.empty_ball(centre, tri) && self.can_add(tri) {
                    self.add_triangle(tri, centre);
                    return true;
                }
            }
        }
        false
    }

    fn pivot(&self, front: &Front) -> Option<(u32, P3)> {
        let (i, j, o) = (front.from, front.to, front.opposite);
        let (pi, pj) = (self.points[i as usize], self.points[j as usize]);
        let middle = scale(add(pi, pj), 0.5);
        let mut axis = sub(pj, pi);
        let span = length(axis);
        if span < 1.0e-12 {
            return None;
        }
        axis = scale(axis, 1.0 / span);
        let u = sub(front.centre, middle);
        let toward = sub(self.points[o as usize], pi);
        let away = sub(toward, scale(axis, dot(toward, axis)));
        if dot(cross(axis, u), away) > 0.0 {
            axis = scale(axis, -1.0);
        }
        let mut best: Option<(f64, u32, P3)> = None;
        for k in self.tree.within(middle, 2.0 * self.radius) {
            let k = k as u32;
            if k == i || k == j || k == o {
                continue;
            }
            let pk = self.points[k as usize];
            let tri = [j, i, k];
            let normal = cross(sub(pi, pj), sub(pk, pj));
            if !self.consistent(tri, normal) {
                continue;
            }
            let Some(centre) = ball_centre(pj, pi, pk, self.radius) else {
                continue;
            };
            let v = sub(centre, middle);
            let mut angle = dot(cross(u, v), axis).atan2(dot(u, v));
            if angle < 0.0 {
                angle += 2.0 * PI;
            }
            if !(1.0e-9..=PI).contains(&angle) {
                continue;
            }
            if best.as_ref().is_none_or(|b| angle < b.0) {
                best = Some((angle, k, centre));
            }
        }
        let (_, k, centre) = best?;
        if self.inner(k) || !self.can_add([j, i, k]) || !self.empty_ball(centre, [i, j, k]) {
            return None;
        }
        Some((k, centre))
    }

    fn grow(&mut self) {
        while let Some(front) = self.stack.pop() {
            let key = undirected(front.from, front.to);
            if self.edges.get(&key).copied() != Some(1) {
                continue;
            }
            if let Some((k, centre)) = self.pivot(&front) {
                self.add_triangle([front.to, front.from, k], centre);
            }
        }
    }

    fn reopen(&mut self) {
        let boundary: Vec<(u32, u32, u32)> = self
            .opposite
            .iter()
            .filter(|((a, b), _)| self.edges[&undirected(*a, *b)] == 1)
            .map(|((a, b), o)| (*a, *b, *o))
            .collect();
        for (a, b, o) in boundary {
            let pts = [a, b, o].map(|i| self.points[i as usize]);
            if let Some(centre) = ball_centre(pts[0], pts[1], pts[2], self.radius) {
                self.stack.push(Front {
                    from: a,
                    to: b,
                    opposite: o,
                    centre,
                });
            }
        }
    }
}

pub fn ball_pivoting(cloud: &Cloud, radii: &[f64]) -> Result<Vec<[u32; 3]>> {
    let Some(normals) = &cloud.normals else {
        bail!("ball pivoting needs oriented normals, run `normals` first");
    };
    if radii.is_empty() || radii.iter().any(|r| !(*r > 0.0)) {
        bail!("ball pivoting needs one or more positive ball radii");
    }
    let tree = KdTree::build(&cloud.positions);
    let mut sorted: Vec<f64> = radii.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.dedup();
    let n = cloud.len();
    let mut mesher = Mesher {
        points: &cloud.positions,
        normals: normals.iter().map(|v| v.map(f64::from)).collect(),
        tree: &tree,
        radius: sorted[0],
        triangles: Vec::new(),
        opposite: HashMap::new(),
        edges: HashMap::new(),
        open_edges: vec![0; n],
        used: vec![false; n],
        stack: Vec::new(),
    };
    for &radius in &sorted {
        mesher.radius = radius;
        mesher.reopen();
        mesher.grow();
        for p in 0..n as u32 {
            if !mesher.used[p as usize] && mesher.seed(p) {
                mesher.grow();
            }
        }
    }
    let unique: HashSet<[u32; 3]> = mesher.triangles.iter().copied().collect();
    debug_assert_eq!(unique.len(), mesher.triangles.len());
    Ok(mesher.triangles)
}
