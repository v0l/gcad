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
fn torque_through_lossy_gears() {
    let lossy = LIFT.replace("ratio=360/(pi*20)", "ratio=360/(pi*20) efficiency=0.5 back=0.25");
    let outcome = simulated(
        "torque_through_lossy_gears",
        &[
            ("base.gcad", BASE),
            ("block.gcad", BLOCK),
            ("pinion.gcad", PINION),
            ("lift.gasm", &lossy),
        ],
        "sim torque\npart lift.gasm",
    );
    let held = 20.0 * 20.0 * 20.0 * 7.85 / 1.0e6 * G * 0.01;
    close(reading(&outcome, "drive"), held / 0.5, 1.0e-6);
    close(reading(&outcome, "drive.back"), held * 0.25, 1.0e-6);
    assert!(outcome.reading("lift.back").is_none());
}

#[test]
fn drag_on_joints() {
    let dragged = LIFT
        .replace("min=0 max=50", "min=0 max=50 drag=0.1")
        .replace("turn about=spin", "turn about=spin drag=0.0002")
        .replace("ratio=360/(pi*20)", "ratio=360/(pi*20) efficiency=0.5 back=0.25");
    let outcome = simulated(
        "drag_on_joints",
        &[
            ("base.gcad", BASE),
            ("block.gcad", BLOCK),
            ("pinion.gcad", PINION),
            ("lift.gasm", &dragged),
        ],
        "sim torque\npart lift.gasm",
    );
    let force = 20.0 * 20.0 * 20.0 * 7.85 / 1.0e6 * G;
    let held = force * 0.01;
    close(
        reading(&outcome, "drive"),
        held / 0.5 + 0.1 * 0.01 / 0.5 + 0.0002,
        1.0e-6,
    );
    close(
        reading(&outcome, "drive.back"),
        held * 0.25 - 0.1 * 0.01 - 0.0002,
        1.0e-6,
    );
    close(reading(&outcome, "lift"), force + 0.1 + 0.0002 / 0.01, 1.0e-6);
    close(reading(&outcome, "lift.back"), force - 0.1 - 0.0002 / 0.01, 1.0e-6);
}

#[test]
fn worm_that_locks() {
    let worm = LIFT.replace("ratio=360/(pi*20)", "ratio=360/(pi*20) lead=3.58 friction=0.1");
    let outcome = simulated(
        "worm_that_locks",
        &[
            ("base.gcad", BASE),
            ("block.gcad", BLOCK),
            ("pinion.gcad", PINION),
            ("lift.gasm", &worm),
        ],
        "sim torque\npart lift.gasm",
    );
    let held = 20.0 * 20.0 * 20.0 * 7.85 / 1.0e6 * G * 0.01;
    let lead = 3.58_f64.to_radians();
    let efficiency = lead.tan() / (lead + 0.1_f64.atan()).tan();
    close(reading(&outcome, "drive"), held / efficiency, 1.0e-6);
    assert_eq!(reading(&outcome, "drive.back"), 0.0);
    assert!(outcome.notes.iter().any(|n| n.starts_with("drive locks")));
}

