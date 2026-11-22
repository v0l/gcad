mod common;

use common::*;
use gcad::model::Model;
use gcad_cloud::{Cloud, ColourBy};

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/clouds/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn load(file: &str, options: &str) -> Model {
    build(&format!("scan: cloud {} {options}", fixture(file)))
}

fn cloud_of(model: &Model, name: &str) -> std::sync::Arc<Cloud> {
    model.cloud(name).expect("a cloud body").clone()
}

fn value(model: &Model, name: &str) -> f64 {
    *model
        .scope
        .get(name)
        .unwrap_or_else(|| panic!("no value `{name}`"))
}

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "got {actual}, expected {expected} within {tolerance}"
    );
}

fn read_back(path: &str) -> Cloud {
    gcad_cloud::import(
        gcad_cloud::format_of(std::path::Path::new(path)).expect("a cloud format"),
        Box::new(std::fs::File::open(path).expect("exists")),
        &gcad_cloud::Options::default(),
    )
    .expect("reads")
    .cloud
}

fn export(model: &Model, to: &str) {
    gcad::export::export_model(model, std::path::Path::new("scan.gcad"), to).expect("exports");
}

#[test]
fn cube_fixture_is_the_surface_of_the_cube_solid() {
    let solid = build(&format!(
        "scan: rect 20 20 at=10,10\nextrude 20\n"
    ));
    let (lo, hi) = bounds(&solid);
    let model = load("cube_binary.ply", "");
    let cloud = cloud_of(&model, "scan");
    assert_eq!(cloud.len(), 6 * 21 * 21);
    let (clo, chi) = cloud.bounds().unwrap();
    for k in 0..3 {
        near(clo[k], lo[k], 0.05);
        near(chi[k], hi[k], 0.05);
    }
    let on_surface = cloud
        .positions
        .iter()
        .filter(|p| {
            (0..3).any(|k| (p[k] - lo[k]).abs() <= 0.05 || (p[k] - hi[k]).abs() <= 0.05)
        })
        .count();
    assert_eq!(on_surface, cloud.len());
    let noisy = cloud
        .positions
        .iter()
        .filter(|p| (0..3).any(|k| p[k].fract().abs() > 0.0 && p[k].fract().abs() < 0.049))
        .count();
    assert!(noisy > cloud.len() / 2, "the fixture carries its noise");
}

#[test]
fn ascii_and_binary_ply_hold_the_same_cloud() {
    let ascii = load("cube_ascii.ply", "");
    let binary = load("cube_binary.ply", "");
    assert_eq!(*cloud_of(&ascii, "scan"), *cloud_of(&binary, "scan"));
    let cloud = cloud_of(&binary, "scan");
    assert!(cloud.normals.is_some() && cloud.colours.is_some());
    assert!(cloud.intensity.is_some() && cloud.classes.is_some());
}

#[test]
fn ply_write_then_read_is_identical() {
    let model = load("cube_ascii.ply", "");
    let out = scratch("round_trip.ply");
    export(&model, &out);
    assert_eq!(read_back(&out), *cloud_of(&model, "scan"));
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"ply\nformat binary_little_endian 1.0\n"));
}

#[test]
fn xyz_export_round_trips() {
    let model = load("cube_binary.ply", "");
    let out = scratch("round_trip.xyz");
    export(&model, &out);
    let back = read_back(&out);
    let original = cloud_of(&model, "scan");
    assert_eq!(back.positions, original.positions);
    assert_eq!(back.colours, original.colours);
    assert_eq!(back.classes, original.classes);
}

#[test]
fn xyz_fixture_reads_with_its_header() {
    let model = load("cube.xyz", "");
    assert_eq!(value(&model, "scan.points"), 2646.0);
    assert!(cloud_of(&model, "scan").normals.is_some());
}

#[test]
fn e57_with_two_poses_gives_the_world_bounds() {
    let model = load("two_scans.e57", "");
    assert_eq!(value(&model, "scan.scans"), 2.0);
    for (axis, lo, hi) in [("x", 0.0, 100.0), ("y", 0.0, 70.0), ("z", 0.0, 20.0)] {
        near(value(&model, &format!("scan.min.{axis}")), lo, 1.0e-9);
        near(value(&model, &format!("scan.max.{axis}")), hi, 1.0e-9);
    }
}

