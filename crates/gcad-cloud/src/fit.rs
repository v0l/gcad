use crate::cloud::{Cloud, P3};
use crate::ops::{Rng, cross, dot, fit_plane, par_map, sub, unit_or_zero};

const ALIGN: f64 = 0.85;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Plane,
    Sphere,
    Cylinder,
    Cone,
}

impl Kind {
    pub fn parse(text: &str) -> Option<Kind> {
        match text {
            "plane" => Some(Kind::Plane),
            "sphere" => Some(Kind::Sphere),
            "cylinder" => Some(Kind::Cylinder),
            "cone" => Some(Kind::Cone),
            _ => None,
        }
    }

    pub fn needs_normals(self) -> bool {
        self != Kind::Plane
    }

    fn sample(self) -> usize {
        match self {
            Kind::Plane | Kind::Cone => 3,
            _ => 2,
        }
    }

    fn rounds(self) -> usize {
        match self {
            Kind::Plane => 500,
            _ => 1000,
        }
    }

    fn widening(self) -> [f64; 4] {
        match self {
            Kind::Plane => [1.0; 4],
            _ => [3.0, 2.0, 1.0, 1.0],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Primitive {
    Plane { origin: P3, normal: P3 },
    Sphere { centre: P3, radius: f64 },
    Cylinder { origin: P3, axis: P3, radius: f64 },
    Cone { apex: P3, axis: P3, half_angle: f64 },
}

fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: P3, s: f64) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}

impl Primitive {
    pub fn distance(&self, p: P3) -> f64 {
        match *self {
            Primitive::Plane { origin, normal } => dot(normal, sub(p, origin)),
            Primitive::Sphere { centre, radius } => norm(sub(p, centre)) - radius,
            Primitive::Cylinder {
                origin,
                axis,
                radius,
            } => {
                let a = unit_or_zero(axis);
                let v = sub(p, origin);
                norm(sub(v, scale(a, dot(v, a)))) - radius
            }
            Primitive::Cone {
                apex,
                axis,
                half_angle,
            } => {
                let a = unit_or_zero(axis);
                let v = sub(p, apex);
                let along = dot(v, a);
                if along < 0.0 {
                    return norm(v);
                }
                let perp = norm(sub(v, scale(a, along)));
                let theta = half_angle.to_radians();
                perp * theta.cos() - along * theta.sin()
            }
        }
    }

    pub fn normal_at(&self, p: P3) -> P3 {
        match *self {
            Primitive::Plane { normal, .. } => normal,
            Primitive::Sphere { centre, .. } => unit_or_zero(sub(p, centre)),
            Primitive::Cylinder { origin, axis, .. } => {
                let a = unit_or_zero(axis);
                let v = sub(p, origin);
                unit_or_zero(sub(v, scale(a, dot(v, a))))
            }
            Primitive::Cone {
                apex,
                axis,
                half_angle,
            } => {
                let a = unit_or_zero(axis);
                let v = sub(p, apex);
                let out = unit_or_zero(sub(v, scale(a, dot(v, a))));
                let theta = half_angle.to_radians();
                sub(scale(out, theta.cos()), scale(a, theta.sin()))
            }
        }
    }

    fn shifted(&self, by: P3) -> Primitive {
        match *self {
            Primitive::Plane { origin, normal } => Primitive::Plane {
                origin: add(origin, by),
                normal,
            },
            Primitive::Sphere { centre, radius } => Primitive::Sphere {
                centre: add(centre, by),
                radius,
            },
            Primitive::Cylinder {
                origin,
                axis,
                radius,
            } => Primitive::Cylinder {
                origin: add(origin, by),
                axis,
                radius,
            },
            Primitive::Cone {
                apex,
                axis,
                half_angle,
            } => Primitive::Cone {
                apex: add(apex, by),
                axis,
                half_angle,
            },
        }
    }

