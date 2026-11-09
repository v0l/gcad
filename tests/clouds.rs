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
#[ignore = "missing: clean and sample, gcad #445"]
fn crop_keeps_the_points_in_a_box() {
    let before = cloud_of(&load("cube_binary.ply", ""), "scan");
    let expected = before.positions.iter().filter(|p| p[0] <= 10.0).count();
    let model = build(&format!("{}crop scan box=-1,-1,-1,10,21,21", imported_scan()));
    near(value(&model, "scan.max.x"), 10.0, 0.05);
    assert_eq!(value(&model, "scan.points"), expected as f64);
}

#[test]
#[ignore = "missing: clean and sample, gcad #445"]
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
#[ignore = "missing: clean and sample, gcad #445"]
fn downsample_every_and_count_are_seeded() {
    let one = build(&format!("{}downsample scan count=300", imported_scan()));
    let two = build(&format!("{}downsample scan count=300", imported_scan()));
    assert_eq!(value(&one, "scan.points"), 300.0);
    assert_eq!(*cloud_of(&one, "scan"), *cloud_of(&two, "scan"));
    let every = build(&format!("{}downsample scan every=10", imported_scan()));
    assert_eq!(value(&every, "scan.points"), 265.0);
}

#[test]
#[ignore = "missing: clean and sample, gcad #445"]
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
#[ignore = "missing: clean and sample, gcad #445"]
fn radius_denoise_removes_isolated_points() {
    let model = build(&format!("{}denoise scan radius=10 min=5", fixture_with_outliers()));
    assert_eq!(value(&model, "scan.points"), 2646.0);
}

#[test]
#[ignore = "missing: clean and sample, gcad #445"]
fn normals_on_the_cube_faces_are_within_2_degrees_and_consistent() {
    let model = build(&format!("{}normals scan k=20 toward=10,10,10", imported_scan()));
    let cloud = cloud_of(&model, "scan");
    let normals = cloud.normals.as_ref().expect("normals");
    for (p, n) in cloud.positions.iter().zip(normals) {
        let axis = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs())).unwrap();
        assert!(n[axis].abs() > (2.0f32.to_radians()).cos());
        let outward = (p[axis] - 10.0).signum() as f32;
        assert_eq!(n[axis].signum(), outward);
    }
}

#[test]
#[ignore = "missing: clean and sample, gcad #445"]
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
#[ignore = "missing: clean and sample, gcad #445"]
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
#[ignore = "missing: clean and sample, gcad #445"]
fn colour_by_and_info() {
    let model = build(&format!("{}colour scan by=height\ninfo scan", imported_scan()));
    assert!(cloud_of(&model, "scan").colours.is_some());
    let info = summary(&format!("{}info scan", imported_scan()));
    assert!(info.contains("2646 points") && info.contains("spacing"), "{info}");
}

#[test]
#[ignore = "missing: clean and sample, gcad #445"]
fn ten_million_points_through_voxel_and_stat() {
    let source = "scan: cloud_synthetic count=10000000 seed=1\ndownsample scan voxel=0.05\ndenoise scan stat k=16 sigma=2";
    let started = std::time::Instant::now();
    let model = build(source);
    assert!(value(&model, "scan.points") > 0.0);
    assert!(started.elapsed().as_secs() < 120);
}

#[test]
#[ignore = "missing: registration, gcad #446"]
fn point_pairs_with_three_exact_pairs_give_zero_rms() {
    let source = format!(
        "a: cloud {0}\nb: cloud {0}\nrotate b z 30\nmove b 40,0,0\nalign b to=a pairs=40,0,0>0,0,0 49.3,5,0>10,0,0 40,0,20>0,0,20",
        fixture("cube_binary.ply")
    );
    let model = build(&source);
    near(value(&model, "b.align.rms"), 0.0, 1.0e-6);
}

#[test]
#[ignore = "missing: registration, gcad #446"]
fn icp_recovers_a_known_transform_for_point_and_plane() {
    for kind in ["point", "plane"] {
        let source = format!(
            "a: cloud {0}\nnormals a k=20\nb: cloud {0}\nrotate b z 3\nmove b 0.5,0.2,0\nalign b to=a icp {kind} max_dist=20 iters=50",
            fixture("cube_binary.ply")
        );
        let model = build(&source);
        assert!(value(&model, "b.align.rms") < 0.1, "{kind}");
        assert!(value(&model, "b.align.iterations") <= 50.0);
    }
}

#[test]
#[ignore = "missing: registration, gcad #446"]
fn coarse_alignment_recovers_a_90_degree_500_offset() {
    let source = format!(
        "a: cloud {0}\nb: cloud {0}\nrotate b z 90\nmove b 500,0,0\nalign b to=a coarse voxel=2",
        fixture("cube_binary.ply")
    );
    let model = build(&source);
    assert!(value(&model, "b.align.rms") < 0.2);
    near(value(&model, "b.centre.x"), value(&model, "a.centre.x"), 0.2);
}

