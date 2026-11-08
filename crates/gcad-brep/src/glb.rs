use crate::Model;
use crate::geometry;
use anyhow::Result;
use gcad_mesh::gltf;
use monstertruck::modeling::*;

type Colour = [f64; 3];

pub fn glb_of(model: &Model) -> Result<Vec<u8>> {
    let rest = model.rest_meshes();
    let tints = model.mesh_tints();
    let parts = model.named_parts();
    let size = parts
        .iter()
        .map(|(_, solid, _)| geometry::bounds(solid).diameter())
        .fold(0.0, f64::max);
    let tolerance = (size * GLB_TOLERANCE).max(1.0e-4);
    let solids: Vec<(String, crate::mesh::Mesh, Colour)> = parts
        .into_iter()
        .map(|(name, solid, colour)| {
            (
                name.clone(),
                faceted_mesh(solid, &name, tolerance),
                colour.unwrap_or([0.7, 0.7, 0.7]),
            )
        })
        .collect();
    let mut bodies: Vec<gltf::Body<'_>> = solids
        .iter()
        .map(|(name, mesh, colour)| gltf::Body {
            name: name.clone(),
            mesh,
            rig: None,
            colours: vec![*colour],
            gloss: vec![0.0],
            detail: None,
        })
        .collect();
    bodies.extend(rest.iter().map(|(name, mesh)| {
        let colours = tints
            .iter()
            .find(|(n, ..)| n == name)
            .map(|(_, _, t)| t.clone())
            .unwrap_or_default();
        gltf::Body {
            name: name.clone(),
            mesh,
            rig: model.rig_of(name),
            colours,
            gloss: model.part_gloss_of(name, mesh),
            detail: model.scene.detail_of(name),
        }
    }));
    gltf::glb(&bodies)
}

const GLB_TOLERANCE: f64 = 3e-3;

fn faceted_mesh(solid: &Solid, name: &str, tolerance: f64) -> crate::mesh::Mesh {
    let polygons = geometry::mesh(solid, tolerance);
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
