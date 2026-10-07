mod common;

use common::*;
use linecad::{export, render};

#[test]
fn step() {
    let model = build(&part("plate"));
    let path = scratch("plate.step");
    export::export(&model.solids(), &path).expect("exports");
    let text = std::fs::read_to_string(&path).expect("written");
    assert!(text.starts_with("ISO-10303-21;"));
    assert!(text.contains("MANIFOLD_SOLID_BREP") && text.contains("CLOSED_SHELL"));
}

#[test]
#[ignore = "needs OpenCascade: set LINECAD_OCP_PYTHON to a python with OCP and run with --ignored"]
fn step_opens_in_opencascade() {
    let python = std::env::var("LINECAD_OCP_PYTHON").expect("LINECAD_OCP_PYTHON");
    let script = r#"
import sys
from OCP.STEPControl import STEPControl_Reader
from OCP.BRepCheck import BRepCheck_Analyzer
from OCP.BRepGProp import BRepGProp
from OCP.GProp import GProp_GProps
r = STEPControl_Reader()
r.ReadFile(sys.argv[1])
r.TransferRoots()
shape = r.OneShape()
props = GProp_GProps()
BRepGProp.VolumeProperties_s(shape, props, 1e-7)
print(BRepCheck_Analyzer(shape).IsValid(), props.Mass())
"#;
    let inline = [
        ("rounded-box", "rect 30 20\nbase: extrude 10\nfillet 2 all"),
        (
            "chamfered-box",
            "rect 30 20\nbase: extrude 10\nchamfer 2 all",
        ),
        (
            "revolve-cut",
            "rect 40 40\nbase: extrude 10\nplane XZ\nrect 4 4 at=10,10\nrevolve 360 axis=y mode=cut",
        ),
        ("rounded-poly", "poly 0,0 30,0 30,20 0,20 r=4\nextrude 5"),
        (
            "partly-rounded",
            "rect 20 20\nbase: extrude 20\nfillet 2 base.end&base.side|>X&>Y",
        ),
        (
            "counterbored",
            "rect 40 30\nbase: extrude 10\nplane base.end\nhole 3.2 10,0 cbore=6,3\nhole 3.2 -10,0 csink=6.4,90",
        ),
        ("mirrored", "rect 20 10 at=10,0\nextrude 5\nmirror YZ"),
        (
            "lofted",
            "rect 20 20\nsection\nplane XY offset=10\ncircle 10\nsection\nloft",
        ),
        ("spring", "circle 2\nhelix r=10 pitch=5 turns=2\nsweep"),
        (
            "drafted",
            "rect 20 20\nbase: extrude 10\ndraft 5 base.side neutral=base.start",
        ),
        (
            "shelled",
            "rect 40 30 r=4\nbase: extrude 20\nshell 2 open=base.end",
        ),
        (
            "lettered",
            "rect 60 15\nbase: extrude 2\nplane base.end\ntext CAD size=8 at=-10,-4\nextrude 1",
        ),
        (
            "sloted",
            "slot 30 10\nextrude 3\nplane XY offset=3\nellipse 10 6\nextrude 2",
        ),
    ];
    let named = ["plate", "bracket", "pipe", "ring"].map(|name| (name, part(name)));
    for (name, source) in named.iter().map(|(n, s)| (*n, s.as_str())).chain(inline) {
        let model = build(source);
        let path = scratch(&format!("{name}.step"));
        export::export(&model.solids(), &path).expect("exports");
        let output = std::process::Command::new(&python)
            .args(["-c", script, &path])
            .output()
            .expect("python runs");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut words = stdout.split_whitespace();
        assert_eq!(
            words.next(),
            Some("True"),
            "{name}: OpenCascade rejects the shape: {stdout} {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let occt: f64 = words.next().and_then(|w| w.parse().ok()).expect("a volume");
        let ours = volume(&model);
        assert!(
            (occt - ours).abs() < ours * 0.001,
            "{name}: OpenCascade volume {occt}, ours {ours}"
        );
    }
}

#[test]
fn stl() {
    let model = build("rect 10 10\nextrude 5");
    let path = scratch("box.stl");
    export::export(&model.solids(), &path).expect("exports");
    let bytes = std::fs::read(&path).expect("written");
    let triangles = u32::from_le_bytes(bytes[80..84].try_into().expect("header")) as usize;
    assert_eq!(bytes.len(), 84 + triangles * 50);
    let vertex = |t: usize, k: usize| -> [f64; 3] {
        let at = 84 + t * 50 + 12 + k * 12;
        [0, 4, 8].map(|o| {
            f32::from_le_bytes(bytes[at + o..at + o + 4].try_into().expect("float")) as f64
        })
    };
    let signed: f64 = (0..triangles)
        .map(|t| {
            let (a, b, c) = (vertex(t, 0), vertex(t, 1), vertex(t, 2));
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum();
    assert!((signed - 500.0).abs() < 1.0e-3, "STL encloses {signed}");
}

#[test]
fn png() {
    let model = build(&part("plate"));
    let path = scratch("plate.png");
    render::render(&model.solids(), &path).expect("renders");
    let image = image::open(&path).expect("a png").to_rgb8();
    assert_eq!(image.dimensions(), (1200, 1200));
    let background = *image.get_pixel(5, 5);
    let drawn = image.pixels().filter(|p| **p != background).count();
    assert!(drawn > 50_000, "only {drawn} pixels drawn");
}

#[test]
fn step_import() {
    let model = build(&part("plate"));
    let path = scratch("import.step");
    export::export(&model.solids(), &path).expect("exports");
    assert_volume(&build(&format!("import {path}")), volume(&model), 0.0005);
}

#[test]
#[ignore = "missing: obj export"]
fn obj() {
    let path = scratch("box.obj");
    export::export(&build("rect 10 10\nextrude 5").solids(), &path).expect("exports");
    assert!(
        std::fs::read_to_string(&path)
            .expect("written")
            .lines()
            .any(|l| l.starts_with("f "))
    );
}

#[test]
#[ignore = "missing: 3mf export"]
fn three_mf() {
    let path = scratch("box.3mf");
    export::export(&build("rect 10 10\nextrude 5").solids(), &path).expect("exports");
    assert_eq!(&std::fs::read(&path).expect("written")[..2], b"PK");
}

#[test]
#[ignore = "missing: svg drawing export"]
fn drawing() {
    let path = scratch("plate.svg");
    export::export(&build(&part("plate")).solids(), &path).expect("exports");
    assert!(
        std::fs::read_to_string(&path)
            .expect("written")
            .contains("<path")
    );
}

#[test]
#[ignore = "missing: stl import"]
fn stl_import() {
    let path = scratch("import.stl");
    export::export(&build("rect 10 10\nextrude 5").solids(), &path).expect("exports");
    assert_volume(&build(&format!("import {path}")), 500.0, 1.0e-6);
}

#[test]
#[ignore = "missing: color"]
fn step_colours() {
    let path = scratch("red.step");
    export::export(&build("rect 10 10\nextrude 5\ncolor red").solids(), &path).expect("exports");
    assert!(
        std::fs::read_to_string(&path)
            .expect("written")
            .contains("COLOUR_RGB")
    );
}
