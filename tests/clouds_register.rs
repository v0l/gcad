mod common;

use common::*;
use gcad::model::{Model, run};
use gcad::parse::parse_program;
use gcad_cloud::ops::{self, M4};
use gcad_cloud::register::{self, Settings, icp};
use gcad_cloud::{Cloud, KdTree};
use std::sync::Arc;

const BRACKET: &str = "body part\npoly 0,0 120,0 120,40 40,40 40,90 0,90\nextrude 30\n";

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/clouds/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn cloud_of(model: &Model, name: &str) -> Arc<Cloud> {
    model.cloud(name).expect("a cloud body").clone()
}

fn value(model: &Model, name: &str) -> f64 {
    *model.scope.get(name).unwrap_or_else(|| panic!("no value `{name}`"))
}

fn reports(source: &str) -> Vec<String> {
    let lines = parse_program(source).expect("parses");
    run(&lines)
        .steps
        .into_iter()
        .map(|(line, result)| match result {
            Ok(text) => text,
            Err(error) => panic!("line {} `{}`: {error:#}", line.number, line.text),
        })
        .collect()
}

fn reported_rms(text: &str) -> f64 {
    let after = text.split("rms ").nth(1).expect("an rms in the report");
    after.split([',', ';', ' ']).next().unwrap().parse().unwrap()
}

fn bracket_cloud(count: usize, seed: u64, noise: f64) -> Arc<Cloud> {
    let model = build(&format!("{BRACKET}s: sample part count={count} seed={seed} noise={noise}"));
    cloud_of(&model, "s")
}

fn rms_to(a: &Cloud, b: &Cloud) -> f64 {
    let tree = KdTree::build(&a.positions);
    (b.positions.iter().map(|p| tree.nearest(*p, 1)[0].1.powi(2)).sum::<f64>() / b.len() as f64).sqrt()
}

fn matrix_error(found: &M4, truth: &M4) -> (f64, f64) {
    let relative = ops::multiply(*found, *truth);
    let shift = (0..3).map(|k| relative[k][3].powi(2)).sum::<f64>().sqrt();
    (shift, register::rotation_angle(&relative))
}

fn known_motion() -> M4 {
    ops::multiply(
        ops::translation([4.0, -3.0, 2.0]),
        ops::about([60.0, 40.0, 15.0], ops::rotation([0.3, 0.5, 0.8], 6.0)),
    )
}

fn inverse_of(m: &M4) -> M4 {
    let mut inv = ops::IDENTITY;
    for i in 0..3 {
        for j in 0..3 {
            inv[i][j] = m[j][i];
        }
        inv[i][3] = -(0..3).map(|j| m[j][i] * m[j][3]).sum::<f64>();
    }
    inv
}

#[test]
fn icp_recovers_a_known_transform_for_point_and_plane() {
    let target = register::with_normals(&bracket_cloud(30000, 1, 0.0)).unwrap();
    let truth = known_motion();
    let moved = ops::transform(&target, &truth).unwrap();
    for plane in [false, true] {
        let settings = Settings { plane, max_dist: 30.0, iters: 300, trim: 1.0 };
        let result = icp(&moved, &target, ops::IDENTITY, &settings).unwrap();
        let (shift, angle) = matrix_error(&result.matrix, &truth);
        println!(
            "plane={plane} iterations {} rms {} shift {shift} angle {angle}",
            result.iterations, result.rms
        );
        assert!(shift < 0.1, "plane={plane} shift {shift}");
        assert!(angle < 0.05, "plane={plane} angle {angle}");
        assert!(result.iterations >= 1);
        let _ = inverse_of(&truth);
    }
}

#[test]
fn align_icp_reports_iterations_rms_and_a_transform_line() {
    let steps = reports(&format!(
        "{BRACKET}a: sample part count=30000 seed=1\nb: sample part count=30000 seed=2\nrotate b axis=z angle=4 about=60,40,15\nmove b by=3,-2,1\nnormals a k=20\nalign b to=a icp plane max_dist=30 iters=100"
    ));
    let text = steps.last().unwrap();
    assert!(text.contains("iterations"), "{text}");
    assert!(text.contains("transform b "), "{text}");
    assert!(reported_rms(text) < 0.3, "{text}");
}

