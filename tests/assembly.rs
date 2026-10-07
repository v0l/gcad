mod common;

use common::scratch;
use linecad::model::{Model, run_path};

const BOX: &str = "rect 40 40\nextrude 10\nbody lid\nplane XY offset=10\nrect 40 40\nextrude 2\n";

fn files(test: &str, assembly: &str, parts: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(scratch(test));
    std::fs::create_dir_all(&dir).expect("dir");
    for (name, text) in parts {
        std::fs::write(dir.join(name), text).expect("part");
    }
    let path = dir.join("top.lasm");
    std::fs::write(&path, assembly).expect("assembly");
    path
}

fn steps(
    test: &str,
    assembly: &str,
    parts: &[(&str, &str)],
) -> (Model, Vec<Result<String, String>>) {
    let run = run_path(&files(test, assembly, parts), &[], None).expect("runs");
    let results = run
        .steps
        .iter()
        .map(|(_, r)| r.as_ref().map(Clone::clone).map_err(|e| format!("{e:#}")))
        .collect();
    (run.model, results)
}

fn built(test: &str, assembly: &str, parts: &[(&str, &str)]) -> (Model, String) {
    let (model, results) = steps(test, assembly, parts);
    match results.last() {
        Some(Ok(text)) => (model, text.clone()),
        Some(Err(error)) => panic!("{error}"),
        None => panic!("empty"),
    }
}

fn failed(test: &str, assembly: &str, parts: &[(&str, &str)]) -> String {
    match steps(test, assembly, parts).1.last() {
        Some(Err(error)) => error.clone(),
        other => panic!("expected a failure, got {other:?}"),
    }
}

const HINGE: &str = "part box box.lcad\naxis hinge -20,20,10 20,20,10\n";

#[test]
fn parts_from_files() {
    let (model, _) = built(
        "parts_from_files",
        "part box box.lcad\npart spare box.lcad body=lid\nmove spare 0,0,20",
        &[("box.lcad", BOX)],
    );
    assert_eq!(model.body_names(), ["box.main", "box.lid", "spare"]);
    let spare = linecad::geometry::bounds(&model.named_body("spare").expect("spare"));
    assert!((spare.min().z - 30.0).abs() < 1.0e-6, "{spare:?}");
}

#[test]
fn part_variables() {
    let (model, _) = built(
        "part_variables",
        "let size=20\npart plate plate.lcad w=size",
        &[("plate.lcad", "let w=10\nrect w w\nextrude 1")],
    );
    let plate = linecad::geometry::bounds(&model.named_body("plate").expect("plate"));
    assert!((plate.max().x - 10.0).abs() < 1.0e-6, "{plate:?}");
}

