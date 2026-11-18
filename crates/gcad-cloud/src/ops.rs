use crate::cloud::{Cloud, P3, Point};
use crate::kdtree::KdTree;
use anyhow::{Result, bail};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};

pub type M4 = [[f64; 4]; 4];

pub const IDENTITY: M4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

#[derive(Clone, Debug)]
pub enum Region {
    Box { lo: P3, hi: P3 },
    Sphere { centre: P3, radius: f64 },
    Plane { origin: P3, normal: P3, above: bool },
    Prism(Vec<[f64; 2]>),
}

#[derive(Clone, Copy, Debug)]
pub enum Orient {
    Tree,
    Toward(P3),
    Origins,
}

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0xd1b5_4a32_d192_ed03)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

pub fn par_map<T: Send, F: Fn(usize) -> T + Sync>(n: usize, f: F) -> Vec<T> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let threads = std::thread::available_parallelism().map_or(1, |v| v.get()).min(16);
        if n >= 2048 && threads > 1 {
            let chunk = n.div_ceil(threads);
            let f = &f;
            return std::thread::scope(|scope| {
                let handles: Vec<_> = (0..threads)
                    .map(|t| {
                        scope.spawn(move || {
                            (t * chunk..((t + 1) * chunk).min(n))
                                .map(f)
                                .collect::<Vec<T>>()
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|h| h.join().expect("a worker thread"))
                    .collect()
            });
        }
    }
    (0..n).map(f).collect()
}

pub(crate) fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn unit_or_zero(v: P3) -> P3 {
    let length = dot(v, v).sqrt();
    if length < 1.0e-300 {
        [0.0; 3]
    } else {
        [v[0] / length, v[1] / length, v[2] / length]
    }
}

fn polygon_contains(polygon: &[[f64; 2]], x: f64, y: f64) -> bool {
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[j]);
        if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

impl Region {
    pub fn contains(&self, p: P3) -> bool {
        match self {
            Region::Box { lo, hi } => (0..3).all(|k| p[k] >= lo[k] && p[k] <= hi[k]),
            Region::Sphere { centre, radius } => {
                let d = sub(p, *centre);
                dot(d, d) <= radius * radius
            }
            Region::Plane {
                origin,
                normal,
                above,
            } => {
                let side = dot(sub(p, *origin), *normal);
                if *above { side >= 0.0 } else { side <= 0.0 }
            }
            Region::Prism(polygon) => polygon_contains(polygon, p[0], p[1]),
        }
    }
}

pub fn crop(cloud: &Cloud, region: &Region) -> Result<Cloud> {
    if let Region::Prism(polygon) = region {
        if polygon.len() < 3 {
            bail!("a crop polygon needs at least 3 points, got {}", polygon.len());
        }
    }
    if let Region::Sphere { radius, .. } = region {
        if *radius <= 0.0 {
            bail!("a crop sphere needs a positive radius, got {radius}");
        }
    }
    let kept: Vec<usize> = (0..cloud.len())
        .filter(|&i| region.contains(cloud.positions[i]))
        .collect();
    Ok(cloud.select(&kept))
}

#[derive(Default)]
struct CellHasher(u64);

impl Hasher for CellHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }

    fn write_i64(&mut self, value: i64) {
        self.write_u64(value as u64);
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn finish(&self) -> u64 {
        self.0 ^ (self.0 >> 29)
    }
}

struct Cell {
    first: usize,
    count: u32,
    position: P3,
    normal: P3,
    colour: [u32; 3],
    intensity: f64,
}

