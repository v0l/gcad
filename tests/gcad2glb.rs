use gcad::export::glb_of;
use gcad::model::run;
use gcad::parse::parse_program;

fn exported(source: &str) -> Vec<u8> {
    let result = run(&parse_program(source).expect("parses"));
    if let Some((line, Err(error))) = result.steps.last() {
        panic!("line {} `{}`: {error:#}", line.number, line.text);
    }
    glb_of(&result.model).expect("exports")
}

fn example(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/examples/mesh/{name}.gcad",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("example")
}

#[test]
fn rigged_creature_matches_gcad_export_byte_for_byte() {
    let source = example("rigged_creature");
    assert_eq!(
        gcad_mesh::to_glb(&source).expect("builds"),
        exported(&source)
    );
}

#[test]
fn creature_matches_gcad_export_byte_for_byte() {
    let source = example("creature");
    assert_eq!(
        gcad_mesh::to_glb(&source).expect("builds"),
        exported(&source)
    );
}

#[test]
fn same_spec_gives_identical_bytes() {
    let source = example("rigged_creature");
    assert_eq!(
        gcad_mesh::to_glb(&source).expect("first"),
        gcad_mesh::to_glb(&source).expect("second")
    );
}

#[test]
fn solid_operations_are_refused_with_the_line_number() {
    let error = gcad_mesh::build("a: blob 0,0,0 r=2\nremesh 0.5\nhole 3 0,0\n").unwrap_err();
    assert_eq!(error.line, 3);
    assert!(error.message.contains("hole"), "{error}");
}

#[test]
fn bad_arguments_name_their_line() {
    let error = gcad_mesh::build("a: blob 0,0,0 r=2\nb: blob 1,2 r=2\n").unwrap_err();
    assert_eq!(error.line, 2);
    let error = gcad_mesh::build("a: blob 0,0,0 r=2\n\"oops\n").unwrap_err();
    assert_eq!(error.line, 2);
}

#[test]
fn lower_detail_gives_fewer_triangles() {
    let source = "a: blob 0,0,0 r=5\nremesh 0.3\n";
    let fine = gcad_mesh::build(source).expect("fine");
    let coarse = gcad_mesh::build_with(
        source,
        &gcad_mesh::Options {
            detail: 3.0,
            ..Default::default()
        },
    )
    .expect("coarse");
    assert!(coarse.bodies[0].indices.len() * 4 < fine.bodies[0].indices.len());
}

#[test]
fn fox_matches_gcad_export_byte_for_byte() {
    let source = example("fox");
    assert_eq!(
        gcad_mesh::to_glb(&source).expect("builds"),
        exported(&source)
    );
}

#[test]
fn bone_matrices_bend_the_skinned_positions() {
    let model = gcad_mesh::build(&example("rigged_creature")).expect("builds");
    let body = &model.bodies[0];
    let clip = body.clip_index("walk").expect("walk");
    let rest = body.bone_matrices(None, 0.0);
    let moved = body.bone_matrices(Some(clip), 0.0);
    assert_eq!(rest.len(), body.joint_count());
    let a = body.skinned_positions(&rest);
    let b = body.skinned_positions(&moved);
    let travel = a
        .iter()
        .zip(&b)
        .map(|(p, q)| (0..3).map(|k| (p[k] - q[k]).abs()).fold(0.0, f32::max))
        .fold(0.0, f32::max);
    assert!(travel > 0.5, "{travel}");
    let drift = a
        .iter()
        .zip(&body.positions)
        .map(|(p, q)| (0..3).map(|k| (p[k] - q[k]).abs()).fold(0.0, f32::max))
        .fold(0.0, f32::max);
    assert!(drift < 1.0e-4, "{drift}");
}

const SCULPT: &str =
    "a: blob 0,0,0 r=5\nremesh 0.1\nbrush b type=draw size=1 strength=0.2\ndab b at=0,0,5\n";

#[test]
fn a_triangle_budget_finishes_every_body_at_export() {
    let full = gcad_mesh::build(SCULPT).expect("full");
    let budget = gcad_mesh::build_with(
        SCULPT,
        &gcad_mesh::Options {
            triangles: Some(3000),
            ..Default::default()
        },
    )
    .expect("budget");
    assert!(full.bodies[0].indices.len() / 3 > 20000);
    assert_eq!(budget.bodies[0].indices.len() / 3, 3000);
}

#[test]
fn an_error_bound_keeps_the_surface_within_it() {
    let budget = gcad_mesh::build_with(
        SCULPT,
        &gcad_mesh::Options {
            max_error: Some(0.01),
            ..Default::default()
        },
    )
    .expect("budget");
    let full = gcad_mesh::build(SCULPT).expect("full");
    let kept = budget.bodies[0].indices.len();
    assert!(kept * 2 < full.bodies[0].indices.len(), "{kept}");
}

