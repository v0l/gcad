use super::fixed::HashSet;
use robust::Coord3D;

pub const NONE: u32 = u32::MAX;

pub type Point = [f64; 3];

fn c(p: Point) -> Coord3D<f64> {
    Coord3D {
        x: p[0],
        y: p[1],
        z: p[2],
    }
}

pub fn orient(a: Point, b: Point, cc: Point, d: Point) -> f64 {
    robust::orient3d(c(a), c(b), c(cc), c(d))
}

pub fn face_key(mut face: [u32; 3]) -> [u32; 3] {
    face.sort_unstable();
    face
}

pub fn face_of(tet: [u32; 4], opposite: usize) -> [u32; 3] {
    let mut face = [0; 3];
    let mut k = 0;
    for (i, v) in tet.iter().enumerate() {
        if i != opposite {
            face[k] = *v;
            k += 1;
        }
    }
    face
}

pub enum Insert {
    Added(u32),
    Existing(u32),
    Encroached([u32; 3]),
    Blocked,
    Failed,
}

#[derive(Clone, Copy)]
pub enum Mode<'a> {
    Free,
    Walls,
    Split {
        crossing: &'a [[u32; 3]],
        edges: &'a [[u32; 2]],
        seeds: &'a [u32],
    },
}

impl Mode<'_> {
    fn wall(&self, set: &HashSet<[u32; 3]>, face: &[u32; 3]) -> bool {
        match self {
            Mode::Free => false,
            Mode::Walls => set.contains(face),
            Mode::Split { crossing, .. } => set.contains(face) && !crossing.contains(face),
        }
    }
}

#[derive(Default)]
pub struct Delaunay {
    pub points: Vec<Point>,
    pub tets: Vec<[u32; 4]>,
    pub adj: Vec<[u32; 4]>,
    pub dead: Vec<bool>,
    pub inside: Vec<bool>,
    pub constrained: HashSet<[u32; 3]>,
    free: Vec<u32>,
    last: u32,
    stamp: Vec<u32>,
    round: u32,
    pub born: Vec<u32>,
}

impl Delaunay {
    pub fn new(low: Point, high: Point) -> Delaunay {
        let centre = [0, 1, 2].map(|k| (low[k] + high[k]) / 2.0);
        let size = (0..3).map(|k| high[k] - low[k]).fold(1.0e-9, f64::max) * 1.0e3;
        let points = vec![
            [centre[0] - size, centre[1] - size, centre[2] - size],
            [centre[0] + 3.0 * size, centre[1] - size, centre[2] - size],
            [centre[0] - size, centre[1] + 3.0 * size, centre[2] - size],
            [centre[0] - size, centre[1] - size, centre[2] + 3.0 * size],
        ];
        let mut tet = [0, 1, 2, 3];
        if orient(points[0], points[1], points[2], points[3]) < 0.0 {
            tet.swap(0, 1);
        }
        Delaunay {
            points,
            tets: vec![tet],
            adj: vec![[NONE; 4]],
            dead: vec![false],
            inside: vec![false],
            stamp: vec![0],
            ..Default::default()
        }
    }

    pub fn is_super(&self, v: u32) -> bool {
        v < 4
    }