pub fn voxel_downsample(cloud: &Cloud, size: f64) -> Result<Cloud> {
    if !(size > 0.0) || !size.is_finite() {
        bail!("`voxel` must be a positive size, got {size}");
    }
    let mut index: HashMap<(i64, i64, i64), u32, BuildHasherDefault<CellHasher>> =
        HashMap::default();
    let mut cells: Vec<Cell> = Vec::new();
    for (i, p) in cloud.positions.iter().enumerate() {
        let key = (
            (p[0] / size).floor() as i64,
            (p[1] / size).floor() as i64,
            (p[2] / size).floor() as i64,
        );
        let slot = *index.entry(key).or_insert_with(|| {
            cells.push(Cell {
                first: i,
                count: 0,
                position: [0.0; 3],
                normal: [0.0; 3],
                colour: [0; 3],
                intensity: 0.0,
            });
            cells.len() as u32 - 1
        });
        let cell = &mut cells[slot as usize];
        cell.count += 1;
        (0..3).for_each(|k| cell.position[k] += p[k]);
        if let Some(normals) = &cloud.normals {
            (0..3).for_each(|k| cell.normal[k] += normals[i][k] as f64);
        }
        if let Some(colours) = &cloud.colours {
            (0..3).for_each(|k| cell.colour[k] += colours[i][k] as u32);
        }
        if let Some(intensity) = &cloud.intensity {
            cell.intensity += intensity[i] as f64;
        }
    }
    let mut out = Cloud::with_layout(cloud.layout(), cells.len());
    for cell in &cells {
        let n = cell.count as f64;
        let mut point = cloud.point(cell.first);
        point.position = cell.position.map(|v| v / n);
        if point.normal.is_some() {
            point.normal = Some(unit_or_zero(cell.normal).map(|v| v as f32));
        }
        if point.colour.is_some() {
            point.colour = Some(cell.colour.map(|v| ((v as f64 / n).round()) as u8));
        }
        if point.intensity.is_some() {
            point.intensity = Some((cell.intensity / n) as f32);
        }
        out.push(point);
    }
    Ok(out)
}

pub fn every(cloud: &Cloud, step: usize) -> Result<Cloud> {
    if step == 0 {
        bail!("`every` must be at least 1");
    }
    let kept: Vec<usize> = (0..cloud.len()).step_by(step).collect();
    Ok(cloud.select(&kept))
}

pub fn sample_count(cloud: &Cloud, count: usize, seed: u64) -> Result<Cloud> {
    if count == 0 {
        bail!("`count` must be at least 1");
    }
    if count >= cloud.len() {
        return Ok(cloud.clone());
    }
    let mut rng = Rng::new(seed);
    let mut kept = Vec::with_capacity(count);
    for i in 0..cloud.len() {
        let needed = (count - kept.len()) as f64;
        let left = (cloud.len() - i) as f64;
        if rng.unit() * left < needed {
            kept.push(i);
            if kept.len() == count {
                break;
            }
        }
    }
    Ok(cloud.select(&kept))
}

pub fn denoise_stat(cloud: &Cloud, k: usize, sigma: f64) -> Result<Cloud> {
    if k == 0 {
        bail!("`k` must be at least 1");
    }
    if !(sigma >= 0.0) {
        bail!("`sigma` must not be negative, got {sigma}");
    }
    if cloud.len() <= k + 1 {
        return Ok(cloud.clone());
    }
    let tree = KdTree::build(&cloud.positions);
    let means = par_map(cloud.len(), |i| {
        let found = tree.nearest(cloud.positions[i], k + 1);
        let (sum, count) = found
            .iter()
            .filter(|(j, _)| *j != i)
            .take(k)
            .fold((0.0, 0usize), |(s, c), (_, d)| (s + d, c + 1));
        sum / count.max(1) as f64
    });
    let n = means.len() as f64;
    let mean = means.iter().sum::<f64>() / n;
    let variance = means.iter().map(|m| (m - mean) * (m - mean)).sum::<f64>() / n;
    let limit = mean + sigma * variance.sqrt();
    let kept: Vec<usize> = (0..cloud.len()).filter(|&i| means[i] <= limit).collect();
    Ok(cloud.select(&kept))
}

pub fn denoise_radius(cloud: &Cloud, radius: f64, min: usize) -> Result<Cloud> {
    if !(radius > 0.0) {
        bail!("`radius` must be positive, got {radius}");
    }
    let tree = KdTree::build(&cloud.positions);
    let crowded = par_map(cloud.len(), |i| {
        tree.within(cloud.positions[i], radius).len().saturating_sub(1) >= min
    });
    let kept: Vec<usize> = (0..cloud.len()).filter(|&i| crowded[i]).collect();
    Ok(cloud.select(&kept))
}

