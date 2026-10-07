use crate::geometry;
use monstertruck::modeling::*;
use monstertruck::topology::compress::{
    CompressedEdge, CompressedEdgeIndex, CompressedFace, CompressedShell, CompressedSolid,
};

fn midpoint(curve: &Curve) -> Point3 {
    let (t0, t1) = curve.range_tuple();
    curve.subs((t0 + t1) / 2.0)
}

fn edge_set(face: &CompressedFace<Surface>) -> Vec<usize> {
    let mut indices: Vec<usize> = face.boundaries.iter().flatten().map(|e| e.index).collect();
    indices.sort_unstable();
    indices
}

pub(crate) fn weld_union(a: &Solid, b: &Solid) -> Option<Solid> {
    let tolerance = geometry::bounds(a)
        .diameter()
        .max(geometry::bounds(b).diameter())
        * 1.0e-7;
    let (ca, cb) = (a.compress(), b.compress());
    let ([sa], [sb]) = (ca.boundaries.as_slice(), cb.boundaries.as_slice()) else {
        return None;
    };
    let mut vertices = sa.vertices.clone();
    let vertex_map: Vec<usize> = sb
        .vertices
        .iter()
        .map(|p| {
            vertices
                .iter()
                .position(|q| q.distance(*p) < tolerance)
                .unwrap_or_else(|| {
                    vertices.push(*p);
                    vertices.len() - 1
                })
        })
        .collect();
    let mut edges = sa.edges.clone();
    let edge_map: Vec<(usize, bool)> = sb
        .edges
        .iter()
        .map(|edge| {
            let (front, back) = (vertex_map[edge.vertices.0], vertex_map[edge.vertices.1]);
            let middle = midpoint(&edge.curve);
            edges
                .iter()
                .position(|known| {
                    (known.vertices == (front, back) || known.vertices == (back, front))
                        && midpoint(&known.curve).distance(middle) < tolerance
                })
                .map(|i| (i, edges[i].vertices == (front, back)))
                .unwrap_or_else(|| {
                    edges.push(CompressedEdge {
                        vertices: (front, back),
                        curve: edge.curve.clone(),
                    });
                    (edges.len() - 1, true)
                })
        })
        .collect();
    let faces_b: Vec<CompressedFace<Surface>> = sb
        .faces
        .iter()
        .map(|face| CompressedFace {
            boundaries: face
                .boundaries
                .iter()
                .map(|wire| {
                    wire.iter()
                        .map(|use_| {
                            let (index, same) = edge_map[use_.index];
                            CompressedEdgeIndex {
                                index,
                                orientation: use_.orientation == same,
                            }
                        })
                        .collect()
                })
                .collect(),
            orientation: face.orientation,
            surface: face.surface.clone(),
        })
        .collect();
    let keys_b: Vec<Vec<usize>> = faces_b.iter().map(edge_set).collect();
    let mut dropped_b = vec![false; faces_b.len()];
    let kept_a: Vec<CompressedFace<Surface>> = sa
        .faces
        .iter()
        .filter(|face| {
            let key = edge_set(face);
            match keys_b
                .iter()
                .enumerate()
                .position(|(j, other)| !dropped_b[j] && *other == key)
            {
                Some(j) => {
                    dropped_b[j] = true;
                    false
                }
                None => true,
            }
        })
        .cloned()
        .collect();
    if !dropped_b.iter().any(|d| *d) {
        return None;
    }
    let faces = kept_a
        .into_iter()
        .chain(
            faces_b
                .into_iter()
                .zip(dropped_b)
                .filter(|(_, d)| !d)
                .map(|(f, _)| f),
        )
        .collect();
    let shell = CompressedShell {
        vertices,
        edges,
        faces,
        vertex_stable_ids: None,
        edge_stable_ids: None,
        face_stable_ids: None,
    };
    let solid = Solid::extract(CompressedSolid {
        boundaries: vec![shell],
        id_allocator: None,
        attributes: None,
    })
    .ok()?;
    let shell = &solid.boundaries()[0];
    (shell.shell_condition() == monstertruck::topology::shell::ShellCondition::Closed)
        .then_some(solid)
}
