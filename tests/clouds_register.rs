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

fn coarse_recovery(a: &Cloud, b: &Cloud, truth: &M4, voxel: f64, reach: f64, coarse_first: bool) -> (f64, f64) {
    let target = register::with_normals(a).unwrap();
    let moved = ops::transform(b, truth).unwrap();
    let start = if coarse_first {
        register::coarse(&moved, &target, voxel, 1).unwrap()
    } else {
        ops::IDENTITY
    };
    let settings = Settings { plane: true, max_dist: reach, iters: 50, trim: 0.9 };
    let result = icp(&moved, &target, start, &settings).unwrap();
    let error = matrix_error(&result.matrix, truth);
    println!("coarse_first={coarse_first} shift {} angle {}", error.0, error.1);
    error
}

fn offset_motion(about: [f64; 3], axis: [f64; 3]) -> M4 {
    ops::multiply(
        ops::translation([500.0, 0.0, 0.0]),
        ops::about(about, ops::rotation(axis, 90.0)),
    )
}

#[test]
fn coarse_alignment_recovers_a_90_degree_500_offset() {
    let a = bracket_cloud(40000, 1, 0.0);
    let b = bracket_cloud(40000, 2, 0.0);
    let truth = offset_motion([60.0, 40.0, 15.0], [0.0, 0.0, 1.0]);
    let (shift, angle) = coarse_recovery(&a, &b, &truth, 6.0, 15.0, true);
    assert!(shift < 0.01 && angle < 0.01, "shift {shift} angle {angle}");
    let (shift, angle) = coarse_recovery(&a, &b, &truth, 6.0, 1000.0, false);
    assert!(shift >= 0.5 || angle >= 0.1, "icp alone matched: shift {shift} angle {angle}");
    let model = build(&format!(
        "{BRACKET}a: sample part count=40000 seed=1\nb: sample part count=40000 seed=2\nrotate b axis=z angle=90 about=60,40,15\nmove b by=500,0,0\nalign b to=a coarse voxel=6"
    ));
    let rms = rms_to(&cloud_of(&model, "a"), &cloud_of(&model, "b"));
    assert!(rms < 0.5, "rms {rms}");
}

fn bunny_clouds() -> (Cloud, Cloud) {
    let source = format!(
        "bunny: import {} mesh\nclean bunny\na: points bunny every=1\nb: points bunny every=2\nscale a 1000\nscale b 1000",
        fixture("bunny.obj")
    );
    let model = build(&source);
    ((*cloud_of(&model, "a")).clone(), (*cloud_of(&model, "b")).clone())
}

#[test]
fn coarse_alignment_recovers_the_bunny_offset() {
    let (a, b) = bunny_clouds();
    let truth = offset_motion([0.0; 3], [0.0, 1.0, 0.0]);
    let (shift, angle) = coarse_recovery(&a, &b, &truth, 8.0, 40.0, true);
    assert!(shift < 0.01 && angle < 0.01, "shift {shift} angle {angle}");
    let (shift, angle) = coarse_recovery(&a, &b, &truth, 8.0, 1000.0, false);
    assert!(shift >= 0.5 || angle >= 0.1, "icp alone matched: shift {shift} angle {angle}");
}

fn plane_faces_fit_within(model: &Model, noise: f64) -> Result<usize, String> {
    let faces: [&[f64]; 3] = [&[0.0, 40.0, 120.0], &[0.0, 40.0, 90.0], &[0.0, 30.0]];
    let count = value(model, "f.count") as usize;
    let mut checked = 0;
    for k in 1..=count {
        let name = format!("f{k}");
        if cloud_of(model, &name).len() < 300 {
            continue;
        }
        let normal: Vec<f64> = ["x", "y", "z"].iter().map(|a| value(model, &format!("{name}.normal.{a}"))).collect();
        let origin: Vec<f64> = ["x", "y", "z"].iter().map(|a| value(model, &format!("{name}.origin.{a}"))).collect();
        let axis = (0..3).max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs())).unwrap();
        let offset = faces[axis].iter().map(|f| (origin[axis] - f).abs()).fold(f64::INFINITY, f64::min);
        let rms = value(model, &format!("{name}.rms"));
        println!("{name} axis {axis} normal {normal:?} origin {origin:?} points {} offset error {offset} rms {rms}", cloud_of(model, &name).len());        if offset > 0.1 {
            return Err(format!("{name} is {offset} from every face on its axis"));
        }
        if rms > 3.0 * noise {
            return Err(format!("{name} residual rms {rms} above 3x noise"));
        }
        checked += 1;
    }
    Ok(checked)
}

fn partial_scans(motion: &str, align: &str) -> Model {
    build(&format!(
        "{BRACKET}a: sample part count=60000 seed=1 noise=0.05 box=-1,-1,-1,70,100,40\nb: sample part count=60000 seed=2 noise=0.05 box=30,-1,-1,130,100,40\n{motion}{align}merge a b into all\nfit all plane tol=0.3 into=f all max=20"
    ))
}

#[test]
fn overlapping_partial_scans_merge_into_a_clean_cloud() {
    let model = partial_scans(
        "rotate b axis=z angle=3 about=60,40,15\nmove b by=2,-1,1\n",
        "align b to=a icp plane max_dist=20 iters=100\n",
    );
    assert_eq!(cloud_of(&model, "all").scan_ids().len(), 2);
    let checked = plane_faces_fit_within(&model, 0.05).expect("planes fit within the noise");
    assert!(checked >= 6, "only {checked} large planes");
}

#[test]
fn a_scan_left_0_3_mm_off_fails_the_plane_check() {
    let model = partial_scans("move b by=0.3,0,0\n", "");
    assert!(plane_faces_fit_within(&model, 0.05).is_err());
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
fn sampling_a_solid_lies_on_its_surface_with_the_requested_noise() {
    let flat = bracket_cloud(5000, 1, 0.0);
    assert_eq!(flat.len(), 5000);
    let on_face = |p: &[f64; 3]| {
        let near = |v: f64, f: f64| (v - f).abs() < 1.0e-6;
        near(p[2], 0.0) || near(p[2], 30.0) || near(p[1], 0.0) || near(p[0], 0.0) || near(p[0], 120.0) || near(p[1], 90.0) || near(p[1], 40.0) || near(p[0], 40.0)
    };
    assert!(flat.positions.iter().all(on_face));
    let noisy = bracket_cloud(5000, 1, 0.1);
    let off = noisy.positions.iter().filter(|p| !on_face(p)).count();
    assert!(off > 4000);
    let boxed = build(&format!("{BRACKET}s: sample part count=2000 box=-1,-1,-1,20,100,40"));
    assert!(cloud_of(&boxed, "s").positions.iter().all(|p| p[0] <= 20.0));
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
