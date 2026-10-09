mod common;

use common::scratch;
use gcad::sim::Outcome;
use std::path::PathBuf;

const BAR: &str = "rect 100 10 at=50,0\nextrude 10 both\nmaterial aluminium\n";
const BASE: &str = "rect 20 20 at=0,-30\nextrude 10 both\nmaterial steel\n";
const PENDULUM: &str = "part base base.gcad\npart bar bar.gcad\naxis pivot 0,0,0 0,1,0\njoint swing bar base turn about=pivot min=-90 max=90\n";
const G: f64 = 9.81;

fn written(test: &str, files: &[(&str, &str)], spec: &str) -> PathBuf {
    let dir = PathBuf::from(scratch(test));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    for (name, text) in files {
        std::fs::write(dir.join(name), text).expect("write part");
    }
    let path = dir.join("test.gsim");
    std::fs::write(&path, spec).expect("write spec");
    path
}

fn spec_of(path: &std::path::Path) -> gcad::model::Model {
    let run = gcad::model::run_path(path, &[], None).expect("reads");
    if let Some((line, Err(error))) = run.steps.last() {
        panic!("line {} `{}`: {error:#}", line.number, line.text);
    }
    run.model
}

fn simulated(test: &str, files: &[(&str, &str)], spec: &str) -> Outcome {
    let model = spec_of(&written(test, files, spec));
    gcad::sim::simulate(model.sim.as_deref().expect("a simulation")).expect("simulates")
}

fn pendulum_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("bar.gcad", BAR),
        ("base.gcad", BASE),
        ("arm.gasm", PENDULUM),
    ]
}

fn reading(outcome: &Outcome, name: &str) -> f64 {
    outcome
        .reading(name)
        .unwrap_or_else(|| panic!("no reading `{name}` in {:?}", outcome.readings))
        .value
}

fn close(actual: f64, expected: f64, relative: f64) {
    assert!(
        (actual - expected).abs() <= expected.abs() * relative + 1.0e-12,
        "{actual} is not {expected} within {}%",
        relative * 100.0
    );
}

fn bar_kg() -> f64 {
    100.0 * 10.0 * 10.0 * 2.7 / 1.0e6
}

#[test]
fn torque_of_a_pendulum() {
    let outcome = simulated(
        "torque_of_a_pendulum",
        &pendulum_files(),
        "sim torque\npart arm.gasm\npayload bar 100 at=100,0,0\nsweep swing steps=6",
    );
    let full = G * (bar_kg() * 0.05 + 0.1 * 0.1);
    close(reading(&outcome, "swing"), full, 1.0e-4);
    let series = &outcome.series[0];
    for (angle, torque) in series.x.iter().zip(&series.curves[0].values) {
        close(*torque, -full * angle.to_radians().cos(), 1.0e-4);
    }
}

#[test]
fn payload_follows_the_pose() {
    let outcome = simulated(
        "payload_follows_the_pose",
        &pendulum_files(),
        "sim torque\npart arm.gasm\npayload bar 100 at=100,0,0\npose swing 60",
    );
    let full = G * (bar_kg() * 0.05 + 0.1 * 0.1);
    close(reading(&outcome, "swing"), full / 2.0, 1.0e-4);
}

const BLOCK: &str = "rect 20 20\nextrude 20\nmaterial steel\n";
const PINION: &str = "circle 20\nextrude 5\n";
const LIFT: &str = "part base base.gcad\npart block block.gcad\npart pinion pinion.gcad\nmove pinion 40,0,0\naxis spin 40,0,0 40,0,1\njoint lift block base slide along=0,0,1 min=0 max=50\njoint drive pinion base turn about=spin\ncouple drive lift ratio=360/(pi*20)\n";