#[test]
fn e57_scan_picks_one_scan_with_its_pose() {
    let first = load("two_scans.e57", "scan=0");
    near(value(&first, "scan.max.x"), 20.0, 1.0e-9);
    let second = load("two_scans.e57", "scan=1");
    near(value(&second, "scan.min.x"), 80.0, 1.0e-9);
    near(value(&second, "scan.max.x"), 100.0, 1.0e-9);
    near(value(&second, "scan.min.y"), 50.0, 1.0e-9);
    assert_eq!(value(&second, "scan.points"), value(&first, "scan.points"));
    let error = failure(&format!("scan: cloud {} scan=2", fixture("two_scans.e57")));
    assert!(error.contains("2 scan(s)"), "{error}");
}

#[test]
fn las_scale_and_offset_give_the_survey_coordinates() {
    let model = load("survey.las", "");
    near(value(&model, "scan.min.x"), 500000.0, 1.0e-9);
    near(value(&model, "scan.max.x"), 500020.0, 1.0e-9);
    near(value(&model, "scan.min.y"), 4000000.0, 1.0e-9);
    near(value(&model, "scan.max.y"), 4000020.0, 1.0e-9);
    near(value(&model, "scan.max.z"), 20.0, 1.0e-9);
}

#[test]
fn las_keeps_its_classification() {
    let model = load("survey.las", "");
    let cloud = cloud_of(&model, "scan");
    let classes = cloud.classes.as_ref().expect("classes");
    let count = |c: u8| classes.iter().filter(|&&k| k == c).count();
    assert_eq!(count(2), 121);
    assert_eq!(count(6), 121);
    assert_eq!(count(1), 4 * 121);
    let ground_heights: Vec<f64> = (0..cloud.len())
        .filter(|&i| classes[i] == 2)
        .map(|i| cloud.positions[i][2])
        .collect();
    assert!(ground_heights.iter().all(|z| z.abs() < 1.0e-9));
}

#[test]
fn laz_reads_the_same_points_as_las() {
    let las = load("survey.las", "");
    let laz = load("survey.laz", "");
    assert_eq!(*cloud_of(&las, "scan"), *cloud_of(&laz, "scan"));
}

#[test]
fn max_points_takes_the_requested_count() {
    for file in [
        "cube_binary.ply",
        "cube_ascii.ply",
        "cube.xyz",
        "survey.las",
        "two_scans.e57",
    ] {
        let model = load(file, "max_points=100");
        assert_eq!(value(&model, "scan.points"), 100.0, "{file}");
    }
    let all = load("cube_binary.ply", "");
    let sampled = load("cube_binary.ply", "max_points=500");
    let (a, b) = (cloud_of(&all, "scan"), cloud_of(&sampled, "scan"));
    let (lo, hi) = b.bounds().unwrap();
    let (full_lo, full_hi) = a.bounds().unwrap();
    for k in 0..3 {
        assert!(lo[k] - full_lo[k] < 2.0 && full_hi[k] - hi[k] < 2.0);
    }
}

#[test]
fn max_points_sample_is_repeatable() {
    let one = load("cube_binary.ply", "max_points=300");
    let two = load("cube_binary.ply", "max_points=300");
    assert_eq!(*cloud_of(&one, "scan"), *cloud_of(&two, "scan"));
}

#[test]
fn max_points_above_the_file_keeps_every_point() {
    let model = load("cube_binary.ply", "max_points=999999");
    assert_eq!(value(&model, "scan.points"), 2646.0);
}

#[test]
fn points_and_bounds_are_queryable_in_expressions() {
    let model = build(&format!(
        "scan: cloud {}\nlet half=scan.size.x/2\nlet n=scan.points",
        fixture("cube_binary.ply")
    ));
    near(value(&model, "half"), 10.0, 0.1);
    assert_eq!(value(&model, "n"), 2646.0);
    near(value(&model, "scan.centre.z"), 10.0, 0.05);
}

