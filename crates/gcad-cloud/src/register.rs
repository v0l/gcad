use crate::cloud::{Cloud, P3};
use crate::fit::solve;
use crate::kdtree::KdTree;
use crate::ops::{
    IDENTITY, M4, Orient, Rng, cross, dot, estimate_normals, multiply, par_map, rotation, sub, unit_or_zero,
    voxel_downsample,
};
use anyhow::{Result, bail};

const ITERATION_LIMIT: usize = 100_000;
const WORKING_POINTS: usize = 40_000;
const STOP_ANGLE: f64 = 1.0e-10;
const STOP_SHIFT: f64 = 1.0e-10;
const BINS: usize = 11;
const FEATURE: usize = BINS * 3;
const KEYPOINTS: usize = 3000;
const SCORING_POINTS: usize = 400;
const HYPOTHESES: usize = 20_000;
const CANDIDATES: usize = 6;
const EDGE_RATIO: f64 = 0.9;

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub plane: bool,
    pub max_dist: f64,
    pub iters: usize,
    pub trim: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Registration {
    pub matrix: M4,
    pub rms: f64,
    pub iterations: usize,
    pub pairs: usize,
}

pub fn apply(m: &M4, p: P3) -> P3 {
    std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}

fn apply_direction(m: &M4, n: P3) -> P3 {
    std::array::from_fn(|r| m[r][0] * n[0] + m[r][1] * n[1] + m[r][2] * n[2])
}

fn jacobi(mut a: [[f64; 4]; 4]) -> ([f64; 4], [[f64; 4]; 4]) {
    let mut v = [[0.0; 4]; 4];
    (0..4).for_each(|i| v[i][i] = 1.0);
    for _ in 0..60 {
        let off: f64 = (0..4)
            .flat_map(|i| (i + 1..4).map(move |j| (i, j)))
            .map(|(i, j)| a[i][j] * a[i][j])
            .sum();
        if off < 1.0e-300 {
            break;
        }
        for p in 0..3 {
            for q in p + 1..4 {
                if a[p][q].abs() < 1.0e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..4 {
                    let (akp, akq) = (a[k][p], a[k][q]);
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for k in 0..4 {
                    let (apk, aqk) = (a[p][k], a[q][k]);
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for k in 0..4 {
                    let (vkp, vkq) = (v[k][p], v[k][q]);
                    v[k][p] = c * vkp - s * vkq;
                    v[k][q] = s * vkp + c * vkq;
                }
            }
        }
    }
    ([a[0][0], a[1][1], a[2][2], a[3][3]], v)
}

fn centroid(points: &[P3]) -> P3 {
    let n = points.len().max(1) as f64;
    let sum = points
        .iter()
        .fold([0.0; 3], |s, p| [s[0] + p[0], s[1] + p[1], s[2] + p[2]]);
    sum.map(|v| v / n)
}

pub fn fit_rigid(source: &[P3], target: &[P3]) -> Result<M4> {
    if source.len() != target.len() || source.len() < 3 {
        bail!("a rigid fit needs at least 3 point pairs, got {}", source.len().min(target.len()));
    }
    let (cs, ct) = (centroid(source), centroid(target));
    let mut s = [[0.0; 3]; 3];
    for (a, b) in source.iter().zip(target) {
        let (a, b) = (sub(*a, cs), sub(*b, ct));
        for i in 0..3 {
            for j in 0..3 {
                s[i][j] += a[i] * b[j];
            }
        }
    }
    let n = [
        [
            s[0][0] + s[1][1] + s[2][2],
            s[1][2] - s[2][1],
            s[2][0] - s[0][2],
            s[0][1] - s[1][0],
        ],
        [
            s[1][2] - s[2][1],
            s[0][0] - s[1][1] - s[2][2],
            s[0][1] + s[1][0],
            s[2][0] + s[0][2],
        ],
        [
            s[2][0] - s[0][2],
            s[0][1] + s[1][0],
            -s[0][0] + s[1][1] - s[2][2],
            s[1][2] + s[2][1],
        ],
        [
            s[0][1] - s[1][0],
            s[2][0] + s[0][2],
            s[1][2] + s[2][1],
            -s[0][0] - s[1][1] + s[2][2],
        ],
    ];
    let (values, vectors) = jacobi(n);
    let best = (0..4)
        .max_by(|&a, &b| values[a].total_cmp(&values[b]))
        .unwrap_or(0);
    let [w, x, y, z] = [0, 1, 2, 3].map(|k| vectors[k][best]);
    let norm = (w * w + x * x + y * y + z * z).sqrt();
    if norm < 1.0e-300 {
        bail!("the point pairs do not fix a rotation");
    }
    let [w, x, y, z] = [w / norm, x / norm, y / norm, z / norm];
    let r = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
    ];
    let mut m = IDENTITY;
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = r[i][j];
        }
        m[i][3] = ct[i] - (0..3).map(|j| r[i][j] * cs[j]).sum::<f64>();
    }
    Ok(m)
}