pub(crate) fn eigen_symmetric(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..40 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        let scale = a[0][0].abs() + a[1][1].abs() + a[2][2].abs();
        if off <= 1.0e-14 * scale || scale == 0.0 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1.0e-300 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for k in 0..3 {
                let (akp, akq) = (a[k][p], a[k][q]);
                a[k][p] = c * akp - s * akq;
                a[k][q] = s * akp + c * akq;
            }
            for k in 0..3 {
                let (apk, aqk) = (a[p][k], a[q][k]);
                a[p][k] = c * apk - s * aqk;
                a[q][k] = s * apk + c * aqk;
            }
            for row in v.iter_mut() {
                let (vp, vq) = (row[p], row[q]);
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    ([a[0][0], a[1][1], a[2][2]], v)
}

pub(crate) struct Fit {
    pub(crate) centroid: P3,
    pub(crate) normal: P3,
    pub(crate) variation: f64,
    pub(crate) spread: f64,
}

pub(crate) fn fit_plane(points: &[P3]) -> Fit {
    let n = points.len() as f64;
    let centroid = points
        .iter()
        .fold([0.0; 3], |s, p| [s[0] + p[0], s[1] + p[1], s[2] + p[2]])
        .map(|v| v / n);
    let mut covariance = [[0.0; 3]; 3];
    for p in points {
        let d = sub(*p, centroid);
        for r in 0..3 {
            for c in 0..3 {
                covariance[r][c] += d[r] * d[c];
            }
        }
    }
    covariance.iter_mut().flatten().for_each(|v| *v /= n);
    let (values, vectors) = eigen_symmetric(covariance);
    let lowest = (0..3)
        .min_by(|&a, &b| values[a].total_cmp(&values[b]))
        .unwrap_or(0);
    let total: f64 = values.iter().sum();
    Fit {
        centroid,
        normal: unit_or_zero([vectors[0][lowest], vectors[1][lowest], vectors[2][lowest]]),
        variation: if total > 0.0 { values[lowest].max(0.0) / total } else { 0.0 },
        spread: total,
    }
}

const CREASE: f64 = 0.5;
const SHARP: f64 = 0.003;
const CANDIDATES: usize = 48;

fn fit_sharp(points: &[P3], first: &Fit, seed: u64) -> P3 {
    let tolerance = 0.1 * first.spread.sqrt();
    let inliers = |origin: P3, normal: P3| -> Vec<P3> {
        points
            .iter()
            .copied()
            .filter(|p| dot(normal, sub(*p, origin)).abs() <= tolerance)
            .collect()
    };
    let mut rng = Rng::new(seed);
    let mut best: Option<(usize, P3, P3)> = None;
    for _ in 0..CANDIDATES {
        let (a, b, c) = (
            points[rng.below(points.len())],
            points[rng.below(points.len())],
            points[rng.below(points.len())],
        );
        let normal = cross(sub(b, a), sub(c, a));
        if dot(normal, normal).sqrt() < 1.0e-9 * first.spread.max(1.0e-300) {
            continue;
        }
        let normal = unit_or_zero(normal);
        let count = inliers(a, normal).len();
        if best.is_none_or(|(most, _, _)| count > most) {
            best = Some((count, a, normal));
        }
    }
    let Some((count, mut origin, mut normal)) = best else {
        return first.normal;
    };
    if count < 3 {
        return first.normal;
    }
    for _ in 0..4 {
        let chosen = inliers(origin, normal);
        if chosen.len() < 3 {
            break;
        }
        let fit = fit_plane(&chosen);
        origin = fit.centroid;
        normal = fit.normal;
    }
    normal
}

struct Graph {
    offsets: Vec<usize>,
    targets: Vec<u32>,
}

fn symmetric_graph(neighbours: &[Vec<u32>]) -> Graph {
    let n = neighbours.len();
    let mut degree = vec![0usize; n + 1];
    for (i, list) in neighbours.iter().enumerate() {
        for &j in list {
            degree[i] += 1;
            degree[j as usize] += 1;
        }
    }
    let mut offsets = vec![0usize; n + 1];
    for i in 0..n {
        offsets[i + 1] = offsets[i] + degree[i];
    }
    let mut filled = offsets.clone();
    let mut targets = vec![0u32; offsets[n]];
    for (i, list) in neighbours.iter().enumerate() {
        for &j in list {
            targets[filled[i]] = j;
            filled[i] += 1;
            targets[filled[j as usize]] = i as u32;
            filled[j as usize] += 1;
        }
    }
    Graph { offsets, targets }
}

fn orient_by_tree(positions: &[P3], normals: &mut [P3], neighbours: &[Vec<u32>]) {
    let graph = symmetric_graph(neighbours);
    let n = positions.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| positions[b][2].total_cmp(&positions[a][2]));
    let mut done = vec![false; n];
    let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    let push = |heap: &mut BinaryHeap<Reverse<(u32, u32, u32)>>,
                done: &[bool],
                normals: &[P3],
                from: usize| {
        for &to in &graph.targets[graph.offsets[from]..graph.offsets[from + 1]] {
            if !done[to as usize] {
                let weight = (1.0 - dot(normals[from], normals[to as usize]).abs()) as f32;
                heap.push(Reverse((weight.max(0.0).to_bits(), to, from as u32)));
            }
        }
    };
    for &root in &order {
        if done[root] {
            continue;
        }
        if normals[root][2] < 0.0 {
            normals[root] = normals[root].map(|v| -v);
        }
        done[root] = true;
        push(&mut heap, &done, normals, root);
        while let Some(Reverse((_, to, from))) = heap.pop() {
            let to = to as usize;
            if done[to] {
                continue;
            }
            let (a, b) = (normals[from as usize], normals[to]);
            let along = dot(a, b);
            let flip = if along.abs() >= CREASE {
                along < 0.0
            } else {
                let vote: f64 = neighbours[to]
                    .iter()
                    .map(|&j| dot(b, sub(positions[j as usize], positions[to])))
                    .sum();
                if vote.abs() > 1.0e-9 { vote > 0.0 } else { along < 0.0 }
            };
            if flip {
                normals[to] = normals[to].map(|v| -v);
            }
            done[to] = true;
            push(&mut heap, &done, normals, to);
        }
    }
}

