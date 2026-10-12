use super::{Field, Outcome, Reading, Spec, statics::FieldView};
use crate::flow::gpu::Gpu;
use crate::flow::lattice::{Domain, INLET, Lattice, OUTLET, Settings};
use crate::flow::voxel::{Grid, place_surface, surface};
use anyhow::{Result, bail};
use monstertruck::modeling::*;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Wind {
    pub speed: f64,
    pub toward: Vector3,
    pub about: Point3,
}

pub const AIR: (f64, f64) = (1.204, 1.516e-5);
const LATTICE_SPEED: f64 = 0.04;
const SIDE: usize = 0;
const UP: usize = 1;
const WIND: usize = 2;
const ACROSS: f64 = 32.0;
const UPSTREAM: f64 = 4.0;
const DOWNSTREAM: f64 = 5.0;
const ASIDE: f64 = 3.0;
fn bytes_per_cell(spec: &Spec) -> f64 {
    if spec.half { 84.0 } else { 160.0 }
}

struct Part {
    name: String,
    points: Vec<Point3>,
    triangles: Vec<[usize; 3]>,
    skin: (Vec<Point3>, Vec<[usize; 3]>),
}

struct Setup {
    wind: Wind,
    axes: [Vector3; 3],
    grid: Grid,
    across: f64,
    frontal: f64,
    centre: Point3,
    bounds: (Point3, Point3),
    parts: Vec<Part>,
}

impl Setup {
    fn to_lattice(&self, p: Point3) -> Point3 {
        let v = p.to_vec();
        Point3::new(
            v.dot(self.axes[0]),
            v.dot(self.axes[1]),
            v.dot(self.axes[2]),
        )
    }

    fn to_world(&self, v: [f64; 3]) -> Vector3 {
        self.axes[0] * v[0] + self.axes[1] * v[1] + self.axes[2] * v[2]
    }

    fn cells(&self) -> usize {
        self.grid.size.iter().product()
    }

    fn pass(&self) -> usize {
        (self.across / self.grid.spacing / LATTICE_SPEED).ceil() as usize
    }

    fn reynolds(&self, air: (f64, f64)) -> f64 {
        self.wind.speed * self.across * 1.0e-3 / air.1
    }
}

fn frontal_area(setup: &Setup, low: Point3, high: Point3) -> (f64, [f64; 2]) {
    let pixel = (high[SIDE] - low[SIDE]).max(high[UP] - low[UP]) / 1024.0;
    if pixel <= 0.0 {
        return (0.0, [low[SIDE], low[UP]]);
    }
    let (w, h) = (
        ((high[SIDE] - low[SIDE]) / pixel).ceil() as usize + 1,
        ((high[UP] - low[UP]) / pixel).ceil() as usize + 1,
    );
    let mut covered = vec![false; w * h];
    for part in &setup.parts {
        let flat: Vec<[f64; 2]> = part
            .points
            .iter()
            .map(|&p| {
                let q = setup.to_lattice(p);
                [(q[SIDE] - low[SIDE]) / pixel, (q[UP] - low[UP]) / pixel]
            })
            .collect();
        for t in &part.triangles {
            let [a, b, c] = t.map(|v| flat[v]);
            let twice = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if twice.abs() < 1.0e-12 {
                continue;
            }
            let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
            let x1 = (a[0].max(b[0]).max(c[0]).ceil() as usize).min(w - 1);
            let y0 = a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize;
            let y1 = (a[1].max(b[1]).max(c[1]).ceil() as usize).min(h - 1);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let p = [x as f64 + 0.5, y as f64 + 0.5];
                    let edge = |u: [f64; 2], v: [f64; 2]| {
                        ((v[0] - u[0]) * (p[1] - u[1]) - (p[0] - u[0]) * (v[1] - u[1]))
                            * twice.signum()
                    };
                    if edge(a, b) >= 0.0 && edge(b, c) >= 0.0 && edge(c, a) >= 0.0 {
                        covered[x + y * w] = true;
                    }
                }
            }
        }
    }
    let mut sum = [0.0f64; 2];
    let mut count = 0usize;
    for (k, _) in covered.iter().enumerate().filter(|(_, c)| **c) {
        sum[0] += (k % w) as f64 + 0.5;
        sum[1] += (k / w) as f64 + 0.5;
        count += 1;
    }
    let centre = if count == 0 {
        [low[SIDE], low[UP]]
    } else {
        [
            low[SIDE] + sum[0] / count as f64 * pixel,
            low[UP] + sum[1] / count as f64 * pixel,
        ]
    };
    (count as f64 * pixel * pixel, centre)
}

fn axes(toward: Vector3) -> [Vector3; 3] {
    let x = toward.normalize();
    let up = if x.z.abs() < 0.9 {
        Vector3::unit_z()
    } else {
        Vector3::unit_y()
    };
    let z = (up - x * up.dot(x)).normalize();
    [z.cross(x), z, x]
}

