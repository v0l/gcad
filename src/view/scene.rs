use crate::geometry;
use monstertruck::meshing::prelude::*;
use monstertruck::modeling::*;
use std::sync::atomic::{AtomicU64, Ordering};

pub type V3 = [f32; 3];

#[derive(Clone, Default)]
pub struct Surface {
    pub positions: Vec<V3>,
    pub normals: Vec<V3>,
    pub colour: V3,
}

#[derive(Default)]
pub struct Scene {
    pub id: u64,
    pub surfaces: Vec<Surface>,
    pub centre: V3,
    pub radius: f32,
}

pub struct Highlight<'a> {
    pub faces: &'a [usize],
    pub colour: V3,
}

pub const BODY: V3 = [0.62, 0.66, 0.72];
pub const EDGE: V3 = [0.05, 0.06, 0.07];

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

fn face_colour(index: usize, highlights: &[Highlight<'_>]) -> V3 {
    highlights
        .iter()
        .rev()
        .find(|h| h.faces.contains(&index))
        .map_or(BODY, |h| h.colour)
}

fn faces(solid: &Solid, highlights: &[Highlight<'_>], out: &mut Vec<Surface>) {
    let meshed = solid.robust_triangulation(geometry::mesh_tolerance(solid) * 0.5);
    let faces = meshed
        .boundaries()
        .iter()
        .flat_map(|shell| shell.face_iter().cloned())
        .collect::<Vec<_>>();
    for (index, face) in faces.iter().enumerate() {
        let Some(mesh) = face.surface() else { continue };
        let mut surface = Surface {
            colour: face_colour(index, highlights),
            ..Default::default()
        };
        let positions = mesh.positions();
        let normals = mesh.normals();
        for triangle in mesh.faces().triangle_iter() {
            let corners = triangle.map(|v| v3(positions[v.pos]));
            let flat = norm(cross(
                sub(corners[1], corners[0]),
                sub(corners[2], corners[0]),
            ));
            for (vertex, corner) in triangle.iter().zip(corners) {
                surface.positions.push(corner);
                let n = vertex
                    .nor
                    .and_then(|i| normals.get(i))
                    .map_or(flat, |n| norm([n.x as f32, n.y as f32, n.z as f32]));
                surface.normals.push(n);
            }
        }
        out.push(surface);
    }
}

fn polyline(edge: &Edge) -> Vec<V3> {
    let curve = edge.curve();
    let (t0, t1) = curve.range_tuple();
    let count = if matches!(curve, Curve::Line(_)) {
        1
    } else {
        32
    };
    (0..=count)
        .map(|i| v3(curve.subs(t0 + (t1 - t0) * i as f64 / count as f64)))
        .collect()
}

fn tube(points: &[V3], radius: f32, out: &mut Surface) {
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let along = sub(b, a);
        if along.iter().all(|c| c.abs() < 1.0e-9) {
            continue;
        }
        let along = norm(along);
        let helper = if along[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let u = norm(cross(along, helper));
        let w = cross(along, u);
        let ring: Vec<V3> = (0..4)
            .map(|k| {
                let angle = k as f32 * std::f32::consts::FRAC_PI_2;
                let (c, s) = (angle.cos(), angle.sin());
                [
                    u[0] * c + w[0] * s,
                    u[1] * c + w[1] * s,
                    u[2] * c + w[2] * s,
                ]
            })
            .collect();
        for k in 0..4 {
            let (n0, n1) = (ring[k], ring[(k + 1) % 4]);
            let at = |p: V3, n: V3| {
                [
                    p[0] + n[0] * radius,
                    p[1] + n[1] * radius,
                    p[2] + n[2] * radius,
                ]
            };
            for (p, n) in [(a, n0), (b, n0), (b, n1), (a, n0), (b, n1), (a, n1)] {
                out.positions.push(at(p, n));
                out.normals.push(n);
            }
        }
    }
}

pub fn build(
    solid: &Solid,
    highlights: &[Highlight<'_>],
    marked: &[Edge],
    marked_colour: V3,
) -> Scene {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let bounds = geometry::bounds(solid);
    let radius = (bounds.diameter() / 2.0).max(1.0e-3) as f32;
    let mut surfaces = Vec::new();
    faces(solid, highlights, &mut surfaces);
    let mut edges = Surface {
        colour: EDGE,
        ..Default::default()
    };
    let mut seen = Vec::new();
    solid
        .boundaries()
        .iter()
        .flat_map(|shell| shell.edge_iter())
        .filter(|edge| {
            let fresh = !seen.contains(&edge.id());
            seen.push(edge.id());
            fresh && !marked.iter().any(|m| m.is_same(edge))
        })
        .for_each(|edge| tube(&polyline(&edge), radius * 0.0018, &mut edges));
    surfaces.push(edges);
    let mut highlighted = Surface {
        colour: marked_colour,
        ..Default::default()
    };
    marked
        .iter()
        .for_each(|edge| tube(&polyline(edge), radius * 0.005, &mut highlighted));
    surfaces.push(highlighted);
    Scene {
        id: NEXT.fetch_add(1, Ordering::Relaxed),
        surfaces,
        centre: v3(bounds.center()),
        radius,
    }
}
