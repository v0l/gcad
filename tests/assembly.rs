mod common;

use common::scratch;
use gcad::model::{Model, run_path};

const BOX: &str = "rect 40 40\nextrude 10\nbody lid\nplane XY offset=10\nrect 40 40\nextrude 2\n";

fn files(test: &str, assembly: &str, parts: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(scratch(test));
    std::fs::create_dir_all(&dir).expect("dir");
    for (name, text) in parts {
        std::fs::write(dir.join(name), text).expect("part");
    }
    let path = dir.join("top.gasm");
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

const HINGE: &str = "part box box.gcad\naxis hinge -20,20,10 20,20,10\n";

#[test]
fn parts_from_files() {
    let (model, _) = built(
        "parts_from_files",
        "part box box.gcad\npart spare box.gcad body=lid\nmove spare 0,0,20",
        &[("box.gcad", BOX)],
    );
    assert_eq!(model.body_names(), ["box.main", "box.lid", "spare"]);
    let spare = gcad::geometry::bounds(&model.named_body("spare").expect("spare"));
    assert!((spare.min().z - 30.0).abs() < 1.0e-6, "{spare:?}");
}

#[test]
fn part_variables() {
    let (model, _) = built(
        "part_variables",
        "let size=20\npart plate plate.gcad w=size",
        &[("plate.gcad", "let w=10\nrect w w\nextrude 1")],
    );
    let plate = gcad::geometry::bounds(&model.named_body("plate").expect("plate"));
    assert!((plate.max().x - 10.0).abs() < 1.0e-6, "{plate:?}");
}

#[test]
fn hinge_opens_the_lid() {
    let (model, _) = built(
        "hinge_opens_the_lid",
        &format!("{HINGE}joint open box.lid box.main turn about=hinge min=-120 max=0 at=-90"),
        &[("box.gcad", BOX)],
    );
    let b = gcad::geometry::bounds(&model.named_body("box.lid").expect("lid"));
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
        "drawer.gcad",
        "rect 40 40\nextrude 10\nbody drawer\nplane XY offset=10\nrect 30 30\nextrude 5",
    )];
    let (model, _) = built(
        "slide_moves_a_drawer",
        "part chest drawer.gcad\njoint pull chest.drawer chest.main slide along=0,-1,0 min=0 max=30 at=25",
        &parts,
    );
    let b = gcad::geometry::bounds(&model.named_body("chest.drawer").expect("drawer"));
    assert!(
        (b.min().y + 40.0).abs() < 1.0e-6 && (b.max().y + 10.0).abs() < 1.0e-6,
        "{b:?}"
    );
    let error = failed(
        "slide_out_of_range",
        "part chest drawer.gcad\njoint pull chest.drawer chest.main slide along=0,-1,0 min=0 max=30 at=40",
        &parts,
    );
    assert!(error.contains("goes from 0 to 30"), "{error}");
}

#[test]
fn pose_moves_children() {
    let (model, _) = built(
        "pose_moves_children",
        &format!(
            "{HINGE}part knob knob.gcad\nmove knob 0,-10,12\njoint open box.lid box.main turn about=hinge min=-120 max=0\njoint fix knob box.lid slide along=0,0,1 min=0 max=0\npose open -90"
        ),
        &[("box.gcad", BOX), ("knob.gcad", "circle 6\nextrude 4")],
    );
    let b = gcad::geometry::bounds(&model.named_body("knob").expect("knob"));
    assert!(b.min().y > 21.9 && b.max().y < 26.1, "{b:?}");
}

#[test]
fn clear_assembly() {
    let (_, text) = built(
        "clear_assembly",
        &format!(
            "{HINGE}joint open box.lid box.main turn about=hinge min=-120 max=0 at=-90\ninterference none"
        ),
        &[("box.gcad", BOX)],
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
        &[("box.gcad", BOX)],
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
        &[("box.gcad", BOX)],
    );
    assert!(error.contains("overlap"), "{error}");
}

#[test]
fn geometry_stays_in_parts() {
    let error = failed(
        "geometry_stays_in_parts",
        "part box box.gcad\nrect 10 10",
        &[("box.gcad", BOX)],
    );
    assert!(error.contains("part (.gcad) file"), "{error}");
    let error = common::failure(
        "rect 10 10\nextrude 1\nbody b\nrect 5 5\nextrude 1\njoint j b main slide along=1,0,0",
    );
    assert!(error.contains("assembly (.gasm) file"), "{error}");
}

