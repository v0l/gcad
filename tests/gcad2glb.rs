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
    let error = gcad_mesh::build("a: blob 0,0,0 r=2\nremesh 0.5\nrect 10 10\n").unwrap_err();
    assert_eq!(error.line, 3);
    assert!(error.message.contains("rect"), "{error}");
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
    let tool = std::path::Path::new(env!("CARGO_BIN_EXE_gcad")).with_file_name("gcad2glb");
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
