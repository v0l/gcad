use crate::geometry;
use monstertruck::modeling::*;
use monstertruck::topology::compress::{
    CompressedEdge, CompressedEdgeIndex, CompressedFace, CompressedShell,
};
use monstertruck::topology::shell::ShellCondition;
use std::collections::HashMap;

type Use = CompressedEdgeIndex;

fn flipped(wire: &[Use]) -> Vec<Use> {
    wire.iter()
        .rev()
        .map(|u| Use {
            index: u.index,
            orientation: !u.orientation,
        })
        .collect()
}

fn outward_loops(face: &CompressedFace<Surface>) -> Vec<Vec<Use>> {
    face.boundaries
        .iter()
        .map(|wire| {
            if face.orientation {
                wire.clone()
            } else {
                flipped(wire)
            }
        })
        .collect()
}

fn plane_of(face: &CompressedFace<Surface>) -> Option<(Vector3, f64)> {
    let Surface::Plane(plane) = &face.surface else {
        return None;
    };
    let normal = if face.orientation {
        plane.normal()
    } else {
        -plane.normal()
    };
    Some((normal, plane.origin().to_vec().dot(normal)))
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn ends(edges: &[CompressedEdge<Curve>], u: &Use) -> (usize, usize) {
    let (a, b) = edges[u.index].vertices;
    if u.orientation { (a, b) } else { (b, a) }
}

fn chain(edges: &[CompressedEdge<Curve>], uses: Vec<Use>) -> Option<Vec<Vec<Use>>> {
    let mut from: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, u) in uses.iter().enumerate() {
        from.entry(ends(edges, u).0).or_default().push(i);
    }
    let mut taken = vec![false; uses.len()];
    let mut loops = Vec::new();
    while let Some(first) = taken.iter().position(|t| !t) {
        taken[first] = true;
        let start = ends(edges, &uses[first]).0;
        let mut wire = vec![uses[first]];
        let mut at = ends(edges, &uses[first]).1;
        while at != start {
            let next = from.get(&at)?.iter().copied().find(|&i| !taken[i])?;
            taken[next] = true;
            wire.push(uses[next]);
            at = ends(edges, &uses[next]).1;
        }
        loops.push(wire);
    }
    Some(loops)
}

fn fused(
    edges: &[CompressedEdge<Curve>],
    faces: &[&CompressedFace<Surface>],
) -> Option<CompressedFace<Surface>> {
    let uses: Vec<Use> = faces
        .iter()
        .flat_map(|f| outward_loops(f))
        .flatten()
        .collect();
    let mut count: HashMap<usize, usize> = HashMap::new();
    uses.iter()
        .for_each(|u| *count.entry(u.index).or_default() += 1);
    let outline: Vec<Use> = uses.into_iter().filter(|u| count[&u.index] == 1).collect();
    let loops = chain(edges, outline)?;
    let keep = faces[0];
    Some(CompressedFace {
        boundaries: loops
            .iter()
            .map(|wire| {
                if keep.orientation {
                    wire.clone()
                } else {
                    flipped(wire)
                }
            })
            .collect(),
        orientation: keep.orientation,
        surface: keep.surface.clone(),
    })
}

