fn git(dir: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

fn main() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let code = [
        "log",
        "-1",
        "--format=%h",
        "--abbrev=12",
        "--",
        "src",
        "crates",
        "sims-proto",
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
    ];
    let full = match git(root, &code) {
        Some(commit) => format!("{version}+{commit}"),
        None => version,
    };
    println!("cargo:rustc-env=GCAD_BUILD_VERSION={full}");
    if let Some(dir) = git(root, &["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={dir}/HEAD");
    }
    if let Some(common) = git(root, &["rev-parse", "--path-format=absolute", "--git-common-dir"]) {
        println!("cargo:rerun-if-changed={common}/packed-refs");
        if let Some(branch) = git(root, &["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={common}/{branch}");
        }
    }
}