#[test]
#[ignore = "missing: registration, gcad #446"]
fn overlapping_partial_scans_merge_into_a_clean_cloud() {
    let source = format!(
        "a: cloud {0}\ncrop a box=-1,-1,-1,21,21,12\nb: cloud {0}\ncrop b box=-1,-1,8,21,21,21\nalign scans a b\nmerge a b into both\nfit both plane tol=0.5 into=f all max=6",
        fixture("cube_binary.ply")
    );
    let model = build(&source);
    assert_eq!(value(&model, "f.count"), 6.0);
}

#[test]
#[ignore = "missing: registration, gcad #446"]
fn scan_to_cad_icp_gives_an_rms_within_the_noise() {
    let source = format!(
        "part: rect 20 20 at=10,10\nextrude 20\nscan: cloud {}\nmove scan 0.3,0.2,0\nalign scan to=part icp point max_dist=5",
        fixture("cube_binary.ply")
    );
    let model = build(&source);
    assert!(value(&model, "scan.align.rms") < 0.1);
}

#[test]
#[ignore = "missing: segmentation and fitting, gcad #447"]
fn fit_all_finds_six_planes_on_a_scanned_box() {
    let model = build(&format!("{}fit scan plane tol=0.5 into=f all max=20", imported_scan()));
    assert_eq!(value(&model, "f.count"), 6.0);
    for k in 1..=6 {
        let normal = [
            value(&model, &format!("f{k}.normal.x")),
            value(&model, &format!("f{k}.normal.y")),
            value(&model, &format!("f{k}.normal.z")),
        ];
        let axis = normal.iter().cloned().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(axis > 0.5f64.to_radians().cos());
    }
}

#[test]
#[ignore = "missing: segmentation and fitting, gcad #447"]
fn fit_cylinder_finds_radius_and_axis() {
    let model = build("scan: cloud_synthetic cylinder r=10 h=40 noise=0.2\nfit scan cylinder tol=0.5 into=c");
    near(value(&model, "c.radius"), 10.0, 0.1);
    near(value(&model, "c.axis.z").abs(), 1.0, 0.5f64.to_radians().cos().abs() - 1.0 + 1.0);
}

#[test]
#[ignore = "missing: segmentation and fitting, gcad #447"]
fn fit_sphere_and_cone() {
    let model = build("scan: cloud_synthetic sphere r=25 noise=0.2\nfit scan sphere tol=0.5 into=s");
    near(value(&model, "s.radius"), 25.0, 0.2);
}

#[test]
#[ignore = "missing: segmentation and fitting, gcad #447"]
fn three_separated_blobs_cluster_into_three() {
    let source = "scan: cloud_synthetic blobs=3 spacing=100 r=10\ncluster scan dist=5 min=100 into=parts";
    let model = build(source);
    assert_eq!(value(&model, "parts.count"), 3.0);
}

#[test]
#[ignore = "missing: segmentation and fitting, gcad #447"]
fn region_growing_splits_a_box_into_its_faces() {
    let model = build(&format!(
        "{}normals scan k=20\ngrow scan angle=5 curvature=0.05 into=regions",
        imported_scan()
    ));
    assert_eq!(value(&model, "regions.count"), 6.0);
}

#[test]
#[ignore = "missing: segmentation and fitting, gcad #447"]
fn slice_of_a_scanned_extrusion_rebuilds_its_volume() {
    let source = format!(
        "{}slice scan plane=z thickness=1 into=s\nextrude s 20",
        imported_scan()
    );
    let model = build(&source);
    let volume = volume(&model);
    near(volume, 8000.0, 80.0);
}

#[test]
#[ignore = "missing: segmentation and fitting, gcad #447"]
fn ground_filter_matches_the_stored_classification() {
    let model = build(&format!("scan: cloud {}\nclassify scan ground", fixture("survey.las")));
    let cloud = cloud_of(&model, "scan");
    let classes = cloud.classes.as_ref().unwrap();
    let stored: Vec<bool> = load_stored_ground();
    let agree = classes.iter().zip(&stored).filter(|(c, s)| (**c == 2) == **s).count();
    assert!(agree as f64 >= 0.95 * classes.len() as f64);
}

fn load_stored_ground() -> Vec<bool> {
    cloud_of(&load("survey.las", ""), "scan")
        .classes
        .as_ref()
        .unwrap()
        .iter()
        .map(|&c| c == 2)
        .collect()
}

#[test]
#[ignore = "missing: segmentation and fitting, gcad #447"]
fn a_fitted_plane_makes_a_workplane_and_a_cylinder_a_hole() {
    let source = format!(
        "{}fit scan plane tol=0.5 into=f\nplane f.plane\nrect 4 4\nextrude 2",
        imported_scan()
    );
    let model = build(&source);
    assert_eq!(model.solids().len(), 1);
}