#[test]
fn torque_on_a_joint() {
    let outcome = simulated(
        "torque_on_a_joint",
        &pendulum_files(),
        "sim torque\npart arm.gasm\ngravity 0,0,0\ntorque swing 2.5",
    );
    close(reading(&outcome, "swing"), 2.5, 1.0e-6);
    let swing = outcome.reading("swing").expect("swing");
    assert!(swing.at.contains("pushing negative"), "{}", swing.at);
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
fn peak_stress_leaves_out_the_fixed_edge() {
    let run = |mesh: f64| {
        simulated(
            &format!("peak_stress_fixed_edge_{mesh}"),
            &[("beam.gcad", BEAM)],
            &format!(
                "sim static\npart beam.gcad\nroot: fix <X\nload >X force=0,0,-100\nmesh {mesh}"
            ),
        )
    };
    let (coarse, fine) = (run(4.0), run(2.0));
    let inertia = 20.0 * 10.0_f64.powi(3) / 12.0;
    for (outcome, reach) in [(&coarse, 4.0), (&fine, 2.0)] {
        let root = 100.0 * (200.0 - reach) * 5.0 / inertia;
        close(reading(outcome, "stress"), root, 0.05);
        assert!(reading(outcome, "support_stress") > reading(outcome, "stress"));
        close(
            reading(outcome, "safety"),
            250.0 / reading(outcome, "stress"),
            1.0e-9,
        );
    }
    assert!(reading(&fine, "support_stress") > reading(&coarse, "support_stress"));
}

#[test]
fn small_part_keeps_its_own_stress_band() {
    let outcome = simulated(
        "small_part_keeps_its_own_stress_band",
        &[
            ("slab.gcad", "rect 200 200\nextrude 20\nmaterial steel\n"),
            (
                "post.gcad",
                "rect 6 6\nextrude 10\nmaterial aluminium E=70 nu=0.33 yield=50\n",
            ),
            ("post.gasm", "part slab slab.gcad\npart post post.gcad\nmove post 0,0,20\n"),
        ],
        "sim static\npart post.gasm\nfix slab:<Z\nload post:>Z force=100,0,0",
    );
    let safety = outcome
        .readings
        .iter()
        .find(|r| r.name == "safety")
        .expect("a safety reading");
    assert!(safety.at.contains("`post`"), "{}", safety.at);
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
    let fresh = gcad::sim::picture(spec, &[]).expect("a simulation");
    assert!(fresh.stale.is_none());
    let view = fresh.field.expect("a field");
    assert!(!view.triangles.is_empty() && !view.values.is_empty());
    assert!(view.shade.iter().all(|s| (0.0..=1.0).contains(s)));
    std::fs::write(
        path.with_file_name("beam.gcad"),
        BEAM.replace("200 20", "210 20"),
    )
    .expect("edit");
    let stale = gcad::sim::picture(spec, &[]).expect("a simulation");
    assert!(stale.stale.is_some_and(|why| why.contains("beam.gcad")));
    assert!(stale.outcome.is_some(), "a stale result is still shown");
    assert!(stale.field.is_some_and(|f| !f.values.is_empty()));
}

#[test]
fn setup_shown_before_a_result() {
    let path = written(
        "setup_shown_before_a_result",
        &[("beam.gcad", BEAM)],
        "sim static\npart beam.gcad\nfix <X\nload >X force=0,0,-100\nmesh 8",
    );
    let model = spec_of(&path);
    let spec = model.sim.as_deref().expect("sim");
    let picture = gcad::sim::picture(spec, &[]).expect("a simulation");
    assert!(picture.outcome.is_none());
    assert!(picture.stale.is_some_and(|why| why.contains("no result")));
    let view = picture.field.expect("the mesh");
    assert!(!view.triangles.is_empty() && view.values.is_empty());
    let kinds: Vec<_> = view.marks.iter().map(|m| m.kind).collect();
    assert!(
        kinds.contains(&gcad::sim::statics::Stimulus::Push),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&gcad::sim::statics::Stimulus::Hold),
        "{kinds:?}"
    );
    assert!(view.probe(view.positions[0]).is_none());
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
fn tet_mesh_of_stacked_cylinders() {
    let mut source = String::from("circle 20\ns0: extrude 10");
    for (k, length) in [4, 86, 4, 86].into_iter().enumerate() {
        source += &format!("\nplane s{k}.end\ncircle 20\ns{}: extrude {length}", k + 1);
    }
    let solid = solid_of(&source);
    for size in [3.0, 4.0, 5.0] {
        let q = checked_mesh(&solid, size).quality();
        close(q.volume, gcad::geometry::volume(&solid), 0.03);
        assert!(q.min_dihedral > 1.0 && q.slivers < 10, "{size}: {q:?}");
    }
}

#[test]
fn slightly_flat_elements_keep_the_mesh_size() {
    let mut source = String::from("circle 20\ns0: extrude 10");
    for (k, length) in [4, 86, 4, 86].into_iter().enumerate() {
        source += &format!("\nplane s{k}.end\ncircle 20\ns{}: extrude {length}", k + 1);
    }
    let (mesh, used) = gcad::sim::mesh::mesh_part(&solid_of(&source), 4.0).expect("meshes");
    let worst = mesh.quality().min_dihedral;
    assert!(worst > 0.5 && worst < 2.0, "{worst}");
    assert_eq!(used, 4.0);
}

#[test]
fn tet_mesh_of_a_bore_with_a_tiny_step() {
    let solid = solid_of(
        "circle 18\ncircle 8.04\nshaft: extrude 20\nplane shaft.end\ncircle 13.4\ncircle 8.06\nextrude 12",
    );
    for size in [6.0, 3.6, 2.2] {
        let q = checked_mesh(&solid, size).quality();
        close(q.volume, gcad::geometry::volume(&solid), 0.03);
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
    let stack = "part loose_1 block.gcad\nmove loose_1 50,0,0\npart a block.gcad\npart b block.gcad\nmove b 0,0,10\n";
    let outcome = simulated(
        "leave_parts_out",
        &[("block.gcad", SLAB), ("stack.gasm", stack)],
        "sim static\npart stack.gasm\nleave loose*\nslide a:<Z\nslide a:<X\nslide a:<Y\nslide b:<X\nslide b:<Y\nload b:>Z force=0,0,1000\nmesh 4",
    );
    close(reading(&outcome, "stress"), 2.5, 0.1);
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
        "sim static\npart stack.gasm\nslide a:<Z\nslide a:<X\nslide a:<Y\nslide b:<X\nslide b:<Y\nload b:>Z force=0,0,1000\ntop: probe 0,0,20\njoint: probe 0,0,10.5\nmesh 3",
    );
    let axial: f64 = 1000.0 * 20.0 / (400.0 * 200.0e3);
    let lateral = 0.29 * 2.5 / 200.0e3 * 10.0;
    close(
        reading(&outcome, "top.displacement"),
        (axial * axial + 2.0 * lateral * lateral).sqrt(),
        0.005,
    );
    close(reading(&outcome, "joint.stress"), 2.5, 0.01);
    close(reading(&outcome, "stress"), 2.5, 0.1);
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
fn torque_about_a_named_axis() {
    let rod = [("rod.gcad", "circle 20\nrod: extrude 100\nmaterial steel\n")];
    let spec = |axis: &str| {
        format!(
            "sim static\npart rod.gcad\nbase: fix rod.start\ntorque rod.end 10 axis={axis}\nmesh 6"
        )
    };
    let named = simulated("torque_about_named_z", &rod, &spec("z"));
    let spelled = simulated("torque_about_spelled_z", &rod, &spec("0,0,1"));
    let flipped = simulated("torque_about_minus_z", &rod, &spec("-z"));
    close(reading(&named, "base.moment"), 10.0, 1.0e-3);
    close(
        reading(&named, "displacement"),
        reading(&spelled, "displacement"),
        1.0e-9,
    );
    close(
        reading(&flipped, "displacement"),
        reading(&named, "displacement"),
        1.0e-6,
    );
}

#[test]
fn probe_on_a_curved_face_snaps_to_the_mesh() {
    let disc = [(
        "disc.gcad",
        "material steel\nc: circle 100\nrod: extrude 20\n",
    )];
    let spec = |at: &str| {
        format!(
            "sim static\npart disc.gcad\nfix rod.start\nload rod.end force=0,0,-10\nrim: probe {at}\nmesh 40"
        )
    };
    let between = 50.0 * 7.0_f64.to_radians().cos();
    let side = 50.0 * 7.0_f64.to_radians().sin();
    let outcome = simulated(
        "probe_on_a_curved_face",
        &disc,
        &spec(&format!("{between},{side},10")),
    );
    let rim = outcome.reading("rim.stress").expect("a reading");
    assert!(rim.at.contains("outside the mesh"), "{}", rim.at);
    assert!(rim.value > 0.0);
    let model = spec_of(&written("probe_off_the_part", &disc, &spec("60,0,10")));
    let error = gcad::sim::simulate(model.sim.as_deref().expect("sim")).expect_err("too far");
    assert!(
        format!("{error:#}").contains("not inside the part"),
        "{error:#}"
    );
}

const SHAFT: &str = "circle 20\na: extrude 10\nplane a.end\ncircle 24\nl: extrude 6\nplane l.end\ncircle 20\nm1: extrude 84\nplane m1.end\ncircle 24\nc: extrude 6\nplane c.end\ncircle 20\nm2: extrude 84\nplane m2.end\ncircle 24\nr: extrude 6\nplane r.end\nrect 14 14\nsq: extrude 10\nmaterial steel\n";

fn shaft_on(left: &str, right: &str) -> Outcome {
    simulated(
        &format!("shaft_on_{}", left.replace(' ', "_")),
        &[("shaft.gcad", SHAFT)],
        &format!(
            "sim static\npart shaft.gcad\nleft: {left}\nright: {right}\nkey: slide >X\nload c.side force=0,-1000,0\nmid: probe 0,0,103\nmesh 3"
        ),
    )
}

#[test]
fn bearings_carry_force_and_no_moment() {
    let outcome = shaft_on("bearing l.side axial", "bearing r.side");
    close(reading(&outcome, "left"), 500.0, 0.01);
    close(reading(&outcome, "right"), 500.0, 0.01);
    for side in ["left.moment", "right.moment"] {
        assert!(
            outcome.reading(side).is_none_or(|r| r.value < 0.01),
            "{}",
            outcome.table()
        );
    }
    let inertia = std::f64::consts::PI * 20.0_f64.powi(4) / 64.0;
    let span: f64 = 174.0;
    let bending = 1000.0 * span.powi(3) / (48.0 * 200.0e3 * inertia);
    let shear = 1000.0 * span / (4.0 * 0.9 * 77.5e3 * std::f64::consts::PI * 100.0);
    close(reading(&outcome, "mid.displacement"), bending + shear, 0.06);
    let pinned = shaft_on("pin l.side", "pin r.side");
    assert!(reading(&pinned, "left.moment") > 10.0, "{}", pinned.table());
}

#[test]
fn bearings_need_one_held_along_the_axis() {
    let path = written(
        "bearings_need_one_held_along_the_axis",
        &[("shaft.gcad", SHAFT)],
        "sim static\npart shaft.gcad\nleft: bearing l.side\nright: bearing r.side\nkey: slide >X\nload c.side force=0,-1000,0\nmesh 3",
    );
    let model = spec_of(&path);
    let error = gcad::sim::simulate(model.sim.as_deref().expect("sim")).expect_err("slides");
    assert!(format!("{error:#}").contains("free to move"), "{error:#}");
    let pinned = written(
        "short_pin_suggests_a_bearing",
        &[("shaft.gcad", SHAFT)],
        "sim static\npart shaft.gcad\npin l.side",
    );
    let run = gcad::model::run_path(&pinned, &[], None).expect("runs");
    let (_, summary) = run.steps.last().expect("a line");
    let summary = summary.as_ref().expect("pins");
    assert!(summary.contains("`bearing`"), "{summary}");
}

#[test]
fn revolved_profile_on_its_axis_meshes() {
    let bar = "plane XZ\npen 0,0\nline 5,0\nline 5,-100\nline 0,-100\nclose\nbar: revolve 360 axis=y\nmaterial steel\n";
    let outcome = simulated(
        "revolved_profile_on_its_axis_meshes",
        &[("bar.gcad", bar)],
        "sim static\npart bar.gcad\nfix >Z\nload <Z force=0,0,-1000\nmid: probe 0,0,-50\nmesh 2",
    );
    close(
        reading(&outcome, "mid.stress"),
        1000.0 / (std::f64::consts::PI * 25.0),
        0.02,
    );
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

const CUBE: &str = "plane XY\nrect 100 100\nextrude 100\n";

#[test]
#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "GPU, run with --features gpu-tests"
)]
fn wind_on_a_cube() {
    let outcome = simulated(
        "wind_on_a_cube",
        &[("cube.gcad", CUBE)],
        "sim flow\npart cube.gcad\nwind 10 toward=1,0,0\nmesh 5",
    );
    let q = 0.5 * 1.204 * 10.0 * 10.0;
    let drag = reading(&outcome, "drag");
    close(reading(&outcome, "cda"), 1.05 * 0.01, 0.08);
    close(reading(&outcome, "pressure"), q, 0.04);
    assert!(reading(&outcome, "side").abs() < 0.05 * drag);
    assert!(reading(&outcome, "lift").abs() < 0.05 * drag);
    close(reading(&outcome, "moment.y"), 0.05 * drag, 0.1);
    let turned = simulated(
        "wind_on_a_cube_turned",
        &[("cube.gcad", CUBE)],
        "sim flow\npart cube.gcad\nwind 10 toward=0,-1,0\nmesh 5",
    );
    close(reading(&turned, "drag"), drag, 0.03);
    close(reading(&turned, "moment.x"), 0.05 * drag, 0.1);
}

#[test]
fn flow_setup_shown_before_a_result() {
    use gcad::sim::statics::Stimulus;
    use monstertruck::modeling::InnerSpace;
    let path = written(
        "flow_setup_shown_before_a_result",
        &[("cube.gcad", CUBE)],
        "sim flow\npart cube.gcad\nwind 10 toward=1,0,0\nduct 300 200\nmesh 5\nchip: heat <Z 10",
    );
    let model = spec_of(&path);
    let spec = model.sim.as_deref().expect("sim");
    let picture = gcad::sim::picture(spec, &[]).expect("a simulation");
    assert!(picture.outcome.is_none());
    let view = picture.field.expect("the parts");
    assert!(!view.triangles.is_empty() && view.values.is_empty());
    let heat = view
        .marks
        .iter()
        .find(|m| m.kind == Stimulus::Heat)
        .expect("the heated face");
    assert!(
        heat.text.contains("10 W") && !heat.outline.is_empty(),
        "{} {}",
        heat.text,
        heat.outline.len()
    );
    assert!(heat.at.z.abs() < 1.0e-6, "{:?}", heat.at);
    let wind = view
        .marks
        .iter()
        .find(|m| m.kind == Stimulus::Wind)
        .expect("the wind");
    assert!(
        (wind.at - monstertruck::modeling::Point3::new(-50.0, 0.0, 50.0)).magnitude() < 1.0e-6,
        "{:?}",
        wind.at
    );
    let bounds: monstertruck::modeling::BoundingBox<_> = view.positions.iter().copied().collect();
    close(bounds.max().y - bounds.min().y, 300.0, 0.01);
    close(bounds.max().z - bounds.min().z, 200.0, 0.01);
}

#[test]
fn torque_readings_without_a_field() {
    let path = written(
        "torque_readings_without_a_field",
        &pendulum_files(),
        "sim torque\npart arm.gasm\nsweep swing steps=4",
    );
    let model = spec_of(&path);
    let spec = model.sim.as_deref().expect("sim");
    let outcome = gcad::sim::simulate(spec).expect("solves");
    gcad::sim::save(&path, &[], &outcome).expect("saves");
    let picture = gcad::sim::picture(spec, &[]).expect("a simulation");
    assert!(picture.stale.is_none() && picture.field.is_none());
    assert!(!picture.outcome.expect("the result").series.is_empty());
}

#[test]
#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "GPU, run with --features gpu-tests"
)]
fn wind_in_half_precision() {
    let outcome = simulated(
        "wind_in_half_precision",
        &[("cube.gcad", CUBE)],
        "sim flow\npart cube.gcad\nwind 10 toward=1,0,0\nmesh 5\nprecision half",
    );
    close(reading(&outcome, "cda"), 1.05 * 0.01, 0.08);
    assert!(outcome.notes.iter().any(|n| n.contains("16-bit")));
    assert!(outcome.notes.iter().any(|n| n.contains("known to")));
}

