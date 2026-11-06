use std::path::PathBuf;
use std::process::Command;

#[test]
fn c_program_builds_and_reads_a_model() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let mut deps = PathBuf::from(std::env::current_exe().expect("exe"));
    deps.pop();
    let lib_dir = deps.parent().expect("profile dir").to_path_buf();
    let static_lib = lib_dir.join("libgcad_mesh.a");
    assert!(static_lib.exists(), "{}", static_lib.display());
    let exe = out.join("c_abi_test");
    let compile = Command::new("cc")
        .arg(root.join("tests/c_abi.c"))
        .arg("-I")
        .arg(root.join("include"))
        .arg(&static_lib)
        .args(["-lm", "-lpthread", "-ldl", "-o"])
        .arg(&exe)
        .output()
        .expect("cc");
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let example = root.join("../../examples/mesh/rigged_creature.gcad");
    let run = Command::new(&exe).arg(example).output().expect("run");
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}
