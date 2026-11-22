mod common;

use common::*;
use gcad::model::Model;
use gcad_cloud::Cloud;
use std::sync::Arc;

const CUBE: &str = "body part\nrect 20 20 at=10,10\nextrude 20\n";

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/clouds/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn cube_scan() -> String {
    format!("{CUBE}scan: cloud {}\n", fixture("cube_binary.ply"))
}

fn value(model: &Model, name: &str) -> f64 {
    *model.scope.get(name).unwrap_or_else(|| panic!("no value `{name}`"))
}

fn cloud_of(model: &Model, name: &str) -> Arc<Cloud> {
    model.cloud(name).expect("a cloud body").clone()
}

fn near(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} is not within {tolerance} of {b}");
}

#[test]
fn deviation_of_an_offset_cloud_has_the_right_mean_and_sign() {
    let model = build(&format!(
        "{}cloud_offset scan by=0.2\ndeviation scan to=part max=0.5 into=d",
        cube_scan()
    ));
    near(value(&model, "d.mean"), 0.2, 0.01);
    near(value(&model, "d.min"), 0.2, 0.01);
    near(value(&model, "d.max"), 0.2, 0.01);
    assert!(value(&model, "d.std") < 0.01);
    assert_eq!(value(&model, "d.within"), 100.0);
    let inward = build(&format!(
        "{}cloud_offset scan by=-0.2\ndeviation scan to=part max=0.1 into=d",
        cube_scan()
    ));
    near(value(&inward, "d.mean"), -0.2, 0.01);
    assert_eq!(value(&inward, "d.within"), 0.0);
}

#[test]
fn deviation_flags_only_the_moved_face() {
    let model = build(&format!(
        "{}cloud_offset scan by=1 box=19.5,-1,-1,21,21,21 dir=1,0,0\ndeviation scan to=part max=0.5 into=d",
        cube_scan()
    ));
    let scan = cloud_of(&model, "scan");
    let deviation = scan.deviation.as_ref().expect("a deviation column");
    let flagged: Vec<usize> = (0..scan.len()).filter(|&i| deviation[i].abs() > 0.5).collect();
    assert_eq!(flagged.len(), 525);
    assert!(flagged.iter().all(|&i| (scan.positions[i][0] - 21.0).abs() < 1.0e-6));
    near(100.0 - value(&model, "d.within"), 100.0 * 525.0 / 2646.0, 0.01);
    near(value(&model, "d.max"), 1.0, 0.01);
    near(value(&model, "d.min"), 0.0, 0.01);
}

#[test]
fn deviation_works_against_a_mesh_body() {
    let model = build(&format!(
        "ball: blob 0,0,0 r=10\nremesh 0.5\nscan: sample ball count=4000\ncloud_offset scan by=0.3 dir=0,0,1\ndeviation scan to=ball max=0.5 into=d"
    ));
    assert!(value(&model, "d.max") > 0.25 && value(&model, "d.max") < 0.31);
    assert!(value(&model, "d.within") > 99.0);
}

#[test]
fn spacing_of_two_fitted_cylinders_matches_the_cad() {
    let model = build(
        "scan: cloud_synthetic two_cylinders r=5 spacing=30 noise=0.1 count=40000\nfit scan cylinder tol=0.5 into=c all max=2 min=2000\nmeasure scan c1 c2 into=gap\nmeasure scan diameter c1 into=dia",
    );
    near(value(&model, "gap.distance"), 30.0, 0.1);
    near(value(&model, "dia.diameter"), 10.0, 0.1);
}

#[test]
fn point_to_plane_measure_is_signed() {
    let model = build(&format!(
        "{}fit scan plane tol=0.3 into=f all max=6\nmeasure scan 5,5,30 f1 into=h",
        cube_scan()
    ));
    let d = value(&model, "h.signed").abs();
    assert!([10.0, 20.0, 30.0, 5.0, 15.0, 25.0].iter().any(|e| (d - e).abs() < 0.3), "{d}");
    assert!(model.scope.contains_key("h.signed"));
}

#[test]
fn check_lines_use_the_deviation_values() {
    let passing = build(&format!(
        "{}cloud_offset scan by=0.2\ndeviation scan to=part max=0.5 into=d\ncheck d.max < 0.3",
        cube_scan()
    ));
    assert!(passing.checks.last().unwrap().passed);
    let failing = failure(&format!(
        "{}cloud_offset scan by=0.4\ndeviation scan to=part max=0.5 into=d\ncheck d.max < 0.3",
        cube_scan()
    ));
    assert!(failing.contains("d.max"), "{failing}");
}

#[test]
fn deviation_colour_map_colours_by_sign() {
    let model = build(&format!(
        "{}cloud_offset scan by=1 box=19.5,-1,-1,21,21,21 dir=1,0,0\ndeviation scan to=part max=0.5 into=d",
        cube_scan()
    ));
    let scan = cloud_of(&model, "scan");
    let moved = (0..scan.len()).find(|&i| scan.positions[i][0] > 20.5).unwrap();
    let still = (0..scan.len()).find(|&i| scan.positions[i][0] < 5.0).unwrap();
    let colours = scan.colours.as_ref().unwrap();
    assert_eq!(colours[moved], gcad_cloud::colour::diverging(1.0));
    assert_eq!(colours[still], gcad_cloud::colour::diverging(0.5));
}