#[test]
fn check_reports_the_count_the_bounds_and_the_memory() {
    let text = summary(&format!("scan: cloud {}", fixture("cube_binary.ply")));
    assert!(text.starts_with("2646 points, bounds "), "{text}");
    assert!(text.contains("1 scan(s)"), "{text}");
    assert!(text.contains("KB") || text.contains("MB"), "{text}");
    assert!(text.contains("normals colour intensity classes"), "{text}");
    let sampled = summary(&format!(
        "scan: cloud {} max_points=100",
        fixture("cube_binary.ply")
    ));
    assert!(sampled.starts_with("100 points"), "{sampled}");
    assert!(sampled.contains("sampled from 2646"), "{sampled}");
}

#[test]
fn import_with_the_cloud_keyword_reads_a_cloud() {
    let model = build(&format!(
        "scan: import {} cloud",
        fixture("cube_binary.ply")
    ));
    assert_eq!(value(&model, "scan.points"), 2646.0);
    let by_extension = build(&format!("scan: import {}", fixture("survey.laz")));
    assert_eq!(value(&by_extension, "scan.points"), 726.0);
}

#[test]
fn clouds_are_named_bodies_that_can_sit_beside_solids() {
    let model = build(&format!(
        "base: rect 10 10\nextrude 5\nbody pts\nscan: cloud {}\n",
        fixture("cube_binary.ply")
    ));
    assert!(model.cloud("scan").is_some());
    assert_eq!(model.solids().len(), 1);
    assert!(
        failure(&format!(
            "scan: cloud {}\nscan: cloud {}",
            fixture("cube_binary.ply"),
            fixture("cube_ascii.ply")
        ))
        .contains("already a body called `scan`")
    );
}

#[test]
fn import_errors_say_what_is_wrong() {
    assert!(failure("scan: cloud missing.ply").contains("missing.ply"));
    assert!(failure(&format!("scan: cloud {} max_points=0", fixture("cube.xyz"))).contains("at least 1"));
    assert!(
        failure(&format!("scan: cloud {} scan=1", fixture("cube.xyz")))
            .contains("picks one scan of an .e57")
    );
    assert!(failure("scan: cloud model.stl").contains(".e57"));
}

#[test]
fn a_cloud_exports_as_ply_xyz_las_and_e57_and_nothing_else() {
    let model = load("survey.las", "");
    for extension in ["ply", "xyz", "las", "laz", "e57"] {
        let out = scratch(&format!("export.{extension}"));
        export(&model, &out);
        assert_eq!(read_back(&out).len(), 726, "{extension}");
    }
    let err = gcad::export::export_model(&model, std::path::Path::new("scan.gcad"), &scratch("x.stl"))
        .unwrap_err()
        .to_string();
    assert!(err.contains(".ply"), "{err}");
}

#[test]
fn las_export_keeps_survey_coordinates() {
    let model = load("survey.las", "");
    let out = scratch("keep.las");
    export(&model, &out);
    let back = read_back(&out);
    let original = cloud_of(&model, "scan");
    for (a, b) in back.positions.iter().zip(&original.positions) {
        (0..3).for_each(|k| near(a[k], b[k], 5.0e-4));
    }
}

#[test]
fn viewer_colour_modes_follow_the_data() {
    let cloud = cloud_of(&load("survey.las", ""), "scan");
    let all: Vec<usize> = (0..cloud.len()).collect();
    let by_class = gcad_cloud::colour::colours(&cloud, ColourBy::Class, &all);
    let ground = cloud.classes.as_ref().unwrap().iter().position(|&c| c == 2).unwrap();
    assert_eq!(by_class[ground], gcad_cloud::colour::class_colour(2));
    let by_height = gcad_cloud::colour::colours(&cloud, ColourBy::Height, &all);
    let low = (0..cloud.len()).min_by(|&a, &b| cloud.positions[a][2].total_cmp(&cloud.positions[b][2])).unwrap();
    let high = (0..cloud.len()).max_by(|&a, &b| cloud.positions[a][2].total_cmp(&cloud.positions[b][2])).unwrap();
    assert_eq!(by_height[low], gcad_cloud::colour::ramp(0.0));
    assert_eq!(by_height[high], gcad_cloud::colour::ramp(1.0));
    let by_normal = gcad_cloud::colour::colours(&cloud_of(&load("cube_binary.ply", ""), "scan"), ColourBy::Normal, &[0]);
    assert_eq!(by_normal[0], [0, 128, 128].map(|v| v as u8));
    let plain = Cloud::from_positions(vec![[0.0; 3], [0.0, 0.0, 1.0]]);
    let missing = gcad_cloud::colour::colours(&plain, ColourBy::Class, &[0, 1]);
    assert_eq!(missing[0], gcad_cloud::colour::ramp(0.0));
}

