mod common;

use common::*;
use std::f64::consts::PI;

fn frustum(a: f64, b: f64, h: f64) -> f64 {
    h / 3.0 * (a + b + (a * b).sqrt())
}

#[test]
fn extrude() {
    assert_bounds(
        &build("rect 10 10\nextrude 5"),
        [-5.0, -5.0, 0.0],
        [5.0, 5.0, 5.0],
    );
}

#[test]
fn extrude_reversed() {
    assert_bounds(
        &build("rect 10 10\nextrude -5"),
        [-5.0, -5.0, -5.0],
        [5.0, 5.0, 0.0],
    );
}

#[test]
fn extrude_draft() {
    let top = 20.0 - 2.0 * 10.0 * 5.0_f64.to_radians().tan();
    assert_volume(
        &build("rect 20 20\nextrude 10 draft=5"),
        frustum(400.0, top * top, 10.0),
        0.0005,
    );
}

#[test]
fn extrude_symmetric() {
    assert_bounds(
        &build("rect 10 10\nextrude 10 both"),
        [-5.0, -5.0, -5.0],
        [5.0, 5.0, 5.0],
    );
}

#[test]
fn extrude_up_to_face() {
    let model = build(
        "rect 40 30\nbase: extrude 10\nplane XY offset=20\nrect 10 10\nextrude upto=base.end",
    );
    assert_volume(&model, 13000.0, 1.0e-6);
}

#[test]
fn cut_blind() {
    assert_volume(
        &build("rect 40 30\nbase: extrude 10\nplane base.end\nrect 20 10\ncut 4"),
        11200.0,
        1.0e-6,
    );
}

#[test]
fn cut_through() {
    assert_volume(
        &build("rect 40 30\nbase: extrude 10\nplane base.end\nrect 10 10\ncut thru"),
        11000.0,
        1.0e-6,
    );
}

#[test]
fn cut_draft() {
    let d = 4.0 * 3.0_f64.to_radians().tan();
    let pocket =
        4.0 / 6.0 * (200.0 + (20.0 - 2.0 * d) * (10.0 - 2.0 * d) + 4.0 * (20.0 - d) * (10.0 - d));
    let model = build("rect 40 30\nbase: extrude 10\nplane base.end\nrect 20 10\ncut 4 draft=3");
    assert_volume(&model, 12000.0 - pocket, 0.0005);
}

#[test]
fn revolve_full() {
    assert_volume(
        &build("plane XZ\nrect 4 10 at=8,5\nrevolve 360 axis=y"),
        PI * 64.0 * 10.0,
        0.0005,
    );
}

#[test]
fn revolve_partial() {
    assert_volume(
        &build("plane XZ\nrect 4 10 at=8,5\nrevolve 90 axis=y"),
        PI * 64.0 * 10.0 / 4.0,
        0.0005,
    );
}

#[test]
fn sweep_bent_path() {
    let model = build("circle 6\npath 0,0,0 0,0,20 30,0,20 30,20,20 r=8\nsweep");
    assert_volume(&model, PI * 9.0 * (38.0 + 8.0 * PI), 0.0005);
}

#[test]
fn sweep_straight_path() {
    assert_volume(
        &build("rect 4 4\npath 0,0,0 10,10,10\nsweep"),
        16.0 * 300.0_f64.sqrt(),
        1.0e-6,
    );
}

#[test]
fn sweep_helix() {
    let length = 3.0 * ((20.0 * PI).powi(2) + 25.0).sqrt();
    assert_volume(
        &build("circle 2\nhelix r=10 pitch=5 turns=3\nsweep"),
        PI * length,
        0.005,
    );
}

#[test]
fn loft() {
    let model = build("rect 20 20\nsection\nplane XY offset=10\nrect 10 10\nsection\nloft");
    assert_volume(&model, frustum(400.0, 100.0, 10.0), 0.0005);
}

#[test]
fn shell() {
    let model = build("rect 40 30\nbase: extrude 20\nshell 2 open=base.end");
    assert_volume(&model, 24000.0 - 36.0 * 26.0 * 18.0, 1.0e-6);
}

#[test]
fn face_draft() {
    let top = 20.0 - 2.0 * 10.0 * 5.0_f64.to_radians().tan();
    let model = build("rect 20 20\nbase: extrude 10\ndraft 5 base.side neutral=base.start");
    assert_volume(&model, frustum(400.0, top * top, 10.0), 0.0005);
}

#[test]
fn push_face() {
    assert_volume(
        &build("rect 40 30\nbase: extrude 3\npush base.end 2"),
        6000.0,
        1.0e-6,
    );
}