    fn params(&self) -> Vec<f64> {
        match *self {
            Primitive::Plane { .. } => Vec::new(),
            Primitive::Sphere { centre, radius } => vec![centre[0], centre[1], centre[2], radius],
            Primitive::Cylinder {
                origin,
                axis,
                radius,
            } => vec![
                origin[0], origin[1], origin[2], axis[0], axis[1], axis[2], radius,
            ],
            Primitive::Cone {
                apex,
                axis,
                half_angle,
            } => vec![
                apex[0],
                apex[1],
                apex[2],
                axis[0],
                axis[1],
                axis[2],
                half_angle.to_radians(),
            ],
        }
    }

    fn with_params(&self, x: &[f64]) -> Primitive {
        match self {
            Primitive::Plane { .. } => *self,
            Primitive::Sphere { .. } => Primitive::Sphere {
                centre: [x[0], x[1], x[2]],
                radius: x[3],
            },
            Primitive::Cylinder { .. } => Primitive::Cylinder {
                origin: [x[0], x[1], x[2]],
                axis: [x[3], x[4], x[5]],
                radius: x[6],
            },
            Primitive::Cone { .. } => Primitive::Cone {
                apex: [x[0], x[1], x[2]],
                axis: [x[3], x[4], x[5]],
                half_angle: x[6].to_degrees(),
            },
        }
    }

    fn normalised(self) -> Primitive {
        match self {
            Primitive::Cylinder {
                origin,
                axis,
                radius,
            } => Primitive::Cylinder {
                origin,
                axis: unit_or_zero(axis),
                radius,
            },
            Primitive::Cone {
                apex,
                axis,
                half_angle,
            } => Primitive::Cone {
                apex,
                axis: unit_or_zero(axis),
                half_angle,
            },
            other => other,
        }
    }
}

pub(crate) fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() < 1.0e-300 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..n {
            let factor = a[row][col] / a[col][col];
            for k in col..n {
                a[row][k] -= factor * a[col][k];
            }
            b[row] -= factor * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let tail: f64 = (row + 1..n).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - tail) / a[row][row];
    }
    Some(x)
}

fn cost(start: &Primitive, x: &[f64], points: &[P3]) -> f64 {
    let p = start.with_params(x);
    points.iter().map(|q| p.distance(*q).powi(2)).sum()
}

fn levenberg(start: Primitive, points: &[P3]) -> Primitive {
    let mut x = start.params();
    if x.is_empty() {
        return start;
    }
    let n = x.len();
    let mut lambda = 1.0e-3;
    let mut current = cost(&start, &x, points);
    for _ in 0..40 {
        let base = start.with_params(&x);
        let residual: Vec<f64> = points.iter().map(|q| base.distance(*q)).collect();
        let columns: Vec<Vec<f64>> = (0..n)
            .map(|k| {
                let h = 1.0e-6 * (1.0 + x[k].abs());
                let mut y = x.clone();
                y[k] += h;
                let moved = start.with_params(&y);
                points
                    .iter()
                    .zip(&residual)
                    .map(|(q, r)| (moved.distance(*q) - r) / h)
                    .collect()
            })
            .collect();
        let mut jtj = vec![vec![0.0; n]; n];
        let mut jtr = vec![0.0; n];
        for a in 0..n {
            for b in 0..n {
                jtj[a][b] = columns[a].iter().zip(&columns[b]).map(|(u, v)| u * v).sum();
            }
            jtr[a] = columns[a].iter().zip(&residual).map(|(u, r)| u * r).sum();
        }
        let mut improved = false;
        for _ in 0..10 {
            let mut damped = jtj.clone();
            for k in 0..n {
                damped[k][k] += lambda * (jtj[k][k] + 1.0e-9);
            }
            let Some(step) = solve(damped, jtr.iter().map(|v| -v).collect()) else {
                lambda *= 10.0;
                continue;
            };
            let trial: Vec<f64> = x.iter().zip(&step).map(|(a, b)| a + b).collect();
            let c = cost(&start, &trial, points);
            if c.is_finite() && c < current {
                let gain = current - c;
                x = trial;
                lambda = (lambda * 0.3).max(1.0e-12);
                improved = gain > 1.0e-14 * current.max(1.0e-30);
                current = c;
                break;
            }
            lambda *= 10.0;
        }
        if !improved {
            break;
        }
    }
    start.with_params(&x).normalised()
}