#[test]
#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "GPU, run with --features gpu-tests"
)]
fn wind_loads_a_static_part() {
    let path = written(
        "wind_loads_a_static_part",
        &[("cube.gcad", CUBE)],
        "sim flow\npart cube.gcad\nwind 10 toward=0,1,0\nmesh 5",
    );
    let flow = path.with_file_name("flow.gsim");
    std::fs::rename(&path, &flow).expect("rename");
    std::fs::remove_file(gcad::sim::result_path(&flow)).ok();
    let load =
        "sim static\npart cube.gcad\nmaterial aluminium\nbase: fix <Z\nwind flow.gsim\nmesh 15";
    std::fs::write(&path, load).expect("write spec");
    let early = gcad::sim::simulate(spec_of(&path).sim.as_deref().expect("sim"));
    assert!(format!("{:#}", early.expect_err("no flow result yet")).contains("no result yet"),);
    let outcome = gcad::sim::simulate(spec_of(&flow).sim.as_deref().expect("sim")).expect("flows");
    let json = outcome.to_json(&gcad::sim::stamp(&flow, &[]));
    std::fs::write(gcad::sim::result_path(&flow), json.to_string()).expect("write result");
    let statics = gcad::sim::simulate(spec_of(&path).sim.as_deref().expect("sim")).expect("loads");
    close(reading(&statics, "base"), reading(&outcome, "drag"), 0.06);
    close(
        reading(&statics, "base.moment"),
        reading(&outcome, "moment.x").abs(),
        0.12,
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "GPU, run with --features gpu-tests"
)]
fn wind_shelters_a_part_behind() {
    let pair = "part front cube.gcad\npart back cube.gcad\nmove back 250,0,0\n";
    let outcome = simulated(
        "wind_shelters_a_part_behind",
        &[("cube.gcad", CUBE), ("pair.gasm", pair)],
        "sim flow\npart pair.gasm\nwind 10 toward=1,0,0\nmesh 5",
    );
    let (front, back) = (
        reading(&outcome, "front.drag"),
        reading(&outcome, "back.drag"),
    );
    close(front + back, reading(&outcome, "drag"), 1.0e-9);
    assert!(back < 0.5 * front, "front {front} back {back}");
}

