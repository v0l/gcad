use linecad::geometry::volume;
use linecad::model::{Model, run};
use linecad::parse::parse_program;
use linecad::select::{select_edges, select_faces};
use std::f64::consts::PI;

fn build(source: &str) -> Model {
    let lines = parse_program(source).expect("parses");
    let result = run(&lines);
    if let Some((line, Err(error))) = result.steps.last() {
        panic!("line {} `{}`: {error:#}", line.number, line.text);
    }
    result.model
}

fn failure(source: &str) -> String {
    let lines = parse_program(source).expect("parses");
    let result = run(&lines);
    match result.steps.last() {
        Some((_, Err(error))) => format!("{error:#}"),
        _ => panic!("expected `{source}` to fail"),
    }
}

fn assert_volume(model: &Model, expected: f64, relative: f64) {
    let actual = volume(model.solid.as_ref().expect("a solid"));
    assert!(
        (actual - expected).abs() <= expected * relative,
        "volume {actual:.4}, expected {expected:.4} within {:.2}%",
        relative * 100.0
    );
}

fn part(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/examples/parts/{name}.lcad",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("example exists")
}

const SPANDREL: f64 = 1.0 - PI / 4.0;

#[test]
fn rounded_plate_with_holes() {
    let model = build(
        "let w=40 h=30 t=3\nrect w h r=4\nbase: extrude t\nplane base.end\nhole 3.2 15,10 -15,10 -15,-10 15,-10",
    );
    let plate = (1200.0 - (4.0 - PI) * 16.0) * 3.0;
    assert_volume(&model, plate - 4.0 * PI * 1.6 * 1.6 * 3.0, 0.001);
}

#[test]
fn plate_example_builds() {
    let model = build(&part("plate"));
    assert_volume(&model, 3441.98, 0.002);
}

#[test]
fn top_perimeter_fillet() {
    let model = build("rect 40 30\nbase: extrude 3\nfillet 1 base.end&base.side");
    assert_volume(
        &model,
        3600.0 - 140.0 * SPANDREL + 4.0 * 0.095_870_338,
        0.0005,
    );
}

#[test]
fn drafted_extrude_is_a_frustum() {
    let model = build("rect 20 20\nextrude 10 draft=5");
    let top = 20.0 - 2.0 * 10.0 * 5.0_f64.to_radians().tan();
    let (a, b) = (400.0, top * top);
    assert_volume(&model, 10.0 / 3.0 * (a + b + (a * b).sqrt()), 0.0005);
}

#[test]
fn pocket_cut() {
    let model = build("rect 40 30\nbase: extrude 10\nplane base.end\nrect 20 10\ncut 4");
    assert_volume(&model, 12000.0 - 800.0, 0.0005);
}

#[test]
fn ring_example_is_revolved() {
    assert_volume(&build(&part("ring")), PI * (100.0 - 36.0) * 10.0, 0.002);
}

#[test]
fn pipe_example_follows_its_path() {
    let centerline = 12.0 + 14.0 + 12.0 + 2.0 * (PI / 2.0 * 8.0);
    assert_volume(&build(&part("pipe")), PI * 9.0 * centerline, 0.002);
}

#[test]
fn bracket_example_builds() {
    assert_volume(&build(&part("bracket")), 13800.03, 0.002);
}

#[test]
fn boss_on_a_face_fuses_with_the_plate() {
    let model = build("rect 40 30\nbase: extrude 5\nplane base.end\ncircle 10 at=5,0\nextrude 8");
    assert_volume(&model, 6000.0 + PI * 25.0 * 8.0, 0.002);
}

#[test]
fn selectors_pick_the_expected_edges() {
    let model = build("rect 40 30\nbase: extrude 3\nplane base.end\nholes: hole 4 0,0");
    let solid = model.solid.as_ref().expect("a solid");
    let tolerance = model.tolerance();
    let count = |selector: &str| {
        select_edges(selector, solid, &model.groups, tolerance)
            .expect("selects")
            .len()
    };
    assert_eq!(count("base.end&base.side"), 4);
    assert_eq!(count("base.side&base.side"), 4);
    assert_eq!(count("holes.side&base.end"), 4);
    assert_eq!(count("base.end"), 8);
    assert_eq!(
        select_faces(">Z", solid, &model.groups, tolerance)
            .expect("selects")
            .len(),
        1
    );
    assert_eq!(
        select_faces("+X,-X", solid, &model.groups, tolerance)
            .expect("selects")
            .len(),
        2
    );
}

#[test]
fn face_workplanes_keep_world_coordinates() {
    let model = build("rect 40 30\nbase: extrude 3\nplane >X");
    let frame = model.frame.expect("a plane");
    assert!(
        (frame.origin.x - 20.0).abs() < 1.0e-9
            && frame.origin.y.abs() < 1.0e-9
            && frame.origin.z.abs() < 1.0e-9
    );
    assert!((frame.y.z - 1.0).abs() < 1.0e-9);
}

#[test]
fn errors_say_what_to_do() {
    assert!(failure("rect 10 10\nextrude 1\nfillet 1 nope.end").contains("known groups"));
    assert!(failure("bend 3").contains("unknown operation"));
    assert!(failure("rect w 10").contains("let w"));
    assert!(failure("rect 10 10\nplane XZ").contains("extrude or cut it"));
    assert!(failure("rect 10 10 r=5").contains("under half"));
    assert!(failure("rect 10 10\nextrude 2 depth=3").contains("no `depth` argument"));
    assert!(failure("cut 3").contains("needs a sketch"));
}

#[test]
fn every_cube_edge_reports_the_missing_vertex_blend() {
    assert!(
        failure("rect 20 20\nextrude 20\nfillet 2 all")
            .to_lowercase()
            .contains("vertex")
    );
}
