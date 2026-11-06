mod common;

use gcad_mesh::{Options, build_with};

fn prop(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/props/{name}.gcad",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("prop")
}

fn mesh_options(vars: &[(&str, f64)]) -> Options {
    Options {
        y_up: false,
        vars: vars.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        ..Default::default()
    }
}

fn compare(name: &str, vars: &[(&str, f64)]) {
    let source = prop(name);
    let model = build_with(&source, &mesh_options(vars)).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut volume = 0.0;
    let mut bodies = 0;
    for body in &model.bodies {
        bodies += 1;
        for p in &body.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k] as f64);
                hi[k] = hi[k].max(p[k] as f64);
            }
        }
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
    assert!(bodies > 0);
    let reference = kernel_model(name, vars);
    let (rlo, rhi) = common::bounds(&reference);
    for k in 0..3 {
        assert!(
            (lo[k] - rlo[k]).abs() < 0.05,
            "{name} min[{k}] {} vs {}",
            lo[k],
            rlo[k]
        );
        assert!(
            (hi[k] - rhi[k]).abs() < 0.05,
            "{name} max[{k}] {} vs {}",
            hi[k],
            rhi[k]
        );
    }
    let expected = common::volume(&reference);
    assert!(
        (volume - expected).abs() <= expected * 0.03,
        "{name} volume {volume} vs {expected}"
    );
}

fn kernel_model(name: &str, vars: &[(&str, f64)]) -> gcad::model::Model {
    let path = std::path::PathBuf::from(format!(
        "{}/tests/fixtures/props/{name}.gcad",
        env!("CARGO_MANIFEST_DIR")
    ));
    let vars: Vec<(String, f64)> = vars.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let result = gcad::model::run_path(&path, &vars, None).expect("reads");
    if let Some((line, Err(error))) = result.steps.last() {
        panic!("line {} `{}`: {error:#}", line.number, line.text);
    }
    result.model
}

#[test]
fn barrier_matches_kernel() {
    compare("barrier", &[]);
}
#[test]
fn hurdle_matches_kernel() {
    compare("hurdle", &[]);
}
#[test]
fn gate_matches_kernel() {
    compare("gate", &[]);
}
#[test]
fn train_matches_kernel() {
    compare("train", &[]);
}
#[test]
fn coin_matches_kernel() {
    compare("coin", &[]);
}
#[test]
fn arch_matches_kernel() {
    compare("arch", &[]);
}
#[test]
fn road_matches_kernel() {
    compare("road", &[]);
}
#[test]
fn buildings_match_kernel() {
    for (style, floors) in [(1.0, 2.0), (2.0, 3.0), (3.0, 4.0), (4.0, 5.0), (5.0, 3.0)] {
        compare("building", &[("style", style), ("floors", floors)]);
    }
}

#[test]
fn repro_builds_a_closed_flat_shaded_part() {
    let source = "foot: rect 26 26 r=4\nfoot: extrude 8\ncolor #5b6478\n";
    let model = gcad_mesh::build(source).expect("builds");
    assert_eq!(model.bodies.len(), 1);
    assert!(gcad_mesh::to_glb(source).expect("glb").len() > 100);
}

#[test]
fn profile_inside_another_is_a_hole() {
    let source = "rect 40 40\ncircle 10\nring: extrude 5\n";
    let model = build_with(source, &mesh_options(&[])).expect("builds");
    let body = &model.bodies[0];
    let volume: f64 = body
        .indices
        .chunks(3)
        .map(|t| {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| body.positions[i as usize].map(f64::from));
            a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0])
        })
        .sum::<f64>()
        / 6.0;
    let expected = (1600.0 - 24.0 * 25.0 * (std::f64::consts::TAU / 24.0).sin() / 2.0 * 1.0) * 5.0;
    assert!(
        (volume - expected).abs() < expected * 0.01,
        "{volume} vs {expected}"
    );
}

#[test]
fn negative_extrude_goes_below_the_plane() {
    let model = build_with("rect 10 10\ne: extrude -4\n", &mesh_options(&[])).expect("builds");
    let zs: Vec<f32> = model.bodies[0].positions.iter().map(|p| p[2]).collect();
    assert!(zs.iter().all(|z| *z <= 1.0e-6 && *z >= -4.0 - 1.0e-6));
    assert!(zs.iter().any(|z| (*z + 4.0).abs() < 1.0e-6));
}
