mod exact;
pub mod surface;

use anyhow::{Result, bail};
use monstertruck::modeling::Solid;

pub use exact::Exact;
pub use sims_tet::*;
pub use surface::surface_mesh;

pub type TetMesh = sims_tet::TetMesh<Exact>;

pub fn mesh_solid(solid: &Solid, size: f64) -> Result<TetMesh> {
    if size <= 0.0 {
        bail!("the mesh size must be above zero");
    }
    let surface = {
        let _stage = crate::progress::stage("meshing the surface", None, "");
        surface_mesh(solid, size)?
    };
    fill(surface, size)
}
