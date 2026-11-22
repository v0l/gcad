mod common;

use common::*;
use gcad::model::Model;
use gcad_cloud::Cloud;
use std::sync::Arc;

const CUBE: &str = "body part\nrect 20 20 at=10,10\nextrude 20\n";

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/clouds/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn cube_scan() -> String {
    format!("{CUBE}scan: cloud {}\n", fixture("cube_binary.ply"))
}

fn value(model: &Model, name: &str) -> f64 {
    *model.scope.get(name).unwrap_or_else(|| panic!("no value `{name}`"))
}

fn cloud_of(model: &Model, name: &str) -> Arc<Cloud> {
    model.cloud(name).expect("a cloud body").clone()
}

fn near(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} is not within {tolerance} of {b}");
}

#[test]
fn deviation_of_an_offset_cloud_has_the_right_mean_and_sign() {
    let model = build(&format!(
        "{}cloud_offset scan by=0.2\ndeviation scan to=part max=0.5 into=d",
        cube_scan()
    ));
    near(value(&model, "d.mean"), 0.2, 0.01);
    near(value(&model, "d.min"), 0.2, 0.06);
    near(value(&model, "d.max"), 0.2, 0.06);
    assert!(value(&model, "d.std") < 0.03);
    assert_eq!(value(&model, "d.within"), 100.0);
    let inward = build(&format!(
        "{}cloud_offset scan by=-0.2\ndeviation scan to=part max=0.1 into=d",
        cube_scan()
    ));
    near(value(&inward, "d.mean"), -0.2, 0.05);
    assert!(value(&inward, "d.within") < 30.0);
}

#[test]
fn deviation_flags_only_the_moved_face() {
    let model = build(&format!(
        "{}cloud_offset scan by=1 box=19.5,0.5,0.5,21,19.5,19.5 dir=1,0,0\ndeviation scan to=part max=0.5 into=d",
        cube_scan()
    ));
    let scan = cloud_of(&model, "scan");
    let deviation = scan.deviation.as_ref().expect("a deviation column");
    let flagged: Vec<usize> = (0..scan.len()).filter(|&i| deviation[i].abs() > 0.5).collect();
    assert_eq!(flagged.len(), 361);
    assert!(flagged.iter().all(|&i| (scan.positions[i][0] - 21.0).abs() < 0.06));
    near(100.0 - value(&model, "d.within"), 100.0 * 361.0 / 2646.0, 0.01);
    near(value(&model, "d.max"), 1.0, 0.06);
    near(value(&model, "d.min"), 0.0, 0.06);
}

#[test]
fn deviation_works_against_a_mesh_body() {
    let model = build(&format!(
        "ball: blob 0,0,0 r=10\nremesh 0.5\nscan: sample ball count=4000\ncloud_offset scan by=0.3 dir=0,0,1\ndeviation scan to=ball max=0.5 into=d"
    ));
    assert!(value(&model, "d.max") > 0.25 && value(&model, "d.max") < 0.31);
    assert!(value(&model, "d.within") > 99.0);
}

#[test]
fn spacing_of_two_fitted_cylinders_matches_the_cad() {
    let model = build(
        "scan: cloud_synthetic two_cylinders r=5 spacing=30 noise=0.1 count=40000\nfit scan cylinder tol=0.5 into=c all max=2 min=2000\nmeasure scan c1 c2 into=gap\nmeasure scan diameter c1 into=dia",
    );
    near(value(&model, "gap.distance"), 30.0, 0.1);
    near(value(&model, "dia.diameter"), 10.0, 0.1);
}

#[test]
fn point_to_plane_measure_is_signed() {
    let model = build(&format!(
        "{}fit scan plane tol=0.3 into=f all max=6\nmeasure scan 5,5,30 f1 into=h",
        cube_scan()
    ));
    let d = value(&model, "h.signed").abs();
    assert!([10.0, 20.0, 30.0, 5.0, 15.0, 25.0].iter().any(|e| (d - e).abs() < 0.3), "{d}");
    assert!(model.scope.contains_key("h.signed"));
}

#[test]
fn check_lines_use_the_deviation_values() {
    let passing = build(&format!(
        "{}cloud_offset scan by=0.2\ndeviation scan to=part max=0.5 into=d\ncheck d.max < 0.3",
        cube_scan()
    ));
    assert!(passing.checks.last().unwrap().passed);
    let failing = failure(&format!(
        "{}cloud_offset scan by=0.4\ndeviation scan to=part max=0.5 into=d\ncheck d.max < 0.3",
        cube_scan()
    ));
    assert!(failing.contains("d.max"), "{failing}");
}

