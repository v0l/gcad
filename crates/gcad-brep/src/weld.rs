use crate::geometry;
use crate::select;
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

struct Flat {
    origin: Point3,
    x: Vector3,
    y: Vector3,
    normal: Vector3,
}

impl Flat {
    fn new(origin: Point3, normal: Vector3) -> Flat {
        let normal = normal.normalize();
        let seed = if normal.x.abs() < 0.9 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        };
        let x = (seed - normal * seed.dot(normal)).normalize();
        Flat {
            origin,
            x,
            y: normal.cross(x),
            normal,
        }
    }

    fn uv(&self, p: Point3) -> (f64, f64) {
        let d = p - self.origin;
        (d.dot(self.x), d.dot(self.y))
    }

    fn height(&self, p: Point3) -> f64 {
        (p - self.origin).dot(self.normal)
    }
}

fn polygon(wire: &Wire, flat: &Flat) -> Vec<(f64, f64)> {
    wire.edge_iter()
        .flat_map(|edge| {
            let curve = edge.oriented_curve();
            let (t0, t1) = curve.range_tuple();
            (0..24)
                .map(move |i| curve.subs(t0 + (t1 - t0) * i as f64 / 24.0))
                .collect::<Vec<_>>()
        })
        .map(|p| flat.uv(p))
        .collect()
}

fn inside(polygon: &[(f64, f64)], (u, v): (f64, f64)) -> bool {
    let mut crossings = false;
    let n = polygon.len();
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        if (a.1 > v) != (b.1 > v) && u < a.0 + (v - a.1) * (b.0 - a.0) / (b.1 - a.1) {
            crossings = !crossings;
        }
    }
    crossings
}

fn all_inside(points: &[(f64, f64)], polygon: &[(f64, f64)]) -> bool {
    points.iter().all(|&p| inside(polygon, p))
}

fn none_inside(points: &[(f64, f64)], polygon: &[(f64, f64)]) -> bool {
    points.iter().all(|&p| !inside(polygon, p))
}

fn face_points(face: &Face) -> Vec<Point3> {
    let mut points: Vec<Point3> = face
        .boundaries()
        .iter()
        .flat_map(|wire| wire.edge_iter().collect::<Vec<_>>())
        .flat_map(|edge| geometry::curve_samples(&edge.oriented_curve()))
        .collect();
    match face.oriented_surface() {
        Surface::Plane(_) => {}
        Surface::BsplineSurface(surface) => {
            points.extend(surface.control_points().iter().flatten().copied())
        }
        Surface::NurbsSurface(surface) => points.extend(
            surface
                .control_points()
                .iter()
                .flatten()
                .map(|p| Point3::from_homogeneous(*p)),
        ),
        _ => {
            let bounds: BoundingBox<Point3> = points.iter().collect();
            let reach = bounds.diameter();
            let centre = bounds.center();
            points.extend(
                [-1.0, 1.0]
                    .iter()
                    .flat_map(|&s| {
                        [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()]
                            .map(|axis| centre + axis * s * reach)
                    })
                    .collect::<Vec<_>>(),
            );
        }
    }
    points
}

fn planar(face: &Face) -> Option<Plane> {
    match face.oriented_surface() {
        Surface::Plane(plane) => Some(plane),
        _ => None,
    }
}

