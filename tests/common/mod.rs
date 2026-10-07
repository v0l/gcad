#![allow(dead_code)]

use linecad::geometry;
use linecad::model::{Model, run};
use linecad::parse::parse_program;

pub const SPANDREL: f64 = 1.0 - std::f64::consts::PI / 4.0;
pub const SPANDREL_CENTROID: f64 =
    (10.0 - 3.0 * std::f64::consts::PI) / (12.0 - 3.0 * std::f64::consts::PI);
pub const ROUND_CORNER_OVERLAP: f64 = 0.095_870_338;

pub fn build(source: &str) -> Model {
    let lines = parse_program(source).expect("parses");
    let result = run(&lines);
    if let Some((line, Err(error))) = result.steps.last() {
        panic!("line {} `{}`: {error:#}", line.number, line.text);
    }
    result.model
}

pub fn failure(source: &str) -> String {
    let lines = match parse_program(source) {
        Ok(lines) => lines,
        Err(error) => return format!("{error:#}"),
    };
    match run(&lines).steps.last() {
        Some((_, Err(error))) => format!("{error:#}"),
        _ => panic!("expected `{source}` to fail"),
    }
}

pub fn volume(model: &Model) -> f64 {
    model
        .solids()
        .iter()
        .map(|solid| geometry::volume_at(solid, geometry::bounds(solid).diameter() * 2.0e-5))
        .sum()
}

pub fn assert_volume(model: &Model, expected: f64, relative: f64) {
    let actual = volume(model);
    assert!(
        (actual - expected).abs() <= expected.abs() * relative,
        "volume {actual:.4}, expected {expected:.4} within {:.3}%",
        relative * 100.0
    );
}

pub fn bounds(model: &Model) -> ([f64; 3], [f64; 3]) {
    model
        .solids()
        .iter()
        .map(|solid| geometry::bounds(solid))
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

pub fn assert_bounds(model: &Model, min: [f64; 3], max: [f64; 3]) {
    let (got_min, got_max) = bounds(model);
    let close = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.0e-3);
    assert!(
        close(got_min, min) && close(got_max, max),
        "bounds {got_min:?}..{got_max:?}, expected {min:?}..{max:?}"
    );
}

pub fn part(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/examples/parts/{name}.lcad",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("example exists")
}

pub fn summary(source: &str) -> String {
    let lines = parse_program(source).expect("parses");
    let result = run(&lines);
    match result.steps.last() {
        Some((_, Ok(text))) => text.clone(),
        Some((line, Err(error))) => panic!("line {} `{}`: {error:#}", line.number, line.text),
        None => panic!("empty program"),
    }
}

pub fn scratch(name: &str) -> String {
    let dir = std::env::temp_dir().join("linecad-tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name).to_string_lossy().into_owned()
}