fn fuse_shell(
    shell: &CompressedShell<Point3, Curve, Surface>,
    tolerance: f64,
) -> Option<Vec<CompressedFace<Surface>>> {
    let planes: Vec<Option<(Vector3, f64)>> = shell.faces.iter().map(plane_of).collect();
    let mut users: HashMap<usize, Vec<usize>> = HashMap::new();
    for (f, face) in shell.faces.iter().enumerate() {
        face.boundaries
            .iter()
            .flatten()
            .for_each(|u| users.entry(u.index).or_default().push(f));
    }
    let mut parent: Vec<usize> = (0..shell.faces.len()).collect();
    let mut joined = false;
    for pair in users.values() {
        let [f, g] = pair.as_slice() else { continue };
        if f == g {
            continue;
        }
        let (Some((n, d)), Some((m, e))) = (planes[*f], planes[*g]) else {
            continue;
        };
        if n.dot(m) > 1.0 - 1.0e-9 && (d - e).abs() < tolerance {
            let (a, b) = (root(&mut parent, *f), root(&mut parent, *g));
            if a != b {
                parent[a] = b;
                joined = true;
            }
        }
    }
    if !joined {
        return None;
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for f in 0..shell.faces.len() {
        groups.entry(root(&mut parent, f)).or_default().push(f);
    }
    let mut order: Vec<Vec<usize>> = groups.into_values().collect();
    order.sort_unstable_by_key(|group| group[0]);
    Some(
        order
            .into_iter()
            .flat_map(|group| {
                let faces: Vec<&CompressedFace<Surface>> =
                    group.iter().map(|&f| &shell.faces[f]).collect();
                match (group.len(), fused(&shell.edges, &faces)) {
                    (1, _) | (_, None) => faces.into_iter().cloned().collect::<Vec<_>>(),
                    (_, Some(face)) => vec![face],
                }
            })
            .collect(),
    )
}

fn straight(edge: &CompressedEdge<Curve>) -> bool {
    matches!(edge.curve, Curve::Line(_))
}

fn through(
    shell: &CompressedShell<Point3, Curve, Surface>,
    used: &[bool],
) -> Option<(usize, usize, usize)> {
    let mut touching: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, edge) in shell.edges.iter().enumerate() {
        if used[index] {
            touching.entry(edge.vertices.0).or_default().push(index);
            touching.entry(edge.vertices.1).or_default().push(index);
        }
    }
    let other = |index: usize, v: usize| {
        let (a, b) = shell.edges[index].vertices;
        if a == v { b } else { a }
    };
    let mut candidates: Vec<(&usize, &Vec<usize>)> = touching.iter().collect();
    candidates.sort_unstable_by_key(|(v, _)| **v);
    candidates.into_iter().find_map(|(&v, near)| {
        let [e, f] = near.as_slice() else { return None };
        if e == f || !straight(&shell.edges[*e]) || !straight(&shell.edges[*f]) {
            return None;
        }
        let (a, b) = (other(*e, v), other(*f, v));
        if a == b {
            return None;
        }
        let p = shell.vertices[v];
        let (da, db) = (shell.vertices[a] - p, shell.vertices[b] - p);
        let level = da.cross(db).magnitude() <= 1.0e-9 * da.magnitude() * db.magnitude();
        (level && da.dot(db) < 0.0).then_some((v, *e, *f))
    })
}

fn join_straight(shell: &mut CompressedShell<Point3, Curve, Surface>) -> bool {
    let mut changed = false;
    loop {
        let mut used = vec![false; shell.edges.len()];
        shell
            .faces
            .iter()
            .flat_map(|f| f.boundaries.iter().flatten())
            .for_each(|u| used[u.index] = true);
        let Some((v, e, f)) = through(shell, &used) else {
            return changed;
        };
        let merged = shell.edges.len();
        let (start, end) = {
            let (a, b) = shell.edges[e].vertices;
            let (c, d) = shell.edges[f].vertices;
            (if a == v { b } else { a }, if c == v { d } else { c })
        };
        shell.edges.push(CompressedEdge {
            vertices: (start, end),
            curve: Curve::Line(Line(shell.vertices[start], shell.vertices[end])),
        });
        let edges = shell.edges.clone();
        for face in &mut shell.faces {
            for wire in &mut face.boundaries {
                let Some(at) = wire.iter().position(|u| u.index == e || u.index == f) else {
                    continue;
                };
                let n = wire.len();
                let (first, second) =
                    if wire[(at + 1) % n].index == e || wire[(at + 1) % n].index == f {
                        (at, (at + 1) % n)
                    } else {
                        ((at + n - 1) % n, at)
                    };
                let from = ends(&edges, &wire[first]).0;
                let joined = Use {
                    index: merged,
                    orientation: from == start,
                };
                if second == 0 {
                    wire.remove(first);
                    wire[0] = joined;
                } else {
                    wire[first] = joined;
                    wire.remove(second);
                }
            }
        }
        shell.edge_stable_ids = None;
        changed = true;
    }
}

pub(crate) fn fuse_coplanar(solid: &Solid) -> Solid {
    let tolerance = geometry::bounds(solid).diameter() * 1.0e-7;
    let mut compressed = solid.compress();
    let mut changed = false;
    for shell in &mut compressed.boundaries {
        if let Some(faces) = fuse_shell(shell, tolerance) {
            shell.faces = faces;
            shell.face_stable_ids = None;
            changed = true;
            join_straight(shell);
        }
    }
    if !changed {
        return solid.clone();
    }
    Solid::extract(compressed)
        .ok()
        .filter(|fused| {
            fused
                .boundaries()
                .iter()
                .all(|shell| shell.shell_condition() == ShellCondition::Closed)
        })
        .unwrap_or_else(|| solid.clone())
}