    pub fn alive(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.tets.len()).filter(|&t| !self.dead[t])
    }

    pub fn coords(&self, tet: [u32; 4]) -> [Point; 4] {
        tet.map(|v| self.points[v as usize])
    }

    fn replaced(&self, t: usize, i: usize, p: Point) -> f64 {
        let mut q = self.coords(self.tets[t]);
        q[i] = p;
        orient(q[0], q[1], q[2], q[3])
    }

    fn in_sphere(&self, t: usize, p: Point) -> f64 {
        let q = self.coords(self.tets[t]);
        robust::insphere(c(q[0]), c(q[1]), c(q[2]), c(q[3]), c(p))
    }

    pub fn locate(&mut self, p: Point, start: u32) -> Option<u32> {
        let mut t = if start != NONE && !self.dead[start as usize] {
            start
        } else if !self.dead[self.last as usize] {
            self.last
        } else {
            self.alive().next()? as u32
        };
        let mut seed = (t as u64).wrapping_mul(2_654_435_761) ^ 0x9e37_79b9;
        for _ in 0..(self.tets.len() * 4 + 64) {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let offset = (seed % 4) as usize;
            let mut moved = false;
            for k in 0..4 {
                let i = (k + offset) % 4;
                if self.replaced(t as usize, i, p) < 0.0 {
                    let next = self.adj[t as usize][i];
                    if next == NONE {
                        return None;
                    }
                    t = next;
                    moved = true;
                    break;
                }
            }
            if !moved {
                return Some(t);
            }
        }
        None
    }

    fn next_round(&mut self) -> u32 {
        self.round = self.round.wrapping_add(1);
        if self.round == 0 {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.round = 1;
        }
        self.round
    }

    pub fn insert(&mut self, p: Point, start: u32, mode: Mode) -> Insert {
        let Some(home) = self.locate(p, start) else {
            return Insert::Failed;
        };
        self.insert_from(p, home, mode)
    }

    pub fn insert_from(&mut self, p: Point, seed: u32, mode: Mode) -> Insert {
        for &v in &self.tets[seed as usize] {
            if self.points[v as usize] == p {
                return Insert::Existing(v);
            }
        }
        let round = self.next_round();
        let mut cavity = vec![seed];
        self.stamp[seed as usize] = round;
        if let Mode::Split { seeds, .. } = mode {
            for &extra in seeds {
                if self.stamp[extra as usize] != round {
                    self.stamp[extra as usize] = round;
                    cavity.push(extra);
                }
            }
        }
        let mut k = 0;
        while k < cavity.len() {
            let t = cavity[k] as usize;
            k += 1;
            for i in 0..4 {
                let n = self.adj[t][i];
                if n == NONE || self.stamp[n as usize] == round {
                    continue;
                }
                if mode.wall(&self.constrained, &face_key(face_of(self.tets[t], i))) {
                    continue;
                }
                if self.in_sphere(n as usize, p) > 0.0 {
                    self.stamp[n as usize] = round;
                    cavity.push(n);
                    if !matches!(mode, Mode::Free) && cavity.len() > 20_000 {
                        return Insert::Failed;
                    }
                }
            }
        }
        loop {
            let mut grown = false;
            let mut index = 0;
            while index < cavity.len() {
                let t = cavity[index] as usize;
                index += 1;
                for i in 0..4 {
                    let n = self.adj[t][i];
                    if n != NONE && self.stamp[n as usize] == round {
                        continue;
                    }
                    if self.replaced(t, i, p) > 0.0 {
                        continue;
                    }
                    if n == NONE {
                        return Insert::Failed;
                    }
                    if mode.wall(&self.constrained, &face_key(face_of(self.tets[t], i))) {
                        return Insert::Blocked;
                    }
                    self.stamp[n as usize] = round;
                    cavity.push(n);
                    grown = true;
                }
            }
            if !grown {
                break;
            }
        }
        let mut boundary: Vec<(u32, usize, u32)> = Vec::new();
        for &t in &cavity {
            for i in 0..4 {
                let n = self.adj[t as usize][i];
                if n == NONE || self.stamp[n as usize] != round {
                    boundary.push((t, i, n));
                }
            }
        }
        let mut rim: HashSet<u32> = HashSet::default();
        for &(t, i, _) in &boundary {
            rim.extend(face_of(self.tets[t as usize], i));
        }
        if cavity
            .iter()
            .any(|&t| self.tets[t as usize].iter().any(|v| !rim.contains(v)))
        {
            return Insert::Blocked;
        }
        match mode {
            Mode::Walls => {
                for &(t, i, _) in &boundary {
                    let face = face_key(face_of(self.tets[t as usize], i));
                    if self.constrained.contains(&face) && self.encroaches(face, p) {
                        return Insert::Encroached(face);
                    }
                }
            }
            Mode::Split { edges, .. } => {
                let mut found: Vec<[u32; 2]> = Vec::new();
                for &(t, i, _) in &boundary {
                    let f = face_of(self.tets[t as usize], i);
                    for k in 0..3 {
                        let mut e = [f[k], f[(k + 1) % 3]];
                        e.sort_unstable();
                        found.push(e);
                    }
                }
                if edges.iter().any(|e| {
                    let mut e = *e;
                    e.sort_unstable();
                    found.iter().filter(|f| **f == e).count() < 2
                }) {
                    return Insert::Blocked;
                }
            }
            Mode::Free => {}
        }
        let id = self.points.len() as u32;
        self.points.push(p);
        let boundary: Vec<([u32; 4], usize, u32, bool)> = boundary
            .into_iter()
            .map(|(t, i, n)| (self.tets[t as usize], i, n, self.inside[t as usize]))
            .collect();
        let mut made: Vec<u32> = Vec::with_capacity(boundary.len());
        let mut edges: Vec<([u32; 2], u32, usize)> = Vec::with_capacity(boundary.len() * 3);
        for &(old, i, n, inside) in &boundary {
            let mut tet = old;
            tet[i] = id;
            let slot = match self.free.pop() {
                Some(s) => {
                    self.tets[s as usize] = tet;
                    self.adj[s as usize] = [NONE; 4];
                    self.dead[s as usize] = false;
                    self.inside[s as usize] = inside;
                    s
                }
                None => {
                    self.tets.push(tet);
                    self.adj.push([NONE; 4]);
                    self.dead.push(false);
                    self.inside.push(inside);
                    self.stamp.push(0);
                    (self.tets.len() - 1) as u32
                }
            };
            self.adj[slot as usize][i] = n;
            if n != NONE {
                let face = face_of(old, i);
                let other = self.tets[n as usize];
                let j = (0..4)
                    .find(|&j| !face.contains(&other[j]))
                    .expect("a neighbour has one vertex off the shared face");
                self.adj[n as usize][j] = slot;
            }
            for j in 0..4 {
                if j == i {
                    continue;
                }
                let mut edge = [0u32; 2];
                let mut e = 0;
                for (m, &v) in tet.iter().enumerate() {
                    if m != i && m != j {
                        edge[e] = v;
                        e += 1;
                    }
                }
                edge.sort_unstable();
                match edges.iter().position(|(key, _, _)| *key == edge) {
                    Some(at) => {
                        let (_, other, oj) = edges.swap_remove(at);
                        self.adj[slot as usize][j] = other;
                        self.adj[other as usize][oj] = slot;
                    }
                    None => edges.push((edge, slot, j)),
                }
            }
            made.push(slot);
        }
        for &t in &cavity {
            self.dead[t as usize] = true;
            self.free.push(t);
        }
        self.last = made[0];
        self.born = made;
        Insert::Added(id)
    }

    fn encroaches(&self, face: [u32; 3], p: Point) -> bool {
        let [a, b, cc] = face.map(|v| self.points[v as usize]);
        let d = |x: Point, y: Point| [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
        let dot = |x: Point, y: Point| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
        let cross = |x: Point, y: Point| {
            [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ]
        };
        let (u, v) = (d(b, a), d(cc, a));
        let n = cross(u, v);
        let nn = dot(n, n);
        if nn < 1.0e-300 {
            return false;
        }
        let w = cross(n, u);
        let z = cross(v, n);
        let (uu, vv) = (dot(u, u), dot(v, v));
        let offset = [0, 1, 2].map(|k| (z[k] * uu + w[k] * vv) / (2.0 * nn));
        let centre = [a[0] + offset[0], a[1] + offset[1], a[2] + offset[2]];
        let r2 = dot(offset, offset);
        let q = d(p, centre);
        dot(q, q) < r2 * 0.999
    }

    pub fn tets_of_face(&mut self, face: [u32; 3]) -> Option<[u32; 2]> {
        let key = face_key(face);
        let q = face.map(|v| self.points[v as usize]);
        let centre = [0, 1, 2].map(|k| (q[0][k] + q[1][k] + q[2][k]) / 3.0);
        let start = self.locate(centre, NONE)?;
        let mut stack = vec![start];
        let mut seen = HashSet::default();
        while let Some(t) = stack.pop() {
            if !seen.insert(t) || seen.len() > 64 {
                continue;
            }
            for i in 0..4 {
                if face_key(face_of(self.tets[t as usize], i)) == key {
                    let n = self.adj[t as usize][i];
                    return (n != NONE).then_some([t, n]);
                }
                let n = self.adj[t as usize][i];
                if n != NONE {
                    stack.push(n);
                }
            }
        }
        None
    }

    pub fn faces(&self) -> HashSet<[u32; 3]> {
        self.alive()
            .flat_map(|t| (0..4).map(move |i| face_key(face_of(self.tets[t], i))))
            .collect()
    }
}
