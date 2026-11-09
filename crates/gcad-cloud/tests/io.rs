use gcad_cloud::{Cloud, Format, KdTree, Layout, Options, Point, Pose, e57, import_bytes, las, ply, xyz};

fn grid(n: usize) -> Cloud {
    let mut cloud = Cloud::from_positions(Vec::new());
    for i in 0..n {
        for j in 0..n {
            cloud.push(Point::at([i as f64, j as f64, 0.0]));
        }
    }
    cloud
}

fn rich() -> Cloud {
    let layout = Layout {
        normals: true,
        colours: true,
        intensity: true,
        classes: true,
    };
    let mut cloud = Cloud::with_layout(layout, 4);
    for k in 0..4 {
        let mut p = Point::at([k as f64 * 0.1 + 1.0e-9, 500000.123456789, -3.5]);
        p.normal = Some([0.0, 0.6, 0.8]);
        p.colour = Some([k as u8 * 10, 20, 250]);
        p.intensity = Some(k as f32 / 8.0);
        p.class = Some(k as u8 + 1);
        p.scan = k as u16;
        cloud.push(p);
    }
    cloud
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gcad-cloud-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn binary_ply_round_trips_every_column() {
    let cloud = rich();
    let mut bytes = Vec::new();
    ply::write(&cloud, &mut bytes).unwrap();
    let back = import_bytes(Format::Ply, bytes, &Options::default()).unwrap().cloud;
    assert_eq!(back, cloud);
}

#[test]
fn ascii_ply_round_trips_every_column() {
    let cloud = rich();
    let mut bytes = Vec::new();
    ply::write_ascii(&cloud, &mut bytes).unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("format ascii 1.0"));
    let back = import_bytes(Format::Ply, bytes, &Options::default()).unwrap().cloud;
    assert_eq!(back, cloud);
}

#[test]
fn big_endian_ply_reads() {
    let mut bytes = b"ply\nformat binary_big_endian 1.0\nelement vertex 2\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nend_header\n".to_vec();
    for (p, c) in [([1.5f32, 2.0, 3.0], [1u8, 2, 3]), ([4.0, 5.0, 6.25], [4, 5, 6])] {
        p.iter().for_each(|v| bytes.extend(v.to_be_bytes()));
        bytes.extend(c);
    }
    let cloud = import_bytes(Format::Ply, bytes, &Options::default()).unwrap().cloud;
    assert_eq!(cloud.positions, vec![[1.5, 2.0, 3.0], [4.0, 5.0, 6.25]]);
    assert_eq!(cloud.colours.unwrap(), vec![[1, 2, 3], [4, 5, 6]]);
}

#[test]
fn ply_faces_are_skipped_after_the_vertices() {
    let text = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n";
    let cloud = import_bytes(Format::Ply, text.as_bytes().to_vec(), &Options::default())
        .unwrap()
        .cloud;
    assert_eq!(cloud.len(), 3);
}

#[test]
fn ply_without_xyz_is_refused() {
    let text = "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nend_header\n0 0\n";
    let error = import_bytes(Format::Ply, text.as_bytes().to_vec(), &Options::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("x, y and z"), "{error}");
}

#[test]
fn max_points_gives_the_requested_count_for_every_format() {
    let cloud = grid(40);
    let wanted = Options {
        max_points: Some(100),
        scan: None,
    };
    let mut bytes = Vec::new();
    ply::write(&cloud, &mut bytes).unwrap();
    assert_eq!(import_bytes(Format::Ply, bytes, &wanted).unwrap().cloud.len(), 100);
    let mut text = Vec::new();
    xyz::write(&cloud, &mut text).unwrap();
    let sampled = import_bytes(Format::Xyz, text, &wanted).unwrap();
    assert_eq!(sampled.cloud.len(), 100);
    assert_eq!(sampled.in_file, 1600);
}