pub fn pair_rms(matrix: &M4, source: &[P3], target: &[P3]) -> f64 {
    let sum: f64 = source
        .iter()
        .zip(target)
        .map(|(s, t)| {
            let d = sub(apply(matrix, *s), *t);
            dot(d, d)
        })
        .sum();
    (sum / source.len().max(1) as f64).sqrt()
}

pub fn rotation_angle(m: &M4) -> f64 {
    let trace = m[0][0] + m[1][1] + m[2][2];
    ((trace - 1.0) / 2.0).clamp(-1.0, 1.0).acos().to_degrees()
}

fn working_set(cloud: &Cloud) -> Vec<usize> {
    let n = cloud.len();
    let step = n.div_ceil(WORKING_POINTS).max(1);
    (0..n).step_by(step).collect()
}

struct Matches {
    source: Vec<P3>,
    target: Vec<P3>,
    normals: Vec<P3>,
    residual: Vec<f64>,
}

fn match_points(
    moved: &[P3],
    tree: &KdTree,
    target: &Cloud,
    settings: &Settings,
) -> Matches {
    let found = par_map(moved.len(), |i| {
        tree.nearest(moved[i], 1)
            .first()
            .map(|&(j, d)| (i, j, d))
            .filter(|&(_, _, d)| d <= settings.max_dist)
    });
    let mut pairs: Vec<(usize, usize, f64)> = found.into_iter().flatten().collect();
    pairs.sort_by(|a, b| a.2.total_cmp(&b.2));
    let keep = ((pairs.len() as f64 * settings.trim).ceil() as usize).min(pairs.len());
    pairs.truncate(keep);
    let normals_of = target.normals.as_ref();
    let mut out = Matches {
        source: Vec::with_capacity(keep),
        target: Vec::with_capacity(keep),
        normals: Vec::new(),
        residual: Vec::with_capacity(keep),
    };
    for (i, j, d) in pairs {
        out.source.push(moved[i]);
        out.target.push(target.positions[j]);
        match normals_of {
            Some(normals) if settings.plane => {
                let n = normals[j].map(|v| v as f64);
                out.normals.push(n);
                out.residual.push(dot(n, sub(moved[i], target.positions[j])));
            }
            _ => out.residual.push(d),
        }
    }
    out
}

fn plane_step(m: &Matches) -> Option<M4> {
    let mut ata = vec![vec![0.0; 6]; 6];
    let mut atb = vec![0.0; 6];
    for ((p, q), n) in m.source.iter().zip(&m.target).zip(&m.normals) {
        let c = cross(*p, *n);
        let row = [c[0], c[1], c[2], n[0], n[1], n[2]];
        let b = dot(*n, sub(*q, *p));
        for i in 0..6 {
            for j in 0..6 {
                ata[i][j] += row[i] * row[j];
            }
            atb[i] += row[i] * b;
        }
    }
    let ridge = (0..6).map(|i| ata[i][i]).sum::<f64>() * 1.0e-12;
    (0..6).for_each(|i| ata[i][i] += ridge);
    let x = solve(ata, atb)?;
    let w = [x[0], x[1], x[2]];
    let angle = dot(w, w).sqrt();
    let mut step = rotation(w, angle.to_degrees());
    (0..3).for_each(|k| step[k][3] = x[3 + k]);
    Some(step)
}

fn point_step(m: &Matches) -> Option<M4> {
    fit_rigid(&m.source, &m.target).ok()
}