#[test]
fn torque_through_gears() {
    let outcome = simulated(
        "torque_through_gears",
        &[
            ("base.gcad", BASE),
            ("block.gcad", BLOCK),
            ("pinion.gcad", PINION),
            ("lift.gasm", LIFT),
        ],
        "sim torque\npart lift.gasm\nsweep lift steps=5\nexpect lift<10 drive>0.5",
    );
    let force = 20.0 * 20.0 * 20.0 * 7.85 / 1.0e6 * G;
    close(reading(&outcome, "lift"), force, 1.0e-6);
    close(reading(&outcome, "drive"), force * 0.01, 1.0e-6);
    assert_eq!(outcome.reading("lift").map(|r| r.unit.as_str()), Some("N"));
    assert_eq!(
        outcome.reading("drive").map(|r| r.unit.as_str()),
        Some("N·m")
    );
    assert!(
        outcome
            .notes
            .iter()
            .any(|n| n.contains("no material") && n.contains("pinion"))
    );
    assert_eq!(outcome.failed().len(), 1, "{:?}", outcome.checks);
    assert!(outcome.failed()[0].starts_with("drive>0.5"));
}

#[test]
fn torque_needs_joints() {
    let path = written(
        "torque_needs_joints",
        &[("bar.gcad", BAR)],
        "sim torque\npart bar.gcad",
    );
    let model = spec_of(&path);
    let error =
        gcad::sim::simulate(model.sim.as_deref().expect("a simulation")).expect_err("fails");
    assert!(format!("{error}").contains("joints"), "{error}");
}

#[test]
fn result_goes_stale() {
    let path = written(
        "result_goes_stale",
        &pendulum_files(),
        "sim torque\npart arm.gasm\nsweep swing steps=4",
    );
    let outcome = gcad::sim::simulate(spec_of(&path).sim.as_deref().expect("sim")).expect("runs");
    let json = outcome.to_json(&gcad::sim::stamp(&path, &[]));
    std::fs::write(gcad::sim::result_path(&path), json.to_string()).expect("write result");
    assert_eq!(gcad::sim::stale(&path, &[]), None);
    let read = Outcome::read(&gcad::sim::result_path(&path)).expect("reads back");
    close(reading(&read, "swing"), reading(&outcome, "swing"), 1.0e-12);
    std::fs::write(
        path.with_file_name("bar.gcad"),
        BAR.replace("100 10", "120 10"),
    )
    .expect("edit part");
    let why = gcad::sim::stale(&path, &[]).expect("stale");
    assert!(
        why.contains("bar.gcad") && !why.contains("base.gcad"),
        "{why}"
    );
    let chart = path.with_file_name("chart.png");
    gcad::plot::chart(&read, chart.to_str().expect("utf-8")).expect("draws");
    let image = image::open(&chart).expect("png").to_rgb8();
    assert_eq!(image.width(), 1400);
    assert!(image.pixels().any(|p| p.0 == gcad::plot::PALETTE[0]));
}

#[test]
fn material_properties() {
    let model = common::build("rect 10 10\nextrude 10\nmaterial pla");
    let pla = &model.materials["main"];
    assert_eq!(pla.mechanics.young, Some(2.8));
    assert!(pla.mechanics.printed);
    let model = common::build(
        "rect 10 10\nextrude 10\nmaterial delrin density=1.41 E=3.1 nu=0.35 yield=65",
    );
    let own = &model.materials["main"];
    assert_eq!(own.density, 1.41);
    assert_eq!(own.mechanics.young, Some(3.1));
    assert_eq!(own.mechanics.strength, Some(65.0));
    assert_eq!(own.mechanics.conductivity, None);
    let model = common::build("rect 10 10\nextrude 10\nmaterial steel yield=355");
    assert_eq!(model.materials["main"].mechanics.strength, Some(355.0));
    assert_eq!(model.materials["main"].mechanics.young, Some(200.0));
    assert!(common::failure("rect 10 10\nextrude 10\nmaterial steel nu=0.7").contains("nu=0.7"));
}

const BEAM: &str = "rect 200 20 at=100,0\nbeam: extrude 10\nmaterial steel\n";