#[test]
fn uniform_sample_covers_the_whole_cloud() {
    let cloud = grid(40);
    let mut bytes = Vec::new();
    ply::write(&cloud, &mut bytes).unwrap();
    let sampled = import_bytes(
        Format::Ply,
        bytes,
        &Options {
            max_points: Some(100),
            scan: None,
        },
    )
    .unwrap()
    .cloud;
    let (lo, hi) = sampled.bounds().unwrap();
    assert!(lo[0] <= 3.0 && hi[0] >= 36.0, "{lo:?} {hi:?}");
}

#[test]
fn xyz_reads_plain_and_coloured_lines() {
    let text = "# scan\n1 2 3\n4,5,6\n7;8;9\n";
    let cloud = import_bytes(Format::Xyz, text.as_bytes().to_vec(), &Options::default())
        .unwrap()
        .cloud;
    assert_eq!(cloud.positions, vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]);
    let coloured = "1 2 3 255 0 10\n";
    let cloud = import_bytes(Format::Xyz, coloured.as_bytes().to_vec(), &Options::default())
        .unwrap()
        .cloud;
    assert_eq!(cloud.colours.unwrap(), vec![[255, 0, 10]]);
}

#[test]
fn csv_with_a_header_names_its_columns() {
    let text = "id,x,y,z\n1,10,20,30\n2,11,21,31\n";
    let cloud = import_bytes(Format::Csv, text.as_bytes().to_vec(), &Options::default())
        .unwrap()
        .cloud;
    assert_eq!(cloud.positions, vec![[10.0, 20.0, 30.0], [11.0, 21.0, 31.0]]);
}

#[test]
fn pts_skips_its_count_line_and_reads_intensity() {
    let text = "2\n1 2 3 500\n4 5 6 -20\n";
    let cloud = import_bytes(Format::Pts, text.as_bytes().to_vec(), &Options::default())
        .unwrap()
        .cloud;
    assert_eq!(cloud.len(), 2);
    assert_eq!(cloud.intensity.unwrap(), vec![500.0, -20.0]);
}

#[test]
fn xyz_error_names_the_line() {
    let error = import_bytes(Format::Xyz, b"1 2 3\n4 five 6\n".to_vec(), &Options::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("line 2"), "{error}");
}

#[test]
fn e57_with_two_poses_has_the_world_bounds() {
    let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/clouds/two_scans.e57"));
    let file = std::fs::File::open(path).unwrap();
    let both = gcad_cloud::import(Format::E57, Box::new(file), &Options::default()).unwrap();
    assert_eq!(both.scans, 2);
    let (lo, hi) = both.cloud.bounds().unwrap();
    for (a, b) in lo.iter().chain(&hi).zip([0.0, 0.0, 0.0, 100.0, 70.0, 20.0]) {
        assert!((a - b).abs() < 1.0e-9, "{lo:?} {hi:?}");
    }
    assert_eq!(both.cloud.scan_ids(), vec![0, 1]);
}

#[test]
fn e57_scan_option_picks_one_scan_and_its_pose() {
    let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/clouds/two_scans.e57"));
    let pick = |n| {
        gcad_cloud::import(
            Format::E57,
            Box::new(std::fs::File::open(path).unwrap()),
            &Options {
                max_points: None,
                scan: Some(n),
            },
        )
    };
    let (lo, hi) = pick(1).unwrap().cloud.bounds().unwrap();
    for (a, b) in lo.iter().chain(&hi).zip([80.0, 50.0, 0.0, 100.0, 70.0, 20.0]) {
        assert!((a - b).abs() < 1.0e-9, "{lo:?} {hi:?}");
    }
    let error = pick(2).err().unwrap().to_string();
    assert!(error.contains("2 scan(s)"), "{error}");
}

#[test]
fn e57_write_then_read_keeps_points_and_colour() {
    let cloud = rich();
    let path = scratch("round.e57");
    e57::write(&path, &[(&cloud, Pose::default())]).unwrap();
    let back = gcad_cloud::import(
        Format::E57,
        Box::new(std::fs::File::open(&path).unwrap()),
        &Options::default(),
    )
    .unwrap()
    .cloud;
    assert_eq!(back.len(), 4);
    assert_eq!(back.colours.unwrap(), cloud.colours.unwrap());
    for (a, b) in back.positions.iter().zip(&cloud.positions) {
        (0..3).for_each(|k| assert!((a[k] - b[k]).abs() < 1.0e-9));
    }
}