#[test]
#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "GPU, run with --features gpu-tests"
)]
fn wind_seals_a_closed_box() {
    let outcome = simulated(
        "wind_seals_a_closed_box",
        &[
            (
                "base.gcad",
                "plane XY\nrect 100 100\nextrude 99\nplane >Z\nrect 98 98\ncut 98\n",
            ),
            (
                "lid.gcad",
                "plane XY\nrect 100 100\nextrude 1\nplane >Z\nhole 6 20,20 depth=1\n",
            ),
            (
                "case.gasm",
                "part base base.gcad\npart lid lid.gcad\nmove lid 0,0,99\n",
            ),
        ],
        "sim flow\npart case.gasm\nwind 10 toward=1,1,0\nmesh 5\naverage 2",
    );
    let sealed = outcome
        .notes
        .iter()
        .find(|n| n.contains("sealed"))
        .and_then(|n| n.split(' ').next()?.parse::<f64>().ok())
        .unwrap_or_else(|| panic!("no sealed air in {:?}", outcome.notes));
    let inside = 98.0f64.powi(3) * 1.0e-3;
    assert!(
        sealed > 0.7 * inside && sealed < 1.05 * inside,
        "{sealed} cm³ sealed against {inside:.0}"
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "GPU, run with --features gpu-tests"
)]
fn wind_draws_the_flow() {
    let path = written(
        "wind_draws_the_flow",
        &[("cube.gcad", CUBE)],
        "sim flow\npart cube.gcad\nwind 10 toward=1,0,0\nmesh 5\naverage 4",
    );
    let model = spec_of(&path);
    let spec = model.sim.as_deref().expect("sim");
    let outcome = gcad::sim::simulate(spec).expect("flows");
    assert_eq!(outcome.slices.len(), 2);
    for slice in &outcome.slices {
        assert_eq!(slice.speed.len(), slice.width * slice.height());
        assert!(
            slice.speed.iter().any(|v| *v < 0.0),
            "the cube shows in the slice"
        );
        let free = slice.speed[slice.width / 2];
        assert!((free - 10.0).abs() < 1.0, "free stream {free} m/s");
    }
    let traced: Vec<&gcad::sim::Stream> = outcome
        .streams
        .iter()
        .filter(|s| !s.points.is_empty())
        .collect();
    assert!(traced.len() > 20, "{} streamlines", traced.len());
    let slowed = traced
        .iter()
        .filter(|s| s.speed.iter().any(|v| *v < 8.5))
        .count();
    assert!(
        slowed * 10 >= traced.len() * 9,
        "{slowed} of {} slowed by the cube",
        traced.len()
    );
    let q = 0.5 * 1.204 * 10.0 * 10.0;
    assert!(traced.iter().all(|s| (0.0..1.0).contains(&s.turbulence)));
    assert!(
        traced.iter().any(|s| s.turbulence > 0.05),
        "the wake is unsteady"
    );
    assert!(
        traced.iter().all(|s| s.pressure < 1.1 * q),
        "no line above stagnation"
    );
    assert!(
        traced.iter().any(|s| s.pressure > 0.3 * q),
        "lines meeting the face"
    );
    assert!(
        traced.iter().any(|s| s.pressure < -0.3 * q),
        "lines through the suction at the edges"
    );
    for stream in &traced {
        assert!(stream.points[0][0] < -50.0, "seeded upstream");
        assert!(
            stream
                .points
                .iter()
                .all(|p| { p[0].abs() > 45.0 || p[1].abs() > 45.0 || p[2] < 5.0 || p[2] > 95.0 })
        );
    }
    let movie = outcome.movie.as_ref().expect("frames");
    assert_eq!(movie.planes.len(), outcome.slices.len());
    for (reel, slice) in movie.planes.iter().zip(&outcome.slices) {
        assert!(reel.frames.len() > 1);
        assert!(
            reel.frames
                .iter()
                .all(|f| f.len() == slice.width * slice.height())
        );
        assert!(
            reel.frames.windows(2).any(|w| w[0] != w[1]),
            "the flow moves"
        );
    }
    assert_eq!(movie.streams.len(), movie.planes[0].frames.len());
    assert!(
        movie
            .streams
            .iter()
            .all(|f| f.len() == movie.streams[0].len() || !f.is_empty())
    );
    assert!(
        movie
            .streams
            .windows(2)
            .any(|w| w[0].iter().zip(&w[1]).any(|(a, b)| a.points != b.points)),
        "the lines move from frame to frame"
    );
    for line in movie.streams.iter().flatten() {
        let mut buckets = std::collections::HashMap::<[i64; 3], usize>::new();
        for p in &line.points {
            *buckets
                .entry(p.map(|v| (v / 10.0).floor() as i64))
                .or_default() += 1;
        }
        assert!(
            buckets.values().all(|&n| n <= 12),
            "a line circles in place"
        );
    }
    let frames = gcad::sim::movie_path(&path);
    movie.write(&frames).expect("writes");
    let back = gcad::sim::Movie::read(&frames).expect("reads");
    assert_eq!(back.planes[1].frames, movie.planes[1].frames);
    assert_eq!(back.streams.len(), movie.streams.len());
    let (a, b) = (&movie.streams[0][0], &back.streams[0][0]);
    assert_eq!(a.points.len(), b.points.len());
    for (p, q) in a.points.iter().zip(&b.points) {
        for k in 0..3 {
            assert!((p[k] - q[k]).abs() < 0.1, "{p:?} against {q:?}");
        }
    }
    close(back.seconds, movie.seconds, 1.0e-6);
    let view = gcad::sim::wind::field_view(spec, &outcome).expect("view");
    assert!(
        view.movie.is_some(),
        "the viewer finds the frames beside the spec"
    );
    for (k, slice) in outcome.slices.iter().enumerate() {
        let at = |c: f64, r: f64| {
            monstertruck::modeling::Point3::new(
                slice.origin[0] + c * slice.right[0] + r * slice.down[0],
                slice.origin[1] + c * slice.right[1] + r * slice.down[1],
                slice.origin[2] + c * slice.right[2] + r * slice.down[2],
            )
        };
        let upstream = view
            .slice_probe(k, at(3.0, slice.height() as f64 / 2.0))
            .expect("air upstream");
        assert!((upstream.mean - 10.0).abs() < 1.0, "{} m/s", upstream.mean);
        assert!(
            view.slice_probe(k, monstertruck::modeling::Point3::new(0.0, 0.0, 50.0))
                .is_none(),
            "inside the cube is not air"
        );
    }
    let path = std::path::PathBuf::from(scratch("wind_draws_the_flow")).join("flow.png");
    gcad::plot::flow(&outcome.slices, path.to_str().expect("utf-8")).expect("draws");
    let image = image::open(&path).expect("png").to_rgb8();
    assert!(image.width() > 1000 && image.height() > 500);
}