#[test]
fn bar_in_tension() {
    let outcome = simulated(
        "bar_in_tension",
        &[(
            "bar.gcad",
            "rect 100 10 at=50,0\nbar: extrude 10 both\nmaterial steel\n",
        )],
        "sim static\npart bar.gcad\nslide <X\nslide <Y\nslide <Z\nload >X force=1000,0,0\nmid: probe 50,0,0\nmesh 4",
    );
    close(reading(&outcome, "stress"), 10.0, 1.0e-6);
    close(reading(&outcome, "mid.stress"), 10.0, 1.0e-6);
    close(
        reading(&outcome, "mid.displacement"),
        1000.0 * 50.0 / (100.0 * 200.0e3),
        1.0e-3,
    );
    close(reading(&outcome, "safety"), 25.0, 1.0e-6);
    close(reading(&outcome, "slide1"), 1000.0, 1.0e-6);
    let field = outcome.field.as_ref().expect("a field");
    assert_eq!(field.values.len(), field.displacement.len());
}

#[test]
fn cantilever_stress() {
    let outcome = simulated(
        "cantilever_stress",
        &[("beam.gcad", BEAM)],
        "sim static\npart beam.gcad\nroot: fix <X\nload >X force=0,0,-100\ntop: probe 100,0,10\nmesh 4",
    );
    let inertia = 20.0 * 10.0_f64.powi(3) / 12.0;
    let tip = 100.0 * 200.0_f64.powi(3) / (3.0 * 200.0e3 * inertia);
    close(reading(&outcome, "displacement"), tip, 0.02);
    close(
        reading(&outcome, "top.stress"),
        100.0 * 100.0 * 5.0 / inertia,
        0.01,
    );
    close(reading(&outcome, "root"), 100.0, 1.0e-6);
}

#[test]
fn thick_cylinder_under_pressure() {
    let ring = "circle 80\nbase: extrude 10\nplane base.end\nh: hole 40 0,0\nrect 100 100 at=-50,0\ncut thru\nrect 100 100 at=50,-50\ncut thru\nmaterial steel\n";
    let outcome = simulated(
        "thick_cylinder_under_pressure",
        &[("ring.gcad", ring)],
        "sim static\npart ring.gcad\nslide <X\nslide <Y\nslide <Z\npressure h.side 10\nmid: probe 21.2132,21.2132,5\ninner: probe 14.1421,14.1421,5\nmesh 3",
    );
    let (a, b, p) = (20.0_f64, 40.0_f64, 10.0);
    let lame = |r: f64| {
        let k = p * a * a / (b * b - a * a);
        let (hoop, radial) = (k * (1.0 + b * b / (r * r)), k * (1.0 - b * b / (r * r)));
        (hoop * hoop + radial * radial - hoop * radial).sqrt()
    };
    close(reading(&outcome, "mid.stress"), lame(30.0), 0.01);
    close(reading(&outcome, "inner.stress"), lame(20.0), 0.02);
    let hoop = p * (b * b + a * a) / (b * b - a * a);
    close(
        reading(&outcome, "inner.displacement"),
        a / 200.0e3 * (hoop + 0.29 * p),
        0.01,
    );
    close(reading(&outcome, "slide1"), p * a * 10.0, 1.0e-3);
}

#[test]
fn static_needs_supports_and_stiffness() {
    let path = written(
        "static_needs_supports",
        &[("beam.gcad", BEAM)],
        "sim static\npart beam.gcad\nload >X force=0,0,-1",
    );
    let error =
        gcad::sim::simulate(spec_of(&path).sim.as_deref().expect("sim")).expect_err("no supports");
    assert!(format!("{error}").contains("support"), "{error}");
    let path = written(
        "static_needs_stiffness",
        &[(
            "beam.gcad",
            "rect 20 20\nextrude 5\nmaterial stepper density=3\n",
        )],
        "sim static\npart beam.gcad\nfix <X\nload >X force=0,0,-1",
    );
    let error =
        gcad::sim::simulate(spec_of(&path).sim.as_deref().expect("sim")).expect_err("no stiffness");
    assert!(format!("{error}").contains("E="), "{error}");
}

