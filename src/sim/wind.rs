use super::{Field, Outcome, Reading, Spec, statics::FieldView};
use crate::flow::lattice::{Domain, Heat, INLET, Lattice, OUTLET, Settings, WALL};
use crate::flow::voxel::{Grid, place_surface, surface};
use anyhow::{Result, bail};
use monstertruck::modeling::*;
use std::collections::HashMap;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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

struct Spinning {
    joint: String,
    reach: f64,
    middle: Point3,
    through: Point3,
    axis: Vector3,
    omega: f64,
    parts: Vec<usize>,
}

struct Setup {
    wind: Wind,
    speed: f64,
    spins: Vec<Spinning>,
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
        self.speed * self.across * 1.0e-3 / air.1
    }

    fn spun(&self, part: usize) -> Option<usize> {
        self.spins.iter().position(|s| s.parts.contains(&part))
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
    let mut setup = frame(spec)?;
    use rayon::prelude::*;
    let spacing = setup.grid.spacing;
    let skins: Vec<_> = spec
        .named()?
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
    Ok(setup)
}

fn spinning(spec: &Spec, names: &[String]) -> Result<Vec<Spinning>> {
    let rig = spec.target()?.rig();
    spec.spins
        .iter()
        .map(|spin| {
            let Some(joint) = rig.joints.iter().find(|j| j.name == spin.joint) else {
                bail!("no joint called `{}`", spin.joint);
            };
            let crate::model::JointKind::Turn { through, axis } = joint.kind else {
                bail!("`{}` is not a `turn` joint", spin.joint);
            };
            let carried = crate::model::subtree(&rig.joints, &joint.child);
            let parts: Vec<usize> = names
                .iter()
                .enumerate()
                .filter(|(_, n)| {
                    carried.iter().any(|c| {
                        *n == c
                            || n.strip_prefix(c.as_str())
                                .is_some_and(|r| r.starts_with('.'))
                    })
                })
                .map(|(k, _)| k)
                .collect();
            if parts.is_empty() {
                bail!("`{}` carries nothing left in the flow", spin.joint);
            }
            Ok(Spinning {
                joint: spin.joint.clone(),
                reach: 0.0,
                middle: through,
                through,
                axis: axis.normalize(),
                omega: spin.rpm * std::f64::consts::TAU / 60.0,
                parts,
            })
        })
        .collect()
}