fn material_fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/materials/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn triple(got: [Option<f64>; 3], want: [f64; 3]) {
    for (g, w) in got.iter().zip(want) {
        close(g.expect("a value"), w, 1.0e-9);
    }
}

fn materials_of(
    path: &std::path::Path,
) -> std::collections::HashMap<String, gcad::model::Material> {
    spec_of(path).materials
}

#[test]
fn material_library_is_found() {
    let dir = PathBuf::from(scratch("material_library_is_found"));
    let sub = dir.join("parts");
    std::fs::create_dir_all(dir.join(".git")).expect("dir");
    std::fs::create_dir_all(&sub).expect("dir");
    std::fs::write(
        dir.join("materials.toml"),
        "[petg-cf]\ndensity = 1.3\nE = 3.8\nnu = 0.36\nyield = 52\nprinted = true\n\n[pla]\ndensity = 1.25\nE = 3.5\nnu = 0.35\n",
    )
    .expect("library");
    std::fs::write(
        sub.join("materials.toml"),
        "[petg-cf]\ndensity = 1.31\nE = 3.9\n",
    )
    .expect("nearer library");
    let part = |name: &str, material: &str| {
        let path = sub.join(name);
        std::fs::write(
            &path,
            format!("rect 10 10\nextrude 10\nmaterial {material}\n"),
        )
        .expect("part");
        path
    };
    let near = &materials_of(&part("near.gcad", "PETG-CF"))["main"];
    assert_eq!((near.density, near.mechanics.young), (1.31, Some(3.9)));
    let pla = &materials_of(&part("pla.gcad", "pla"))["main"];
    assert_eq!((pla.density, pla.mechanics.young), (1.25, Some(3.5)));
    std::fs::remove_file(sub.join("materials.toml")).expect("remove");
    let far = &materials_of(&part("far.gcad", "petg-cf"))["main"];
    assert_eq!((far.density, far.mechanics.strength), (1.3, Some(52.0)));
    assert!(far.mechanics.printed);
    let bad = part("bad.gcad", "unobtainium");
    let run = gcad::model::run_path(&bad, &[], None).expect("runs");
    let (_, step) = run.steps.last().expect("a step");
    let error = format!("{:#}", step.as_ref().expect_err("unknown material"));
    assert!(
        error.contains("petg-cf") && error.contains("materials.toml"),
        "{error}"
    );
    std::fs::write(dir.join("materials.toml"), "[odd]\ndensity = 1\nE11 = 3\n")
        .expect("bad library");
    let run = gcad::model::run_path(&part("odd.gcad", "odd"), &[], None).expect("runs");
    let (_, step) = run.steps.last().expect("a step");
    let error = format!("{:#}", step.as_ref().expect_err("unknown key"));
    assert!(
        error.contains("`E11`") && error.contains("E1, E2, E3"),
        "{error}"
    );
}