#[test]
fn part_errors_name_the_file() {
    let error = failed(
        "part_errors_name_the_file",
        "part bad bad.gcad",
        &[("bad.gcad", "rect 10 10\nfillet 1 all")],
    );
    assert!(error.contains("bad.gcad:2"), "{error}");
}

const PLATE: &str = "rect 40 30\nbase: extrude 5\nplane base.end\nhole: hole 4.2 10,5 -10,-5\n";
const PIN: &str = "pin: circle 4\npin: extrude 20\n";

fn centre(model: &Model, name: &str) -> [f64; 3] {
    let b = gcad::geometry::bounds(&model.named_body(name).expect("part"));
    let c = b.center();
    [c.x, c.y, c.z]
}

#[test]
fn concentric_pin_in_a_hole() {
    let (model, text) = built(
        "concentric_pin_in_a_hole",
        "part plate plate.gcad\npart pin pin.gcad\nrotate pin 90 axis=y\nmove pin 8,4,30\nconcentric pin:pin.side plate:hole.side\nflush pin:pin.start plate:base.end",
        &[("plate.gcad", PLATE), ("pin.gcad", PIN)],
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
        "part plate plate.gcad\npart pin pin.gcad\nconcentric pin:pin.side plate:hole.side near=-10,-5,0\nflush pin:pin.start plate:base.end offset=1",
        &[("plate.gcad", PLATE), ("pin.gcad", PIN)],
    );
    let [x, y, z] = centre(&model, "pin");
    assert!(
        (x + 10.0).abs() < 1.0e-6 && (y + 5.0).abs() < 1.0e-6 && (z - 16.0).abs() < 1.0e-6,
        "{x} {y} {z}"
    );
}

#[test]
fn holes_line_up() {
    let parts = [("plate.gcad", PLATE)];
    let (_, text) = built(
        "holes_line_up",
        "part a plate.gcad\npart b plate.gcad\nmove b 0,0,5\naligned b:hole.side a:hole.side",
        &parts,
    );
    assert!(text.contains("2 hole(s) line up"), "{text}");
    let error = failed(
        "holes_do_not_line_up",
        "part a plate.gcad\npart b plate.gcad\nmove b 0.5,0,5\naligned b:hole.side a:hole.side",
        &parts,
    );
    assert!(error.contains("0.500 off"), "{error}");
}

const LID_WITH_HOLES: &str = "rect 40 40\nextrude 10\nplane XY offset=10\npilot: hole 2 15,15 depth=8\nbody lid\nplane XY offset=10\nrect 40 40\ntop: extrude 2\nplane top.end\nscrews: hole 2.5 15,15\n";
const SCREW: &str = "circle 4\nhead: extrude 1.5\ncircle 2\nshank: extrude -10\n";

#[test]
fn a_screw_locks_the_hinge() {
    let parts = [("box.gcad", LID_WITH_HOLES), ("screw.gcad", SCREW)];
    let screwed = format!(
        "{HINGE}joint open box.lid box.main turn about=hinge min=-120 max=0\npart screw screw.gcad\nconcentric screw:shank.side box.lid:screws.side\nflush screw:head.start box.lid:top.end\nconcentric screw:shank.side box.main:pilot.side\n"
    );
    let (model, text) = built("a_screw_locks_the_hinge", &screwed, &parts);
    assert!(text.contains("ties it to `box.main`"), "{text}");
    let b = gcad::geometry::bounds(&model.named_body("screw").expect("screw"));
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
            "{HINGE}joint open box.lid box.main turn about=hinge min=-120 max=0\npart screw screw.gcad\nconcentric screw:shank.side box.lid:screws.side\npose open -90"
        ),
        &parts,
    );
    let b = gcad::geometry::bounds(&model.named_body("screw").expect("screw"));
    assert!(
        b.max().z - b.min().z < 4.1,
        "the screw turns with the lid: {b:?}"
    );
}