fn frame(spec: &Spec) -> Result<Setup> {
    let names: Vec<String> = spec.named()?.into_iter().map(|(n, _)| n).collect();
    let spins = spinning(spec, &names)?;
    let wind = match (spec.wind.clone(), spins.first()) {
        (Some(wind), _) => wind,
        (None, Some(spin)) => Wind {
            speed: 0.0,
            toward: spin.axis,
            about: spin.through,
        },
        (None, None) => {
            bail!("a flow simulation needs `wind speed toward=x,y,z` or a part to `spin`")
        }
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
    let mut tip: f64 = 0.0;
    let mut spins = spins;
    for spin in &mut spins {
        let (mut first, mut last) = (f64::INFINITY, f64::NEG_INFINITY);
        for &k in &spin.parts {
            for &p in &parts[k].points {
                let v = p - spin.through;
                let along = v.dot(spin.axis);
                spin.reach = spin.reach.max((v - spin.axis * along).magnitude());
                first = first.min(along);
                last = last.max(along);
            }
        }
        spin.middle = spin.through + spin.axis * (0.5 * (first + last));
        tip = tip.max(spin.omega.abs() * spin.reach * 1.0e-3);
    }
    let speed = wind.speed.max(tip);
    if speed <= 0.0 {
        bail!("nothing moves the air");
    }
    let mut setup = Setup {
        wind,
        speed,
        spins,
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
    let across = match spec.duct {
        Some([w, h]) => 2.0 * w * h / (w + h),
        None => frontal.sqrt().max(1.0e-6),
    };
    let spacing = spec.mesh.unwrap_or(across / ACROSS);
    let mut margin = Vector3::new(ASIDE, ASIDE, ASIDE) * across;
    margin[WIND] = UPSTREAM * across;
    let mut origin = low - margin - Vector3::new(0.5, 0.5, 0.5) * spacing;
    let mut extent = high - low + Vector3::new(2.0, 2.0, 2.0) * (ASIDE * across);
    extent[WIND] = high[WIND] - low[WIND] + (UPSTREAM + DOWNSTREAM) * across;
    let mut size = [0, 1, 2].map(|d| (extent[d] / spacing).ceil() as usize + 1);
    if let Some(duct) = spec.duct {
        for (axis, width) in [(SIDE, duct[0]), (UP, duct[1])] {
            let middle = 0.5 * (low[axis] + high[axis]);
            origin[axis] = middle - 0.5 * width - 0.5 * spacing;
            size[axis] = (width / spacing).round() as usize + 2;
            setup.centre[axis] = middle;
        }
    }
    setup.across = across;
    setup.frontal = frontal;
    setup.grid = Grid {
        origin,
        spacing,
        size,
    };
    Ok(setup)
}

pub fn plan(spec: &Spec) -> Result<String> {
    let setup = frame(spec)?;
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
    Ok(frame(spec)?.grid.spacing)
}

fn step_seconds(setup: &Setup) -> f64 {
    LATTICE_SPEED * setup.grid.spacing * 1.0e-3 / setup.speed
}

fn steps(setup: &Setup, spec: &Spec) -> (usize, usize, usize) {
    let pass = setup.pass();
    let through = (setup.grid.size[WIND] as f64 / LATTICE_SPEED) as usize;
    let most = if setup.spins.is_empty() { 40.0 } else { 160.0 };
    let average = (spec.average.unwrap_or(most) * pass as f64) as usize;
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
    let gpu = crate::flow::gpu::open()?;
    let size = setup.grid.size;
    let mut domain = Domain {
        half: spec.half,
        ..Domain::new(size)
    };
    let mut unseen = Vec::new();
    let voxelizing = crate::progress::stage(
        "placing parts in the grid",
        Some(setup.parts.len()),
        "parts",
    );
    for (k, part) in setup.parts.iter().enumerate() {
        voxelizing.progress(k as f64, || format!("`{}`", part.name));
        if setup.spun(k).is_some() {
            continue;
        }
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
    voxelizing.set(setup.parts.len() as f64);
    drop(voxelizing);
    if unseen.len() == setup.parts.len() {
        bail!("every part is thinner than a cell; use a finer `mesh`");
    }
    domain.close_diagonals();
    if spec.duct.is_some() {
        for z in 0..size[WIND] {
            for y in 0..size[UP] {
                for x in 0..size[SIDE] {
                    if x == 0 || y == 0 || x == size[SIDE] - 1 || y == size[UP] - 1 {
                        let cell = domain.at(x, y, z);
                        domain.flags[cell] = WALL;
                        domain.body[cell] = 0;
                    }
                }
            }
        }
    }
    let speed = LATTICE_SPEED;
    let inflow = speed * setup.wind.speed / setup.speed;
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
            if domain.flags[inlet] == WALL {
                continue;
            }
            domain.flags[outlet] = OUTLET;
            domain.outflow.push((outlet, domain.at(x, y, last - 1)));
            if inflow > 0.0 {
                domain.flags[inlet] = INLET;
                let velocity = [
                    0.02 * inflow * noise(),
                    0.02 * inflow * noise(),
                    inflow * (1.0 + 0.02 * noise()),
                ];
                domain.inflow.push((inlet, domain.at(x, y, 1), velocity));
            } else {
                domain.flags[inlet] = OUTLET;
                domain.outflow.push((inlet, domain.at(x, y, 1)));
            }
        }
    }
    if inflow == 0.0 && spec.duct.is_none() {
        let [nx, ny, _] = size;
        for z in 1..last {
            for y in 0..ny {
                for x in 0..nx {
                    if x != 0 && y != 0 && x != nx - 1 && y != ny - 1 {
                        continue;
                    }
                    let cell = domain.at(x, y, z);
                    if domain.flags[cell] == WALL {
                        continue;
                    }
                    let inward = |v: usize, n: usize| v.clamp(1, n - 2);
                    domain.flags[cell] = OUTLET;
                    domain
                        .outflow
                        .push((cell, domain.at(inward(x, nx), inward(y, ny), z)));
                }
            }
        }
    }
    let inlets: Vec<usize> = if inflow > 0.0 {
        domain.inflow.iter().map(|i| i.0).collect()
    } else {
        domain.outflow.iter().map(|o| o.0).collect()
    };
    let sealed = domain.seal(&inlets);
    domain.cuts.clear();
    let step = step_seconds(&setup);
    for (k, part) in setup.parts.iter().enumerate() {
        let Some(s) = setup.spun(k) else { continue };
        let spin = &setup.spins[s];
        let points: Vec<Point3> = part.points.iter().map(|&p| setup.to_lattice(p)).collect();
        let axis = setup.to_lattice(Point3::from_vec(spin.axis)).to_vec();
        let rotor = crate::flow::rotor::rotor(
            &mut domain,
            &setup.grid,
            (&points, &part.triangles),
            k as u16 + 1,
            crate::flow::rotor::Axle {
                centre: setup.to_lattice(spin.through),
                axis,
                omega: spin.omega * step,
            },
        );
        if !rotor.solid.iter().any(|&s| s) {
            unseen.push(part.name.clone());
        }
        domain.rotors.push(rotor);
    }
    let dx = setup.grid.spacing * 1.0e-3;
    let viscosity = air.1 * speed / (setup.speed * dx);
    let ambient = spec.ambient.unwrap_or(20.0);
    let heating = heating(
        spec,
        &setup,
        &mut domain,
        viscosity,
        step_seconds(&setup),
        air,
    )?;
    let mut settings = Settings {
        sponge: if inflow > 0.0 || spec.duct.is_none() {
            (0.5 * setup.across / setup.grid.spacing).min(40.0)
        } else {
            0.0
        },
        open_air: inflow == 0.0 && spec.duct.is_none(),
        stream: [0.0, 0.0, inflow],
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
    let cells = setup.cells() as f64;
    let rate = |done: usize, since: crate::host::Instant| {
        let seconds = since.elapsed().as_secs_f64().max(1.0e-9);
        format!(
            "{:.0} million cell updates a second",
            done as f64 * cells / seconds / 1.0e6
        )
    };
    let settling = crate::progress::stage(
        "settling the flow",
        Some(ramp.div_ceil(20) * 20 + settle),
        "steps",
    );
    let started = crate::host::Instant::now();
    let mut done = 0;
    let chunk_of = 200;
    for k in 1..=20 {
        settings.inflow = k as f64 / 20.0;
        lattice.throttle(settings.inflow);
        let mut left = ramp.div_ceil(20);
        while left > 0 {
            let chunk = left.min(chunk_of);
            lattice.run(&settings, chunk)?;
            left -= chunk;
            done += chunk;
            settling.progress(done as f64, || {
                format!("ramping the wind up, {}", rate(done, started))
            });
        }
    }
    let velocity_scale = setup.speed / speed;
    let early = settle * 2 / 3;
    let mut left = early;
    while left > 0 {
        let chunk = left.min(chunk_of);
        lattice.run(&settings, chunk)?;
        left -= chunk;
        done += chunk;
        settling.progress(done as f64, || rate(done, started));
    }
    let every_settle = (setup.pass() / 20).clamp(1, 200);
    let mut left = settle - early;
    while left > 0 {
        let chunk = left.min(every_settle);
        lattice.run(&settings, chunk)?;
        lattice.accumulate();
        left -= chunk;
        done += chunk;
        settling.progress(done as f64, || {
            format!("taking the mean for streamlines, {}", rate(done, started))
        });
    }
    drop(settling);
    let (seeds, probes): (Vec<[f64; 3]>, Vec<Vec<[f64; 3]>>) = {
        let all = if setup.wind.speed > 0.0 {
            stream_seeds(&setup)
        } else {
            fan_seeds(&setup)
        };
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
    let dx = setup.grid.spacing * 1.0e-3;
    let force_scale = air.0 * velocity_scale * velocity_scale * dx * dx;
    let arms: Vec<Option<(usize, [f64; 3])>> = links
        .iter()
        .map(|link| {
            let s = setup.spun(link[1] as usize - 1)?;
            let spin = &setup.spins[s];
            let lever = centre(&setup, link[0] as usize) - spin.through;
            Some((
                s,
                [0, 1, 2].map(|d| lever.cross(setup.axes[d]).dot(spin.axis) * 1.0e-3),
            ))
        })
        .collect();
    let spin_torques = |values: &[[f32; 4]]| {
        let mut torques = vec![0.0; setup.spins.len()];
        for (arm, v) in arms.iter().zip(values) {
            if let Some((s, arm)) = arm {
                torques[*s] -= force_scale * (0..3).map(|d| arm[d] * v[d] as f64).sum::<f64>();
            }
        }
        torques
    };
    let mut flows: Vec<f64> = Vec::new();
    let discs: Vec<(usize, usize, Vec<bool>)> = setup
        .spins
        .iter()
        .enumerate()
        .filter(|(_, spin)| spin.axis.dot(setup.axes[WIND]).abs() > 0.999)
        .map(|(k, spin)| {
            let middle = setup.to_lattice(spin.middle);
            let index = (((middle[WIND] - setup.grid.origin[WIND]) / setup.grid.spacing).round()
                as usize)
                .min(size[WIND] - 1);
            let mask = (0..size[SIDE] * size[UP])
                .map(|c| {
                    let p = setup.grid.centre(c % size[SIDE], c / size[SIDE], index);
                    let (a, b) = (p[SIDE] - middle[SIDE], p[UP] - middle[UP]);
                    (a * a + b * b).sqrt() <= spin.reach
                })
                .collect();
            (k, index, mask)
        })
        .collect();
    let mut disc_flows: Vec<Vec<f64>> = vec![Vec::new(); discs.len()];
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
    let top = 1.6 * setup.speed;
    let to_speed = setup.speed / speed;
    if !setup.spins.is_empty() {
        lattice.sum_forces();
    }
    let mut pending: Option<Ticked> = None;
    let mut drags: Vec<f64> = Vec::new();
    let mut temperatures: Vec<f64> = Vec::new();
    let mut requested_ticks = 0;
    let mut converged = None;
    let averaging = crate::progress::stage("averaging the forces", Some(samples * every), "steps");
    let averaging_from = crate::host::Instant::now();
    let to_newtons = setup.wind.speed / speed;
    let to_newtons = air.0 * to_newtons * to_newtons * (setup.grid.spacing * 1.0e-3).powi(2);
    loop {
        let requested = (converged.is_none() && requested_ticks < samples)
            .then(|| -> Result<Ticked> {
                lattice.run(&settings, every)?;
                lattice.accumulate();
                lattice.accumulate_temperature();
                let framed = requested_ticks % frame_every == 0;
                Ok(Ticked {
                    planes: planes
                        .iter()
                        .map(|w| lattice.request_plane(w.axis, w.index))
                        .collect(),
                    snapshot: (framed && !seeds.is_empty()).then(|| lattice.request_snapshot()),
                    links: lattice.request_links(),
                    probes: lattice.request_probes(),
                    outflow: spec
                        .duct
                        .is_some()
                        .then(|| lattice.request_plane(WIND, size[WIND] - 2)),
                    discs: discs
                        .iter()
                        .map(|d| lattice.request_plane(WIND, d.1))
                        .collect(),
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
        drags.push(match spin_torques(&values).first() {
            Some(&torque) => torque,
            None => values.iter().map(|v| v[WIND] as f64).sum(),
        });
        averaging.progress((drags.len() * every) as f64, || {
            let (name, unit, scale) = match setup.spins.first() {
                Some(spin) => (format!("torque on {}", spin.joint), "N·m", 1.0),
                None => ("drag".to_string(), "N", to_newtons),
            };
            let mean = drags.iter().sum::<f64>() / drags.len() as f64 * scale;
            let known = match standard_error(&drags, batch) {
                Some(e) => format!(", known to {:.1}% (stops at 1%)", e * 100.0),
                None => String::new(),
            };
            format!(
                "{name} {} {unit}{known}, {}",
                super::result::short(mean),
                rate(drags.len() * every, averaging_from)
            )
        });
        for ((series, disc), plane) in disc_flows.iter_mut().zip(&discs).zip(ready.discs) {
            let (_, plane) = lattice.take_plane(plane)?;
            let sign = setup.spins[disc.0].axis.dot(setup.axes[WIND]).signum();
            series.push(
                sign * plane
                    .iter()
                    .zip(&disc.2)
                    .filter(|(v, inside)| **inside && v[3] > 0.0)
                    .map(|(v, _)| v[2] as f64)
                    .sum::<f64>()
                    * velocity_scale
                    * dx
                    * dx,
            );
        }
        if let Some(outflow) = ready.outflow {
            let (_, plane) = lattice.take_plane(outflow)?;
            flows.push(
                plane
                    .iter()
                    .filter(|v| v[3] > 0.0)
                    .map(|v| v[2] as f64)
                    .sum::<f64>()
                    * velocity_scale
                    * dx
                    * dx,
            );
        }
        for (sum, value) in sums.iter_mut().zip(values) {
            for d in 0..4 {
                sum[d] += value[d] as f64;
            }
        }
        if let Some(heating) = &heating {
            temperatures.push(heating.mean(&lattice.take_probes(ready.probes)?));
        }
        if converged.is_none()
            && spec.average.is_none()
            && drags.len().is_multiple_of(batch)
            && drags.len() >= MIN_BATCHES * batch
            && let Some(error) = standard_error(&drags, batch)
            && error <= TARGET_ERROR
            && settled(&drags)
            && (heating.is_none()
                || standard_error(&temperatures, batch).is_some_and(|e| e <= TARGET_ERROR)
                    && settled(&temperatures))
            && disc_flows.iter().chain([&flows]).all(|series| {
                series.is_empty()
                    || standard_error(series, batch).is_some_and(|e| e <= TARGET_ERROR)
                        && settled(series)
            })
        {
            converged = Some(error);
        }
    }
    drop(averaging);
    let _finishing = crate::progress::stage("streamlines and the surface pressure", None, "");
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
                    spread.max(0.0).sqrt() * velocity_scale / setup.speed
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
    let along = if setup.wind.speed > 0.0 {
        "wind"
    } else {
        "axis"
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
                        format!("upright slice along the {along}")
                    } else {
                        format!("level slice along the {along}")
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
    ];
    if q > 0.0 {
        readings.push(reading(
            "cda",
            along(total, WIND) / q,
            "m²",
            "drag over the dynamic pressure",
        ));
    }
    let mean_values: Vec<[f32; 4]> = means.iter().map(|m| m.map(|v| v as f32)).collect();
    for (spin, torque) in setup.spins.iter().zip(spin_torques(&mean_values)) {
        let thrust: f64 = spin.parts.iter().map(|&k| forces[k].dot(spin.axis)).sum();
        readings.push(reading(
            &format!("{}.torque", spin.joint),
            torque,
            "N·m",
            "to keep it turning, about the joint axis",
        ));
        readings.push(reading(
            &format!("{}.power", spin.joint),
            torque * spin.omega,
            "W",
            "the shaft power it takes",
        ));
        readings.push(reading(
            &format!("{}.thrust", spin.joint),
            thrust,
            "N",
            "the air's push on it along the joint axis",
        ));
    }
    for (disc, series) in discs.iter().zip(&disc_flows) {
        readings.push(reading(
            &format!("{}.flow", setup.spins[disc.0].joint),
            series.iter().sum::<f64>() / series.len().max(1) as f64,
            "m³/s",
            "through the disc its blades sweep, along the joint axis",
        ));
    }
    if !flows.is_empty() {
        readings.push(reading(
            "flow",
            flows.iter().sum::<f64>() / flows.len() as f64,
            "m³/s",
            "along the duct, through its far end",
        ));
    }
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
    let mut field = Field {
        title: "mean surface pressure".into(),
        unit: "Pa".into(),
        values,
        displacement: Vec::new(),
    };
    let mut balance = None;
    if let Some(heating) = &heating {
        let mean: Vec<f64> = lattice
            .temperature_sums()?
            .iter()
            .map(|&v| v as f64 / samples.max(1) as f64)
            .collect();
        let watts = heating.watts();
        let mut heated = 0.0;
        for source in &heating.sources {
            let rise: f64 = source.cells.iter().map(|&(c, w)| w * mean[c]).sum();
            heated += rise * source.watts / watts;
            readings.push(reading(
                &source.label,
                ambient + rise,
                "°C",
                "mean temperature of the heated faces",
            ));
        }
        let mut peaks = vec![f64::NEG_INFINITY; n];
        let mut body_cells: Vec<HashMap<usize, f64>> = vec![HashMap::new(); n];
        for (cell, &rise) in mean.iter().enumerate() {
            let body = domain.body[cell] as usize;
            if domain.flags[cell] == WALL && body > 0 {
                peaks[body - 1] = peaks[body - 1].max(rise);
                body_cells[body - 1].insert(cell, ambient + rise);
            }
        }
        let peak = peaks.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        readings.push(reading(
            "temperature",
            ambient + peak,
            "°C",
            "hottest part, mean over the run",
        ));
        if n > 1 {
            for (part, peak) in setup.parts.iter().zip(&peaks) {
                if peak.is_finite() {
                    readings.push(reading(
                        &format!("{}.temperature", part.name),
                        ambient + peak,
                        "°C",
                        "",
                    ));
                }
            }
        }
        readings.push(reading(
            "resistance",
            heated / watts,
            "K/W",
            "heated faces above the inlet air, per watt",
        ));
        if inflow > 0.0 {
            let (_, plane) = lattice.plane(WIND, size[WIND] - 2)?;
            let capacity = air.0 * AIR_HEAT_CAPACITY;
            let carried: f64 = plane
                .iter()
                .enumerate()
                .filter(|(_, v)| v[3] > 0.0)
                .map(|(k, v)| {
                    let cell = k + (size[WIND] - 2) * size[SIDE] * size[UP];
                    v[2] as f64 * velocity_scale * mean[cell] * capacity * dx * dx
                })
                .sum();
            balance = Some((carried, watts));
        }
        use rayon::prelude::*;
        field = Field {
            title: "mean surface temperature".into(),
            unit: "°C".into(),
            values: setup
                .parts
                .par_iter()
                .enumerate()
                .map(|(k, part)| {
                    part.skin
                        .0
                        .iter()
                        .map(|&p| {
                            let p = setup.to_lattice(p);
                            nearest_cell(&setup, p, |c| body_cells[k].contains_key(&c))
                                .map_or(ambient, |c| body_cells[k][&c])
                        })
                        .collect::<Vec<f64>>()
                })
                .collect::<Vec<_>>()
                .concat(),
            displacement: Vec::new(),
        };
    }
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
    if let Some((carried, watts)) = balance {
        notes.push(format!(
            "the air carries {carried:.2} W of the {watts} W out of the box ({:.0}%); the rest is still warming the parts or crossing the outlet in eddies",
            100.0 * carried / watts
        ));
    }
    if spec.half {
        notes.push("populations stored as 16-bit floats (`precision half`), a preview: drag on the test cube moved 1.3%, laminar benchmarks read up to 25% off".into());
    }
    if let Some(error) = error {
        notes.push(format!(
            "the mean {} is known to {:.1}% (standard error over batches of {BATCH_PASSES} passes){}",
            match setup.spins.first() {
                Some(spin) => format!("torque on {}", spin.joint),
                None => "drag".to_string(),
            },
            100.0 * error,
            if converged.is_some() {
                format!(", so averaging stopped after {:.0} passes", (samples * every) as f64 / setup.pass() as f64)
            } else {
                String::new()
            }
        ));
    }
    for spin in &setup.spins {
        notes.push(format!(
            "{} turns at {} rpm, {} turns over the run; its cells are redrawn every step and the blades are a staircase of cells",
            spin.joint,
            crate::plot::label(spin.omega * 60.0 / std::f64::consts::TAU),
            crate::plot::label((spin.omega * total_steps as f64 * step_seconds(&setup) / std::f64::consts::TAU * 10.0).round() / 10.0),
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
        field: Some(field),
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

fn fan_seeds(setup: &Setup) -> Vec<[f64; 3]> {
    let mut seeds = Vec::new();
    for spin in &setup.spins {
        let axis = spin.axis;
        let side = axis.cross(if axis.x.abs() < 0.9 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        });
        let side = side.normalize();
        let up = axis.cross(side);
        for ring in 1..=8 {
            let radius = spin.reach * (0.15 + 0.95 * ring as f64 / 8.0);
            let count = 12 + 6 * ring;
            for k in 0..count {
                let angle =
                    std::f64::consts::TAU * (k as f64 + 0.5 * (ring % 2) as f64) / count as f64;
                let p = spin.middle + (side * angle.cos() + up * angle.sin()) * radius;
                let q = (setup.to_lattice(p) - setup.grid.origin) / setup.grid.spacing;
                seeds.push([q.x, q.y, q.z]);
            }
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
    let trace = |start: [f64; 3], sign: f64| {
        let velocity = |p: [f64; 3]| velocity(p).map(|v| v.map(|c| sign * c));
        let mut p = start;
        let mut points = Vec::new();
        let mut speeds = Vec::new();
        let mut along = Vec::new();
        let mut visited: HashMap<[i64; 3], usize> = HashMap::new();
        for n in 0..limit {
            let Some(v) = velocity(p) else { break };
            let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if n % 2 == 0 {
                along.push(p);
                points.push(world(p));
                speeds.push((speed * scale * 100.0).round() / 100.0);
                let cell = p.map(|v| (v / stride).floor() as i64);
                let here = points.len();
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
        (points, speeds, along)
    };
    let both = setup.wind.speed == 0.0;
    seeds
        .par_iter()
        .map(|&start| {
            let (mut points, mut speeds, mut along) = trace(start, 1.0);
            if both {
                let (back, back_speeds, back_along) = trace(start, -1.0);
                let skip = usize::from(!points.is_empty());
                points = back.into_iter().skip(skip).rev().chain(points).collect();
                speeds = back_speeds
                    .into_iter()
                    .skip(skip)
                    .rev()
                    .chain(speeds)
                    .collect();
                along = back_along
                    .into_iter()
                    .skip(skip)
                    .rev()
                    .chain(along)
                    .collect();
            }
            let stream = super::Stream {
                points,
                speed: speeds,
                ..Default::default()
            };
            (stream.points.len() > 2).then_some((stream, along))
        })
        .collect()
}

fn disturbed(stream: &super::Stream, setup: &Setup) -> bool {
    let slowest = stream.speed.iter().copied().fold(f64::INFINITY, f64::min);
    slowest < 0.85 * setup.speed
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
    probes: crate::flow::lattice::PendingProbes,
    outflow: Option<crate::flow::lattice::PendingPlane>,
    discs: Vec<crate::flow::lattice::PendingPlane>,
    framed: bool,
}

const PRANDTL: f64 = 0.71;
const AIR_HEAT_CAPACITY: f64 = 1006.0;

struct Source {
    label: String,
    watts: f64,
    cells: Vec<(usize, f64)>,
}

struct Heating {
    sources: Vec<Source>,
    weights: Vec<f64>,
}

impl Heating {
    fn watts(&self) -> f64 {
        self.sources.iter().map(|s| s.watts).sum()
    }

    fn mean(&self, theta: &[f32]) -> f64 {
        let total: f64 = self.weights.iter().sum();
        self.weights
            .iter()
            .zip(theta)
            .map(|(w, t)| w * *t as f64)
            .sum::<f64>()
            / total.max(f64::MIN_POSITIVE)
    }
}

fn heating(
    spec: &Spec,
    setup: &Setup,
    domain: &mut Domain,
    viscosity: f64,
    seconds: f64,
    air: (f64, f64),
) -> Result<Option<Heating>> {
    use super::fem::thermal::Boundary;
    let mut fluxes = Vec::new();
    for boundary in &spec.thermal {
        match boundary {
            Boundary::Flux {
                label,
                faces,
                watts,
            } => fluxes.push((label.clone(), faces.clone(), *watts)),
            _ => bail!(
                "a flow simulation takes `heat` lines; `temperature` and `convect` belong to `sim thermal`, the air here does the convecting"
            ),
        }
    }
    if fluxes.is_empty() {
        return Ok(None);
    }
    let model = spec.target()?;
    let names: Vec<String> = model.named_solids().into_iter().map(|(n, _)| n).collect();
    let solids = spec.named()?;
    let dx = setup.grid.spacing * 1.0e-3;
    let capacity = air.0 * AIR_HEAT_CAPACITY;
    let air_alpha = viscosity / PRANDTL;
    let mut alphas = Vec::new();
    for (k, part) in setup.parts.iter().enumerate() {
        if setup.spun(k).is_some() {
            alphas.push(air_alpha);
            continue;
        }
        let conductivity = model
            .materials
            .get(&part.name)
            .and_then(|m| m.mechanics.conductivity)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "give `{}` a material with a conductivity, like `material aluminium`, so heat can flow through it",
                    part.name
                )
            })?;
        alphas.push(conductivity / capacity * seconds / (dx * dx));
    }
    let mut cells: Vec<[f32; 2]> = domain
        .flags
        .iter()
        .zip(&domain.body)
        .map(|(&flag, &body)| match (flag, body as usize) {
            (WALL, body) if body > 0 => [alphas[body - 1] as f32, 0.0],
            _ => [air_alpha as f32, 0.0],
        })
        .collect();
    for &cell in domain.rotors.iter().flat_map(|r| &r.cells) {
        cells[cell] = [air_alpha as f32, 0.0];
    }
    let mut sources = Vec::new();
    for (label, faces, watts) in fluxes {
        let mut weights: HashMap<usize, f64> = HashMap::new();
        for tag in faces {
            let (index, face) = (tag / super::fem::STRIDE, tag % super::fem::STRIDE);
            let name = &names[index];
            let Some(k) = setup.parts.iter().position(|p| &p.name == name) else {
                bail!("`{label}` heats `{name}`, which the simulation leaves out");
            };
            if setup.spun(k).is_some() {
                bail!(
                    "`{label}` heats `{name}`, which spins; only parts that stand still can carry heat"
                );
            }
            let Some((_, solid)) = solids.iter().find(|(n, _)| n == name) else {
                continue;
            };
            use monstertruck::meshing::prelude::*;
            let meshed = solid.robust_triangulation(0.2 * setup.grid.spacing);
            let Some(mesh) = meshed
                .boundaries()
                .iter()
                .flat_map(|shell| shell.face_iter().cloned())
                .nth(face)
                .and_then(|f| f.surface())
            else {
                continue;
            };
            let positions = mesh.positions();
            let body = k as u16 + 1;
            let keep = |c: usize| domain.flags[c] == WALL && domain.body[c] == body;
            for triangle in mesh.faces().triangle_iter() {
                let [a, b, c] = triangle.map(|v| positions[v.pos]);
                let area = (b - a).cross(c - a).magnitude() / 2.0;
                let longest = [(b - a), (c - b), (a - c)]
                    .iter()
                    .map(|e| e.magnitude())
                    .fold(0.0, f64::max);
                let n = (longest / (0.5 * setup.grid.spacing)).ceil().max(1.0) as usize;
                for i in 0..n {
                    for j in 0..n - i {
                        let (u, v) = (
                            (i as f64 + 1.0 / 3.0) / n as f64,
                            (j as f64 + 1.0 / 3.0) / n as f64,
                        );
                        let p = a + (b - a) * u + (c - a) * v;
                        if let Some(cell) = nearest_cell(setup, setup.to_lattice(p), keep) {
                            *weights.entry(cell).or_default() += area / (n * n) as f64;
                        }
                    }
                }
            }
        }
        let total: f64 = weights.values().sum();
        if total <= 0.0 {
            bail!("`{label}` heats faces that have no cells; use a finer `mesh`");
        }
        let mut list: Vec<(usize, f64)> =
            weights.into_iter().map(|(c, w)| (c, w / total)).collect();
        list.sort_by_key(|&(c, _)| c);
        for &(cell, share) in &list {
            cells[cell][1] += (watts * share * seconds / (capacity * dx * dx * dx)) as f32;
        }
        sources.push(Source {
            label,
            watts,
            cells: list,
        });
    }
    let probes: Vec<usize> = sources
        .iter()
        .flat_map(|s| s.cells.iter().map(|c| c.0))
        .collect();
    let weights: Vec<f64> = sources
        .iter()
        .flat_map(|s| s.cells.iter().map(move |c| c.1 * s.watts))
        .collect();
    domain.heat = Some(Heat { cells, probes });
    Ok(Some(Heating { sources, weights }))
}

fn nearest_cell(setup: &Setup, p: Point3, keep: impl Fn(usize) -> bool) -> Option<usize> {
    let grid = &setup.grid;
    let at = (p - grid.origin) / grid.spacing;
    let base = [
        at.x.round() as i64,
        at.y.round() as i64,
        at.z.round() as i64,
    ];
    let [nx, ny, nz] = grid.size.map(|n| n as i64);
    let mut best: Option<(f64, usize)> = None;
    for reach in 1..=3i64 {
        for dz in -reach..=reach {
            for dy in -reach..=reach {
                for dx in -reach..=reach {
                    let c = [base[0] + dx, base[1] + dy, base[2] + dz];
                    if c[0] < 0 || c[1] < 0 || c[2] < 0 || c[0] >= nx || c[1] >= ny || c[2] >= nz {
                        continue;
                    }
                    let cell = (c[0] + (c[1] + c[2] * ny) * nx) as usize;
                    if keep(cell) {
                        let d =
                            (Vector3::new(c[0] as f64, c[1] as f64, c[2] as f64) - at).magnitude2();
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, cell));
                        }
                    }
                }
            }
        }
        if best.is_some() {
            break;
        }
    }
    best.map(|(_, cell)| cell)
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
    nearest_cell(setup, p, |c| links.contains_key(&c)).map_or(0.0, |c| links[&c])
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

type FacePatch = (Vec<u32>, Vec<[u32; 3]>, Point3, f64);

pub fn setup_view(spec: &Spec) -> Result<FieldView> {
    use super::fem::thermal::Boundary;
    use super::statics::{Mark, Stimulus};
    use monstertruck::meshing::prelude::*;
    let setup = frame(spec)?;
    let names: Vec<String> = spec
        .target()?
        .named_solids()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let mut positions: Vec<Point3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut edges: Vec<[u32; 3]> = Vec::new();
    let mut faces: HashMap<usize, FacePatch> = HashMap::new();
    for (name, solid) in spec.named()? {
        let index = names.iter().position(|n| *n == name).unwrap_or(0);
        let meshed = solid.robust_triangulation(crate::geometry::mesh_tolerance(solid));
        let surfaces = meshed
            .boundaries()
            .iter()
            .flat_map(|shell| shell.face_iter().cloned())
            .map(|f| f.surface())
            .collect::<Vec<_>>();
        for (face, mesh) in surfaces.into_iter().enumerate() {
            let Some(mesh) = mesh else { continue };
            let base = positions.len();
            positions.extend(mesh.positions().iter().copied());
            let tris: Vec<[usize; 3]> = mesh
                .faces()
                .triangle_iter()
                .map(|t| t.map(|v| v.pos))
                .collect();
            let (mut area, mut middle) = (0.0, Vector3::zero());
            for t in &tris {
                let [a, b, c] = t.map(|v| positions[base + v]);
                let weight = (b - a).cross(c - a).magnitude() / 2.0;
                area += weight;
                middle += (a.to_vec() + b.to_vec() + c.to_vec()) / 3.0 * weight;
            }
            triangles.extend(tris.iter().map(|t| t.map(|v| (base + v) as u32)));
            let outline: Vec<[u32; 3]> = creases(&positions[base..], &tris)
                .into_iter()
                .map(|[a, b]| [a, a, b].map(|v| (base + v) as u32))
                .collect();
            edges.extend(&outline);
            let nodes = (base as u32..positions.len() as u32).collect();
            let centre = Point3::from_vec(middle / area.max(f64::MIN_POSITIVE));
            faces.insert(
                index * super::fem::STRIDE + face,
                (nodes, outline, centre, area),
            );
        }
    }
    if positions.is_empty() {
        bail!("there is nothing left in the wind");
    }
    let mut marks = Vec::new();
    for boundary in &spec.thermal {
        let Boundary::Flux {
            label,
            faces: tags,
            watts,
        } = boundary
        else {
            continue;
        };
        let (mut nodes, mut outline) = (Vec::new(), Vec::new());
        let (mut middle, mut area) = (Vector3::zero(), 0.0);
        for (n, o, c, a) in tags.iter().filter_map(|t| faces.get(t)) {
            nodes.extend(n);
            outline.extend(o);
            middle += c.to_vec() * *a;
            area += a;
        }
        let at = Point3::from_vec(middle / area.max(f64::MIN_POSITIVE));
        let spot = nodes.iter().copied().min_by(|&a, &b| {
            (positions[a as usize] - at)
                .magnitude2()
                .total_cmp(&(positions[b as usize] - at).magnitude2())
        });
        marks.push(Mark {
            kind: Stimulus::Heat,
            text: format!("{label}: {} W", crate::plot::label(*watts)),
            nodes,
            outline,
            spot,
            at,
            toward: None,
        });
    }
    let bounds: BoundingBox<Point3> = positions.iter().copied().collect();
    let toward = setup.wind.toward.normalize();
    let centre = bounds.center();
    let upstream = positions
        .iter()
        .map(|p| (p - centre).dot(toward))
        .fold(0.0, f64::min);
    marks.push(Mark {
        kind: Stimulus::Wind,
        text: format!(
            "wind {} m/s toward {}, {}, {}",
            crate::plot::label(setup.wind.speed),
            crate::plot::label(setup.wind.toward.x),
            crate::plot::label(setup.wind.toward.y),
            crate::plot::label(setup.wind.toward.z)
        ),
        nodes: Vec::new(),
        outline: Vec::new(),
        spot: None,
        at: centre + toward * upstream,
        toward: Some(toward),
    });
    let grid = &setup.grid;
    let corner = |k: usize| {
        let lattice: [f64; 3] = [0, 1, 2].map(|d| {
            let cells = if k >> d & 1 == 0 {
                0.5
            } else {
                grid.size[d] as f64 - 1.5
            };
            grid.origin[d] + cells * grid.spacing
        });
        Point3::from_vec(setup.to_world(lattice))
    };
    let base = positions.len() as u32;
    positions.extend((0..8).map(corner));
    let mut outline = Vec::new();
    for a in 0..8u32 {
        for d in 0..3 {
            let b = a | 1 << d;
            if b != a {
                outline.push([base + a, base + a, base + b]);
            }
        }
    }
    let metres = |d: usize| {
        crate::plot::label(((grid.size[d] - 2) as f64 * grid.spacing * 1.0e-2).round() / 10.0)
    };
    marks.push(Mark {
        kind: Stimulus::Wind,
        text: format!(
            "{} {} x {} x {} m, {} mm cells",
            if spec.duct.is_some() {
                "duct"
            } else {
                "air box"
            },
            metres(SIDE),
            metres(UP),
            metres(WIND),
            crate::plot::label(grid.spacing)
        ),
        nodes: Vec::new(),
        outline,
        spot: Some(base + 7),
        at: corner(7),
        toward: None,
    });
    let count = positions.len();
    Ok(FieldView {
        shade: vec![0.0; count],
        triangles,
        edges,
        low: 0.0,
        high: 1.0,
        scale: 0.0,
        title: if spec.duct.is_some() {
            "parts in the duct".to_string()
        } else {
            "parts in the air".to_string()
        },
        unit: String::new(),
        values: Vec::new(),
        moved: vec![Vector3::zero(); count],
        rest: positions.clone(),
        positions,
        marks,
        index: Default::default(),
        slices: Vec::new(),
        streams: Vec::new(),
        travel: Default::default(),
        movie: None,
    })
}

pub(super) fn creases(points: &[Point3], triangles: &[[usize; 3]]) -> Vec<[usize; 2]> {
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
    let (samples, reach) = surface_samples(file)?;
    Ok(super::fem::elastic::SurfacePressure::new(samples, reach))
}

pub fn surface_samples(file: &std::path::Path) -> Result<(Vec<(Point3, f64)>, f64)> {
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
    Ok((
        samples
            .into_iter()
            .zip(field.values.iter().map(|pa| pa * 1.0e-6))
            .collect(),
        2.0 * setup.grid.spacing,
    ))
}
