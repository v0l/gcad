pub use gcad_drafting::drafting::*;

use anyhow::Result;
use std::path::Path;

pub fn run(path: &Path, vars: &[(String, f64)]) -> Result<Drawing> {
    crate::sim::install();
    gcad_drafting::drafting::run(path, vars)
}
