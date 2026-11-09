mod common;

use common::*;
use gcad::model::Model;

const T: f64 = 300.0;
const H: f64 = 2700.0;
const OUT_X: f64 = 8000.0;
const OUT_Y: f64 = 6000.0;

fn building() -> String {
    [
        "units mm",
        "building \"Test house\" north=20 datum=0",
        "level L0 0 \"Ground\"",
        "level L1 2700 \"First\"",
        "level L2 5400 \"Roof\"",
        "grid A x=0",
        "grid B x=8000",
        "grid 1 y=0",
        "grid 2 y=6000",
        "wall_type ext brick:100 pir:100 block:100",
        "wall_type int block:100",
        "outer: wall A1 B1 B2 A2 closed type=ext base=L0 top=L1 justify=outside",
        "inner: wall 4000,300 4000,5700 type=int base=L0 top=L1 justify=centre",
        "d1: door on=outer at=2000 w=900 h=2100 swing=left mark=D01",
        "win1: window on=outer at=6000 w=1200 h=1200 sill=900 mark=W01",
        "",
    ]
    .join("\n")
}

fn with(extra: &str) -> String {
    format!("{}{extra}", building())
}

fn solid_volume(solid: &monstertruck::modeling::Solid) -> f64 {
    gcad::geometry::volume_at(solid, gcad::geometry::bounds(solid).diameter() * 2.0e-5)
}

fn volume_of(model: &Model, prefix: &str) -> f64 {
    model
        .named_solids()
        .iter()
        .filter(|(name, _)| name == prefix || name.starts_with(&format!("{prefix}.")))
        .map(|(_, solid)| solid_volume(solid))
        .sum()
}

fn count_of(model: &Model, prefix: &str) -> usize {
    model
        .named_solids()
        .iter()
        .filter(|(name, _)| name.starts_with(&format!("{prefix}.")))
        .count()
}

fn group_bounds(model: &Model, prefix: &str) -> ([f64; 3], [f64; 3]) {
    model
        .named_solids()
        .iter()
        .filter(|(name, _)| name == prefix || name.starts_with(&format!("{prefix}.")))
        .map(|(_, solid)| gcad::geometry::bounds(solid))
        .fold(
            ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
            |(lo, hi), b| {
                let (min, max) = (b.min(), b.max());
                (
                    [lo[0].min(min.x), lo[1].min(min.y), lo[2].min(min.z)],
                    [hi[0].max(max.x), hi[1].max(max.y), hi[2].max(max.z)],
                )
            },
        )
}

fn number_after(text: &str, key: &str) -> f64 {
    let at = text
        .find(key)
        .unwrap_or_else(|| panic!("no `{key}` in {text}"))
        + key.len();
    text[at..]
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no number after `{key}` in {text}"))
}

const TOP_TREAD: f64 = 16.0 * 3000.0 / 17.0;

fn body_bounds(model: &Model, name: &str) -> ([f64; 3], [f64; 3]) {
    let b = gcad::geometry::bounds(&model.named_body(name).expect("body"));
    let (lo, hi) = (b.min(), b.max());
    ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z])
}

fn near(actual: f64, expected: f64, relative: f64) {
    assert!(
        (actual - expected).abs() <= expected.abs().max(1.0) * relative,
        "got {actual:.3}, expected {expected:.3} within {:.3}%",
        relative * 100.0
    );
}

fn house_files(test: &str, extra: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(scratch(test));
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("house.gcad"), building()).expect("house");
    for (name, text) in extra {
        std::fs::write(dir.join(name), text).expect("file");
    }
    dir
}

fn gcad(dir: &std::path::Path, args: &[&str]) -> (bool, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("runs");
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), text)
}

fn schedule(test: &str, source: &str, kind: &str) -> String {
    let dir = house_files(test, &[("model.gcad", source)]);
    let (ok, text) = gcad(&dir, &["schedule", "model.gcad", kind, "--csv"]);
    assert!(ok, "{text}");
    text
}

fn csv_cell(csv: &str, row: &str, column: &str) -> f64 {
    let mut lines = csv.lines();
    let header: Vec<&str> = lines.next().expect("header").split(',').collect();
    let at = header.iter().position(|h| *h == column).expect("column");
    let cells: Vec<&str> = lines
        .map(|l| l.split(',').collect::<Vec<_>>())
        .find(|c| c[0] == row)
        .expect("row");
    cells[at].parse().expect("number")
}

fn plan_svg(test: &str, sheet: &str) -> String {
    let dir = house_files(test, &[("house.gdwg", sheet)]);
    let drawing = gcad::drafting::run(&dir.join("house.gdwg"), &[]).expect("runs");
    assert!(
        drawing.failed().is_none(),
        "{:?}",
        drawing.failed().map(|(l, e)| format!("{}: {e:#}", l.text))
    );
    gcad::drafting::svg(&drawing.sheets[0])
}

fn ifc_of(test: &str, source: &str) -> String {
    let dir = house_files(test, &[("model.gcad", source)]);
    let (ok, text) = gcad(&dir, &["export", "model.gcad", "model.ifc"]);
    assert!(ok, "{text}");
    std::fs::read_to_string(dir.join("model.ifc")).expect("ifc")
}

fn entities(ifc: &str, class: &str) -> usize {
    ifc.lines()
        .filter(|l| l.contains(&format!("={class}(")))
        .count()
}

fn mesh_volume(source: &str) -> (f64, usize) {
    let model = gcad_mesh::build_with(
        source,
        &gcad_mesh::Options {
            y_up: false,
            ..Default::default()
        },
    )
    .expect("builds in gcad-mesh");
    let mut volume = 0.0;
    let mut triangles = 0;
    for body in &model.bodies {
        triangles += body.indices.len() / 3;
        volume += body
            .indices
            .chunks(3)
            .map(|t| {
                let [a, b, c] =
                    [t[0], t[1], t[2]].map(|i| body.positions[i as usize].map(f64::from));
                a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0])
            })
            .sum::<f64>()
            / 6.0;
    }
    (volume, triangles)
}

const DATUMS: &str = "units mm\nlevel L0 0 \"Ground\"\nlevel L1 2700 \"First\"\ngrid A x=0\ngrid B x=4000\ngrid 1 y=0\ngrid 2 y=3000\nwall_type ext brick:100 pir:100 block:100\nwall_type int block:100\n";

fn datums(extra: &str) -> String {
    format!("{DATUMS}{extra}")
}

#[test]
fn units_and_building_header() {
    let text = summary("units mm\nbuilding \"Test house\" north=20 datum=150");
    assert!(text.contains("Test house"), "{text}");
    assert!(text.contains("north 20"), "{text}");
    assert!(failure("units furlong").contains("mm"));
    assert!(failure("building \"x\" north=400").contains("north"));
}

#[test]
fn levels_are_heights() {
    let model = build(&datums("w1: wall 0,0 4000,0 type=int base=L0 top=L1"));
    assert_bounds(&model, [0.0, -50.0, 0.0], [4000.0, 50.0, 2700.0]);
    let raised = build(&datums(
        "w1: wall 0,0 4000,0 type=int base=L0+150 top=L1-300",
    ));
    assert_bounds(&raised, [0.0, -50.0, 150.0], [4000.0, 50.0, 2400.0]);
    assert!(failure(&datums("w1: wall 0,0 1,0 type=int top=L9")).contains("L9"));
    assert!(failure("level L0 0\nlevel L0 3000").contains("already"));
}

#[test]
fn grids_are_coordinates() {
    let model = build(&datums("w1: wall A1 B1 type=int base=L0 top=L1"));
    assert_bounds(&model, [0.0, -50.0, 0.0], [4000.0, 50.0, 2700.0]);
    let offset = build(&datums(
        "w1: wall A1+0,500 B1+0,500 type=int base=L0 top=L1",
    ));
    assert_bounds(&offset, [0.0, 450.0, 0.0], [4000.0, 550.0, 2700.0]);
    assert!(failure(&datums("w1: wall A1 C1 type=int top=L1")).contains("C"));
}

#[test]
fn grids_from_a_file() {
    let dir = house_files(
        "grids_from_a_file",
        &[
            ("grids.csv", "A,x,0\nB,x,4000\nC,x,8000\n1,y,0\n2,y,6000\n"),
            (
                "model.gcad",
                "gridfile grids.csv\nwall_type int block:100\nw1: wall A1 C1 type=int top=3000",
            ),
        ],
    );
    let run = gcad::model::run_path(&dir.join("model.gcad"), &[], None).expect("runs");
    let b = gcad::geometry::bounds(&run.model.named_body("w1.block").expect("w1.block"));
    near(b.max().x, 8000.0, 1.0e-9);
}

#[test]
fn wall_is_its_layers() {
    let model = build(&datums(
        "w1: wall 0,0 4000,0 type=ext base=L0 top=L1 justify=outside",
    ));
    near(volume_of(&model, "w1"), 4000.0 * T * H, 1.0e-6);
    for (layer, material) in [("brick", "brick"), ("pir", "pir"), ("block", "block")] {
        let name = format!("w1.{layer}");
        near(
            solid_volume(&model.named_body(&name).expect("layer")),
            4000.0 * 100.0 * H,
            1.0e-6,
        );
        assert_eq!(model.materials[&name].name, material);
    }
    let (lo, hi) = body_bounds(&model, "w1.brick");
    near(lo[1], 0.0, 1.0e-9);
    near(hi[1], 100.0, 1.0e-9);
    assert!(failure("wall_type bad brick:0").contains("thickness"));
    assert!(failure("wall_type bad unobtainium:100").contains("unobtainium"));
}

#[test]
fn wall_export_is_the_same_every_run() {
    let dir = std::path::PathBuf::from(scratch("wall_export_is_the_same_every_run"));
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("house.gcad"), building()).expect("house");
    let bytes = |name: &str| {
        let (ok, text) = gcad(&dir, &["export", "house.gcad", name]);
        assert!(ok, "{text}");
        std::fs::read(dir.join(name)).expect("stl")
    };
    assert_eq!(bytes("a.stl"), bytes("b.stl"));
}

#[test]
fn wall_justification_and_offset() {
    let line = |j: &str| build(&datums(&format!("w1: wall 0,0 4000,0 type=ext top=L1 {j}")));
    assert_bounds(
        &line("justify=outside"),
        [0.0, 0.0, 0.0],
        [4000.0, 300.0, H],
    );
    assert_bounds(
        &line("justify=centre"),
        [0.0, -150.0, 0.0],
        [4000.0, 150.0, H],
    );
    assert_bounds(
        &line("justify=inside"),
        [0.0, -300.0, 0.0],
        [4000.0, 0.0, H],
    );
    assert_bounds(
        &line("justify=centre offset=150"),
        [0.0, 0.0, 0.0],
        [4000.0, 300.0, H],
    );
    assert_bounds(&line(""), [0.0, -150.0, 0.0], [4000.0, 150.0, H]);
}