fn setup(spec: &Spec) -> Result<Setup> {
    let Some(wind) = spec.wind.clone() else {
        bail!("a flow simulation needs `wind speed toward=x,y,z`");
    };
    let axes = axes(wind.toward);
    let mut parts = Vec::new();
    for (name, solid) in spec.named()? {
        let (points, triangles) = surface(solid, f64::INFINITY);
        parts.push(Part {
            name,
            points,
            triangles,
            skin: (Vec::new(), Vec::new()),
        });
    }
    if parts.is_empty() {
        bail!("there is nothing left in the wind");
    }
    let mut setup = Setup {
        wind,
        axes,
        grid: Grid {
            origin: Point3::origin(),
            spacing: 1.0,
            size: [1, 1, 1],
        },
        across: 0.0,
        frontal: 0.0,
        centre: Point3::origin(),
        bounds: (Point3::origin(), Point3::origin()),
        parts,
    };
    let mut low = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut high = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for part in &setup.parts {
        for &p in &part.points {
            let q = setup.to_lattice(p);
            for d in 0..3 {
                low[d] = low[d].min(q[d]);
                high[d] = high[d].max(q[d]);
            }
        }
    }
    let (frontal, middle) = frontal_area(&setup, low, high);
    setup.centre[SIDE] = middle[0];
    setup.centre[UP] = middle[1];
    setup.centre[WIND] = 0.5 * (low[WIND] + high[WIND]);
    setup.bounds = (low, high);
    let across = frontal.sqrt().max(1.0e-6);
    let spacing = spec.mesh.unwrap_or(across / ACROSS);
    {
        use rayon::prelude::*;
        let solids = spec.named()?;
        let skins: Vec<_> = solids
            .par_iter()
            .map(|(_, solid)| {
                let tolerance = (0.2 * spacing).max(crate::geometry::mesh_tolerance(solid));
                let (points, triangles) =
                    crate::geometry::welded(&crate::geometry::mesh(solid, tolerance));
                sampled(&points, &triangles, spacing)
            })
            .collect();
        for (part, skin) in setup.parts.iter_mut().zip(skins) {
            part.skin = skin;
        }
    }
    let mut margin = Vector3::new(ASIDE, ASIDE, ASIDE) * across;
    margin[WIND] = UPSTREAM * across;
    let origin = low - margin - Vector3::new(0.5, 0.5, 0.5) * spacing;
    let mut extent = high - low + Vector3::new(2.0, 2.0, 2.0) * (ASIDE * across);
    extent[WIND] = high[WIND] - low[WIND] + (UPSTREAM + DOWNSTREAM) * across;
    setup.across = across;
    setup.frontal = frontal;
    setup.grid = Grid {
        origin,
        spacing,
        size: [0, 1, 2].map(|d| (extent[d] / spacing).ceil() as usize + 1),
    };
    Ok(setup)
}

pub fn plan(spec: &Spec) -> Result<String> {
    let setup = setup(spec)?;
    let air = spec.air.unwrap_or(AIR);
    let [nx, ny, nz] = setup.grid.size;
    let steps = steps(&setup, spec);
    Ok(format!(
        "{nx} x {ny} x {nz} cells of {} mm, {:.1} million, about {:.1} GB of GPU memory\nfrontal area {:.4} m², {:.0} cells across its square root of {} mm, Reynolds number {:.3e}\nat most {} steps, {:.3} s of air",
        crate::plot::label(setup.grid.spacing),
        setup.cells() as f64 / 1.0e6,
        setup.cells() as f64 * bytes_per_cell(spec) / 1.0e9,
        setup.frontal * 1.0e-6,
        setup.across / setup.grid.spacing,
        crate::plot::label(setup.across),
        setup.reynolds(air),
        steps.0 + steps.1 + steps.2,
        (steps.0 + steps.1 + steps.2) as f64 * step_seconds(&setup),
    ))
}

pub fn cell(spec: &Spec) -> Result<f64> {
    Ok(setup(spec)?.grid.spacing)
}

fn step_seconds(setup: &Setup) -> f64 {
    LATTICE_SPEED * setup.grid.spacing * 1.0e-3 / setup.wind.speed
}

fn steps(setup: &Setup, spec: &Spec) -> (usize, usize, usize) {
    let pass = setup.pass();
    let through = (setup.grid.size[WIND] as f64 / LATTICE_SPEED) as usize;
    let average = (spec.average.unwrap_or(40.0) * pass as f64) as usize;
    (2 * pass, through, average.max(1))
}

const BATCH_PASSES: usize = 4;
const MIN_BATCHES: usize = 5;
const TARGET_ERROR: f64 = 0.01;

fn settled(series: &[f64]) -> bool {
    let (early, late) = series.split_at(series.len() / 2);
    let mean = |s: &[f64]| s.iter().sum::<f64>() / s.len().max(1) as f64;
    let whole = mean(series).abs();
    whole > 0.0 && (mean(early) - mean(late)).abs() <= 2.0 * TARGET_ERROR * whole
}

