use crate::model::stored::{dir, file_of, read_stored, write_stored};
use crate::packed::PackedSolid;
use monstertruck::modeling::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

type NodeUvs = (u32, Vec<(usize, [f64; 2])>);

#[derive(Serialize, Deserialize)]
struct StoredMesh {
    key: String,
    nodes: Vec<[f64; 3]>,
    tets: Vec<[u32; 4]>,
    boundary: Vec<([u32; 3], usize)>,
    faces: usize,
    surfaces: Vec<Surface>,
    uvs: Vec<NodeUvs>,
    used: f64,
}

fn mesh_dir() -> Option<PathBuf> {
    Some(dir()?.with_file_name("meshes"))
}

fn mesh_key(solid: &Solid, size: f64) -> Option<String> {
    use sha2::Digest;
    let bytes = bitcode::serialize(&PackedSolid::of(solid)).ok()?;
    let digest = sha2::Sha256::digest(&bytes);
    let shape: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let exe = std::env::current_exe()
        .and_then(std::fs::metadata)
        .ok()
        .map(|meta| (meta.len(), meta.modified().ok()));
    Some(format!(
        "{}|{exe:?}|{size:?}|{shape}",
        env!("GCAD_BUILD_VERSION")
    ))
}

pub fn fetch_mesh(solid: &Solid, size: f64) -> Option<(super::tet::TetMesh, f64)> {
    let dir = mesh_dir()?;
    let key = mesh_key(solid, size)?;
    let stored: StoredMesh = read_stored(&file_of(&dir, &key))?;
    if stored.key != key {
        return None;
    }
    let mesh = super::tet::TetMesh {
        nodes: stored.nodes.into_iter().map(Point3::from).collect(),
        tets: stored.tets,
        boundary: stored.boundary,
        faces: stored.faces,
        surfaces: stored.surfaces.into_iter().map(super::tet::Exact).collect(),
        uvs: stored
            .uvs
            .into_iter()
            .map(|(node, uvs)| {
                (
                    node,
                    uvs.into_iter().map(|(f, uv)| (f, uv.into())).collect(),
                )
            })
            .collect(),
    };
    Some((mesh, stored.used))
}

pub fn put_mesh(solid: &Solid, size: f64, (mesh, used): &(super::tet::TetMesh, f64)) {
    let Some(dir) = mesh_dir() else { return };
    let Some(key) = mesh_key(solid, size) else {
        return;
    };
    let stored = StoredMesh {
        key: key.clone(),
        nodes: mesh.nodes.iter().map(|&p| p.into()).collect(),
        tets: mesh.tets.clone(),
        boundary: mesh.boundary.clone(),
        faces: mesh.faces,
        surfaces: mesh.surfaces.iter().map(|s| s.0.clone()).collect(),
        uvs: mesh
            .uvs
            .iter()
            .map(|(&node, uvs)| (node, uvs.iter().map(|&(f, uv)| (f, uv.into())).collect()))
            .collect(),
        used: *used,
    };
    write_stored(&dir, &file_of(&dir, &key), &stored);
}
