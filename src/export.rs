use crate::geometry;
use anyhow::{Result, bail};
use monstertruck::mesh::stl::{StlType, write};
use monstertruck::modeling::*;
use monstertruck::step::save::{CompleteStepDisplay, StepHeaderDescriptor, StepModel};

pub fn export(solid: &Solid, path: &str) -> Result<()> {
    let extension = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "step" | "stp" => {
            let header = StepHeaderDescriptor {
                organization_system: "linecad".to_string(),
                ..Default::default()
            };
            let step =
                CompleteStepDisplay::new(StepModel::from(&solid.compress()), header).to_string();
            std::fs::write(path, step)?;
        }
        "stl" => {
            let mesh = geometry::mesh(solid, geometry::mesh_tolerance(solid) * 0.25);
            write(&mesh, &mut std::fs::File::create(path)?, StlType::Binary)?;
        }
        other => bail!("cannot export `.{other}`, use .step or .stl"),
    }
    Ok(())
}
