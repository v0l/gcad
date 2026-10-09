mod delaunay;
mod fixed;
mod flip;
mod sizing;
pub mod surface;

use anyhow::{Result, bail};
use delaunay::{Delaunay, Insert, Mode, NONE, Point, face_key, face_of, orient};
pub use fixed::{HashMap, HashSet};
use monstertruck::modeling::*;

use monstertruck::modeling::Face;
pub use surface::{SurfaceMesh, surface_mesh};

pub struct TetMesh {
    pub nodes: Vec<Point3>,
    pub tets: Vec<[u32; 4]>,
    pub boundary: Vec<([u32; 3], usize)>,
    pub faces: usize,
    pub surfaces: Vec<Face>,
    pub uvs: HashMap<u32, Vec<(usize, Vector2)>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Quality {
    pub nodes: usize,
    pub tets: usize,
    pub volume: f64,
    pub min_dihedral: f64,
    pub max_ratio: f64,
    pub slivers: usize,
}

fn p3(p: Point) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}

fn sub(a: Point, b: Point) -> Vector3 {
    Vector3::new(a[0] - b[0], a[1] - b[1], a[2] - b[2])
}

pub fn circumcentre(q: [Point; 4]) -> Option<Point> {
    let (a, b, c) = (sub(q[1], q[0]), sub(q[2], q[0]), sub(q[3], q[0]));
    let det = 2.0 * a.dot(b.cross(c));
    if det.abs() < 1.0e-300 {
        return None;
    }
    let v =
        (b.cross(c) * a.magnitude2() + c.cross(a) * b.magnitude2() + a.cross(b) * c.magnitude2())
            / det;
    Some([q[0][0] + v.x, q[0][1] + v.y, q[0][2] + v.z])
}

pub fn dihedrals(q: [Point; 4]) -> [f64; 6] {
    let normal = |i: usize| {
        let f = [0, 1, 2, 3]
            .into_iter()
            .filter(|&k| k != i)
            .collect::<Vec<_>>();
        let n = sub(q[f[1]], q[f[0]]).cross(sub(q[f[2]], q[f[0]]));
        let toward = sub(q[i], q[f[0]]);
        if n.dot(toward) > 0.0 { -n } else { n }
    };
    let n = [normal(0), normal(1), normal(2), normal(3)];
    let pairs = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
    pairs.map(|(i, j)| {
        let cos = n[i].dot(n[j]) / (n[i].magnitude() * n[j].magnitude()).max(1.0e-300);
        std::f64::consts::PI - cos.clamp(-1.0, 1.0).acos()
    })
}

fn shape(q: [Point; 4]) -> (f64, f64) {
    let shortest = (0..4)
        .flat_map(|i| (i + 1..4).map(move |j| (i, j)))
        .map(|(i, j)| sub(q[i], q[j]).magnitude())
        .fold(f64::INFINITY, f64::min);
    let radius = circumcentre(q).map_or(f64::INFINITY, |c| sub(c, q[0]).magnitude());
    (radius, radius / shortest)
}

fn morton(points: &[Point3], low: Point3, span: f64) -> Vec<usize> {
    let spread = |v: f64| -> u64 {
        let mut x = ((v.clamp(0.0, 1.0)) * 1_048_575.0) as u64;
        x = (x | (x << 32)) & 0x1f00000000ffff;
        x = (x | (x << 16)) & 0x1f0000ff0000ff;
        x = (x | (x << 8)) & 0x100f00f00f00f00f;
        x = (x | (x << 4)) & 0x10c30c30c30c30c3;
        (x | (x << 2)) & 0x1249249249249249
    };
    let mut order: Vec<(u64, usize)> = points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let r = (p - low) / span;
            (spread(r.x) | (spread(r.y) << 1) | (spread(r.z) << 2), i)
        })
        .collect();
    order.sort_unstable();
    order.into_iter().map(|(_, i)| i).collect()
}

