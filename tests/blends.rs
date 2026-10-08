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
    assert_volume(&model, 8000.0 - 4.0 * 20.0 * 9.0 * SPANDREL, 2.0e-5);
}

#[test]
fn fillet_cylinder_rim() {
    let model = build("circle 10\nboss: extrude 10\nfillet 1 boss.end");
    assert_volume(
        &model,
        PI * 250.0 - 2.0 * PI * (5.0 - SPANDREL_CENTROID) * SPANDREL,
        1.0e-4,
    );
}

#[test]
fn fillet_every_edge_of_a_plate_with_holes() {
    let (a, b, h) = (38.0, 28.0, 3.0);
    let plate = a * b * h + 2.0 * (a * b + b * h + a * h) + PI * (a + b + h) + 4.0 / 3.0 * PI;
    let rims = 4.0 * 2.0 * PI * (2.0 + SPANDREL_CENTROID) * SPANDREL;
    assert_volume(
        &build("rect 40 30\nbase: extrude 5\nplane base.end\nhole 4 10,0 -10,0\nfillet 1 all"),
        plate - 2.0 * PI * 4.0 * 5.0 - rims,
        1.0e-4,
    );
}

#[test]
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
fn fillet_every_edge_of_a_prism() {
    let model = build("ngon 30 6\nextrude 10\nfillet 1.5 all");
    assert_volume(&model, 5754.47, 0.0005);
}

#[test]
fn fillet_three_edges_at_one_corner() {
    let model = build("rect 20 20\nextrude 20\nfillet 2 >X&>Y|>X&>Z|>Y&>Z");
    let removed = 3.0 * 18.0 * SPANDREL * 4.0 + 8.0 * (1.0 - PI / 6.0);
    assert_volume(&model, 8000.0 - removed, 0.0002);
}

#[test]
fn fillet_some_edges_at_a_corner() {
    let model =
        build("rect 20 20\nbase: extrude 20\nfillet 2 base.end&base.side|base.side&base.side");
    let prism = (400.0 - (4.0 - PI) * 4.0) * 20.0;
    let removed =
        64.0 * SPANDREL * 4.0 + 2.0 * PI * (2.0 - 2.0 * SPANDREL_CENTROID) * SPANDREL * 4.0;
    assert_volume(&model, prism - removed, 0.001);
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

#[test]
fn chamfer_every_edge_of_a_box() {
    let (side, d): (f64, f64) = (20.0, 2.0);
    let strips = 12.0 * (side - 2.0 * d) * d * d / 2.0;
    let corners = 8.0 * (d.powi(3) - d.powi(3) / 6.0);
    let model = build("rect 20 20\nextrude 20\nchamfer 2 all");
    assert_volume(&model, side.powi(3) - strips - corners, 1.0e-5);
}

#[test]
fn fillet_every_edge_of_a_rounded_prism() {
    let model = build("rect 20 20 r=5\nextrude 10\nfillet 1 all");
    let v = volume(&model);
    assert!(v < (400.0 - (4.0 - PI) * 25.0) * 10.0 && v > 3500.0, "{v}");
}

const L_SHAPE: &str = "poly 0,0 20,0 20,10 10,10 10,20 0,20\nbase: extrude 10\n";
const L_ROUNDED_AREA: f64 = 300.0 - 4.0 * SPANDREL;
const L_ROUNDED_PERIMETER: f64 = 80.0 + 6.0 * (PI / 2.0 - 2.0);
const BAND_INSET: f64 = SPANDREL;
const BAND_INSET_SQUARED: f64 = 5.0 / 3.0 - PI / 2.0;

fn rounded_box(a: f64, b: f64, c: f64) -> f64 {
    let (a, b, c) = (a - 2.0, b - 2.0, c - 2.0);
    a * b * c + 2.0 * (a * b + b * c + a * c) + PI * (a + b + c) + 4.0 / 3.0 * PI
}

fn rounded_square_prism(side: f64, depth: f64) -> f64 {
    depth * (side * side - (4.0 - PI)) + 2.0 * PI * BAND_INSET_SQUARED
}

#[test]
fn fillet_every_edge_of_an_l_shape() {
    assert_volume(
        &build(&format!("{L_SHAPE}fillet 1 all")),
        L_ROUNDED_AREA * 10.0 - 2.0 * L_ROUNDED_PERIMETER * BAND_INSET
            + 2.0 * PI * BAND_INSET_SQUARED,
        0.0002,
    );
}

#[test]
fn fillet_inside_edge_ending_at_a_sharp_corner() {
    assert_volume(
        &build(&format!(
            "{L_SHAPE}fillet 1 base.end&base.side|base.side&base.side"
        )),
        L_ROUNDED_AREA * 10.0 - L_ROUNDED_PERIMETER * BAND_INSET + PI * BAND_INSET_SQUARED,
        0.0002,
    );
}

#[test]
fn chamfer_every_edge_of_an_l_shape() {
    let band = 300.0 - 40.0 + 4.0 / 3.0 - 2.5 / 3.0 + 3.5 / 3.0;
    assert_volume(
        &build(&format!("{L_SHAPE}chamfer 1 all")),
        8.0 * 298.0 + 2.0 * band,
        1.0e-5,
    );
}

#[test]
fn fillet_every_edge_of_a_pocket() {
    assert_volume(
        &build("rect 40 40\nbase: extrude 10\nplane base.end\nrect 20 20\ncut 5\nfillet 1 all"),
        rounded_box(40.0, 40.0, 10.0) - rounded_square_prism(20.0, 5.0),
        0.0002,
    );
}

#[test]
fn fillet_pocket_floor_with_sharp_walls() {
    assert_volume(
        &build(
            "rect 40 40\nbase: extrude 10\nplane base.end\nrect 20 20\npocket: cut 5\nfillet 1 base.side&base.side|base.end&base.side|base.start&base.side|pocket.end&pocket.side",
        ),
        rounded_box(40.0, 40.0, 10.0) - 2000.0 + 80.0 * SPANDREL - 4.0 * ROUND_CORNER_OVERLAP,
        0.0002,
    );
}

#[test]
fn fillet_every_edge_of_a_boss() {
    assert_volume(
        &build("rect 40 40\nbase: extrude 5\nplane base.end\nrect 20 20\nextrude 5\nfillet 1 all"),
        rounded_box(40.0, 40.0, 5.0) + rounded_square_prism(20.0, 5.0),
        0.0002,
    );
}

#[test]
fn fillet_variable_chain() {
    let model = build(&format!("{PLATE}fillet 1 base.end&base.side to=2"));
    let v = volume(&model);
    assert!(
        v < 3600.0 - 140.0 * SPANDREL && v > 3600.0 - 140.0 * SPANDREL * 4.0,
        "{v}"
    );
}

#[test]
fn full_round() {
    assert_volume(
        &build("rect 40 4\nbase: extrude 10\nfillet full base.end"),
        40.0 * (32.0 + 2.0 * PI),
        0.0005,
    );
}
