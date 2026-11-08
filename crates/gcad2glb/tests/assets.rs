#[cfg(feature = "brep")]
use gcad2glb::gcad_brep;
use gcad2glb::gcad_mesh;
use gcad2glb::{Assets, Fit, Options};

fn root() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/../..")
}

fn example(name: &str) -> String {
    format!("{}/examples/mesh/{name}.gcad", root())
}

const PLATE: &str =
    "rect 60 30\nplate: extrude 4\nplane plate.end\nhole 6 -20,0 20,0\ncolor #8a9aa8\n";

#[test]
fn a_mesh_file_gives_the_gcad_mesh_model_and_bytes() {
    let path = example("rigged_creature");
    let assets = Assets::from_path(&path, &Options::default()).expect("builds");
    let source = std::fs::read_to_string(&path).unwrap();
    let direct = gcad2glb::gcad_mesh::build(&source).unwrap();
    assert_eq!(assets.bodies.len(), direct.bodies.len());
    assert_eq!(assets.bodies[0].indices, direct.bodies[0].indices);
    assert!(!assets.bodies[0].joints.is_empty());
    assert!(assets.bodies[0].clip_index("walk").is_some());
    assert_eq!(
        assets.to_glb().unwrap(),
        gcad2glb::gcad_mesh::to_glb(&source).unwrap()
    );
    let from_text = Assets::from_source(&source, &Options::default()).unwrap();
    assert_eq!(from_text.to_glb().unwrap(), assets.to_glb().unwrap());
}

#[cfg(feature = "brep")]
#[test]
fn a_solid_file_becomes_a_tessellated_body() {
    let assets = Assets::from_source(PLATE, &Options::default()).expect("builds");
    assert_eq!(assets.bodies.len(), 1);
    let plate = &assets.bodies[0];
    assert_eq!(plate.name, "main");
    assert_eq!(plate.parts.len(), 1);
    assert!((plate.parts[0].colour[0] - 0x8a as f32 / 255.0).abs() < 1e-3);
    assert_eq!(plate.normals.len(), plate.positions.len());
    assert!(plate.indices.len() / 3 > 50, "{}", plate.indices.len());
    assert!(plate.joints.is_empty());
    let (lo, hi) = assets.bounds().unwrap();
    assert!((hi[1] - lo[1] - 4.0).abs() < 1e-3, "{lo:?} {hi:?}");
    assert!((hi[0] - lo[0] - 60.0).abs() < 1e-3, "{lo:?} {hi:?}");
    let lines = gcad_mesh::parse::parse_program(PLATE).unwrap();
    let run = gcad_brep::run(&lines);
    assert_eq!(
        assets.to_glb().unwrap(),
        gcad_brep::glb::glb_of(&run.model).unwrap()
    );
}

#[cfg(feature = "brep")]
#[test]
fn a_mixed_file_holds_the_solid_and_the_rigged_mesh() {
    let path = example("bracket_mascot");
    let assets = Assets::from_path(&path, &Options::default()).expect("builds");
    let names: Vec<&str> = assets.bodies.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["main", "mascot"]);
    let plate = assets.body("main").unwrap();
    assert!(plate.joints.is_empty() && !plate.indices.is_empty());
    let mascot = assets.body("mascot").unwrap();
    assert_eq!(mascot.joints.len(), 2);
    assert_eq!(mascot.joint_indices.len(), mascot.positions.len());
    let nod = mascot.clip_index("nod").expect("nod");
    let rest = mascot.skinned_positions(&mascot.bone_matrices(None, 0.0));
    let nodded = mascot.skinned_positions(&mascot.bone_matrices(Some(nod), 0.5));
    assert!(
        rest.iter()
            .zip(&nodded)
            .any(|(a, b)| (a[2] - b[2]).abs() > 0.5)
    );
    let run = gcad_brep::run_path(std::path::Path::new(&path), &[], None).unwrap();
    assert_eq!(
        assets.to_glb().unwrap(),
        gcad_brep::glb::glb_of(&run.model).unwrap()
    );
}