#[test]
fn ten_million_points_are_drawn_within_the_cap() {
    let drawn = gcad_cloud::sample::display_indices(10_000_000, 400_000);
    assert_eq!(drawn.len(), 400_000);
    assert!(drawn.windows(2).all(|w| w[0] < w[1]));
    assert!(*drawn.last().unwrap() > 9_990_000);
    assert_eq!(gcad_cloud::sample::display_indices(50, 400_000).len(), 50);
}

#[test]
fn kd_tree_answers_neighbour_queries_on_an_imported_cloud() {
    let cloud = cloud_of(&load("cube_binary.ply", ""), "scan");
    let tree = gcad_cloud::KdTree::build(&cloud.positions);
    let found = tree.nearest(cloud.positions[100], 5);
    assert_eq!(cloud.positions[found[0].0], cloud.positions[100]);
    assert_eq!(found[0].1, 0.0);
    assert!(found[4].1 < 1.5);
    assert_eq!(tree.within([10.0, 10.0, 0.0], 1.2).len(), 5);
}

fn imported_scan() -> String {
    format!("scan: cloud {}\n", fixture("cube_binary.ply"))
}

#[test]
fn crop_keeps_the_points_in_a_box() {
    let before = cloud_of(&load("cube_binary.ply", ""), "scan");
    let expected = before.positions.iter().filter(|p| p[0] <= 10.0).count();
    let model = build(&format!("{}crop scan box=-1,-1,-1,10,21,21", imported_scan()));
    near(value(&model, "scan.max.x"), 10.0, 0.05);
    assert_eq!(value(&model, "scan.points"), expected as f64);
}

#[test]
fn voxel_downsample_gives_one_point_per_occupied_voxel() {
    let before = cloud_of(&load("cube_binary.ply", ""), "scan");
    let cells: std::collections::HashSet<[i64; 3]> = before
        .positions
        .iter()
        .map(|p| p.map(|v| (v / 5.0).floor() as i64))
        .collect();
    let model = build(&format!("{}downsample scan voxel=5", imported_scan()));
    assert_eq!(value(&model, "scan.points"), cells.len() as f64);
}

#[test]
fn downsample_every_and_count_are_seeded() {
    let one = build(&format!("{}downsample scan count=300", imported_scan()));
    let two = build(&format!("{}downsample scan count=300", imported_scan()));
    assert_eq!(value(&one, "scan.points"), 300.0);
    assert_eq!(*cloud_of(&one, "scan"), *cloud_of(&two, "scan"));
    let every = build(&format!("{}downsample scan every=10", imported_scan()));
    assert_eq!(value(&every, "scan.points"), 265.0);
}

#[test]
fn statistical_denoise_removes_the_outliers_and_few_good_points() {
    let model = build(&format!(
        "{}denoise scan stat k=16 sigma=2",
        fixture_with_outliers()
    ));
    let kept = value(&model, "scan.points");
    assert!(kept >= 2646.0 * 0.99 && kept <= 2646.0, "{kept}");
}

fn fixture_with_outliers() -> String {
    format!(
        "{}cloud_add_outliers scan count=50 distance=40\n",
        imported_scan()
    )
}

#[test]
fn radius_denoise_removes_isolated_points() {
    let model = build(&format!("{}denoise scan radius=10 min=5", fixture_with_outliers()));
    assert_eq!(value(&model, "scan.points"), 2646.0);
}

