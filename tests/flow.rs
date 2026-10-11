use gcad::flow::gpu::Gpu;
use gcad::flow::lattice::{C, Collision, Domain, INLET, Lattice, MOVING, OUTLET, Settings, WALL};

static DEVICE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn device() -> (std::sync::MutexGuard<'static, ()>, Gpu) {
    let guard = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
    (guard, Gpu::open().expect("a compute device"))
}

fn at(size: [usize; 3], x: usize, y: usize, z: usize) -> usize {
    x + (y + z * size[1]) * size[0]
}

fn domain(size: [usize; 3], flags: Vec<u8>) -> Domain {
    Domain {
        flags,
        ..Domain::new(size)
    }
}

#[test]
fn poiseuille_between_plates() {
    let (_device, gpu) = device();
    let size = [4, 34, 4];
    let mut flags = vec![0u8; size.iter().product()];
    for x in 0..size[0] {
        for z in 0..size[2] {
            flags[at(size, x, 0, z)] = WALL;
            flags[at(size, x, size[1] - 1, z)] = WALL;
        }
    }
    for collision in [Collision::Bgk, Collision::Regularized] {
        let settings = Settings {
            force: [1.0e-6, 0.0, 0.0],
            collision,
            ..Settings::laminar(1.0 / 30.0)
        };
        let mut lattice = Lattice::new(&gpu, &domain(size, flags.clone())).expect("lattice");
        lattice.run(&settings, 40_000).expect("runs");
        let fields = lattice.fields(settings.force).expect("fields");
        let (low, high) = (0.5, size[1] as f64 - 1.5);
        let peak = settings.force[0] * (high - low).powi(2) / (8.0 * settings.viscosity);
        for y in 1..size[1] - 1 {
            let y = y as f64;
            let exact = settings.force[0] / (2.0 * settings.viscosity) * (y - low) * (high - y);
            let got = fields.velocity[at(size, 1, y as usize, 1)][0];
            assert!(
                (got - exact).abs() <= peak * 2.0e-3,
                "{collision:?} y {y}: {got} against {exact} ({} on {})",
                gpu.name,
                peak
            );
        }
    }
}

fn cavity(side: usize, reynolds: f64, steps: usize) -> impl Fn(f64) -> f64 {
    let (_device, gpu) = device();
    let size = [side + 2, side + 2, 1];
    let mut flags = vec![0u8; size.iter().product()];
    for i in 0..size[0] {
        flags[at(size, i, 0, 0)] = WALL;
        flags[at(size, 0, i, 0)] = WALL;
        flags[at(size, size[0] - 1, i, 0)] = WALL;
    }
    for i in 1..size[0] - 1 {
        flags[at(size, i, size[1] - 1, 0)] = MOVING;
    }
    let lid = 0.1;
    let settings = Settings {
        wall: [lid, 0.0, 0.0],
        ..Settings::laminar(lid * side as f64 / reynolds)
    };
    let mut lattice = Lattice::new(&gpu, &domain(size, flags.clone())).expect("lattice");
    lattice.run(&settings, steps).expect("runs");
    let fields = lattice.fields(settings.force).expect("fields");
    move |y: f64| {
        let j = y * side as f64 + 0.5;
        let (j0, t) = (j.floor() as usize, j - j.floor());
        let column = |j: usize| {
            0.5 * (fields.velocity[at(size, side / 2, j, 0)][0]
                + fields.velocity[at(size, side / 2 + 1, j, 0)][0])
        };
        (column(j0) * (1.0 - t) + column(j0 + 1) * t) / lid
    }
}

fn against_ghia(u: impl Fn(f64) -> f64, table: &[(f64, f64)], tolerance: f64) {
    for &(y, expected) in table {
        let got = u(y);
        assert!(
            (got - expected).abs() <= tolerance,
            "u at y {y}: {got:.4} against Ghia's {expected}"
        );
    }
}

#[test]
fn lid_driven_cavity_re100() {
    let ghia = [
        (0.9766, 0.84123),
        (0.9688, 0.78871),
        (0.9609, 0.73722),
        (0.9531, 0.68717),
        (0.8516, 0.23151),
        (0.7344, 0.00332),
        (0.6172, -0.13641),
        (0.5, -0.20581),
        (0.4531, -0.21090),
        (0.2813, -0.15662),
        (0.1719, -0.10150),
        (0.1016, -0.06434),
        (0.0703, -0.04775),
        (0.0625, -0.04192),
        (0.0547, -0.03717),
    ];
    against_ghia(cavity(128, 100.0, 60_000), &ghia, 0.01);
}

