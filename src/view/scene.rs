use crate::geometry;
use crate::model::Model;
use monstertruck::meshing::prelude::*;
use monstertruck::modeling::*;
use std::sync::atomic::{AtomicU64, Ordering};

pub type V3 = [f32; 3];

#[derive(Clone, Default)]
pub struct Surface {
    pub positions: Vec<V3>,
    pub normals: Vec<V3>,
    pub colour: V3,
    pub body: usize,
    pub face: Option<usize>,
}

#[derive(Clone, Default)]
pub struct Lines {
    pub segments: Vec<[V3; 2]>,
    pub colour: V3,
    pub body: usize,
    pub width: f32,
}

#[derive(Clone)]
pub struct Part {
    pub name: String,
    pub colour: V3,
    pub volume: f64,
    pub current: bool,
}

#[derive(Clone)]
pub struct FaceInfo {
    pub body: usize,
    pub kind: String,
    pub area: f64,
}

#[derive(Default)]
pub struct Scene {
    pub id: u64,
    pub surfaces: Vec<Surface>,
    pub lines: Vec<Lines>,
    pub centre: V3,
    pub radius: f32,
    pub parts: Vec<Part>,
    pub vertices: Vec<(usize, V3)>,
    pub faces: Vec<FaceInfo>,
}

pub struct Highlight<'a> {
    pub faces: &'a [usize],
    pub colour: V3,
}

pub const BODY: V3 = [0.62, 0.66, 0.72];
pub const EDGE: V3 = [0.05, 0.06, 0.07];
pub const EDGE_WIDTH: f32 = 1.5;
pub const MARKED_WIDTH: f32 = 3.5;

