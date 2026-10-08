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
fn cut_from_inside_a_cylinder() {
    let model = build(
        "plane YZ\ncircle 24\nb: extrude 65\nplane XY offset=-10\nrect 70 30 at=32.5,0\ncut 2",
    );
    let segment = 144.0 * (10.0_f64 / 12.0).acos() - 10.0 * 44.0_f64.sqrt();
    assert_bounds(&model, [0.0, -12.0, -10.0], [65.0, 12.0, 12.0]);
    assert_volume(&model, (PI * 144.0 - segment) * 65.0, 1.0e-4);
}

#[test]
fn cut_beside_a_taller_block() {
    assert_volume(
        &build(
            "rect 40 30\nbase: extrude 10\nrect 10 30 at=15,0\nextrude 20\nplane base.end\nrect 20 10 at=10,0\ncut 4",
        ),
        14200.0,
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
fn rib() {
    let model = build(
        "rect 40 20 at=20,0\nbase: extrude 2\nplane YZ offset=2\nrect 16 29 at=0,15.5\nwall: extrude 2\nplane XZ\npen 4,22\nline 24,2\nrib 2",
    );
    assert_volume(&model, 1600.0 + 928.0 - 32.0 + 400.0, 0.0005);
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

fn threaded_section(pitch: f64, radius: f64, external: bool) -> f64 {
    threaded_slice(pitch, radius, external, 2_000)
}

fn threaded_volume(pitch: f64, length: f64, external: bool, reach: impl Fn(f64) -> f64) -> f64 {
    let slices = 2_000;
    (0..slices)
        .map(|i| {
            let z = length * (i as f64 + 0.5) / slices as f64;
            threaded_slice(pitch, reach(z), external, 4_000)
        })
        .sum::<f64>()
        * length
        / slices as f64
}

fn threaded_slice(pitch: f64, radius: f64, external: bool, steps: usize) -> f64 {
    let crest = 3.0;
    let root = crest - 5.0 / 8.0 * 3.0_f64.sqrt() / 2.0 * pitch;
    let profile = |s: f64| {
        let s = s / pitch;
        let r = if s < 1.0 / 8.0 {
            crest
        } else if s < 7.0 / 16.0 {
            crest + (root - crest) * (s - 1.0 / 8.0) / (5.0 / 16.0)
        } else if s < 11.0 / 16.0 {
            root
        } else {
            root + (crest - root) * (s - 11.0 / 16.0) / (5.0 / 16.0)
        };
        if external {
            r.min(radius)
        } else {
            r.max(radius)
        }
    };
    let mean = (0..steps)
        .map(|i| profile(pitch * (i as f64 + 0.5) / steps as f64).powi(2))
        .sum::<f64>()
        / steps as f64;
    PI * mean
}

fn thread_turns(model: &gcad::model::Model) -> Vec<f64> {
    use monstertruck::modeling::*;
    let solid = &model.solids()[0];
    let mut turns = Vec::new();
    for edge in solid.boundaries()[0].edge_iter() {
        let (front, back) = (edge.front().point(), edge.back().point());
        if (back.z - front.z).abs() < 1.0 {
            continue;
        }
        let curve = edge.oriented_curve();
        let (t0, t1) = curve.range_tuple();
        let (p, q) = (curve.subs(t0), curve.subs(t0 + (t1 - t0) * 0.01));
        let swept = (p.x * q.y - p.y * q.x) * (q.z - p.z).signum();
        turns.push(swept);
    }
    turns
}

#[test]
fn modelled_thread() {
    assert_volume(
        &build("circle 6\nrod: extrude 3\nthread M6 on=rod.side"),
        3.0 * threaded_section(1.0, 3.0, true),
        2.0e-4,
    );
}

#[test]
fn thread_turns_right_handed() {
    let turns = thread_turns(&build("circle 6\nrod: extrude 3\nthread M6 on=rod.side"));
    assert!(
        !turns.is_empty() && turns.iter().all(|t| *t > 0.0),
        "{turns:?}"
    );
    let turns = thread_turns(&build(
        "circle 6\nrod: extrude 3\nthread M6 on=rod.side left",
    ));
    assert!(
        !turns.is_empty() && turns.iter().all(|t| *t < 0.0),
        "{turns:?}"
    );
}

#[test]
fn thread_on_an_undersized_rod_with_a_fine_pitch() {
    assert_volume(
        &build("circle 5.9\nrod: extrude 3\nthread M6 on=rod.side pitch=0.75"),
        3.0 * threaded_section(0.75, 2.95, true),
        2.0e-4,
    );
}

#[test]
fn tapped_thread() {
    assert_volume(
        &build(
            "rect 20 20\nbase: extrude 4\nplane base.end\nh: hole 5 0,0 thread=M6\nthread M6 on=h.side",
        ),
        1600.0 - 4.0 * threaded_section(1.0, 2.5, false),
        2.0e-5,
    );
}

#[test]
fn thread_up_to_a_bolt_head() {
    let head = 3.0 * 3.0_f64.sqrt() / 2.0 * 25.0 * 4.0;
    assert_volume(
        &build(
            "ngon 10 6\nhead: extrude 4\nplane head.end\ncircle 6\nshank: extrude 4\nthread M6 on=shank.side",
        ),
        head + 4.0 * threaded_section(1.0, 3.0, true),
        2.0e-4,
    );
}

#[test]
fn thread_runs_out_into_chamfers() {
    assert_volume(
        &build(
            "circle 6\nrod: extrude 6\nchamfer 1 rod.end&rod.side|rod.start&rod.side\nthread M6 on=rod.side",
        ),
        threaded_volume(1.0, 6.0, true, |z| 3.0_f64.min(2.0 + z).min(8.0 - z)),
        2.0e-4,
    );
}

#[test]
fn left_hand_thread_runs_out_into_a_chamfer() {
    assert_volume(
        &build(
            "circle 5.9\nrod: extrude 6\nchamfer 0.8 rod.end&rod.side\nthread M6 on=rod.side left pitch=0.75",
        ),
        threaded_volume(0.75, 6.0, true, |z| 2.95_f64.min(8.15 - z)),
        2.0e-4,
    );
}

#[test]
fn tapped_thread_under_a_countersink() {
    assert_volume(
        &build(
            "rect 20 20\nbase: extrude 4\nplane base.end\nh: hole 5 0,0 csink=7,90 thread=M6\nthread M6 on=h.side",
        ),
        1600.0 - threaded_volume(1.0, 4.0, false, |z| 2.5_f64.max(z - 0.5)),
        2.0e-5,
    );
}

#[test]
fn thread_needs_a_round_face_that_fits() {
    assert!(
        failure("circle 6\nrod: extrude 3\nchamfer 0.3 rod.end&rod.side\nthread M6 on=rod.side")
            .contains("past the thread's root")
    );
    assert!(
        failure("circle 6\nrod: extrude 3\nfillet 1 rod.end&rod.side\nthread M6 on=rod.side")
            .contains("not a round")
    );
    assert!(failure("circle 8\nrod: extrude 3\nthread M6 on=rod.side").contains("rod between"));
    assert!(failure("rect 6 6\nbox: extrude 3\nthread M6 on=box.side").contains("round face"));
    assert!(failure("circle 6\nrod: extrude 3\nthread M7 on=rod.side").contains("unknown thread"));
    assert!(
        failure("circle 6\nrod: extrude 3\nthread M6 on=rod.side\nchamfer 0.5 rod.end")
            .contains("unchanged")
    );
}

#[test]
fn shell_two_openings() {
    assert_volume(
        &build("rect 40 30\nbase: extrude 20\nshell 2 open=base.end,base.start"),
        (1200.0 - 936.0) * 20.0,
        1.0e-6,
    );
}

#[test]
fn shell_revolved() {
    let model = build(
        "plane XZ\npen 0,0\nline 10,0\narc 0,10 center=0,0\nclose\ndome: revolve 360\nshell 1 open=dome.caps",
    );
    assert_volume(&model, 2.0 / 3.0 * PI * (1000.0 - 729.0), 0.002);
}

#[test]
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
fn push_curved() {
    assert_volume(
        &build("circle 20\nbase: extrude 10\npush base.end -2"),
        PI * 100.0 * 8.0,
        0.0005,
    );
}

#[test]
fn wrap_text() {
    let model = build(
        "circle 20\nbase: extrude 20\nplane XZ\ntext HI size=8 at=-5,6\nwrap base.side depth=0.5",
    );
    assert!(volume(&model) < volume(&build("circle 20\nbase: extrude 20")) - 1.0);
}

#[test]
fn wrap_rectangle() {
    let plain = volume(&build("circle 40\nbase: extrude 20"));
    let cut = volume(&build(
        "circle 40\nbase: extrude 20\nplane XZ\nrect 10 6 at=0,10\nwrap base.side depth=1",
    ));
    let raised = volume(&build(
        "circle 40\nbase: extrude 20\nplane XZ\nrect 10 6 at=0,10\nwrap base.side depth=1 raise",
    ));
    let angle = 10.0 / 20.0;
    let removed = 0.5 * angle * (400.0 - 361.0) * 6.0;
    let added = 0.5 * angle * (441.0 - 400.0) * 6.0;
    assert!(
        ((plain - cut) - removed).abs() < removed * 0.02,
        "{}",
        plain - cut
    );
    assert!(
        ((raised - plain) - added).abs() < added * 0.02,
        "{}",
        raised - plain
    );
    assert!(
        failure("circle 40\nbase: extrude 20\nplane XY\nrect 4 4\nwrap base.side depth=1")
            .contains("along the cylinder's axis")
    );
}

#[test]
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
fn step_flush_with_sides() {
    assert_volume(
        &build("rect 20 20\nbase: extrude 10\nplane base.end\nrect 20 10 at=0,5\nextrude 5"),
        5000.0,
        1.0e-6,
    );
}

#[test]
fn shell_follows_rounded_corners() {
    assert_volume(
        &build("rect 40 30\nbase: extrude 20\nfillet 3 base.side&base.side\nshell 2 open=base.end"),
        (1200.0 - (4.0 - PI) * 9.0) * 20.0 - (36.0 * 26.0 - (4.0 - PI)) * 18.0,
        1.0e-4,
    );
}

#[test]
fn shell_as_thick_as_the_rounding() {
    assert_volume(
        &build("rect 40 30\nbase: extrude 20\nfillet 2 base.side&base.side\nshell 2 open=base.end"),
        (1200.0 - (4.0 - PI) * 4.0) * 20.0 - 36.0 * 26.0 * 18.0,
        1.0e-4,
    );
}

#[test]
fn shell_follows_a_rounded_floor() {
    let outside = 24000.0 - 140.0 * SPANDREL * 4.0 + 4.0 * ROUND_CORNER_OVERLAP * 8.0;
    let inside = 38.0 * 28.0 * 19.0 - 132.0 * SPANDREL + 4.0 * ROUND_CORNER_OVERLAP;
    assert_volume(
        &build(
            "rect 40 30\nbase: extrude 20\nfillet 2 base.start&base.side\nshell 1 open=base.end",
        ),
        outside - inside,
        1.0e-4,
    );
}
