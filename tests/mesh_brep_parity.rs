use gcad::model::parity::{self, Measured};
use std::f64::consts::{PI, TAU};

struct Built {
    kernel: gcad_mesh::Model,
    mesh: gcad_mesh::Model,
}

fn kernel_of(source: &str) -> gcad_mesh::Model {
    let lines = gcad::parse::parse_program(source).expect("parses");
    let run = gcad::model::run(&lines);
    if let Some((line, Err(error))) = run.steps.last() {
        panic!("kernel line {} `{}`: {error:#}", line.number, line.text);
    }
    gcad::model::glb::mesh_model(&run.model, false)
}

fn mesh_of(source: &str) -> gcad_mesh::Model {
    gcad_mesh::build_with(
        source,
        &gcad_mesh::Options {
            y_up: false,
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("gcad-mesh: {e}"))
}

fn built(source: &str) -> Built {
    Built {
        kernel: kernel_of(source),
        mesh: mesh_of(source),
    }
}

impl Built {
    fn measured(&self) -> (Vec<Measured>, Vec<Measured>) {
        (parity::measured(&self.kernel), parity::measured(&self.mesh))
    }

    fn one(&self) -> (Measured, Measured) {
        let (k, m) = self.measured();
        assert_eq!((k.len(), m.len()), (1, 1), "one body each");
        (k[0].clone(), m[0].clone())
    }
}

fn points(model: &gcad_mesh::Model) -> Vec<[f64; 3]> {
    let mut out: Vec<[f64; 3]> = Vec::new();
    for p in model.bodies.iter().flat_map(|b| &b.positions) {
        let p = p.map(f64::from);
        if !out
            .iter()
            .any(|q| (0..3).all(|k| (p[k] - q[k]).abs() < 1.0e-6))
        {
            out.push(p);
        }
    }
    out
}

fn near_any(p: [f64; 3], set: &[[f64; 3]], within: f64) -> bool {
    set.iter()
        .any(|q| (0..3).all(|k| (p[k] - q[k]).abs() <= within))
}

fn assert_bbox(k: &Measured, m: &Measured, within: f64, what: &str) {
    for i in 0..3 {
        assert!(
            (k.lo[i] - m.lo[i]).abs() <= within && (k.hi[i] - m.hi[i]).abs() <= within,
            "{what}: axis {i} kernel {:?}..{:?} mesh {:?}..{:?}",
            k.lo,
            k.hi,
            m.lo,
            m.hi
        );
    }
}

const KERNEL_RIM: usize = 32;
const KERNEL_CURVED: f64 = 0.01;
const KERNEL_TORUS: f64 = 0.02;
const BETWEEN_24: f64 = 0.015;

fn close(a: f64, b: f64, relative: f64) -> bool {
    (a - b).abs() <= b.abs() * relative
}

fn polygon_area(r: f64, n: usize) -> f64 {
    n as f64 / 2.0 * r * r * (TAU / n as f64).sin()
}

fn inscribed(n: usize) -> f64 {
    n as f64 / TAU * (TAU / n as f64).sin()
}

#[test]
fn ngon_corners_match_the_kernel() {
    for n in [3, 5, 6, 8, 64] {
        for angle in [0.0, 30.0] {
            for (ax, ay) in [(0.0, 0.0), (3.0, -2.0)] {
                for plane in ["XY", "XZ", "YZ"] {
                    for offset in [0.0, 2.5] {
                        let source = format!(
                            "plane {plane} offset={offset}\nngon 4 {n} at={ax},{ay} angle={angle}\np: extrude 1.5\n"
                        );
                        let both = built(&source);
                        let (k, m) = both.one();
                        assert_bbox(&k, &m, 1.0e-6, &source);
                        let exact = polygon_area(2.0, n) * 1.5;
                        assert!(
                            close(m.volume, exact, 1.0e-6),
                            "{source}: mesh {}",
                            m.volume
                        );
                        assert!(
                            close(k.volume, exact, 1.0e-6),
                            "{source}: kernel {}",
                            k.volume
                        );
                        let (kp, mp) = (points(&both.kernel), points(&both.mesh));
                        assert_eq!(kp.len(), 2 * n, "{source}: kernel corners");
                        assert_eq!(mp.len(), 2 * n, "{source}: mesh corners");
                        assert!(
                            mp.iter().all(|p| near_any(*p, &kp, 1.0e-6)),
                            "{source}: corners differ"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn ngon_first_corner_sits_at_its_angle_on_both_sides() {
    let source = "ngon 4 5 at=1,1 angle=30\np: extrude 1\n";
    let corner = [
        1.0 + 2.0 * 30f64.to_radians().cos(),
        1.0 + 2.0 * 30f64.to_radians().sin(),
        0.0,
    ];
    let both = built(source);
    assert!(near_any(corner, &points(&both.kernel), 1.0e-6));
    assert!(near_any(corner, &points(&both.mesh), 1.0e-6));
    let first = both.mesh.bodies[0].positions[0].map(f64::from);
    assert!(
        (0..3).all(|k| (first[k] - corner[k]).abs() < 1.0e-6),
        "{first:?}"
    );
}

#[test]
fn both_builders_wind_solids_outward() {
    for source in [
        "ngon 4 6\np: extrude 2\n",
        "ngon 4 6\np: extrude -2\n",
        "plane XZ offset=1\nngon 4 6\np: extrude -2\n",
        "plane YZ\nellipse 4 2\np: extrude 2 scale=0.5\n",
    ] {
        let (k, m) = built(source).one();
        assert!(
            k.volume > 0.0 && m.volume > 0.0,
            "{source}: {} {}",
            k.volume,
            m.volume
        );
    }
}

#[test]
fn negative_extrudes_match_the_kernel() {
    for source in [
        "ngon 4 6 at=1,2\np: extrude -3\n",
        "plane XZ offset=2\nngon 4 8\np: extrude -3\n",
        "ngon 4 6\np: extrude -3 scale=0.5\n",
    ] {
        let both = built(source);
        let (k, m) = both.one();
        assert_bbox(&k, &m, 1.0e-6, source);
        assert!(
            close(m.volume, k.volume, 1.0e-6),
            "{source}: {} vs {}",
            m.volume,
            k.volume
        );
        assert!(
            points(&both.mesh)
                .iter()
                .all(|p| near_any(*p, &points(&both.kernel), 1.0e-6))
        );
    }
}

#[test]
fn ellipses_match_the_kernel_within_their_segments() {
    for (dx, dy) in [(4.0, 2.0), (2.0, 4.0)] {
        for (ax, ay) in [(0.0, 0.0), (1.5, -1.0)] {
            let source = format!("ellipse {dx} {dy} at={ax},{ay}\np: extrude 2\n");
            let (k, m) = built(&source).one();
            assert_bbox(&k, &m, 1.0e-5, &source);
            let exact = PI * dx / 2.0 * dy / 2.0 * 2.0;
            assert!(
                close(k.volume, exact, KERNEL_CURVED),
                "{source}: kernel {}",
                k.volume
            );
            assert!(
                close(m.volume, exact * inscribed(24), 1.0e-6),
                "{source}: mesh {}",
                m.volume
            );
            assert!(
                close(m.volume, k.volume, BETWEEN_24),
                "{source}: {} vs {}",
                m.volume,
                k.volume
            );
        }
    }
}

#[test]
fn circles_match_the_kernel_within_their_segments() {
    for (d, n) in [(10.0, 24), (50.0, 32)] {
        for (ax, ay) in [(0.0, 0.0), (7.0, 3.0)] {
            for plane in ["XY", "XZ", "YZ"] {
                let source =
                    format!("plane {plane} offset=1\ncircle {d} at={ax},{ay}\np: extrude 4\n");
                let (k, m) = built(&source).one();
                assert_bbox(&k, &m, 1.0e-5 * d, &source);
                let exact = PI * d * d / 4.0 * 4.0;
                assert!(
                    close(k.volume, exact, KERNEL_CURVED),
                    "{source}: kernel {}",
                    k.volume
                );
                assert!(
                    close(m.volume, exact * inscribed(n), 1.0e-6),
                    "{source}: mesh {}",
                    m.volume
                );
                assert!(
                    close(m.volume, k.volume, BETWEEN_24),
                    "{source}: {} vs {}",
                    m.volume,
                    k.volume
                );
            }
        }
    }
}

#[test]
fn curved_profiles_use_fixed_segments_not_the_kernel_tolerance() {
    for (source, segments) in [
        ("circle 10\np: extrude 1\n", 24),
        ("circle 50\np: extrude 1\n", 32),
        ("ellipse 4 2\np: extrude 1\n", 24),
        ("ellipse 50 20\np: extrude 1\n", 32),
    ] {
        let both = built(source);
        let rim =
            |model: &gcad_mesh::Model| points(model).iter().filter(|p| p[2].abs() < 1.0e-9).count();
        assert_eq!(rim(&both.mesh), segments, "{source}");
        assert_eq!(rim(&both.kernel), KERNEL_RIM, "{source}: kernel rim");
    }
}

#[test]
fn scaled_extrudes_match_the_kernel() {
    for source in [
        "ngon 4 6\np: extrude 3 scale=0.5\n",
        "ngon 4 6 at=3,0\np: extrude 3 scale=0.5\n",
        "plane XZ offset=1\nngon 4 5 angle=30\np: extrude 3 scale=0.5\n",
    ] {
        let both = built(source);
        let (k, m) = both.one();
        assert_bbox(&k, &m, 1.0e-6, source);
        assert!(
            close(m.volume, k.volume, 1.0e-6),
            "{source}: {} vs {}",
            m.volume,
            k.volume
        );
        assert!(
            points(&both.mesh)
                .iter()
                .all(|p| near_any(*p, &points(&both.kernel), 1.0e-6))
        );
    }
    let (k, m) = built("circle 4\np: extrude 3 scale=0.5\n").one();
    let exact = PI * 3.0 / 3.0 * (4.0 + 2.0 + 1.0);
    assert!(
        close(k.volume, exact, KERNEL_CURVED),
        "kernel frustum {}",
        k.volume
    );
    assert!(
        close(m.volume, exact * inscribed(24), 1.0e-6),
        "mesh frustum {}",
        m.volume
    );
    assert_bbox(&k, &m, 1.0e-5, "circle frustum");
}

#[test]
fn scale_zero_makes_the_same_cone_in_both() {
    for source in [
        "ngon 4 6\np: extrude 3 scale=0\n",
        "ngon 4 64 angle=30\np: extrude -3 scale=0\n",
        "plane XZ offset=2\nngon 4 5 at=1,1\np: extrude 3 scale=0\n",
    ] {
        let both = built(source);
        let (k, m) = both.one();
        assert_bbox(&k, &m, 1.0e-6, source);
        assert!(
            close(m.volume, k.volume, 1.0e-6),
            "{source}: {} vs {}",
            m.volume,
            k.volume
        );
        let kp = points(&both.kernel);
        assert!(
            points(&both.mesh).iter().all(|p| near_any(*p, &kp, 1.0e-6)),
            "{source}"
        );
    }
    let (k, m) = built("circle 4\np: extrude 3 scale=0\n").one();
    let exact = PI * 4.0 * 3.0 / 3.0;
    assert!(
        close(k.volume, exact, KERNEL_CURVED),
        "kernel cone {}",
        k.volume
    );
    assert!(
        close(m.volume, exact * inscribed(24), 1.0e-6),
        "mesh cone {}",
        m.volume
    );
    assert_bbox(&k, &m, 1.0e-5, "circle cone");
}

#[test]
fn a_cone_needs_a_profile_without_holes() {
    let source = "circle 10\ncircle 4\np: extrude 3 scale=0\n";
    let err = gcad_mesh::build(source)
        .err()
        .expect("mesh refuses")
        .message;
    assert!(err.contains("without holes"), "{err}");
    let lines = gcad::parse::parse_program(source).expect("parses");
    let run = gcad::model::run(&lines);
    match run.steps.last() {
        Some((_, Err(error))) => {
            assert!(format!("{error:#}").contains("without holes"), "{error:#}")
        }
        _ => panic!("kernel builds a cone with a hole"),
    }
}

#[test]
fn tori_match_the_kernel_within_their_segments() {
    for (source, around, sides) in [
        ("rim: torus 8 2\n", 24, 8),
        (
            "plane XZ offset=1\nrim: torus 8 2 at=2,1 around=64 sides=32\n",
            64,
            32,
        ),
    ] {
        let (k, m) = built(source).one();
        assert_bbox(&k, &m, 1.0e-5, source);
        let exact = 2.0 * PI * PI * 4.0 * 1.0;
        assert!(
            close(k.volume, exact, KERNEL_TORUS),
            "{source}: kernel {}",
            k.volume
        );
        let faceted = inscribed(around) * inscribed(sides);
        assert!(
            close(m.volume, exact * faceted, 1.0e-3),
            "{source}: mesh {} faceted exact {}",
            m.volume,
            exact * faceted
        );
    }
}

#[test]
fn shell_blobs_come_out_the_same_through_both() {
    let source = "b: blob 1,2,3 r=2,1,0.5 mode=shell segments=12\n";
    let both = built(source);
    assert_eq!(
        both.kernel.bodies[0].positions,
        both.mesh.bodies[0].positions
    );
    assert_eq!(both.kernel.bodies[0].indices, both.mesh.bodies[0].indices);
    let (k, m) = both.one();
    let exact = 4.0 / 3.0 * PI * 2.0 * 1.0 * 0.5;
    assert!(m.volume > exact * 0.8 && m.volume < exact, "{}", m.volume);
    assert_eq!(k.volume, m.volume);
}

#[test]
fn holes_match_the_kernel_and_mesh_extrudes_do_not_cut() {
    let both = built("ngon 20 6\nngon 8 6 angle=30\np: extrude 2\n");
    let (k, m) = both.one();
    assert_bbox(&k, &m, 1.0e-6, "hexagon ring");
    let exact = (polygon_area(10.0, 6) - polygon_area(4.0, 6)) * 2.0;
    assert!(close(k.volume, exact, 1.0e-6) && close(m.volume, exact, 1.0e-6));
    let cut = "rect 10 10\np: extrude 3\nplane XY offset=3\nngon 4 6\nh: extrude -2 mode=cut\n";
    let err = gcad_mesh::build(cut)
        .err()
        .expect("gcad-mesh has no booleans")
        .message;
    assert!(err.contains("mode"), "{err}");
    let kernel = parity::measured(&kernel_of(cut));
    assert!(close(
        kernel[0].volume,
        300.0 - polygon_area(2.0, 6) * 2.0,
        1.0e-6
    ));
}

#[test]
fn body_names_and_colours_match_the_kernel() {
    let source = "ngon 4 6\np: extrude 2\nbody b\ncircle 4 at=6,0\nq: extrude 1\ncolor #00ff00\nbody c\nngon 2 3\nr: extrude 1\ncolor #ff8000\n";
    let (k, m) = built(source).measured();
    let differences = parity::compare(&k, &m).expect("same bodies");
    assert_eq!(
        differences
            .iter()
            .map(|d| d.body.as_str())
            .collect::<Vec<_>>(),
        ["main", "b", "c"]
    );
    for d in &differences {
        assert!(d.colour < 1.0e-3, "{} colour off by {}", d.body, d.colour);
    }
    assert_eq!(k[0].parts[0].0, "main");
    assert_eq!(m[0].parts[0].0, "p");
}

#[test]
fn compare_names_bodies_that_only_one_side_builds() {
    let k = parity::measured(&kernel_of(
        "ngon 4 6\np: extrude 2\nbody b\nngon 2 3\nq: extrude 1\n",
    ));
    let m = parity::measured(&mesh_of("ngon 4 6\np: extrude 2\n"));
    let err = parity::compare(&k, &m).err().expect("differs");
    assert!(err.contains("\"b\""), "{err}");
}

fn area_of(model: &gcad_mesh::Model) -> f64 {
    model
        .bodies
        .iter()
        .flat_map(|body| {
            body.indices.chunks(3).map(|t| {
                let [a, b, c] =
                    [t[0], t[1], t[2]].map(|i| body.positions[i as usize].map(f64::from));
                let (u, v) = (
                    [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                    [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
                );
                let n = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() / 2.0
            })
        })
        .sum()
}

fn open_edges(model: &gcad_mesh::Model) -> usize {
    let mut edges: std::collections::HashMap<([i64; 3], [i64; 3]), usize> = Default::default();
    let key = |p: [f32; 3]| p.map(|c| (c as f64 * 1.0e5).round() as i64);
    for body in &model.bodies {
        for t in body.indices.chunks(3) {
            for k in 0..3 {
                let (a, b) = (
                    key(body.positions[t[k] as usize]),
                    key(body.positions[t[(k + 1) % 3] as usize]),
                );
                if a != b {
                    *edges.entry((a.min(b), a.max(b))).or_default() += 1;
                }
            }
        }
    }
    edges.values().filter(|&&n| n == 1).count()
}

#[test]
fn open_extrudes_are_the_same_wall_in_both() {
    for (n, plane, depth) in [
        (8, "XY", 3.0_f64),
        (6, "XZ", 3.0),
        (5, "YZ", -3.0),
        (28, "XY", 3.66),
    ] {
        let open =
            format!("plane {plane} offset=1\nngon 4 {n} at=1,0\nt: extrude {depth} open=1\n");
        let both = built(&open);
        let wall = n as f64 * 4.0 * (PI / n as f64).sin() * depth.abs();
        for (side, model) in [("mesh", &both.mesh), ("kernel", &both.kernel)] {
            assert!(
                close(area_of(model), wall, 1.0e-6),
                "{open}: {side} area {}",
                area_of(model)
            );
            assert_eq!(open_edges(model), 2 * n, "{open}: {side} rims");
        }
        let (k, m) = both.one();
        assert_bbox(&k, &m, 1.0e-6, &open);
        let (kp, mp) = (points(&both.kernel), points(&both.mesh));
        assert_eq!(kp.len(), 2 * n, "{open}: kernel corners");
        assert!(mp.iter().all(|p| near_any(*p, &kp, 1.0e-6)), "{open}");
    }
}

#[test]
fn open_curved_extrudes_match_the_kernel_area_within_their_segments() {
    for source in [
        "circle 4\nt: extrude 3 open=1\n",
        "ellipse 4 2\nt: extrude 3 open=1 scale=0.5\n",
    ] {
        let both = built(source);
        let (k, m) = both.one();
        assert_bbox(&k, &m, 1.0e-5, source);
        let (ka, ma) = (area_of(&both.kernel), area_of(&both.mesh));
        assert!(close(ma, ka, 0.01), "{source}: kernel {ka} mesh {ma}");
        assert_eq!(open_edges(&both.mesh), 2 * 24, "{source}");
    }
    let exact = TAU * 2.0 * 3.0;
    assert!(close(
        area_of(&kernel_of("circle 4\nt: extrude 3 open=1\n")),
        exact,
        0.01
    ));
}

fn kernel_error(source: &str) -> String {
    let lines = gcad::parse::parse_program(source).expect("parses");
    match gcad::model::run(&lines).steps.last() {
        Some((_, Err(error))) => format!("{error:#}"),
        _ => panic!("the kernel builds `{source}`"),
    }
}

#[test]
fn both_refuse_the_same_open_extrudes() {
    for (source, says) in [
        (
            "circle 10\ncircle 4\nt: extrude 3 open=1\n",
            "without holes",
        ),
        (
            "rect 10 10\np: extrude 3\nngon 4 6\nt: extrude 2 open=1\n",
            "its own body",
        ),
        (
            "ngon 4 6\nt: extrude 2 open=1\nrect 10 10\np: extrude 3\n",
            "its own body",
        ),
        (
            "ngon 4 6\nt: extrude 2 open=1\nfillet 0.5 t.end&t.side\n",
            "open",
        ),
        ("ngon 4 6\nt: extrude 2 open=2\n", "open=1"),
    ] {
        let mesh = gcad_mesh::build(source)
            .err()
            .expect("gcad-mesh refuses")
            .message;
        assert!(mesh.contains(says), "{source}: gcad-mesh says {mesh}");
        let kernel = kernel_error(source);
        assert!(kernel.contains(says), "{source}: kernel says {kernel}");
    }
    let cut = "rect 10 10\np: extrude 3\nngon 4 6\nt: extrude 2 open=1 mode=cut\n";
    assert!(kernel_error(cut).contains("open"), "{}", kernel_error(cut));
    assert!(gcad_mesh::build(cut).is_err());
}

const BALLSORT_TUBE: &str = "body glass\nplane XY\nngon 1.03 28\nglass: extrude 3.66 open=1\ncolor #d1f2ff\nbody foot\nplane XY\nngon 1.2 28\nfoot: extrude 0.14\ncolor #8c73d9\nbody rim\nplane XY offset=3.66\nrim: torus 1.08 0.09 around=40 sides=10\ncolor #ebf7ff\nbody glint\nplane XY offset=0.457\nrect 0.07 0.02 at=-0.3,-0.43\nglint: extrude 2.855\ncolor #ffffff\n";

#[test]
fn the_ballsort_tube_builds_the_same_through_both() {
    let both = built(BALLSORT_TUBE);
    let (k, m) = both.measured();
    let differences = parity::compare(&k, &m).expect("same bodies");
    for d in &differences {
        let bound = if d.body == "rim" {
            1.0 - inscribed(40) * inscribed(10) + KERNEL_TORUS
        } else {
            0.03
        };
        assert!(d.within(bound), "{d:?}");
    }
    let glass = |model: &gcad_mesh::Model| {
        let body = model
            .bodies
            .iter()
            .find(|b| b.name == "glass")
            .expect("glass");
        gcad_mesh::Model {
            bodies: vec![body.clone()],
        }
    };
    assert!(close(
        area_of(&glass(&both.kernel)),
        area_of(&glass(&both.mesh)),
        1.0e-6
    ));
    let exported = gcad2glb::Assets::from_source(BALLSORT_TUBE, &gcad2glb::Options::default())
        .expect("gcad2glb builds it");
    assert_eq!(exported.bodies.len(), 4);
}

fn check_mesh(source: &str, name: &str, extra: &[&str]) -> std::process::Output {
    let dir = std::env::temp_dir().join("gcad-tests");
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join(name);
    std::fs::write(&path, source).expect("writes");
    std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
        .arg("check-mesh")
        .arg(&path)
        .args(extra)
        .output()
        .expect("runs gcad")
}

#[test]
fn check_mesh_reports_each_body_and_passes_within_the_bound() {
    let out = check_mesh(
        "ngon 4 6\np: extrude 2 scale=0\nbody b\ncircle 10\nq: extrude 1\ncolor #ff0000\n",
        "check-mesh-ok.gcad",
        &[],
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.lines().any(|l| l.starts_with("main ")), "{text}");
    assert!(text.lines().any(|l| l.starts_with("b ")), "{text}");
}

#[test]
fn check_mesh_fails_above_the_bound() {
    let out = check_mesh(
        "circle 10\np: extrude 1\n",
        "check-mesh-tight.gcad",
        &["--bound", "0.001"],
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("main"), "{err}");
}

#[test]
fn check_mesh_compares_the_area_of_an_open_body() {
    let out = check_mesh(BALLSORT_TUBE, "check-mesh-tube.gcad", &["--bound", "0.1"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let glass = text
        .lines()
        .find(|l| l.starts_with("glass "))
        .expect("glass row");
    assert!(glass.contains("open, area"), "{glass}");
}

#[test]
fn check_mesh_skips_files_gcad_mesh_does_not_build() {
    let out = check_mesh(
        "rect 10 10\np: extrude 3\nchamfer 1 p.end\n",
        "check-mesh-kernel.gcad",
        &[],
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("gcad-mesh does not build it"), "{text}");
}
