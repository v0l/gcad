mod common;

use common::*;
use std::f64::consts::PI;

#[test]
fn union() {
    assert_volume(
        &build("rect 20 20\nextrude 10\nplane XY offset=5\nrect 20 20 at=10,10\nextrude 10"),
        7500.0,
        1.0e-6,
    );
}

#[test]
fn subtract() {
    assert_volume(
        &build("rect 20 20\nbase: extrude 10\nplane base.end\nrect 10 10 at=10,10\ncut thru"),
        3750.0,
        1.0e-6,
    );
}

#[test]
fn intersect() {
    assert_volume(
        &build("rect 20 20\nextrude 10\nrect 20 20 at=10,10\nextrude 10 mode=intersect"),
        1000.0,
        1.0e-6,
    );
}

#[test]
fn mirror() {
    let model = build("rect 20 10 at=10,0\nextrude 5\nmirror YZ");
    assert_bounds(&model, [-20.0, -5.0, 0.0], [20.0, 5.0, 5.0]);
    assert_volume(&model, 2000.0, 1.0e-6);
}

#[test]
fn linear_pattern() {
    let model = build(
        "rect 40 30\nbase: extrude 3\nplane base.end\nh: hole 3 -15,0\nrepeat h count=4 step=10,0",
    );
    assert_volume(&model, 3600.0 - 4.0 * PI * 2.25 * 3.0, 0.0002);
}

#[test]
fn circular_pattern() {
    let model = build(
        "circle 40\nbase: extrude 5\nplane base.end\nh: hole 4 15,0\nrepeat h count=6 angle=360",
    );
    assert_volume(&model, PI * 400.0 * 5.0 - 6.0 * PI * 4.0 * 5.0, 0.0002);
}

#[test]
fn translate() {
    assert_bounds(
        &build("rect 10 10\nextrude 5\nmove 10,0,0"),
        [5.0, -5.0, 0.0],
        [15.0, 5.0, 5.0],
    );
}

#[test]
fn rotate() {
    assert_bounds(
        &build("rect 20 10\nextrude 5\nrotate 90 axis=z"),
        [-5.0, -10.0, 0.0],
        [5.0, 10.0, 5.0],
    );
}

#[test]
fn scale() {
    assert_volume(&build("rect 10 10\nextrude 5\nscale 2"), 4000.0, 1.0e-6);
}

#[test]
fn split() {
    assert_volume(
        &build("rect 10 10\nextrude 10\nsplit XY offset=5 keep=below"),
        500.0,
        1.0e-6,
    );
}

#[test]
fn separate_bodies() {
    let model = build("rect 10 10\nextrude 10\nbody second\nrect 10 10 at=50,0\nextrude 10");
    assert_volume(&model, 2000.0, 1.0e-6);
}

#[test]
fn combine_bodies() {
    let model = build(
        "rect 20 20\nextrude 10\nbody b\nplane XY offset=-1\nrect 10 10 at=10,10\nextrude 12\ncombine main b mode=cut",
    );
    assert_volume(&model, 3750.0, 1.0e-6);
}

#[test]
fn mirror_feature() {
    let model = build(
        "rect 40 20\nbase: extrude 5\nplane base.end\ncircle 6 at=10,0\nboss: extrude 5\nmirror YZ of=boss",
    );
    assert_volume(&model, 4000.0 + 2.0 * PI * 9.0 * 5.0, 0.0005);
}

#[test]
#[ignore = "missing: repeat along="]
fn pattern_along_path() {
    let model = build(
        "rect 40 30\nbase: extrude 3\nplane base.end\nh: hole 3 -15,0\npath -15,0,3 15,0,3\nrepeat h along=path count=4",
    );
    assert_volume(&model, 3600.0 - 4.0 * PI * 2.25 * 3.0, 0.0002);
}

#[test]
fn transform_copy() {
    assert_volume(
        &build("rect 10 10\nextrude 5\nmove 20,0,0 copy"),
        1000.0,
        1.0e-6,
    );
}

#[test]
fn scale_unevenly() {
    let model = build("rect 10 10\nextrude 5\nscale 2,1,1");
    assert_bounds(&model, [-10.0, -5.0, 0.0], [10.0, 5.0, 5.0]);
}

#[test]
fn assembly_mate() {
    let model = build("rect 20 20\nextrude 10\nbody lid\nrect 20 20\nextrude 2\nplace lid on=>Z");
    assert_bounds(&model, [-10.0, -10.0, 0.0], [10.0, 10.0, 12.0]);
}

#[test]
fn interference() {
    let text = summary(
        "rect 20 20\nextrude 10\nbody b\nplane XY offset=-1\nrect 20 16 at=10,0\nextrude 12\nmeasure overlap main b",
    );
    assert!(text.contains("1600.000"), "{text}");
}
