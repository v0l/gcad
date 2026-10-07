mod common;

use common::*;
use std::f64::consts::PI;

const PLATE: &str = "rect 40 30\nbase: extrude 3\n";

#[test]
fn fillet_edge() {
    assert_volume(
        &build(&format!("{PLATE}fillet 1 base.end&>Y")),
        3600.0 - 40.0 * SPANDREL,
        0.0002,
    );
}

#[test]
fn fillet_open_chain() {
    let model = build(&format!("{PLATE}fillet 1 base.end&>Y,>X"));
    assert_volume(
        &model,
        3600.0 - 70.0 * SPANDREL + ROUND_CORNER_OVERLAP,
        0.0002,
    );
}

#[test]
fn fillet_closed_chain_with_corners() {
    let model = build(&format!("{PLATE}fillet 1 base.end&base.side"));
    assert_volume(
        &model,
        3600.0 - 140.0 * SPANDREL + 4.0 * ROUND_CORNER_OVERLAP,
        0.0002,
    );
}

#[test]
fn fillet_smooth_chain() {
    let model = build("rect 40 30 r=4\nbase: extrude 3\nfillet 1 base.end&base.side");
    let base = (1200.0 - (4.0 - PI) * 16.0) * 3.0;
    let removed = 108.0 * SPANDREL + 2.0 * PI * (4.0 - SPANDREL_CENTROID) * SPANDREL;
    assert_volume(&model, base - removed, 0.0005);
}

#[test]
fn fillet_vertical_edges() {
    let model = build("rect 20 20\nbase: extrude 20\nfillet 3 base.side&base.side");
    assert_volume(&model, 8000.0 - 4.0 * 20.0 * 9.0 * SPANDREL, 0.0005);
}

#[test]
fn fillet_cylinder_rim() {
    let model = build("circle 10\nboss: extrude 10\nfillet 1 boss.end");
    assert_volume(
        &model,
        PI * 250.0 - 2.0 * PI * (5.0 - SPANDREL_CENTROID) * SPANDREL,
        0.002,
    );
}

#[test]
#[ignore = "missing: vertex blends"]
fn fillet_every_edge_of_a_box() {
    let (s, r): (f64, f64) = (16.0, 2.0);
    let rounded = s.powi(3) + 6.0 * s * s * r + 3.0 * PI * s * r * r + 4.0 / 3.0 * PI * r.powi(3);
    assert_volume(
        &build("rect 20 20\nextrude 20\nfillet 2 all"),
        rounded,
        0.002,
    );
}

#[test]
fn fillet_variable_radius() {
    let model = build(&format!("{PLATE}fillet 1 base.end&>Y to=2"));
    assert_volume(&model, 3600.0 - 40.0 * SPANDREL * 7.0 / 3.0, 0.001);
}

#[test]
fn chamfer_closed_chain() {
    assert_volume(
        &build(&format!("{PLATE}chamfer 1 base.end&base.side")),
        3600.0 - 70.0 + 4.0 / 3.0,
        1.0e-5,
    );
}

#[test]
fn chamfer_hole_rim() {
    let model = build(
        "rect 20 20\nbase: extrude 20\nplane base.end\nh: hole 4 0,0\nchamfer 0.5 h.side&base.end",
    );
    let removed = PI * 4.0 * 20.0 + 2.0 * PI * (2.0 + 0.5 / 3.0) * 0.125;
    assert_volume(&model, 8000.0 - removed, 0.0002);
}

#[test]
fn chamfer_two_distances() {
    assert_volume(
        &build(&format!("{PLATE}chamfer 1 base.end&>Y d2=2")),
        3560.0,
        1.0e-5,
    );
}
