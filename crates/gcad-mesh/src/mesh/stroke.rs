use super::alpha::Alpha;
use super::brush::Profile;
use super::grid::{Grid, Surface};
use super::{
    Mesh, P3, Region, Selection, add, cross, dot, length, normalized, reflect, scale, sub,
};
use std::collections::{BinaryHeap, HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Draw,
    Clay,
    Inflate,
    Smooth,
    Pinch,
    Crease,
    Slash,
    Flatten,
    Trim,
    Polish,
    Move,
    SnakeHook,
    Nudge,
    Stamp,
}

pub const KINDS: &str = "draw (standard), clay (buildup), inflate, smooth, pinch, crease (dam_standard), slash, flatten (planar), trim, polish (hpolish), move, snake_hook, nudge or stamp (alpha)";

impl Kind {
    pub fn parse(text: &str) -> Option<Kind> {
        Some(match text {
            "draw" | "standard" => Kind::Draw,
            "clay" | "buildup" | "clay_buildup" => Kind::Clay,
            "inflate" => Kind::Inflate,
            "smooth" => Kind::Smooth,
            "pinch" => Kind::Pinch,
            "crease" | "dam_standard" | "dam" => Kind::Crease,
            "slash" => Kind::Slash,
            "flatten" | "planar" => Kind::Flatten,
            "trim" => Kind::Trim,
            "polish" | "hpolish" => Kind::Polish,
            "move" | "grab" => Kind::Move,
            "snake_hook" | "snakehook" => Kind::SnakeHook,
            "nudge" => Kind::Nudge,
            "stamp" | "alpha" | "noise" => Kind::Stamp,
            _ => return None,
        })
    }

    pub fn default_strength(self, size: f64) -> f64 {
        match self {
            Kind::Draw | Kind::Clay | Kind::Inflate | Kind::Stamp => 0.25 * size,
            Kind::Crease | Kind::Slash => 0.3 * size,
            Kind::Smooth | Kind::Flatten | Kind::Trim | Kind::Move | Kind::SnakeHook => 1.0,
            Kind::Pinch | Kind::Polish | Kind::Nudge => 0.5,
        }
    }