const RATIO: f64 = 2.0;
const SLIVER: f64 = 10.0;

pub fn mesh_solid(solid: &Solid, size: f64) -> Result<TetMesh> {
    if size <= 0.0 {
        bail!("the mesh size must be above zero");
    }
    let mut surface = surface_mesh(solid, size)?;
    let bounds: BoundingBox<Point3> = surface.points.iter().copied().collect();
    let (low, high) = (bounds.min(), bounds.max());
    let span = (high - low).magnitude().max(1.0e-9);
    let mut dt = Delaunay::new([low.x, low.y, low.z], [high.x, high.y, high.z]);
    let mut id: Vec<u32> = vec![NONE; surface.points.len()];
    let place = |dt: &mut Delaunay, p: Point3| -> Result<u32> {
        match dt.insert([p.x, p.y, p.z], NONE, Mode::Free) {
            Insert::Added(v) | Insert::Existing(v) => Ok(v),
            _ => bail!("the mesher could not place a point at {p:?}"),
        }
    };
    for i in morton(&surface.points, low, span) {
        id[i] = place(&mut dt, surface.points[i])?;
    }
    let start = surface.points.len();
    for round in 0.. {
        let faces = dt.faces();
        let missing: Vec<usize> = (0..surface.triangles.len())
            .filter(|&t| !faces.contains(&face_key(surface.triangles[t].map(|v| id[v as usize]))))
            .collect();
        if missing.is_empty() {
            break;
        }
        if round > 40 || surface.points.len() > start * 6 + 1000 {
            bail!(
                "the mesher could not recover {} surface triangle(s); try a smaller `mesh` size",
                missing.len()
            );
        }
        let edges = surface.edges();
        let mut split: HashSet<[u32; 2]> = HashSet::default();
        for t in missing {
            let tri = surface.triangles[t];
            let longest = (0..3)
                .map(|k| {
                    let mut e = [tri[k], tri[(k + 1) % 3]];
                    e.sort_unstable();
                    e
                })
                .max_by(|a, b| {
                    let len = |e: &[u32; 2]| {
                        (surface.points[e[0] as usize] - surface.points[e[1] as usize]).magnitude()
                    };
                    len(a).total_cmp(&len(b))
                })
                .expect("three edges");
            split.insert(longest);
        }
        let mut split: Vec<[u32; 2]> = split.into_iter().collect();
        split.sort_unstable();
        let mut taken: HashSet<usize> = HashSet::default();
        for edge in split {
            let owners = &edges[&edge];
            if owners.iter().any(|t| !taken.insert(*t)) {
                continue;
            }
            let at = surface.split_edge(edge, owners);
            id.push(place(&mut dt, at)?);
            let fresh = surface.triangles.len() - owners.len();
            taken.extend(fresh..surface.triangles.len());
        }
        surface.delaunay_flips();
    }
    let mut skin = Skin::new(&surface, &id);
    dt.constrained = skin.index.keys().copied().collect();
    classify(&mut dt)?;
    let least = (size / 64.0).max(span * 1.0e-4);
    let limit = size * 0.75;
    let bad = |dt: &Delaunay, t: usize| -> bool {
        if dt.dead[t] || !dt.inside[t] {
            return false;
        }
        let (radius, ratio) = shape(dt.coords(dt.tets[t]));
        radius > limit || ratio > RATIO
    };
    let cap = dt.points.len() * 40 + 2_000_000;
    let mut queue: Vec<u32> = dt
        .alive()
        .filter(|&t| bad(&dt, t))
        .map(|t| t as u32)
        .collect();
    let mut rejected: HashSet<[u32; 4]> = HashSet::default();
    while let Some(t) = queue.pop() {
        if !bad(&dt, t as usize) || rejected.contains(&dt.tets[t as usize]) {
            continue;
        }
        if dt.points.len() > cap {
            break;
        }
        let Some(c) = circumcentre(dt.coords(dt.tets[t as usize])) else {
            rejected.insert(dt.tets[t as usize]);
            continue;
        };
        let outcome = dt.insert_from(c, t, Mode::Walls);
        match outcome {
            Insert::Added(_) => {
                let born = std::mem::take(&mut dt.born);
                queue.extend(born.iter().copied().filter(|&n| bad(&dt, n as usize)));
                dt.born = born;
            }
            Insert::Encroached(face) if skin.splittable(&dt, face, least) => {
                if skin.split(&mut dt, face) {
                    let born = std::mem::take(&mut dt.born);
                    queue.extend(born.iter().copied().filter(|&n| bad(&dt, n as usize)));
                    dt.born = born;
                    queue.push(t);
                } else {
                    skin.stuck.insert(face);
                    rejected.insert(dt.tets[t as usize]);
                }
            }
            _ => {
                rejected.insert(dt.tets[t as usize]);
            }
        }
    }
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for _ in 0..6 {
        let slivers: Vec<u32> = dt
            .alive()
            .filter(|&t| dt.inside[t] && sliver(dt.coords(dt.tets[t])))
            .map(|t| t as u32)
            .collect();
        if slivers.is_empty() {
            break;
        }
        for t in slivers {
            if dt.dead[t as usize] || !dt.inside[t as usize] {
                continue;
            }
            let q = dt.coords(dt.tets[t as usize]);
            let walled = (0..4).any(|i| {
                dt.constrained
                    .contains(&face_key(face_of(dt.tets[t as usize], i)))
            });
            if !walled {
                let centroid = [0, 1, 2].map(|k| (q[0][k] + q[1][k] + q[2][k] + q[3][k]) / 4.0);
                if let Insert::Added(_) = dt.insert_from(centroid, t, Mode::Walls) {
                    continue;
                }
            }
            let Some(c) = circumcentre(q) else { continue };
            let radius = sub(c, q[0]).magnitude();
            for attempt in 0..8 {
                if dt.dead[t as usize] {
                    break;
                }
                let jitter = if attempt == 0 { 0.0 } else { radius * 0.3 };
                let p = [0, 1, 2].map(|k| c[k] + jitter * (2.0 * random() - 1.0));
                match dt.insert_from(p, t, Mode::Walls) {
                    Insert::Added(_) => break,
                    Insert::Encroached(face) if skin.splittable(&dt, face, least) => {
                        if skin.split(&mut dt, face) {
                            break;
                        }
                        skin.stuck.insert(face);
                    }
                    _ => {}
                }
            }
        }
    }
    let mut tags: HashMap<[u32; 3], usize> = skin
        .index
        .iter()
        .map(|(key, &s)| (*key, skin.tags[s]))
        .collect();
    shed_flat_caps(&mut dt, &mut tags);
    let mut mesh = finish(&dt, &tags, &skin)?;
    mesh.smooth(4);
    mesh.flip(6);
    mesh.smooth(4);
    Ok(mesh)
}

