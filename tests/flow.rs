use gcad::flow::gpu::Gpu;
use gcad::flow::lattice::{Domain, INLET, Lattice, OUTLET, Settings};

static DEVICE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn device() -> (std::sync::MutexGuard<'static, ()>, Gpu) {
    let guard = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
    (guard, gcad::flow::gpu::open().expect("a compute device"))
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
#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "GPU, run with --features gpu-tests"
)]
fn sphere_drag_re50() {
    let (_device, gpu) = device();
    let drag = sphere_drag(&gpu, 20.0);
    eprintln!("sphere drag at 20 cells across {drag:.4}");
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