#[test]
fn export_tris_flag_matches_gcad2glb() {
    let dir = std::env::temp_dir().join("gcad-tests");
    std::fs::create_dir_all(&dir).expect("dir");
    let spec = dir.join("budget.gcad");
    std::fs::write(&spec, SCULPT).expect("writes");
    let (a, b) = (dir.join("budget-a.glb"), dir.join("budget-b.glb"));
    let gcad = std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
        .args([
            "export",
            spec.to_str().unwrap(),
            a.to_str().unwrap(),
            "--tris",
            "3000",
        ])
        .output()
        .expect("runs gcad");
    assert!(
        gcad.status.success(),
        "{}",
        String::from_utf8_lossy(&gcad.stderr)
    );
    let tool = gcad2glb_tool();
    let glb = std::process::Command::new(&tool)
        .args([
            "--tris",
            "3000",
            spec.to_str().unwrap(),
            b.to_str().unwrap(),
        ])
        .output()
        .expect("runs gcad2glb");
    assert!(
        glb.status.success(),
        "{}",
        String::from_utf8_lossy(&glb.stderr)
    );
    let (from_gcad, from_tool) = (std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    let expected = gcad_mesh::to_glb_with(
        SCULPT,
        &gcad_mesh::Options {
            triangles: Some(3000),
            ..Default::default()
        },
    )
    .expect("builds");
    assert_eq!(
        (from_gcad.len(), from_tool.len()),
        (expected.len(), expected.len())
    );
    assert!(
        from_gcad == expected && from_tool == expected,
        "same length, different bytes"
    );
}

fn gcad2glb_tool() -> std::path::PathBuf {
    built_gcad2glb("gcad2glb-tool", &[])
}

fn light_gcad2glb_tool() -> std::path::PathBuf {
    built_gcad2glb("gcad2glb-light", &["--no-default-features"])
}

fn built_gcad2glb(dir: &str, flags: &[&str]) -> std::path::PathBuf {
    let gcad = std::path::Path::new(env!("CARGO_BIN_EXE_gcad"));
    let profile_dir = gcad.parent().expect("profile dir");
    let release = profile_dir.file_name().is_some_and(|n| n == "release");
    let target = profile_dir.parent().expect("target dir").join(dir);
    let tool = target
        .join(if release { "release" } else { "debug" })
        .join(gcad.file_name().expect("exe name"))
        .with_file_name("gcad2glb")
        .with_extension(std::env::consts::EXE_EXTENSION);
    let mut build = std::process::Command::new(env!("CARGO"));
    build
        .args([
            "build",
            "-p",
            "gcad2glb",
            "--bin",
            "gcad2glb",
            "--target-dir",
        ])
        .arg(&target)
        .args(flags);
    if release {
        build.arg("--release");
    }
    let status = build.status().expect("builds gcad2glb");
    assert!(status.success(), "building gcad2glb");
    tool
}

fn gcad2glb_of(tool: &std::path::Path, spec: &str, out: &std::path::Path) -> std::process::Output {
    std::process::Command::new(tool)
        .args([spec, out.to_str().unwrap()])
        .output()
        .expect("runs gcad2glb")
}

#[test]
fn gcad2glb_writes_solids_and_meshes_like_export() {
    let dir = std::env::temp_dir().join("gcad-tests");
    std::fs::create_dir_all(&dir).expect("dir");
    let spec = format!(
        "{}/examples/mesh/bracket_mascot.gcad",
        env!("CARGO_MANIFEST_DIR")
    );
    let (a, b) = (dir.join("mixed-a.glb"), dir.join("mixed-b.glb"));
    let gcad = std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
        .args(["export", &spec, a.to_str().unwrap()])
        .output()
        .expect("runs gcad");
    assert!(
        gcad.status.success(),
        "{}",
        String::from_utf8_lossy(&gcad.stderr)
    );
    let glb = gcad2glb_of(&gcad2glb_tool(), &spec, &b);
    assert!(
        glb.status.success(),
        "{}",
        String::from_utf8_lossy(&glb.stderr)
    );
    let (from_gcad, from_tool) = (std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    assert!(from_gcad == from_tool, "same file, different bytes");
    let n = u32::from_le_bytes(from_tool[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&from_tool[20..20 + n]).unwrap();
    let names: Vec<&str> = doc["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["name"].as_str())
        .collect();
    assert!(
        names.contains(&"main") && names.contains(&"mascot"),
        "{names:?}"
    );
    assert_eq!(doc["skins"].as_array().map(Vec::len), Some(1));
    assert_eq!(doc["animations"][0]["name"], "nod");
}

#[test]
fn light_gcad2glb_refuses_solids_with_the_line_and_matches_on_meshes() {
    let dir = std::env::temp_dir().join("gcad-tests");
    std::fs::create_dir_all(&dir).expect("dir");
    let root = env!("CARGO_MANIFEST_DIR");
    let light = light_gcad2glb_tool();
    let full = gcad2glb_tool();
    let mixed = format!("{root}/examples/mesh/bracket_mascot.gcad");
    let refused = gcad2glb_of(&light, &mixed, &dir.join("light-mixed.glb"));
    assert!(!refused.status.success());
    let message = String::from_utf8_lossy(&refused.stderr);
    assert!(
        message.contains("line 5") && message.contains("without the brep feature"),
        "{message}"
    );
    for name in ["creature", "fox", "rigged_creature", "skin_arm"] {
        let spec = format!("{root}/examples/mesh/{name}.gcad");
        let (a, b) = (
            dir.join(format!("light-{name}.glb")),
            dir.join(format!("full-{name}.glb")),
        );
        assert!(gcad2glb_of(&light, &spec, &a).status.success(), "{name}");
        assert!(gcad2glb_of(&full, &spec, &b).status.success(), "{name}");
        assert!(
            std::fs::read(&a).unwrap() == std::fs::read(&b).unwrap(),
            "{name}"
        );
    }
}

const BAKED: &str = "ball: blob 0,0,0 r=10\nremesh 0.25\nbrush w type=dam_standard size=0.5 strength=0.3\nstroke w from=-4,0,9.2 to=4,0,9.2\nfinish decimate=1500\nbake normals size=64\nlod 600 200\n";

#[test]
fn export_and_gcad2glb_write_the_same_bytes_for_a_bake_and_lod() {
    let dir = std::env::temp_dir().join("gcad-tests");
    std::fs::create_dir_all(&dir).expect("dir");
    let spec = dir.join("baked.gcad");
    std::fs::write(&spec, BAKED).expect("writes");
    let (a, b) = (dir.join("baked-a.glb"), dir.join("baked-b.glb"));
    let gcad = std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
        .args(["export", spec.to_str().unwrap(), a.to_str().unwrap()])
        .output()
        .expect("runs gcad");
    assert!(
        gcad.status.success(),
        "{}",
        String::from_utf8_lossy(&gcad.stderr)
    );
    let glb = std::process::Command::new(gcad2glb_tool())
        .args([spec.to_str().unwrap(), b.to_str().unwrap()])
        .output()
        .expect("runs gcad2glb");
    assert!(
        glb.status.success(),
        "{}",
        String::from_utf8_lossy(&glb.stderr)
    );
    let (from_gcad, from_tool) = (std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    assert_eq!(from_gcad, from_tool);
    assert_eq!(from_gcad, gcad_mesh::to_glb(BAKED).expect("builds"));
    let text = String::from_utf8_lossy(&from_gcad);
    assert!(
        text.contains("MSFT_lod") && text.contains("main_LOD2") && text.contains("normalTexture")
    );
}

#[test]
fn export_height_flag_matches_gcad2glb_and_the_library() {
    let dir = std::env::temp_dir().join("gcad-tests");
    std::fs::create_dir_all(&dir).expect("dir");
    let spec = format!(
        "{}/examples/mesh/rigged_creature.gcad",
        env!("CARGO_MANIFEST_DIR")
    );
    let (a, b) = (dir.join("height-a.glb"), dir.join("height-b.glb"));
    let flags = ["--height", "1", "--centre"];
    let gcad = std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
        .args(["export", &spec, a.to_str().unwrap()])
        .args(flags)
        .output()
        .expect("runs gcad");
    assert!(
        gcad.status.success(),
        "{}",
        String::from_utf8_lossy(&gcad.stderr)
    );
    let glb = std::process::Command::new(gcad2glb_tool())
        .args(flags)
        .args([spec.as_str(), b.to_str().unwrap()])
        .output()
        .expect("runs gcad2glb");
    assert!(
        glb.status.success(),
        "{}",
        String::from_utf8_lossy(&glb.stderr)
    );
    let expected = gcad_mesh::gltf::fit(
        &gcad_mesh::to_glb(&example("rigged_creature")).expect("builds"),
        &gcad_mesh::gltf::Fit {
            height: Some(1.0),
            centre: true,
            ..Default::default()
        },
    )
    .expect("fits");
    assert!(std::fs::read(&a).unwrap() == expected);
    assert!(std::fs::read(&b).unwrap() == expected);
}

const COIN: &str = "let r=50 t=12\nplane XZ offset=-t/2\ncircle 2*r at=0,r\ncoin: extrude t\ncolor #f2a900\nbody face\nplane XZ offset=-t/2-1.5\ncircle 2*r-16 at=0,r\nface: extrude t+3\ncolor #ffc21a\n";

#[test]
fn flat_solids_match_export_through_the_tool_and_the_library() {
    let dir = std::env::temp_dir().join("gcad-tests");
    std::fs::create_dir_all(&dir).expect("dir");
    let spec = dir.join("flat-coin.gcad");
    std::fs::write(&spec, COIN).expect("writes");
    let (a, b) = (dir.join("flat-coin-a.glb"), dir.join("flat-coin-b.glb"));
    let flags = ["--height", "1", "--centre"];
    let gcad = std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
        .args(["export", spec.to_str().unwrap(), a.to_str().unwrap()])
        .args(flags)
        .output()
        .expect("runs gcad");
    assert!(
        gcad.status.success(),
        "{}",
        String::from_utf8_lossy(&gcad.stderr)
    );
    let tool = std::process::Command::new(gcad2glb_tool())
        .args(flags)
        .args([spec.to_str().unwrap(), b.to_str().unwrap()])
        .output()
        .expect("runs gcad2glb");
    assert!(
        tool.status.success(),
        "{}",
        String::from_utf8_lossy(&tool.stderr)
    );
    let options = gcad2glb::Options {
        fit: gcad2glb::Fit {
            height: Some(1.0),
            centre: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let from_text = gcad2glb::Assets::from_source(COIN, &options)
        .and_then(|a| a.to_glb())
        .expect("builds");
    let exported = std::fs::read(&a).unwrap();
    assert_eq!(exported.len(), std::fs::read(&b).unwrap().len());
    assert!(exported == std::fs::read(&b).unwrap(), "tool differs");
    assert!(exported == from_text, "library differs");
}

#[test]
fn the_library_matches_export_with_set_and_scale_on_a_solid_part() {
    let dir = std::env::temp_dir().join("gcad-tests");
    std::fs::create_dir_all(&dir).expect("dir");
    let spec = dir.join("set-plate.gcad");
    std::fs::write(
        &spec,
        "let w=60\nrect w 30\nplate: extrude 4\nplane plate.end\nhole 6 -20,0 20,0\n",
    )
    .expect("writes");
    let out = dir.join("set-plate.glb");
    let gcad = std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
        .args(["export", spec.to_str().unwrap(), out.to_str().unwrap()])
        .args(["--set", "w=90", "--scale", "0.01", "--centre"])
        .output()
        .expect("runs gcad");
    assert!(
        gcad.status.success(),
        "{}",
        String::from_utf8_lossy(&gcad.stderr)
    );
    let options = gcad2glb::Options {
        fit: gcad2glb::Fit {
            scale: Some(0.01),
            centre: true,
            ..Default::default()
        },
        ..gcad2glb::Options::default().set("w", 90.0)
    };
    let assets = gcad2glb::Assets::from_path(&spec, &options).expect("builds");
    let (lo, hi) = assets.bounds().unwrap();
    assert!((hi[0] - lo[0] - 90.0).abs() < 1e-3);
    assert!(std::fs::read(&out).unwrap() == assets.to_glb().expect("writes"));
}

#[test]
fn export_scale_flag_fits_a_solid_part() {
    let dir = std::env::temp_dir().join("gcad-tests");
    std::fs::create_dir_all(&dir).expect("dir");
    let spec = dir.join("coin.gcad");
    std::fs::write(&spec, "circle 100\ncoin: extrude 12\n").expect("writes");
    let out = dir.join("coin-scaled.glb");
    let gcad = std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
        .args(["export", spec.to_str().unwrap(), out.to_str().unwrap()])
        .args(["--scale", "0.01"])
        .output()
        .expect("runs gcad");
    assert!(
        gcad.status.success(),
        "{}",
        String::from_utf8_lossy(&gcad.stderr)
    );
    let bytes = std::fs::read(&out).unwrap();
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&bytes[20..20 + n]).unwrap();
    let root = &doc["nodes"][doc["scenes"][0]["nodes"][0].as_u64().unwrap() as usize];
    assert_eq!(root["name"], "model");
    assert_eq!(root["scale"][0].as_f64(), Some(0.01));
    let refused = std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
        .args(["export", spec.to_str().unwrap(), "/tmp/gcad-tests/coin.stl"])
        .args(["--height", "1"])
        .output()
        .expect("runs gcad");
    assert!(!refused.status.success());
}