#[test]
fn directional_material_takes_the_weakest() {
    let library = "[printed-pla]\ndensity = 1.24\nE1 = 3.0\nE2 = 3.0\nE3 = 1.5\nnu12 = 0.35\nnu13 = 0.33\nnu23 = 0.33\nG12 = 1.1\nG13 = 0.6\nG23 = 0.6\nXt = 50\nYt = 50\nZt = 20\nprinted = true\n";
    let path = written(
        "directional_material_takes_the_weakest",
        &[
            ("materials.toml", library),
            (
                "bar.gcad",
                "rect 100 10 at=50,0\nbar: extrude 10 both\nmaterial printed-pla\n",
            ),
        ],
        "sim static\npart bar.gcad\nslide <X\nslide <Y\nslide <Z\nload >X force=1000,0,0\nmid: probe 50,0,0\nmesh 4",
    );
    let outcome =
        gcad::sim::simulate(spec_of(&path).sim.as_deref().expect("sim")).expect("simulates");
    let strain: f64 = 1000.0 / (100.0 * 1.5e3);
    let along = strain * 50.0;
    let across = 0.35 * strain * 5.0;
    close(
        reading(&outcome, "mid.displacement"),
        (along * along + 2.0 * across * across).sqrt(),
        1.0e-3,
    );
    close(reading(&outcome, "safety"), 2.0, 1.0e-6);
    assert!(
        outcome
            .notes
            .iter()
            .any(|n| n.contains("printed-pla") && n.contains("weakest") && n.contains("E 1.5 GPa")),
        "{:?}",
        outcome.notes
    );
    let inputs = gcad::sim::inputs(&path);
    assert!(
        inputs.iter().any(|f| f.ends_with("materials.toml")),
        "{inputs:?}"
    );
}