struct Skin {
    tris: Vec<[u32; 3]>,
    tags: Vec<usize>,
    alive: Vec<bool>,
    index: HashMap<[u32; 3], usize>,
    edges: HashMap<[u32; 2], Vec<usize>>,
    uvs: HashMap<u32, Vec<(usize, Vector2)>>,
    faces: Vec<Face>,
    stuck: HashSet<[u32; 3]>,
}

fn edge_key(a: u32, b: u32) -> [u32; 2] {
    if a < b { [a, b] } else { [b, a] }
}

impl Skin {
    fn new(surface: &SurfaceMesh, id: &[u32]) -> Skin {
        let mut skin = Skin {
            tris: Vec::new(),
            tags: Vec::new(),
            alive: Vec::new(),
            index: HashMap::default(),
            edges: HashMap::default(),
            uvs: HashMap::default(),
            faces: surface.faces.clone(),
            stuck: HashSet::default(),
        };
        for (v, list) in surface.uvs.iter().enumerate() {
            skin.uvs.insert(id[v], list.clone());
        }
        for (t, tag) in surface.triangles.iter().zip(&surface.tags) {
            skin.add(t.map(|v| id[v as usize]), *tag);
        }
        skin
    }

    fn add(&mut self, tri: [u32; 3], tag: usize) {
        let s = self.tris.len();
        self.tris.push(tri);
        self.tags.push(tag);
        self.alive.push(true);
        self.index.insert(face_key(tri), s);
        for k in 0..3 {
            self.edges
                .entry(edge_key(tri[k], tri[(k + 1) % 3]))
                .or_default()
                .push(s);
        }
    }