#[test]
fn closed_wall_loop() {
    let model = build(&building());
    let ring = OUT_X * OUT_Y - (OUT_X - 2.0 * T) * (OUT_Y - 2.0 * T);
    let door = 900.0 * 2100.0 * T;
    let window = 1200.0 * 1200.0 * T;
    near(volume_of(&model, "outer"), ring * H - door - window, 1.0e-5);
    let layers = model
        .named_solids()
        .iter()
        .filter(|(n, _)| n.starts_with("outer."))
        .map(|(_, s)| *s)
        .collect::<Vec<_>>();
    for (i, a) in layers.iter().enumerate() {
        for b in &layers[i + 1..] {
            assert!(gcad::geometry::overlap_volume(a, b, 64) < 1.0e-3);
        }
    }
}

#[test]
fn polyline_and_arc_walls() {
    let l = build(&datums(
        "w1: wall 0,0 4000,0 4000,3000 type=int top=L1 justify=centre",
    ));
    near(volume_of(&l, "w1"), (4000.0 + 3000.0) * 100.0 * H, 1.0e-5);
    let arc = build(&datums(
        "w2: wall arc 0,0 2000,2000 4000,0 type=int top=L1 justify=centre",
    ));
    let r: f64 = 2000.0;
    let ring = std::f64::consts::PI * ((r + 50.0).powi(2) - (r - 50.0).powi(2)) / 2.0;
    near(volume_of(&arc, "w2"), ring * H, 1.0e-3);
}

#[test]
fn wall_from_a_sketch_profile() {
    let model = build(&datums(
        "rect 4000 3000 at=2000,1500\nw3: wall profile type=int top=L1 justify=outside",
    ));
    let ring = 4000.0 * 3000.0 - 3800.0 * 2800.0;
    near(volume_of(&model, "w3"), ring * H, 1.0e-5);
}

#[test]
fn sloped_and_battered_walls() {
    let sloped = build(&datums(
        "w1: wall 0,0 4000,0 type=int top=L1 top_offset=0,600 justify=centre",
    ));
    near(
        volume_of(&sloped, "w1"),
        4000.0 * 100.0 * (H + 300.0),
        1.0e-5,
    );
    let battered = build(&datums(
        "w2: wall 0,0 4000,0 type=int top=L1 batter=1 justify=centre",
    ));
    near(volume_of(&battered, "w2"), 4000.0 * 100.0 * H, 1.0e-5);
    let (_, hi) = body_bounds(&battered, "w2.block");
    near(hi[1], 50.0 + H * 1.0f64.to_radians().tan(), 1.0e-4);
}

#[test]
fn wall_with_pilasters() {
    let model = build(&datums(
        "w1: wall 0,0 4000,0 type=int top=L1 justify=centre pilaster=300x200@2000",
    ));
    near(
        volume_of(&model, "w1"),
        (4000.0 * 100.0 + 300.0 * 200.0) * H,
        1.0e-5,
    );
}

#[test]
fn tee_join_has_no_gap_or_overlap() {
    let model = build(&building());
    near(
        volume_of(&model, "inner"),
        100.0 * (OUT_Y - 2.0 * T) * H,
        1.0e-5,
    );
    let inner = model.named_body("inner.block").expect("inner");
    for layer in ["outer.brick", "outer.pir", "outer.block"] {
        let outer = model.named_body(layer).expect("outer");
        assert!(gcad::geometry::overlap_volume(&inner, &outer, 64) < 1.0e-3);
    }
}

#[test]
fn wall_join_overrides() {
    let corner = |how: &str| {
        build(&datums(&format!(
            "w1: wall 0,0 4000,0 type=int top=L1\nw2: wall 0,0 0,3000 type=int top=L1\nwall_join w1 w2 {how}"
        )))
    };
    let both = (4000.0 + 3000.0) * 100.0 * H;
    for how in ["mitre", "butt"] {
        near(
            volume_of(&corner(how), "w1") + volume_of(&corner(how), "w2"),
            both,
            1.0e-5,
        );
    }
    let mitre = corner("mitre");
    near(volume_of(&mitre, "w1"), 4000.0 * 100.0 * H, 1.0e-5);
    let butt = corner("butt");
    near(volume_of(&butt, "w1"), (4000.0 + 50.0) * 100.0 * H, 1.0e-5);
    near(volume_of(&butt, "w2"), (3000.0 - 50.0) * 100.0 * H, 1.0e-5);
    assert!(
        failure(&datums("w1: wall 0,0 1000,0 type=int top=L1\nw2: wall 0,500 1000,500 type=int top=L1\nwall_join w1 w2 mitre"))
            .contains("do not meet")
    );
}

#[test]
fn stacked_wall() {
    let model = build(&datums(
        "w1: wall 0,0 4000,0 type=ext:0..900,int:900..L1 top=L1 justify=inside",
    ));
    near(
        volume_of(&model, "w1"),
        4000.0 * (300.0 * 900.0 + 100.0 * 1800.0),
        1.0e-5,
    );
}

#[test]
fn wall_sweep_runs_along_the_wall() {
    let model = build(&datums(
        "w1: wall 0,0 4000,0 type=int top=L1 justify=centre\nskirt: wall_sweep w1.inside rect 15x100 at=0",
    ));
    near(volume_of(&model, "skirt"), 4000.0 * 15.0 * 100.0, 1.0e-5);
}

#[test]
fn wall_cut_by_a_profile() {
    let model = build(&datums(
        "w1: wall 0,0 4000,0 type=int top=L1 justify=centre\nwall_cut w1 rect 1000 500 at=2000,2450",
    ));
    near(
        volume_of(&model, "w1"),
        4000.0 * 100.0 * H - 1000.0 * 500.0 * 100.0,
        1.0e-5,
    );
}

#[test]
fn wall_faces_are_selectors() {
    let text = summary(&datums(
        "w1: wall 0,0 4000,0 type=ext top=L1 justify=outside\nmeasure w1.outside w1.inside",
    ));
    assert!(text.contains("300.000"), "{text}");
    let top = summary(&datums(
        "w1: wall 0,0 4000,0 type=ext top=L1 justify=outside\nmeasure box w1.top",
    ));
    assert!(top.contains("4000.000 x 300.000 x 0.000"), "{top}");
}

#[test]
fn opening_cuts_every_layer() {
    let model = build(&datums(
        "w1: wall 0,0 4000,0 type=ext top=L1 justify=outside\no1: opening on=w1 at=1200 w=900 h=2100 sill=0",
    ));
    for layer in ["brick", "pir", "block"] {
        near(
            solid_volume(&model.named_body(&format!("w1.{layer}")).expect("layer")),
            (4000.0 * H - 900.0 * 2100.0) * 100.0,
            1.0e-5,
        );
    }
}

#[test]
fn moving_a_wall_moves_its_openings() {
    let at = |y: f64| {
        build(&datums(&format!(
            "w1: wall 0,{y} 4000,{y} type=ext top=L1 justify=outside\nd1: door on=w1 at=1200 w=900 h=2100"
        )))
    };
    let (a, _) = body_bounds(&at(0.0), "d1.leaf");
    let (b, _) = body_bounds(&at(500.0), "d1.leaf");
    near(b[1] - a[1], 500.0, 1.0e-9);
    near(a[0], 1200.0 - 450.0 + 70.0, 1.0e-3);
}

#[test]
fn openings_that_leave_the_wall_or_overlap_fail() {
    let wall = "w1: wall 0,0 4000,0 type=ext top=L1 justify=outside\n";
    assert!(
        failure(&datums(&format!(
            "{wall}o1: opening on=w1 at=3800 w=900 h=2100"
        )))
        .contains("past the end")
    );
    assert!(
        failure(&datums(&format!(
            "{wall}o1: opening on=w1 at=1200 w=900 h=3000"
        )))
        .contains("above the top")
    );
    assert!(
        failure(&datums(&format!(
            "{wall}o1: opening on=w1 at=1200 w=900 h=2100\no2: opening on=w1 at=1800 w=900 h=2100"
        )))
        .contains("o1")
    );
    assert!(failure(&datums("o1: opening on=nowall at=0 w=900 h=2100")).contains("nowall"));
}

#[test]
fn door_has_frame_leaf_and_mark() {
    let model = build(&datums(
        "w1: wall 0,0 4000,0 type=ext top=L1 justify=outside\nd1: door on=w1 at=1200 w=900 h=2100 swing=left hinge=left in frame=70x45 leaf=44 mark=D01",
    ));
    let frame = 2.0 * 2100.0 * 70.0 * 45.0 + (900.0 - 2.0 * 70.0) * 70.0 * 45.0;
    near(volume_of(&model, "d1.frame"), frame, 1.0e-4);
    let leaf = (900.0 - 2.0 * 70.0) * (2100.0 - 70.0) * 44.0;
    near(volume_of(&model, "d1.leaf"), leaf, 1.0e-3);
    let text = summary(&datums(
        "w1: wall 0,0 4000,0 type=ext top=L1 justify=outside\nd1: door on=w1 at=1200 w=900 h=2100 mark=D01",
    ));
    assert!(text.contains("D01"), "{text}");
}

#[test]
fn door_swings() {
    for swing in ["left", "right", "double", "slide", "fold"] {
        let model = build(&datums(&format!(
            "w1: wall 0,0 4000,0 type=ext top=L1 justify=outside\nd1: door on=w1 at=2000 w=1600 h=2100 swing={swing}"
        )));
        let leaves = count_of(&model, "d1") - 1;
        let expected = match swing {
            "double" | "slide" => 2,
            "fold" => 4,
            _ => 1,
        };
        assert_eq!(leaves, expected, "{swing}");
    }
    assert!(
        failure(&datums(
            "w1: wall 0,0 4000,0 type=ext top=L1\nd1: door on=w1 at=2000 w=900 h=2100 swing=up"
        ))
        .contains("swing")
    );
}

#[test]
fn window_panes_and_glass() {
    let model = build(&datums(
        "w1: wall 0,0 4000,0 type=ext top=L1 justify=outside\nwin1: window on=w1 at=2000 w=1200 h=1200 sill=900 panes=2x1 frame=60 mullion=40 glazing=double mark=W01",
    ));
    let (lo, hi) = body_bounds(&model, "win1.frame");
    near(lo[2], 900.0, 1.0e-6);
    near(hi[2], 2100.0, 1.0e-6);
    let glass_w = (1200.0 - 2.0 * 60.0 - 40.0) / 2.0;
    let glass_h = 1200.0 - 2.0 * 60.0;
    assert_eq!(count_of(&model, "win1.glass"), 2);
    let pane = solid_volume(&model.named_body("win1.glass.1").expect("pane"));
    near(pane, glass_w * glass_h * 24.0, 1.0e-3);
    assert!(model.materials["win1.glass.1"].name.contains("glass"));
}