#[test]
fn lid_driven_cavity_re1000() {
    let ghia = [
        (0.9766, 0.65928),
        (0.9688, 0.57492),
        (0.9609, 0.51117),
        (0.9531, 0.46604),
        (0.8516, 0.33304),
        (0.7344, 0.18719),
        (0.6172, 0.05702),
        (0.5, -0.06080),
        (0.4531, -0.10648),
        (0.2813, -0.27805),
        (0.1719, -0.38289),
        (0.1016, -0.29730),
        (0.0703, -0.22220),
        (0.0625, -0.20196),
        (0.0547, -0.18109),
    ];
    against_ghia(cavity(256, 1000.0, 250_000), &ghia, 0.015);
}

#[test]
#[ignore = "throughput on the GPU, run with --ignored --nocapture"]
fn throughput() {
    let (_device, gpu) = device();
    let size = [128, 128, 128];
    let flags = vec![0u8; size.iter().product()];
    let settings = Settings {
        force: [1.0e-7, 0.0, 0.0],
        ..Settings::laminar(0.01)
    };
    let mut lattice = Lattice::new(&gpu, &domain(size, flags.clone())).expect("lattice");
    lattice.run(&settings, 20).expect("warms up");
    let start = std::time::Instant::now();
    let steps = 400;
    lattice.run(&settings, steps).expect("runs");
    let seconds = start.elapsed().as_secs_f64();
    let cells = size.iter().product::<usize>() as f64;
    println!(
        "{}: {:.0} million cell updates a second",
        gpu.name,
        cells * steps as f64 / seconds / 1.0e6
    );
}

fn schaefer_turek(
    diameter: usize,
    reynolds: f64,
    steps: usize,
    samples: usize,
    every: usize,
) -> Channel {
    let (_device, gpu) = device();
    let d = diameter as f64;
    let height = (4.1 * d).round() as usize;
    let length = 22 * diameter;
    let size = [length + 2, height + 2, 1];
    let mut domain = Domain::new(size);
    let (cx, cy, r) = (0.5 + 2.0 * d, 0.5 + 2.0 * d, d / 2.0);
    for x in 0..size[0] {
        for y in 0..size[1] {
            let cell = domain.at(x, y, 0);
            if y == 0 || y == size[1] - 1 {
                domain.flags[cell] = WALL;
            } else if (x as f64 - cx).powi(2) + (y as f64 - cy).powi(2) <= r * r {
                domain.flags[cell] = WALL;
                domain.body[cell] = 1;
            }
        }
    }
    let solid = |x: f64, y: f64| (x - cx).powi(2) + (y - cy).powi(2) <= r * r;
    for x in 1..size[0] - 1 {
        for y in 1..size[1] - 1 {
            if solid(x as f64, y as f64) {
                continue;
            }
            for (j, c) in C.iter().enumerate().skip(1) {
                let (sx, sy) = (x as f64 - c[0] as f64, y as f64 - c[1] as f64);
                if !solid(sx, sy) {
                    continue;
                }
                let (dx, dy) = (-c[0] as f64, -c[1] as f64);
                let (ox, oy) = (x as f64 - cx, y as f64 - cy);
                let a = dx * dx + dy * dy;
                let b = 2.0 * (ox * dx + oy * dy);
                let k = ox * ox + oy * oy - r * r;
                let t = (-b - (b * b - 4.0 * a * k).max(0.0).sqrt()) / (2.0 * a);
                domain.cuts.push((domain.at(x, y, 0), j, t.clamp(0.0, 1.0)));
            }
        }
    }
    let peak = 0.0375;
    let h = height as f64;
    for y in 1..size[1] - 1 {
        let yp = y as f64 - 0.5;
        let u = 4.0 * peak * yp * (h - yp) / (h * h);
        let (inlet, outlet) = (domain.at(0, y, 0), domain.at(size[0] - 1, y, 0));
        domain.flags[inlet] = INLET;
        domain.flags[outlet] = OUTLET;
        domain
            .inflow
            .push((inlet, domain.at(1, y, 0), [u, 0.0, 0.0]));
        domain.outflow.push((outlet, domain.at(size[0] - 2, y, 0)));
    }
    let mean = 2.0 / 3.0 * peak;
    let settings = Settings::laminar(mean * d / reynolds);
    let mut lattice = Lattice::new(&gpu, &domain).expect("lattice");
    lattice.run(&settings, steps).expect("runs");
    let scale = 0.5 * mean * mean * d;
    let mut history = Vec::new();
    for _ in 0..samples {
        lattice.run(&settings, every).expect("runs");
        let force = lattice.forces(1).expect("forces")[0];
        history.push((force[0] / scale, force[1] / scale));
    }
    let last = lattice.forces(1).expect("forces")[0];
    let fields = lattice.fields(settings.force).expect("fields");
    let row = cy as usize;
    let p = |x: usize| fields.density[domain.at(x, row, 0)] / 3.0;
    let (front, back) = ((cx - r) as usize, (cx + r) as usize + 1);
    let pressure =
        (p(front) + 0.5 * (p(front) - p(front - 1))) - (p(back) - 0.5 * (p(back + 1) - p(back)));
    Channel {
        drag: last[0] / scale,
        lift: last[1] / scale,
        pressure: pressure / (mean * mean),
        history,
        period: every as f64 * mean / d,
    }
}

