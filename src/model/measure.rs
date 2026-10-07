use super::Model;
use super::args::Args;
use crate::geometry;
use crate::parse::Line;
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::meshing::prelude::*;
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
        let args = Args::new(line, &["what"], &["pull", "min"], true)?;
        let what = args.text("what")?;
        match (what, args.rest.as_slice()) {
            ("mass", []) => {
                let mass = mass_properties(&self.solids());
                let c = mass.centroid;
                let weighed = self.weights();
                let grams: f64 = weighed.iter().filter_map(|(_, g)| *g).sum();
                let bare: Vec<String> = weighed
                    .iter()
                    .filter(|(_, g)| g.is_none())
                    .map(|(n, _)| n.clone())
                    .collect();
                let weight = match (grams > 0.0, bare.is_empty()) {
                    (false, _) => String::new(),
                    (true, true) => format!(", mass {grams:.3} g"),
                    (true, false) => format!(
                        ", mass {grams:.3} g without {} (no material)",
                        bare.join(", ")
                    ),
                };
                Ok(format!(
                    "volume {:.3} area {:.3} centroid {:.3},{:.3},{:.3}{weight}",
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
            ("thickness", []) => self.thickness(),
            ("draft", []) => {
                let pull = args.values.get("pull").copied().unwrap_or("z");
                let least = args.optional_number("min", &self.scope)?.unwrap_or(1.0);
                self.draft(pull, least)
            }
            (faces_a, [faces_b]) => self.distance(faces_a, faces_b),
            _ => bail!(
                "`measure` takes `mass`, `thickness`, `draft pull=`, `overlap body body`, or two face selectors"
            ),
        }
    }

    pub fn weights(&self) -> Vec<(String, Option<f64>)> {
        self.named_solids()
            .into_iter()
            .map(|(name, solid)| {
                let grams = self
                    .materials
                    .get(&name)
                    .map(|m| geometry::volume(solid).abs() * m.density / 1000.0);
                (name, grams)
            })
            .collect()
    }

    fn thickness(&self) -> Result<String> {
        let solid = self.active("measure")?;
        let mesh = geometry::mesh(solid, geometry::mesh_tolerance(solid) * 5.0);
        let positions = mesh.positions();
        let floor = self.tolerance() * 10.0;
        let (thinnest, at) = mesh
            .faces()
            .triangle_iter()
            .filter_map(|triangle| {
                let [a, b, c] = triangle.map(|v| positions[v.pos]);
                let normal = (b - a).cross(c - a);
                if normal.magnitude() < 1.0e-14 {
                    return None;
                }
                let centre = Point3::from_vec((a.to_vec() + b.to_vec() + c.to_vec()) / 3.0);
                let inward = -normal.normalize();
                geometry::ray_hits(&mesh, centre, inward)
                    .into_iter()
                    .filter(|(t, n)| *t > floor && n.dot(inward) > 0.0)
                    .map(|(t, _)| t)
                    .min_by(f64::total_cmp)
                    .map(|t| (t, centre))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .ok_or_else(|| anyhow!("could not find the walls of the solid"))?;
        Ok(format!(
            "thickness min {thinnest:.3} at {:.3},{:.3},{:.3}",
            tidy(at.x),
            tidy(at.y),
            tidy(at.z)
        ))
    }

    fn draft(&self, pull: &str, least: f64) -> Result<String> {
        let solid = self.active("measure")?;
        let pull = super::args::axis(pull)?;
        let tolerance = geometry::mesh_tolerance(solid);
        let mut drafts = Vec::new();
        for shell in solid.boundaries() {
            for face in shell.robust_triangulation(tolerance).face_iter() {
                let Some(mesh) = face.surface() else { continue };
                let positions = mesh.positions();
                let angle = mesh
                    .faces()
                    .triangle_iter()
                    .filter_map(|triangle| {
                        let [a, b, c] = triangle.map(|v| positions[v.pos]);
                        let normal = (b - a).cross(c - a);
                        (normal.magnitude() > 1.0e-14).then(|| {
                            normal
                                .normalize()
                                .dot(pull)
                                .clamp(-1.0, 1.0)
                                .asin()
                                .to_degrees()
                        })
                    })
                    .min_by(|a, b| a.abs().total_cmp(&b.abs()));
                drafts.extend(angle);
            }
        }
        let under = drafts.iter().filter(|d| d.abs() < least - 1.0e-6).count();
        let smallest = drafts
            .iter()
            .copied()
            .min_by(|a, b| a.abs().total_cmp(&b.abs()))
            .unwrap_or(90.0);
        Ok(format!(
            "{under} faces under {least}° of draft, least {:.3}° across {} faces",
            tidy(smallest),
            drafts.len()
        ))
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
