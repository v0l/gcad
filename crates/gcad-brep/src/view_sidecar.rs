pub fn sidecar(file: &std::path::Path) -> std::path::PathBuf {
    let mut name = file.as_os_str().to_os_string();
    name.push(".view");
    name.into()
}

pub fn view_is_stale(file: &std::path::Path) -> bool {
    let changed = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    matches!((changed(file), changed(&sidecar(file))), (Some(f), Some(v)) if f > v)
}

pub fn recorded_pixel(file: &std::path::Path) -> Option<f64> {
    let text = crate::host::read_to_string(&sidecar(file)).ok()?;
    let pixels: f64 = text
        .split_whitespace()
        .find_map(|w| w.strip_prefix("pixels="))?
        .parse()
        .ok()?;
    (pixels > 0.0 && pixels.is_finite()).then(|| 1.0 / pixels)
}
