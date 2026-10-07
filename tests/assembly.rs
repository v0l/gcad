mod common;

use common::*;

const HINGED: &str = "rect 40 40\nextrude 10\nbody lid\nplane XY offset=10\nrect 40 40\nextrude 2\naxis hinge -20,20,10 20,20,10\n";

#[test]
fn hinge_opens_the_lid() {
    let model = build(&format!(
        "{HINGED}joint open lid main turn about=hinge min=-120 max=0 at=-90"
    ));
    let lid = model.named_body("lid").expect("lid");
    let b = linecad::geometry::bounds(&lid);
    assert!(
        (b.min().z - 10.0).abs() < 1.0e-6 && (b.max().z - 50.0).abs() < 1.0e-6,
        "{b:?}"
    );
    assert!(
        (b.min().y - 20.0).abs() < 1.0e-6 && (b.max().y - 22.0).abs() < 1.0e-6,
        "{b:?}"
    );
}

#[test]
fn pose_moves_children() {
    let model = build(&format!(
        "{HINGED}joint open lid main turn about=hinge min=-120 max=0\nbody knob\nplane XY offset=12\ncircle 6 at=0,-10\nextrude 4\njoint fix knob lid slide along=0,0,1 min=0 max=0\npose open -90"
    ));
    let knob = model.named_body("knob").expect("knob");
    let b = linecad::geometry::bounds(&knob);
    assert!(b.min().y > 21.9 && b.max().y < 26.1, "{b:?}");
}

#[test]
fn clear_assembly() {
    let text = summary(&format!(
        "{HINGED}joint open lid main turn about=hinge min=-120 max=0 at=-90\ninterference none"
    ));
    assert!(text.contains("no overlaps"), "{text}");
}

#[test]
fn sweep_finds_a_clash() {
    let text = summary(&format!(
        "{HINGED}joint open lid main turn about=hinge min=-60 max=60\ninterference joint=open steps=4"
    ));
    assert!(
        text.contains("at 30.0°") && text.contains("lid and main") || text.contains("main and lid"),
        "{text}"
    );
}

#[test]
fn strict_interference_fails() {
    let error = failure(&format!(
        "{HINGED}joint open lid main turn about=hinge min=0 max=60 at=45\ninterference none"
    ));
    assert!(error.contains("overlap"), "{error}");
}

#[test]
fn slide_moves_a_drawer() {
    let model = build(
        "rect 40 40\nextrude 10\nbody drawer\nplane XY offset=10\nrect 30 30\nextrude 5\njoint pull drawer main slide along=0,-1,0 min=0 max=30 at=25",
    );
    let b = linecad::geometry::bounds(&model.named_body("drawer").expect("drawer"));
    assert!(
        (b.min().y + 40.0).abs() < 1.0e-6 && (b.max().y + 10.0).abs() < 1.0e-6,
        "{b:?}"
    );
    assert!(failure("rect 40 40\nextrude 10\nbody drawer\nplane XY offset=10\nrect 30 30\nextrude 5\njoint pull drawer main slide along=0,-1,0 min=0 max=30 at=40").contains("goes from 0 to 30"));
}
