mod common;

use common::*;
use linecad::select::{select_edges, select_faces};

#[test]
fn variables() {
    assert_volume(
        &build("let w=40 h=w*3/4 t=(w-h)/5\nrect w h\nextrude t"),
        40.0 * 30.0 * 2.0,
        1.0e-6,
    );
}

#[test]
fn groups_and_edges() {
    let model = build("rect 40 30\nbase: extrude 3\nplane base.end\nholes: hole 4 0,0");
    let solid = model.solid.as_ref().expect("a solid");
    let tolerance = model.tolerance();
    let count = |selector: &str| {
        select_edges(selector, solid, &model.groups, tolerance)
            .expect("selects")
            .len()
    };
    assert_eq!(count("base.end&base.side"), 4);
    assert_eq!(count("base.side&base.side"), 4);
    assert_eq!(count("holes.side&base.end"), 4);
    assert_eq!(count("base.end"), 8);
    assert_eq!(count("base.end&base.side|holes.side&base.start"), 8);
}

#[test]
fn directional_faces() {
    let model = build("rect 40 30\nbase: extrude 3\nplane base.end\nrect 10 10\nextrude 4");
    let solid = model.solid.as_ref().expect("a solid");
    let faces = |selector: &str| {
        select_faces(selector, solid, &model.groups, model.tolerance())
            .expect("selects")
            .len()
    };
    assert_eq!(faces(">Z"), 1);
    assert_eq!(faces("+Z"), 2);
    assert_eq!(faces("+X,-X"), 4);
    assert_eq!(faces("all"), 11);
}

#[test]
fn groups_survive_later_cuts() {
    let model = build(
        "rect 40 30\nbase: extrude 10\nplane base.end\nrect 10 10\ncut 3\nplane >X\nrect 4 4 at=0,5\ncut 2",
    );
    let solid = model.solid.as_ref().expect("a solid");
    let faces = select_faces("base.end", solid, &model.groups, model.tolerance()).expect("selects");
    assert_eq!(faces.len(), 1);
}

#[test]
fn errors() {
    assert!(failure("rect 10 10\nextrude 1\nfillet 1 nope.end").contains("known groups"));
    assert!(failure("bend 3").contains("unknown operation"));
    assert!(failure("rect w 10").contains("let w"));
    assert!(failure("rect 10 10\nplane XZ").contains("extrude or cut it"));
    assert!(failure("rect 10 10 r=5").contains("under half"));
    assert!(failure("rect 10 10\nextrude 2 depth=3").contains("no `depth` argument"));
    assert!(failure("cut 3").contains("needs a sketch"));
    assert!(failure("# not an operation").contains("not an operation"));
}
