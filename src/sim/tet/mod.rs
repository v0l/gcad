mod exact;
pub mod surface;

use anyhow::{Result, bail};
use monstertruck::modeling::Solid;

pub use exact::Exact;
pub use sims_tet::*;
pub use surface::surface_mesh;

pub type TetMesh = sims_tet::TetMesh<Exact>;

pub fn mesh_solid(solid: &Solid, size: f64) -> Result<TetMesh> {
    mesh_solid_reporting(solid, size, &|_| {})
}

pub fn mesh_solid_reporting(solid: &Solid, size: f64, report: &dyn Fn(String)) -> Result<TetMesh> {
    if size <= 0.0 {
        bail!("the mesh size must be above zero");
    }
    report("meshing the surface".into());
    fill(surface_mesh(solid, size)?, size, report)
}