#[test]
fn stress_field_render() {
    let path = written(
        "stress_field_render",
        &[("beam.gcad", BEAM)],
        "sim static\npart beam.gcad\nfix <X\nload >X force=0,0,-100\nmesh 8",
    );
    let model = spec_of(&path);
    let spec = model.sim.as_deref().expect("sim");
    let outcome = gcad::sim::simulate(spec).expect("solves");
    std::fs::write(
        gcad::sim::result_path(&path),
        outcome.to_json(&gcad::sim::stamp(&path, &[])).to_string(),
    )
    .expect("write");
    let picture = path.with_file_name("stress.png");
    gcad::sim::render(
        spec,
        &gcad::sim::result_path(&path),
        picture.to_str().expect("utf-8"),
    )
    .expect("draws");
    let image = image::open(&picture).expect("png").to_rgb8();
    let warm = image
        .pixels()
        .filter(|p| p.0[0] > 120 && p.0[2] < 60)
        .count();
    let cold = image
        .pixels()
        .filter(|p| p.0[2] > 120 && p.0[0] < 60)
        .count();
    assert!(warm > 100 && cold > 100, "{warm} warm, {cold} cold pixels");
}

#[test]
fn conduction_through_a_wall() {
    let outcome = simulated(
        "conduction_through_a_wall",
        &[(
            "wall.gcad",
            "rect 100 100\nwall: extrude 10\nmaterial aluminium\n",
        )],
        "sim thermal\npart wall.gcad\nhot: temperature wall.start 100\ncold: temperature wall.end 20\nmesh 5",
    );
    close(
        reading(&outcome, "cold"),
        167.0 * 0.01 * 80.0 / 0.01,
        1.0e-6,
    );
    close(
        reading(&outcome, "hot"),
        -167.0 * 0.01 * 80.0 / 0.01,
        1.0e-6,
    );
}

#[test]
fn fin_with_convection() {
    let outcome = simulated(
        "fin_with_convection",
        &[(
            "fin.gcad",
            "rect 100 10 at=50,0\nfin: extrude 10 both\nmaterial aluminium\n",
        )],
        "sim thermal\npart fin.gcad\nbase: temperature <X 100\nair: convect fin.start,fin.end,>X,>Y,<Y h=20 ambient=20\ntip: probe 100,0,0\nmesh 2.5",
    );
    let (h, perimeter, k, area, length): (f64, f64, f64, f64, f64) =
        (2.0e-5, 40.0, 0.167, 100.0, 100.0);
    let m = (h * perimeter / (k * area)).sqrt();
    let r = h / (m * k);
    let (c, s) = ((m * length).cosh(), (m * length).sinh());
    let heat = (h * perimeter * k * area).sqrt() * 80.0 * (s + r * c) / (c + r * s);
    close(reading(&outcome, "air"), heat, 0.005);
    close(reading(&outcome, "base"), -heat, 0.005);
    close(
        reading(&outcome, "tip.temperature"),
        20.0 + 80.0 / (c + r * s),
        0.002,
    );
}

#[test]
fn heat_needs_a_boundary() {
    let path = written(
        "heat_needs_a_boundary",
        &[("beam.gcad", BEAM)],
        "sim thermal\npart beam.gcad\nsource 5",
    );
    let error =
        gcad::sim::simulate(spec_of(&path).sim.as_deref().expect("sim")).expect_err("floats");
    assert!(format!("{error}").contains("temperature"), "{error}");
}

#[test]
fn cantilever_modes() {
    let outcome = simulated(
        "cantilever_modes",
        &[("beam.gcad", BEAM)],
        "sim modal\npart beam.gcad\nfix <X\nmodes 3\nmesh 2",
    );
    let stiffness = 200.0e9 * 0.02 * 0.01_f64.powi(3) / 12.0;
    let line_mass = 7850.0 * 0.02 * 0.01;
    let first = 1.875_f64.powi(2) / (2.0 * std::f64::consts::PI)
        * (stiffness / (line_mass * 0.2_f64.powi(4))).sqrt();
    close(reading(&outcome, "mode1"), first, 0.01);
    close(reading(&outcome, "mode2"), first * 2.0, 0.02);
    let at = &outcome.reading("mode1").expect("mode1").at;
    assert!(at.contains("z 61%"), "{at}");
}