struct Channel {
    drag: f64,
    lift: f64,
    pressure: f64,
    history: Vec<(f64, f64)>,
    period: f64,
}

#[test]
fn cylinder_in_a_channel_re20() {
    let Channel {
        drag,
        lift,
        pressure,
        ..
    } = schaefer_turek(40, 20.0, 240_000, 0, 1);
    assert!(
        (pressure - 2.9375).abs() / 2.9375 < 0.01,
        "pressure difference {pressure} against 2.9375"
    );
    assert!(
        (drag - 5.5795).abs() / 5.5795 < 0.01,
        "drag {drag} against 5.5795"
    );
    assert!((lift - 0.0106).abs() < 0.01, "lift {lift} against 0.0106");
}

#[test]
fn cylinder_in_a_channel_re100() {
    let run = schaefer_turek(40, 100.0, 300_000, 1500, 40);
    let drag = run.history.iter().map(|h| h.0).fold(f64::MIN, f64::max);
    let lift = run.history.iter().map(|h| h.1).fold(f64::MIN, f64::max);
    let ups: Vec<usize> = run
        .history
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0].1 < 0.0 && w[1].1 >= 0.0)
        .map(|(k, _)| k)
        .collect();
    let cycles = (ups.len() - 1) as f64;
    let strouhal = cycles / ((ups[ups.len() - 1] - ups[0]) as f64 * run.period);
    eprintln!("drag max {drag:.4} lift max {lift:.4} strouhal {strouhal:.4}");
    assert!(
        (drag - 3.23).abs() < 0.07,
        "peak drag {drag} against 3.22 to 3.24"
    );
    assert!(
        (lift - 1.0).abs() < 0.06,
        "peak lift {lift} against 0.99 to 1.01"
    );
    assert!(
        (strouhal - 0.3).abs() < 0.008,
        "Strouhal {strouhal} against 0.295 to 0.305"
    );
}

fn solid_from(name: &str, source: &str) -> monstertruck::modeling::Solid {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("flow");
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join(format!("{name}.gcad"));
    std::fs::write(&path, source).expect("write");
    let run = gcad::model::run_path(&path, &[], None).expect("runs");
    run.model.solid.clone().expect("a solid")
}