#[test]
#[ignore = "missing: surface reconstruction, gcad #448"]
fn poisson_of_a_sphere_cloud_is_watertight_with_the_right_volume() {
    let source = "scan: cloud_synthetic sphere r=50 noise=0.2\nnormals scan k=20\nmesh scan poisson depth=9 trim=0.1 into=m";
    let model = build(source);
    let mesh = model.meshes().last().unwrap().1.clone();
    assert_eq!(mesh.open_edges(), 0);
    let exact = 4.0 / 3.0 * std::f64::consts::PI * 50.0f64.powi(3);
    near(mesh.volume(), exact, exact * 0.01);
}

#[test]
#[ignore = "missing: surface reconstruction, gcad #448"]
fn ball_pivoting_covers_the_sphere_area() {
    let source = "scan: cloud_synthetic sphere r=50 noise=0.2\nnormals scan k=20\nmesh scan pivot r=2,4,8 into=m";
    let model = build(source);
    let mesh = model.meshes().last().unwrap().1.clone();
    let area = 4.0 * std::f64::consts::PI * 2500.0;
    assert!(mesh.area() >= 0.98 * area);
}

#[test]
#[ignore = "missing: surface reconstruction, gcad #448"]
fn filling_a_known_hole_closes_it_and_decimation_hits_the_target() {
    let source = "ball: blob 0,0,0 r=10\nremesh 0.5\nopen_hole ball at=0,0,10 r=3\nfill ball max_edges=100\ndecimate ball to=500";
    let model = build(source);
    let mesh = model.meshes().last().unwrap().1.clone();
    assert_eq!(mesh.open_edges(), 0);
    near(mesh.triangles.len() as f64, 500.0, 5.0);
}

#[test]
#[ignore = "missing: surface reconstruction, gcad #448"]
fn clean_and_taubin_smoothing_keep_the_mesh_manifold() {
    let source = "ball: blob 0,0,0 r=10\nremesh 0.5\nsmooth ball taubin iters=10\nclean ball";
    let model = build(source);
    assert_eq!(model.meshes().last().unwrap().1.open_edges(), 0);
}

#[test]
#[ignore = "missing: surface reconstruction, gcad #448"]
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
#[ignore = "missing: inspection, gcad #449"]
fn deviation_of_an_offset_cloud_has_the_right_mean_and_sign() {
    let source = format!(
        "part: rect 20 20 at=10,10\nextrude 20\nscan: cloud {}\ndeviation scan to=part max=0.5 into=d",
        fixture("cube_binary.ply")
    );
    let model = build(&source);
    near(value(&model, "d.mean"), 0.0, 0.05);
    assert!(value(&model, "d.within") > 0.99);
}

#[test]
#[ignore = "missing: inspection, gcad #449"]
fn deviation_flags_only_the_moved_face() {
    let source = format!(
        "part: rect 20 20 at=10,10\nextrude 20\nscan: cloud {}\ncrop_face_move scan face=+x by=1\ndeviation scan to=part max=0.5 into=d",
        fixture("cube_binary.ply")
    );
    let model = build(&source);
    let outside = 1.0 - value(&model, "d.within");
    near(outside, 441.0 / 2646.0, 0.01);
    near(value(&model, "d.max"), 1.0, 0.1);
}

#[test]
#[ignore = "missing: inspection, gcad #449"]
fn spacing_of_two_fitted_cylinders_matches_the_cad() {
    let source = "scan: cloud_synthetic two_cylinders r=5 spacing=30 noise=0.1\nfit scan cylinder tol=0.5 into=a\nfit scan cylinder tol=0.5 into=b\nmeasure scan a b";
    let text = summary(source);
    assert!(text.contains("30.0"), "{text}");
}

#[test]
#[ignore = "missing: inspection, gcad #449"]
fn check_lines_use_the_deviation_values() {
    let source = format!(
        "part: rect 20 20 at=10,10\nextrude 20\nscan: cloud {}\ndeviation scan to=part max=0.5 into=d\ncheck d.max < 0.3",
        fixture("cube_binary.ply")
    );
    assert!(failure(&source).contains("d.max"));
}

#[test]
#[ignore = "missing: inspection, gcad #449"]
fn scan_section_is_drawn_beside_the_cad_section() {
    let drawing = "section scan plane=z at=10 deviation=1";
    assert!(drawing.contains("deviation"));
    panic!("a .gdwg section of a scan is not built yet");
}

#[test]
#[ignore = "missing: inspection, gcad #449"]
fn inspection_report_csv_is_stable() {
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
            .args(["inspect", "examples/clouds/inspect.gcad", "--report", &scratch("report.csv")])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("runs");
        std::fs::read(scratch("report.csv")).expect("a report")
    };
    assert_eq!(run(), run());
}

#[test]
#[ignore = "missing: inspection, gcad #449"]
fn deviation_colour_map_renders() {
    let source = format!(
        "part: rect 20 20 at=10,10\nextrude 20\nscan: cloud {}\ndeviation scan to=part max=0.5 into=d\ncolour scan by=deviation",
        fixture("cube_binary.ply")
    );
    let model = build(&source);
    assert!(cloud_of(&model, "scan").colours.is_some());
}
