use std::path::PathBuf;
use std::process::Command;

#[test]
fn c_program_builds_and_reads_a_model() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let mut deps = std::env::current_exe().expect("exe");
    deps.pop();
    let profile = deps.parent().expect("profile dir").to_path_buf();
    let release = profile.file_name().is_some_and(|n| n == "release");
    let target = profile.parent().expect("target dir").join("c-abi");
    let lib_dir = target.join(if release { "release" } else { "debug" });
    let static_lib = lib_dir.join("libgcad_mesh.a");
    let mut build = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    build.args(["build", "-p", "gcad-mesh", "--lib", "--target-dir"]);
    build.arg(&target);
    if release {
        build.arg("--release");
    }
    let built = build.current_dir(&root).output().expect("cargo build");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
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