fn centroid(points: &[P3]) -> P3 {
    let sum = points.iter().fold([0.0; 3], |s, p| add(s, *p));
    scale(sum, 1.0 / points.len().max(1) as f64)
}

fn refine(start: Primitive, points: &[P3]) -> Primitive {
    if let Primitive::Plane { normal, .. } = start {
        let fit = fit_plane(points);
        let flipped = if dot(fit.normal, normal) < 0.0 {
            scale(fit.normal, -1.0)
        } else {
            fit.normal
        };
        return Primitive::Plane {
            origin: fit.centroid,
            normal: flipped,
        };
    }
    let centre = centroid(points);
    let local: Vec<P3> = points.iter().map(|p| sub(*p, centre)).collect();
    let moved = levenberg(start.shifted(scale(centre, -1.0)), &local).shifted(centre);
    match moved {
        Primitive::Cylinder {
            origin,
            axis,
            radius,
        } => {
            let along = dot(sub(centre, origin), axis);
            Primitive::Cylinder {
                origin: add(origin, scale(axis, along)),
                axis,
                radius,
            }
        }
        other => other,
    }
}

fn closest_approach(p1: P3, n1: P3, p2: P3, n2: P3) -> Option<P3> {
    let w = sub(p1, p2);
    let b = dot(n1, n2);
    let (d, e) = (dot(n1, w), dot(n2, w));
    let denominator = 1.0 - b * b;
    if denominator < 1.0e-3 {
        return None;
    }
    let t = (b * e - d) / denominator;
    let s = (e - b * d) / denominator;
    Some(scale(
        add(add(p1, scale(n1, t)), add(p2, scale(n2, s))),
        0.5,
    ))
}

fn solve3(rows: [P3; 3], rhs: P3) -> Option<P3> {
    let a: Vec<Vec<f64>> = rows.iter().map(|r| r.to_vec()).collect();
    solve(a, rhs.to_vec()).map(|x| [x[0], x[1], x[2]])
}

fn normal_of(cloud: &Cloud, i: usize) -> P3 {
    cloud
        .normals
        .as_ref()
        .map_or([0.0; 3], |n| n[i].map(|v| v as f64))
}

fn hypothesis(kind: Kind, cloud: &Cloud, pick: &[usize]) -> Option<Primitive> {
    let p: Vec<P3> = pick.iter().map(|&i| cloud.positions[i]).collect();
    let n: Vec<P3> = pick.iter().map(|&i| normal_of(cloud, i)).collect();
    match kind {
        Kind::Plane => {
            let normal = unit_or_zero(cross(sub(p[1], p[0]), sub(p[2], p[0])));
            (dot(normal, normal) > 0.5).then_some(Primitive::Plane {
                origin: p[0],
                normal,
            })
        }
        Kind::Sphere => {
            let centre = closest_approach(p[0], n[0], p[1], n[1])?;
            let radius = (norm(sub(p[0], centre)) + norm(sub(p[1], centre))) / 2.0;
            (radius > 1.0e-9).then_some(Primitive::Sphere { centre, radius })
        }
        Kind::Cylinder => {
            let axis = unit_or_zero(cross(n[0], n[1]));
            if norm(cross(n[0], n[1])) < 0.2 {
                return None;
            }
            let flat = |v: P3| unit_or_zero(sub(v, scale(axis, dot(v, axis))));
            let centre = closest_approach(p[0], flat(n[0]), p[1], flat(n[1]))?;
            let across = |q: P3| {
                let v = sub(q, centre);
                norm(sub(v, scale(axis, dot(v, axis))))
            };
            let radius = (across(p[0]) + across(p[1])) / 2.0;
            (radius > 1.0e-9).then_some(Primitive::Cylinder {
                origin: centre,
                axis,
                radius,
            })
        }
        Kind::Cone => {
            let rows = [n[0], n[1], n[2]];
            let rhs = [dot(n[0], p[0]), dot(n[1], p[1]), dot(n[2], p[2])];
            let apex = solve3(rows, rhs)?;
            let mut axis = unit_or_zero(cross(sub(n[0], n[1]), sub(n[0], n[2])));
            if dot(axis, axis) < 0.5 {
                return None;
            }
            let along: f64 = p.iter().map(|q| dot(sub(*q, apex), axis)).sum();
            if along < 0.0 {
                axis = scale(axis, -1.0);
            }
            let angles: Vec<f64> = p
                .iter()
                .map(|q| {
                    let v = sub(*q, apex);
                    (dot(v, axis) / norm(v).max(1.0e-12)).clamp(-1.0, 1.0).acos()
                })
                .collect();
            let half_angle = (angles.iter().sum::<f64>() / 3.0).to_degrees();
            (2.0..88.0)
                .contains(&half_angle)
                .then_some(Primitive::Cone {
                    apex,
                    axis,
                    half_angle,
                })
        }
    }
}

