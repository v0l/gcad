use std::fmt::Write;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        } else if !path.to_string_lossy().ends_with(".result.json") {
            found.push(path);
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let examples = root.join("examples");
    println!("cargo:rerun-if-changed={}", examples.display());
    let mut found = Vec::new();
    walk(&examples, &mut found);
    found.sort();
    let mut out = String::from("pub const FILES: &[(&str, &[u8])] = &[\n");
    for path in &found {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        writeln!(
            out,
            "    ({name:?}, include_bytes!({:?})),",
            path.display().to_string()
        )
        .unwrap();
    }
    out.push_str("];\n");
    let target = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("bundle.rs");
    std::fs::write(target, out).unwrap();
}