fn v3(p: Point3) -> V3 {
    [p.x as f32, p.y as f32, p.z as f32]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: V3) -> V3 {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2])
        .sqrt()
        .max(1.0e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn face_colour(index: usize, base: V3, highlights: &[Highlight<'_>]) -> V3 {
    highlights
        .iter()
        .rev()
        .find(|h| h.faces.contains(&index))
        .map_or(base, |h| h.colour)
}

fn describe(surface: &monstertruck::modeling::Surface) -> String {
    match surface {
        monstertruck::modeling::Surface::Plane(plane) => {
            let n = plane.normal();
            let tidy = |v: f64| if v.abs() < 5.0e-4 { 0.0 } else { v };
            format!(
                "flat, normal {:.3}, {:.3}, {:.3}",
                tidy(n.x),
                tidy(n.y),
                tidy(n.z)
            )
        }
        _ => "curved".to_string(),
    }
}

fn faces(
    solid: &Solid,
    body: usize,
    base: V3,
    highlights: &[Highlight<'_>],
    out: &mut Vec<Surface>,
    info: &mut Vec<FaceInfo>,
) {
    let meshed = solid.robust_triangulation(geometry::mesh_tolerance(solid) * 0.5);
    let originals: Vec<Face> = crate::select::faces(solid);
    let faces = meshed
        .boundaries()
        .iter()
        .flat_map(|shell| shell.face_iter().cloned())
        .collect::<Vec<_>>();
    for (index, face) in faces.iter().enumerate() {
        let Some(mesh) = face.surface() else { continue };
        let mut surface = Surface {
            colour: face_colour(index, base, highlights),
            body,
            face: Some(info.len()),
            ..Default::default()
        };
        let positions = mesh.positions();
        let normals = mesh.normals();
        let mut area = 0.0;
        let outward = face.orientation();
        for triangle in mesh.faces().triangle_iter() {
            let triangle = if outward {
                triangle
            } else {
                [triangle[0], triangle[2], triangle[1]]
            };
            let corners = triangle.map(|v| v3(positions[v.pos]));
            let [a, b, c] = triangle.map(|v| positions[v.pos]);
            area += (b - a).cross(c - a).magnitude() / 2.0;
            let flat = norm(cross(
                sub(corners[1], corners[0]),
                sub(corners[2], corners[0]),
            ));
            for (vertex, corner) in triangle.iter().zip(corners) {
                surface.positions.push(corner);
                let n = vertex.nor.and_then(|i| normals.get(i)).map_or(flat, |n| {
                    let n = norm([n.x as f32, n.y as f32, n.z as f32]);
                    if outward { n } else { [-n[0], -n[1], -n[2]] }
                });
                surface.normals.push(n);
            }
        }
        info.push(FaceInfo {
            body,
            kind: originals
                .get(index)
                .map_or_else(|| "face".to_string(), |f| describe(&f.oriented_surface())),
            area,
        });
        out.push(surface);
    }
}

fn polyline(edge: &Edge) -> Vec<V3> {
    geometry::curve_samples(&edge.curve())
        .into_iter()
        .map(v3)
        .collect()
}

fn segments(points: &[V3], out: &mut Vec<[V3; 2]>) {
    out.extend(
        points
            .windows(2)
            .filter(|pair| sub(pair[1], pair[0]).iter().any(|c| c.abs() > 1.0e-9))
            .map(|pair| [pair[0], pair[1]]),
    );
}

pub fn build(
    model: &Model,
    highlights: &[Highlight<'_>],
    marked: &[Edge],
    marked_colour: V3,
) -> Scene {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let current = model.current_body();
    let bodies: Vec<(String, &Solid)> = model
        .bodies
        .iter()
        .map(|(name, solid)| (name.clone(), solid))
        .chain(model.solid.as_ref().map(|solid| (current.clone(), solid)))
        .collect();
    let bounds: BoundingBox<Point3> = bodies
        .iter()
        .flat_map(|(_, s)| {
            let b = geometry::bounds(s);
            [b.min(), b.max()]
        })
        .collect();
    let radius = (bounds.diameter() / 2.0).max(1.0e-3) as f32;
    let mut surfaces = Vec::new();
    let mut lines = Vec::new();
    let mut info = Vec::new();
    let mut parts = Vec::new();
    let mut vertices = Vec::new();
    let skip: std::collections::HashSet<_> = marked.iter().map(|m| m.id()).collect();
    for (body, (name, solid)) in bodies.iter().enumerate() {
        let is_current = *name == current && model.solid.is_some();
        let colour = model
            .colours
            .get(name)
            .map_or(BODY, |c| c.map(|v| v as f32));
        let own: &[Highlight<'_>] = if is_current { highlights } else { &[] };
        faces(solid, body, colour, own, &mut surfaces, &mut info);
        let mut edges = Lines {
            colour: EDGE,
            body,
            width: EDGE_WIDTH,
            ..Default::default()
        };
        let mut seen = std::collections::HashSet::new();
        solid
            .boundaries()
            .iter()
            .flat_map(|shell| shell.edge_iter())
            .filter(|edge| seen.insert(edge.id()) && !skip.contains(&edge.id()))
            .for_each(|edge| segments(&polyline(&edge), &mut edges.segments));
        lines.push(edges);
        let mut seen = std::collections::HashSet::new();
        vertices.extend(
            solid
                .boundaries()
                .iter()
                .flat_map(|shell| shell.vertex_iter())
                .filter(|v| seen.insert(v.id()))
                .map(|v| (body, v3(v.point()))),
        );
        parts.push(Part {
            name: name.clone(),
            colour,
            volume: geometry::volume(solid),
            current: is_current,
        });
    }
    let current_index = parts.iter().position(|p| p.current).unwrap_or(0);
    let mut highlighted = Lines {
        colour: marked_colour,
        body: current_index,
        width: MARKED_WIDTH,
        ..Default::default()
    };
    marked
        .iter()
        .for_each(|edge| segments(&polyline(edge), &mut highlighted.segments));
    lines.push(highlighted);
    Scene {
        id: NEXT.fetch_add(1, Ordering::Relaxed),
        surfaces,
        lines,
        centre: v3(bounds.center()),
        radius,
        parts,
        vertices,
        faces: info,
    }
}