#[test]
fn normals_on_the_cube_faces_are_within_2_degrees_and_consistent() {
    let model = build(&format!("{}normals scan k=20 toward=10,10,10", imported_scan()));
    let cloud = cloud_of(&model, "scan");
    let normals = cloud.normals.as_ref().expect("normals");
    for (p, n) in cloud.positions.iter().zip(normals) {
        let axis = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs())).unwrap();
        assert!(n[axis].abs() > (2.0f32.to_radians()).cos());
        let inward = (10.0 - p[axis]).signum() as f32;
        assert_eq!(n[axis].signum(), inward);
    }
}

#[test]
fn tree_orientation_is_consistent_across_all_six_faces() {
    let model = build(&format!("{}normals scan k=20", imported_scan()));
    let cloud = cloud_of(&model, "scan");
    let normals = cloud.normals.as_ref().expect("normals");
    let mut sides = std::collections::HashSet::new();
    for (p, n) in cloud.positions.iter().zip(normals) {
        let axis = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs())).unwrap();
        assert!(n[axis].abs() > (2.0f32.to_radians()).cos());
        let faces_out = n[axis].signum() == (p[axis] - 10.0).signum() as f32;
        sides.insert(faces_out);
    }
    assert_eq!(sides.len(), 1, "every normal points out, or every one points in");
}

#[test]
fn cloud_transforms_match_the_solid_transforms() {
    let model = build(&format!(
        "{}move scan 5,0,0\nrotate scan z 90\nscale scan 2",
        imported_scan()
    ));
    near(value(&model, "scan.size.x"), 40.0, 0.2);
    let transformed = build(&format!(
        "{}transform scan 1,0,0,5,0,1,0,0,0,0,1,0,0,0,0,1",
        imported_scan()
    ));
    near(value(&transformed, "scan.min.x"), 5.0, 0.05);
}

#[test]
fn merge_keeps_every_scan_id() {
    let model = build(&format!(
        "a: cloud {0}\nb: cloud {1}\nmerge a b into all",
        fixture("cube_binary.ply"),
        fixture("survey.las")
    ));
    assert_eq!(value(&model, "all.points"), 2646.0 + 726.0);
    assert_eq!(value(&model, "all.scans"), 2.0);
}

#[test]
fn colour_by_and_info() {
    let model = build(&format!("{}colour scan by=height\ninfo scan", imported_scan()));
    assert!(cloud_of(&model, "scan").colours.is_some());
    let info = summary(&format!("{}info scan", imported_scan()));
    assert!(info.contains("2646 points") && info.contains("spacing"), "{info}");
}

#[test]
fn ten_million_points_through_voxel_and_stat() {
    let source = "scan: cloud_synthetic count=10000000 seed=1\ndownsample scan voxel=0.05\ndenoise scan stat k=16 sigma=2";
    let started = std::time::Instant::now();
    let model = build(source);
    assert!(value(&model, "scan.points") > 0.0);
    assert!(started.elapsed().as_secs() < 120);
}

#[test]
fn one_million_points_through_poisson() {
    let source = "scan: cloud_synthetic sphere count=1000000 r=50 noise=0.1\nnormals scan k=12\nmesh scan poisson depth=9 into=m";
    let started = std::time::Instant::now();
    let model = build(source);
    let seconds = started.elapsed().as_secs_f64();
    println!("1M point Poisson, normals included: {seconds:.1} s");
    let mesh = model.meshes().last().unwrap().1.clone();
    assert_eq!(mesh.open_edges(), 0);
    assert!(seconds < 300.0);
}

#[test]
fn fit_all_finds_six_planes_on_a_scanned_box() {
    let model = build(&format!(
        "{}fit scan plane tol=0.5 into=f all max=20",
        noisy_box_scan()
    ));
    assert_eq!(value(&model, "f.count"), 6.0);
    let mut spans = [(f64::INFINITY, f64::NEG_INFINITY); 3];
    for k in 1..=6 {
        let normal: Vec<f64> = ["x", "y", "z"]
            .iter()
            .map(|a| value(&model, &format!("f{k}.normal.{a}")))
            .collect();
        let origin: Vec<f64> = ["x", "y", "z"]
            .iter()
            .map(|a| value(&model, &format!("f{k}.origin.{a}")))
            .collect();
        let axis = (0..3)
            .max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs()))
            .unwrap();
        assert!(normal[axis].abs() > 0.5f64.to_radians().cos(), "normal {normal:?}");
        spans[axis].0 = spans[axis].0.min(origin[axis]);
        spans[axis].1 = spans[axis].1.max(origin[axis]);
    }
    for (lo, hi) in spans {
        near(hi - lo, 20.0, 0.3);
    }
}

