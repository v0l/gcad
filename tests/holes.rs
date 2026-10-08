mod common;

use common::*;
use std::f64::consts::PI;

const BLOCK: &str = "rect 40 30\nbase: extrude 10\nplane base.end\n";

#[test]
fn through() {
    assert_volume(
        &build(&format!("{BLOCK}hole 4 0,0")),
        12000.0 - PI * 4.0 * 10.0,
        0.0002,
    );
}

#[test]
fn several() {
    let model = build(&format!("{BLOCK}hole 4 10,5 -10,5 -10,-5 10,-5"));
    assert_volume(&model, 12000.0 - 4.0 * PI * 4.0 * 10.0, 0.0002);
}

#[test]
fn blind() {
    assert_volume(
        &build(&format!("{BLOCK}hole 4 0,0 depth=5")),
        12000.0 - PI * 4.0 * 5.0,
        0.0002,
    );
}

#[test]
fn counterbore() {
    let model = build(&format!("{BLOCK}hole 3.2 0,0 cbore=6,3"));
    assert_volume(
        &model,
        12000.0 - PI * 1.6 * 1.6 * 7.0 - PI * 9.0 * 3.0,
        0.0002,
    );
}

#[test]
fn countersink() {
    let cone = PI * 1.6 / 3.0 * (3.2 * 3.2 + 3.2 * 1.6 + 1.6 * 1.6) - PI * 1.6 * 1.6 * 1.6;
    let model = build(&format!("{BLOCK}hole 3.2 0,0 csink=6.4,90"));
    assert_volume(&model, 12000.0 - PI * 1.6 * 1.6 * 10.0 - cone, 0.0002);
}

#[test]
fn threaded() {
    let model = build(&format!("{BLOCK}tapped: hole 3.3 0,0 thread=M4"));
    assert_volume(&model, 12000.0 - PI * 1.65 * 1.65 * 10.0, 0.0002);
}

#[test]
fn angled() {
    let model = build("rect 40 30\nbase: extrude 10\nplane >Z rx=30\nhole 4 0,0");
    assert_volume(
        &model,
        12000.0 - PI * 4.0 * 10.0 / 30.0_f64.to_radians().cos(),
        0.0005,
    );
}

#[test]
fn on_a_curved_face() {
    let model = build("circle 20\nrod: extrude 20\nhole 4 0,10 on=rod.side");
    assert_volume(&model, PI * 100.0 * 20.0 - PI * 4.0 * 20.0, 0.002);
}

#[test]
fn hole_rims_are_exact_circles() {
    use monstertruck::modeling::*;
    let model = build(&format!("{BLOCK}hole 4 10,0 -10,0"));
    let solid = &model.solids()[0];
    let mut rims = 0;
    for edge in solid.boundaries()[0].edge_iter() {
        if matches!(edge.curve(), Curve::Line(_)) {
            continue;
        }
        let curve = edge.curve();
        let (t0, t1) = curve.range_tuple();
        let centre_x = 10.0 * edge.front().point().x.signum();
        let worst = (0..=40)
            .map(|i| {
                let p = curve.subs(t0 + (t1 - t0) * i as f64 / 40.0);
                ((p.x - centre_x).hypot(p.y) - 2.0).abs()
            })
            .fold(0.0, f64::max);
        assert!(worst < 1.0e-9, "rim strays {worst} from its circle");
        assert!(matches!(curve, Curve::NurbsCurve(_)));
        rims += 1;
    }
    assert!(rims >= 8 * 2, "{rims}");
}
