use super::Model;
use super::args::Args;
use crate::geometry;
use crate::parse::Line;
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

pub struct MassProperties {
    pub volume: f64,
    pub area: f64,
    pub centroid: Point3,
}

pub fn mass_properties(solids: &[&Solid]) -> MassProperties {
    let (mut volume, mut area, mut moment) = (0.0, 0.0, Vector3::new(0.0, 0.0, 0.0));
    for solid in solids {
        let mesh = geometry::mesh(solid, geometry::bounds(solid).diameter() * 2.0e-5);
        let positions = mesh.positions();
        for triangle in mesh.faces().triangle_iter() {
            let [a, b, c] = triangle.map(|v| positions[v.pos].to_vec());
            let six = a.dot(b.cross(c));
            volume += six / 6.0;
            moment += (a + b + c) * (six / 24.0);
            area += (b - a).cross(c - a).magnitude() / 2.0;
        }
    }
    MassProperties {
        volume,
        area,
        centroid: Point3::from_vec(moment / volume),
    }
}

fn tidy(v: f64) -> f64 {
    if v.abs() < 5.0e-4 { 0.0 } else { v }
}

impl Model {
    pub(crate) fn op_measure(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["what"], &[], true)?;
        let what = args.text("what")?;
        match (what, args.rest.as_slice()) {
            ("mass", []) => {
                let mass = mass_properties(&self.solids());
                let c = mass.centroid;
                Ok(format!(
                    "volume {:.3} area {:.3} centroid {:.3},{:.3},{:.3}",
                    mass.volume,
                    mass.area,
                    tidy(c.x),
                    tidy(c.y),
                    tidy(c.z)
                ))
            }
            ("overlap", [a, b]) => {
                let (a, b) = (self.named_body(a)?, self.named_body(b)?);
                let overlap = match monstertruck::solid::and_normalized(&a, &b) {
                    Ok(common) => geometry::volume(&common),
                    Err(_) => 0.0,
                };
                Ok(format!("overlap {overlap:.3}"))
            }
            (faces_a, [faces_b]) => self.distance(faces_a, faces_b),
            _ => bail!("`measure` takes `mass`, `overlap body body`, or two face selectors"),
        }
    }

    fn distance(&self, a: &str, b: &str) -> Result<String> {
        let solid = self.active("measure")?;
        let tolerance = self.tolerance();
        let faces = select::faces(solid);
        let pick = |selector: &str| -> Result<Vec<Face>> {
            let indices = select::select_faces(selector, solid, &self.groups, tolerance)?;
            if indices.is_empty() {
                bail!("`{selector}` matched no faces");
            }
            Ok(indices.into_iter().map(|i| faces[i].clone()).collect())
        };
        let (first, second) = (pick(a)?, pick(b)?);
        let planes = |set: &[Face]| -> Option<Vec<Plane>> {
            set.iter()
                .map(|f| match f.oriented_surface() {
                    Surface::Plane(p) => Some(p),
                    _ => None,
                })
                .collect()
        };
        if let (Some(pa), Some(pb)) = (planes(&first), planes(&second)) {
            let n = pa[0].normal();
            if pa
                .iter()
                .chain(&pb)
                .all(|p| p.normal().cross(n).magnitude() < 1.0e-9)
            {
                let gap = (pa[0].origin() - pb[0].origin()).dot(n).abs();
                return Ok(format!("distance {gap:.3} between parallel faces"));
            }
        }
        let samples = |set: &[Face]| -> Vec<Point3> {
            set.iter()
                .flat_map(|face| {
                    face.edge_iter().flat_map(|edge| {
                        let curve = edge.curve();
                        let (t0, t1) = curve.range_tuple();
                        (0..=16)
                            .map(move |i| curve.subs(t0 + (t1 - t0) * i as f64 / 16.0))
                            .collect::<Vec<_>>()
                    })
                })
                .collect()
        };
        let (pa, pb) = (samples(&first), samples(&second));
        let gap = pa
            .iter()
            .flat_map(|p| pb.iter().map(move |q| p.distance(*q)))
            .fold(f64::INFINITY, f64::min);
        if !gap.is_finite() {
            return Err(anyhow!("could not sample the faces"));
        }
        Ok(format!("distance {gap:.3} between face edges"))
    }
}