    fn remove(&mut self, s: usize) {
        let tri = self.tris[s];
        self.alive[s] = false;
        self.index.remove(&face_key(tri));
        for k in 0..3 {
            let key = edge_key(tri[k], tri[(k + 1) % 3]);
            if let Some(list) = self.edges.get_mut(&key) {
                list.retain(|&x| x != s);
                if list.is_empty() {
                    self.edges.remove(&key);
                }
            }
        }
    }

    fn uv(&self, v: u32, face: usize) -> Option<Vector2> {
        self.uvs
            .get(&v)?
            .iter()
            .find(|(f, _)| *f == face)
            .map(|(_, uv)| *uv)
    }

    fn splittable(&self, dt: &Delaunay, face: [u32; 3], least: f64) -> bool {
        if self.stuck.contains(&face) {
            return false;
        }
        let Some(&s) = self.index.get(&face) else {
            return false;
        };
        let p = self.tris[s].map(|v| dt.points[v as usize]);
        (0..3).any(|k| sub(p[k], p[(k + 1) % 3]).magnitude() > least * 2.0)
    }

    fn on_surface(
        &self,
        face: usize,
        uv: Option<Vector2>,
        fallback: Point,
    ) -> (Point, Option<Vector2>) {
        match uv {
            Some(uv) => {
                let at = self.faces[face].surface().subs(uv.x, uv.y);
                ([at.x, at.y, at.z], Some(uv))
            }
            None => (fallback, None),
        }
    }

    fn split(&mut self, dt: &mut Delaunay, face: [u32; 3]) -> bool {
        self.split_at(dt, face, true) || self.split_at(dt, face, false)
    }