fn standard_error(series: &[f64], batch: usize) -> Option<f64> {
    let means: Vec<f64> = series
        .chunks_exact(batch)
        .map(|c| c.iter().sum::<f64>() / batch as f64)
        .collect();
    let n = means.len();
    if n < 2 {
        return None;
    }
    let mean = means.iter().sum::<f64>() / n as f64;
    let spread = means.iter().map(|m| (m - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    (mean.abs() > 0.0).then(|| (spread / n as f64).sqrt() / mean.abs())
}

pub fn run(spec: &Spec) -> Result<Outcome> {
    let setup = setup(spec)?;
    let air = spec.air.unwrap_or(AIR);
    let gpu = Gpu::open()?;
    let size = setup.grid.size;
    let mut domain = Domain {
        half: spec.half,
        ..Domain::new(size)
    };
    let mut unseen = Vec::new();
    for (k, part) in setup.parts.iter().enumerate() {
        let points: Vec<Point3> = part.points.iter().map(|&p| setup.to_lattice(p)).collect();
        let placed = place_surface(
            &mut domain,
            &setup.grid,
            &points,
            &part.triangles,
            k as u16 + 1,
        );
        if placed == 0 {
            unseen.push(part.name.clone());
        }
    }
    if unseen.len() == setup.parts.len() {
        bail!("every part is thinner than a cell; use a finer `mesh`");
    }
    domain.close_diagonals();
    let speed = LATTICE_SPEED;
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut noise = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    let last = size[WIND] - 1;
    for y in 0..size[1] {
        for x in 0..size[0] {
            let (inlet, outlet) = (domain.at(x, y, 0), domain.at(x, y, last));
            domain.flags[inlet] = INLET;
            domain.flags[outlet] = OUTLET;
            let velocity = [
                0.02 * speed * noise(),
                0.02 * speed * noise(),
                speed * (1.0 + 0.02 * noise()),
            ];
            domain.inflow.push((inlet, domain.at(x, y, 1), velocity));
            domain.outflow.push((outlet, domain.at(x, y, last - 1)));
        }
    }
    let inlets: Vec<usize> = domain.inflow.iter().map(|i| i.0).collect();
    let sealed = domain.seal(&inlets);
    domain.cuts.clear();
    let dx = setup.grid.spacing * 1.0e-3;
    let viscosity = air.1 * speed / (setup.wind.speed * dx);
    let mut settings = Settings {
        sponge: (0.5 * setup.across / setup.grid.spacing).min(40.0),
        stream: [0.0, 0.0, speed],
        sponge_axis: WIND,
        smagorinsky: 0.14,
        bulk: 0.1,
        ..Settings::laminar(viscosity)
    };
    let mut lattice = Lattice::new(&gpu, &domain).map_err(|e| {
        anyhow::anyhow!(
            "{e}; they need about {:.1} GB, so use a coarser `mesh`",
            setup.cells() as f64 * bytes_per_cell(spec) / 1.0e9
        )
    })?;
    let (ramp, settle, average) = steps(&setup, spec);
    for k in 1..=20 {
        settings.inflow = k as f64 / 20.0;
        lattice.run(&settings, ramp.div_ceil(20))?;
    }
    let velocity_scale = setup.wind.speed / speed;
    let early = settle * 2 / 3;
    lattice.run(&settings, early)?;
    let every_settle = (setup.pass() / 20).clamp(1, 200);
    let mut left = settle - early;
    while left > 0 {
        let chunk = left.min(every_settle);
        lattice.run(&settings, chunk)?;
        lattice.accumulate();
        left -= chunk;
    }
    let (seeds, probes): (Vec<[f64; 3]>, Vec<Vec<[f64; 3]>>) = {
        let all = stream_seeds(&setup);
        let traced = streamlines(&setup, &lattice.mean_field()?, velocity_scale, &all);
        all.into_iter()
            .zip(traced)
            .filter_map(|(seed, line)| {
                let (stream, along) = line?;
                disturbed(&stream, &setup).then_some((seed, along))
            })
            .unzip()
    };
    let mut churn: Vec<Vec<[f64; 5]>> = probes.iter().map(|p| vec![[0.0; 5]; p.len()]).collect();
    let mut moving: Vec<Vec<super::Stream>> = Vec::new();
    let every = (setup.pass() / 20).clamp(1, 200);
    let samples = average.div_ceil(every);
    let batch = (BATCH_PASSES * setup.pass()).div_ceil(every).max(1);
    let links = lattice.links.clone();
    let mut sums = vec![[0.0f64; 4]; links.len()];
    let cut = |axis: usize| {
        (((setup.centre[axis] - setup.grid.origin[axis]) / setup.grid.spacing).round() as usize)
            .min(size[axis] - 1)
    };
    let mut planes: Vec<Watch> = [SIDE, UP]
        .iter()
        .map(|&axis| Watch {
            axis,
            index: cut(axis),
            ..Default::default()
        })
        .collect();
    let frame_every = samples.div_ceil(FRAMES).max(1);
    let top = 1.6 * setup.wind.speed;
    let to_speed = setup.wind.speed / speed;
    let mut pending: Option<Ticked> = None;
    let mut drags: Vec<f64> = Vec::new();
    let mut requested_ticks = 0;
    let mut converged = None;
    loop {
        let requested = (converged.is_none() && requested_ticks < samples)
            .then(|| -> Result<Ticked> {
                lattice.run(&settings, every)?;
                lattice.accumulate();
                let framed = requested_ticks % frame_every == 0;
                Ok(Ticked {
                    planes: planes
                        .iter()
                        .map(|w| lattice.request_plane(w.axis, w.index))
                        .collect(),
                    snapshot: (framed && !seeds.is_empty()).then(|| lattice.request_snapshot()),
                    links: lattice.request_links(),
                    framed,
                })
            })
            .transpose()?;
        if requested.is_some() {
            requested_ticks += 1;
        }
        let Some(ready) = std::mem::replace(&mut pending, requested) else {
            if converged.is_some() || requested_ticks >= samples {
                break;
            }
            continue;
        };
        for (watch, plane) in planes.iter_mut().zip(ready.planes) {
            let (width, values) = lattice.take_plane(plane)?;
            if watch.mean.is_empty() {
                watch.mean = vec![[0.0; 3]; values.len()];
            }
            for (m, v) in watch.mean.iter_mut().zip(&values) {
                for d in 0..3 {
                    m[d] += v[d] as f64;
                }
            }
            watch.width = width;
            if ready.framed {
                watch
                    .frames
                    .push(frame_bytes(&values, width, to_speed, top));
            }
            watch.last = values;
        }
        if let Some(snapshot) = ready.snapshot {
            let now = lattice.take_snapshot(snapshot)?;
            for (sums, points) in churn.iter_mut().zip(&probes) {
                for (a, p) in sums.iter_mut().zip(points) {
                    if let Some((v, _)) = sample(&now, *p) {
                        for k in 0..3 {
                            a[k] += v[k];
                        }
                        a[3] += v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
                        a[4] += 1.0;
                    }
                }
            }
            moving.push(
                streamlines(&setup, &now, velocity_scale, &seeds)
                    .into_iter()
                    .map(|line| line.map(|(s, _)| s).unwrap_or_default())
                    .collect(),
            );
        }
        let values = lattice.take_links(ready.links)?;
        drags.push(values.iter().map(|v| v[WIND] as f64).sum());
        for (sum, value) in sums.iter_mut().zip(values) {
            for d in 0..4 {
                sum[d] += value[d] as f64;
            }
        }
        if converged.is_none()
            && spec.average.is_none()
            && drags.len().is_multiple_of(batch)
            && drags.len() >= MIN_BATCHES * batch
            && let Some(error) = standard_error(&drags, batch)
            && error <= TARGET_ERROR
            && settled(&drags)
        {
            converged = Some(error);
        }
    }
    let samples = drags.len();
    let error = converged.or_else(|| standard_error(&drags, batch));
    let mean = lattice.mean_field()?;
    let gauge = air.0 * velocity_scale * velocity_scale / 3.0;
    let streams: Vec<super::Stream> = streamlines(&setup, &mean, velocity_scale, &seeds)
        .into_iter()
        .zip(&churn)
        .map(|(line, sums)| {
            let Some((mut stream, along)) = line else {
                return super::Stream::default();
            };
            stream.pressure = along
                .iter()
                .filter_map(|p| sample(&mean, *p)?.1)
                .map(|rho| (rho - 1.0) * gauge)
                .fold(
                    0.0,
                    |best: f64, p| if p.abs() > best.abs() { p } else { best },
                );
            stream.pressure = (stream.pressure * 10.0).round() / 10.0;
            stream.turbulence = sums
                .iter()
                .filter(|a| a[4] > 0.0)
                .map(|a| {
                    let n = a[4];
                    let m = [a[0] / n, a[1] / n, a[2] / n];
                    let spread = a[3] / n - (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]);
                    spread.max(0.0).sqrt() * velocity_scale / setup.wind.speed
                })
                .fold(0.0, f64::max);
            stream.turbulence = (stream.turbulence * 1000.0).round() / 1000.0;
            stream
        })
        .collect();
    let movie = super::Movie {
        top,
        seconds: (every * frame_every) as f64 * step_seconds(&setup),
        planes: planes
            .iter()
            .map(|w| super::Reel {
                width: w.mean.len() / w.width.max(1),
                height: w.width,
                frames: w.frames.clone(),
            })
            .collect(),
        streams: moving,
    };
    let slices = planes
        .into_iter()
        .map(
            |Watch {
                 axis,
                 index,
                 width,
                 mean,
                 last,
                 ..
             }| {
                let other = if axis == SIDE { UP } else { SIDE };
                let height = width;
                let long = mean.len() / width.max(1);
                let mut first = [0usize; 3];
                first[axis] = index;
                first[other] = height - 1;
                let corner = setup.grid.centre(first[0], first[1], first[2]);
                let origin = Point3::origin() + setup.to_world([corner.x, corner.y, corner.z]);
                let step = |a: usize, sign: f64| {
                    let mut v = [0.0; 3];
                    v[a] = sign * setup.grid.spacing;
                    let w = setup.to_world(v);
                    [w.x, w.y, w.z]
                };
                let round = |v: f64| (v * 100.0).round() / 100.0;
                let mut slice = super::Slice {
                    title: if axis == SIDE {
                        "upright slice along the wind".into()
                    } else {
                        "level slice along the wind".into()
                    },
                    width: long,
                    spacing: setup.grid.spacing,
                    across: if axis == SIDE {
                        "up".into()
                    } else {
                        "side".into()
                    },
                    origin: [origin.x, origin.y, origin.z],
                    right: step(WIND, 1.0),
                    down: step(other, -1.0),
                    ..Default::default()
                };
                for row in 0..height {
                    let a = height - 1 - row;
                    for c in 0..long {
                        let k = a + c * width;
                        let solid = last[k][3] < 0.0;
                        let m = mean[k].map(|v| v / samples as f64 * velocity_scale);
                        let l = last[k].map(|v| v as f64 * velocity_scale);
                        let magnitude =
                            |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                        slice
                            .speed
                            .push(if solid { -1.0 } else { round(magnitude(m)) });
                        slice.snapshot.push(if solid {
                            -1.0
                        } else {
                            round(magnitude([l[0], l[1], l[2]]))
                        });
                        slice.along.push(if solid { 0.0 } else { round(m[WIND]) });
                        slice.side.push(if solid { 0.0 } else { round(m[other]) });
                    }
                }
                slice
            },
        )
        .collect::<Vec<_>>();
    let means: Vec<[f64; 4]> = sums.iter().map(|s| s.map(|v| v / samples as f64)).collect();
    let force_scale = air.0 * velocity_scale * velocity_scale * dx * dx;
    let pressure_scale = air.0 * velocity_scale * velocity_scale / 3.0;
    let n = setup.parts.len();
    let mut forces = vec![Vector3::zero(); n];
    let mut moment = Vector3::zero();
    let mut at_link: Vec<HashMap<usize, f64>> = vec![HashMap::new(); n];
    for (link, mean) in links.iter().zip(&means) {
        let (cell, body) = (link[0] as usize, link[1] as usize - 1);
        let force = setup.to_world([mean[0], mean[1], mean[2]]) * force_scale;
        let position = centre(&setup, cell);
        forces[body] += force;
        moment += (position - setup.wind.about).cross(force) * 1.0e-3;
        at_link[body].insert(cell, (mean[3] - 1.0) * pressure_scale);
    }
    let total: Vector3 = forces.iter().copied().sum();
    let along = |f: Vector3, d: usize| f.dot(setup.axes[d]);
    let q = 0.5 * air.0 * setup.wind.speed * setup.wind.speed;
    let mut readings = vec![
        reading("drag", along(total, WIND), "N", "along the wind"),
        reading("side", along(total, SIDE), "N", "across the wind, level"),
        reading("lift", along(total, UP), "N", "across the wind, up"),
        reading(
            "cda",
            along(total, WIND) / q,
            "m²",
            "drag over the dynamic pressure",
        ),
    ];
    let about = setup.wind.about;
    for (d, axis) in ["x", "y", "z"].iter().enumerate() {
        readings.push(reading(
            &format!("moment.{axis}"),
            moment[d],
            "N·m",
            &format!("about {},{},{}", about.x, about.y, about.z),
        ));
    }
    if n > 1 {
        for (part, force) in setup.parts.iter().zip(&forces) {
            for (d, name) in [(WIND, "drag"), (SIDE, "side"), (UP, "lift")] {
                readings.push(reading(
                    &format!("{}.{name}", part.name),
                    along(*force, d),
                    "N",
                    "",
                ));
            }
        }
    }
    let values: Vec<f64> = {
        use rayon::prelude::*;
        setup
            .parts
            .par_iter()
            .enumerate()
            .map(|(k, part)| {
                let points = &part.skin.0;
                points
                    .iter()
                    .map(|&p| nearest(&setup, &at_link[k], setup.to_lattice(p)))
                    .collect::<Vec<f64>>()
            })
            .collect::<Vec<_>>()
            .concat()
    };
    let (high, low) = values
        .iter()
        .fold((f64::NEG_INFINITY, f64::INFINITY), |(h, l), &v| {
            (h.max(v), l.min(v))
        });
    readings.push(reading(
        "pressure",
        high,
        "Pa",
        "highest mean pressure on the surface",
    ));
    readings.push(reading(
        "suction",
        low,
        "Pa",
        "lowest mean pressure on the surface",
    ));
    let total_steps = ramp + settle + samples * every;
    let mut notes = vec![
        format!(
            "{} x {} x {} cells of {} mm, {} steps; forces averaged over {:.3} s of air after {:.3} s to settle",
            size[0],
            size[1],
            size[2],
            crate::plot::label(setup.grid.spacing),
            total_steps,
            (samples * every) as f64 * step_seconds(&setup),
            (ramp + settle) as f64 * step_seconds(&setup)
        ),
        format!(
            "Reynolds number {:.3e} on {} mm; a large-eddy simulation that does not resolve the boundary layers, so sharp-edged parts read better than smooth rounded ones",
            setup.reynolds(air),
            crate::plot::label(setup.across)
        ),
    ];
    if spec.half {
        notes.push("populations stored as 16-bit floats (`precision half`), a preview: drag on the test cube moved 1.3%, laminar benchmarks read up to 25% off".into());
    }
    if let Some(error) = error {
        notes.push(format!(
            "the mean drag is known to {:.1}% (standard error over batches of {BATCH_PASSES} passes){}",
            100.0 * error,
            if converged.is_some() {
                format!(", so averaging stopped after {:.0} passes", (samples * every) as f64 / setup.pass() as f64)
            } else {
                String::new()
            }
        ));
    }
    if sealed > 0 {
        notes.push(format!(
            "{:.0} cm³ of air sealed inside the parts is left out of the flow",
            sealed as f64 * setup.grid.spacing.powi(3) * 1.0e-3
        ));
    }
    if !unseen.is_empty() {
        notes.push(format!(
            "{} take no cells of their own, being inside other parts or thinner than a cell, and read no force",
            unseen.join(", ")
        ));
    }
    Ok(Outcome {
        kind: "flow".into(),
        slices,
        streams,
        movie: Some(movie),
        readings,
        notes,
        field: Some(Field {
            title: "mean surface pressure".into(),
            unit: "Pa".into(),
            values,
            displacement: Vec::new(),
        }),
        ..Default::default()
    })
}

fn sampled(
    points: &[Point3],
    triangles: &[[usize; 3]],
    spacing: f64,
) -> (Vec<Point3>, Vec<[usize; 3]>) {
    let mut out = Vec::new();
    let mut index: HashMap<[i64; 3], usize> = HashMap::new();
    let quantum = spacing * 1.0e-6;
    let mut add = |p: Point3, out: &mut Vec<Point3>| -> usize {
        let key = [p.x, p.y, p.z].map(|v| (v / quantum).round() as i64);
        *index.entry(key).or_insert_with(|| {
            out.push(p);
            out.len() - 1
        })
    };
    let mut tris = Vec::new();
    for t in triangles {
        let [a, b, c] = t.map(|v| points[v]);
        let longest = [(b - a), (c - b), (a - c)]
            .iter()
            .map(|e| e.magnitude())
            .fold(0.0, f64::max);
        let n = ((longest / spacing).ceil() as usize).clamp(1, 64);
        let at = |i: usize, j: usize| {
            a + (b - a) * (i as f64 / n as f64) + (c - a) * (j as f64 / n as f64)
        };
        let ids: Vec<Vec<usize>> = (0..=n)
            .map(|i| (0..=n - i).map(|j| add(at(i, j), &mut out)).collect())
            .collect();
        for i in 0..n {
            for j in 0..n - i {
                tris.push([ids[i][j], ids[i + 1][j], ids[i][j + 1]]);
                if i + j + 1 < n {
                    tris.push([ids[i + 1][j], ids[i + 1][j + 1], ids[i][j + 1]]);
                }
            }
        }
    }
    (out, tris)
}

fn stream_seeds(setup: &Setup) -> Vec<[f64; 3]> {
    let (low, high) = setup.bounds;
    let spacing = setup.grid.spacing;
    let fine = |p: Point3| {
        let q = (p - setup.grid.origin) / spacing;
        [q.x, q.y, q.z]
    };
    let widen = |a: usize| {
        let pad = 0.15 * (high[a] - low[a]) + 0.1 * setup.across;
        (low[a] - pad, high[a] + pad)
    };
    let (side, up) = (widen(SIDE), widen(UP));
    let aspect = (side.1 - side.0) / (up.1 - up.0).max(1.0e-9);
    let wanted = 2400.0;
    let along_side = ((wanted * aspect).sqrt().round() as usize).clamp(4, 160);
    let along_up = ((wanted / along_side as f64).round() as usize).clamp(4, 160);
    let mut seeds = Vec::new();
    for i in 0..along_side {
        for j in 0..along_up {
            let mut p = Point3::origin();
            p[SIDE] = side.0 + (side.1 - side.0) * (i as f64 + 0.5) / along_side as f64;
            p[UP] = up.0 + (up.1 - up.0) * (j as f64 + 0.5) / along_up as f64;
            p[WIND] = low[WIND] - 0.4 * setup.across;
            seeds.push(fine(p));
        }
    }
    seeds
}

fn sample(field: &crate::flow::lattice::MeanField, p: [f64; 3]) -> Option<([f64; 3], Option<f64>)> {
    let stride = field.stride as f64;
    let [cx, cy, _] = field.size;
    let c = p.map(|v| (v - 0.5 * (stride - 1.0)) / stride);
    let base = c.map(|v| v.floor());
    if base
        .iter()
        .zip(field.size)
        .any(|(b, n)| *b < 0.0 || *b + 1.0 >= n as f64)
    {
        return None;
    }
    let [bx, by, bz] = base.map(|v| v as usize);
    let f = [c[0] - base[0], c[1] - base[1], c[2] - base[2]];
    let mut total = [0.0; 3];
    let mut mass = 0.0;
    let mut weight = 0.0;
    for (dx, dy, dz) in [
        (0, 0, 0),
        (1, 0, 0),
        (0, 1, 0),
        (1, 1, 0),
        (0, 0, 1),
        (1, 0, 1),
        (0, 1, 1),
        (1, 1, 1),
    ] {
        let w = [dx, dy, dz]
            .iter()
            .zip(f)
            .map(|(d, t)| if *d == 1 { t } else { 1.0 - t })
            .product::<f64>();
        let at = (bx + dx) + ((by + dy) + (bz + dz) * cy) * cx;
        match field.velocity[at] {
            Some(v) => {
                for k in 0..3 {
                    total[k] += w * v[k] as f64;
                }
                mass += w * field.density.get(at).copied().flatten().unwrap_or(1.0) as f64;
                weight += w;
            }
            None if w > 0.5 => return None,
            None => {}
        }
    }
    (weight > 0.0).then(|| {
        let scale = weight.max(1.0);
        (
            total.map(|v| v / scale),
            (!field.density.is_empty()).then(|| mass / weight),
        )
    })
}

fn streamlines(
    setup: &Setup,
    field: &crate::flow::lattice::MeanField,
    scale: f64,
    seeds: &[[f64; 3]],
) -> Vec<Option<(super::Stream, Vec<[f64; 3]>)>> {
    use rayon::prelude::*;
    let stride = field.stride as f64;
    let size = setup.grid.size.map(|n| n as f64);
    let velocity = |p: [f64; 3]| sample(field, p).map(|(v, _)| v);
    let spacing = setup.grid.spacing;
    let step = 0.5 * stride;
    let limit = (3.0 * size[WIND] / step) as usize;
    let calm = 0.02 * LATTICE_SPEED;
    let world = |p: [f64; 3]| {
        let local = setup.grid.origin + Vector3::new(p[0], p[1], p[2]) * spacing;
        let w = Point3::origin() + setup.to_world([local.x, local.y, local.z]);
        [w.x, w.y, w.z].map(|v| (v * 10.0).round() / 10.0)
    };
    seeds
        .par_iter()
        .map(|&start| {
            let mut p = start;
            let mut stream = super::Stream::default();
            let mut along = Vec::new();
            let mut visited: HashMap<[i64; 3], usize> = HashMap::new();
            for n in 0..limit {
                let Some(v) = velocity(p) else { break };
                let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                if n % 2 == 0 {
                    along.push(p);
                    stream.points.push(world(p));
                    stream.speed.push((speed * scale * 100.0).round() / 100.0);
                    let cell = p.map(|v| (v / stride).floor() as i64);
                    let here = stream.points.len();
                    if visited
                        .insert(cell, here)
                        .is_some_and(|then| here - then > 3)
                    {
                        break;
                    }
                }
                if speed < calm {
                    break;
                }
                let mid = [0, 1, 2].map(|k| p[k] + 0.5 * step * v[k] / speed);
                let Some(w) = velocity(mid) else { break };
                let half = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
                if half < calm {
                    break;
                }
                p = [0, 1, 2].map(|k| p[k] + step * w[k] / half);
                if p.iter().zip(size).any(|(v, n)| *v < 0.0 || *v > n - 1.0) {
                    break;
                }
            }
            (stream.points.len() > 2).then_some((stream, along))
        })
        .collect()
}

fn disturbed(stream: &super::Stream, setup: &Setup) -> bool {
    let slowest = stream.speed.iter().copied().fold(f64::INFINITY, f64::min);
    slowest < 0.85 * setup.wind.speed
}

#[derive(Default)]
struct Watch {
    axis: usize,
    index: usize,
    width: usize,
    mean: Vec<[f64; 3]>,
    last: Vec<[f32; 4]>,
    frames: Vec<Vec<u8>>,
}

struct Ticked {
    planes: Vec<crate::flow::lattice::PendingPlane>,
    snapshot: Option<crate::flow::lattice::PendingSnapshot>,
    links: crate::flow::lattice::PendingLinks,
    framed: bool,
}

const FRAMES: usize = 240;

fn frame_bytes(values: &[[f32; 4]], width: usize, scale: f64, top: f64) -> Vec<u8> {
    let long = values.len() / width.max(1);
    let mut out = Vec::with_capacity(values.len());
    for row in 0..width {
        let a = width - 1 - row;
        for c in 0..long {
            let v = values[a + c * width];
            out.push(if v[3] < 0.0 {
                255
            } else {
                let speed = ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as f64).sqrt() * scale;
                (speed / top * 254.0).round().min(254.0) as u8
            });
        }
    }
    out
}