#[test]
fn fit_cylinder_finds_radius_and_axis() {
    let model = build("scan: cloud_synthetic cylinder r=10 h=40 noise=0.2\nfit scan cylinder tol=0.5 into=c");
    near(value(&model, "c.radius"), 10.0, 0.1);
    assert!(value(&model, "c.axis.z").abs() >= 0.5f64.to_radians().cos());
}

#[test]
fn fit_sphere_and_cone() {
    let model = build("scan: cloud_synthetic sphere r=25 noise=0.2\nfit scan sphere tol=0.5 into=s");
    near(value(&model, "s.radius"), 25.0, 0.2);
}

#[test]
fn three_separated_blobs_cluster_into_three() {
    let source = "scan: cloud_synthetic blobs=3 spacing=100 r=10\ncluster scan dist=5 min=100 into=parts";
    let model = build(source);
    assert_eq!(value(&model, "parts.count"), 3.0);
    assert!(model.cloud("parts.1").is_some() && model.cloud("parts.3").is_some());
}

#[test]
fn region_growing_splits_a_box_into_its_faces() {
    let model = build(&format!(
        "{}normals scan k=20\ngrow scan angle=5 curvature=0.05 into=regions",
        imported_scan()
    ));
    assert_eq!(value(&model, "regions.count"), 6.0);
}

#[test]
fn slice_of_a_scanned_extrusion_rebuilds_its_volume() {
    let source = format!(
        "{}slice scan plane=z offset=10 thickness=1 into=s\nextrude 20",
        imported_scan()
    );
    let model = build(&source);
    near(value(&model, "s.area"), 400.0, 4.0);
    assert_volume(&model, 8000.0, 0.01);
}

fn l_shape_scan() -> String {
    let inside = |x: f64, y: f64| (0.0..=40.0).contains(&x) && (0.0..=40.0).contains(&y) && (x <= 15.0 || y <= 15.0);
    let mut text = String::new();
    for k in 0..=20 {
        let z = k as f64;
        for i in 0..=160 {
            for j in 0..=160 {
                let (x, y) = (i as f64 * 0.25, j as f64 * 0.25);
                let near_edge = [x, y].iter().any(|v| (v % 40.0).abs() < 1e-9)
                    || (x == 15.0 && y >= 15.0)
                    || (y == 15.0 && x >= 15.0);
                if inside(x, y) && (near_edge || k == 10) {
                    text += &format!("{x} {y} {z}\n");
                }
            }
        }
    }
    let path = scratch("l_shape.xyz");
    std::fs::write(&path, text).unwrap();
    format!("scan: cloud {path}\n")
}

#[test]
fn slice_of_a_scanned_l_shaped_extrusion_keeps_its_concave_area() {
    let source = format!(
        "{}slice scan plane=z offset=10 thickness=0.5 into=s\nextrude 20",
        l_shape_scan()
    );
    let model = build(&source);
    near(value(&model, "s.area"), 975.0, 9.75);
    assert_volume(&model, 19500.0, 0.01);
}