fn patch(
    solid: &Solid,
    tool_faces: &[Face],
    start: usize,
    normal: Vector3,
    tolerance: f64,
) -> Option<Solid> {
    let [shell] = solid.boundaries().as_slice() else {
        return None;
    };
    let start_face = &tool_faces[start];
    let flat = Flat::new(planar(start_face)?.origin(), normal);
    let start_loops = start_face.boundaries();
    let tool_outer = polygon(&start_loops[0], &flat);
    let tool_holes: Vec<Vec<(f64, f64)>> = start_loops[1..]
        .iter()
        .map(|wire| polygon(wire, &flat))
        .collect();

    let faces: Vec<Face> = shell.face_iter().cloned().collect();
    let target = faces.iter().position(|face| {
        planar(face).is_some_and(|plane| {
            plane.normal().dot(normal) > 1.0 - 1.0e-9
                && flat.height(plane.origin()).abs() < tolerance * 10.0
                && {
                    let loops = face.boundaries();
                    let outer = polygon(&loops[0], &flat);
                    all_inside(&tool_outer, &outer)
                        && none_inside(&outer, &tool_outer)
                        && loops[1..].iter().all(|hole| {
                            let hole = polygon(hole, &flat);
                            none_inside(&tool_outer, &hole)
                        })
                }
        })
    })?;
    let existing = &faces[target];
    let existing_loops = existing.boundaries();

    let mut kept_holes = Vec::new();
    let mut moved: Vec<Vec<Wire>> = vec![Vec::new(); tool_holes.len()];
    for hole in &existing_loops[1..] {
        let points = polygon(hole, &flat);
        if none_inside(&points, &tool_outer) {
            kept_holes.push(hole.clone());
            continue;
        }
        let home = tool_holes
            .iter()
            .position(|tool_hole| all_inside(&points, tool_hole))?;
        if tool_holes
            .iter()
            .any(|tool_hole| !all_inside(&points, tool_hole) && !none_inside(&points, tool_hole))
        {
            return None;
        }
        moved[home].push(hole.clone());
    }

    let reach = tool_faces
        .iter()
        .flat_map(face_points)
        .map(|p| flat.height(p))
        .fold(0.0, f64::max);
    let footprint: BoundingBox<Point3> = tool_outer
        .iter()
        .map(|&(u, v)| Point3::new(u, v, 0.0))
        .collect();
    let rises = faces.iter().enumerate().any(|(i, face)| {
        if i == target {
            return false;
        }
        let points = face_points(face);
        let below = points.iter().all(|p| flat.height(*p) <= tolerance * 10.0);
        let above = points.iter().all(|p| flat.height(*p) >= reach);
        if below || above {
            return false;
        }
        let spread: BoundingBox<Point3> = points
            .iter()
            .map(|p| {
                let (u, v) = flat.uv(*p);
                Point3::new(u, v, 0.0)
            })
            .collect();
        (0..2)
            .all(|k| spread.min()[k] <= footprint.max()[k] && footprint.min()[k] <= spread.max()[k])
    });
    if rises {
        return None;
    }

    let surface = existing.oriented_surface();
    let mut wires = vec![existing_loops[0].clone()];
    wires.extend(kept_holes);
    wires.push(start_loops[0].clone());
    let mut result: Vec<Face> = faces
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != target)
        .map(|(_, face)| face.clone())
        .collect();
    result.push(Face::try_new(wires, surface.clone()).ok()?);
    for (hole, inner) in start_loops[1..].iter().zip(moved) {
        let mut wires = vec![hole.clone()];
        wires.extend(inner);
        result.push(Face::try_new(wires, surface.clone()).ok()?);
    }
    result.extend(
        tool_faces
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != start)
            .map(|(_, face)| face.clone()),
    );
    Solid::try_new(vec![result.into()]).ok()
}

fn weld_patch(solid: &Solid, tool: &Solid) -> Option<Solid> {
    let tolerance = geometry::bounds(solid)
        .diameter()
        .max(geometry::bounds(tool).diameter())
        * 1.0e-7;
    let tool_faces = select::faces(tool);
    tool_faces
        .iter()
        .enumerate()
        .filter_map(|(i, face)| planar(face).map(|plane| (i, -plane.normal())))
        .find_map(|(i, normal)| patch(solid, &tool_faces, i, normal, tolerance))
}

pub(crate) fn weld(a: &Solid, b: &Solid) -> Option<Solid> {
    weld_union(a, b)
        .or_else(|| weld_patch(a, b))
        .or_else(|| weld_patch(b, a))
}