#[test]
fn point_pairs_with_three_exact_pairs_give_zero_rms() {
    let motion = known_motion();
    let source = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 20.0, 5.0]];
    let pairs: Vec<String> = source
        .iter()
        .map(|p| {
            let q = register::apply(&motion, *p);
            format!("{},{},{}>{},{},{}", p[0], p[1], p[2], q[0], q[1], q[2])
        })
        .collect();
    let steps = reports(&format!(
        "a: cloud_synthetic count=200 seed=1\nb: cloud_synthetic count=200 seed=1\nalign b to=a pairs={}",
        pairs.join(" ")
    ));
    let text = steps.last().unwrap();
    assert!(reported_rms(text) < 1.0e-9, "{text}");
    let direct = register::fit_rigid(&source, &source.map(|p| register::apply(&motion, p))).unwrap();
    let (shift, angle) = matrix_error(&direct, &inverse_of(&motion));
    assert!(shift < 1.0e-9 && angle < 1.0e-6);
}

#[test]
fn coarse_alignment_recovers_a_90_degree_500_offset() {
    let model = build(&format!(
        "{BRACKET}a: sample part count=40000 seed=1\nb: sample part count=40000 seed=2\nrotate b axis=z angle=90 about=60,40,15\nmove b by=500,0,0\nalign b to=a coarse voxel=6"
    ));
    let rms = rms_to(&cloud_of(&model, "a"), &cloud_of(&model, "b"));
    assert!(rms < 0.5, "rms {rms}");
}

#[test]
fn coarse_alignment_recovers_the_bunny_offset() {
    let source = format!(
        "bunny: import {} mesh\nclean bunny\na: points bunny every=1\nb: points bunny every=2\nscale a 1000\nscale b 1000\nrotate b axis=y angle=90\nmove b by=500,0,0\nalign b to=a coarse voxel=8",
        fixture("bunny.obj")
    );
    let model = build(&source);
    let rms = rms_to(&cloud_of(&model, "a"), &cloud_of(&model, "b"));
    assert!(rms < 1.0, "rms {rms}");
}

#[test]
fn overlapping_partial_scans_merge_into_a_clean_cloud() {
    let source = format!(
        "{BRACKET}a: sample part count=40000 seed=1 noise=0.05 box=-1,-1,-1,70,100,40\nb: sample part count=40000 seed=2 noise=0.05 box=50,-1,-1,130,100,40\nrotate b axis=z angle=3 about=60,40,15\nmove b by=2,-1,1\nalign b to=a icp plane max_dist=20 iters=100\nmerge a b into all\nfit all plane tol=0.3 into=f all max=20"
    );
    let model = build(&source);
    assert_eq!(cloud_of(&model, "all").scan_ids().len(), 2);
    assert!(value(&model, "f.count") >= 6.0);
}

#[test]
fn align_scans_chains_sequentially_and_says_so() {
    let steps = reports(&format!(
        "{BRACKET}a: sample part count=20000 seed=1\nb: sample part count=20000 seed=2\nc: sample part count=20000 seed=3\nrotate b axis=z angle=2 about=60,40,15\nrotate c axis=z angle=-2 about=60,40,15\nalign scans a b c max_dist=20 iters=100"
    ));
    let text = steps.last().unwrap();
    assert!(text.contains("sequential"), "{text}");
    assert_eq!(text.matches("transform ").count(), 2, "{text}");
}

#[test]
fn scan_to_cad_icp_gives_an_rms_within_the_noise() {
    let steps = reports(&format!(
        "{BRACKET}s: sample part count=20000 seed=3 noise=0.1\nrotate s axis=x angle=3 about=60,40,15\nmove s by=2,2,-2\nalign s to=part icp plane max_dist=15 iters=100"
    ));
    let rms = reported_rms(steps.last().unwrap());
    assert!(rms < 0.15, "rms {rms}");
}

#[test]
fn align_errors_say_what_is_wrong() {
    let few = failure(
        "a: cloud_synthetic count=100\nb: cloud_synthetic count=100 seed=2\nalign b to=a pairs=0,0,0>1,1,1 1,0,0>2,1,1",
    );
    assert!(few.contains("at least 3"), "{few}");
    let plain = failure("a: cloud_synthetic count=100\nalign a to=nothing icp");
    assert!(plain.contains("nothing"), "{plain}");
}

#[test]
fn register_example_runs() {
    let source = std::fs::read_to_string(format!(
        "{}/examples/clouds/register.gcad",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("example exists");
    let model = build(&source);
    assert!(cloud_of(&model, "all").len() > 0);
}
