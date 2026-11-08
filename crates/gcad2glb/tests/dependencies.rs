use std::process::Command;

fn cargo() -> Command {
    let mut cargo = Command::new(env!("CARGO"));
    cargo.current_dir(env!("CARGO_MANIFEST_DIR"));
    cargo
}

fn tree(features: &[&str]) -> String {
    let out = cargo()
        .args(["tree", "-p", "gcad2glb", "-e", "normal", "--prefix", "none"])
        .args(features)
        .output()
        .expect("runs cargo tree");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn the_mesh_only_library_has_no_kernel_in_its_tree() {
    let light = tree(&["--no-default-features"]);
    for heavy in [
        "monstertruck",
        "gcad-brep",
        "image",
        "egui",
        "wgpu",
        "sims-",
    ] {
        assert!(!light.contains(heavy), "{heavy} in\n{light}");
    }
    assert!(light.contains("gcad-mesh"), "{light}");
    let full = tree(&[]);
    assert!(full.contains("monstertruck") && full.contains("gcad-brep"));
    for heavy in [
        "egui",
        "wgpu",
        "sims-fem",
        "sims-flow",
        "sims-field",
        "faer",
    ] {
        assert!(!full.contains(heavy), "{heavy} in\n{full}");
    }
}

#[test]
fn the_mesh_only_library_checks_for_wasm32() {
    let target =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gcad2glb-wasm");
    let out = cargo()
        .args([
            "check",
            "-p",
            "gcad2glb",
            "--lib",
            "--no-default-features",
            "--target",
            "wasm32-unknown-unknown",
            "--target-dir",
        ])
        .arg(&target)
        .output()
        .expect("runs cargo check");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
