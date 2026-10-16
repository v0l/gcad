mod common;

use common::*;
use std::f64::consts::PI;

#[test]
fn thru_cut_across_thin_sheet_rows() {
    for z0 in [2265.0, 2300.0] {
        let source = format!(
            "let z0={z0}\nplane YZ offset=330\npoly -38,z0 -38,z0+60 -69.5,z0+80 -69.5,z0+120 -38,z0+140 -38,z0+200 -38.5,z0+200 -38.5,z0+140 -70,z0+120 -70,z0+80 -38.5,z0+60 -38.5,z0\nrib: extrude 4320\nrepeat rib count=2 step=0,200\nplane XZ offset=100\npoly 320,2596 4660,2488 4660,3500 320,3500\ncut thru"
        );
        let line_at = |x: f64| 2596.0 - 108.0 * (x - 320.0) / 4340.0;
        let mean_height = (line_at(330.0) + line_at(4650.0)) / 2.0 - z0;
        assert_volume(&build(&source), 0.5 * 4320.0 * mean_height, 1.0e-6);
    }
}

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
fn split_either_side() {
    for (keep, kept, x) in [("above", 350.0, 3.0), ("below", 650.0, 3.0)] {
        let model = build(&format!(
            "rect 20 10\nextrude 5\ns: split on=YZ offset=3 keep={keep}"
        ));
        assert_volume(&model, kept, 1.0e-6);
        let solid = model.solid.as_ref().expect("solid");
        let faces = gcad::select::select_faces("s.cut", solid, &model.groups, model.tolerance())
            .expect("selects");
        let all = gcad::select::faces(solid);
        assert_eq!(faces.len(), 1, "{keep}");
        assert!(
            all[faces[0]]
                .vertex_iter()
                .all(|v| (v.point().x - x).abs() < 1.0e-9)
        );
    }
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

#[test]
fn check_sends_separate_parts_to_an_assembly() {
    let check = |name: &str, source: &str| {
        let path = scratch(name);
        std::fs::write(&path, source).expect("written");
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
            .env("GCAD_NO_PART_CACHE", "1")
            .args(["check", &path])
            .output()
            .expect("gcad runs");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let two = check(
        "two-parts.gcad",
        "rect 10 10\nextrude 5\nbody lid\nplane XY offset=5\nrect 10 10\nextrude 1\n",
    );
    assert!(
        two.contains("2 separate bodies (main, lid)") && two.contains(".gasm"),
        "{two}"
    );
    let one = check(
        "one-part.gcad",
        "rect 10 10\nextrude 5\nbody b\nrect 4 4 at=10,0\nextrude 8\ncombine main b\n",
    );
    assert!(!one.contains("separate bodies"), "{one}");
}
