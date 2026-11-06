use gcad::geometry::Meshed;
use gcad_mesh::{Options, build_with};

type Tri = [[f64; 3]; 3];

fn source(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/props/{name}.gcad",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("prop")
}

fn vars_of(vars: &[(&str, f64)]) -> Vec<(String, f64)> {
    vars.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

pub fn mesh_triangles(text: &str, vars: &[(&str, f64)]) -> Vec<Tri> {
    let options = Options {
        y_up: false,
        vars: vars_of(vars),
        ..Default::default()
    };
    let model = build_with(text, &options).expect("builds");
    model
        .bodies
        .iter()
        .flat_map(|body| {
            body.indices
                .chunks(3)
                .map(|t| [t[0], t[1], t[2]].map(|i| body.positions[i as usize].map(f64::from)))
        })
        .collect()
}

pub fn kernel_triangles_of(path: &std::path::Path, vars: &[(&str, f64)]) -> Vec<Tri> {
    let result = gcad::model::run_path(path, &vars_of(vars), None).expect("reads");
    if let Some((line, Err(error))) = result.steps.last() {
        panic!("line {} `{}`: {error:#}", line.number, line.text);
    }
    result
        .model
        .solids()
        .iter()
        .flat_map(|solid| {
            Meshed::new(solid)
                .triangles()
                .iter()
                .map(|t| t.map(|p| [p.x, p.y, p.z]))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn kernel_triangles(name: &str, vars: &[(&str, f64)]) -> Vec<Tri> {
    kernel_triangles_of(
        std::path::Path::new(&format!(
            "{}/tests/fixtures/props/{name}.gcad",
            env!("CARGO_MANIFEST_DIR")
        )),
        vars,
    )
}

const SIZE: usize = 256;

fn project(p: [f64; 3], view: usize) -> (f64, f64) {
    match view {
        0 => (p[0], p[2]),
        1 => (p[1], p[2]),
        _ => (p[0], p[1]),
    }
}

fn raster(tris: &[Tri], view: usize, lo: (f64, f64), scale: f64, w: usize, h: usize) -> Vec<bool> {
    let mut grid = vec![false; w * h];
    for tri in tris {
        let q = tri.map(|p| {
            let (u, v) = project(p, view);
            ((u - lo.0) * scale, (v - lo.1) * scale)
        });
        let area = (q[1].0 - q[0].0) * (q[2].1 - q[0].1) - (q[1].1 - q[0].1) * (q[2].0 - q[0].0);
        if area.abs() < 1.0e-12 {
            continue;
        }
        let x0 = q
            .iter()
            .map(|p| p.0)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as usize;
        let x1 = (q
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil() as usize)
            .min(w);
        let y0 = q
            .iter()
            .map(|p| p.1)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as usize;
        let y1 = (q
            .iter()
            .map(|p| p.1)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil() as usize)
            .min(h);
        for y in y0..y1 {
            for x in x0..x1 {
                let c = (x as f64 + 0.5, y as f64 + 0.5);
                let edge = |a: (f64, f64), b: (f64, f64)| {
                    ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)) * area.signum()
                };
                if edge(q[0], q[1]) >= 0.0 && edge(q[1], q[2]) >= 0.0 && edge(q[2], q[0]) >= 0.0 {
                    grid[y * w + x] = true;
                }
            }
        }
    }
    grid
}

pub fn silhouette_difference(a: &[Tri], b: &[Tri], view: usize) -> f64 {
    let points = || b.iter().flatten().map(|p| project(*p, view));
    let lo = (
        points().map(|p| p.0).fold(f64::INFINITY, f64::min),
        points().map(|p| p.1).fold(f64::INFINITY, f64::min),
    );
    let hi = (
        points().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
        points().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
    );
    let scale = (SIZE as f64 - 1.0) / (hi.0 - lo.0).max(hi.1 - lo.1);
    let w = ((hi.0 - lo.0) * scale).ceil() as usize + 1;
    let h = ((hi.1 - lo.1) * scale).ceil() as usize + 1;
    let ga = raster(a, view, lo, scale, w, h);
    let gb = raster(b, view, lo, scale, w, h);
    let xor = ga.iter().zip(&gb).filter(|(x, y)| x != y).count();
    let union = ga.iter().zip(&gb).filter(|(x, y)| **x || **y).count();
    xor as f64 / union.max(1) as f64
}

fn check(name: &str, vars: &[(&str, f64)]) {
    let ours = mesh_triangles(&source(name), vars);
    let reference = kernel_triangles(name, vars);
    for (view, label) in ["front", "side", "top"].iter().enumerate() {
        let difference = silhouette_difference(&ours, &reference, view);
        println!("silhouette {name} {label}: {:.3}%", difference * 100.0);
        assert!(
            difference < 0.02,
            "{name} {label} silhouette differs {difference}"
        );
    }
}

#[test]
fn barrier_silhouette_matches_kernel() {
    check("barrier", &[]);
}

#[test]
fn hurdle_silhouette_matches_kernel() {
    check("hurdle", &[]);
}

#[test]
fn gate_silhouette_matches_kernel() {
    check("gate", &[]);
}

#[test]
fn train_silhouette_matches_kernel() {
    check("train", &[]);
}

#[test]
fn coin_silhouette_matches_kernel() {
    check("coin", &[]);
}

#[test]
fn arch_silhouette_matches_kernel() {
    check("arch", &[]);
}

#[test]
fn road_silhouette_matches_kernel() {
    check("road", &[]);
}

#[test]
fn building_silhouettes_match_kernel() {
    for (style, floors) in [(1.0, 2.0), (2.0, 3.0), (3.0, 4.0), (4.0, 5.0), (5.0, 3.0)] {
        check("building", &[("style", style), ("floors", floors)]);
    }
}

fn normal_areas(tris: &[Tri]) -> [f64; 7] {
    let mut bins = [0.0; 7];
    for t in tris {
        let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
        let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length < 1.0e-12 {
            continue;
        }
        let bin = (0..3)
            .find_map(|k| {
                let c = n[k] / length;
                (c.abs() > 0.999).then_some(k * 2 + usize::from(c < 0.0))
            })
            .unwrap_or(6);
        bins[bin] += length / 2.0;
    }
    let total: f64 = bins.iter().sum();
    bins.map(|b| b / total)
}

fn check_normals(name: &str) {
    let ours = normal_areas(&mesh_triangles(&source(name), &[]));
    let reference = normal_areas(&kernel_triangles(name, &[]));
    println!("normals {name} ours {ours:.3?} kernel {reference:.3?}");
    for (k, (a, b)) in ours.iter().zip(&reference).enumerate() {
        assert!((a - b).abs() < 0.03, "{name} normal bin {k}: {a} vs {b}");
    }
}

#[test]
fn coin_face_normals_match_kernel() {
    check_normals("coin");
}

#[test]
fn barrier_face_normals_match_kernel() {
    check_normals("barrier");
}

fn rim_deviation(tris: &[Tri], r: f64) -> f64 {
    let centre = (10.0 - r, 10.0 - r);
    let mut worst: f64 = 0.0;
    for t in tris {
        let mut samples: Vec<[f64; 3]> = Vec::new();
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            samples.push([
                (a[0] + b[0]) / 2.0,
                (a[1] + b[1]) / 2.0,
                (a[2] + b[2]) / 2.0,
            ]);
            samples.push(a);
        }
        for p in samples {
            if p[1].abs() < 5.0 && p[0] > centre.0 + 1.0e-6 && p[2] > centre.1 + 1.0e-6 {
                let d = ((p[0] - centre.0).powi(2) + (p[2] - centre.1).powi(2)).sqrt();
                worst = worst.max((d - r).abs());
            }
        }
    }
    worst
}

fn rim_case(r: f64) -> (f64, f64) {
    let text = format!("rect 20 20 at=0,0\nb: extrude 10\nfillet {r} b.end&b.side\n");
    let ours = mesh_triangles(&text, &[]);
    let dir = std::env::temp_dir().join(format!("rim_{r}_{}.gcad", std::process::id()));
    std::fs::write(&dir, &text).expect("write");
    let reference = kernel_triangles_of(&dir, &[]);
    let _ = std::fs::remove_file(&dir);
    (rim_deviation(&ours, r), rim_deviation(&reference, r))
}

#[test]
fn fillet_rim_stays_close_to_the_kernel_arc() {
    for r in [1.0, 3.0] {
        let (ours, kernel) = rim_case(r);
        println!("fillet r={r}: bevel rings {ours:.4} mm, kernel tessellation {kernel:.4} mm");
        assert!(ours < 0.02 * r + 1.0e-6, "r={r}: {ours}");
    }
}

fn glb_triangles(glb: &[u8]) -> usize {
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
    json["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["primitives"].as_array().unwrap())
        .map(|p| {
            json["accessors"][p["indices"].as_u64().unwrap() as usize]["count"]
                .as_u64()
                .unwrap() as usize
                / 3
        })
        .sum()
}

fn triangle_counts(name: &str, vars: &[(&str, f64)]) -> (usize, usize) {
    let text = source(name);
    let options = Options {
        vars: vars_of(vars),
        ..Default::default()
    };
    let ours = glb_triangles(
        &gcad_mesh::build_with(&text, &options)
            .unwrap()
            .to_glb()
            .unwrap(),
    );
    let path = format!(
        "{}/tests/fixtures/props/{name}.gcad",
        env!("CARGO_MANIFEST_DIR")
    );
    let result = gcad::model::run_path(std::path::Path::new(&path), &vars_of(vars), None).unwrap();
    let kernel = glb_triangles(&gcad::export::glb_of(&result.model).unwrap());
    (ours, kernel)
}

#[test]
fn props_stay_within_five_times_the_kernel_glb_triangles() {
    let cases: [(&str, Vec<(&str, f64)>); 8] = [
        ("barrier", vec![]),
        ("hurdle", vec![]),
        ("gate", vec![]),
        ("train", vec![]),
        ("coin", vec![]),
        ("arch", vec![]),
        ("road", vec![]),
        ("building", vec![("style", 4.0), ("floors", 5.0)]),
    ];
    for (name, vars) in cases {
        let (ours, kernel) = triangle_counts(name, &vars);
        println!("triangles {name}: gcad-mesh {ours}, kernel glb {kernel}");
        assert!(ours <= kernel * 5, "{name}: {ours} vs {kernel}");
    }
}

#[test]
fn building_all_props_is_fast() {
    let names = [
        "barrier", "hurdle", "gate", "train", "coin", "arch", "road", "building",
    ];
    let mut total = std::time::Duration::ZERO;
    for name in names {
        let text = source(name);
        let options = Options {
            vars: vars_of(&[("style", 4.0), ("floors", 5.0)]),
            ..Default::default()
        };
        let mut times: Vec<std::time::Duration> = (0..20)
            .map(|_| {
                let start = std::time::Instant::now();
                let model = gcad_mesh::build_with(&text, &options).unwrap();
                std::hint::black_box(model);
                start.elapsed()
            })
            .collect();
        times.sort();
        println!("build {name}: median {:?}", times[10]);
        total += times[10];
    }
    println!("build total median {total:?}");
    assert!(total < std::time::Duration::from_secs(2));
}