#[test]
fn window_head_shapes() {
    let area = |head: &str| {
        let model = build(&datums(&format!(
            "w1: wall 0,0 4000,0 type=int top=L1 justify=centre\nwin1: window on=w1 at=2000 w=1200 h=1200 sill=900 head={head}"
        )));
        4000.0 * H - volume_of(&model, "w1") / 100.0
    };
    near(area("flat"), 1200.0 * 1200.0, 1.0e-4);
    let round = 1200.0 * 600.0 + std::f64::consts::PI * 600.0 * 600.0 / 2.0;
    near(area("round"), round, 1.0e-3);
}

#[test]
fn garage_door() {
    let model = build(&datums(
        "w1: wall 0,0 4000,0 type=ext top=L1 justify=outside\ng1: garage_door on=w1 at=2000 w=2400 h=2100 panels=4",
    ));
    assert_eq!(count_of(&model, "g1.panel"), 4);
    near(
        volume_of(&model, "w1"),
        (4000.0 * H - 2400.0 * 2100.0) * T,
        1.0e-5,
    );
}

#[test]
fn door_from_a_part_file() {
    let dir = house_files(
        "door_from_a_part_file",
        &[
            (
                "flush.gcad",
                "let w=900 h=2100 t=44\nbody leaf\nrect w-10 t at=0,0\nextrude h-5",
            ),
            (
                "model.gcad",
                "level L1 2700\nwall_type int block:100\nw1: wall 0,0 4000,0 type=int top=L1 justify=centre\nd1: door on=w1 at=2000 w=826 h=2040 part=flush.gcad",
            ),
        ],
    );
    let run = gcad::model::run_path(&dir.join("model.gcad"), &[], None).expect("runs");
    near(
        solid_volume(&run.model.named_body("d1.leaf").expect("leaf")),
        816.0 * 44.0 * 2035.0,
        1.0e-5,
    );
}

#[test]
fn slab_with_an_opening() {
    let model = build(&with(
        "s1: slab L1 A1 B1 B2 A2 type=concrete:200\nslab_opening s1 1000,1000 3000,1000 3000,2000 1000,2000",
    ));
    near(
        volume_of(&model, "s1"),
        (OUT_X * OUT_Y - 2000.0 * 1000.0) * 200.0,
        1.0e-5,
    );
    let (_, hi) = body_bounds(&model, "s1.concrete");
    near(hi[2], 2700.0, 1.0e-6);
}

#[test]
fn layered_slab_is_its_layers() {
    let model = build(&datums(
        "s1: slab L1 0,0 4000,0 4000,3000 0,3000 type=concrete:200+screed:50+tile:10",
    ));
    for (layer, t, top) in [
        ("concrete", 200.0, 2640.0),
        ("screed", 50.0, 2690.0),
        ("tile", 10.0, 2700.0),
    ] {
        let name = format!("s1.{layer}");
        near(
            solid_volume(&model.named_body(&name).expect("layer")),
            12.0e6 * t,
            1.0e-6,
        );
        near(body_bounds(&model, &name).1[2], top, 1.0e-6);
    }
}

#[test]
fn sloped_slab_falls_to_a_point() {
    let model = build(&datums(
        "s1: slab L0 0,0 4000,0 4000,3000 0,3000 type=concrete:100 slope=0.02 falls_to=4000,1500",
    ));
    near(volume_of(&model, "s1"), 12.0e6 * 100.0, 1.0e-4);
    let (lo, hi) = body_bounds(&model, "s1.concrete");
    near(hi[2] - lo[2], 100.0 + 4000.0 * 0.02, 1.0e-4);
}

fn export_twice(test: &str, source: &str) {
    let dir = house_files(test, &[("model.gcad", source)]);
    let bytes = |name: &str| {
        let (ok, text) = gcad(&dir, &["export", "model.gcad", name]);
        assert!(ok, "{text}");
        std::fs::read(dir.join(name)).expect("stl")
    };
    assert_eq!(bytes("a.stl"), bytes("b.stl"));
}

#[test]
fn slab_export_is_the_same_every_run() {
    export_twice(
        "slab_export_is_the_same_every_run",
        &with(
            "s1: slab L1 A1 B1 B2 A2 type=concrete:200+screed:50\nslab_opening s1 1000,1000 3000,1000 3000,2000 1000,2000\ns2: slab L0 0,0 4000,0 4000,3000 0,3000 type=concrete:100 slope=0.02 falls_to=4000,1500",
        ),
    );
}

#[test]
fn ceiling_in_a_room() {
    let model = build(&with(
        "room hall at=2000,3000\nc1: ceiling room=hall h=2400 type=gypsum:13",
    ));
    let (lo, hi) = body_bounds(&model, "c1.gypsum");
    near(lo[2], 2400.0, 1.0e-6);
    near(hi[2], 2413.0, 1.0e-6);
    near(volume_of(&model, "c1"), 3650.0 * 5400.0 * 13.0, 1.0e-5);
}

#[test]
fn floor_finish_in_a_room() {
    let model = build(&with(
        "room hall at=2000,3000\nf1: floor_finish room=hall type=oak:20",
    ));
    near(volume_of(&model, "f1"), 3650.0 * 5400.0 * 20.0, 1.0e-5);
}

#[test]
fn room_that_is_not_a_rectangle_fails() {
    let l_shape = "w: wall 0,0 4000,0 4000,3000 2000,3000 2000,1500 0,1500 closed type=int justify=centre top=L1\n";
    assert!(failure(&datums(&format!("{l_shape}room r at=1000,700"))).contains("rectangle"));
    assert!(failure(&datums("room r at=1000,700")).contains("no walls"));
    assert!(failure(&with("c1: ceiling room=nope h=2400 type=gypsum:13")).contains("nope"));
}

#[test]
fn ceiling_export_is_the_same_every_run() {
    export_twice(
        "ceiling_export_is_the_same_every_run",
        &with(
            "room hall at=2000,3000\nc1: ceiling room=hall h=2400 type=gypsum:13\nf1: floor_finish room=hall type=screed:50+oak:20",
        ),
    );
}

#[test]
fn strip_and_pad_foundations() {
    let model = build(&with(
        "fs: foundation strip outer w=600 d=900\nfp: foundation pad 4000,3000 w=1000 l=1000 d=500",
    ));
    let centre = 2.0 * (OUT_X - T) + 2.0 * (OUT_Y - T);
    near(volume_of(&model, "fs"), centre * 600.0 * 900.0, 1.0e-4);
    near(volume_of(&model, "fp"), 1000.0 * 1000.0 * 500.0, 1.0e-6);
    let (_, hi) = body_bounds(&model, "fp");
    near(hi[2], 0.0, 1.0e-6);
}

#[test]
fn ground_slab_inside_the_walls() {
    let model = build(&with("g: ground_slab outer type=concrete:150+pir:100"));
    let inside = (OUT_X - 2.0 * T) * (OUT_Y - 2.0 * T);
    near(volume_of(&model, "g"), inside * 250.0, 1.0e-5);
}

#[test]
fn foundation_export_is_the_same_every_run() {
    export_twice(
        "foundation_export_is_the_same_every_run",
        &with(
            "fs: foundation strip outer w=600 d=900\nfp: foundation pad 4000,3000 w=1000 l=1000 d=500\ng: ground_slab outer type=concrete:150+pir:100",
        ),
    );
}

#[test]
fn gable_roof_ridge_and_area() {
    let model = build(&with(
        "r1: roof outer type=gable pitch=45 overhang=0 thickness=200 ridge=x base=L1",
    ));
    let (_, hi) = body_bounds(&model, "r1");
    let rise = OUT_Y / 2.0;
    near(
        hi[2],
        2700.0 + rise + 200.0 / 45f64.to_radians().cos(),
        1.0e-4,
    );
    let slope = OUT_Y / 2.0 / 45f64.to_radians().cos();
    near(volume_of(&model, "r1"), 2.0 * OUT_X * slope * 200.0, 2.0e-2);
}

#[test]
fn roof_export_is_the_same_every_run() {
    export_twice(
        "roof_export_is_the_same_every_run",
        &with(
            "r1: roof outer type=gable pitch=40 overhang=450 thickness=200 ridge=x base=L1\nr2: roof 0,7000 4000,7000 4000,10000 0,10000 closed type=mansard pitch=30 base=L1\nr3: roof 5000,7000 9000,7000 9000,10000 5000,10000 closed type=barrel pitch=40 base=L1",
        ),
    );
}

#[test]
fn roof_types() {
    for kind in [
        "hip", "half_hip", "shed", "flat", "mansard", "gambrel", "barrel",
    ] {
        let model = build(&with(&format!(
            "r1: roof outer type={kind} pitch=35 overhang=450 thickness=200 base=L1"
        )));
        let (lo, hi) = body_bounds(&model, "r1");
        near(lo[0], -450.0, 1.0e-3);
        near(hi[0], OUT_X + 450.0, 1.0e-3);
        assert!(hi[2] > 2700.0, "{kind}");
    }
    let hip = build(&with(
        "r1: roof outer type=hip pitch=45 overhang=0 thickness=200 base=L1",
    ));
    let (_, hi) = body_bounds(&hip, "r1");
    near(hi[2], 2700.0 + OUT_Y / 2.0 + 200.0 * 2f64.sqrt(), 1.0e-3);
    assert!(failure(&with("r1: roof 0,0 1000,0 1000,1000 type=gable pitch=30")).contains("closed"));
}

#[test]
fn per_edge_pitch() {
    let model = build(&with(
        "r1: roof outer type=hip pitch=45 overhang=0 thickness=200 base=L1\npitch_edge r1.eave.1 60",
    ));
    let (_, hi) = body_bounds(&model, "r1");
    assert!(hi[2] > 2700.0 + OUT_Y / 2.0);
}

#[test]
fn trimmed_wall_export_is_the_same_every_run() {
    export_twice(
        "trimmed_wall_export_is_the_same_every_run",
        &with(
            "r1: roof outer type=hip pitch=35 overhang=300 thickness=200 base=L1 walls=outer\npitch_edge r1.eave.1 50",
        ),
    );
}

#[test]
fn walls_trim_to_the_roof_underside() {
    let model = build(&with(
        "r1: roof outer type=gable pitch=45 overhang=0 thickness=200 ridge=x base=L1 walls=outer",
    ));
    let half = OUT_Y / 2.0;
    let above = OUT_X * half * half - (OUT_X - 2.0 * T) * (half * half - T * T);
    let before = volume_of(&build(&building()), "outer");
    near(volume_of(&model, "outer"), before + above, 1.0e-4);
}

