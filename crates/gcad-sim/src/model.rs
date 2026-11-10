pub use gcad_brep::*;

use anyhow::Result;
use std::path::Path;

pub fn run_path(path: &Path, vars: &[(String, f64)], until: Option<usize>) -> Result<Run> {
    crate::install();
    gcad_brep::run_path(path, vars, until)
}

pub fn run_source(path: &Path, source: &str, vars: &[(String, f64)]) -> Result<Run> {
    crate::install();
    gcad_brep::run_source(path, source, vars)
}

pub fn snapshots_path(
    path: &Path,
    vars: &[(String, f64)],
) -> Result<(Vec<gcad_parse::Line>, Vec<Snapshot>)> {
    crate::install();
    gcad_brep::snapshots_path(path, vars)
}

pub fn snapshots_path_kept(path: &Path, vars: &[(String, f64)]) -> Result<KeptRun> {
    crate::install();
    gcad_brep::snapshots_path_kept(path, vars)
}

#[cfg(feature = "game-assets")]
pub fn run_path_recorded(path: &Path, vars: &[(String, f64)]) -> Result<(Run, crate::host::Reads)> {
    crate::install();
    gcad_brep::run_path_recorded(path, vars)
}