    fn split_at(&mut self, dt: &mut Delaunay, face: [u32; 3], project: bool) -> bool {
        let Some(&s) = self.index.get(&face) else {
            return false;
        };
        let tri = self.tris[s];
        let tag = self.tags[s];
        let p = tri.map(|v| dt.points[v as usize]);
        let (u, w) = (sub(p[1], p[0]), sub(p[2], p[0]));
        let n = u.cross(w);
        let nn = n.magnitude2();
        let centre = if nn > 1.0e-300 {
            let offset = (n.cross(u) * w.magnitude2() + w.cross(n) * u.magnitude2()) / (2.0 * nn);
            Some(offset)
        } else {
            None
        };
        let bary = centre.map(|o| {
            let d00 = u.dot(u);
            let d01 = u.dot(w);
            let d11 = w.dot(w);
            let d20 = o.dot(u);
            let d21 = o.dot(w);
            let den = d00 * d11 - d01 * d01;
            let b1 = (d11 * d20 - d01 * d21) / den;
            let b2 = (d00 * d21 - d01 * d20) / den;
            [1.0 - b1 - b2, b1, b2]
        });
        let Cut {
            at: m,
            uv,
            crossing,
            tris: new_tris,
            edges,
        } = match bary {
            Some(b) if b.iter().all(|x| *x > 0.15) => {
                let flat = [0, 1, 2].map(|k| p[0][k] * b[0] + p[1][k] * b[1] + p[2][k] * b[2]);
                let uvs: Option<Vec<Vector2>> = tri.iter().map(|&v| self.uv(v, tag)).collect();
                let guess = uvs
                    .map(|q| q[0] * b[0] + q[1] * b[1] + q[2] * b[2])
                    .filter(|_| project);
                let (m, uv) = self.on_surface(tag, guess, flat);
                let [a, bb, c] = tri;
                Cut {
                    at: m,
                    uv: uv.map(|uv| (tag, uv)),
                    crossing: vec![s],
                    tris: vec![
                        ([a, bb, MARK], tag),
                        ([bb, c, MARK], tag),
                        ([c, a, MARK], tag),
                    ],
                    edges: vec![edge_key(a, bb), edge_key(bb, c), edge_key(c, a)],
                }
            }
            _ => {
                let (k, _) = (0..3)
                    .map(|k| (k, sub(p[k], p[(k + 1) % 3]).magnitude()))
                    .fold((0, -1.0), |best, x| if x.1 > best.1 { x } else { best });
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                let Some(owners) = self.edges.get(&edge_key(a, b)).cloned() else {
                    return false;
                };
                if owners.len() != 2 {
                    return false;
                }
                let chord = [0, 1, 2].map(|j| (p[k][j] + p[(k + 1) % 3][j]) / 2.0);
                let same = self.tags[owners[0]] == self.tags[owners[1]];
                let guess = match (same && project, self.uv(a, tag), self.uv(b, tag)) {
                    (true, Some(ua), Some(ub)) => Some((ua + ub) / 2.0),
                    _ => None,
                };
                let (m, uv) = self.on_surface(tag, guess, chord);
                let mut tris = Vec::new();
                let mut edges = Vec::new();
                for &o in &owners {
                    let t = self.tris[o];
                    let j = (0..3)
                        .find(|&j| edge_key(t[j], t[(j + 1) % 3]) == edge_key(a, b))
                        .expect("owner holds the edge");
                    let (x, y, z) = (t[j], t[(j + 1) % 3], t[(j + 2) % 3]);
                    tris.push(([x, MARK, z], self.tags[o]));
                    tris.push(([MARK, y, z], self.tags[o]));
                    edges.push(edge_key(x, z));
                    edges.push(edge_key(y, z));
                }
                Cut {
                    at: m,
                    uv: uv.map(|uv| (tag, uv)),
                    crossing: owners,
                    tris,
                    edges,
                }
            }
        };
        let crossing_keys: Vec<[u32; 3]> =
            crossing.iter().map(|&o| face_key(self.tris[o])).collect();
        let mut seeds = Vec::new();
        for key in &crossing_keys {
            let Some(pair) = dt.tets_of_face(*key) else {
                return false;
            };
            seeds.extend(pair);
        }
        let seed = seeds[0];
        let mode = Mode::Split {
            crossing: &crossing_keys,
            edges: &edges,
            seeds: &seeds,
        };
        let Insert::Added(v) = dt.insert_from(m, seed, mode) else {
            return false;
        };
        for &o in &crossing {
            self.remove(o);
        }
        for key in &crossing_keys {
            dt.constrained.remove(key);
        }
        if let Some((face, uv)) = uv {
            self.uvs.insert(v, vec![(face, uv)]);
        }
        for (tri, tag) in new_tris {
            let tri = tri.map(|x| if x == MARK { v } else { x });
            dt.constrained.insert(face_key(tri));
            self.add(tri, tag);
        }
        true
    }
}

const MARK: u32 = u32::MAX - 1;

struct Cut {
    at: Point,
    uv: Option<(usize, Vector2)>,
    crossing: Vec<usize>,
    tris: Vec<([u32; 3], usize)>,
    edges: Vec<[u32; 2]>,
}

fn sliver(q: [Point; 4]) -> bool {
    dihedrals(q).iter().any(|a| a.to_degrees() < SLIVER)
}

