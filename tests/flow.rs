use gcad::flow::gpu::Gpu;
use gcad::flow::lattice::{Collision, FLUID, Lattice, MOVING, Settings, WALL};

fn at(size: [usize; 3], x: usize, y: usize, z: usize) -> usize {
    x + (y + z * size[1]) * size[0]
}

#[test]
fn poiseuille_between_plates() {
    let gpu = Gpu::open().expect("a compute device");
    let size = [4, 34, 4];
    let mut flags = vec![FLUID; size.iter().product()];
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
            ..Settings::laminar(1.0 / 6.0)
        };
        let mut lattice = Lattice::new(&gpu, size, &flags).expect("lattice");
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
    let gpu = Gpu::open().expect("a compute device");
    let size = [side + 2, side + 2, 1];
    let mut flags = vec![FLUID; size.iter().product()];
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
    let mut lattice = Lattice::new(&gpu, size, &flags).expect("lattice");
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
    let gpu = Gpu::open().expect("a compute device");
    let size = [128, 128, 128];
    let flags = vec![FLUID; size.iter().product()];
    let settings = Settings {
        force: [1.0e-7, 0.0, 0.0],
        ..Settings::laminar(0.01)
    };
    let mut lattice = Lattice::new(&gpu, size, &flags).expect("lattice");
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