fn reading(name: &str, value: f64, unit: &str, at: &str) -> Reading {
    Reading {
        name: name.into(),
        value,
        unit: unit.into(),
        at: at.into(),
    }
}

fn centre(setup: &Setup, cell: usize) -> Point3 {
    let [nx, ny, _] = setup.grid.size;
    let local = setup
        .grid
        .centre(cell % nx, (cell / nx) % ny, cell / (nx * ny));
    Point3::origin() + setup.to_world([local.x, local.y, local.z])
}

fn nearest(setup: &Setup, links: &HashMap<usize, f64>, p: Point3) -> f64 {
    let grid = &setup.grid;
    let at = (p - grid.origin) / grid.spacing;
    let base = [
        at.x.round() as i64,
        at.y.round() as i64,
        at.z.round() as i64,
    ];
    let [nx, ny, nz] = grid.size.map(|n| n as i64);
    let mut best = (f64::INFINITY, 0.0);
    for reach in 1..=3i64 {
        for dz in -reach..=reach {
            for dy in -reach..=reach {
                for dx in -reach..=reach {
                    let c = [base[0] + dx, base[1] + dy, base[2] + dz];
                    if c[0] < 0 || c[1] < 0 || c[2] < 0 || c[0] >= nx || c[1] >= ny || c[2] >= nz {
                        continue;
                    }
                    let cell = (c[0] + (c[1] + c[2] * ny) * nx) as usize;
                    if let Some(&value) = links.get(&cell) {
                        let d =
                            (Vector3::new(c[0] as f64, c[1] as f64, c[2] as f64) - at).magnitude2();
                        if d < best.0 {
                            best = (d, value);
                        }
                    }
                }
            }
        }
        if best.0.is_finite() {
            break;
        }
    }
    best.1
}

