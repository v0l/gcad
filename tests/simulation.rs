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
fn field_reads_at_a_point() {
    let path = written(
        "field_reads_at_a_point",
        &[(
            "bar.gcad",
            "rect 100 10 at=50,0\nbar: extrude 10 both\nmaterial steel\n",
        )],
        "sim static\npart bar.gcad\nslide <X\nslide <Y\nslide <Z\nload >X force=1000,0,0\nmesh 4",
    );
    let model = spec_of(&path);
    let spec = model.sim.as_deref().expect("sim");
    let outcome = gcad::sim::simulate(spec).expect("solves");
    let view = gcad::sim::statics::field_view(spec, &outcome).expect("a field");
    let (v, _) = view
        .triangles
        .iter()
        .flatten()
        .map(|&v| (v as usize, (view.rest[v as usize].x - 50.0).abs()))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("surface nodes");
    let probe = view.probe(view.positions[v]).expect("a reading");
    close(probe.value, 10.0, 1.0e-6);
    close(probe.at.x, view.rest[v].x, 1.0e-9);
    close(
        probe.moved.x,
        1000.0 * view.rest[v].x / (100.0 * 200.0e3),
        1.0e-6,
    );
    let texts: Vec<&str> = view.marks.iter().map(|m| m.text.as_str()).collect();
    assert!(texts.contains(&">X: 1000.0 N (1000.0, 0, 0)"), "{texts:?}");
    assert!(
        texts.contains(&"slide1: slides on its face, holds 1000.0 N"),
        "{texts:?}"
    );
    let push = view
        .marks
        .iter()
        .find(|m| m.text.starts_with(">X"))
        .expect("load mark");
    close(push.toward.expect("a direction").x, 1.0, 1.0e-12);
    assert!(
        outcome
            .notes
            .iter()
            .any(|n| n == "applied >X: 1000.0 N (1000.0, 0, 0)"),
        "{:?}",
        outcome.notes
    );
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
    let view = gcad::sim::current_field(spec).expect("an up to date field");
    assert!(!view.triangles.is_empty());
    assert!(view.shade.iter().all(|s| (0.0..=1.0).contains(s)));
    std::fs::write(
        path.with_file_name("beam.gcad"),
        BEAM.replace("200 20", "210 20"),
    )
    .expect("edit");
    assert!(
        gcad::sim::current_field(spec).is_none(),
        "a stale result is not shown"
    );
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
fn large_heat_problem_solves_iteratively() {
    let outcome = simulated(
        "large_heat_problem_solves_iteratively",
        &[(
            "fin.gcad",
            "rect 100 10 at=50,0\nfin: extrude 10 both\nmaterial aluminium\n",
        )],
        "sim thermal\npart fin.gcad\nbase: temperature <X 100\nair: convect fin.start,fin.end,>X,>Y,<Y h=20 ambient=20\ntip: probe 100,0,0\nmesh 1",
    );
    assert!(
        outcome
            .notes
            .iter()
            .any(|n| n.contains("solved iteratively")),
        "{}",
        outcome.table()
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
fn large_eigenproblems_iterate() {
    let free = simulated(
        "large_free_modes",
        &[("beam.gcad", BEAM)],
        "sim modal\npart beam.gcad\nmodes 2\nmesh 2",
    );
    let stiffness = 200.0e9 * 0.02 * 0.01_f64.powi(3) / 12.0;
    let line_mass = 7850.0 * 0.02 * 0.01;
    let first = 4.730_f64.powi(2) / (2.0 * std::f64::consts::PI)
        * (stiffness / (line_mass * 0.2_f64.powi(4))).sqrt();
    close(reading(&free, "mode1"), first, 0.02);
    assert!(
        free.notes
            .iter()
            .any(|n| n.contains("eigenvalue iterations"))
    );
    let column = simulated(
        "large_buckling",
        &[("col.gcad", "rect 10 10\ncol: extrude 100\nmaterial steel\n")],
        "sim buckling\npart col.gcad\nfix <Z\nload >Z force=0,0,-1000\nmesh 1.2",
    );
    let euler = std::f64::consts::PI.powi(2) * 200.0e3 * (10.0_f64.powi(4) / 12.0)
        / (4.0 * 100.0_f64.powi(2));
    close(reading(&column, "buckling"), euler / 1000.0, 0.01);
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
fn tet_mesh_of_a_ring_gear_part() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/robot/turret.gcad");
    let run = gcad::model::run_path(&path, &[], None).expect("runs");
    let solid = run.model.solid.clone().expect("a solid");
    let size = gcad::geometry::bounds(&solid).diameter() / 20.0;
    let mesh = checked_mesh(&solid, size);
    let q = mesh.quality();
    close(q.volume, gcad::geometry::volume(&solid), 0.01);
    assert!(mesh.quadratic_nodes() < 1_000_000, "{q:?}");
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

const SLAB: &str = "rect 20 20\nblk: extrude 10\nmaterial steel\n";

#[test]
fn large_bonded_problem_converges() {
    let stack = "part a block.gcad\npart b block.gcad\nmove b 0,0,10\n";
    let outcome = simulated(
        "large_bonded_problem_converges",
        &[("block.gcad", SLAB), ("stack.gasm", stack)],
        "sim static\npart stack.gasm\nslide a:<Z\nslide a:<X\nslide a:<Y\nslide b:<X\nslide b:<Y\nload b:>Z force=0,0,1000\ntop: probe 0,0,20\nmesh 1.2",
    );
    let axial: f64 = 1000.0 * 20.0 / (400.0 * 200.0e3);
    let lateral = 0.29 * 2.5 / 200.0e3 * 10.0;
    close(
        reading(&outcome, "top.displacement"),
        (axial * axial + 2.0 * lateral * lateral).sqrt(),
        0.005,
    );
    let iterations: usize = outcome
        .notes
        .iter()
        .find_map(|n| {
            n.split("the solver took ")
                .nth(1)?
                .split(' ')
                .next()?
                .parse()
                .ok()
        })
        .expect("an iterative solve");
    assert!(iterations < 120, "{iterations} iterations");
}

#[test]
fn mesh_convergence_check() {
    let ell = "pen 0,0\nline 40,0\nline 40,10\nline 10,10\nline 10,40\nline 0,40\nclose\nell: extrude 10\nmaterial steel\n";
    let corner = simulated(
        "mesh_convergence_corner",
        &[("ell.gcad", ell)],
        "sim static\npart ell.gcad\nfix <X\nload >X force=0,-500,0\nmesh 3\nconverge within=2",
    );
    let note = corner
        .notes
        .iter()
        .find(|n| n.starts_with("mesh check"))
        .expect("a mesh check note");
    assert!(
        note.contains("`stress` moved") && note.contains("not settled"),
        "{note}"
    );
    let bar = simulated(
        "mesh_convergence_bar",
        &[(
            "bar.gcad",
            "rect 100 10 at=50,0\nbar: extrude 10 both\nmaterial steel\n",
        )],
        "sim static\npart bar.gcad\nslide <X\nslide <Y\nslide <Z\nload >X force=1000,0,0\nmid: probe 50,0,0\nmesh 4\nconverge",
    );
    assert!(
        bar.notes
            .iter()
            .any(|n| n.contains("every reading is within 5%")),
        "{}",
        bar.table()
    );
}

#[test]
fn friction_holds_until_it_slides() {
    let stack = "part a block.gcad\npart b block.gcad\nmove b 0,0,10\n";
    let run = |name: &str, push: f64, extra: &str| {
        written(
            name,
            &[("block.gcad", SLAB), ("stack.gasm", stack)],
            &format!(
                "sim static\npart stack.gasm\nfix a:<Z\nload b:>Z force={push},0,-1000\n{extra}\nlow: probe 0,0,5\nmesh 4"
            ),
        )
    };
    let bonded = gcad::sim::simulate(
        spec_of(&run("friction_bonded", 270.0, ""))
            .sim
            .as_deref()
            .expect("sim"),
    )
    .expect("bonded solves");
    let held = gcad::sim::simulate(
        spec_of(&run("friction_held", 270.0, "contact a b friction=0.3"))
            .sim
            .as_deref()
            .expect("sim"),
    )
    .expect("friction holds 0.9 of its limit");
    close(
        reading(&held, "low.stress"),
        reading(&bonded, "low.stress"),
        0.02,
    );
    assert!(
        held.notes.iter().any(|n| n.contains("slipping")),
        "{}",
        held.table()
    );
    let error = gcad::sim::simulate(
        spec_of(&run("friction_slides", 330.0, "contact a b friction=0.3"))
            .sim
            .as_deref()
            .expect("sim"),
    )
    .expect_err("friction gives way above its limit");
    assert!(format!("{error}").contains("slide"), "{error}");
}

#[test]
fn contact_vibrates_as_bonded() {
    let stack = "part a block.gcad\npart b block.gcad\nmove b 0,0,10\n";
    let run = |name: &str, extra: &str| {
        simulated(
            name,
            &[("block.gcad", SLAB), ("stack.gasm", stack)],
            &format!("sim modal\npart stack.gasm\nfix a:<Z\nmodes 1\n{extra}\nmesh 4"),
        )
    };
    let bonded = run("modes_bonded", "");
    let touching = run("modes_touching", "contact a b");
    close(
        reading(&touching, "mode1"),
        reading(&bonded, "mode1"),
        1.0e-9,
    );
}

#[test]
fn preload_by_cooling_one_part() {
    let outcome = simulated(
        "preload_by_cooling_one_part",
        &[
            (
                "plate.gcad",
                "rect 40 40\nplate: extrude 20\nmaterial steel\n",
            ),
            ("rod.gcad", "circle 8\nrod: extrude 50\nmaterial steel\n"),
            (
                "tube.gcad",
                "circle 30\ncircle 24\ntube: extrude 50\nmaterial aluminium\n",
            ),
            (
                "joint.gasm",
                "part a plate.gcad\nmove a 0,0,-20\npart rod rod.gcad\npart tube tube.gcad\npart b plate.gcad\nmove b 0,0,50\n",
            ),
        ],
        "sim static\npart joint.gasm\nslide a:<Z\nslide a:<X\nslide a:<Y\nwarm -100 part=rod\nmid: probe 0,0,25\nwall: probe 13.5,0,25\nmesh 3",
    );
    let rod = std::f64::consts::PI * 16.0;
    let tube = std::f64::consts::PI / 4.0 * (900.0 - 576.0);
    let pull = reading(&outcome, "mid.stress") * rod;
    let push = reading(&outcome, "wall.stress") * tube;
    close(pull, push, 0.02);
    let rigid = 12.0e-6 * 100.0 / (1.0 / (200.0e3 * rod) + 1.0 / (69.0e3 * tube));
    close(pull, rigid, 0.1);
    assert!(
        pull < rigid,
        "the plates flex, so the preload falls short of the rigid figure"
    );
}

#[test]
fn floating_part_is_refused() {
    let apart = "part a block.gcad\npart b block.gcad\nmove b 0,0,15\n";
    let path = written(
        "floating_part_is_refused",
        &[("block.gcad", SLAB), ("stack.gasm", apart)],
        "sim static\npart stack.gasm\nfix a:<Z\nload b:>Z force=0,0,-100\nmesh 4",
    );
    let error = gcad::sim::simulate(spec_of(&path).sim.as_deref().expect("sim"))
        .expect_err("b touches nothing");
    let text = format!("{error}");
    assert!(text.contains("`b` floats free"), "{text}");
    let heat = written(
        "floating_part_is_refused_heat",
        &[("block.gcad", SLAB), ("stack.gasm", apart)],
        "sim thermal\npart stack.gasm\ntemperature a:<Z 50\nmesh 4",
    );
    let error = gcad::sim::simulate(spec_of(&heat).sim.as_deref().expect("sim"))
        .expect_err("b touches nothing");
    assert!(format!("{error}").contains("`b` floats free"), "{error}");
}

#[test]
fn bonded_joint_carries_moment() {
    let head = |hole: &str| {
        format!(
            "plane XY offset=87\ncircle 44\nf: extrude 8\nplane f.end\ncircle 24\npost: extrude 127\nplane XY offset=107\n{hole}\nbore: cut 20\nmaterial steel\n"
        )
    };
    for (name, section) in [("square", "rect 13 13"), ("round", "circle 15")] {
        let rod = format!("{section}\nr: extrude 107\nmaterial steel\n");
        let outcome = simulated(
            &format!("bonded_joint_carries_moment_{name}"),
            &[
                ("rod.gcad", rod.as_str()),
                ("head.gcad", head(section).as_str()),
                ("joint.gasm", "part s rod.gcad\npart h head.gcad\n"),
            ],
            "sim static\npart joint.gasm\nmesh 4\nroot: fix s:r.start\nload h:post.end force=0,100,0",
        );
        close(reading(&outcome, "root.moment"), 22.2, 1.0e-3);
    }
}

#[test]
fn leave_parts_out() {
    let stack = "part a block.gcad\npart b block.gcad\nmove b 0,0,10\npart loose_1 block.gcad\nmove loose_1 50,0,0\n";
    let outcome = simulated(
        "leave_parts_out",
        &[("block.gcad", SLAB), ("stack.gasm", stack)],
        "sim static\npart stack.gasm\nleave loose*\nslide a:<Z\nslide a:<X\nslide a:<Y\nslide b:<X\nslide b:<Y\nload b:>Z force=0,0,1000\nmesh 4",
    );
    close(reading(&outcome, "stress"), 2.5, 0.05);
    let path = written(
        "leave_parts_out_bad",
        &[("block.gcad", SLAB), ("stack.gasm", stack)],
        "sim static\npart stack.gasm\nleave nothing*\nfix a:<Z\nmesh 4",
    );
    let run = gcad::model::run_path(&path, &[], None).expect("runs");
    let (_, step) = run.steps.last().expect("a step");
    assert!(
        format!("{:#}", step.as_ref().expect_err("no part matches")).contains("matches no part"),
        "{step:?}"
    );
}

#[test]
fn bonded_parts_carry_load() {
    let stack = "part a block.gcad\npart b block.gcad\nmove b 0,0,10\n";
    let outcome = simulated(
        "bonded_parts_carry_load",
        &[("block.gcad", SLAB), ("stack.gasm", stack)],
        "sim static\npart stack.gasm\nslide a:<Z\nslide a:<X\nslide a:<Y\nslide b:<X\nslide b:<Y\nload b:>Z force=0,0,1000\ntop: probe 0,0,20\nmesh 3",
    );
    let axial: f64 = 1000.0 * 20.0 / (400.0 * 200.0e3);
    let lateral = 0.29 * 2.5 / 200.0e3 * 10.0;
    close(
        reading(&outcome, "top.displacement"),
        (axial * axial + 2.0 * lateral * lateral).sqrt(),
        0.005,
    );
    close(reading(&outcome, "stress"), 2.5, 0.05);
    assert!(outcome.notes.iter().any(|n| n.contains("bonded a to b")));
}

#[test]
fn bonded_parts_conduct_heat() {
    let stack = "part a block.gcad\npart b block.gcad\nmove b 0,0,10\nmaterial b aluminium\n";
    let outcome = simulated(
        "bonded_parts_conduct_heat",
        &[("block.gcad", SLAB), ("stack.gasm", stack)],
        "sim thermal\npart stack.gasm\nhot: temperature a:<Z 100\ncold: temperature b:>Z 20\nmesh 3",
    );
    let resistance = 0.01 / (50.0 * 4.0e-4) + 0.01 / (167.0 * 4.0e-4);
    close(reading(&outcome, "cold"), 80.0 / resistance, 0.005);
}

#[test]
fn bonded_parts_vibrate_as_one() {
    let stack = "part a block.gcad\npart b block.gcad\nmove b 0,0,10\n";
    let joined = simulated(
        "bonded_parts_vibrate_as_one",
        &[("block.gcad", SLAB), ("stack.gasm", stack)],
        "sim modal\npart stack.gasm\nfix a:<Z\nmodes 1\nmesh 3",
    );
    let whole = simulated(
        "solid_block_vibrates",
        &[("tall.gcad", "rect 20 20\nblk: extrude 20\nmaterial steel\n")],
        "sim modal\npart tall.gcad\nfix <Z\nmodes 1\nmesh 3",
    );
    close(reading(&joined, "mode1"), reading(&whole, "mode1"), 0.01);
}

#[test]
fn shaft_in_torsion() {
    let outcome = simulated(
        "shaft_in_torsion",
        &[("rod.gcad", "circle 20\nrod: extrude 100\nmaterial steel\n")],
        "sim static\npart rod.gcad\nbase: fix rod.start\ntorque rod.end 10\nmid: probe 10,0,50\nmesh 3",
    );
    let shear = 16.0 * 10.0e3 / (std::f64::consts::PI * 20.0_f64.powi(3));
    close(
        reading(&outcome, "mid.stress"),
        3.0_f64.sqrt() * shear,
        0.01,
    );
    let modulus = 200.0e3 / (2.0 * 1.29);
    let polar = std::f64::consts::PI * 20.0_f64.powi(4) / 32.0;
    let twist = 10.0e3 * 50.0 / (modulus * polar);
    close(reading(&outcome, "mid.displacement"), twist * 10.0, 0.01);
    close(reading(&outcome, "base.moment"), 10.0, 1.0e-3);
}

#[test]
fn pinned_link() {
    let link = "rect 100 10 at=50,0\nbar: extrude 10\nplane bar.end\na: hole 6 10,0\nb: hole 6 90,0\nmaterial steel\n";
    let outcome = simulated(
        "pinned_link",
        &[("link.gcad", link)],
        "sim static\npart link.gcad\nleft: pin a.side\nright: pin b.side\nload >Y force=0,-500,0\nmesh 2",
    );
    for side in ["left", "right"] {
        let at = &outcome.reading(side).expect("reaction").at;
        assert!(at.contains(",250.0,"), "{side}: {at}");
    }
    let path = written(
        "lone_pin",
        &[("link.gcad", link)],
        "sim static\npart link.gcad\npin a.side\nload >Y force=0,-500,0\nmesh 4",
    );
    let error =
        gcad::sim::simulate(spec_of(&path).sim.as_deref().expect("sim")).expect_err("turns");
    assert!(format!("{error}").contains("turn"), "{error}");
}

#[test]
fn large_problems_iterate() {
    let outcome = simulated(
        "large_problems_iterate",
        &[("beam.gcad", BEAM)],
        "sim static\npart beam.gcad\nfix <X\nload >X force=0,0,-100\ntop: probe 100,0,10\nmesh 2",
    );
    let inertia = 20.0 * 10.0_f64.powi(3) / 12.0;
    close(
        reading(&outcome, "top.stress"),
        100.0 * 100.0 * 5.0 / inertia,
        0.005,
    );
    let note = outcome
        .notes
        .iter()
        .find(|n| n.contains("iterations"))
        .expect("solver note");
    let iterations: usize = note
        .split("took ")
        .nth(1)
        .and_then(|t| t.split(' ').next())
        .and_then(|t| t.parse().ok())
        .expect("a count");
    assert!(iterations > 1 && iterations < 100, "{note}");
}

#[test]
fn example_simulations() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/parts");
    for name in ["bracket", "heatsink"] {
        let path = root.join(format!("{name}.gsim"));
        let outcome =
            gcad::sim::simulate(spec_of(&path).sim.as_deref().expect("sim")).expect("runs");
        assert!(outcome.failed().is_empty(), "{name}: {}", outcome.table());
    }
}

#[test]
fn column_buckling() {
    let outcome = simulated(
        "column_buckling",
        &[("col.gcad", "rect 10 10\ncol: extrude 200\nmaterial steel\n")],
        "sim buckling\npart col.gcad\nfix <Z\nload >Z force=0,0,-1000\nmesh 3",
    );
    let euler = std::f64::consts::PI.powi(2) * 200.0e3 * (10.0_f64.powi(4) / 12.0)
        / (4.0 * 200.0_f64.powi(2));
    close(reading(&outcome, "buckling"), euler / 1000.0, 0.005);
    let pulled = written(
        "column_in_tension",
        &[("col.gcad", "rect 10 10\ncol: extrude 200\nmaterial steel\n")],
        "sim buckling\npart col.gcad\nfix <Z\nload >Z force=0,0,1000\nmesh 6",
    );
    let error = gcad::sim::simulate(spec_of(&pulled).sim.as_deref().expect("sim"))
        .expect_err("no buckling");
    assert!(format!("{error}").contains("cannot buckle"), "{error}");
}

#[test]
fn thermal_expansion() {
    let bar = "rect 100 10 at=50,0\nbar: extrude 10 both\nmaterial steel\n";
    let held = simulated(
        "thermal_expansion_held",
        &[("bar.gcad", bar)],
        "sim static\npart bar.gcad\nslide <X\nends: slide >X\nslide <Y\nslide <Z\nwarm 50\nmid: probe 50,0,0\nmesh 4",
    );
    close(
        reading(&held, "mid.stress"),
        200.0e3 * 12.0e-6 * 50.0,
        1.0e-6,
    );
    close(
        reading(&held, "ends"),
        200.0e3 * 12.0e-6 * 50.0 * 100.0,
        1.0e-6,
    );
    let free = simulated(
        "thermal_expansion_free",
        &[("bar.gcad", bar)],
        "sim static\npart bar.gcad\nslide <X\nslide <Y\nslide <Z\nwarm 50\nmesh 4",
    );
    assert!(reading(&free, "stress") < 1.0e-6);
    let growth: f64 = 12.0e-6 * 50.0;
    let corner = (growth * 100.0_f64)
        .hypot(growth * 10.0)
        .hypot(growth * 10.0);
    close(reading(&free, "displacement"), corner, 1.0e-6);
}

#[test]
fn expansion_from_a_heat_result() {
    let bar = "rect 100 10 at=50,0\nbar: extrude 10 both\nmaterial steel\n";
    let dir = PathBuf::from(scratch("expansion_from_a_heat_result"));
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(
        dir.join("hot.gsim"),
        "sim thermal\npart bar.gcad\ntemperature <X 70\ntemperature >X 70\nmesh 4\n",
    )
    .expect("heat spec");
    let free = simulated(
        "expansion_from_a_heat_result",
        &[("bar.gcad", bar)],
        "sim static\npart bar.gcad\nslide <X\nslide <Y\nslide <Z\nwarm hot.gsim from=20\nmesh 4",
    );
    assert!(reading(&free, "stress") < 1.0e-4, "{}", free.table());
    let growth: f64 = 12.0e-6 * 50.0;
    let corner = (growth * 100.0_f64)
        .hypot(growth * 10.0)
        .hypot(growth * 10.0);
    close(reading(&free, "displacement"), corner, 1.0e-6);
    let path = written(
        "expansion_from_a_heat_result",
        &[("bar.gcad", bar)],
        "sim static\npart bar.gcad\nslide <X\nslide <Y\nslide <Z\nwarm hot.gsim\nmesh 5",
    );
    let error =
        gcad::sim::simulate(spec_of(&path).sim.as_deref().expect("sim")).expect_err("mismatch");
    assert!(
        format!("{error}").contains("same `part` and `mesh`"),
        "{error}"
    );
}

#[test]
fn contact_presses_but_lets_go() {
    let stack = "part a block.gcad\npart b block.gcad\nmove b 0,0,10\n";
    let holds = "slide a:<Z\nslide b:>Z\nslide a:<X\nslide a:<Y\nslide b:<X\nslide b:<Y\n";
    let run = |name: &str, warm: &str| {
        simulated(
            name,
            &[("block.gcad", SLAB), ("stack.gasm", stack)],
            &format!(
                "sim static\npart stack.gasm\n{holds}{warm}\ncontact a b\nlow: probe 0,0,5\nmesh 3"
            ),
        )
    };
    let squeezed = run("contact_squeezed", "warm 50");
    close(
        reading(&squeezed, "low.stress"),
        200.0e3 * 12.0e-6 * 50.0,
        0.005,
    );
    let pulled = run("contact_pulled", "warm -50");
    assert!(reading(&pulled, "low.stress") < 0.5, "{}", pulled.table());
    assert!(
        pulled
            .notes
            .iter()
            .any(|n| n.contains("frictionless contact"))
    );
}