#[test]
fn dormer_on_a_roof() {
    let model = build(&with(
        "r1: roof outer type=gable pitch=45 thickness=200 ridge=x base=L1\ndm1: dormer on=r1 at=4000,1500 w=1200 type=gable",
    ));
    let (lo, hi) = body_bounds(&model, "dm1");
    near(hi[0] - lo[0], 1200.0 + 2.0 * 100.0, 5.0e-2);
}

#[test]
fn skylight_on_a_roof() {
    let model = build(&with(
        "r1: roof outer type=gable pitch=45 overhang=0 thickness=200 ridge=x base=L1\nsk1: skylight on=r1 at=4000,1500 w=780 h=980",
    ));
    let before = volume_of(
        &build(&with(
            "r1: roof outer type=gable pitch=45 overhang=0 thickness=200 ridge=x base=L1",
        )),
        "r1",
    );
    near(
        before - volume_of(&model, "r1"),
        780.0 * 980.0 * 200.0,
        1.0e-2,
    );
}

#[test]
fn dormer_and_skylight_export_is_the_same_every_run() {
    export_twice(
        "dormer_and_skylight_export_is_the_same_every_run",
        &with(
            "r1: roof outer type=gable pitch=45 thickness=200 ridge=x base=L1\ndm1: dormer on=r1 at=2500,1500 w=1200 type=gable\nsk1: skylight on=r1 at=6000,4500 w=780 h=980",
        ),
    );
}

#[test]
fn dormer_volume_is_the_part_above_the_roof() {
    let model = build(&with(
        "r1: roof outer type=gable pitch=45 thickness=200 ridge=x base=L1\ndm1: dormer on=r1 at=4000,1500 w=1200 front=400 pitch=45 type=gable",
    ));
    let lift = 200.0 * 2f64.sqrt();
    let eave = 2700.0 + 1500.0 + lift + 400.0;
    let mut expected = 0.0;
    let steps = 400;
    let (dx, dy) = (1400.0 / steps as f64, 1500.0 / steps as f64);
    for i in 0..steps {
        for j in 0..steps {
            let (x, y) = (
                3300.0 + (i as f64 + 0.5) * dx,
                1500.0 + (j as f64 + 0.5) * dy,
            );
            let dormer = eave + (x - 3300.0).min(4700.0 - x) + lift;
            let roof = 2700.0 + y + lift;
            expected += (dormer - roof).max(0.0) * dx * dy;
        }
    }
    near(volume_of(&model, "dm1"), expected, 5.0e-3);
    assert!(failure(&with("r1: roof outer type=gable pitch=45 thickness=200 ridge=x base=L1\ndm1: dormer on=r1 at=4000,1500 w=1200 front=3000 type=gable")).contains("above the main roof"));
}

#[test]
fn chimney_through_the_roof() {
    let model = build(&with(
        "r1: roof outer type=gable pitch=45 thickness=200 ridge=x base=L1\nch: chimney 6000,3000 w=600 d=600 up_to=r1 over=900",
    ));
    let (_, hi) = body_bounds(&model, "ch");
    near(
        hi[2],
        2700.0 + OUT_Y / 2.0 + 200.0 * 2f64.sqrt() + 900.0,
        1.0e-3,
    );
    let bare = build(&with(
        "r1: roof outer type=gable pitch=45 thickness=200 ridge=x base=L1",
    ));
    let column = 600.0 * 600.0 * 200.0 * 2f64.sqrt();
    near(
        volume_of(&bare, "r1") - volume_of(&model, "r1"),
        column,
        1.0e-3,
    );
    near(
        volume_of(&model, "ch"),
        600.0 * 600.0 * (2700.0 + OUT_Y / 2.0 + 200.0 * 2f64.sqrt() + 900.0),
        1.0e-4,
    );
    let a = model.named_body("r1").expect("r1");
    let b = model.named_body("ch").expect("ch");
    assert!(gcad::geometry::overlap_volume(&a, &b, 64) < 1.0e-3);
}

#[test]
fn chimney_export_is_the_same_every_run() {
    export_twice(
        "chimney_export_is_the_same_every_run",
        &with(
            "r1: roof outer type=hip pitch=40 overhang=300 thickness=200 base=L1\nch: chimney 6500,2000 w=600 d=500 up_to=r1 over=900",
        ),
    );
}

#[test]
fn gutter_and_downpipe() {
    let model = build(&with(
        "r1: roof outer type=gable pitch=45 overhang=300 thickness=200 ridge=x base=L1\ngu: gutter along=r1.eave.1 w=100 slope=0.005\ndp: downpipe from=gu.end d=68 to=L0",
    ));
    let (lo, hi) = body_bounds(&model, "gu");
    near(hi[0] - lo[0], OUT_X + 600.0, 1.0e-3);
    let (lo, _) = body_bounds(&model, "dp");
    near(lo[2], 0.0, 1.0e-6);
    near(
        volume_of(&model, "gu"),
        (OUT_X + 600.0) * 100.0 * 75.0,
        1.0e-4,
    );
    let (_, hi) = body_bounds(&model, "dp");
    near(hi[2], 2400.0 - 75.0 - (OUT_X + 600.0) * 0.005, 1.0e-6);
}

#[test]
fn gutter_export_is_the_same_every_run() {
    export_twice(
        "gutter_export_is_the_same_every_run",
        &with(
            "r1: roof outer type=gable pitch=45 overhang=300 thickness=200 ridge=x base=L1\ngu: gutter along=r1.eave.1 w=100 slope=0.005\ndp: downpipe from=gu.end d=68 to=L0",
        ),
    );
}

#[test]
fn parapet_on_a_flat_roof() {
    let model = build(&with(
        "r1: roof outer type=flat thickness=200 base=L1\npa: parapet outer h=600 coping=50",
    ));
    let (_, hi) = body_bounds(&model, "pa");
    near(hi[2], 2700.0 + 600.0 + 50.0, 1.0e-6);
    let a = model.named_body("r1").expect("r1");
    let b = model.named_body("pa").expect("pa");
    assert!(gcad::geometry::overlap_volume(&a, &b, 64) < 1.0e-3);
    let inside = (OUT_X - 2.0 * T) * (OUT_Y - 2.0 * T);
    near(volume_of(&model, "r1"), inside * 200.0, 1.0e-6);
    let ring = OUT_X * OUT_Y - inside;
    near(volume_of(&model, "pa"), ring * 650.0, 1.0e-6);
}

#[test]
fn parapet_export_is_the_same_every_run() {
    export_twice(
        "parapet_export_is_the_same_every_run",
        &with(
            "r1: roof outer type=flat thickness=200 base=L1\npa: parapet outer h=600 coping=50 overhang=40",
        ),
    );
}

#[test]
fn roof_edges_are_selectors() {
    let text = summary(&with(
        "r1: roof outer type=hip pitch=45 overhang=0 thickness=200 base=L1\nmeasure box r1.ridge",
    ));
    assert!(
        text.contains(&format!("{:.3} x 0.000", OUT_X - OUT_Y)),
        "{text}"
    );
}

#[test]
fn roof_join_meets_two_roofs() {
    let model = build(&with(
        "r1: roof outer type=gable pitch=45 thickness=200 ridge=x base=L1\nr2: roof 3000,6000 5000,6000 5000,8000 3000,8000 closed type=gable pitch=45 thickness=200 ridge=y base=L1\nroof_join r2 r1",
    ));
    let a = model.named_body("r1").expect("r1");
    let b = model.named_body("r2").expect("r2");
    assert!(gcad::geometry::overlap_volume(&a, &b, 64) < 1.0e-3);
    let alone = volume_of(
        &build(&with(
            "r2: roof 3000,6000 5000,6000 5000,8000 3000,8000 closed type=gable pitch=45 thickness=200 ridge=y base=L1",
        )),
        "r2",
    );
    let lift = 200.0 * 2f64.sqrt();
    let mut extra = 0.0;
    let steps = 400;
    let (dx, dy) = (2000.0 / steps as f64, 3000.0 / steps as f64);
    for i in 0..steps {
        for j in 0..steps {
            let (x, y) = (
                3000.0 + (i as f64 + 0.5) * dx,
                3000.0 + (j as f64 + 0.5) * dy,
            );
            let wing = 2700.0 + (x - 3000.0).min(5000.0 - x);
            let host = 2700.0 + y.min(6000.0 - y) + lift;
            extra += (wing + lift - wing.max(host)).max(0.0) * dx * dy;
        }
    }
    near(volume_of(&model, "r2") - alone, extra, 5.0e-3);
}

#[test]
fn roof_extruded_from_a_profile() {
    let model = build(&datums(
        "plane XZ\npoly 0,2700 2000,4700 4000,2700\nr1: roof extrude 3000 thickness=200",
    ));
    let (lo, hi) = body_bounds(&model, "r1");
    near(hi[1] - lo[1], 3000.0, 1.0e-6);
}

#[test]
fn extruded_roof_volume_is_its_section_times_length() {
    let model = build(&datums(
        "plane XZ\npoly 0,2700 2000,4700 4000,2700\nr1: roof extrude 3000 thickness=200",
    ));
    near(
        volume_of(&model, "r1"),
        4000.0 * 200.0 * 2f64.sqrt() * 3000.0,
        1.0e-4,
    );
}

#[test]
fn joined_roof_export_is_the_same_every_run() {
    export_twice(
        "joined_roof_export_is_the_same_every_run",
        &with(
            "r1: roof outer type=gable pitch=45 thickness=200 ridge=x base=L1\nr2: roof 3000,6000 5000,6000 5000,8000 3000,8000 closed type=gable pitch=45 thickness=200 ridge=y base=L1\nroof_join r2 r1\nplane XZ\npoly 0,2700 2000,4700 4000,2700\nr3: roof extrude 3000 thickness=200",
        ),
    );
}

#[test]
fn straight_stair_follows_the_rule() {
    let text = summary(
        "level L0 0\nlevel L1 3000\nst1: stair from=L0 to=L1 at=0,0 dir=0 shape=straight width=1000 rise=auto tread=auto rule=2R+T=630 max_rise=180",
    );
    assert!(text.contains("17 risers of 176.5"), "{text}");
    let tread = 630.0 - 2.0 * 3000.0 / 17.0;
    assert!(text.contains(&format!("16 treads of {tread:.1}")), "{text}");
    assert!(
        text.contains(&format!("going {:.0}", 16.0 * tread)),
        "{text}"
    );
    let model = build(
        "level L0 0\nlevel L1 3000\nst1: stair from=L0 to=L1 at=0,0 dir=0 width=1000 max_rise=180",
    );
    let (lo, hi) = body_bounds(&model, "st1.flight.1");
    near(hi[1] - lo[1], 1000.0, 1.0e-6);
    near(hi[2], TOP_TREAD, 1.0e-6);
    near(lo[2], 0.0, 1.0e-6);
}