pub fn field_view(spec: &Spec, outcome: &Outcome) -> Result<FieldView> {
    let Some(field) = &outcome.field else {
        bail!("the result has no field to draw; rerun `gcad sim`");
    };
    let setup = setup(spec)?;
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    let mut edges = Vec::new();
    for part in &setup.parts {
        let (points, tris) = part.skin.clone();
        let base = positions.len() as u32;
        edges.extend(
            creases(&points, &tris)
                .into_iter()
                .map(|[a, b]| [a as u32 + base, a as u32 + base, b as u32 + base]),
        );
        triangles.extend(tris.iter().map(|t| t.map(|v| v as u32 + base)));
        positions.extend(points);
    }
    if positions.len() != field.values.len() {
        bail!("the result does not match the parts; rerun `gcad sim`");
    }
    let high = field
        .values
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let low = field.values.iter().copied().fold(f64::INFINITY, f64::min);
    let span = (high - low).max(1.0e-12);
    Ok(FieldView {
        shade: field.values.iter().map(|v| (v - low) / span).collect(),
        triangles,
        edges,
        low,
        high: low + span,
        scale: 0.0,
        title: field.title.clone(),
        unit: field.unit.clone(),
        values: field.values.clone(),
        moved: vec![Vector3::zero(); positions.len()],
        slices: outcome.slices.clone(),
        streams: outcome.streams.clone(),
        travel: Default::default(),
        movie: spec
            .origin
            .as_deref()
            .map(super::movie_path)
            .and_then(|path| super::Movie::read(&path).ok())
            .filter(|movie| {
                movie.planes.len() == outcome.slices.len()
                    && movie
                        .planes
                        .iter()
                        .zip(&outcome.slices)
                        .all(|(reel, slice)| {
                            reel.width == slice.width && reel.height == slice.height()
                        })
            })
            .map(std::sync::Arc::new),
        rest: positions.clone(),
        positions,
        marks: Vec::new(),
        index: Default::default(),
    })
}