fn face_edges(f: [u32; 3]) -> [[u32; 2]; 3] {
    [0, 1, 2].map(|k| edge_key(f[k], f[(k + 1) % 3]))
}

fn shed_flat_caps(dt: &mut Delaunay, tags: &mut HashMap<[u32; 3], usize>) {
    let mut rims: HashMap<[u32; 2], i32> = HashMap::default();
    for f in tags.keys() {
        for e in face_edges(*f) {
            *rims.entry(e).or_default() += 1;
        }
    }
    for _ in 0..6 {
        let on_skin: HashSet<u32> = tags.keys().flat_map(|f| f.iter().copied()).collect();
        let mut changed = false;
        for t in 0..dt.tets.len() {
            if dt.dead[t] || !dt.inside[t] {
                continue;
            }
            let tet = dt.tets[t];
            if !tet.iter().all(|v| on_skin.contains(v)) {
                continue;
            }
            let least = dihedrals(dt.coords(tet))
                .into_iter()
                .fold(f64::INFINITY, f64::min)
                .to_degrees();
            if least >= 5.0 {
                continue;
            }
            let faces: Vec<(usize, [u32; 3])> =
                (0..4).map(|i| (i, face_key(face_of(tet, i)))).collect();
            let (walls, others): (Vec<_>, Vec<_>) =
                faces.into_iter().partition(|(_, f)| tags.contains_key(f));
            if walls.is_empty() || walls.len() == 4 {
                continue;
            }
            if others.iter().any(|&(i, _)| {
                let n = dt.adj[t][i];
                n == NONE || !dt.inside[n as usize]
            }) {
                continue;
            }
            let mut change: HashMap<[u32; 2], i32> = HashMap::default();
            for (_, f) in &walls {
                for e in face_edges(*f) {
                    *change.entry(e).or_default() -= 1;
                }
            }
            for (_, f) in &others {
                for e in face_edges(*f) {
                    *change.entry(e).or_default() += 1;
                }
            }
            if change
                .iter()
                .any(|(e, d)| !matches!(rims.get(e).copied().unwrap_or(0) + d, 0 | 2))
            {
                continue;
            }
            for (e, d) in change {
                *rims.entry(e).or_default() += d;
            }
            let tag = tags[&walls[0].1];
            dt.inside[t] = false;
            for (_, f) in &walls {
                tags.remove(f);
                dt.constrained.remove(f);
            }
            for (_, f) in others {
                tags.insert(f, tag);
                dt.constrained.insert(f);
            }
            changed = true;
        }
        if !changed {
            break;
        }
    }
}

fn classify(dt: &mut Delaunay) -> Result<()> {
    let count = dt.tets.len();
    let mut outside = vec![false; count];
    let mut stack: Vec<usize> = dt
        .alive()
        .filter(|&t| dt.tets[t].iter().any(|&v| dt.is_super(v)))
        .collect();
    stack.iter().for_each(|&t| outside[t] = true);
    while let Some(t) = stack.pop() {
        for i in 0..4 {
            let n = dt.adj[t][i];
            if n == NONE || outside[n as usize] {
                continue;
            }
            if dt.constrained.contains(&face_key(face_of(dt.tets[t], i))) {
                continue;
            }
            outside[n as usize] = true;
            stack.push(n as usize);
        }
    }
    for (t, out) in outside.iter().enumerate() {
        dt.inside[t] = !dt.dead[t] && !out;
    }
    let mut sides: HashMap<[u32; 3], (usize, usize)> = HashMap::default();
    for t in dt.alive() {
        for i in 0..4 {
            let key = face_key(face_of(dt.tets[t], i));
            if dt.constrained.contains(&key) {
                let entry = sides.entry(key).or_default();
                if dt.inside[t] {
                    entry.0 += 1;
                } else {
                    entry.1 += 1;
                }
            }
        }
    }
    let wrong = sides.values().filter(|&&(i, o)| i != 1 || o != 1).count();
    if wrong > 0 || dt.alive().all(|t| !dt.inside[t]) {
        bail!(
            "the surface mesh does not close around a volume ({wrong} faces do not separate inside from outside)"
        );
    }
    Ok(())
}