#[test]
fn stair_export_is_the_same_every_run() {
    export_twice(
        "stair_export_is_the_same_every_run",
        "level L0 0\nlevel L1 3000\nst1: stair from=L0 to=L1 at=0,0 dir=30 width=1000 stringer=closed nosing=20\nst2: stair from=L0 to=L1 at=3000,0 dir=90 width=900 stringer=mono\nst3: stair from=L0 to=L1 at=6000,0 dir=0 shape=L width=900\nst4: stair from=L0 to=L1 at=0,4000 dir=0 shape=U width=900 landing=1000\nst5: stair from=L0 to=L1 at=6000,4000 dir=0 shape=winder width=900\nst6: stair from=L0 to=L1 at=10000,4000 dir=0 shape=spiral width=800",
    );
}

#[test]
fn stair_flight_volume_is_its_profile() {
    let model = build(
        "level L0 0\nlevel L1 2700\nst1: stair from=L0 to=L1 at=0,0 dir=0 width=1000 max_rise=180",
    );
    let (n, r, t) = (14.0_f64, 180.0_f64, 270.0_f64);
    let depth = 150.0 / r.atan2(t).cos();
    let foot = depth * t / r;
    let area = t * r * n * (n + 1.0) / 2.0 - r / (2.0 * t) * (n * t - foot).powi(2);
    near(volume_of(&model, "st1.flight"), area * 1000.0, 1.0e-3);
}

#[test]
fn stair_shapes() {
    for shape in ["L", "U", "winder", "spiral"] {
        let model = build(&format!(
            "level L0 0\nlevel L1 3000\nst1: stair from=L0 to=L1 at=0,0 dir=0 shape={shape} width=900 landing=900"
        ));
        let (_, hi) = group_bounds(&model, "st1");
        assert!(
            hi[2] >= TOP_TREAD - 1.0e-6 && hi[2] <= 3000.0 + 1.0e-6,
            "{shape} {}",
            hi[2]
        );
    }
    let u = build(
        "level L0 0\nlevel L1 3000\nst1: stair from=L0 to=L1 at=0,0 dir=0 shape=U width=900 landing=900",
    );
    assert_eq!(count_of(&u, "st1.flight"), 2);
    assert_eq!(count_of(&u, "st1.landing"), 1);
}

#[test]
fn stair_stringers_and_nosing() {
    for (stringer, count) in [("closed", 2), ("open", 2), ("mono", 1)] {
        let model = build(&format!(
            "level L0 0\nlevel L1 3000\nst1: stair from=L0 to=L1 at=0,0 dir=0 stringer={stringer} nosing=20"
        ));
        assert_eq!(count_of(&model, "st1.stringer"), count, "{stringer}");
    }
}

#[test]
fn steep_stair_and_low_headroom_fail() {
    assert!(
        failure("level L0 0\nlevel L1 3000\nst1: stair from=L0 to=L1 at=0,0 dir=0 risers=10")
            .contains("2R+T")
    );
    assert!(failure(&with("s1: slab L1 A1 B1 B2 A2 type=concrete:200\nslab_opening s1 1000,1000 2000,1000 2000,2000 1000,2000\nst1: stair from=L0 to=L1 at=1000,1500 dir=0 width=900")).contains("headroom"));
}

#[test]
fn stair_cut_opens_the_slab() {
    let model = build(&with(
        "s1: slab L1 A1 B1 B2 A2 type=concrete:200\nst1: stair from=L0 to=L1 at=1000,1000 dir=0 width=1000 max_rise=180\nstair_cut st1 s1 headroom=2000",
    ));
    assert!(volume_of(&model, "s1") < OUT_X * OUT_Y * 200.0 - 1000.0 * 1000.0 * 200.0);
    let from = (2500.0 - 2000.0 - 180.0) * 270.0 / 180.0;
    near(
        volume_of(&model, "s1"),
        (OUT_X * OUT_Y - (14.0 * 270.0 - from) * 1000.0) * 200.0,
        1.0e-6,
    );
}

#[test]
fn ramp_slope_and_landings() {
    let text = summary(
        "rp1: ramp from=0,0,0 to=13500,0,600 slope=1:20 width=1200 landing=1500 every=10000",
    );
    assert!(text.contains("1 landing"), "{text}");
    assert!(failure("rp1: ramp from=0,0,0 to=6000,0,600 slope=1:20 width=1200").contains("1:10"));
}

#[test]
fn ladder_rungs() {
    let model = build("la: ladder 0,0,0 0,0,3000 width=400 rungs=@300");
    assert_eq!(count_of(&model, "la.rung"), 9);
}

#[test]
fn ramp_volume_and_export_are_steady() {
    let model =
        build("rp1: ramp from=0,0,0 to=13500,0,600 slope=1:20 width=1200 landing=1500 every=10000");
    near(volume_of(&model, "rp1"), 13500.0 * 1200.0 * 200.0, 1.0e-6);
    export_twice(
        "ramp_volume_and_export_are_steady",
        "rp1: ramp from=0,0,0 to=13500,4000,600 slope=1:20 width=1200 landing=1500 every=10000\nla: ladder 0,0,0 300,0,3000 width=400 rungs=@300",
    );
}

#[test]
fn railing_posts_and_balusters() {
    let model = build(
        "rl1: railing path 0,0,0 3000,0,0 height=900 post=50@1500 baluster=12@120 top=round:40",
    );
    assert_eq!(count_of(&model, "rl1.post"), 3);
    assert_eq!(count_of(&model, "rl1.baluster"), 2 * (1500 / 120));
    let (_, hi) = body_bounds(&model, "rl1.top");
    near(hi[2], 900.0, 1.0e-6);
}

#[test]
fn railing_along_a_stair() {
    let model = build(
        "level L0 0\nlevel L1 3000\nst1: stair from=L0 to=L1 at=0,0 dir=0 width=1000 max_rise=180\nrl1: railing along=st1.edge height=900 post=50@1500",
    );
    let (_, hi) = body_bounds(&model, "rl1.top");
    near(hi[2], TOP_TREAD + 900.0, 1.0e-3);
}

#[test]
fn glass_balustrade_and_handrail() {
    let model = build(
        "gb: railing path 0,0,0 3000,0,0 height=1100 glass=12 post=none\nhr: handrail path 0,100,0 3000,100,0 height=900 d=40 bracket=@1000",
    );
    near(
        volume_of(&model, "gb.glass"),
        3000.0 * 1100.0 * 12.0,
        2.0e-2,
    );
    assert_eq!(count_of(&model, "hr.bracket"), 4);
}

#[test]
fn railing_export_is_the_same_every_run() {
    export_twice(
        "railing_export_is_the_same_every_run",
        "level L0 0\nlevel L1 3000\nst1: stair from=L0 to=L1 at=0,0 dir=0 shape=L width=1000\nrl1: railing along=st1.edge height=900 post=50@1500 baluster=12@120\ngb: railing path 0,3000,0 3000,3000,0 3000,5000,0 height=1100 glass=12 post=none\nhr: handrail path 0,3100,0 3000,3100,0 height=900 d=40 bracket=@1000",
    );
}

#[test]
fn columns_at_grid_crossings() {
    let model = build(&datums(
        "c1: column A1 rect 300x300 base=L0 top=L1 material=concrete-c30-37\nc2: column B2 round 300 base=L0 top=L1",
    ));
    near(volume_of(&model, "c1"), 300.0 * 300.0 * H, 1.0e-6);
    near(
        volume_of(&model, "c2"),
        std::f64::consts::PI * 150.0 * 150.0 * H,
        1.0e-3,
    );
    let (lo, hi) = body_bounds(&model, "c2");
    near((lo[0] + hi[0]) / 2.0, 4000.0, 1.0e-6);
    assert_eq!(model.materials["c1"].name, "concrete-c30-37");
}

#[test]
fn column_at_every_grid_crossing() {
    let model = build(&datums("cg: column_grid rect 200x200 base=L0 top=L1"));
    assert_eq!(count_of(&model, "cg"), 4);
}

#[test]
fn column_export_is_the_same_every_run() {
    export_twice(
        "column_export_is_the_same_every_run",
        &datums(
            "c1: column A1 rect 300x300 top=L1\nc2: column B2 round 300 top=L1\nc3: column A2 tube 100x100x5 top=L1 material=steel\ncg: column_grid rect 200x200 base=L1 top=L1+2400",
        ),
    );
}

#[test]
fn beam_between_grid_points() {
    let model = build(&datums("b1: beam A1 B1 bar 200x400 z=L1 material=c24"));
    near(volume_of(&model, "b1"), 4000.0 * 200.0 * 400.0, 1.0e-6);
    let (_, hi) = body_bounds(&model, "b1");
    near(hi[2], 2700.0, 1.0e-6);
}

#[test]
fn joists_across_a_floor() {
    let model = build(&datums(
        "j: joist 0,0 4000,0 4000,3000 0,3000 bar 47x200 spacing=400 dir=y z=L1 material=c24",
    ));
    assert_eq!(count_of(&model, "j"), 11);
}

#[test]
fn truss_member_counts() {
    for (kind, members) in [("king", 4), ("queen", 6), ("fink", 7)] {
        let model = build(&format!(
            "t: truss 0,0,2700 8000,0,2700 type={kind} pitch=30 member=bar 47x150 spacing=600 count=1"
        ));
        assert_eq!(count_of(&model, "t"), members, "{kind}");
    }
    let row = build(
        "t: truss 0,0,2700 8000,0,2700 type=fink pitch=30 member=bar 47x150 spacing=600 count=10",
    );
    assert_eq!(count_of(&row, "t"), 70);
}

#[test]
fn framing_export_is_the_same_every_run() {
    export_twice(
        "framing_export_is_the_same_every_run",
        &datums(
            "b1: beam A1 B1 bar 200x400 z=L1\nj: joist 0,0 4000,0 4000,3000 0,3000 bar 47x200 spacing=400 dir=x z=L1-400\nt: truss 0,0,2700 4000,0,2700 type=fink pitch=30 member=bar 47x150 spacing=600 count=3",
        ),
    );
}

#[test]
#[ignore = "missing: braces"]
fn brace_between_columns() {
    let model = build(&datums(
        "c1: column A1 rect 200x200 top=L1\nc2: column B1 rect 200x200 top=L1\nbr: brace A1 B1 rod 20 from=L0 to=L1 type=x",
    ));
    assert_eq!(count_of(&model, "br"), 2);
}

#[test]
#[ignore = "missing: footings"]
fn footing_under_a_column() {
    let model = build(&datums(
        "c1: column A1 rect 300x300 top=L1\nf1: footing c1 1200x1200x500",
    ));
    near(volume_of(&model, "f1"), 1200.0 * 1200.0 * 500.0, 1.0e-6);
}

