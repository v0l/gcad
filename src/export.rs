use crate::geometry;
use anyhow::{Result, bail};
use monstertruck::mesh::stl::{StlType, write};
use monstertruck::modeling::*;
use monstertruck::step::save::{CompleteStepDisplay, StepHeaderDescriptor, StepModels};

pub fn export(solids: &[&Solid], path: &str) -> Result<()> {
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
            let compressed: Vec<_> = solids.iter().map(|solid| solid.compress()).collect();
            let step = CompleteStepDisplay::new(
                compressed.iter().collect::<StepModels<'_, _, _, _>>(),
                header,
            )
            .to_string();
            std::fs::write(path, step)?;
        }
        "stl" => {
            let mut mesh = monstertruck::mesh::PolygonMesh::default();
            solids.iter().for_each(|solid| {
                mesh.merge(geometry::mesh(
                    solid,
                    (geometry::bounds(solid).diameter() * 2.0e-4).max(0.01),
                ))
            });
            write(&mesh, &mut std::fs::File::create(path)?, StlType::Binary)?;
        }
        "obj" => {
            let mut mesh = monstertruck::mesh::PolygonMesh::default();
            solids.iter().for_each(|solid| {
                mesh.merge(geometry::mesh(
                    solid,
                    (geometry::bounds(solid).diameter() * 2.0e-4).max(0.01),
                ))
            });
            monstertruck::mesh::obj::write(&mesh, std::fs::File::create(path)?)?;
        }
        other => bail!("cannot export `.{other}`, use .step, .stl or .obj"),
    }
    Ok(())
}