fn finish(dt: &Delaunay, tags: &HashMap<[u32; 3], usize>, skin: &Skin) -> Result<TetMesh> {
    let mut renumber: Vec<u32> = vec![NONE; dt.points.len()];
    let mut nodes = Vec::new();
    let mut tets = Vec::new();
    let mut boundary = Vec::new();
    for t in dt.alive().filter(|&t| dt.inside[t]) {
        let tet = dt.tets[t].map(|v| {
            if renumber[v as usize] == NONE {
                renumber[v as usize] = nodes.len() as u32;
                nodes.push(p3(dt.points[v as usize]));
            }
            renumber[v as usize]
        });
        tets.push(tet);
        for i in 0..4 {
            let face = face_of(dt.tets[t], i);
            if let Some(&tag) = tags.get(&face_key(face)) {
                let q = face.map(|v| dt.points[v as usize]);
                let opposite = dt.points[dt.tets[t][i] as usize];
                let mut out = face.map(|v| renumber[v as usize]);
                if orient(q[0], q[1], q[2], opposite) < 0.0 {
                    out.swap(1, 2);
                }
                boundary.push((out, tag));
            }
        }
    }
    let uvs = skin
        .uvs
        .iter()
        .filter(|(v, _)| renumber[**v as usize] != NONE)
        .map(|(v, list)| (renumber[*v as usize], list.clone()))
        .collect();
    Ok(TetMesh {
        nodes,
        tets,
        boundary,
        faces: skin.faces.len(),
        surfaces: skin.faces.clone(),
        uvs,
    })
}

pub fn mean_ratio(q: [Point; 4]) -> f64 {
    let six = orient(q[0], q[1], q[2], q[3]);
    if six <= 0.0 {
        return -1.0;
    }
    let volume = six / 6.0;
    let squares: f64 = (0..4)
        .flat_map(|i| (i + 1..4).map(move |j| (i, j)))
        .map(|(i, j)| sub(q[i], q[j]).magnitude2())
        .sum();
    12.0 * (3.0 * volume).powf(2.0 / 3.0) / squares
}

impl TetMesh {
    pub fn quadratic_nodes(&self) -> usize {
        let mut edges = HashSet::default();
        for tet in &self.tets {
            for i in 0..4 {
                for j in i + 1..4 {
                    let (a, b) = (tet[i].min(tet[j]), tet[i].max(tet[j]));
                    edges.insert((a, b));
                }
            }
        }
        self.nodes.len() + edges.len()
    }

