use anyhow::{Result, anyhow};
use monstertruck::modeling::*;
use monstertruck::topology::compress::{
    CompressedEdge, CompressedFace, CompressedShell, CompressedSolid,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct PackedSolid(Vec<PackedShell>);

#[derive(Clone, Serialize, Deserialize)]
struct PackedShell {
    vertices: Vec<Point3>,
    edges: Vec<CompressedEdge<Curve>>,
    faces: Vec<CompressedFace<Surface>>,
}

impl PackedSolid {
    pub fn of(solid: &Solid) -> PackedSolid {
        PackedSolid(
            solid
                .compress()
                .boundaries
                .into_iter()
                .map(|shell| PackedShell {
                    vertices: shell.vertices,
                    edges: shell.edges,
                    faces: shell.faces,
                })
                .collect(),
        )
    }

    pub fn solid(self) -> Result<Solid> {
        Solid::extract(CompressedSolid {
            boundaries: self
                .0
                .into_iter()
                .map(|shell| CompressedShell {
                    vertices: shell.vertices,
                    edges: shell.edges,
                    faces: shell.faces,
                    vertex_stable_ids: None,
                    edge_stable_ids: None,
                    face_stable_ids: None,
                })
                .collect(),
            id_allocator: None,
            attributes: None,
        })
        .map_err(|e| anyhow!("{e}"))
    }
}
