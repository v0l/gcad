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
#[ignore = "missing: static stress simulation"]
fn cantilever_stress() {
    let outcome = simulated(
        "cantilever_stress",
        &[("beam.gcad", BEAM)],
        "sim static\npart beam.gcad\nfix <X\nload >X force=0,0,-100\nmesh 2",
    );
    let inertia = 20.0 * 10.0_f64.powi(3) / 12.0;
    close(
        reading(&outcome, "displacement"),
        100.0 * 200.0_f64.powi(3) / (3.0 * 200.0e3 * inertia),
        0.03,
    );
    close(
        reading(&outcome, "stress"),
        100.0 * 200.0 * 5.0 / inertia,
        0.1,
    );
}

#[test]
#[ignore = "missing: thermal simulation"]
fn conduction_through_a_wall() {
    let outcome = simulated(
        "conduction_through_a_wall",
        &[(
            "wall.gcad",
            "rect 100 100\nwall: extrude 10\nmaterial aluminium\n",
        )],
        "sim thermal\npart wall.gcad\nhot: temperature wall.start 100\ncold: temperature wall.end 20\nmesh 5",
    );
    close(reading(&outcome, "cold"), 167.0 * 0.01 * 80.0 / 0.01, 0.01);
}

#[test]
#[ignore = "missing: modal simulation"]
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
    close(reading(&outcome, "mode1"), first, 0.03);
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