    pub fn smooth(&mut self, passes: usize) {
        use rayon::prelude::*;
        let mut star: Vec<Vec<u32>> = vec![Vec::new(); self.nodes.len()];
        for (t, tet) in self.tets.iter().enumerate() {
            for &v in tet {
                star[v as usize].push(t as u32);
            }
        }
        let fixed: HashSet<u32> = self
            .boundary
            .iter()
            .flat_map(|(f, _)| f.iter().copied())
            .collect();
        let around: Vec<Vec<u32>> = (0..self.nodes.len())
            .into_par_iter()
            .map(|v| {
                let mut list: Vec<u32> = star[v]
                    .iter()
                    .flat_map(|&t| self.tets[t as usize])
                    .filter(|&u| u != v as u32)
                    .collect();
                list.sort_unstable();
                list.dedup();
                list
            })
            .collect();
        let mut colour = vec![u32::MAX; self.nodes.len()];
        let mut classes: Vec<Vec<u32>> = Vec::new();
        for v in 0..self.nodes.len() {
            if fixed.contains(&(v as u32)) {
                continue;
            }
            let mut used: Vec<u32> = around[v]
                .iter()
                .map(|&u| colour[u as usize])
                .filter(|&c| c != u32::MAX)
                .collect();
            used.sort_unstable();
            used.dedup();
            let c = (0..).find(|c| used.binary_search(c).is_err()).unwrap_or(0);
            colour[v] = c;
            if classes.len() <= c as usize {
                classes.resize(c as usize + 1, Vec::new());
            }
            classes[c as usize].push(v as u32);
        }
        let worst = |nodes: &[Point3], tets: &[u32], v: u32, at: Point3| -> f64 {
            tets.iter()
                .map(|&t| {
                    mean_ratio(self.tets[t as usize].map(|u| {
                        let p = if u == v { at } else { nodes[u as usize] };
                        [p.x, p.y, p.z]
                    }))
                })
                .fold(f64::INFINITY, f64::min)
        };
        let axes = [
            Vector3::unit_x(),
            -Vector3::unit_x(),
            Vector3::unit_y(),
            -Vector3::unit_y(),
            Vector3::unit_z(),
            -Vector3::unit_z(),
        ];
        for _ in 0..passes {
            let mut moved = 0;
            for class in &classes {
                let nodes = &self.nodes;
                let moves: Vec<(u32, Point3)> = class
                    .par_iter()
                    .filter_map(|&v| {
                        let tets = &star[v as usize];
                        let home = nodes[v as usize];
                        let before = worst(nodes, tets, v, home);
                        if before > 0.5 {
                            return None;
                        }
                        let ring = &around[v as usize];
                        let centre = ring
                            .iter()
                            .map(|&u| nodes[u as usize].to_vec())
                            .sum::<Vector3>()
                            / ring.len() as f64;
                        let mut best = (before, home);
                        for w in [1.0, 0.5, 0.25] {
                            let trial = home + (Point3::from_vec(centre) - home) * w;
                            let after = worst(nodes, tets, v, trial);
                            if after > best.0 + 1.0e-6 {
                                best = (after, trial);
                            }
                        }
                        let reach = ring
                            .iter()
                            .map(|&u| (nodes[u as usize] - home).magnitude())
                            .sum::<f64>()
                            / ring.len() as f64;
                        let mut step = reach * 0.1;
                        for _ in 0..12 {
                            let mut improved = false;
                            for axis in axes {
                                let trial = best.1 + axis * step;
                                let after = worst(nodes, tets, v, trial);
                                if after > best.0 + 1.0e-9 {
                                    best = (after, trial);
                                    improved = true;
                                }
                            }
                            if !improved {
                                step *= 0.5;
                            }
                        }
                        (best.1 != home).then_some((v, best.1))
                    })
                    .collect();
                moved += moves.len();
                for (v, p) in moves {
                    self.nodes[v as usize] = p;
                }
            }
            if moved == 0 {
                break;
            }
        }
    }

    pub fn points(&self, t: usize) -> [Point; 4] {
        self.tets[t].map(|v| {
            let p = self.nodes[v as usize];
            [p.x, p.y, p.z]
        })
    }

    pub fn quality(&self) -> Quality {
        let mut volume = 0.0;
        let mut min_dihedral = f64::INFINITY;
        let mut max_ratio: f64 = 0.0;
        let mut slivers = 0;
        for t in 0..self.tets.len() {
            let q = self.points(t);
            volume += orient(q[0], q[1], q[2], q[3]) / 6.0;
            let least = dihedrals(q)
                .into_iter()
                .fold(f64::INFINITY, f64::min)
                .to_degrees();
            min_dihedral = min_dihedral.min(least);
            if least < SLIVER {
                slivers += 1;
            }
            max_ratio = max_ratio.max(shape(q).1);
        }
        Quality {
            nodes: self.nodes.len(),
            tets: self.tets.len(),
            volume,
            min_dihedral,
            max_ratio,
            slivers,
        }
    }
}
