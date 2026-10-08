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
fn regular_polygon() {
    assert_volume(
        &build("ngon 20 6\nextrude 5"),
        1.5 * 3.0_f64.sqrt() * 100.0 * 5.0,
        1.0e-6,
    );
}

fn involute_gear_area(teeth: f64, module: f64) -> f64 {
    involute_outline_area(teeth, module, (1.25, 1.0))
}

fn involute_outline_area(teeth: f64, module: f64, (inner, outer): (f64, f64)) -> f64 {
    let pressure = 20.0_f64.to_radians();
    let pitch = module * teeth / 2.0;
    let base = pitch * pressure.cos();
    let involute = |a: f64| a.tan() - a;
    let half =
        |r: f64| PI / (2.0 * teeth) + involute(pressure) - involute((base / r.max(base)).acos());
    let (root, tip) = (pitch - inner * module, pitch + outer * module);
    let steps = 20000;
    let tooth: f64 = (0..steps)
        .map(|i| {
            let r = root + (tip - root) * (i as f64 + 0.5) / steps as f64;
            2.0 * half(r) * r * (tip - root) / steps as f64
        })
        .sum();
    PI * root * root + teeth * tooth
}

#[test]
fn involute_gear() {
    assert_volume(
        &build("gear 20 2\nextrude 6"),
        involute_gear_area(20.0, 2.0) * 6.0,
        2.0e-4,
    );
    assert_volume(
        &build("gear 9 2\nextrude 6"),
        involute_gear_area(9.0, 2.0) * 6.0,
        2.0e-4,
    );
}

#[test]
fn internal_gear() {
    assert_volume(
        &build("circle 80\ngear 54 1 internal\nextrude 6"),
        (PI * 1600.0 - involute_outline_area(54.0, 1.0, (1.0, 1.25))) * 6.0,
        2.0e-4,
    );
}

#[test]
fn slot() {
    assert_volume(
        &build("slot 30 10\nextrude 5"),
        (200.0 + 25.0 * PI) * 5.0,
        0.0002,
    );
}

#[test]
fn ellipse() {
    assert_volume(
        &build("ellipse 20 10\nextrude 5"),
        PI * 10.0 * 5.0 * 5.0,
        0.0005,
    );
}

#[test]
fn lines_and_arcs() {
    let model = build("pen 0,0\nline 20,0\narc 20,20 via=30,10\nline 0,20\nclose\nextrude 5");
    assert_volume(&model, (400.0 + 50.0 * PI) * 5.0, 0.0002);
}

#[test]
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
fn text() {
    let model = build("text LINECAD size=10\nextrude 1");
    assert!(volume(&model) > 10.0);
}

#[test]
fn rounded_polygon() {
    assert_volume(
        &build("poly 0,0 30,0 30,20 0,20 r=4\nextrude 5"),
        (600.0 - (4.0 - PI) * 16.0) * 5.0,
        0.0002,
    );
}

#[test]
fn chamfered_polygon() {
    assert_volume(
        &build("poly 0,0 30,0 30,20 0,20 c=2\nextrude 5"),
        (600.0 - 8.0) * 5.0,
        1.0e-6,
    );
}

#[test]
fn offset_outline() {
    assert_volume(
        &build("rect 20 10\noffset 2\nextrude 5"),
        (24.0 * 14.0 - 200.0) * 5.0,
        1.0e-6,
    );
}

#[test]
fn arc_by_centre() {
    assert_volume(
        &build("pen 0,0\nline 20,0\narc 0,20 center=0,0\nclose\nextrude 5"),
        100.0 * PI * 5.0,
        0.0002,
    );
}

#[test]
fn constraints() {
    let model = build(
        "point a 0,0\npoint b\npoint c\ndist a b 30\ndist b c 40\ndist a c 50\nhorizontal a b\npoly a b c\nextrude 1",
    );
    assert_volume(&model, 600.0, 1.0e-6);
}

#[test]
fn trim() {
    assert_volume(
        &build("pen 0,0\nline 30,0\nline 30,20\nline 0,20\nline 0,-5\nclose trim\nextrude 5"),
        3000.0,
        1.0e-6,
    );
}

#[test]
fn construction_geometry() {
    assert_volume(
        &build("rect 10 10\ncircle 20 construct\nextrude 5"),
        500.0,
        1.0e-6,
    );
}

#[test]
fn sketch_mirror() {
    assert_volume(
        &build("poly 1,0 11,0 1,10\nreflect y\nextrude 5"),
        500.0,
        1.0e-6,
    );
}

#[test]
fn sketch_pattern() {
    let model = build(
        "rect 40 40\nbase: extrude 5\nplane base.end\ncircle 4 at=10,0\narray count=4 angle=360\ncut thru",
    );
    assert_volume(&model, 8000.0 - 4.0 * PI * 4.0 * 5.0, 0.0002);
}

#[test]
fn dxf_import() {
    let path = scratch("square.dxf");
    let dxf = "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\n0\n90\n4\n70\n1\n10\n0\n20\n0\n10\n20\n20\n0\n10\n20\n20\n20\n10\n0\n20\n20\n0\nENDSEC\n0\nEOF\n";
    std::fs::write(&path, dxf).expect("writes");
    assert_volume(&build(&format!("dxf {path}\nextrude 5")), 2000.0, 1.0e-6);
}

#[test]
fn svg_import() {
    let path = scratch("square.svg");
    std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg"><rect x="0" y="0" width="20" height="10"/></svg>"#).expect("writes");
    assert_volume(&build(&format!("svg {path}\nextrude 5")), 1000.0, 1.0e-6);
}

#[test]
fn rectangle_from_constraints() {
    let model = build(
        "point a 0,0\npoint b near=30,1\npoint c near=31,20\npoint d near=1,21\nhorizontal a b\nperpendicular a b b c\nparallel a b d c\nparallel b c a d\ndist a b 30\ndist b c 20\npoly a b c d\nextrude 1",
    );
    assert_volume(&model, 600.0, 1.0e-6);
}

#[test]
fn square_with_midpoint_and_online() {
    let text = summary(
        "point a 0,0\npoint b near=10,0\npoint c near=10,10\npoint d near=0,10\npoint m near=5,5\nhorizontal a b\nperpendicular a b b c\nperpendicular b c c d\nequal a b b c\nequal b c c d\ndist a b 10\nmidpoint m a c\npoint e near=2,3\nonline e b d\ncoincident e m",
    );
    assert!(
        text.contains("m 5.000,5.000") && text.contains("0 degree(s) of freedom"),
        "{text}"
    );
}
