use gcad_plot::result::stamp_of;
use std::path::{Path, PathBuf};

const FILE_KINDS: &[&str] = &[
    "gcad", "gasm", "gsim", "step", "stp", "svg", "dxf", "stl", "ttf", "otf", "toml", "xml",
    "fcmat",
];
const READ_INTO: &[&str] = &["gcad", "gasm", "gsim"];

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn inputs(spec: &Path) -> Vec<PathBuf> {
    let mut found = vec![spec.to_path_buf()];
    let mut index = 0;
    while index < found.len() {
        let file = found[index].clone();
        index += 1;
        if !READ_INTO.contains(&extension(&file).as_str()) {
            continue;
        }
        let Ok(text) = crate::host::read_to_string(&file) else {
            continue;
        };
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        for library in crate::model::materials::discover(Some(&dir)) {
            if !found.contains(&library) {
                found.push(library);
            }
        }
        for word in text.lines().flat_map(str::split_whitespace) {
            let word = word.rsplit('=').next().unwrap_or(word);
            let path = dir.join(word);
            if FILE_KINDS.contains(&extension(&path).as_str())
                && crate::host::is_file(&path)
                && !found.contains(&path)
            {
                found.push(path);
            }
        }
    }
    found
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub fn stamp(spec: &Path, vars: &[(String, f64)]) -> Vec<(String, String)> {
    let dir = spec.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut stamp: Vec<(String, String)> = inputs(spec)
        .into_iter()
        .map(|file| {
            let bytes = crate::host::read(&file).unwrap_or_default();
            let name = file.strip_prefix(&dir).unwrap_or(&file);
            (name.display().to_string(), format!("{:016x}", fnv(&bytes)))
        })
        .collect();
    if !vars.is_empty() {
        let set = format!("{vars:?}");
        stamp.push(("--set".to_string(), format!("{:016x}", fnv(set.as_bytes()))));
    }
    stamp.push(("gcad".to_string(), env!("CARGO_PKG_VERSION").to_string()));
    stamp
}

pub fn stale(spec: &Path, vars: &[(String, f64)]) -> Option<String> {
    let path = super::result_path(spec);
    let Ok(bytes) = crate::host::read(&path) else {
        return Some(format!("no result yet; run `gcad sim {}`", spec.display()));
    };
    let had = match stamp_of(&bytes) {
        Ok(stamp) => stamp,
        Err(why) => return Some(format!("{}: {why}", path.display())),
    };
    let now = stamp(spec, vars);
    let changed: Vec<String> = now
        .iter()
        .filter(|entry| !had.contains(entry))
        .map(|(file, _)| file.clone())
        .chain(
            had.iter()
                .filter(|(file, _)| !now.iter().any(|(f, _)| f == file))
                .map(|(file, _)| file.clone()),
        )
        .collect();
    (!changed.is_empty()).then(|| {
        format!(
            "{} is stale ({} changed); rerun `gcad sim {}`",
            path.display(),
            changed.join(", "),
            spec.display()
        )
    })
}