#[test]
fn a_sub_assembly_keeps_its_mates() {
    let parts = [
        ("box.gcad", LID_WITH_HOLES),
        ("screw.gcad", SCREW),
        (
            "boxed.gasm",
            "part box box.gcad\naxis hinge -20,20,10 20,20,10\njoint open box.lid box.main turn about=hinge min=-120 max=0\npart screw screw.gcad\nconcentric screw:shank.side box.lid:screws.side\nflush screw:head.start box.lid:top.end\nconcentric screw:shank.side box.main:pilot.side\n",
        ),
    ];
    let error = failed(
        "a_sub_assembly_keeps_its_mates",
        "part kit boxed.gasm\npose kit.open -30\n",
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
    gcad::geometry::bounds(&model.named_body(part).expect("part"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-3
}

#[test]
fn tangent_lays_a_rod_on_a_plate() {
    let parts = [("plate.gcad", SLAB), ("rod.gcad", ROD)];
    let (model, text) = built(
        "tangent_lays_a_rod_on_a_plate",
        "part plate plate.gcad\npart rod rod.gcad\ntangent rod:rod.side plate:plate.end\n",
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
    let parts = [("rod.gcad", ROD)];
    let (model, _) = built(
        "distance_between_axes",
        "part a rod.gcad\npart b rod.gcad\nmove b 30,0,0\ndistance b:rod.side a:rod.side 50\n",
        &parts,
    );
    let b = extent(&model, "b");
    assert!(near(b.min().x, 47.5) && near(b.max().x, 52.5), "{b:?}");
}

#[test]
fn parallel_and_angle_turn_parts() {
    let parts = [("plate.gcad", SLAB), ("rod.gcad", ROD)];
    let (model, _) = built(
        "parallel_turns_a_rod",
        "part a rod.gcad\npart b rod.gcad\nmove b 30,0,0\nrotate b 30 axis=x\nparallel b:rod.end a:rod.end\n",
        &parts,
    );
    let b = extent(&model, "b");
    assert!(near(b.max().z - b.min().z, 40.0), "{b:?}");
    let (model, text) = built(
        "angle_tilts_a_plate",
        "part base plate.gcad\npart lid plate.gcad\nmove lid 0,0,50\nangle lid:plate.end base:plate.end 30\n",
        &parts,
    );
    let b = extent(&model, "lid");
    let tilted = 100.0 * 0.5 + 10.0 * 3f64.sqrt() / 2.0;
    assert!(near(b.max().z - b.min().z, tilted), "{text} {b:?}");
}

#[test]
fn a_parallel_mate_stops_a_joint() {
    let parts = [("rod.gcad", ROD)];
    let assembly = "part a rod.gcad\npart b rod.gcad\npart c rod.gcad\nmove b 50,0,0\nmove c 0,30,0\naxis tilt 50,0,0 60,0,0\njoint t b a turn about=tilt\ndistance c:rod.side a:rod.side 20\nparallel c:rod.end b:rod.end\n";
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
        "part a rod.gcad\npart b rod.gcad\nmove b 50,0,0\nrotate b 10 axis=x\npart c rod.gcad\nmove c 0,30,0\ndistance c:rod.side a:rod.side 20\nparallel c:rod.end b:rod.end\n",
        &parts,
    );
    assert!(error.contains("already hangs off `a`"), "{error}");
}

#[test]
fn a_rod_turns_and_slides_on_one_axis() {
    let parts = [("rod.gcad", ROD), ("plate.gcad", SLAB)];
    let assembly = "part plate plate.gcad\npart rod rod.gcad\naxis up 0,0,0 0,0,1\njoint spin rod plate turn about=up\njoint push rod plate slide along=0,0,1 min=0 max=20\npose push 10\npose spin 90\n";
    let (model, _) = built("a_rod_turns_and_slides", assembly, &parts);
    let b = extent(&model, "rod");
    assert!(near(b.min().z, 10.0) && near(b.max().z, 50.0), "{b:?}");
    let error = failed(
        "turns_and_slides_that_do_not_commute",
        "part plate plate.gcad\npart rod rod.gcad\naxis up 0,0,0 0,0,1\njoint spin rod plate turn about=up\njoint push rod plate slide along=1,0,0\n",
        &parts,
    );
    assert!(error.contains("depend on their order"), "{error}");
}

#[test]
fn coupled_joints_move_together() {
    let parts = [("rod.gcad", ROD), ("plate.gcad", SLAB)];
    let gears = "part frame plate.gcad\npart a rod.gcad\npart b rod.gcad\npart rack rod.gcad\nmove b 20,0,0\nmove rack 0,40,0\naxis ax 0,0,0 0,0,1\naxis bx 20,0,0 20,0,1\njoint ja a frame turn about=ax\njoint jb b frame turn about=bx min=-60 max=60\njoint slide rack frame slide along=1,0,0\ncouple jb ja ratio=-0.5\ncouple slide ja ratio=0.1\n";
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

#[test]
fn materials_follow_parts() {
    let parts = [("rod.gcad", "circle 10\nextrude 10\nmaterial aluminium\n")];
    let (_, text) = built(
        "materials_follow_parts",
        "part a rod.gcad\npart b rod.gcad\nmove b 20,0,0\nmaterial b steel\nmeasure mass\n",
        &parts,
    );
    let volume = std::f64::consts::PI * 25.0 * 10.0;
    let grams = volume * (2.7 + 7.85) / 1000.0;
    let said: f64 = text
        .split("mass ")
        .nth(1)
        .and_then(|t| t.split(' ').next())
        .and_then(|t| t.parse().ok())
        .expect("mass");
    assert!((said - grams).abs() < grams * 2.0e-3, "{text}");
}

#[test]
fn bill_of_materials_counts_parts() {
    let parts = [
        (
            "rod.gcad",
            "let d=10\ncircle d\nextrude 10\nmaterial steel\n",
        ),
        ("plate.gcad", SLAB),
    ];
    let path = files(
        "bill_of_materials_counts_parts",
        "part base plate.gcad\npart a rod.gcad\npart b rod.gcad\npart c rod.gcad d=20\nmove b 20,0,0\nmove c 40,0,0\n",
        &parts,
    );
    let run = run_path(&path, &[], None).expect("runs");
    let items = gcad::bom::items(&run.model, &path);
    let summary: Vec<(usize, &str, &str)> = items
        .iter()
        .map(|i| (i.names.len(), i.file.as_str(), i.variant.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            (1, "plate.gcad", ""),
            (2, "rod.gcad", ""),
            (1, "rod.gcad", "d=20")
        ]
    );
    assert!(items[0].grams.is_none());
    let grams = std::f64::consts::PI * 25.0 * 10.0 * 7.85 / 1000.0;
    assert!((items[1].grams.expect("steel") - grams).abs() < grams * 2.0e-3);
    let table = gcad::bom::table(&items);
    assert!(
        table.ends_with("4 parts, 36.98 g without the parts that have no material"),
        "{table}"
    );
    assert!(
        gcad::bom::csv(&items)
            .lines()
            .nth(2)
            .expect("row")
            .starts_with("2,rod.gcad,main,,steel,")
    );
}

#[test]
fn exploded_views_move_parts_apart() {
    let parts = [("box.gcad", LID_WITH_HOLES), ("screw.gcad", SCREW)];
    let assembly = format!(
        "{HINGE}joint open box.lid box.main turn about=hinge min=-120 max=0\npart screw screw.gcad\nconcentric screw:shank.side box.lid:screws.side\nflush screw:head.start box.lid:top.end\nexplode box.lid 0,0,20\nexplode screw 0,0,10\ninterference none\n"
    );
    let (model, _) = built("exploded_views", &assembly, &parts);
    let top = |scale: f64, index: usize| {
        let parts = model.exploded_parts(scale);
        gcad::geometry::bounds(&parts[index].0).max().z
    };
    let names = model.body_names();
    let (lid, screw) = (
        names.iter().position(|n| n == "box.lid").expect("lid"),
        names.iter().position(|n| n == "screw").expect("screw"),
    );
    assert!(near(top(0.0, lid), 12.0) && near(top(1.0, lid), 32.0));
    assert!(near(top(0.0, screw), 13.5) && near(top(1.0, screw), 43.5));
    assert!(near(top(0.5, screw), 28.5));
    let open: Vec<f64> = model
        .joints
        .iter()
        .map(|j| if j.name == "open" { -90.0 } else { j.value })
        .collect();
    let moved = gcad::model::posed(&model.joints, &open);
    let offsets = gcad::model::explode_offsets(&model.joints, &model.explode, &moved);
    let lid = offsets["box.lid"];
    assert!(near(lid.x, 0.0) && near(lid.y, 20.0) && near(lid.z, 0.0), "{lid:?}");
    let screw = offsets["screw"];
    assert!(near(screw.y, 30.0) && near(screw.z, 0.0), "{screw:?}");
}

const BAR: &str = "let L=40\nrect L+8 8 r=3.9 at=L/2,0\nbar: extrude 3\nplane bar.end\na: hole 3 0,0\nb: hole 3 L,0\n";

const FOUR_BAR: &str = "let ground=40 crank=15 coupler=40 rocker=30
let d=hypot(ground,crank) along=(coupler*coupler-rocker*rocker+d*d)/(2*d)
let h=sqrt(coupler*coupler-along*along)
let cx=along*ground/d+h*crank/d cy=crank-along*crank/d+h*ground/d
part base plate.gcad
part crankbar bar.gcad L=crank
part couplerbar bar.gcad L=coupler
part rockerbar bar.gcad L=rocker
rotate crankbar 90 axis=z
move couplerbar 0,crank,3
rotate couplerbar atan2(cy-crank,cx) axis=z about=0,crank,0
move rockerbar ground,0,0
rotate rockerbar atan2(cy,cx-ground) axis=z about=ground,0,0
axis a 0,0,0 0,0,1
axis knee 0,crank,0 0,crank,1
axis pivot ground,0,0 ground,0,1
joint drive crankbar base turn about=a
joint bend couplerbar crankbar turn about=knee
joint rock rockerbar base turn about=pivot
concentric couplerbar:b.side rockerbar:b.side
";

fn rocker_angle(crank_degrees: f64) -> f64 {
    let (ground, crank, coupler, rocker) = (40.0f64, 15.0f64, 40.0f64, 30.0f64);
    let b = (
        crank * crank_degrees.to_radians().cos(),
        crank * crank_degrees.to_radians().sin(),
    );
    let (dx, dy) = (ground - b.0, -b.1);
    let d = dx.hypot(dy);
    let along = (coupler * coupler - rocker * rocker + d * d) / (2.0 * d);
    let h = (coupler * coupler - along * along).sqrt();
    let c = (
        b.0 + along * dx / d - h * dy / d,
        b.1 + along * dy / d + h * dx / d,
    );
    (c.1).atan2(c.0 - ground).to_degrees()
}

#[test]
fn a_four_bar_linkage_follows_its_crank() {
    let parts = [("plate.gcad", SLAB), ("bar.gcad", BAR)];
    let (model, text) = built("four_bar", &format!("{FOUR_BAR}pose drive 60\n"), &parts);
    let value = |name: &str| {
        model
            .joints
            .iter()
            .find(|j| j.name == name)
            .expect("joint")
            .value
    };
    let turned = rocker_angle(150.0) - rocker_angle(90.0);
    assert!(
        (value("rock") - turned).abs() < 1.0e-3,
        "{text}: rock {} want {turned}",
        value("rock")
    );
    assert!(
        model
            .mates
            .iter()
            .all(|m| m.holds([monstertruck::modeling::Matrix4::from_scale(1.0); 2]))
    );
}

#[test]
fn pattern_copies_a_part_and_its_mates() {
    let plate = "rect 60 20\nbase: extrude 5\nplane base.end\nholes: hole 3 -20,0 0,0 20,0\n";
    let pin = "circle 5\nhead: extrude 1\ncircle 3\npin: extrude -5\n";
    let parts = [("plate.gcad", plate), ("pin.gcad", pin)];
    let pinned = "part plate plate.gcad\npart p pin.gcad\nconcentric p:pin.side plate:holes.side near=-20,0,5\nflush p:head.start plate:base.end\npattern p holes=plate:holes.side\n";
    let (model, text) = built("pattern_holes", pinned, &parts);
    assert!(text.contains("p_2, p_3, each with 2 mate(s)"), "{text}");
    let mut centres: Vec<f64> = ["p", "p_2", "p_3"]
        .iter()
        .map(|n| {
            let b = extent(&model, n);
            assert!(near(b.max().z, 6.0) && near(b.min().z, 0.0), "{n}: {b:?}");
            (b.min().x + b.max().x) / 2.0
        })
        .collect();
    centres.sort_by(f64::total_cmp);
    assert!(
        near(centres[0], -20.0) && near(centres[1], 0.0) && near(centres[2], 20.0),
        "{centres:?}"
    );
    assert_eq!(
        model.joints.iter().filter(|j| j.parent == "plate").count(),
        3
    );
    let (model, _) = built(
        "pattern_step",
        "part plate plate.gcad\npart p pin.gcad\npattern p count=3 step=10,0,0\n",
        &parts,
    );
    assert!(near(
        extent(&model, "p_3").min().x - extent(&model, "p").min().x,
        20.0
    ));
    let (model, _) = built(
        "pattern_turn",
        "part plate plate.gcad\npart p pin.gcad\nmove p 20,0,0\naxis up 0,0,0 0,0,1\npattern p count=4 angle=360 axis=up\n",
        &parts,
    );
    let b = extent(&model, "p_2");
    assert!(near((b.min().y + b.max().y) / 2.0, 20.0), "{b:?}");
    let error = failed(
        "pattern_unmated",
        "part plate plate.gcad\npart p pin.gcad\npattern p holes=plate:holes.side\n",
        &parts,
    );
    assert!(error.contains("mate it into one first"), "{error}");
}

const GEAR: &str = "let z=20\ngear z 2\ncircle 6\nteeth: extrude 6\n";

fn meshed(test: &str, phase: f64) -> Vec<Result<String, String>> {
    let assembly = [
        "part frame frame.gcad",
        "part a gear.gcad z=20",
        "part b gear.gcad z=12",
        "move b 32,0,0",
        &format!("rotate b {phase} axis=z about=32,0,0"),
        "axis wheel 0,0,0 0,0,1",
        "axis pinion 32,0,0 32,0,1",
        "joint drive a frame turn about=wheel",
        "joint follow b frame turn about=pinion",
        "couple follow drive ratio=-20/12",
        "pose drive 7",
        "interference none",
    ]
    .join("\n");
    let frame = "plane XY offset=-20\nrect 80 40 at=16,0\nextrude 5\n";
    steps(
        test,
        &assembly,
        &[("gear.gcad", GEAR), ("frame.gcad", frame)],
    )
    .1
}

#[test]
fn gears_in_mesh_turn_without_touching() {
    let results = meshed("gears_in_mesh", 15.0);
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    let failed = meshed("gears_out_of_phase", 0.0);
    let clash = failed
        .iter()
        .find_map(|r| r.as_ref().err())
        .expect("teeth clash");
    assert!(clash.contains("a and b"), "{clash}");
}

#[test]
fn robot_arm_example() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/robot/arm.gasm");
    let fingers = |vars: &[(String, f64)]| {
        let run = run_path(&path, vars, None).expect("runs");
        let failed: Vec<String> = run
            .steps
            .iter()
            .filter_map(|(line, r)| r.as_ref().err().map(|e| format!("{}: {e:#}", line.number)))
            .collect();
        assert!(failed.is_empty(), "{failed:?}");
        assert_eq!(run.model.body_names().len(), 34);
        ["finger_a", "finger_b"].map(|name| {
            gcad::geometry::bounds(&run.model.named_body(name).expect("finger")).center()
        })
    };
    let [a0, b0] = fingers(&[]);
    let [a1, b1] = fingers(&[("jaw".into(), 5.0)]);
    let (da, db) = (a1 - a0, b1 - b0);
    let length = |v: [f64; 3]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
    assert!((length([da.x, da.y, da.z]) - 5.0).abs() < 1.0e-6, "{da:?}");
    assert!(
        length([da.x + db.x, da.y + db.y, da.z + db.z]) < 1.0e-6,
        "{da:?} {db:?}"
    );
}

#[test]
fn planetary_gears_turn_without_touching() {
    let assembly = [
        "part ring ring.gcad",
        "part sun gear.gcad",
        "part planet gear.gcad a=190",
        "move planet 18,0,0",
        "part carrier carrier.gcad",
        "axis centre 0,0,0 0,0,1",
        "axis pin 18,0,0 18,0,1",
        "joint drive carrier ring turn about=centre",
        "joint input sun ring turn about=centre",
        "couple input drive ratio=1+54/18",
        "joint spin planet carrier turn about=pin",
        "couple spin drive ratio=-54/18",
        "pattern planet count=3 angle=360 axis=centre",
        "pose drive 25",
        "interference none",
    ]
    .join("\n");
    let parts = [
        (
            "ring.gcad",
            "circle 70\ngear 54 1 internal\nteeth: extrude 6\n",
        ),
        (
            "gear.gcad",
            "let a=0\ngear 18 1 angle=a\ncircle 6\nteeth: extrude 6\n",
        ),
        (
            "carrier.gcad",
            "plane XY offset=7\ncircle 50\nplate: extrude 2\n",
        ),
    ];
    let (model, _) = built("planetary", &assembly, &parts);
    let names = model.body_names();
    assert!(names.contains(&"planet_3".to_string()), "{names:?}");
    let value = |name: &str| {
        model
            .joints
            .iter()
            .find(|j| j.name == name)
            .map(|j| j.value)
            .expect(name)
    };
    assert!((value("input") - 100.0).abs() < 1.0e-9);
    assert!((value("spin_3") + 75.0).abs() < 1.0e-9);
    let (_, out_of_phase) = steps(
        "planetary_clash",
        &assembly.replace("a=190", "a=180"),
        &parts,
    );
    assert!(out_of_phase.last().is_some_and(Result::is_err));
}
