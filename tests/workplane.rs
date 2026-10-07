mod common;

use common::*;
use std::f64::consts::PI;

#[test]
fn xy() {
    assert_bounds(
        &build("plane XY\nrect 10 10\nextrude 5"),
        [-5.0, -5.0, 0.0],
        [5.0, 5.0, 5.0],
    );
}

#[test]
fn xz() {
    assert_bounds(
        &build("plane XZ\nrect 10 10\nextrude 5"),
        [-5.0, -5.0, -5.0],
        [5.0, 0.0, 5.0],
    );
}

#[test]
fn yz() {
    assert_bounds(
        &build("plane YZ\nrect 10 10\nextrude 5"),
        [0.0, -5.0, -5.0],
        [5.0, 5.0, 5.0],
    );
}

#[test]
fn offset() {
    assert_bounds(
        &build("plane XY offset=10\nrect 10 10\nextrude 5"),
        [-5.0, -5.0, 10.0],
        [5.0, 5.0, 15.0],
    );
}

#[test]
fn on_a_face() {
    let model = build("rect 40 30\nbase: extrude 5\nplane base.end\ncircle 10 at=5,0\nextrude 8");
    assert_volume(&model, 6000.0 + PI * 25.0 * 8.0, 0.0005);
}

#[test]
fn side_face_keeps_world_coordinates() {
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
fn rotated() {
    assert_bounds(
        &build("plane XY rx=90\nrect 10 10\nextrude 5"),
        [-5.0, -5.0, -5.0],
        [5.0, 0.0, 5.0],
    );
}

#[test]
#[ignore = "missing: plane through three points"]
fn through_three_points() {
    let model = build("plane 0,0,0 10,0,0 0,10,10");
    let n = model.frame.expect("a plane").normal;
    assert!(
        (n.y + 0.5_f64.sqrt()).abs() < 1.0e-9 && (n.z - 0.5_f64.sqrt()).abs() < 1.0e-9,
        "{n:?}"
    );
}

#[test]
#[ignore = "missing: plane edge= angle="]
fn at_an_angle_to_an_edge() {
    let model = build("rect 40 30\nbase: extrude 10\nplane edge=base.end&>Y angle=30");
    let n = model.frame.expect("a plane").normal;
    assert!((n.z - 30.0_f64.to_radians().cos()).abs() < 1.0e-9, "{n:?}");
}

#[test]
#[ignore = "missing: plane path"]
fn normal_to_a_path() {
    let model = build("path 0,0,0 10,0,0\nplane path at=0.5");
    let frame = model.frame.expect("a plane");
    assert!((frame.origin.x - 5.0).abs() < 1.0e-9 && (frame.normal.x - 1.0).abs() < 1.0e-9);
}

#[test]
#[ignore = "missing: axis"]
fn datum_axis() {
    let model = build("axis spin 20,0,0 20,0,1\nplane XZ\nrect 4 10 at=8,5\nrevolve 360 axis=spin");
    assert_volume(&model, 2.0 * PI * 12.0 * 40.0, 0.0005);
}