#[test]
fn material_files_import() {
    let source = format!(
        "materials {}\nmaterials {}\nmaterials {}\nmaterials {}\nrect 10 10\nsteel: extrude 10\nmaterial structural-steel\n",
        material_fixture("workbench.xml"),
        material_fixture("petg.FCMat"),
        material_fixture("printed.FCMat"),
        material_fixture("abs.FCMat"),
    );
    let model = common::build(&source);
    let library = &model.library;
    let steel = &library["structural-steel"];
    close(steel.density.unwrap(), 7.85, 1.0e-9);
    close(steel.mechanics.young.unwrap(), 200.0, 1.0e-9);
    close(steel.mechanics.poisson.unwrap(), 0.3, 1.0e-9);
    close(steel.mechanics.strength.unwrap(), 250.0, 1.0e-4);
    close(steel.mechanics.expansion.unwrap(), 12.0, 1.0e-9);
    assert!(steel.axes.is_none());
    close(model.materials["main"].density, 7.85, 1.0e-9);
    let glass = &library["glass-epoxy-ud"];
    let axes = glass.axes.expect("orthotropic");
    triple(axes.young, [41.0, 9.5, 9.0]);
    triple(axes.poisson, [0.28, 0.29, 0.41]);
    triple(axes.shear, [4.7, 4.6, 3.4]);
    triple(axes.compression, [610.0, 115.0, 110.0]);
    close(axes.shear_strength[0].unwrap(), 75.0, 1.0e-9);
    close(glass.mechanics.young.unwrap(), 9.0, 1.0e-9);
    close(glass.mechanics.strength.unwrap(), 36.0, 1.0e-9);
    close(glass.density.unwrap(), 1.95, 1.0e-9);
    let petg = &library["petg-natural"];
    close(petg.density.unwrap(), 1.27, 1.0e-9);
    close(petg.mechanics.young.unwrap(), 2.1, 1.0e-9);
    close(petg.mechanics.strength.unwrap(), 47.0, 1.0e-9);
    close(petg.mechanics.conductivity.unwrap(), 0.21, 1.0e-9);
    close(petg.mechanics.expansion.unwrap(), 68.0, 1.0e-9);
    let asa = &library["printed-asa"];
    close(asa.density.unwrap(), 1.07, 1.0e-9);
    close(asa.mechanics.young.unwrap(), 1.3, 1.0e-9);
    close(asa.mechanics.poisson.unwrap(), 0.35, 1.0e-9);
    triple(asa.axes.unwrap().shear, [0.7, 0.5, 0.5]);
    let abs = &library["abs-black"];
    close(abs.density.unwrap(), 1.04, 1.0e-9);
    close(abs.mechanics.strength.unwrap(), 30.0, 1.0e-3);
    close(abs.mechanics.expansion.unwrap(), 90.0, 1.0e-9);
    let listed = gcad::model::materials::table(Some(std::path::Path::new(&material_fixture(
        "workbench.xml",
    ))))
    .expect("lists");
    assert!(
        listed.contains("glass-epoxy-ud") && listed.contains("along three axes"),
        "{listed}"
    );
}