pub fn estimate_normals(cloud: &Cloud, k: usize, orient: Orient) -> Result<Cloud> {
    if k < 3 {
        bail!("`k` must be at least 3 to fit a plane, got {k}");
    }
    if cloud.len() < 3 {
        bail!("normals need at least 3 points, the cloud has {}", cloud.len());
    }
    let k = k.min(cloud.len());
    let tree = KdTree::build(&cloud.positions);
    let want_lists = matches!(orient, Orient::Tree);
    let found = par_map(cloud.len(), |i| {
        let near = tree.nearest(cloud.positions[i], k);
        let points: Vec<P3> = near.iter().map(|(j, _)| cloud.positions[*j]).collect();
        let fit = fit_plane(&points);
        let normal = if fit.variation > SHARP {
            fit_sharp(&points, &fit, i as u64)
        } else {
            fit.normal
        };
        let list = if want_lists {
            near.iter()
                .map(|(j, _)| *j as u32)
                .filter(|&j| j as usize != i)
                .collect()
        } else {
            Vec::new()
        };
        (normal, list)
    });
    let (mut normals, lists): (Vec<P3>, Vec<Vec<u32>>) = found.into_iter().unzip();
    match orient {
        Orient::Toward(view) => {
            for (n, p) in normals.iter_mut().zip(&cloud.positions) {
                if dot(*n, sub(view, *p)) < 0.0 {
                    *n = n.map(|v| -v);
                }
            }
        }
        Orient::Origins => {
            if cloud.origins.is_empty() {
                bail!("the cloud holds no scan origins; they come from E57 poses, or give `toward=x,y,z`");
            }
            let origin_of = |scan: u16| {
                cloud
                    .origins
                    .iter()
                    .find(|(id, _)| *id == scan)
                    .or(cloud.origins.first())
                    .map(|(_, at)| *at)
                    .unwrap_or([0.0; 3])
            };
            for (i, n) in normals.iter_mut().enumerate() {
                if dot(*n, sub(origin_of(cloud.scans[i]), cloud.positions[i])) < 0.0 {
                    *n = n.map(|v| -v);
                }
            }
        }
        Orient::Tree => orient_by_tree(&cloud.positions, &mut normals, &lists),
    }
    let mut out = cloud.clone();
    out.normals = Some(normals.into_iter().map(|n| n.map(|v| v as f32)).collect());
    Ok(out)
}

pub fn multiply(a: M4, b: M4) -> M4 {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..4).map(|k| a[r][k] * b[k][c]).sum()))
}

pub fn translation(by: P3) -> M4 {
    let mut m = IDENTITY;
    (0..3).for_each(|k| m[k][3] = by[k]);
    m
}