    fn sweeps(self) -> bool {
        !matches!(
            self,
            Kind::Smooth | Kind::Polish | Kind::Move | Kind::SnakeHook | Kind::Nudge
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Aim {
    Area,
    Vertex,
    Fixed(P3),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    pub kind: Kind,
    pub size: f64,
    pub strength: f64,
    pub profile: Profile,
    pub spacing: f64,
    pub lazy: f64,
    pub alpha: Option<Alpha>,
    pub geodesic: bool,
    pub aim: Aim,
    pub mirror: Option<usize>,
    pub detail: Option<f64>,
    pub pinch: f64,
}

impl Brush {
    pub fn new(kind: Kind, size: f64) -> Brush {
        Brush {
            kind,
            size,
            strength: kind.default_strength(size),
            profile: Profile::default(),
            spacing: 0.25,
            lazy: 0.0,
            alpha: None,
            geodesic: false,
            aim: Aim::Area,
            mirror: None,
            detail: None,
            pinch: match kind {
                Kind::Crease => 0.3,
                Kind::Slash => 0.6,
                _ => 0.0,
            },
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stroke {
    pub path: Vec<P3>,
    pub pressure: Vec<f64>,
    pub to: Option<P3>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Outcome {
    pub moved: usize,
    pub most: f64,
    pub added: usize,
}

struct Sample {
    at: P3,
    normal: P3,
    tangent: P3,
    s: f64,
}

#[derive(Clone, Copy)]
struct Touch {
    d: f64,
    on: P3,
    normal: P3,
    u: f64,
    v: f64,
    s: f64,
}

fn perpendicular(n: P3) -> P3 {
    let other = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    normalized(cross(n, other))
}

fn resample(path: &[P3], step: f64) -> Vec<P3> {
    let mut out = vec![path[0]];
    for w in path.windows(2) {
        let l = length(sub(w[1], w[0]));
        let n = (l / step).ceil().max(1.0) as usize;
        for k in 1..=n {
            out.push(add(w[0], scale(sub(w[1], w[0]), k as f64 / n as f64)));
        }
    }
    out
}

fn lazy(points: &[P3], amount: f64, size: f64) -> Vec<P3> {
    if amount <= 0.0 || points.len() < 3 {
        return points.to_vec();
    }
    let string = amount.clamp(0.0, 1.0) * 2.0 * size;
    let mut out = vec![points[0]];
    let mut q = points[0];
    for &p in &points[1..] {
        let d = length(sub(p, q));
        if d > string {
            q = add(q, scale(sub(p, q), (d - string) / d));
            out.push(q);
        }
    }
    let end = *points.last().expect("points");
    if length(sub(end, q)) > 1.0e-9 {
        let tail = length(sub(end, q));
        let steps = (tail / (size * 0.25)).ceil().max(1.0) as usize;
        for k in 1..=steps {
            out.push(add(q, scale(sub(end, q), k as f64 / steps as f64)));
        }
    }
    out
}

fn pressure_at(pressure: &[f64], t: f64) -> f64 {
    match pressure.len() {
        0 => 1.0,
        1 => pressure[0],
        n => {
            let x = t.clamp(0.0, 1.0) * (n - 1) as f64;
            let k = (x.floor() as usize).min(n - 2);
            pressure[k] + (pressure[k + 1] - pressure[k]) * (x - k as f64)
        }
    }
}

struct Local {
    grid: Grid,
    slack: f64,
    size: f64,
}

impl Local {
    fn new(mesh: &Mesh, size: f64) -> Local {
        Local {
            grid: Grid::new(&mesh.positions, size * 0.5),
            slack: 0.0,
            size,
        }
    }

    fn near(&mut self, mesh: &Mesh, p: P3, r: f64) -> Vec<(u32, f64)> {
        if self.slack > self.size {
            self.grid = Grid::new(&mesh.positions, self.size * 0.5);
            self.slack = 0.0;
        }
        let mut out = Vec::new();
        self.grid.near(&mesh.positions, p, r + self.slack, |i, _| {
            let d = length(sub(mesh.positions[i as usize], p));
            if d < r {
                out.push((i, d));
            }
        });
        out
    }
}

fn geodesic(
    mesh: &Mesh,
    around: &[Vec<u32>],
    seeds: &[(u32, f64)],
    limit: f64,
) -> HashMap<u32, f64> {
    let mut best: HashMap<u32, f64> = HashMap::new();
    let mut heap: BinaryHeap<std::cmp::Reverse<(u64, u32)>> = BinaryHeap::new();
    for &(i, d) in seeds {
        if best.get(&i).is_none_or(|&b| d < b) {
            best.insert(i, d);
            heap.push(std::cmp::Reverse((d.to_bits(), i)));
        }
    }
    while let Some(std::cmp::Reverse((key, i))) = heap.pop() {
        let d = f64::from_bits(key);
        if best.get(&i).is_some_and(|&b| d > b) {
            continue;
        }
        for &j in &around[i as usize] {
            let step = d + length(sub(mesh.positions[i as usize], mesh.positions[j as usize]));
            if step < limit && best.get(&j).is_none_or(|&b| step < b) {
                best.insert(j, step);
                heap.push(std::cmp::Reverse((step.to_bits(), j)));
            }
        }
    }
    best
}

impl Mesh {
    pub fn stroke(&mut self, brush: &Brush, stroke: &Stroke) -> Outcome {
        let mut outcome = Outcome::default();
        if stroke.path.is_empty() {
            return outcome;
        }
        let step = (brush.spacing * brush.size).max(brush.size * 0.02);
        let raw = lazy(&resample(&stroke.path, step), brush.lazy, brush.size);
        if let Some(detail) = brush.detail {
            outcome.added += self.refine_under(&raw, brush, detail);
        }
        let before = self.positions.clone();
        let mut paths = vec![raw.clone()];
        if let Some(axis) = brush.mirror {
            let other: Vec<P3> = raw.iter().map(|&p| reflect(p, axis)).collect();
            if other
                .iter()
                .zip(&raw)
                .any(|(a, b)| length(sub(*a, *b)) > brush.size * 0.25)
            {
                paths.push(other);
            }
        }
        let to = stroke.to;
        match brush.kind {
            k if k.sweeps() => self.sweep(brush, &paths, &stroke.pressure),
            Kind::Move => {
                let by = match to {
                    Some(to) => sub(to, stroke.path[0]),
                    None => sub(*stroke.path.last().expect("path"), stroke.path[0]),
                };
                self.grab(brush, &paths, by)
            }
            Kind::SnakeHook => {
                let mut hook = stroke.path.clone();
                if let Some(to) = to {
                    hook.push(to);
                }
                let hook = lazy(&resample(&hook, step), brush.lazy, brush.size);
                let mut hooks = vec![hook.clone()];
                if paths.len() > 1 {
                    let axis = brush.mirror.expect("mirrored");
                    hooks.push(hook.iter().map(|&p| reflect(p, axis)).collect());
                }
                outcome.added += self.snake(brush, &hooks);
            }
            _ => self.dabs(brush, &paths),
        }
        let n = before.len();
        for (i, p) in self.positions.iter().enumerate() {
            let d = if i < n {
                length(sub(*p, before[i]))
            } else {
                0.0
            };
            if d > 1.0e-12 {
                outcome.moved += 1;
                outcome.most = outcome.most.max(d);
            }
        }
        outcome
    }

    fn free(&self, i: usize) -> f64 {
        1.0 - self.mask_at(i).clamp(0.0, 1.0)
    }

    fn refine_under(&mut self, path: &[P3], brush: &Brush, detail: f64) -> usize {
        let mut regions: Vec<Region> = Vec::new();
        let mut last: Option<P3> = None;
        for &p in path {
            if last.is_none_or(|q| length(sub(p, q)) > brush.size * 0.5) {
                regions.push(Region::Sphere(p, brush.size * 1.25));
                last = Some(p);
            }
        }
        if let Some(axis) = brush.mirror {
            let mirrored: Vec<Region> = regions
                .iter()
                .map(|r| match r {
                    Region::Sphere(c, r) => Region::Sphere(reflect(*c, axis), *r),
                    other => other.clone(),
                })
                .collect();
            regions.extend(mirrored);
        }
        let Ok(selection) = Selection::new(self, regions, 0.0, None) else {
            return 0;
        };
        let (mean, _) = self.edge_lengths(&selection);
        let rounds = ((mean / detail).log2().ceil().max(0.0) as usize + 1).min(8);
        self.refine_to(&selection, detail, rounds)
    }

    fn samples(
        &self,
        path: &[P3],
        brush: &Brush,
        normals: &[P3],
        local: &mut Local,
    ) -> Vec<Sample> {
        let surface = Surface::new(self);
        let mut out: Vec<Sample> = Vec::with_capacity(path.len());
        let mut s = 0.0;
        for (k, &p) in path.iter().enumerate() {
            let hit = surface.closest(p);
            let at = hit.as_ref().map_or(p, |h| h.point);
            let face = hit.as_ref().map_or([0.0, 0.0, 1.0], |h| {
                let [a, b, c] = self.corners(self.triangles[h.triangle as usize]);
                normalized(cross(sub(b, a), sub(c, a)))
            });
            let normal = match brush.aim {
                Aim::Fixed(n) => normalized(n),
                _ => {
                    let sum = local.near(self, at, brush.size).into_iter().fold(
                        [0.0; 3],
                        |acc, (i, d)| {
                            add(
                                acc,
                                scale(normals[i as usize], brush.profile.weight(d / brush.size)),
                            )
                        },
                    );
                    if length(sum) > 1.0e-9 {
                        normalized(sum)
                    } else {
                        face
                    }
                }
            };
            if let Some(prev) = out.last() {
                s += length(sub(at, prev.at));
            }
            let toward = match (path.get(k + 1), k.checked_sub(1).and_then(|j| path.get(j))) {
                (Some(&next), _) => sub(next, p),
                (None, Some(&prev)) => sub(p, prev),
                _ => perpendicular(normal),
            };
            let along = sub(toward, scale(normal, dot(toward, normal)));
            let tangent = if length(along) > 1.0e-12 {
                normalized(along)
            } else {
                perpendicular(normal)
            };
            out.push(Sample {
                at,
                normal,
                tangent,
                s,
            });
        }
        out
    }

    fn touches(
        &self,
        brush: &Brush,
        samples: &[Sample],
        local: &mut Local,
        around: Option<&[Vec<u32>]>,
    ) -> HashMap<u32, Touch> {
        let r = brush.size;
        let mut found: HashMap<u32, Touch> = HashMap::new();
        let single = samples.len() == 1;
        for (k, sample) in samples.iter().enumerate() {
            let reach = r + samples
                .get(k + 1)
                .map_or(0.0, |n| length(sub(n.at, sample.at)));
            for (i, _) in local.near(self, sample.at, reach) {
                let p = self.positions[i as usize];
                let segments = [k.checked_sub(1), Some(k)];
                for j in segments.into_iter().flatten() {
                    let (a, b) = match samples.get(j + 1) {
                        Some(b) if !single => (&samples[j], b),
                        _ => (&samples[j], &samples[j]),
                    };
                    let ab = sub(b.at, a.at);
                    let l2 = dot(ab, ab);
                    let t = if l2 > 1.0e-18 {
                        (dot(sub(p, a.at), ab) / l2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let on = add(a.at, scale(ab, t));
                    let d = length(sub(p, on));
                    if d >= r || found.get(&i).is_some_and(|f| f.d <= d) {
                        continue;
                    }
                    let normal = normalized(add(scale(a.normal, 1.0 - t), scale(b.normal, t)));
                    let tangent = a.tangent;
                    let side = cross(normal, tangent);
                    let off = sub(p, on);
                    let (u, v) = if single {
                        (dot(off, tangent), dot(off, side))
                    } else {
                        (a.s + t * (b.s - a.s), dot(off, side))
                    };
                    found.insert(
                        i,
                        Touch {
                            d,
                            on,
                            normal,
                            u,
                            v,
                            s: a.s + t * (b.s - a.s),
                        },
                    );
                }
            }
        }
        if let (true, Some(around)) = (brush.geodesic, around) {
            let seeds: Vec<(u32, f64)> = samples
                .iter()
                .filter_map(|s| {
                    local
                        .near(self, s.at, r)
                        .into_iter()
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                })
                .collect();
            let surface = geodesic(self, around, &seeds, r);
            found.retain(|i, t| match surface.get(i) {
                Some(&d) => {
                    t.d = d.max(t.d);
                    true
                }
                None => false,
            });
        }
        found
    }

    fn sweep(&mut self, brush: &Brush, paths: &[Vec<P3>], pressure: &[f64]) {
        let normals = self.vertex_normals();
        let around = brush.geodesic.then(|| self.neighbours());
        let mut local = Local::new(self, brush.size);
        let mut best: HashMap<u32, (Touch, f64)> = HashMap::new();
        for path in paths {
            let samples = self.samples(path, brush, &normals, &mut local);
            let total = samples.last().map_or(0.0, |s| s.s).max(1.0e-12);
            for (i, t) in self.touches(brush, &samples, &mut local, around.as_deref()) {
                if best.get(&i).is_none_or(|(b, _)| t.d < b.d) {
                    best.insert(i, (t, total));
                }
            }
        }
        let h = brush.strength;
        let fallback = brush.size * 0.5;
        for (i, (t, total)) in best {
            let i = i as usize;
            let w = brush.profile.weight(t.d / brush.size)
                * pressure_at(pressure, t.s / total)
                * self.free(i);
            if w <= 0.0 {
                continue;
            }
            let p = self.positions[i];
            let n = match brush.aim {
                Aim::Vertex => normals[i],
                _ => t.normal,
            };
            let height = dot(sub(p, t.on), n);
            let lateral = {
                let pull = sub(t.on, p);
                sub(pull, scale(n, dot(pull, n)))
            };
            let alpha = brush
                .alpha
                .as_ref()
                .map_or(1.0, |a| a.value(t.u, t.v, p, fallback));
            let moved = match brush.kind {
                Kind::Draw => add(p, scale(n, h * w * alpha)),
                Kind::Inflate => add(p, scale(normals[i], h * w * alpha)),
                Kind::Stamp => add(p, scale(n, h * w * alpha)),
                Kind::Clay => {
                    let top = h * alpha;
                    if height < top {
                        add(p, scale(n, (top - height) * w))
                    } else {
                        p
                    }
                }
                Kind::Crease | Kind::Slash => add(
                    sub(p, scale(n, h * w * alpha)),
                    scale(lateral, brush.pinch * w),
                ),
                Kind::Pinch => add(p, scale(lateral, (h * w).clamp(-1.0, 1.0))),
                Kind::Flatten => sub(p, scale(n, height * (h * w).min(1.0))),
                Kind::Trim => {
                    if height > 0.0 {
                        sub(p, scale(n, height * (h * w).min(1.0)))
                    } else {
                        p
                    }
                }
                _ => p,
            };
            self.positions[i] = moved;
        }
    }

    fn grab(&mut self, brush: &Brush, paths: &[Vec<P3>], by: P3) {
        let normals = self.vertex_normals();
        let around = brush.geodesic.then(|| self.neighbours());
        let mut local = Local::new(self, brush.size);
        let mut moves: Vec<(usize, P3)> = Vec::new();
        for (k, path) in paths.iter().enumerate() {
            let start = self.samples(&path[..1], brush, &normals, &mut local);
            let shift = if k == 0 {
                by
            } else {
                reflect(by, brush.mirror.expect("mirrored"))
            };
            for (i, t) in self.touches(brush, &start, &mut local, around.as_deref()) {
                let w =
                    brush.profile.weight(t.d / brush.size) * brush.strength * self.free(i as usize);
                moves.push((i as usize, scale(shift, w)));
            }
        }
        for (i, d) in moves {
            self.positions[i] = add(self.positions[i], d);
        }
    }

    fn snake(&mut self, brush: &Brush, hooks: &[Vec<P3>]) -> usize {
        let mut added = 0;
        let steps = hooks[0].len();
        let mut local = Local::new(self, brush.size);
        let mut grabbed: Vec<Vec<(u32, f64)>> = hooks
            .iter()
            .map(|hook| {
                local
                    .near(self, hook[0], brush.size)
                    .into_iter()
                    .map(|(i, d)| {
                        (
                            i,
                            brush.profile.weight(d / brush.size)
                                * brush.strength
                                * self.free(i as usize),
                        )
                    })
                    .collect()
            })
            .collect();
        for k in 1..steps {
            for (h, hook) in hooks.iter().enumerate() {
                let delta = sub(hook[k], hook[k - 1]);
                for &(i, w) in &grabbed[h] {
                    let p = &mut self.positions[i as usize];
                    *p = add(*p, scale(delta, w));
                }
                local.slack += length(delta);
            }
            if let Some(detail) = brush.detail {
                let before = self.positions.len();
                let regions = hooks
                    .iter()
                    .map(|hook| Region::Sphere(hook[k], brush.size * 1.5))
                    .collect();
                if let Ok(selection) = Selection::new(self, regions, 0.0, None) {
                    added += self.refine_to(&selection, detail, 2);
                }
                if self.positions.len() > before {
                    local = Local::new(self, brush.size);
                    grabbed = hooks
                        .iter()
                        .map(|hook| {
                            let tip = hook[k];
                            local
                                .near(self, tip, brush.size)
                                .into_iter()
                                .map(|(i, d)| {
                                    (
                                        i,
                                        brush.profile.weight(d / brush.size)
                                            * brush.strength
                                            * self.free(i as usize),
                                    )
                                })
                                .collect()
                        })
                        .collect();
                }
            }
        }
        added
    }

    fn dabs(&mut self, brush: &Brush, paths: &[Vec<P3>]) {
        let around = self.neighbours();
        let faces = self.faces_of();
        let mut normals = self.vertex_normals();
        let mut local = Local::new(self, brush.size);
        let count = paths.iter().map(Vec::len).max().unwrap_or(0);
        let sampled: Vec<Vec<Sample>> = paths
            .iter()
            .map(|path| self.samples(path, brush, &normals, &mut local))
            .collect();
        for k in 0..count {
            for samples in &sampled {
                let Some(sample) = samples.get(k) else {
                    continue;
                };
                let touched: Vec<(usize, f64)> = local
                    .near(self, sample.at, brush.size)
                    .into_iter()
                    .map(|(i, d)| {
                        (
                            i as usize,
                            brush.profile.weight(d / brush.size) * self.free(i as usize),
                        )
                    })
                    .filter(|(_, w)| *w > 0.0)
                    .collect();
                let step = match samples.get(k + 1) {
                    Some(next) => length(sub(next.at, sample.at)),
                    None => 0.0,
                };
                let most = self.dab(brush, sample, &touched, &around, &normals, step);
                local.slack += most;
                let moved: Vec<usize> = touched.iter().map(|(i, _)| *i).collect();
                self.refresh_normals(&moved, &faces, &mut normals);
            }
        }
    }

    fn dab(
        &mut self,
        brush: &Brush,
        sample: &Sample,
        touched: &[(usize, f64)],
        around: &[Vec<u32>],
        normals: &[P3],
        step: f64,
    ) -> f64 {
        let before: Vec<P3> = touched.iter().map(|&(i, _)| self.positions[i]).collect();
        match brush.kind {
            Kind::Smooth => {
                let passes = brush.strength.ceil().max(1.0) as usize;
                let factor = brush.strength / passes as f64;
                for _ in 0..passes {
                    let targets: Vec<P3> = touched
                        .iter()
                        .map(|&(i, w)| {
                            let ring = &around[i];
                            if ring.is_empty() {
                                return self.positions[i];
                            }
                            let sum = ring
                                .iter()
                                .fold([0.0; 3], |s, &j| add(s, self.positions[j as usize]));
                            let mean = scale(sum, 1.0 / ring.len() as f64);
                            let p = self.positions[i];
                            add(p, scale(sub(mean, p), (factor * w).min(1.0)))
                        })
                        .collect();
                    for (&(i, _), q) in touched.iter().zip(targets) {
                        self.positions[i] = q;
                    }
                }
            }
            Kind::Polish => {
                let (mut c, mut total) = ([0.0; 3], 0.0);
                for &(i, w) in touched {
                    c = add(c, scale(self.positions[i], w));
                    total += w;
                }
                if total > 0.0 {
                    let c = scale(c, 1.0 / total);
                    let n = sample.normal;
                    for &(i, w) in touched {
                        let p = self.positions[i];
                        let h = dot(sub(p, c), n);
                        self.positions[i] = sub(p, scale(n, h * (brush.strength * w).min(1.0)));
                    }
                }
            }
            Kind::Nudge => {
                for &(i, w) in touched {
                    let n = normals[i];
                    let t = sub(sample.tangent, scale(n, dot(sample.tangent, n)));
                    self.positions[i] = add(self.positions[i], scale(t, step * brush.strength * w));
                }
            }
            _ => {}
        }
        touched
            .iter()
            .zip(before)
            .map(|(&(i, _), p)| length(sub(self.positions[i], p)))
            .fold(0.0, f64::max)
    }

    pub fn faces_of(&self) -> Vec<Vec<u32>> {
        let mut faces = vec![Vec::new(); self.positions.len()];
        for (f, t) in self.triangles.iter().enumerate() {
            for &i in t {
                faces[i as usize].push(f as u32);
            }
        }
        faces
    }

    fn refresh_normals(&self, moved: &[usize], faces: &[Vec<u32>], normals: &mut [P3]) {
        let mut ring: HashSet<usize> = HashSet::new();
        for &i in moved {
            for &f in &faces[i] {
                ring.extend(self.triangles[f as usize].iter().map(|&v| v as usize));
            }
        }
        for i in ring {
            let sum = faces[i].iter().fold([0.0; 3], |s, &f| {
                let [a, b, c] = self.corners(self.triangles[f as usize]);
                add(s, cross(sub(b, a), sub(c, a)))
            });
            normals[i] = normalized(sum);
        }
    }
}