#[test]
fn las_scale_and_offset_give_the_right_coordinates() {
    let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/clouds/survey.las"));
    let cloud = gcad_cloud::import(
        Format::Las,
        Box::new(std::fs::File::open(path).unwrap()),
        &Options::default(),
    )
    .unwrap()
    .cloud;
    let (lo, hi) = cloud.bounds().unwrap();
    for (a, b) in lo.iter().chain(&hi).zip([500000.0, 4000000.0, 0.0, 500020.0, 4000020.0, 20.0]) {
        assert!((a - b).abs() < 1.0e-9, "{lo:?} {hi:?}");
    }
}

#[test]
fn las_keeps_classification_and_colour() {
    let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/clouds/survey.las"));
    let cloud = gcad_cloud::import(
        Format::Las,
        Box::new(std::fs::File::open(path).unwrap()),
        &Options::default(),
    )
    .unwrap()
    .cloud;
    let mut classes = cloud.classes.clone().unwrap();
    classes.sort_unstable();
    classes.dedup();
    assert_eq!(classes, vec![1, 2, 6]);
    assert!(cloud.colours.is_some());
}

#[test]
fn laz_decodes_to_the_same_points_as_las() {
    let read = |name: &str, format| {
        let path = format!("{}/../../tests/fixtures/clouds/{name}", env!("CARGO_MANIFEST_DIR"));
        gcad_cloud::import(
            format,
            Box::new(std::fs::File::open(path).unwrap()),
            &Options::default(),
        )
        .unwrap()
        .cloud
    };
    assert_eq!(read("survey.las", Format::Las), read("survey.laz", Format::Laz));
}

#[test]
fn las_write_then_read_round_trips_within_the_scale() {
    let cloud = rich();
    let path = scratch("round.las");
    las::write(&path, &cloud, 0.001).unwrap();
    let back = gcad_cloud::import(
        Format::Las,
        Box::new(std::fs::File::open(&path).unwrap()),
        &Options::default(),
    )
    .unwrap()
    .cloud;
    for (a, b) in back.positions.iter().zip(&cloud.positions) {
        (0..3).for_each(|k| assert!((a[k] - b[k]).abs() <= 0.0005 + 1.0e-9, "{a:?} {b:?}"));
    }
    assert_eq!(back.classes.unwrap(), vec![1, 2, 3, 4]);
}

#[test]
fn kdtree_nearest_matches_brute_force() {
    let mut state = 12345u64;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 11) as f64 / (1u64 << 53) as f64 * 100.0
    };
    let points: Vec<[f64; 3]> = (0..2000).map(|_| [next(), next(), next()]).collect();
    let tree = KdTree::build(&points);
    for _ in 0..50 {
        let q = [next(), next(), next()];
        let mut brute: Vec<(usize, f64)> = points
            .iter()
            .enumerate()
            .map(|(i, p)| (i, (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f64>().sqrt()))
            .collect();
        brute.sort_by(|a, b| a.1.total_cmp(&b.1));
        let found = tree.nearest(q, 8);
        assert_eq!(found.iter().map(|f| f.0).collect::<Vec<_>>(), brute[..8].iter().map(|f| f.0).collect::<Vec<_>>());
        let mut inside = tree.within(q, 12.0);
        inside.sort_unstable();
        let mut expected: Vec<usize> = brute.iter().filter(|b| b.1 <= 12.0).map(|b| b.0).collect();
        expected.sort_unstable();
        assert_eq!(inside, expected);
    }
}

#[test]
fn kdtree_of_nothing_finds_nothing() {
    let tree = KdTree::build(&[]);
    assert!(tree.nearest([0.0; 3], 3).is_empty());
    assert!(tree.within([0.0; 3], 3.0).is_empty());
}

#[test]
fn unknown_extension_names_the_formats() {
    let error = gcad_cloud::format_of(std::path::Path::new("a.stl")).unwrap_err().to_string();
    assert!(error.contains(".e57"), "{error}");
}