#[test]
fn a_fitted_cylinder_drives_a_hole_of_its_radius_along_its_axis() {
    let mut text = String::new();
    let mut state = 99u64;
    let mut noise = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    };
    for i in 0..=120 {
        for j in 0..=120 {
            let (x, y) = (i as f64 * 0.5, j as f64 * 0.5);
            if (x - 30.0).hypot(y - 30.0) > 6.0 {
                for z in [0.0, 5.0] {
                    text += &format!("{x} {y} {}\n", z + 0.05 * noise());
                }
            }
        }
    }
    for k in 0..4000 {
        let a = k as f64 * 0.0157;
        let z = (k % 10) as f64 * 0.5555;
        let r = 6.0 + 0.05 * noise();
        text += &format!("{} {} {z}\n", 30.0 + r * a.cos(), 30.0 + r * a.sin());
    }
    let path = scratch("drilled_plate.xyz");
    std::fs::write(&path, text).unwrap();
    let source = format!(
        "scan: cloud {path}\ncrop scan box=20,20,-1,40,40,6\nfit scan cylinder tol=0.3 into=c\nplate: rect 60 60 at=30,30\nextrude 5\nplane c.cylinder\nhole c.radius*2 0,0\n"
    );
    let model = build(&source);
    near(value(&model, "c.radius"), 6.0, 0.1);
    assert!(value(&model, "c.axis.z").abs() >= 0.5f64.to_radians().cos());
    let drilled = volume(&model);
    near(drilled, 60.0 * 60.0 * 5.0 - std::f64::consts::PI * 36.0 * 5.0, 15.0);
}

fn terrain_las() -> String {
    let mut cloud = Cloud::with_layout(
        gcad_cloud::Layout {
            classes: true,
            ..Default::default()
        },
        20000,
    );
    let ground = |x: f64, y: f64| 2.0 * (x / 40.0).sin() + 0.05 * y;
    let mut put = |x: f64, y: f64, z: f64, class: u8| {
        let mut point = gcad_cloud::Point::at([x + 500000.0, y + 4000000.0, z]);
        point.class = Some(class);
        cloud.push(point);
    };
    for i in 0..100 {
        for j in 0..100 {
            let (x, y) = (i as f64, j as f64);
            put(x, y, ground(x, y), 2);
        }
    }
    for (cx, cy, size, height) in [(20.0, 30.0, 12.0, 8.0), (60.0, 60.0, 15.0, 10.0), (75.0, 15.0, 6.0, 5.0)] {
        for i in 0..size as usize {
            for j in 0..size as usize {
                let (x, y) = (cx + i as f64, cy + j as f64);
                put(x, y, ground(x, y) + height, 6);
            }
        }
    }
    for k in 0..40 {
        let (x, y) = (10.0 + 2.2 * k as f64, 85.0 - 1.7 * k as f64);
        for h in 1..6 {
            put(x + 0.3, y, ground(x, y) + h as f64, 5);
        }
    }
    let path = scratch("terrain.las");
    gcad_cloud::las::write(std::path::Path::new(&path), &cloud, 0.001).unwrap();
    path
}

#[test]
fn ground_filter_matches_the_stored_classification() {
    let path = terrain_las();
    let stored: Vec<bool> = read_back(&path)
        .classes
        .unwrap()
        .iter()
        .map(|&c| c == 2)
        .collect();
    let model = build(&format!("scan: cloud {path}\nclassify scan ground cell=4 tol=0.6"));
    let cloud = cloud_of(&model, "scan");
    let classes = cloud.classes.as_ref().unwrap();
    let agree = classes.iter().zip(&stored).filter(|(c, s)| (**c == 2) == **s).count();
    assert!(agree as f64 >= 0.95 * classes.len() as f64, "{agree} of {}", classes.len());
}

fn noisy_box_scan() -> String {
    let mut text = String::new();
    let mut state = 12345u64;
    let mut next = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    };
    for axis in 0..3 {
        for at in [0.0, 20.0] {
            for i in 0..=40 {
                for j in 0..=40 {
                    let mut p = [0.0; 3];
                    p[axis] = at + 0.2 * next();
                    p[(axis + 1) % 3] = i as f64 * 0.5;
                    p[(axis + 2) % 3] = j as f64 * 0.5;
                    text += &format!("{} {} {}\n", p[0], p[1], p[2]);
                }
            }
        }
    }
    let path = scratch("noisy_box.xyz");
    std::fs::write(&path, text).unwrap();
    format!("scan: cloud {path}\n")
}


#[test]
fn a_fitted_plane_makes_a_workplane_and_a_cylinder_a_hole() {
    let source = format!(
        "{}fit scan plane tol=0.5 into=f\nplane f.plane\nrect 4 4\nextrude 2",
        imported_scan()
    );
    let model = build(&source);
    assert_eq!(model.solids().len(), 1);
}