#[test]
fn curtain_wall_grid() {
    let model = build(&datums(
        "cw1: curtainwall 0,0 6000,0 height=L1 grid=1500x900 mullion=50x100 glass=double door=cell:2,1",
    ));
    assert_eq!(count_of(&model, "cw1.glass"), 4 * 3 - 1);
    assert_eq!(count_of(&model, "cw1.door"), 1);
}

#[test]
fn louvres_and_fins() {
    let model = build(&datums(
        "lv: louvre 0,0 2000,0 z=900..2100 blade=150x20@100 angle=45\nsf: shading_fin 0,0 4000,0 depth=400 t=20@500 height=L1",
    ));
    assert_eq!(count_of(&model, "lv"), 1200 / 100 + 1);
    assert_eq!(count_of(&model, "sf"), 9);
}

#[test]
fn facade_export_is_the_same_every_run() {
    export_twice(
        "facade_export_is_the_same_every_run",
        &with(
            "cw1: curtainwall 0,-2000 6000,-2000 height=L1 grid=1500x900 mullion=50x100 glass=double door=cell:2,1\nlv: louvre 0,-3000 2000,-3000 z=900..2100 blade=150x20@100 angle=45\nsf: shading_fin 0,-4000 4000,-4000 depth=400 t=20@500 height=L1\nbal: balcony on=outer at=4000 level=L1 w=3000 d=1200 slab=concrete:200 railing=1100",
        ),
    );
}

#[test]
fn balcony_slab_and_railing() {
    let model = build(&with(
        "bal: balcony on=outer at=4000 level=L1 w=3000 d=1200 slab=concrete:200 railing=1100",
    ));
    near(
        volume_of(&model, "bal.slab"),
        3000.0 * 1200.0 * 200.0,
        1.0e-5,
    );
    assert!(count_of(&model, "bal.railing") > 0);
}

#[test]
#[ignore = "missing: rooms"]
fn room_found_from_the_walls() {
    let text = summary(&datums(
        "w: wall 0,0 4000,0 4000,3000 0,3000 closed type=int top=L1 justify=outside\nkitchen: room at=2000,1500 name=\"Kitchen\" floor=tile ceiling=plaster wall=paint",
    ));
    let (a, p) = (3800.0 * 2800.0 / 1.0e6, 2.0 * (3.8 + 2.8));
    assert!(text.contains(&format!("area {a:.2} m²")), "{text}");
    assert!(text.contains(&format!("perimeter {p:.2} m")), "{text}");
    assert!(
        text.contains(&format!("volume {:.2} m³", a * 2.7)),
        "{text}"
    );
}

#[test]
#[ignore = "missing: room checks"]
fn unclosed_room_fails() {
    assert!(
        failure(&datums(
            "w: wall 0,0 4000,0 4000,3000 0,3000 type=int top=L1\nr: room at=2000,1500"
        ))
        .contains("not closed")
    );
}

#[test]
#[ignore = "missing: room separators"]
fn room_separator_splits_a_space() {
    let text = summary(&datums(
        "w: wall 0,0 4000,0 4000,3000 0,3000 closed type=int top=L1 justify=outside\nroom_separator 2000,0 2000,3000\na: room at=1000,1500\nb: room at=3000,1500",
    ));
    assert!(
        text.contains(&format!("area {:.2} m²", 1900.0 * 2800.0 / 1.0e6)),
        "{text}"
    );
}

#[test]
#[ignore = "missing: net and gross areas"]
fn net_and_gross_area() {
    let csv = schedule(
        "net_and_gross_area",
        &with("hall: room at=2000,3000\nliving: room at=6000,3000"),
        "rooms",
    );
    let net = 3650.0 * 5400.0 / 1.0e6;
    near(csv_cell(&csv, "hall", "net_area"), net, 1.0e-4);
    near(csv_cell(&csv, "hall", "height"), 2.7, 1.0e-6);
    near(csv_cell(&csv, "hall", "volume"), net * 2.7, 1.0e-4);
    let gross = schedule(
        "net_and_gross_area_gross",
        &with("hall: room at=2000,3000\narea_scheme gross"),
        "areas",
    );
    near(
        csv_cell(&gross, "L0", "gross_area"),
        OUT_X * OUT_Y / 1.0e6,
        1.0e-6,
    );
}

#[test]
#[ignore = "missing: zones"]
fn zones_sum_their_rooms() {
    let csv = schedule(
        "zones_sum_their_rooms",
        &with("hall: room at=2000,3000\nliving: room at=6000,3000\nzone day hall living"),
        "zones",
    );
    near(
        csv_cell(&csv, "day", "net_area"),
        2.0 * 3650.0 * 5400.0 / 1.0e6,
        1.0e-4,
    );
}

#[test]
#[ignore = "missing: rooms on several levels"]
fn rooms_on_two_levels() {
    let csv = schedule(
        "rooms_on_two_levels",
        &with(
            "up: wall A1 B1 B2 A2 closed type=ext base=L1 top=L2 justify=outside\nhall: room at=2000,3000\nbed: room at=2000,3000 level=L1",
        ),
        "rooms",
    );
    assert!(csv.lines().any(|l| l.starts_with("bed,L1,")), "{csv}");
    near(
        csv_cell(&csv, "bed", "net_area"),
        7400.0 * 5400.0 / 1.0e6,
        1.0e-4,
    );
}

#[test]
#[ignore = "missing: door and window schedules"]
fn door_and_window_schedule() {
    let doors = schedule("door_and_window_schedule", &building(), "doors");
    assert!(doors.lines().next().expect("header").starts_with("mark,"));
    assert!(
        doors.contains("D01,") && doors.contains(",900,2100,"),
        "{doors}"
    );
    let windows = schedule("door_and_window_schedule_w", &building(), "windows");
    assert!(
        windows.contains("W01,") && windows.contains(",1200,1200,900,"),
        "{windows}"
    );
}

#[test]
#[ignore = "missing: wall schedules"]
fn wall_schedule_nets_out_openings() {
    let csv = schedule("wall_schedule_nets_out_openings", &building(), "walls");
    let gross = 2.0 * (OUT_X - T + OUT_Y - T) * H / 1.0e6;
    let net = gross - (900.0 * 2100.0 + 1200.0 * 1200.0) / 1.0e6;
    near(csv_cell(&csv, "outer", "gross_area"), gross, 1.0e-4);
    near(csv_cell(&csv, "outer", "net_area"), net, 1.0e-4);
}

#[test]
#[ignore = "missing: quantities by material and layer"]
fn material_quantities_by_layer() {
    let csv = schedule(
        "material_quantities_by_layer",
        &datums("w1: wall 0,0 4000,0 type=ext top=L1 justify=outside"),
        "materials",
    );
    for layer in ["brick", "pir", "block"] {
        near(csv_cell(&csv, layer, "volume"), 4.0 * 0.1 * 2.7, 1.0e-6);
        near(csv_cell(&csv, layer, "area"), 4.0 * 2.7, 1.0e-6);
    }
}

#[test]
#[ignore = "missing: slab, roof, stair and column schedules"]
fn element_schedules() {
    let source = with(
        "s1: slab L1 A1 B1 B2 A2 type=concrete:200\nr1: roof outer type=gable pitch=45 overhang=0 thickness=200 ridge=x base=L1\nc1: column A1 rect 300x300 top=L1",
    );
    let slabs = schedule("element_schedules_s", &source, "slabs");
    near(
        csv_cell(&slabs, "s1", "area"),
        OUT_X * OUT_Y / 1.0e6,
        1.0e-6,
    );
    let roofs = schedule("element_schedules_r", &source, "roofs");
    near(
        csv_cell(&roofs, "r1", "area"),
        2.0 * OUT_X * OUT_Y / 2.0 / 45f64.to_radians().cos() / 1.0e6,
        1.0e-3,
    );
    let columns = schedule("element_schedules_c", &source, "columns");
    near(csv_cell(&columns, "c1", "volume"), 0.3 * 0.3 * 2.7, 1.0e-6);
}

#[test]
#[ignore = "missing: finish schedules"]
fn finish_schedule() {
    let csv = schedule(
        "finish_schedule",
        &with("hall: room at=2000,3000 floor=tile ceiling=plaster wall=paint"),
        "finishes",
    );
    near(
        csv_cell(&csv, "hall", "floor_area"),
        3650.0 * 5400.0 / 1.0e6,
        1.0e-4,
    );
    assert!(csv.contains(",tile,plaster,paint"), "{csv}");
}

#[test]
#[ignore = "missing: cost columns"]
fn cost_columns_from_rates() {
    let dir = house_files(
        "cost_columns_from_rates",
        &[
            (
                "rates.toml",
                "[brick]\nm2 = 80.0\n[pir]\nm3 = 250.0\n[block]\nm2 = 40.0\n",
            ),
            (
                "model.gcad",
                &datums("w1: wall 0,0 4000,0 type=ext top=L1 justify=outside"),
            ),
        ],
    );
    let (ok, csv) = gcad(
        &dir,
        &[
            "schedule",
            "model.gcad",
            "materials",
            "--csv",
            "--rates",
            "rates.toml",
        ],
    );
    assert!(ok, "{csv}");
    near(csv_cell(&csv, "brick", "cost"), 80.0 * 10.8, 1.0e-6);
    near(csv_cell(&csv, "pir", "cost"), 250.0 * 1.08, 1.0e-6);
    near(
        csv_cell(&csv, "total", "cost"),
        80.0 * 10.8 + 250.0 * 1.08 + 40.0 * 10.8,
        1.0e-6,
    );
}