fn small(step: &M4) -> bool {
    rotation_angle(step).to_radians() < STOP_ANGLE
        && (0..3).all(|k| step[k][3].abs() < STOP_SHIFT)
}

fn rms(values: &[f64]) -> f64 {
    (values.iter().map(|v| v * v).sum::<f64>() / values.len().max(1) as f64).sqrt()
}

pub fn icp(source: &Cloud, target: &Cloud, start: M4, settings: &Settings) -> Result<Registration> {
    if source.is_empty() || target.is_empty() {
        bail!("ICP needs points in both clouds");
    }
    if settings.plane && target.normals.is_none() {
        bail!("point-to-plane ICP needs normals on the target; run `normals` on it first");
    }
    if !(settings.max_dist > 0.0) {
        bail!("`max_dist` must be positive, got {}", settings.max_dist);
    }
    if !(settings.trim > 0.0 && settings.trim <= 1.0) {
        bail!("`trim` is the share of pairs kept, above 0 and up to 1, got {}", settings.trim);
    }
    let picks = working_set(source);
    let base: Vec<P3> = picks.iter().map(|&i| source.positions[i]).collect();
    let tree = KdTree::build(&target.positions);
    let mut total = start;
    let mut iterations = 0;
    for _ in 0..settings.iters.min(ITERATION_LIMIT) {
        let moved: Vec<P3> = base.iter().map(|p| apply(&total, *p)).collect();
        let matches = match_points(&moved, &tree, target, settings);
        if matches.source.len() < 3 {
            bail!(
                "ICP found fewer than 3 pairs within max_dist={}; raise `max_dist` or start the clouds closer",
                settings.max_dist
            );
        }
        let step = if settings.plane {
            plane_step(&matches)
        } else {
            point_step(&matches)
        };
        let Some(step) = step else {
            bail!("ICP could not fix a pose: the overlap is a plane, a line or a point");
        };
        total = multiply(step, total);
        iterations += 1;
        if small(&step) {
            break;
        }
    }
    let moved: Vec<P3> = base.iter().map(|p| apply(&total, *p)).collect();
    let matches = match_points(&moved, &tree, target, settings);
    Ok(Registration {
        matrix: total,
        rms: rms(&matches.residual),
        iterations,
        pairs: matches.source.len(),
    })
}

fn flipped_toward(n: P3, d: P3) -> P3 {
    if dot(n, d) < 0.0 { n.map(|v| -v) } else { n }
}

fn pair_feature(ps: P3, ns: P3, pt: P3, nt: P3) -> Option<[usize; 3]> {
    let d = sub(pt, ps);
    let dist = dot(d, d).sqrt();
    if dist < 1.0e-12 {
        return None;
    }
    let d = d.map(|v| v / dist);
    let ns = flipped_toward(ns, d);
    let nt = flipped_toward(nt, d);
    let (u, other, d) = if dot(ns, d) >= dot(nt, d) {
        (ns, nt, d)
    } else {
        (nt, ns, d.map(|v| -v))
    };
    let v = unit_or_zero(cross(u, d));
    let w = cross(u, v);
    let alpha = dot(v, other);
    let phi = dot(u, d);
    let theta = dot(w, other).atan2(dot(u, other));
    let bin = |x: f64, lo: f64, hi: f64| {
        (((x - lo) / (hi - lo) * BINS as f64).floor() as isize).clamp(0, BINS as isize - 1) as usize
    };
    Some([
        bin(alpha, -1.0, 1.0),
        bin(phi, -1.0, 1.0),
        bin(theta, -std::f64::consts::PI, std::f64::consts::PI),
    ])
}