#[test]
fn free_beam_modes() {
    let outcome = simulated(
        "free_beam_modes",
        &[("beam.gcad", BEAM)],
        "sim modal\npart beam.gcad\nmodes 2\nmesh 4",
    );
    let stiffness = 200.0e9 * 0.02 * 0.01_f64.powi(3) / 12.0;
    let line_mass = 7850.0 * 0.02 * 0.01;
    let first = 4.730_f64.powi(2) / (2.0 * std::f64::consts::PI)
        * (stiffness / (line_mass * 0.2_f64.powi(4))).sqrt();
    close(reading(&outcome, "mode1"), first, 0.02);
    assert!(outcome.notes.iter().any(|n| n.contains("rigid")));
}

#[test]
fn robot_arm_motor_torque() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/robot/arm.gsim");
    let outcome = gcad::sim::simulate(spec_of(&path).sim.as_deref().expect("sim")).expect("runs");
    assert!(outcome.failed().is_empty(), "{}", outcome.table());
    let drive = 1.0 + 54.0 / 18.0;
    for (joint, motor) in [
        ("shoulder", "sun_s_turn"),
        ("elbow", "sun_e_turn"),
        ("wrist", "sun_w_turn"),
    ] {
        close(
            reading(&outcome, motor),
            reading(&outcome, joint) / drive,
            1.0e-9,
        );
    }
    close(
        reading(&outcome, "planet_s_turn_3"),
        reading(&outcome, "planet_s_turn"),
        0.0,
    );
    assert!(reading(&outcome, "yaw").abs() < 1.0e-9);
    assert_eq!(outcome.series.len(), 2);
}

fn solid_of(source: &str) -> monstertruck::modeling::Solid {
    common::build(source).solid.expect("a solid")
}

fn checked_mesh(solid: &monstertruck::modeling::Solid, size: f64) -> gcad::sim::tet::TetMesh {
    use std::collections::HashMap;
    let mesh = gcad::sim::tet::mesh_solid(solid, size).expect("meshes");
    let mut faces: HashMap<[u32; 3], usize> = HashMap::new();
    let mut volume = 0.0;
    for t in 0..mesh.tets.len() {
        let q = mesh.points(t);
        let six = robust::orient3d(
            robust::Coord3D {
                x: q[0][0],
                y: q[0][1],
                z: q[0][2],
            },
            robust::Coord3D {
                x: q[1][0],
                y: q[1][1],
                z: q[1][2],
            },
            robust::Coord3D {
                x: q[2][0],
                y: q[2][1],
                z: q[2][2],
            },
            robust::Coord3D {
                x: q[3][0],
                y: q[3][1],
                z: q[3][2],
            },
        );
        assert!(six > 0.0, "tetrahedron {t} is flat or inverted");
        volume += six / 6.0;
        let tet = mesh.tets[t];
        for i in 0..4 {
            let mut f: Vec<u32> = (0..4).filter(|&k| k != i).map(|k| tet[k]).collect();
            f.sort_unstable();
            *faces.entry([f[0], f[1], f[2]]).or_default() += 1;
        }
    }
    let skin: std::collections::HashSet<[u32; 3]> = mesh
        .boundary
        .iter()
        .map(|(f, _)| {
            let mut k = *f;
            k.sort_unstable();
            k
        })
        .collect();
    assert_eq!(
        skin.len(),
        mesh.boundary.len(),
        "a boundary triangle is listed twice"
    );
    for (face, count) in &faces {
        match (count, skin.contains(face)) {
            (1, true) | (2, false) => {}
            other => panic!("face {face:?} is shared {other:?}"),
        }
    }
    let mut edges: HashMap<[u32; 2], usize> = HashMap::new();
    let mut enclosed = 0.0;
    for (f, tag) in &mesh.boundary {
        assert!(*tag < mesh.faces);
        for k in 0..3 {
            let (a, b) = (f[k].min(f[(k + 1) % 3]), f[k].max(f[(k + 1) % 3]));
            *edges.entry([a, b]).or_default() += 1;
        }
        let [a, b, c] = f.map(|v| mesh.nodes[v as usize]);
        enclosed += a.x * (b.y * c.z - b.z * c.y) - a.y * (b.x * c.z - b.z * c.x)
            + a.z * (b.x * c.y - b.y * c.x);
    }
    let open: Vec<_> = edges.iter().filter(|(_, n)| **n != 2).collect();
    assert!(
        open.is_empty(),
        "the boundary is not closed: {:?}",
        &open[..open.len().min(4)]
    );
    close(enclosed / 6.0, volume, 1.0e-9);
    let tagged: std::collections::HashSet<usize> = mesh.boundary.iter().map(|(_, t)| *t).collect();
    assert_eq!(
        tagged.len(),
        mesh.faces,
        "every face of the solid has boundary triangles"
    );
    mesh
}