#[test]
fn deviation_colour_map_colours_by_sign() {
    let model = build(&format!(
        "{}cloud_offset scan by=1 box=19.5,-1,-1,21,21,21 dir=1,0,0\ndeviation scan to=part max=0.5 into=d",
        cube_scan()
    ));
    let scan = cloud_of(&model, "scan");
    let moved = (0..scan.len()).find(|&i| scan.positions[i][0] > 20.5).unwrap();
    let still = (0..scan.len()).find(|&i| scan.positions[i][0] < 5.0).unwrap();
    let colours = scan.colours.as_ref().unwrap();
    assert_eq!(colours[moved], gcad_cloud::colour::diverging(1.0));
    assert_eq!(colours[still], gcad_cloud::colour::diverging(0.5));
}

fn scratch(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("gcad-inspect-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).display().to_string()
}

fn inspect(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
        .arg("inspect")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("runs")
}

#[test]
fn inspection_report_csv_is_stable() {
    let path = scratch("report.csv");
    let run = || {
        let out = inspect(&["examples/clouds/inspect.gcad", "--report", &path]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        std::fs::read_to_string(&path).expect("a report")
    };
    let first = run();
    assert_eq!(first, run());
    assert!(first.contains("deviation,d,points,60000"));
    assert!(first.contains("check,line 6,d.max<2,pass"));
    assert!(first.ends_with("summary,checks,failed,0\n"));
}

#[test]
fn inspection_report_pdf_is_written_and_failing_checks_exit_nonzero() {
    let pdf = scratch("report.pdf");
    assert!(inspect(&["examples/clouds/inspect.gcad", "--report", &pdf]).status.success());
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert!(bytes.ends_with(b"%%EOF\n"));
    assert!(bytes.len() < 1_000_000, "{} bytes", bytes.len());
    let source = std::fs::read_to_string(format!("{}/examples/clouds/inspect.gcad", env!("CARGO_MANIFEST_DIR")))
        .unwrap()
        .replace("check d.max < 2", "check d.max < 0.3")
        .replace("include ../robot/", &format!("include {}/examples/robot/", env!("CARGO_MANIFEST_DIR")));
    let failing = scratch("failing.gcad");
    std::fs::write(&failing, source).unwrap();
    let csv = scratch("failing.csv");
    let out = inspect(&[&failing, "--report", &csv]);
    assert!(!out.status.success());
    let text = std::fs::read_to_string(&csv).unwrap();
    assert!(text.contains("FAIL"), "{text}");
}

#[test]
fn scan_section_is_drawn_beside_the_cad_section() {
    let dir = scratch("section");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        format!("{dir}/p.gcad"),
        format!("{CUBE}scan: cloud {}\ncloud_offset scan by=1 box=19.5,0.5,0.5,21,19.5,19.5 dir=1,0,0\ndeviation scan to=part max=0.5 into=d\n", fixture("cube_binary.ply")),
    )
    .unwrap();
    std::fs::write(
        format!("{dir}/s.gdwg"),
        "part p.gcad\nsheet \"s\" size=A3 scale=1:0.5\nscansection scan to=part axis=y at=10 thickness=1 tol=0.5\n",
    )
    .unwrap();
    let drawing = gcad::drafting::run(std::path::Path::new(&format!("{dir}/s.gdwg")), &[]).unwrap();
    if let Some((line, error)) = drawing.failed() {
        panic!("{}: {error:#}", line.text);
    }
    let text = drawing.steps.last().unwrap().1.as_ref().unwrap().clone();
    assert!(text.contains("beyond 0.5"), "{text}");
    let svg = gcad::drafting::svg(&drawing.sheets[0]);
    assert!(svg.contains("CAD section y=10"));
    assert!(svg.contains("scan section y=10"));
    assert!(svg.contains("#d91a1a"), "red deviation markers");
}

#[test]
fn deviation_colour_map_renders() {
    let model = build(&format!(
        "{}cloud_offset scan by=1 box=19.5,0.5,0.5,21,19.5,19.5 dir=1,0,0\ndeviation scan to=part max=0.5 into=d",
        cube_scan()
    ));
    let path = scratch("map.png");
    gcad::cloud_render::render_clouds(&model, &path).unwrap();
    let image = image::open(&path).unwrap().to_rgb8();
    assert_eq!(image.width(), 1350);
    let seen = |c: [u8; 3]| image.pixels().any(|p| p.0 == c);
    assert!(seen(gcad_cloud::colour::diverging(1.0)), "the moved face is red");
    assert!(seen(gcad_cloud::colour::diverging(0.5)), "the rest is green");
    assert!(image.get_pixel(1215, 100).0 != [255, 255, 255], "legend bar drawn");
}