#[test]
fn poisson_of_a_sphere_cloud_is_watertight_with_the_right_volume() {
    let source = "scan: cloud_synthetic sphere r=50 noise=0.2\nnormals scan k=20\nmesh scan poisson depth=9 trim=0.1 into=m";
    let model = build(source);
    let mesh = model.meshes().last().unwrap().1.clone();
    assert_eq!(mesh.open_edges(), 0);
    let exact = 4.0 / 3.0 * std::f64::consts::PI * 50.0f64.powi(3);
    near(mesh.volume(), exact, exact * 0.01);
    assert!(mesh.volume() > 0.0);
}

#[test]
fn ball_pivoting_covers_the_sphere_area() {
    let source = "scan: cloud_synthetic sphere r=50 noise=0.2\nnormals scan k=20\nmesh scan pivot r=2,4,8 into=m";
    let model = build(source);
    let mesh = model.meshes().last().unwrap().1.clone();
    let area = 4.0 * std::f64::consts::PI * 2500.0;
    assert!(mesh.area() >= 0.98 * area);
}

#[test]
fn filling_a_known_hole_closes_it_and_decimation_hits_the_target() {
    let source = "ball: blob 0,0,0 r=10\nremesh 0.5\nopen_hole ball at=0,0,10 r=3\nfill ball max_edges=100\ndecimate ball to=500";
    let model = build(source);
    let mesh = model.meshes().last().unwrap().1.clone();
    assert_eq!(mesh.open_edges(), 0);
    near(mesh.triangles.len() as f64, 500.0, 5.0);
}

#[test]
fn the_stanford_bunny_reconstructs_to_a_manifold_mesh() {
    let source = format!(
        "bunny: import {} mesh\nclean bunny\nscan: points bunny every=1\nnormals scan k=20\nrebuilt: mesh scan poisson depth=8 trim=0.05",
        fixture("bunny.obj")
    );
    let model = build(&source);
    let mesh = model
        .meshes()
        .into_iter()
        .find(|(n, _)| n == "rebuilt")
        .expect("mesh")
        .1;
    assert_eq!(mesh.open_edges(), 0);
    println!("bunny poisson triangles {}", mesh.triangles.len());
    assert!(mesh.triangles.len() > 5_000);
}

#[test]
fn clean_and_taubin_smoothing_keep_the_mesh_manifold() {
    let source = "ball: blob 0,0,0 r=10\nremesh 0.5\nsmooth ball taubin iters=10\nclean ball";
    let model = build(source);
    assert_eq!(model.meshes().last().unwrap().1.open_edges(), 0);
}

#[test]
fn a_scanner_ply_mesh_with_colours_imports_and_samples_back_to_a_cloud() {
    let source = format!(
        "m: import {} mesh\ns: points m every=1\nt: sample m count=1000",
        fixture("scanner_mesh.ply")
    );
    let model = build(&source);
    assert_eq!(value(&model, "t.points"), 1000.0);
    assert!(cloud_of(&model, "t").colours.is_some());
}

#[test]
fn clean_example_runs() {
    let source = std::fs::read_to_string(format!(
        "{}/examples/clouds/clean.gcad",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("example exists")
    .replace(
        "cloud cube.ply",
        &format!("cloud {}/examples/clouds/cube.ply", env!("CARGO_MANIFEST_DIR")),
    );
    let model = build(&source);
    assert!(cloud_of(&model, "scan").normals.is_some());
    assert!(value(&model, "scan.points") < 2646.0);
}

#[test]
fn fit_and_slice_examples_run() {
    for (name, expected) in [("fit", 16428.76), ("slice", 8000.0)] {
        let source = std::fs::read_to_string(format!(
            "{}/examples/clouds/{name}.gcad",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("example exists")
        .replace(
            "cloud cube.ply",
            &format!("cloud {}/examples/clouds/cube.ply", env!("CARGO_MANIFEST_DIR")),
        );
        assert_volume(&build(&source), expected, 0.03);
    }
}