fn sphere_drag(gpu: &Gpu, d: f64) -> f64 {
    use gcad::flow::voxel::{Grid, place};
    use monstertruck::modeling::Point3;
    let scale = (d / 20.0) as usize;
    let sphere = solid_from(
        "sphere",
        &format!(
            "plane XY\npen 0,{}\narc 0,{} via={},0\nclose\nball: revolve 360 axis=y\n",
            -d / 2.0,
            d / 2.0,
            d / 2.0
        ),
    );
    let size = [(15.0 * d) as usize, (8.0 * d) as usize, (8.0 * d) as usize];
    let grid = Grid {
        origin: Point3::new(-4.0 * d, -(size[1] as f64) / 2.0, -(size[2] as f64) / 2.0),
        spacing: 1.0,
        size,
    };
    let mut domain = Domain::new(size);
    let solid_cells = place(&mut domain, &grid, &sphere, 1);
    let ball = std::f64::consts::PI / 6.0 * d.powi(3);
    assert!(
        (solid_cells as f64 - ball).abs() / ball < 0.05,
        "{solid_cells} cells against {ball:.0}"
    );
    let speed = 0.04;
    for y in 0..size[1] {
        for z in 0..size[2] {
            let (inlet, outlet) = (domain.at(0, y, z), domain.at(size[0] - 1, y, z));
            domain.flags[inlet] = INLET;
            domain.flags[outlet] = OUTLET;
            domain
                .inflow
                .push((inlet, domain.at(1, y, z), [speed, 0.0, 0.0]));
            domain.outflow.push((outlet, domain.at(size[0] - 2, y, z)));
        }
    }
    let mut settings = Settings {
        sponge: 30.0,
        stream: [speed, 0.0, 0.0],
        ..Settings::laminar(speed * d / 50.0)
    };
    let mut lattice = Lattice::new(gpu, &domain).expect("lattice");
    for k in 1..=20 {
        settings.inflow = k as f64 / 20.0;
        lattice.run(&settings, 300 * scale).expect("runs");
    }
    let area = std::f64::consts::PI * d * d / 4.0;
    lattice.run(&settings, 20_000 * scale).expect("runs");
    let mut drags = Vec::new();
    for _ in 0..20 {
        lattice.run(&settings, 500 * scale).expect("runs");
        drags.push(lattice.forces(1).expect("forces")[0][0] / (0.5 * speed * speed * area));
    }
    drags.iter().sum::<f64>() / drags.len() as f64
}

#[test]
fn sphere_drag_re50() {
    let (_device, gpu) = device();
    let drag = sphere_drag(&gpu, 20.0);
    assert!(
        (drag - 1.606).abs() / 1.606 < 0.025,
        "drag {drag} against OpenFOAM's 1.606 in the same box"
    );
}

#[test]
#[ignore = "61 million cells, about 10 GB of GPU memory and ten minutes"]
fn sphere_drag_re50_fine() {
    let (_device, gpu) = device();
    let drag = sphere_drag(&gpu, 40.0);
    eprintln!("sphere drag at 40 cells across {drag:.4}");
    assert!(
        (drag - 1.606).abs() / 1.606 < 0.015,
        "drag {drag} against OpenFOAM's 1.606 in the same box"
    );
}

#[test]
fn pipe_flow_with_curved_walls() {
    let (_device, gpu) = device();
    let radius = 10.3;
    let n = 26;
    let size = [4, n, n];
    let mut domain = Domain::new(size);
    let centre = (n as f64 - 1.0) / 2.0;
    let outside = |y: f64, z: f64| (y - centre).powi(2) + (z - centre).powi(2) > radius * radius;
    for x in 0..size[0] {
        for y in 0..n {
            for z in 0..n {
                if outside(y as f64, z as f64) {
                    let cell = domain.at(x, y, z);
                    domain.flags[cell] = WALL;
                    domain.body[cell] = 1;
                }
            }
        }
    }
    for x in 0..size[0] {
        for y in 0..n {
            for z in 0..n {
                let cell = domain.at(x, y, z);
                if domain.flags[cell] != 0 {
                    continue;
                }
                for (j, c) in C.iter().enumerate().skip(1) {
                    let (sy, sz) = (y as f64 - c[1] as f64, z as f64 - c[2] as f64);
                    if !outside(sy, sz) {
                        continue;
                    }
                    let (oy, oz) = (y as f64 - centre, z as f64 - centre);
                    let (dy, dz) = (-c[1] as f64, -c[2] as f64);
                    let a = dy * dy + dz * dz;
                    let b = 2.0 * (oy * dy + oz * dz);
                    let k = oy * oy + oz * oz - radius * radius;
                    let t = (-b + (b * b - 4.0 * a * k).max(0.0).sqrt()) / (2.0 * a);
                    domain.cuts.push((cell, j, t.clamp(0.0, 1.0)));
                }
            }
        }
    }
    let settings = Settings {
        force: [1.0e-6, 0.0, 0.0],
        ..Settings::laminar(1.0 / 30.0)
    };
    let mut lattice = Lattice::new(&gpu, &domain).expect("lattice");
    lattice.run(&settings, 40_000).expect("runs");
    let fields = lattice.fields(settings.force).expect("fields");
    let mut flow = 0.0;
    for y in 0..n {
        for z in 0..n {
            if !outside(y as f64, z as f64) {
                flow += fields.velocity[domain.at(1, y, z)][0];
            }
        }
    }
    let exact =
        std::f64::consts::PI * settings.force[0] * radius.powi(4) / (8.0 * settings.viscosity);
    assert!(
        (flow - exact).abs() / exact < 0.005,
        "{flow} against {exact}"
    );
}