#[cfg(feature = "brep")]
#[test]
fn an_assembly_becomes_one_body_per_part() {
    let path = format!("{}/examples/assemblies/linkage.gasm", root());
    let assets = Assets::from_path(&path, &Options::default()).expect("builds");
    let names: Vec<&str> = assets.bodies.iter().map(|b| b.name.as_str()).collect();
    for part in ["frame", "crank", "coupler", "rocker"] {
        assert!(names.iter().any(|n| n.starts_with(part)), "{names:?}");
    }
    assert!(assets.triangles() > 100);
    let crank = assets
        .bodies
        .iter()
        .find(|b| b.name.starts_with("crank"))
        .unwrap();
    assert!((crank.parts[0].colour[0] - 0xc0 as f32 / 255.0).abs() < 1e-3);
}

#[test]
fn set_overrides_change_a_mesh_file() {
    let source = "let r=2\na: blob 0,0,0 r=r\nremesh 0.3\n";
    let small = Assets::from_source(source, &Options::default()).unwrap();
    let big = Assets::from_source(source, &Options::default().set("r", 4.0)).unwrap();
    let height = |a: &Assets| {
        let (lo, hi) = a.bounds().unwrap();
        hi[1] - lo[1]
    };
    assert!((height(&small) - 4.0).abs() < 0.3, "{}", height(&small));
    assert!((height(&big) - 8.0).abs() < 0.3, "{}", height(&big));
}

#[cfg(feature = "brep")]
#[test]
fn set_overrides_change_a_solid_file() {
    let source = "let w=10\nrect w w\nblock: extrude 2\n";
    let wide = Assets::from_source(source, &Options::default().set("w", 30.0)).unwrap();
    let (lo, hi) = wide.bounds().unwrap();
    assert!((hi[0] - lo[0] - 30.0).abs() < 1e-3, "{lo:?} {hi:?}");
}