const FINNED: &str = "let w=40 l=50 t=5 fins=8 fin=1.5 h=20
plane YZ
rect w t at=0,t/2
base: extrude l
plane XY offset=t
let pitch=(w-fin)/(fins-1)
rect l fin at=l/2,-w/2+fin/2
fins: extrude h
repeat fins count=fins step=0,pitch
material aluminium
";

#[test]
#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "GPU, run with --features gpu-tests"
)]
fn fan_cools_a_ducted_heatsink() {
    let outcome = simulated(
        "fan_cools_a_ducted_heatsink",
        &[("sink.gcad", FINNED)],
        "sim flow\npart sink.gcad\nwind 2 toward=1,0,0\nduct 40 25\nmesh 0.5\nair temperature=25\nchip: heat <Z 10",
    );
    let (density, viscosity) = gcad::sim::wind::AIR;
    let conductivity = density * 1006.0 * viscosity / 0.71;
    let (b, l, h, t, fin, channels) = (4.0e-3, 50.0e-3, 20.0e-3, 1.5e-3, 167.0, 7.0);
    let speed = 2.0 * (40.0 * 25.0) / (channels * 4.0 * 20.0);
    let reynolds = speed * b / viscosity * b / l;
    let prandtl: f64 = 0.71;
    let developed = reynolds * prandtl / 2.0;
    let developing =
        0.664 * reynolds.sqrt() * prandtl.cbrt() * (1.0 + 3.65 / reynolds.sqrt()).sqrt();
    let nusselt = (developed.powi(-3) + developing.powi(-3)).powf(-1.0 / 3.0);
    let m = (2.0 * nusselt * (conductivity / fin) * (h / b) * (h / t) * (t / l + 1.0)).sqrt();
    let efficiency = m.tanh() / m;
    let teertstra = b / (channels * conductivity * 2.0 * l * h * efficiency * nusselt);
    close(reading(&outcome, "resistance"), teertstra, 0.05);
    close(
        reading(&outcome, "chip"),
        25.0 + 10.0 * reading(&outcome, "resistance"),
        1.0e-6,
    );
    assert!(reading(&outcome, "temperature") >= reading(&outcome, "chip"));
    let carried = outcome
        .notes
        .iter()
        .find(|n| n.contains("the air carries"))
        .expect("an energy balance note");
    let share: f64 = carried
        .split_once("box (")
        .and_then(|(_, rest)| rest.split_once('%'))
        .and_then(|(number, _)| number.parse().ok())
        .expect("a share");
    assert!((95.0..=101.0).contains(&share), "{carried}");
}

#[test]
fn flow_heat_lines_parse() {
    let model = spec_of(&written(
        "flow_heat_lines_parse",
        &[("sink.gcad", FINNED)],
        "sim flow\npart sink.gcad\nwind 2 toward=1,0,0\nduct 40 25\nair temperature=30\nchip: heat <Z 5",
    ));
    let spec = model.sim.as_deref().expect("a simulation");
    assert_eq!(spec.duct, Some([40.0, 25.0]));
    assert_eq!(spec.ambient, Some(30.0));
    assert_eq!(spec.thermal.len(), 1);
}
