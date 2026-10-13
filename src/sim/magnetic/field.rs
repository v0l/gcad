use super::plane::{Plane, Shape, Stuff, Toward};
use super::steel::MU0;
use crate::sim::fem::scalar::{Csr, direct};
use anyhow::{Result, anyhow, bail};

pub struct Field {
    pub points: Vec<[f64; 2]>,
    pub potential: Vec<f64>,
    pub flux: Vec<[f64; 2]>,
    pub energy: f64,
    pub coenergy: f64,
    pub iterations: usize,
}

struct Element {
    nodes: [usize; 3],
    area: f64,
    gradients: [[f64; 2]; 3],
}

fn element(points: &[[f64; 2]], t: [usize; 3]) -> Element {
    let [a, b, c] = t.map(|i| points[i]);
    let twice = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
    let g = |p: [f64; 2], q: [f64; 2]| [(p[1] - q[1]) / twice, (q[0] - p[0]) / twice];
    Element {
        nodes: t,
        area: 0.5 * twice,
        gradients: [g(b, c), g(c, a), g(a, b)],
    }
}

pub fn angle(plane: &Plane, step: i64) -> f64 {
    match plane.gap {
        Some(gap) => std::f64::consts::TAU * step as f64 / gap.nodes as f64,
        None => 0.0,
    }
}

pub fn placed(plane: &Plane, step: i64) -> Vec<[f64; 2]> {
    let (sin, cos) = angle(plane, step).sin_cos();
    plane
        .points
        .iter()
        .zip(&plane.turning)
        .map(|(p, &turns)| {
            if turns {
                [p[0] * cos - p[1] * sin, p[0] * sin + p[1] * cos]
            } else {
                *p
            }
        })
        .collect()
}

pub fn areas(plane: &Plane, count: usize) -> Vec<f64> {
    let mut out = vec![0.0; count];
    for (t, owner) in plane.triangles.iter().zip(&plane.region) {
        if let Some(k) = owner {
            out[*k] += element(&plane.points, *t).area;
        }
    }
    out
}