pub fn scaling(factors: P3) -> M4 {
    let mut m = IDENTITY;
    (0..3).for_each(|k| m[k][k] = factors[k]);
    m
}

pub fn rotation(axis: P3, degrees: f64) -> M4 {
    let [x, y, z] = unit_or_zero(axis);
    let (s, c) = degrees.to_radians().sin_cos();
    let t = 1.0 - c;
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y, 0.0],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x, 0.0],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub fn about(centre: P3, m: M4) -> M4 {
    multiply(
        translation(centre),
        multiply(m, translation(centre.map(|v| -v))),
    )
}

fn cofactor(m: &M4) -> [[f64; 3]; 3] {
    let g = |r: usize, c: usize| m[r][c];
    [
        [
            g(1, 1) * g(2, 2) - g(1, 2) * g(2, 1),
            g(1, 2) * g(2, 0) - g(1, 0) * g(2, 2),
            g(1, 0) * g(2, 1) - g(1, 1) * g(2, 0),
        ],
        [
            g(0, 2) * g(2, 1) - g(0, 1) * g(2, 2),
            g(0, 0) * g(2, 2) - g(0, 2) * g(2, 0),
            g(0, 1) * g(2, 0) - g(0, 0) * g(2, 1),
        ],
        [
            g(0, 1) * g(1, 2) - g(0, 2) * g(1, 1),
            g(0, 2) * g(1, 0) - g(0, 0) * g(1, 2),
            g(0, 0) * g(1, 1) - g(0, 1) * g(1, 0),
        ],
    ]
}

pub fn transform(cloud: &Cloud, m: &M4) -> Result<Cloud> {
    let determinant = {
        let c = cofactor(m);
        m[0][0] * c[0][0] + m[0][1] * c[0][1] + m[0][2] * c[0][2]
    };
    if determinant.abs() < 1.0e-12 {
        bail!("the transform is singular, its determinant is {determinant}");
    }
    let apply = |p: P3| -> P3 {
        std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
    };
    let cof = cofactor(m);
    let mut out = cloud.clone();
    out.positions.iter_mut().for_each(|p| *p = apply(*p));
    out.origins.iter_mut().for_each(|(_, p)| *p = apply(*p));
    if let Some(normals) = &mut out.normals {
        for n in normals.iter_mut() {
            let v = [n[0] as f64, n[1] as f64, n[2] as f64];
            let moved: P3 = std::array::from_fn(|r| cof[r][0] * v[0] + cof[r][1] * v[1] + cof[r][2] * v[2]);
            *n = unit_or_zero(moved).map(|c| c as f32);
        }
    }
    Ok(out)
}

pub fn merge_scans(parts: &[&Cloud]) -> Cloud {
    let mut out = Cloud::default();
    let mut highest: Option<u32> = None;
    for part in parts {
        let ids = part.scan_ids();
        let (Some(&low), Some(&high)) = (ids.first(), ids.last()) else {
            continue;
        };
        let shift = match highest {
            Some(top) if low as u32 <= top => top + 1 - low as u32,
            _ => 0,
        };
        let mut shifted = (*part).clone();
        shifted
            .scans
            .iter_mut()
            .for_each(|s| *s = (*s as u32 + shift).min(u16::MAX as u32) as u16);
        shifted
            .origins
            .iter_mut()
            .for_each(|(id, _)| *id = (*id as u32 + shift).min(u16::MAX as u32) as u16);
        highest = Some(highest.unwrap_or(0).max(high as u32 + shift));
        out.merge(&shifted);
    }
    out
}

pub fn mean_spacing(cloud: &Cloud) -> Option<f64> {
    if cloud.len() < 2 {
        return None;
    }
    let tree = KdTree::build(&cloud.positions);
    let sampled = 20_000usize.min(cloud.len());
    let picks: Vec<usize> = (0..sampled)
        .map(|k| (k as u128 * cloud.len() as u128 / sampled as u128) as usize)
        .collect();
    let nearest = par_map(picks.len(), |k| {
        let i = picks[k];
        tree.nearest(cloud.positions[i], 2)
            .into_iter()
            .find(|(j, _)| *j != i)
            .map(|(_, d)| d)
    });
    let found: Vec<f64> = nearest.into_iter().flatten().collect();
    (!found.is_empty()).then(|| found.iter().sum::<f64>() / found.len() as f64)
}