fn fpfh(cloud: &Cloud, normals: &[P3], radius: f64) -> Vec<[f32; FEATURE]> {
    let tree = KdTree::build(&cloud.positions);
    let neighbours: Vec<Vec<usize>> = par_map(cloud.len(), |i| {
        tree.within(cloud.positions[i], radius)
            .into_iter()
            .filter(|&j| j != i)
            .collect()
    });
    let spfh: Vec<[f32; FEATURE]> = par_map(cloud.len(), |i| {
        let mut h = [0.0f32; FEATURE];
        let mut count = 0.0f32;
        for &j in &neighbours[i] {
            if let Some(bins) = pair_feature(cloud.positions[i], normals[i], cloud.positions[j], normals[j]) {
                (0..3).for_each(|k| h[k * BINS + bins[k]] += 1.0);
                count += 1.0;
            }
        }
        if count > 0.0 {
            h.iter_mut().for_each(|v| *v /= count);
        }
        h
    });
    par_map(cloud.len(), |i| {
        let mut h = spfh[i];
        let list = &neighbours[i];
        if !list.is_empty() {
            let mut extra = [0.0f32; FEATURE];
            let mut weights = 0.0f32;
            for &j in list {
                let d = sub(cloud.positions[i], cloud.positions[j]);
                let w = 1.0 / (dot(d, d).sqrt() as f32).max(1.0e-9);
                (0..FEATURE).for_each(|k| extra[k] += w * spfh[j][k]);
                weights += w;
            }
            (0..FEATURE).for_each(|k| h[k] += extra[k] / weights);
        }
        let total: f32 = h.iter().sum();
        if total > 0.0 {
            h.iter_mut().for_each(|v| *v /= total);
        }
        h
    })
}

