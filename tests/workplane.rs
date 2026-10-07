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
#[ignore = "missing: plane rotation"]
fn rotated() {
    assert_bounds(
        &build("plane XY rx=90\nrect 10 10\nextrude 5"),
        [-5.0, -5.0, -5.0],
        [5.0, 0.0, 5.0],
    );
}