#[test]
#[ignore = "missing: stable schedule output"]
fn schedule_csv_is_sorted_and_stable() {
    let a = schedule("schedule_csv_is_sorted_a", &building(), "walls");
    let b = schedule("schedule_csv_is_sorted_b", &building(), "walls");
    assert_eq!(a, b);
    let names: Vec<&str> = a
        .lines()
        .skip(1)
        .map(|l| l.split(',').next().unwrap_or(""))
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
#[ignore = "missing: schedules in drawings"]
fn schedule_table_on_a_sheet() {
    let svg = plan_svg(
        "schedule_table_on_a_sheet",
        "part house.gcad\nsheet \"Schedules\" size=A3\nschedule doors place=20,200\nschedule windows",
    );
    assert!(svg.contains(">D01<") && svg.contains(">W01<"));
}

#[test]
#[ignore = "missing: building elements in the bill of materials"]
fn bom_lists_building_elements() {
    let dir = house_files("bom_lists_building_elements", &[]);
    let (ok, text) = gcad(&dir, &["bom", "house.gcad"]);
    assert!(ok, "{text}");
    assert!(
        text.contains("outer.brick") && text.contains("d1.leaf"),
        "{text}"
    );
}

#[test]
#[ignore = "missing: floor plans"]
fn plan_draws_the_cut() {
    let svg = plan_svg(
        "plan_draws_the_cut",
        "part house.gcad\nsheet \"Ground floor\" size=A3 scale=1:50\nplan L0 cut=1200",
    );
    let (layers, pieces_per_layer) = (3, 2);
    assert_eq!(
        svg.matches("class=\"poche\"").count(),
        layers * pieces_per_layer + 1
    );
    assert_eq!(svg.matches("class=\"swing\"").count(), 1);
    assert_eq!(svg.matches("class=\"window\"").count(), 1);
}

#[test]
#[ignore = "missing: plan symbols"]
fn plan_symbols() {
    let svg = plan_svg(
        "plan_symbols",
        "part house.gcad\nsheet \"Ground\" size=A3 scale=1:50\nplan L0 cut=1200 grids=1 levels=1\nnortharrow\nscalebar",
    );
    for words in [">A<", ">B<", ">1<", ">2<", ">N<", "class=\"scalebar\""] {
        assert!(svg.contains(words), "the plan lacks {words}");
    }
}

#[test]
#[ignore = "missing: stair arrows on plans"]
fn plan_stair_arrow() {
    let dir = house_files(
        "plan_stair_arrow",
        &[
            (
                "stair.gcad",
                "level L0 0\nlevel L1 3000\nst1: stair from=L0 to=L1 at=0,0 dir=0 width=1000 max_rise=180",
            ),
            (
                "stair.gdwg",
                "part stair.gcad\nsheet \"Stair\" size=A3 scale=1:50\nplan L0 cut=1200",
            ),
        ],
    );
    let drawing = gcad::drafting::run(&dir.join("stair.gdwg"), &[]).expect("runs");
    let svg = gcad::drafting::svg(&drawing.sheets[0]);
    assert!(svg.contains(">UP<") && svg.contains("class=\"break\""));
}

#[test]
#[ignore = "missing: reflected ceiling plans"]
fn reflected_ceiling_plan() {
    let svg = plan_svg(
        "reflected_ceiling_plan",
        "part house.gcad\nsheet \"RCP\" size=A3 scale=1:50\nrcp L0",
    );
    assert!(svg.contains("class=\"ceiling\""));
}

#[test]
#[ignore = "missing: elevations"]
fn elevation_with_level_marks() {
    let svg = plan_svg(
        "elevation_with_level_marks",
        "part house.gcad\nsheet \"South\" size=A3 scale=1:50\nelevation from=-y",
    );
    assert!(svg.contains(">+0 Ground<") && svg.contains(">+2700 First<"));
}

#[test]
#[ignore = "missing: building sections"]
fn section_through_two_points() {
    let svg = plan_svg(
        "section_through_two_points",
        "part house.gcad\nsheet \"A-A\" size=A3 scale=1:50\nsection a-a 2000,-1000 2000,7000",
    );
    assert_eq!(svg.matches("class=\"hatch brick\"").count(), 2);
    assert!(svg.contains(">A-A<"));
}

#[test]
#[ignore = "missing: material hatches"]
fn hatch_fill_area_matches_the_layer() {
    let svg = plan_svg(
        "hatch_fill_area_matches_the_layer",
        "part house.gcad\nsheet \"Ground\" size=A1 scale=1:50\nplan L0 cut=1200\nhatch brick pattern=brick",
    );
    assert!(svg.contains("data-area=\""));
}

#[test]
#[ignore = "missing: tags"]
fn tags_carry_the_marks() {
    let svg = plan_svg(
        "tags_carry_the_marks",
        &format!(
            "part house.gcad\nsheet \"Ground\" size=A3 scale=1:50\nplan L0 cut=1200\ntag door\ntag window\ntag wall"
        ),
    );
    assert!(svg.contains(">D01<") && svg.contains(">W01<") && svg.contains(">ext<"));
}

#[test]
#[ignore = "missing: room tags"]
fn room_tags_show_the_area() {
    let dir = house_files(
        "room_tags_show_the_area",
        &[
            ("rooms.gcad", &with("hall: room at=2000,3000 name=\"Hall\"")),
            (
                "rooms.gdwg",
                "part rooms.gcad\nsheet \"Ground\" size=A3 scale=1:50\nplan L0 cut=1200\ntag room",
            ),
        ],
    );
    let drawing = gcad::drafting::run(&dir.join("rooms.gdwg"), &[]).expect("runs");
    let svg = gcad::drafting::svg(&drawing.sheets[0]);
    assert!(svg.contains(">Hall<"));
    assert!(svg.contains(&format!(">{:.2} m²<", 3650.0 * 5400.0 / 1.0e6)));
}

#[test]
#[ignore = "missing: automatic dimension chains"]
fn auto_dims_sum_to_the_overall() {
    let svg = plan_svg(
        "auto_dims_sum_to_the_overall",
        "part house.gcad\nsheet \"Ground\" size=A3 scale=1:50\nplan L0 cut=1200\ndim auto walls side=-y\ndim auto openings side=-y\ndim auto grid",
    );
    assert!(svg.contains(">8000<"));
    for (a, b) in [
        (0.0, 1550.0),
        (1550.0, 2450.0),
        (2450.0, 5400.0),
        (5400.0, 6600.0),
        (6600.0, OUT_X),
    ] {
        let words = format!(">{:.0}<", b - a);
        assert!(svg.contains(&words), "no {words}");
    }
}

#[test]
#[ignore = "missing: detail viewports"]
fn viewport_detail_at_a_new_scale() {
    let svg = plan_svg(
        "viewport_detail_at_a_new_scale",
        "part house.gcad\nsheet \"Details\" size=A3 scale=1:50\nplan L0 cut=1200\nviewport window=-500,-500,1000,1000 scale=1:5 label=\"corner\"",
    );
    assert!(svg.contains(">corner 1:5<"));
}

#[test]
#[ignore = "missing: sheet sets"]
fn sheet_set_with_a_drawing_list() {
    let dir = house_files(
        "sheet_set_with_a_drawing_list",
        &[(
            "set.gdwg",
            "part house.gcad\ntitle \"Test house\"\nsheet \"Ground\" size=A1 scale=1:50 number=A-101\nplan L0 cut=1200\nsheet \"South\" size=A1 scale=1:50 number=A-201\nelevation from=-y\nsheet \"Index\" size=A3 number=A-000\ndrawinglist",
        )],
    );
    let drawing = gcad::drafting::run(&dir.join("set.gdwg"), &[]).expect("runs");
    assert_eq!(drawing.sheets.len(), 3);
    let index = gcad::drafting::svg(&drawing.sheets[2]);
    assert!(index.contains(">A-101<") && index.contains(">A-201<"));
}

#[test]
#[ignore = "missing: DXF plans with layers"]
fn plan_dxf_has_layers_by_class() {
    let dir = house_files(
        "plan_dxf_has_layers_by_class",
        &[(
            "plan.gdwg",
            "part house.gcad\nsheet \"Ground\" size=A1 scale=1:50\nplan L0 cut=1200",
        )],
    );
    let (ok, text) = gcad(&dir, &["export", "plan.gdwg", "plan.dxf"]);
    assert!(ok, "{text}");
    let dxf = std::fs::read_to_string(dir.join("plan.dxf")).expect("dxf");
    for layer in ["A-WALL", "A-DOOR", "A-GLAZ", "A-GRID"] {
        assert!(dxf.contains(&format!("\n{layer}\n")), "no {layer}");
    }
}

#[test]
#[ignore = "missing: IFC export"]
fn ifc_has_the_spatial_structure() {
    let ifc = ifc_of("ifc_has_the_spatial_structure", &building());
    assert!(ifc.starts_with("ISO-10303-21;"));
    assert!(ifc.contains("FILE_SCHEMA(('IFC4'))"));
    assert_eq!(entities(&ifc, "IFCPROJECT"), 1);
    assert_eq!(entities(&ifc, "IFCSITE"), 1);
    assert_eq!(entities(&ifc, "IFCBUILDING"), 1);
    assert_eq!(entities(&ifc, "IFCBUILDINGSTOREY"), 3);
    assert_eq!(entities(&ifc, "IFCWALL"), 2);
    assert_eq!(entities(&ifc, "IFCDOOR"), 1);
    assert_eq!(entities(&ifc, "IFCWINDOW"), 1);
    assert_eq!(entities(&ifc, "IFCOPENINGELEMENT"), 2);
    assert_eq!(entities(&ifc, "IFCRELVOIDSELEMENT"), 2);
    assert_eq!(entities(&ifc, "IFCRELFILLSELEMENT"), 2);
    assert!(ifc.trim_end().ends_with("END-ISO-10303-21;"));
}

#[test]
#[ignore = "missing: IFC wall layers"]
fn ifc_wall_layers() {
    let ifc = ifc_of("ifc_wall_layers", &building());
    assert_eq!(entities(&ifc, "IFCMATERIALLAYERSET"), 2);
    assert_eq!(entities(&ifc, "IFCMATERIALLAYER"), 4);
    assert!(ifc.contains("IFCMATERIAL('pir'"));
    assert!(ifc.contains("Pset_WallCommon"));
}

#[test]
#[ignore = "missing: IFC elements"]
fn ifc_element_classes() {
    let ifc = ifc_of(
        "ifc_element_classes",
        &with(
            "s1: slab L1 A1 B1 B2 A2 type=concrete:200\nr1: roof outer type=gable pitch=45 thickness=200 ridge=x base=L1\nst1: stair from=L0 to=L1 at=1000,1000 dir=0 width=900\nrl1: railing along=st1.edge height=900\nc1: column A1 rect 300x300 top=L1\nb1: beam A1 B1 bar 200x400 z=L1\nhall: room at=2000,3000",
        ),
    );
    for (class, count) in [
        ("IFCSLAB", 1),
        ("IFCROOF", 1),
        ("IFCSTAIR", 1),
        ("IFCSTAIRFLIGHT", 1),
        ("IFCRAILING", 1),
        ("IFCCOLUMN", 1),
        ("IFCBEAM", 1),
        ("IFCSPACE", 1),
    ] {
        assert_eq!(entities(&ifc, class), count, "{class}");
    }
    assert!(ifc.contains("NetFloorArea"));
}

#[test]
#[ignore = "missing: stable IFC GlobalIds"]
fn ifc_global_ids_are_stable() {
    let a = ifc_of("ifc_global_ids_are_stable_a", &building());
    let b = ifc_of("ifc_global_ids_are_stable_b", &building());
    let ids = |ifc: &str| -> Vec<String> {
        ifc.lines()
            .filter(|l| l.contains("=IFCWALL("))
            .map(|l| l.split('\'').nth(1).unwrap_or("").to_string())
            .collect()
    };
    assert_eq!(ids(&a), ids(&b));
    assert!(ids(&a).iter().all(|id| id.len() == 22));
}

#[test]
#[ignore = "missing: IFC syntax"]
fn ifc_step_syntax_is_valid() {
    let ifc = ifc_of("ifc_step_syntax_is_valid", &building());
    let mut ids = std::collections::BTreeSet::new();
    for line in ifc.lines().filter(|l| l.starts_with('#')) {
        let id = line.split('=').next().expect("id");
        assert!(ids.insert(id.to_string()), "duplicate {id}");
        assert_eq!(
            line.matches('(').count(),
            line.matches(')').count(),
            "{line}"
        );
    }
    for line in ifc.lines().filter(|l| l.starts_with('#')) {
        for reference in line.split(['(', ',', ')']).filter(|w| w.starts_with('#')) {
            assert!(ids.contains(reference.trim()), "{reference} is not defined");
        }
    }
}

#[test]
#[ignore = "missing: IFC import"]
fn ifc_round_trip() {
    let dir = house_files("ifc_round_trip", &[]);
    let (ok, text) = gcad(&dir, &["export", "house.gcad", "house.ifc"]);
    assert!(ok, "{text}");
    std::fs::write(dir.join("underlay.gcad"), "import house.ifc").expect("underlay");
    let run = gcad::model::run_path(&dir.join("underlay.gcad"), &[], None).expect("runs");
    let original = build(&building());
    near(volume(&run.model), volume(&original), 1.0e-3);
}

#[test]
#[ignore = "missing: IFC checks"]
fn ifc_check_finds_orphans() {
    let dir = house_files("ifc_check_finds_orphans", &[]);
    assert!(gcad(&dir, &["export", "house.gcad", "house.ifc"]).0);
    let (ok, text) = gcad(&dir, &["check", "house.ifc"]);
    assert!(ok, "{text}");
    assert!(text.contains("every element is in a storey"), "{text}");
}

#[test]
#[ignore = "missing: image underlays"]
fn image_underlay() {
    let dir = house_files("image_underlay", &[]);
    let image = image::RgbImage::from_pixel(100, 50, image::Rgb([255, 255, 255]));
    image.save(dir.join("plan.png")).expect("png");
    std::fs::write(
        dir.join("model.gcad"),
        "underlay plan.png scale=1:100 dpi=100",
    )
    .expect("model");
    let run = gcad::model::run_path(&dir.join("model.gcad"), &[], None).expect("runs");
    let text = run
        .steps
        .last()
        .map(|(_, r)| r.as_ref().map(String::clone).unwrap_or_default())
        .unwrap_or_default();
    assert!(text.contains("2540 x 1270"), "{text}");
}

#[test]
#[ignore = "missing: terrain"]
fn terrain_from_points() {
    let dir = house_files(
        "terrain_from_points",
        &[
            (
                "site.csv",
                "0,0,0\n20000,0,2000\n20000,20000,2000\n0,20000,0\n",
            ),
            ("site.gcad", "t1: terrain points site.csv depth=1000"),
        ],
    );
    let run = gcad::model::run_path(&dir.join("site.gcad"), &[], None).expect("runs");
    let area = 20000.0 * 20000.0;
    near(
        volume_of(&run.model, "t1"),
        area * (1000.0 + 1000.0),
        1.0e-4,
    );
}

#[test]
#[ignore = "missing: pads and cut and fill"]
fn cut_and_fill_of_a_pad() {
    let text = summary(
        "t1: terrain grid 20000x20000 slope=0.1,0 at=0,0 depth=1000\np1: pad 5000,5000 15000,5000 15000,15000 5000,15000 level=1000\ncut_fill t1 p1",
    );
    let half = 10000.0 * 5000.0 * 500.0 / 2.0 / 1.0e9;
    assert!(text.contains(&format!("cut {half:.2} m³")), "{text}");
    assert!(text.contains(&format!("fill {half:.2} m³")), "{text}");
}

#[test]
#[ignore = "missing: site boundary"]
fn site_boundary_area_and_setback() {
    let text = summary("site_boundary 0,0 30000,0 30000,20000 0,20000 setback=3000");
    assert!(text.contains("area 600.00 m²"), "{text}");
    assert!(
        failure(&format!(
            "site_boundary 0,0 30000,0 30000,20000 0,20000 setback=3000\n{}",
            building().replace("grid A x=0", "grid A x=-1000")
        ))
        .contains("setback")
    );
}

#[test]
#[ignore = "missing: roads, retaining walls and trees"]
fn site_elements() {
    let model = build(
        "rd: road 0,0 20000,0 width=3500 type=asphalt:100\nrw: retaining 0,5000 20000,5000 h=1200 t=250\ntr: tree 5000,8000 h=6000 spread=4000",
    );
    near(volume_of(&model, "rd"), 20000.0 * 3500.0 * 100.0, 1.0e-5);
    near(volume_of(&model, "rw"), 20000.0 * 1200.0 * 250.0, 1.0e-5);
    assert!(model.named_body("tr").is_ok());
}

#[test]
#[ignore = "missing: U-values"]
fn u_value_of_a_wall() {
    let text = summary(&datums(
        "w1: wall 0,0 4000,0 type=ext top=L1\ncheck uvalue w1",
    ));
    let r = 0.13 + 0.1 / 0.81 + 0.1 / 0.022 + 0.1 / 1.33 + 0.04;
    assert!(text.contains(&format!("U {:.3} W/m²K", 1.0 / r)), "{text}");
}

#[test]
#[ignore = "missing: heat loss"]
fn heat_loss_estimate() {
    let text = summary(&with("check heatloss outer dt=20"));
    let r = 0.13 + 0.1 / 0.81 + 0.1 / 0.022 + 0.1 / 1.33 + 0.04;
    let area = (2.0 * (OUT_X + OUT_Y) * H - 900.0 * 2100.0 - 1200.0 * 1200.0) / 1.0e6;
    near(number_after(&text, "outer "), area / r * 20.0, 1.0e-2);
    assert!(text.contains(" W"), "{text}");
}

#[test]
#[ignore = "missing: thermal bridge flags"]
fn thermal_bridge_at_a_slab_edge() {
    let text = summary(&with(
        "s1: slab L1 A1 B1 B2 A2 type=concrete:200\ncheck bridges",
    ));
    assert!(text.contains("s1 crosses outer.pir"), "{text}");
}

#[test]
#[ignore = "missing: daylight check"]
fn daylight_ratio() {
    let text = summary(&with("living: room at=6000,3000\ncheck daylight min=0.1"));
    let ratio = 1200.0 * 1200.0 / (3650.0 * 5400.0);
    assert!(
        text.contains(&format!("living {ratio:.3} under 0.1")),
        "{text}"
    );
}

#[test]
#[ignore = "missing: stair and ramp checks"]
fn stair_and_ramp_rules() {
    let text = summary(
        "level L0 0\nlevel L1 3000\nst1: stair from=L0 to=L1 at=0,0 dir=0 width=800 max_rise=180\ncheck stairs min_width=900",
    );
    assert!(text.contains("st1 width 800 under 900"), "{text}");
}

#[test]
#[ignore = "missing: door width and turning circle checks"]
fn door_clear_width_check() {
    let text = summary(&with("check doors clear=775 turning=1500"));
    assert!(text.contains("D01 clear 760 under 775"), "{text}");
}

#[test]
#[ignore = "missing: egress, room size and fire compartment checks"]
fn egress_and_room_checks() {
    let text = summary(&with(
        "hall: room at=2000,3000\nliving: room at=6000,3000\ncheck egress max=5000\ncheck rooms min_area=20 min_height=2300\ncheck compartments max=500",
    ));
    let area = 3650.0 * 5400.0 / 1.0e6;
    assert!(
        text.contains(&format!("hall area {area:.2} m² under 20")),
        "{text}"
    );
    assert!(text.contains("living egress"), "{text}");
}

#[test]
#[ignore = "missing: sun position"]
fn sun_position() {
    let text = summary("sun lat=51.5 date=21-06 time=12:00 solar");
    let altitude = 90.0 - 51.5 + 23.44;
    assert!(
        (number_after(&text, "altitude ") - altitude).abs() < 0.5,
        "{text}"
    );
    assert!(
        (number_after(&text, "azimuth ") - 180.0).abs() < 0.5,
        "{text}"
    );
}

#[test]
#[ignore = "missing: shadow study"]
fn shadow_of_a_wall() {
    let text = summary(&datums(
        "w1: wall 0,0 4000,0 type=int top=3000\nsun lat=51.5 date=21-06 time=12:00 solar\ncheck shadow w1",
    ));
    let altitude: f64 = 90.0 - 51.5 + 23.44;
    let length = 3000.0 / altitude.to_radians().tan();
    near(number_after(&text, "shadow "), length, 2.0e-2);
}

#[test]
#[ignore = "missing: mesh walls for games"]
fn mesh_wall_matches_the_kernel() {
    let source = datums(
        "w1: wall 0,0 4000,0 4000,3000 0,3000 closed type=int top=L1 justify=outside\nd1: door on=w1 at=2000 w=900 h=2100",
    );
    let (mesh, _) = mesh_volume(&source);
    near(mesh, volume_of(&build(&source), "w1"), 1.0e-2);
}

#[test]
#[ignore = "missing: mesh house for games"]
fn mesh_house_within_budget() {
    let source = with(
        "s1: slab L1 A1 B1 B2 A2 type=concrete:200\nr1: roof outer type=hip pitch=35 overhang=300 thickness=200 base=L1\nst1: stair from=L0 to=L1 at=1000,1000 dir=0 width=900",
    );
    let started = std::time::Instant::now();
    let (mesh, triangles) = mesh_volume(&source);
    let took = started.elapsed().as_secs_f64();
    assert!(triangles < 4000, "{triangles} triangles");
    assert!(
        took < if cfg!(debug_assertions) { 0.05 } else { 0.005 },
        "{took}s"
    );
    near(mesh, volume(&build(&source)), 1.0e-2);
}

#[test]
#[ignore = "missing: mesh door nodes for games"]
fn mesh_door_is_a_hinged_node() {
    let model = gcad_mesh::build_with(
        &building(),
        &gcad_mesh::Options {
            y_up: false,
            ..Default::default()
        },
    )
    .expect("builds");
    let door = model
        .bodies
        .iter()
        .find(|b| b.name == "d1.leaf")
        .expect("a door node");
    let min_x = door
        .positions
        .iter()
        .map(|p| p[0])
        .fold(f32::INFINITY, f32::min);
    near(min_x as f64, 2000.0 - 450.0 + 70.0, 1.0e-3);
}