fn feature_distance(a: &[f32; FEATURE], b: &[f32; FEATURE]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

struct Prepared {
    cloud: Cloud,
    normals: Vec<P3>,
    features: Vec<[f32; FEATURE]>,
    keys: Vec<usize>,
}

fn prepare(cloud: &Cloud, voxel: f64) -> Result<Prepared> {
    let mut down = voxel_downsample(cloud, voxel)?;
    down.normals = None;
    if down.len() < 30 {
        bail!("`voxel={voxel}` leaves {} points, too few for features; use a smaller voxel", down.len());
    }
    let with = estimate_normals(&down, 20, Orient::Tree)?;
    let normals: Vec<P3> = with
        .normals
        .as_ref()
        .map(|n| n.iter().map(|v| v.map(|c| c as f64)).collect())
        .unwrap_or_default();
    let features = fpfh(&with, &normals, voxel * 5.0);
    let step = with.len().div_ceil(KEYPOINTS).max(1);
    let keys = (0..with.len()).step_by(step).collect();
    Ok(Prepared {
        cloud: with,
        normals,
        features,
        keys,
    })
}

pub fn coarse(source: &Cloud, target: &Cloud, voxel: f64, seed: u64) -> Result<M4> {
    if !(voxel > 0.0) {
        bail!("`voxel` must be positive, got {voxel}");
    }
    let s = prepare(source, voxel)?;
    let t = prepare(target, voxel)?;
    let matched: Vec<usize> = par_map(s.keys.len(), |a| {
        let f = &s.features[s.keys[a]];
        t.keys
            .iter()
            .copied()
            .min_by(|&x, &y| {
                feature_distance(f, &t.features[x]).total_cmp(&feature_distance(f, &t.features[y]))
            })
            .unwrap_or(0)
    });
    let tree = KdTree::build(&t.cloud.positions);
    let tolerance = voxel * 1.5;
    let scoring: Vec<P3> = {
        let step = s.cloud.len().div_ceil(SCORING_POINTS).max(1);
        (0..s.cloud.len()).step_by(step).map(|i| s.cloud.positions[i]).collect()
    };
    let score = |m: &M4| -> usize {
        scoring
            .iter()
            .filter(|p| {
                tree.nearest(apply(m, **p), 1)
                    .first()
                    .is_some_and(|&(_, d)| d <= tolerance)
            })
            .count()
    };
    let mut rng = Rng::new(seed);
    let mut best: Vec<(usize, M4)> = Vec::new();
    for _ in 0..HYPOTHESES {
        let picks = [rng.below(s.keys.len()), rng.below(s.keys.len()), rng.below(s.keys.len())];
        if picks[0] == picks[1] || picks[1] == picks[2] || picks[0] == picks[2] {
            continue;
        }
        let a: Vec<P3> = picks.iter().map(|&k| s.cloud.positions[s.keys[k]]).collect();
        let b: Vec<P3> = picks.iter().map(|&k| t.cloud.positions[matched[k]]).collect();
        let consistent = (0..3).all(|i| {
            let j = (i + 1) % 3;
            let (da, db) = (
                dot(sub(a[i], a[j]), sub(a[i], a[j])).sqrt(),
                dot(sub(b[i], b[j]), sub(b[i], b[j])).sqrt(),
            );
            da > voxel && db > voxel && da.min(db) >= EDGE_RATIO * da.max(db)
        });
        if !consistent {
            continue;
        }
        let Ok(m) = fit_rigid(&a, &b) else { continue };
        let value = score(&m);
        if best.len() < CANDIDATES || value > best[best.len() - 1].0 {
            best.push((value, m));
            best.sort_by(|x, y| y.0.cmp(&x.0));
            best.truncate(CANDIDATES);
        }
    }
    if best.is_empty() {
        bail!("coarse alignment found no consistent feature matches; try another `voxel`");
    }
    let settings = Settings {
        plane: true,
        max_dist: voxel * 2.0,
        iters: 30,
        trim: 0.9,
    };
    let refine = Cloud {
        normals: t.cloud.normals.clone(),
        ..t.cloud.clone()
    };
    let _ = &t.normals;
    let mut winner: Option<(f64, M4)> = None;
    for (_, m) in best {
        let Ok(r) = icp(&s.cloud, &refine, m, &settings) else {
            continue;
        };
        let fraction = r.pairs as f64 / s.cloud.len() as f64;
        let value = r.rms.max(1.0e-12) / fraction.max(1.0e-9).powi(2);
        if winner.as_ref().is_none_or(|(v, _)| value < *v) {
            winner = Some((value, r.matrix));
        }
    }
    winner
        .map(|(_, m)| m)
        .ok_or_else(|| anyhow::anyhow!("coarse alignment could not refine any candidate; try another `voxel`"))
}

pub fn with_normals(cloud: &Cloud) -> Result<Cloud> {
    if cloud.normals.is_some() {
        Ok(cloud.clone())
    } else {
        estimate_normals(cloud, 20, Orient::Tree)
    }
}

pub fn add_noise(cloud: &Cloud, sigma: f64, seed: u64) -> Cloud {
    let mut rng = Rng::new(seed);
    let mut out = cloud.clone();
    for (i, p) in out.positions.iter_mut().enumerate() {
        let gauss = (0..12).map(|_| rng.unit()).sum::<f64>() - 6.0;
        let n = cloud.normals.as_ref().map_or([0.0, 0.0, 1.0], |n| n[i].map(|v| v as f64));
        (0..3).for_each(|k| p[k] += n[k] * gauss * sigma);
    }
    out
}

pub fn sample_triangles(triangles: &[[P3; 3]], count: usize, seed: u64) -> Cloud {
    let mut running = Vec::with_capacity(triangles.len());
    let mut total = 0.0;
    for [a, b, c] in triangles {
        let n = cross(sub(*b, *a), sub(*c, *a));
        total += dot(n, n).sqrt() / 2.0;
        running.push(total);
    }
    let mut cloud = Cloud::with_layout(
        crate::cloud::Layout {
            normals: true,
            ..Default::default()
        },
        count,
    );
    let mut rng = Rng::new(seed);
    for _ in 0..count {
        let pick = rng.unit() * total;
        let f = running.partition_point(|&v| v < pick).min(triangles.len() - 1);
        let [a, b, c] = triangles[f];
        let (mut u, mut v) = (rng.unit(), rng.unit());
        if u + v > 1.0 {
            (u, v) = (1.0 - u, 1.0 - v);
        }
        let position = std::array::from_fn(|k| a[k] + u * (b[k] - a[k]) + v * (c[k] - a[k]));
        let n = unit_or_zero(cross(sub(b, a), sub(c, a)));
        let mut point = crate::cloud::Point::at(position);
        point.normal = Some(n.map(|v| v as f32));
        cloud.push(point);
    }
    cloud
}

pub fn matrix_line(name: &str, m: &M4) -> String {
    let numbers: Vec<String> = m.iter().flatten().map(|v| {
        let v = if v.abs() < 1.0e-15 { 0.0 } else { *v };
        format!("{v}")
    }).collect();
    format!("transform {name} {}", numbers.join(","))
}

pub fn moved_normals(m: &M4, n: [f32; 3]) -> [f32; 3] {
    apply_direction(m, n.map(|v| v as f64)).map(|v| v as f32)
}
