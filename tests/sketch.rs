mod common;

use common::*;
use std::f64::consts::PI;

#[test]
fn rect() {
    assert_volume(&build("rect 30 20\nextrude 5"), 3000.0, 1.0e-6);
}

#[test]
fn rect_offset_centre() {
    let model = build("rect 30 20 at=10,-5\nextrude 5");
    assert_bounds(&model, [-5.0, -15.0, 0.0], [25.0, 5.0, 5.0]);
}

#[test]
fn rounded_rect() {
    assert_volume(
        &build("rect 30 20 r=4\nextrude 5"),
        (600.0 - (4.0 - PI) * 16.0) * 5.0,
        0.0002,
    );
}

#[test]
fn circle() {
    assert_volume(&build("circle 10\nextrude 5"), PI * 25.0 * 5.0, 0.0002);
}

#[test]
fn polygon() {
    assert_volume(&build("poly 0,0 30,0 0,20\nextrude 5"), 1500.0, 1.0e-6);
}

#[test]
fn concave_polygon() {
    assert_volume(
        &build("poly 0,0 30,0 30,10 10,10 10,30 0,30\nextrude 5"),
        2500.0,
        1.0e-6,
    );
}

#[test]
fn profile_inside_profile_is_a_hole() {
    assert_volume(
        &build("rect 30 30\ncircle 10\nextrude 5"),
        (900.0 - 25.0 * PI) * 5.0,
        0.0002,
    );
}

#[test]
fn separate_profiles() {
    assert_volume(
        &build("circle 10 at=-20,0\ncircle 10 at=20,0\nextrude 5"),
        2.0 * PI * 25.0 * 5.0,
        0.0002,
    );
}

#[test]
#[ignore = "missing: ngon"]
fn regular_polygon() {
    assert_volume(
        &build("ngon 20 6\nextrude 5"),
        1.5 * 3.0_f64.sqrt() * 100.0 * 5.0,
        1.0e-6,
    );
}

#[test]
#[ignore = "missing: slot"]
fn slot() {
    assert_volume(
        &build("slot 30 10\nextrude 5"),
        (200.0 + 25.0 * PI) * 5.0,
        0.0002,
    );
}

#[test]
#[ignore = "missing: ellipse"]
fn ellipse() {
    assert_volume(
        &build("ellipse 20 10\nextrude 5"),
        PI * 10.0 * 5.0 * 5.0,
        0.0005,
    );
}

#[test]
#[ignore = "missing: pen, line, arc, close"]
fn lines_and_arcs() {
    let model = build("pen 0,0\nline 20,0\narc 20,20 via=30,10\nline 0,20\nclose\nextrude 5");
    assert_volume(&model, (400.0 + 50.0 * PI) * 5.0, 0.0002);
}

#[test]
#[ignore = "missing: spline"]
fn spline() {
    let ring: Vec<String> = (0..12)
        .map(|i| {
            let a = i as f64 / 12.0 * std::f64::consts::TAU;
            format!("{:.6},{:.6}", 10.0 * a.cos(), 10.0 * a.sin())
        })
        .collect();
    let model = build(&format!("spline {} closed\nextrude 5", ring.join(" ")));
    assert_volume(&model, PI * 100.0 * 5.0, 0.01);
}

#[test]
#[ignore = "missing: text"]
fn text() {
    let model = build("text LINECAD size=10\nextrude 1");
    assert!(volume(&model) > 10.0);
}