pub fn solve(plane: &Plane, shapes: &[Shape], step: i64) -> Result<Field> {
    let points = placed(plane, step);
    let turn = angle(plane, step);
    let n = points.len();
    let mut dof: Vec<Option<usize>> = vec![None; n];
    let copied: std::collections::HashSet<usize> = plane.copies.iter().map(|c| c.0).collect();
    let mut count = 0;
    for (i, slot) in dof.iter_mut().enumerate() {
        if !copied.contains(&i) {
            *slot = Some(count);
            count += 1;
        }
    }
    if let Some(gap) = plane.gap {
        let shift = step.rem_euclid(gap.nodes as i64) as usize;
        for &(copy, j) in &plane.copies {
            dof[copy] = dof[plane.ring[(j + shift) % gap.nodes]];
        }
    }
    let elements: Vec<Element> = plane
        .triangles
        .iter()
        .map(|&t| element(&points, t))
        .collect();
    if elements.iter().any(|e| e.area.is_nan() || e.area <= 0.0) {
        bail!("the slice mesh has a flat triangle");
    }
    let section = areas(plane, shapes.len());
    let mut source = vec![0.0; count];
    let nu0 = 1.0 / MU0;
    for (e, owner) in elements.iter().zip(&plane.region) {
        let Some(k) = owner else { continue };
        match &shapes[*k].stuff {
            Stuff::Coil { amps } => {
                let j = amps / section[*k];
                for &v in &e.nodes {
                    if let Some(d) = dof[v] {
                        source[d] += j * e.area / 3.0;
                    }
                }
            }
            Stuff::Magnet { br, mu, toward } => {
                let direction = magnetized(toward, e, &points, shapes[*k].turning, turn);
                let nu = nu0 / mu;
                for (local, &v) in e.nodes.iter().enumerate() {
                    if let Some(d) = dof[v] {
                        let g = e.gradients[local];
                        source[d] += nu * br * e.area * (direction[0] * g[1] - direction[1] * g[0]);
                    }
                }
            }
            _ => {}
        }
    }
    let gradient = |e: &Element, a: &[f64]| -> [f64; 2] {
        let mut g = [0.0; 2];
        for (local, &v) in e.nodes.iter().enumerate() {
            for (sum, part) in g.iter_mut().zip(e.gradients[local]) {
                *sum += a[v] * part;
            }
        }
        g
    };
    let expand = |x: &[f64]| -> Vec<f64> {
        (0..n)
            .map(|i| dof[i].map_or(0.0, |d| x[d]))
            .collect::<Vec<f64>>()
    };
    let rim: Vec<(usize, usize, f64)> = (0..plane.fixed.len())
        .map(|k| {
            let (i, j) = (plane.fixed[k], plane.fixed[(k + 1) % plane.fixed.len()]);
            let (p, q) = (points[i], points[j]);
            let radius = p[0].hypot(p[1]);
            (i, j, nu0 / radius * (q[0] - p[0]).hypot(q[1] - p[1]))
        })
        .collect();
    let energy_of = |a: &[f64]| -> f64 {
        let mut total: f64 = rim
            .iter()
            .map(|&(i, j, w)| w * (a[i] * a[i] + a[i] * a[j] + a[j] * a[j]) / 6.0)
            .sum();
        for (e, owner) in elements.iter().zip(&plane.region) {
            let g = gradient(e, a);
            let b2 = g[0] * g[0] + g[1] * g[1];
            total += e.area
                * match owner.map(|k| &shapes[k].stuff) {
                    Some(Stuff::Steel(curve)) => curve.energy(b2.sqrt()),
                    Some(Stuff::Iron { mu }) => 0.5 * nu0 / mu * b2,
                    Some(Stuff::Magnet { mu, .. }) => 0.5 * nu0 / mu * b2,
                    _ => 0.5 * nu0 * b2,
                };
        }
        total
    };
    let nonlinear = plane
        .region
        .iter()
        .any(|o| o.is_some_and(|k| matches!(shapes[k].stuff, Stuff::Steel(_))));
    let mut x = vec![0.0; count];
    let mut iterations = 0;
    let objective = |x: &[f64]| {
        let a = expand(x);
        energy_of(&a) - x.iter().zip(&source).map(|(u, f)| u * f).sum::<f64>()
    };
    let mut current = objective(&x);
    loop {
        iterations += 1;
        let a = expand(&x);
        let mut triplets: Vec<(u32, u32, f64)> = Vec::with_capacity(elements.len() * 9);
        let mut residual: Vec<f64> = source.iter().map(|f| -f).collect();
        for (e, owner) in elements.iter().zip(&plane.region) {
            let g = gradient(e, &a);
            let b = (g[0] * g[0] + g[1] * g[1]).sqrt();
            let (nu, extra) = match owner.map(|k| &shapes[k].stuff) {
                Some(Stuff::Steel(curve)) => {
                    let (nu, dh) = curve.reluctivity(b);
                    (nu, if b > 1.0e-9 { (dh - nu) / (b * b) } else { 0.0 })
                }
                Some(Stuff::Iron { mu }) | Some(Stuff::Magnet { mu, .. }) => (nu0 / mu, 0.0),
                _ => (nu0, 0.0),
            };
            let along: [f64; 3] =
                std::array::from_fn(|p| e.gradients[p][0] * g[0] + e.gradients[p][1] * g[1]);
            for p in 0..3 {
                let Some(row) = dof[e.nodes[p]] else { continue };
                residual[row] += e.area * nu * along[p];
                for q in 0..3 {
                    let Some(column) = dof[e.nodes[q]] else {
                        continue;
                    };
                    let dot = e.gradients[p][0] * e.gradients[q][0]
                        + e.gradients[p][1] * e.gradients[q][1];
                    triplets.push((
                        row as u32,
                        column as u32,
                        e.area * (nu * dot + extra * along[p] * along[q]),
                    ));
                }
            }
        }
        for &(i, j, w) in &rim {
            let (Some(di), Some(dj)) = (dof[i], dof[j]) else {
                continue;
            };
            residual[di] += w * (2.0 * a[i] + a[j]) / 6.0;
            residual[dj] += w * (a[i] + 2.0 * a[j]) / 6.0;
            triplets.push((di as u32, di as u32, w / 3.0));
            triplets.push((dj as u32, dj as u32, w / 3.0));
            triplets.push((di as u32, dj as u32, w / 6.0));
            triplets.push((dj as u32, di as u32, w / 6.0));
        }
        let matrix = Csr::from_triplets(count, &triplets);
        let right: Vec<f64> = residual.iter().map(|r| -r).collect();
        let step_x =
            direct(&matrix, &right).map_err(|e| anyhow!("the field has no answer: {e}"))?;
        let size = step_x.iter().map(|v| v * v).sum::<f64>().sqrt();
        let scale = x.iter().map(|v| v * v).sum::<f64>().sqrt().max(1.0e-30);
        let mut fraction = 1.0;
        let mut trial;
        loop {
            trial = x
                .iter()
                .zip(&step_x)
                .map(|(u, d)| u + fraction * d)
                .collect::<Vec<f64>>();
            let value = objective(&trial);
            if value <= current + 1.0e-12 * current.abs() || fraction < 1.0e-4 || !nonlinear {
                current = value;
                break;
            }
            fraction *= 0.5;
        }
        x = trial;
        if !nonlinear || size * fraction <= 1.0e-9 * scale || iterations >= 60 {
            if nonlinear && size * fraction > 1.0e-6 * scale {
                bail!("the iron did not settle after {iterations} Newton steps");
            }
            break;
        }
    }
    let potential = expand(&x);
    let flux: Vec<[f64; 2]> = elements
        .iter()
        .map(|e| {
            let g = gradient(e, &potential);
            [g[1], -g[0]]
        })
        .collect();
    let energy = energy_of(&potential);
    let mut coenergy = 0.0;
    for ((e, owner), b) in elements.iter().zip(&plane.region).zip(&flux) {
        let b_size = b[0].hypot(b[1]);
        let w = match owner.map(|k| &shapes[k].stuff) {
            Some(Stuff::Steel(curve)) => {
                let (h, _, work) = curve.h(b_size);
                h * b_size - work
            }
            Some(Stuff::Iron { mu }) => 0.5 * nu0 / mu * b_size * b_size,
            Some(Stuff::Magnet { br, mu, toward }) => {
                let d = magnetized(toward, e, &points, shapes[owner.unwrap()].turning, turn);
                let h = [nu0 / mu * (b[0] - br * d[0]), nu0 / mu * (b[1] - br * d[1])];
                0.5 * MU0 * mu * (h[0] * h[0] + h[1] * h[1]) + br * (d[0] * h[0] + d[1] * h[1])
            }
            _ => 0.5 * nu0 * b_size * b_size,
        };
        coenergy += e.area * w;
    }
    Ok(Field {
        points,
        potential,
        flux,
        energy,
        coenergy,
        iterations,
    })
}