#[test]
fn hinge_opens_the_lid() {
    let (model, _) = built(
        "hinge_opens_the_lid",
        &format!("{HINGE}joint open box.lid box.main turn about=hinge min=-120 max=0 at=-90"),
        &[("box.lcad", BOX)],
    );
    let b = linecad::geometry::bounds(&model.named_body("box.lid").expect("lid"));
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
fn slide_moves_a_drawer() {
    let parts = [(
        "drawer.lcad",
        "rect 40 40\nextrude 10\nbody drawer\nplane XY offset=10\nrect 30 30\nextrude 5",
    )];
    let (model, _) = built(
        "slide_moves_a_drawer",
        "part chest drawer.lcad\njoint pull chest.drawer chest.main slide along=0,-1,0 min=0 max=30 at=25",
        &parts,
    );
    let b = linecad::geometry::bounds(&model.named_body("chest.drawer").expect("drawer"));
    assert!(
        (b.min().y + 40.0).abs() < 1.0e-6 && (b.max().y + 10.0).abs() < 1.0e-6,
        "{b:?}"
    );
    let error = failed(
        "slide_out_of_range",
        "part chest drawer.lcad\njoint pull chest.drawer chest.main slide along=0,-1,0 min=0 max=30 at=40",
        &parts,
    );
    assert!(error.contains("goes from 0 to 30"), "{error}");
}

#[test]
fn pose_moves_children() {
    let (model, _) = built(
        "pose_moves_children",
        &format!(
            "{HINGE}part knob knob.lcad\nmove knob 0,-10,12\njoint open box.lid box.main turn about=hinge min=-120 max=0\njoint fix knob box.lid slide along=0,0,1 min=0 max=0\npose open -90"
        ),
        &[("box.lcad", BOX), ("knob.lcad", "circle 6\nextrude 4")],
    );
    let b = linecad::geometry::bounds(&model.named_body("knob").expect("knob"));
    assert!(b.min().y > 21.9 && b.max().y < 26.1, "{b:?}");
}

#[test]
fn clear_assembly() {
    let (_, text) = built(
        "clear_assembly",
        &format!(
            "{HINGE}joint open box.lid box.main turn about=hinge min=-120 max=0 at=-90\ninterference none"
        ),
        &[("box.lcad", BOX)],
    );
    assert!(text.contains("no overlaps"), "{text}");
}

#[test]
fn sweep_finds_a_clash() {
    let (_, text) = built(
        "sweep_finds_a_clash",
        &format!(
            "{HINGE}joint open box.lid box.main turn about=hinge min=-60 max=60\ninterference joint=open steps=4"
        ),
        &[("box.lcad", BOX)],
    );
    assert!(
        text.contains("at 30.0°") && text.contains("box.main and box.lid"),
        "{text}"
    );
}

#[test]
fn strict_interference_fails() {
    let error = failed(
        "strict_interference_fails",
        &format!(
            "{HINGE}joint open box.lid box.main turn about=hinge min=0 max=60 at=45\ninterference none"
        ),
        &[("box.lcad", BOX)],
    );
    assert!(error.contains("overlap"), "{error}");
}

#[test]
fn geometry_stays_in_parts() {
    let error = failed(
        "geometry_stays_in_parts",
        "part box box.lcad\nrect 10 10",
        &[("box.lcad", BOX)],
    );
    assert!(error.contains("part (.lcad) file"), "{error}");
    let error = common::failure(
        "rect 10 10\nextrude 1\nbody b\nrect 5 5\nextrude 1\njoint j b main slide along=1,0,0",
    );
    assert!(error.contains("assembly (.lasm) file"), "{error}");
}

#[test]
fn part_errors_name_the_file() {
    let error = failed(
        "part_errors_name_the_file",
        "part bad bad.lcad",
        &[("bad.lcad", "rect 10 10\nfillet 1 all")],
    );
    assert!(error.contains("bad.lcad:2"), "{error}");
}

const PLATE: &str = "rect 40 30\nbase: extrude 5\nplane base.end\nhole: hole 4.2 10,5 -10,-5\n";
const PIN: &str = "pin: circle 4\npin: extrude 20\n";

fn centre(model: &Model, name: &str) -> [f64; 3] {
    let b = linecad::geometry::bounds(&model.named_body(name).expect("part"));
    let c = b.center();
    [c.x, c.y, c.z]
}

#[test]
fn concentric_pin_in_a_hole() {
    let (model, text) = built(
        "concentric_pin_in_a_hole",
        "part plate plate.lcad\npart pin pin.lcad\nrotate pin 90 axis=y\nmove pin 8,4,30\nconcentric pin:pin.side plate:hole.side\nflush pin:pin.start plate:base.end",
        &[("plate.lcad", PLATE), ("pin.lcad", PIN)],
    );
    let [x, y, z] = centre(&model, "pin");
    assert!(
        (x - 10.0).abs() < 1.0e-6 && (y - 5.0).abs() < 1.0e-6,
        "{x} {y} {text}"
    );
    assert!((z - 15.0).abs() < 1.0e-6 || (z + 5.0).abs() < 1.0e-6, "{z}");
}

#[test]
fn concentric_picks_the_hole_near_a_point() {
    let (model, _) = built(
        "concentric_picks_the_hole_near_a_point",
        "part plate plate.lcad\npart pin pin.lcad\nconcentric pin:pin.side plate:hole.side near=-10,-5,0\nflush pin:pin.start plate:base.end offset=1",
        &[("plate.lcad", PLATE), ("pin.lcad", PIN)],
    );
    let [x, y, z] = centre(&model, "pin");
    assert!(
        (x + 10.0).abs() < 1.0e-6 && (y + 5.0).abs() < 1.0e-6 && (z - 16.0).abs() < 1.0e-6,
        "{x} {y} {z}"
    );
}

#[test]
fn holes_line_up() {
    let parts = [("plate.lcad", PLATE)];
    let (_, text) = built(
        "holes_line_up",
        "part a plate.lcad\npart b plate.lcad\nmove b 0,0,5\naligned b:hole.side a:hole.side",
        &parts,
    );
    assert!(text.contains("2 hole(s) line up"), "{text}");
    let error = failed(
        "holes_do_not_line_up",
        "part a plate.lcad\npart b plate.lcad\nmove b 0.5,0,5\naligned b:hole.side a:hole.side",
        &parts,
    );
    assert!(error.contains("0.500 off"), "{error}");
}

const LID_WITH_HOLES: &str = "rect 40 40\nextrude 10\nplane XY offset=10\npilot: hole 2 15,15 depth=8\nbody lid\nplane XY offset=10\nrect 40 40\ntop: extrude 2\nplane top.end\nscrews: hole 2.5 15,15\n";
const SCREW: &str = "circle 4\nhead: extrude 1.5\ncircle 2\nshank: extrude -10\n";

#[test]
fn a_screw_locks_the_hinge() {
    let parts = [("box.lcad", LID_WITH_HOLES), ("screw.lcad", SCREW)];
    let screwed = format!(
        "{HINGE}joint open box.lid box.main turn about=hinge min=-120 max=0\npart screw screw.lcad\nconcentric screw:shank.side box.lid:screws.side\nflush screw:head.start box.lid:top.end\nconcentric screw:shank.side box.main:pilot.side\n"
    );
    let (model, text) = built("a_screw_locks_the_hinge", &screwed, &parts);
    assert!(text.contains("ties it to `box.main`"), "{text}");
    let b = linecad::geometry::bounds(&model.named_body("screw").expect("screw"));
    assert!(
        (b.min().z - 2.0).abs() < 1.0e-6 && (b.max().z - 13.5).abs() < 1.0e-6,
        "{b:?}"
    );
    let error = failed(
        "a_screw_locks_the_hinge_pose",
        &format!("{screwed}pose open -30"),
        &parts,
    );
    assert!(
        error.contains("pull apart") && error.contains("box.main:pilot.side"),
        "{error}"
    );
    let (model, _) = built(
        "an_unscrewed_lid_opens",
        &format!(
            "{HINGE}joint open box.lid box.main turn about=hinge min=-120 max=0\npart screw screw.lcad\nconcentric screw:shank.side box.lid:screws.side\npose open -90"
        ),
        &parts,
    );
    let b = linecad::geometry::bounds(&model.named_body("screw").expect("screw"));
    assert!(
        b.max().z - b.min().z < 4.1,
        "the screw turns with the lid: {b:?}"
    );
}

#[test]
fn a_sub_assembly_keeps_its_mates() {
    let parts = [
        ("box.lcad", LID_WITH_HOLES),
        ("screw.lcad", SCREW),
        (
            "boxed.lasm",
            "part box box.lcad\naxis hinge -20,20,10 20,20,10\njoint open box.lid box.main turn about=hinge min=-120 max=0\npart screw screw.lcad\nconcentric screw:shank.side box.lid:screws.side\nflush screw:head.start box.lid:top.end\nconcentric screw:shank.side box.main:pilot.side\n",
        ),
    ];
    let error = failed(
        "a_sub_assembly_keeps_its_mates",
        "part kit boxed.lasm\npose kit.open -30\n",
        &parts,
    );
    assert!(error.contains("pull apart"), "{error}");
}

const SLAB: &str = "rect 100 100\nplate: extrude 10\n";
const ROD: &str = "circle 5\nrod: extrude 40\n";

fn extent(
    model: &Model,
    part: &str,
) -> monstertruck::modeling::BoundingBox<monstertruck::modeling::Point3> {
    linecad::geometry::bounds(&model.named_body(part).expect("part"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-3
}

#[test]
fn tangent_lays_a_rod_on_a_plate() {
    let parts = [("plate.lcad", SLAB), ("rod.lcad", ROD)];
    let (model, text) = built(
        "tangent_lays_a_rod_on_a_plate",
        "part plate plate.lcad\npart rod rod.lcad\ntangent rod:rod.side plate:plate.end\n",
        &parts,
    );
    let b = extent(&model, "rod");
    assert!(
        near(b.min().z, 10.0) && near(b.max().z, 15.0),
        "{text} {b:?}"
    );
}

#[test]
fn distance_between_axes() {
    let parts = [("rod.lcad", ROD)];
    let (model, _) = built(
        "distance_between_axes",
        "part a rod.lcad\npart b rod.lcad\nmove b 30,0,0\ndistance b:rod.side a:rod.side 50\n",
        &parts,
    );
    let b = extent(&model, "b");
    assert!(near(b.min().x, 47.5) && near(b.max().x, 52.5), "{b:?}");
}

#[test]
fn parallel_and_angle_turn_parts() {
    let parts = [("plate.lcad", SLAB), ("rod.lcad", ROD)];
    let (model, _) = built(
        "parallel_turns_a_rod",
        "part a rod.lcad\npart b rod.lcad\nmove b 30,0,0\nrotate b 30 axis=x\nparallel b:rod.end a:rod.end\n",
        &parts,
    );
    let b = extent(&model, "b");
    assert!(near(b.max().z - b.min().z, 40.0), "{b:?}");
    let (model, text) = built(
        "angle_tilts_a_plate",
        "part base plate.lcad\npart lid plate.lcad\nmove lid 0,0,50\nangle lid:plate.end base:plate.end 30\n",
        &parts,
    );
    let b = extent(&model, "lid");
    let tilted = 100.0 * 0.5 + 10.0 * 3f64.sqrt() / 2.0;
    assert!(near(b.max().z - b.min().z, tilted), "{text} {b:?}");
}

#[test]
fn a_parallel_mate_stops_a_joint() {
    let parts = [("rod.lcad", ROD)];
    let assembly = "part a rod.lcad\npart b rod.lcad\npart c rod.lcad\nmove b 50,0,0\nmove c 0,30,0\naxis tilt 50,0,0 60,0,0\njoint t b a turn about=tilt\ndistance c:rod.side a:rod.side 20\nparallel c:rod.end b:rod.end\n";
    let (model, _) = built("a_parallel_mate_stops_a_joint", assembly, &parts);
    assert!(near(extent(&model, "c").min().y, 17.5));
    let error = failed(
        "a_parallel_mate_stops_a_joint_pose",
        &format!("{assembly}pose t 20\n"),
        &parts,
    );
    assert!(
        error.contains("pull apart parallel c:rod.end b:rod.end"),
        "{error}"
    );
    let error = failed(
        "a_second_mate_that_does_not_hold",
        "part a rod.lcad\npart b rod.lcad\nmove b 50,0,0\nrotate b 10 axis=x\npart c rod.lcad\nmove c 0,30,0\ndistance c:rod.side a:rod.side 20\nparallel c:rod.end b:rod.end\n",
        &parts,
    );
    assert!(error.contains("already hangs off `a`"), "{error}");
}

#[test]
fn a_rod_turns_and_slides_on_one_axis() {
    let parts = [("rod.lcad", ROD), ("plate.lcad", SLAB)];
    let assembly = "part plate plate.lcad\npart rod rod.lcad\naxis up 0,0,0 0,0,1\njoint spin rod plate turn about=up\njoint push rod plate slide along=0,0,1 min=0 max=20\npose push 10\npose spin 90\n";
    let (model, _) = built("a_rod_turns_and_slides", assembly, &parts);
    let b = extent(&model, "rod");
    assert!(near(b.min().z, 10.0) && near(b.max().z, 50.0), "{b:?}");
    let error = failed(
        "turns_and_slides_that_do_not_commute",
        "part plate plate.lcad\npart rod rod.lcad\naxis up 0,0,0 0,0,1\njoint spin rod plate turn about=up\njoint push rod plate slide along=1,0,0\n",
        &parts,
    );
    assert!(error.contains("depend on their order"), "{error}");
}

#[test]
fn coupled_joints_move_together() {
    let parts = [("rod.lcad", ROD), ("plate.lcad", SLAB)];
    let gears = "part frame plate.lcad\npart a rod.lcad\npart b rod.lcad\npart rack rod.lcad\nmove b 20,0,0\nmove rack 0,40,0\naxis ax 0,0,0 0,0,1\naxis bx 20,0,0 20,0,1\njoint ja a frame turn about=ax\njoint jb b frame turn about=bx min=-60 max=60\njoint slide rack frame slide along=1,0,0\ncouple jb ja ratio=-0.5\ncouple slide ja ratio=0.1\n";
    let (model, _) = built("coupled_joints", &format!("{gears}pose ja 120\n"), &parts);
    let value = |name: &str| {
        model
            .joints
            .iter()
            .find(|j| j.name == name)
            .expect("joint")
            .value
    };
    assert!(near(value("jb"), -60.0) && near(value("slide"), 12.0));
    assert!(near(extent(&model, "rack").min().x, 12.0 - 2.5));
    let error = failed(
        "driven_joint_posed",
        &format!("{gears}pose jb 10\n"),
        &parts,
    );
    assert!(error.contains("driven by `ja`"), "{error}");
    let error = failed(
        "driven_past_its_range",
        &format!("{gears}pose ja 150\n"),
        &parts,
    );
    assert!(error.contains("`jb` goes from -60 to 60"), "{error}");
}
