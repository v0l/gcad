mod common;

use common::*;
use linecad::{export, render};

fn scratch(name: &str) -> String {
    let dir = std::env::temp_dir().join("linecad-tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name).to_string_lossy().into_owned()
}

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
    for name in ["plate", "bracket", "pipe", "ring"] {
        let model = build(&part(name));
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