fn magnetized(
    toward: &Toward,
    e: &Element,
    points: &[[f64; 2]],
    turning: bool,
    turn: f64,
) -> [f64; 2] {
    match toward {
        Toward::Along(d) => {
            if turning {
                let (s, c) = turn.sin_cos();
                [d[0] * c - d[1] * s, d[0] * s + d[1] * c]
            } else {
                *d
            }
        }
        Toward::Radial(sign) => {
            let m = e.nodes.iter().fold([0.0, 0.0], |m, &v| {
                [m[0] + points[v][0] / 3.0, m[1] + points[v][1] / 3.0]
            });
            let r = m[0].hypot(m[1]).max(1.0e-30);
            [sign * m[0] / r, sign * m[1] / r]
        }
    }
}

pub fn torque(plane: &Plane, field: &Field) -> f64 {
    let Some(gap) = plane.gap else { return 0.0 };
    let quarter = 0.25 * (gap.outer - gap.inner);
    let width = gap.outer - gap.inner - 2.0 * quarter;
    let mut total = 0.0;
    for &e in &plane.band {
        let t = plane.triangles[e];
        let el = element(&field.points, t);
        let m = t.iter().fold([0.0, 0.0], |m, &v| {
            [
                m[0] + field.points[v][0] / 3.0,
                m[1] + field.points[v][1] / 3.0,
            ]
        });
        let r = m[0].hypot(m[1]);
        let (c, s) = (m[0] / r, m[1] / r);
        let b = field.flux[e];
        let radial = b[0] * c + b[1] * s;
        let around = -b[0] * s + b[1] * c;
        total += r * radial * around * el.area;
    }
    total / (MU0 * width)
}

pub fn linkage(plane: &Plane, field: &Field, shape: usize, area: f64) -> f64 {
    let mut total = 0.0;
    for (t, owner) in plane.triangles.iter().zip(&plane.region) {
        if *owner == Some(shape) {
            let el = element(&field.points, *t);
            total += el.area * t.iter().map(|&v| field.potential[v]).sum::<f64>() / 3.0;
        }
    }
    total / area
}
