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

#[test]
fn revolve_cut() {
    let model = build(
        "rect 40 40\nbase: extrude 10\nplane XZ\nrect 4 4 at=10,10\nrevolve 360 axis=y mode=cut",
    );
    assert_volume(&model, 16000.0 - PI * (144.0 - 64.0) * 2.0, 0.0005);
}

#[test]
fn sweep_cut() {
    let model =
        build("rect 40 40\nbase: extrude 10\ncircle 4\npath -30,0,10 30,0,10\nsweep mode=cut");
    assert_volume(&model, 16000.0 - 2.0 * PI * 40.0, 0.0005);
}

#[test]
fn loft_cut() {
    let model = build(
        "rect 40 40\nbase: extrude 10\nplane base.end offset=1\nrect 22 22\nsection\nplane base.end offset=-5\nrect 10 10\nsection\nloft mode=cut",
    );
    assert_volume(&model, 16000.0 - frustum(400.0, 100.0, 5.0), 0.0005);
}

#[test]
fn thin_extrude() {
    assert_volume(
        &build("rect 40 30\nextrude 10 thin=2"),
        (1200.0 - 36.0 * 26.0) * 10.0,
        1.0e-6,
    );
}

#[test]
fn extrude_up_to_offset_face() {
    let model = build(
        "rect 40 30\nbase: extrude 10\nplane XY offset=20\nrect 10 10\nextrude upto=base.end offset=-2",
    );
    assert_volume(&model, 13000.0, 1.0e-6);
}

#[test]
fn extrude_to_next_face() {
    let model = build("rect 40 30\nbase: extrude 10\nplane XY offset=20\nrect 10 10\nextrude next");
    assert_volume(&model, 13000.0, 1.0e-6);
}

#[test]
#[ignore = "missing: rib"]
fn rib() {
    let model = build(
        "rect 40 20 at=20,0\nbase: extrude 2\nplane YZ\nrect 20 30 at=0,15\nwall: extrude 2\nplane XZ\npen 2,22\nline 22,2\nrib 2",
    );
    assert_volume(&model, 1600.0 + 1200.0 - 80.0 + 400.0, 0.0005);
}

#[test]
fn smooth_loft() {
    let model = build(
        "rect 20 20\nsection\nplane XY offset=10\nrect 10 10\nsection\nplane XY offset=20\nrect 20 20\nsection\nloft smooth",
    );
    let v = volume(&model);
    assert!(v > 2000.0 && v < 2.0 * frustum(400.0, 100.0, 10.0), "{v}");
}

#[test]
fn loft_mixed_profiles() {
    let model = build("circle 20\nsection\nplane XY offset=10\nngon 20 6\nsection\nloft");
    let v = volume(&model);
    assert!(
        v > 0.95 * frustum(PI * 100.0, 259.8, 10.0) && v < 1.05 * frustum(PI * 100.0, 259.8, 10.0),
        "{v}"
    );
}

#[test]
fn sweep_smooth_path() {
    let model = build("circle 2\npath 0,0,0 10,0,10 20,0,0 smooth\nsweep");
    let v = volume(&model);
    assert!(v > PI * 28.28 && v < PI * 40.0, "{v}");
}

#[test]
fn sweep_twist() {
    assert_volume(
        &build("rect 4 2\npath 0,0,0 0,0,20\nsweep twist=90"),
        160.0,
        0.002,
    );
}

#[test]
fn sweep_scale() {
    assert_volume(
        &build("circle 4\npath 0,0,0 0,0,10\nsweep scale=0.5"),
        PI / 3.0 * 10.0 * 7.0,
        0.002,
    );
}

#[test]
#[ignore = "missing: thread"]
fn modelled_thread() {
    let model = build("circle 6\nrod: extrude 10\nthread M6 on=rod.side");
    let v = volume(&model);
    assert!(v < PI * 9.0 * 10.0 && v > PI * 2.4 * 2.4 * 10.0, "{v}");
}

#[test]
#[ignore = "missing: shell with several openings"]
fn shell_two_openings() {
    assert_volume(
        &build("rect 40 30\nbase: extrude 20\nshell 2 open=base.end,base.start"),
        (1200.0 - 936.0) * 20.0,
        1.0e-6,
    );
}

#[test]
#[ignore = "missing: shell of non-extrusions"]
fn shell_revolved() {
    let model = build(
        "plane XZ\npen 0,0\nline 10,0\narc 0,10 center=0,0\nclose\ndome: revolve 360\nshell 1",
    );
    assert_volume(&model, 2.0 / 3.0 * PI * (1000.0 - 729.0), 0.002);
}

#[test]
#[ignore = "missing: draft curved faces"]
fn draft_curved() {
    let top = 10.0 - 10.0 * 5.0_f64.to_radians().tan();
    let model = build("circle 20\nbase: extrude 10\ndraft 5 base.side neutral=base.start");
    assert_volume(
        &model,
        PI / 3.0 * 10.0 * (100.0 + top * top + 10.0 * top),
        0.001,
    );
}

#[test]
#[ignore = "missing: push in with curved sides"]
fn push_curved() {
    assert_volume(
        &build("circle 20\nbase: extrude 10\npush base.end -2"),
        PI * 100.0 * 8.0,
        0.0005,
    );
}

#[test]
#[ignore = "missing: wrap"]
fn wrap_text() {
    let model = build("circle 20\nbase: extrude 20\ntext HI size=8\nwrap base.side depth=0.5");
    assert!(volume(&model) < PI * 100.0 * 20.0);
}

#[test]
#[ignore = "missing: thicken"]
fn thicken_face() {
    assert_volume(
        &build("circle 20\nbase: extrude 10\nthicken base.side 1"),
        PI * 121.0 * 10.0,
        0.001,
    );
}

#[test]
fn stack_same_size() {
    assert_volume(
        &build("rect 20 20\nbase: extrude 10\nplane base.end\nrect 20 20\nextrude 5"),
        6000.0,
        1.0e-6,
    );
}

#[test]
fn notch_flush_with_sides() {
    assert_volume(
        &build("rect 20 20\nbase: extrude 10\nplane base.end\nrect 10 20 at=5,0\ncut 5"),
        3000.0,
        1.0e-6,
    );
}

#[test]
#[ignore = "missing: coplanar booleans"]
fn step_flush_with_sides() {
    assert_volume(
        &build("rect 20 20\nbase: extrude 10\nplane base.end\nrect 20 10 at=0,5\nextrude 5"),
        5000.0,
        1.0e-6,
    );
}
