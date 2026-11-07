use super::Spec;
use anyhow::{Result, bail};
use monstertruck::meshing::prelude::*;

pub struct Patch {
    pub tag: usize,
    pub body: String,
    pub points: Vec<Point3>,
    pub triangles: Vec<[usize; 3]>,
}

impl Patch {
    pub fn area(&self) -> f64 {
        self.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|v| self.points[v]);
                (b - a).cross(c - a).magnitude() / 2.0
            })
            .sum()
    }

    pub fn bounds(&self) -> BoundingBox<Point3> {
        self.points.iter().copied().collect()
    }
}

pub fn patches(spec: &Spec, tolerance: f64) -> Result<Vec<Patch>> {
    let everything: Vec<String> = spec
        .target()?
        .named_solids()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let mut out = Vec::new();
    for (name, solid) in spec.named()? {
        let index = everything.iter().position(|n| *n == name).unwrap_or(0);
        let meshed =
            solid.robust_triangulation(tolerance.max(crate::geometry::mesh_tolerance(solid)));
        let surfaces = meshed
            .boundaries()
            .iter()
            .flat_map(|shell| shell.face_iter().cloned())
            .map(|f| {
                f.surface().map(|mut mesh| {
                    if !f.orientation() {
                        mesh.invert();
                    }
                    mesh
                })
            });
        for (face, mesh) in surfaces.enumerate() {
            let Some(mesh) = mesh else { continue };
            out.push(Patch {
                tag: index * super::fem::STRIDE + face,
                body: name.clone(),
                points: mesh.positions().to_vec(),
                triangles: mesh
                    .faces()
                    .triangle_iter()
                    .map(|t| t.map(|v| v.pos))
                    .collect(),
            });
        }
    }
    if out.is_empty() {
        bail!("the part has no faces");
    }
    Ok(out)
}

pub fn volume(patches: &[&Patch]) -> f64 {
    patches
        .iter()
        .flat_map(|p| {
            p.triangles.iter().map(|t| {
                let [a, b, c] = t.map(|v| p.points[v].to_vec());
                a.dot(b.cross(c)) / 6.0
            })
        })
        .sum()
}
