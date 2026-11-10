use crate::Model;
use crate::geometry;
use anyhow::Result;
use monstertruck::modeling::*;
use std::sync::Arc;

pub fn glb_of(model: &Model) -> Result<Vec<u8>> {
    mesh_model(model, true).to_glb()
}

pub fn mesh_model(model: &Model, y_up: bool) -> gcad_mesh::Model {
    let rest = model.rest_meshes();
    let tints = model.mesh_tints();
    let shading = model.mesh_shading();
    let parts = model.named_parts();
    let size = parts
        .iter()
        .map(|(_, solid, _)| geometry::bounds(solid).diameter())
        .fold(0.0, f64::max);
    let tolerance = (size * GLB_TOLERANCE).max(1.0e-4);
    let mut bodies: Vec<gcad_mesh::Body> = parts
        .into_iter()
        .map(|(name, solid, colour)| {
            let mesh = faceted_mesh(solid, &name, tolerance);
            gcad_mesh::Body::new(
                gcad_mesh::RawBody {
                    name,
                    mesh: Arc::new(mesh),
                    rig: None,
                    tints: vec![colour.unwrap_or([0.7, 0.7, 0.7])],
                    gloss: vec![Default::default()],
                    colours: Vec::new(),
                    detail: None,
                },
                y_up,
            )
        })
        .collect();
    bodies.extend(rest.into_iter().map(|(name, mesh)| {
        let colours = tints
            .iter()
            .find(|(n, ..)| *n == name)
            .map(|(_, _, t)| t.clone())
            .unwrap_or_default();
        let vertex_colours = shading
            .iter()
            .find(|(n, ..)| *n == name)
            .map(|(_, _, c, _)| c.iter().map(|c| c.map(|v| v as f32)).collect())
            .unwrap_or_default();
        gcad_mesh::Body::new(
            gcad_mesh::RawBody {
                rig: model.rig_of(&name).cloned(),
                gloss: model.part_shine_of(&name, &mesh),
                detail: model.scene.detail_of(&name),
                name,
                mesh,
                tints: colours,
                colours: vertex_colours,
            },
            y_up,
        )
    }));
    gcad_mesh::Model { bodies }
}

pub(crate) const GLB_TOLERANCE: f64 = 3e-3;

fn faceted_mesh(solid: &Solid, name: &str, tolerance: f64) -> crate::mesh::Mesh {
    faceted(&geometry::mesh(solid, tolerance), name)
}

pub(crate) fn faceted(
    polygons: &monstertruck::meshing::prelude::PolygonMesh,
    name: &str,
) -> crate::mesh::Mesh {
    let positions = polygons.positions();
    let corners: Vec<[f64; 3]> = polygons
        .faces()
        .triangle_iter()
        .flat_map(|t| t.map(|v| positions[v.pos]))
        .map(|p| [p.x, p.y, p.z])
        .collect();
    crate::mesh::Mesh {
        labels: vec![0; corners.len()],
        triangles: (0..corners.len() as u32 / 3)
            .map(|t| [3 * t, 3 * t + 1, 3 * t + 2])
            .collect(),
        positions: corners,
        parts: vec![name.to_string()],
        ..Default::default()
    }
}