fn creases(points: &[Point3], triangles: &[[usize; 3]]) -> Vec<[usize; 2]> {
    let normal = |t: &[usize; 3]| {
        (points[t[1]] - points[t[0]])
            .cross(points[t[2]] - points[t[0]])
            .normalize()
    };
    let mut sides: HashMap<(usize, usize), Vec<Vector3>> = HashMap::new();
    for t in triangles {
        let n = normal(t);
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            sides.entry((a.min(b), a.max(b))).or_default().push(n);
        }
    }
    sides
        .into_iter()
        .filter(|(_, normals)| normals.len() != 2 || normals[0].dot(normals[1]) < 0.5)
        .map(|((a, b), _)| [a, b])
        .collect()
}

pub fn surface_pressure(file: &std::path::Path) -> Result<super::fem::elastic::SurfacePressure> {
    let run = crate::model::run_path(file, &[], None)?;
    if let Some((line, Err(error))) = run.steps.last() {
        bail!("{}:{}: {error:#}", file.display(), line.number);
    }
    let Some(spec) = run.model.sim.as_deref() else {
        bail!("{} is not a simulation", file.display());
    };
    if spec.kind != Some(super::Kind::Flow) {
        bail!("{} is not a `sim flow`", file.display());
    }
    let result = super::result_path(file);
    if !crate::host::is_file(&result) {
        bail!(
            "{} has no result yet; run `gcad sim {}` first",
            file.display(),
            file.display()
        );
    }
    if let Some(why) = super::stale(file, &[]) {
        bail!(
            "the result of {} is stale ({why}); run `gcad sim {}` again",
            file.display(),
            file.display()
        );
    }
    let outcome = Outcome::read(&result)?;
    let Some(field) = outcome.field else {
        bail!("{} has no pressure field; run it again", file.display());
    };
    let setup = setup(spec)?;
    let mut samples = Vec::new();
    for part in &setup.parts {
        samples.extend(part.skin.0.iter().copied());
    }
    if samples.len() != field.values.len() {
        bail!(
            "the result of {} does not match its parts; run it again",
            file.display()
        );
    }
    Ok(super::fem::elastic::SurfacePressure::new(
        samples
            .into_iter()
            .zip(field.values.iter().map(|pa| pa * 1.0e-6)),
        2.0 * setup.grid.spacing,
    ))
}
