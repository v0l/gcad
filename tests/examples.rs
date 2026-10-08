mod common;

use common::*;
use std::f64::consts::PI;

#[test]
fn plate() {
    assert_volume(&build(&part("plate")), 3441.98, 0.001);
}

#[test]
fn bracket() {
    assert_volume(&build(&part("bracket")), 13800.03, 0.001);
}

#[test]
fn ring() {
    assert_volume(&build(&part("ring")), PI * (100.0 - 36.0) * 10.0, 0.0005);
}

#[test]
fn pipe() {
    let centerline = 12.0 + 14.0 + 12.0 + 2.0 * (PI / 2.0 * 8.0);
    assert_volume(&build(&part("pipe")), PI * 9.0 * centerline, 0.0005);
}

#[test]
fn enclosure_case() {
    assert_volume(&build(&part("enclosure-case")), 25240.3, 0.001);
}

#[test]
fn enclosure_lid() {
    assert_volume(&build(&part("enclosure-lid")), 8841.8, 0.001);
}

#[test]
fn enclosure_assembly() {
    let path = std::path::PathBuf::from(format!(
        "{}/examples/assemblies/enclosure.gasm",
        env!("CARGO_MANIFEST_DIR")
    ));
    let run = gcad::model::run_path(&path, &[], None).expect("runs");
    assert!(run.steps.iter().all(|(_, r)| r.is_ok()));
    assert_eq!(
        run.model.body_names(),
        ["case", "lid", "s1", "s1_2", "s1_3", "s1_4"]
    );
    let opened = gcad::model::run_path(&path, &[("open".to_string(), -75.0)], None).expect("runs");
    let (_, last) = opened.steps.last().expect("steps");
    assert!(
        format!("{:#}", last.as_ref().expect_err("screws hold the lid")).contains("pull apart")
    );
}

#[test]
fn linkage() {
    let path = std::path::PathBuf::from(format!(
        "{}/examples/assemblies/linkage.gasm",
        env!("CARGO_MANIFEST_DIR")
    ));
    for turn in [0.0, 90.0, 180.0, 270.0] {
        let run = gcad::model::run_path(&path, &[("turn".to_string(), turn)], None).expect("runs");
        let (line, last) = run.steps.last().expect("steps");
        assert!(last.is_ok(), "turn {turn}: line {}: {last:?}", line.number);
        assert!(
            run.model
                .mates
                .iter()
                .all(|m| m.holds([monstertruck::modeling::Matrix4::from_scale(1.0); 2]))
        );
    }
}