#[test]
fn fit_gives_a_transform_and_the_fitted_glb() {
    let path = example("rigged_creature");
    let options = Options {
        fit: Fit {
            height: Some(1.0),
            centre: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let assets = Assets::from_path(&path, &options).unwrap();
    let (lo, hi) = assets.bounds().unwrap();
    let fit = assets.fit().expect("asked to fit");
    let apply = |p: [f32; 3]| -> [f32; 3] {
        std::array::from_fn(|k| p[k] * fit.scale + fit.translation[k])
    };
    let (a, b) = (apply(lo), apply(hi));
    assert!((b[1] - a[1] - 1.0).abs() < 1e-5, "{a:?} {b:?}");
    assert!(a[1].abs() < 1e-5);
    assert!((a[0] + b[0]).abs() < 1e-5 && (a[2] + b[2]).abs() < 1e-5);
    let source = std::fs::read_to_string(&path).unwrap();
    let expected =
        gcad_mesh::gltf::fit(&gcad_mesh::to_glb(&source).unwrap(), &options.fit).unwrap();
    assert!(assets.to_glb().unwrap() == expected);
    assert!(
        Assets::from_path(&path, &Options::default())
            .unwrap()
            .fit()
            .is_none()
    );
    let both = Options {
        fit: Fit {
            height: Some(1.0),
            scale: Some(2.0),
            centre: false,
        },
        ..Default::default()
    };
    assert!(Assets::from_path(&path, &both).is_err());
}

#[test]
fn lod_and_texture_options_override_the_file() {
    let source = "ball: blob 0,0,0 r=10\nremesh 0.5\nfinish decimate=1500\nbake normals size=64\nlod 600 200\n";
    let file = Assets::from_source(source, &Options::default()).unwrap();
    let ball = &file.bodies[0];
    let lods = ball.lods();
    assert_eq!(
        lods.iter().map(|l| l.indices.len() / 3).collect::<Vec<_>>(),
        [600, 200]
    );
    assert_eq!(lods[0].name, "main_LOD1");
    let map = ball.normal_map().expect("baked");
    assert_eq!(map.size, 64);
    assert!(map.png.starts_with(b"\x89PNG"));
    assert_eq!(map.uvs.len(), map.positions.len());
    assert_eq!(map.indices.len(), ball.indices.len());
    let options = Options {
        lods: Some(vec![gcad2glb::LodSize::Triangles(300)]),
        texture_size: Some(32),
        ..Default::default()
    };
    let changed = Assets::from_source(source, &options).unwrap();
    let ball = &changed.bodies[0];
    assert_eq!(ball.lods().len(), 1);
    assert_eq!(ball.lods()[0].indices.len() / 3, 300);
    assert_eq!(ball.normal_map().unwrap().size, 32);
    let glb = String::from_utf8_lossy(&changed.to_glb().unwrap()).to_string();
    assert!(glb.contains("main_LOD1") && !glb.contains("main_LOD2"));
}

#[test]
fn correctives_come_out_as_morph_targets_with_clip_weights() {
    let assets = Assets::from_path(example("skin_arm_corrective"), &Options::default()).unwrap();
    let body = assets
        .bodies
        .iter()
        .find(|b| !b.targets.is_empty())
        .expect("a corrective");
    assert_eq!(body.targets[0].deltas.len(), body.positions.len());
    assert_eq!(body.target_weights.len(), body.targets.len());
    let clip = body
        .clips
        .iter()
        .find(|c| c.weights.is_some())
        .expect("weights");
    let track = clip.weights.as_ref().unwrap();
    assert_eq!(track.times.len(), track.weights.len());
    assert!(track.weights.iter().any(|w| w[0] > 0.1));
}

#[test]
fn errors_name_the_line_and_its_text() {
    let error = Assets::from_source("a: blob 0,0,0 r=2\nb: blob 1,2 r=2\n", &Options::default())
        .unwrap_err();
    assert_eq!(error.line, 2);
    assert_eq!(error.text, "b: blob 1,2 r=2");
    assert!(
        error.to_string().starts_with("line 2 `b: blob 1,2 r=2`: "),
        "{error}"
    );
}

#[cfg(feature = "brep")]
#[test]
fn a_bad_solid_line_names_its_line() {
    let source = "rect 10 10\nplate: extrude 2\nplane plate.end\nhole 3 0,0\nfillet 99 plate.end\n";
    let error = Assets::from_source(source, &Options::default()).unwrap_err();
    assert_eq!(error.line, 5, "{error}");
    assert_eq!(error.text, "fillet 99 plate.end");
    let typo =
        Assets::from_source("a: blob 0,0,0 r=2\nnonsense 3\n", &Options::default()).unwrap_err();
    assert_eq!(typo.line, 2, "{typo}");
}

#[cfg(not(feature = "brep"))]
#[test]
fn without_brep_a_solid_file_is_refused_with_its_line() {
    let error = Assets::from_path(example("bracket_mascot"), &Options::default()).unwrap_err();
    assert_eq!(error.line, 5, "{error}");
    assert!(
        error.to_string().contains("without the brep feature"),
        "{error}"
    );
    let error = Assets::from_source(PLATE, &Options::default()).unwrap_err();
    assert!(
        error.to_string().contains("without the brep feature"),
        "{error}"
    );
}

#[test]
fn the_cache_key_is_stable_and_follows_source_options_and_version() {
    let options = Options::default();
    let key = gcad2glb::cache_key_with("0.6.1+abc", PLATE, &options);
    assert_eq!(
        key,
        gcad2glb::cache_key_with("0.6.1+abc", PLATE, &options.clone())
    );
    assert_eq!(key, 0xa75f_fa8c_b841_6cec, "{key:#x}");
    let changed = [
        gcad2glb::cache_key_with("0.6.1+abd", PLATE, &options),
        gcad2glb::cache_key_with(
            "0.6.1+abc",
            &PLATE.replace("extrude 4", "extrude 5"),
            &options,
        ),
        gcad2glb::cache_key_with("0.6.1+abc", PLATE, &options.clone().set("r", 1.0)),
        gcad2glb::cache_key_with(
            "0.6.1+abc",
            PLATE,
            &Options {
                detail: 2.0,
                ..Default::default()
            },
        ),
        gcad2glb::cache_key_with(
            "0.6.1+abc",
            PLATE,
            &Options {
                triangles: Some(100),
                ..Default::default()
            },
        ),
        gcad2glb::cache_key_with(
            "0.6.1+abc",
            PLATE,
            &Options {
                fit: Fit {
                    height: Some(1.0),
                    ..Default::default()
                },
                ..Default::default()
            },
        ),
        gcad2glb::cache_key_with(
            "0.6.1+abc",
            PLATE,
            &Options {
                texture_size: Some(64),
                ..Default::default()
            },
        ),
    ];
    for (i, other) in changed.iter().enumerate() {
        assert_ne!(*other, key, "change {i}");
    }
    assert_eq!(
        gcad2glb::cache_key(PLATE, &options),
        gcad2glb::cache_key_with(gcad2glb::VERSION, PLATE, &options)
    );
}
