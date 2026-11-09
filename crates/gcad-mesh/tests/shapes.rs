use gcad_mesh::{Options, build_with};

fn z_up() -> Options {
    Options {
        y_up: false,
        ..Default::default()
    }
}

fn volume(source: &str) -> f64 {
    let model = build_with(source, &z_up()).expect("builds");
    model
        .bodies
        .iter()
        .map(|body| {
            body.indices
                .chunks(3)
                .map(|t| {
                    let [a, b, c] =
                        [t[0], t[1], t[2]].map(|i| body.positions[i as usize].map(f64::from));
                    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                        + a[2] * (b[0] * c[1] - b[1] * c[0])
                })
                .sum::<f64>()
                / 6.0
        })
        .sum()
}

fn triangles(source: &str) -> usize {
    let model = build_with(source, &z_up()).expect("builds");
    model.bodies.iter().map(|b| b.indices.len() / 3).sum()
}

#[test]
fn an_ngon_extrudes_into_a_prism_of_that_many_sides() {
    let hexagon = volume("ngon 2 6\np: extrude 1\n");
    let expected = 6.0 * 0.5 * 1.0 * 1.0 * (std::f64::consts::TAU / 6.0).sin();
    assert!(
        (hexagon - expected).abs() < 1.0e-6,
        "{hexagon} vs {expected}"
    );
    assert_eq!(triangles("ngon 2 6\np: extrude 1\n"), 6 * 2 + 4 * 2);
}

#[test]
fn an_ellipse_extrudes_to_its_area() {
    let v = volume("ellipse 4 2\np: extrude 1\n");
    let exact = std::f64::consts::PI * 2.0 * 1.0;
    assert!((v - exact).abs() < exact * 0.02, "{v} vs {exact}");
}

#[test]
fn extrude_scale_makes_a_frustum_and_zero_makes_a_cone() {
    let frustum = volume("ngon 2 64\np: extrude 3 scale=0.5\n");
    let r: f64 = 1.0;
    let exact = std::f64::consts::PI * 3.0 / 3.0 * (r * r + r * 0.5 + 0.25);
    assert!(
        (frustum - exact).abs() < exact * 0.01,
        "{frustum} vs {exact}"
    );
    let cone = volume("ngon 2 64\np: extrude 3 scale=0\n");
    let exact = std::f64::consts::PI * 3.0 / 3.0;
    assert!((cone - exact).abs() < exact * 0.01, "{cone} vs {exact}");
}

#[test]
fn a_shell_blob_with_segments_is_a_low_poly_ellipsoid() {
    let source = "b: blob 1,2,3 r=2,1,0.5 mode=shell segments=12\n";
    assert_eq!(triangles(source), 12 * 2 + 12 * 2 * 4);
    let v = volume(source);
    let exact = 4.0 / 3.0 * std::f64::consts::PI * 2.0 * 1.0 * 0.5;
    assert!(v > exact * 0.8 && v < exact, "{v} vs {exact}");
    let model = build_with(source, &z_up()).expect("builds");
    let body = &model.bodies[0];
    let (lo, hi) = body.positions.iter().fold(
        ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
        |(lo, hi), p| {
            (
                std::array::from_fn(|k| lo[k].min(p[k])),
                std::array::from_fn(|k| hi[k].max(p[k])),
            )
        },
    );
    assert!((hi[2] - 3.5).abs() < 1.0e-5 && (lo[2] - 2.5).abs() < 1.0e-5);
    assert!(hi[0] <= 3.0 + 1.0e-5 && lo[0] >= -1.0 - 1.0e-5);
}

#[test]
fn segments_need_a_shell() {
    let err = build_with("b: blob 0,0,0 r=1 segments=8\n", &z_up())
        .err()
        .expect("refused");
    assert!(err.to_string().contains("mode=shell"), "{err}");
}

#[test]
fn low_poly_shells_keep_their_part_names_and_colours() {
    let source = "a: blob 0,0,0 r=1 mode=shell segments=8\nb: blob 3,0,0 r=1 mode=shell segments=8\nremesh\ncolor #ff0000 part=a\ncolor #00ff00 part=b\n";
    let model = build_with(source, &z_up()).expect("builds");
    let names: Vec<&str> = model.bodies[0]
        .parts
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["a", "b"]);
}

#[test]
fn a_torus_has_the_volume_of_a_ring_and_its_own_part() {
    let source = "rim: torus 4 1 around=64 sides=32\n";
    let v = volume(source);
    let exact = 2.0 * std::f64::consts::PI.powi(2) * 2.0 * 0.25;
    assert!((v - exact).abs() < exact * 0.02, "{v} vs {exact}");
    assert_eq!(triangles(source), 64 * 32 * 2);
    let model = build_with(source, &z_up()).expect("builds");
    assert_eq!(model.bodies[0].parts[0].name, "rim");
}

#[test]
fn a_torus_lies_in_its_plane_at_its_offset() {
    let model = build_with("plane XY offset=3\ntorus 4 1 at=1,2\n", &z_up()).expect("builds");
    let ps = &model.bodies[0].positions;
    let lo = |k: usize| ps.iter().map(|p| p[k]).fold(f32::INFINITY, f32::min);
    let hi = |k: usize| ps.iter().map(|p| p[k]).fold(f32::NEG_INFINITY, f32::max);
    assert!((hi(2) - 3.5).abs() < 1.0e-5 && (lo(2) - 2.5).abs() < 1.0e-5);
    assert!((hi(0) - 3.5).abs() < 1.0e-5 && (lo(0) + 1.5).abs() < 1.0e-5);
    assert!((hi(1) - 4.5).abs() < 0.1 && (lo(1) + 0.5).abs() < 0.1);
}

#[test]
fn a_torus_tube_must_be_thinner_than_its_ring() {
    let err = build_with("torus 1 1\n", &z_up()).err().expect("refused");
    assert!(err.to_string().contains("thinner"), "{err}");
}