struct Bluff {
    drag: f64,
    strouhal: f64,
    lift: f64,
}

fn square_cylinder(gpu: &Gpu, d: usize, reynolds: f64, smagorinsky: f64) -> Bluff {
    let size = [22 * d, 14 * d, 4 * d];
    let mut domain = Domain::new(size);
    let (front, low) = (6 * d, 7 * d - d / 2);
    for x in front..front + d {
        for y in low..low + d {
            for z in 0..size[2] {
                let cell = domain.at(x, y, z);
                domain.flags[cell] = WALL;
                domain.body[cell] = 1;
            }
        }
    }
    let speed = 0.04;
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut noise = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    for y in 0..size[1] {
        for z in 0..size[2] {
            let (inlet, outlet) = (domain.at(0, y, z), domain.at(size[0] - 1, y, z));
            domain.flags[inlet] = INLET;
            domain.flags[outlet] = OUTLET;
            let stir = 0.02 * speed;
            let velocity = [
                speed * (1.0 + 0.02 * noise()),
                stir * noise(),
                stir * noise(),
            ];
            domain.inflow.push((inlet, domain.at(1, y, z), velocity));
            domain.outflow.push((outlet, domain.at(size[0] - 2, y, z)));
        }
    }
    let mut settings = Settings {
        sponge: 2.0 * d as f64,
        stream: [speed, 0.0, 0.0],
        smagorinsky,
        bulk: 0.1,
        ..Settings::laminar(speed * d as f64 / reynolds)
    };
    let mut lattice = Lattice::new(gpu, &domain).expect("lattice");
    let period = (d as f64 / (0.13 * speed)) as usize;
    for k in 1..=20 {
        settings.inflow = k as f64 / 20.0;
        lattice.run(&settings, period / 20).expect("runs");
    }
    lattice.run(&settings, 8 * period).expect("runs");
    let every = 50;
    let norm = 0.5 * speed * speed * (d * size[2]) as f64;
    let mut history = Vec::new();
    for _ in 0..20 * period / every {
        lattice.run(&settings, every).expect("runs");
        let force = lattice.forces(1).expect("forces")[0];
        history.push((force[0] / norm, force[1] / norm));
    }
    let count = history.len() as f64;
    let drag = history.iter().map(|h| h.0).sum::<f64>() / count;
    let mean_lift = history.iter().map(|h| h.1).sum::<f64>() / count;
    let lift = (history
        .iter()
        .map(|h| (h.1 - mean_lift).powi(2))
        .sum::<f64>()
        / count)
        .sqrt();
    let band = 0.5 * lift;
    let mut below = false;
    let mut ups = Vec::new();
    for (k, h) in history.iter().enumerate() {
        if h.1 < mean_lift - band {
            below = true;
        } else if below && h.1 > mean_lift + band {
            ups.push(k);
            below = false;
        }
    }
    let strouhal = (ups.len() - 1) as f64 * d as f64
        / ((ups[ups.len() - 1] - ups[0]) as f64 * every as f64 * speed);
    Bluff {
        drag,
        strouhal,
        lift,
    }
}

#[test]
#[ignore = "17 million cells for 150,000 steps, about five minutes"]
fn square_cylinder_re22000() {
    let (_device, gpu) = device();
    let run = square_cylinder(&gpu, 24, 22_000.0, 0.14);
    eprintln!(
        "square cylinder drag {:.3} strouhal {:.4} lift rms {:.3}",
        run.drag, run.strouhal, run.lift
    );
    assert!(
        (run.drag - 2.1).abs() < 0.15,
        "drag {} against Lyn's 2.1",
        run.drag
    );
    assert!(
        (run.strouhal - 0.132).abs() < 0.02,
        "Strouhal {} against Lyn's 0.132",
        run.strouhal
    );
}