#[test]
fn tet_mesh_of_a_box() {
    let mesh = checked_mesh(&solid_of("rect 40 30\nextrude 10"), 4.0);
    let q = mesh.quality();
    close(q.volume, 12000.0, 1.0e-9);
    assert!(q.min_dihedral > 5.0 && q.max_ratio < 3.0, "{q:?}");
}

#[test]
fn tet_mesh_of_curved_parts() {
    let pi = std::f64::consts::PI;
    for (source, size, volume) in [
        (
            "rect 40 30\nbase: extrude 10\nplane base.end\nhole 8 0,0",
            3.0,
            12000.0 - pi * 16.0 * 10.0,
        ),
        ("circle 20\nextrude 30", 3.0, pi * 100.0 * 30.0),
        ("rect 20 20\nextrude 10\nfillet 3 all", 2.0, 0.0),
    ] {
        let solid = solid_of(source);
        let mesh = checked_mesh(&solid, size);
        let q = mesh.quality();
        let exact = if volume > 0.0 {
            volume
        } else {
            gcad::geometry::volume(&solid)
        };
        close(q.volume, exact, 0.02);
        assert!(q.min_dihedral > 4.0, "{source}: {q:?}");
    }
}

#[test]
fn tet_mesh_of_example_parts() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/parts");
    for (name, size) in [
        ("bracket", 3.0),
        ("plate", 2.5),
        ("ring", 1.2),
        ("frame", 3.0),
        ("pipe", 1.5),
    ] {
        let run =
            gcad::model::run_path(&root.join(format!("{name}.gcad")), &[], None).expect("runs");
        let solid = run.model.solid.clone().expect("a solid");
        let mesh = checked_mesh(&solid, size);
        let q = mesh.quality();
        close(q.volume, gcad::geometry::volume(&solid), 0.04);
        assert!(q.min_dihedral > 4.0, "{name}: {q:?}");
    }
}

#[test]
fn tet_mesh_of_thin_walls() {
    let solid = solid_of("rect 60 40 r=4\nbox: extrude 25\nshell 2 open=box.end");
    let mesh = checked_mesh(&solid, 5.0);
    close(mesh.quality().volume, gcad::geometry::volume(&solid), 0.01);
}

#[test]
fn mesh_dry_run() {
    let path = written(
        "mesh_dry_run",
        &[("beam.gcad", BEAM)],
        "sim modal\npart beam.gcad\nmesh 4",
    );
    let model = spec_of(&path);
    let spec = model.sim.as_deref().expect("sim");
    let plan = gcad::sim::plan(spec).expect("plans");
    assert!(
        plan.contains("mesh size 4") && plan.contains("tetrahedra"),
        "{plan}"
    );
    let picture = path.with_file_name("mesh.png");
    gcad::sim::mesh::render(
        &gcad::sim::mesh::bodies(spec).expect("meshes"),
        picture.to_str().expect("utf-8"),
    )
    .expect("renders");
    assert!(picture.exists());
}