fn is_inlier(primitive: &Primitive, cloud: &Cloud, i: usize, tol: f64) -> bool {
    let p = cloud.positions[i];
    if primitive.distance(p).abs() > tol {
        return false;
    }
    let n = normal_of(cloud, i);
    dot(n, n) < 0.5 || dot(n, primitive.normal_at(p)).abs() >= ALIGN
}

fn within(primitive: &Primitive, cloud: &Cloud, among: &[usize], tol: f64) -> Vec<usize> {
    let keep = par_map(among.len(), |k| is_inlier(primitive, cloud, among[k], tol));
    among
        .iter()
        .zip(keep)
        .filter_map(|(&i, k)| k.then_some(i))
        .collect()
}

#[derive(Clone, Debug)]
pub struct Found {
    pub primitive: Primitive,
    pub inliers: Vec<usize>,
    pub rms: f64,
}

pub fn fit_one(
    cloud: &Cloud,
    kind: Kind,
    tol: f64,
    among: &[usize],
    seed: u64,
) -> Option<Found> {
    if among.len() < 6 {
        return None;
    }
    let mut rng = Rng::new(seed);
    let stride = (among.len() / 4000).max(1);
    let probe: Vec<usize> = among.iter().step_by(stride).copied().collect();
    let mut best: Option<(Primitive, usize)> = None;
    for _ in 0..kind.rounds() {
        let pick: Vec<usize> = (0..kind.sample())
            .map(|_| among[rng.below(among.len())])
            .collect();
        let Some(candidate) = hypothesis(kind, cloud, &pick) else {
            continue;
        };
        let score = probe
            .iter()
            .filter(|&&i| is_inlier(&candidate, cloud, i, tol))
            .count();
        if best.is_none_or(|(_, s)| score > s) {
            best = Some((candidate, score));
        }
    }
    let (mut primitive, _) = best?;
    for factor in kind.widening() {
        let inliers = within(&primitive, cloud, among, tol * factor);
        if inliers.len() < 6 {
            return None;
        }
        let points: Vec<P3> = inliers.iter().map(|&i| cloud.positions[i]).collect();
        primitive = refine(primitive, &points);
    }
    let inliers = within(&primitive, cloud, among, tol);
    if inliers.is_empty() {
        return None;
    }
    let rms = (inliers
        .iter()
        .map(|&i| primitive.distance(cloud.positions[i]).powi(2))
        .sum::<f64>()
        / inliers.len() as f64)
        .sqrt();
    Some(Found {
        primitive,
        inliers,
        rms,
    })
}

pub fn fit_all(
    cloud: &Cloud,
    kind: Kind,
    tol: f64,
    max: usize,
    min: usize,
    seed: u64,
) -> Vec<Found> {
    let mut left: Vec<usize> = (0..cloud.len()).collect();
    let mut found = Vec::new();
    while found.len() < max {
        let Some(next) = fit_one(cloud, kind, tol, &left, seed + found.len() as u64) else {
            break;
        };
        if next.inliers.len() < min {
            break;
        }
        let mut taken = vec![false; cloud.len()];
        next.inliers.iter().for_each(|&i| taken[i] = true);
        left.retain(|&i| !taken[i]);
        found.push(next);
    }
    found
}