pub fn add_outliers(cloud: &Cloud, count: usize, distance: f64, seed: u64) -> Result<Cloud> {
    if cloud.is_empty() {
        bail!("there are no points to displace outliers from");
    }
    let mut rng = Rng::new(seed);
    let centre = cloud
        .positions
        .iter()
        .fold([0.0; 3], |s, p| [s[0] + p[0], s[1] + p[1], s[2] + p[2]])
        .map(|v| v / cloud.len() as f64);
    let mut out = cloud.clone();
    for _ in 0..count {
        let source = rng.below(cloud.len());
        let p = cloud.positions[source];
        let mut direction = loop {
            let d = [
                rng.unit() * 2.0 - 1.0,
                rng.unit() * 2.0 - 1.0,
                rng.unit() * 2.0 - 1.0,
            ];
            let length = dot(d, d);
            if length > 0.01 && length <= 1.0 {
                break unit_or_zero(d);
            }
        };
        if dot(direction, sub(p, centre)) < 0.0 {
            direction = direction.map(|v| -v);
        }
        let mut point = Point::at(std::array::from_fn(|k| p[k] + direction[k] * distance));
        point.scan = cloud.scans[source];
        out.push(point);
    }
    Ok(out)
}

pub fn synthetic(count: usize, seed: u64, size: f64) -> Cloud {
    let mut rng = Rng::new(seed);
    let mut positions = Vec::with_capacity(count);
    for _ in 0..count {
        let noise = rng.unit() + rng.unit() + rng.unit() - 1.5;
        positions.push([rng.unit() * size, rng.unit() * size, noise * 0.02]);
    }
    Cloud::from_positions(positions)
}

#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub radius: f64,
    pub height: f64,
    pub noise: f64,
    pub spacing: f64,
}

fn jitter(rng: &mut Rng, noise: f64) -> f64 {
    (rng.unit() * 2.0 - 1.0) * noise
}

fn ball_point(rng: &mut Rng, radius: f64) -> P3 {
    loop {
        let d = [
            rng.unit() * 2.0 - 1.0,
            rng.unit() * 2.0 - 1.0,
            rng.unit() * 2.0 - 1.0,
        ];
        if dot(d, d) <= 1.0 {
            return d.map(|v| v * radius);
        }
    }
}

pub fn synthetic_shape(name: &str, count: usize, seed: u64, shape: Shape) -> Result<Cloud> {
    let mut rng = Rng::new(seed);
    let Shape {
        radius,
        height,
        noise,
        spacing,
    } = shape;
    if radius <= 0.0 || height <= 0.0 || noise < 0.0 || count == 0 {
        bail!("`{name}` needs a positive r, h and count, and a noise of 0 or more");
    }
    let tau = std::f64::consts::TAU;
    let mut positions = Vec::with_capacity(count);
    match name {
        "cylinder" | "two_cylinders" => {
            let copies = if name == "cylinder" { 1 } else { 2 };
            for c in 0..copies {
                for _ in 0..count / copies {
                    let a = rng.unit() * tau;
                    let r = radius + jitter(&mut rng, noise);
                    positions.push([
                        r * a.cos() + c as f64 * spacing,
                        r * a.sin(),
                        rng.unit() * height,
                    ]);
                }
            }
        }
        "sphere" => {
            for _ in 0..count {
                let z = rng.unit() * 2.0 - 1.0;
                let a = rng.unit() * tau;
                let ring = (1.0 - z * z).sqrt();
                let r = radius + jitter(&mut rng, noise);
                positions.push([r * ring * a.cos(), r * ring * a.sin(), r * z]);
            }
        }
        other => bail!("`cloud_synthetic` shapes are cylinder, two_cylinders and sphere, not `{other}`"),
    }
    Ok(Cloud::from_positions(positions))
}

pub fn synthetic_blobs(blobs: usize, per_blob: usize, seed: u64, radius: f64, spacing: f64) -> Cloud {
    let mut rng = Rng::new(seed);
    let mut positions = Vec::with_capacity(blobs * per_blob);
    for b in 0..blobs {
        for _ in 0..per_blob {
            let p = ball_point(&mut rng, radius);
            positions.push([p[0] + b as f64 * spacing, p[1], p[2]]);
        }
    }
    Cloud::from_positions(positions)
}
