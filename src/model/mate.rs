use super::Model;
use super::args::{Args, point3};
use super::assembly::Feature;
use crate::geometry;
use crate::parse::Line;
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::meshing::prelude::*;
use monstertruck::modeling::*;

#[derive(Clone, Copy, Debug)]
pub struct Cylinder {
    pub point: Point3,
    pub axis: Vector3,
    pub radius: f64,
}

impl Cylinder {
    fn offset_from(&self, other: &Cylinder) -> f64 {
        let d = other.point - self.point;
        (d - self.axis * d.dot(self.axis)).magnitude()
    }

    fn parallel(&self, other: &Cylinder) -> bool {
        self.axis.cross(other.axis).magnitude() < 1.0e-6
    }
}

fn fit_circle(points: &[(f64, f64)]) -> Option<((f64, f64), f64, f64)> {
    let mut m = [[0.0f64; 3]; 3];
    let mut rhs = [0.0f64; 3];
    for &(x, y) in points {
        let row = [x, y, 1.0];
        let target = -(x * x + y * y);
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += row[i] * row[j];
            }
            rhs[i] += row[i] * target;
        }
    }
    let det = |m: &[[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let whole = det(&m);
    if whole.abs() < 1.0e-300 {
        return None;
    }
    let solve = |k: usize| {
        let mut c = m;
        (0..3).for_each(|i| c[i][k] = rhs[i]);
        det(&c) / whole
    };
    let (d, e, f) = (solve(0), solve(1), solve(2));
    let centre = (-d / 2.0, -e / 2.0);
    let r2 = centre.0 * centre.0 + centre.1 * centre.1 - f;
    if r2 <= 0.0 {
        return None;
    }
    let radius = r2.sqrt();
    let worst = points
        .iter()
        .map(|&(x, y)| ((x - centre.0).hypot(y - centre.1) - radius).abs())
        .fold(0.0, f64::max);
    Some((centre, radius, worst))
}

fn face_cylinder(points: &[Point3], normals: &[Vector3]) -> Option<Cylinder> {
    let first = *normals.first()?;
    let other = normals.iter().max_by(|a, b| {
        first
            .cross(**a)
            .magnitude()
            .total_cmp(&first.cross(**b).magnitude())
    })?;
    let axis = first.cross(*other);
    if axis.magnitude() < 1.0e-3 {
        return None;
    }
    let axis = axis.normalize();
    if normals.iter().any(|n| n.dot(axis).abs() > 2.0e-2) {
        return None;
    }
    let u = axis
        .cross(if axis.x.abs() < 0.9 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        })
        .normalize();
    let v = axis.cross(u);
    let flat: Vec<(f64, f64)> = points
        .iter()
        .map(|p| (p.to_vec().dot(u), p.to_vec().dot(v)))
        .collect();
    let ((cu, cv), radius, worst) = fit_circle(&flat)?;
    if worst > radius * 1.0e-2 {
        return None;
    }
    let along = points.iter().map(|p| p.to_vec().dot(axis)).sum::<f64>() / points.len() as f64;
    Some(Cylinder {
        point: Point3::from_vec(u * cu + v * cv + axis * along),
        axis,
        radius,
    })
}

pub fn cylinders(solid: &Solid, picked: &[usize]) -> Vec<Cylinder> {
    let all = select::faces(solid);
    let tolerance = geometry::mesh_tolerance(solid) * 0.5;
    let mut found: Vec<Cylinder> = Vec::new();
    for &index in picked {
        let Some(face) = all.get(index) else { continue };
        let single: Shell = vec![face.clone()].into();
        let meshed = single.robust_triangulation(tolerance);
        let Some(mesh) = meshed.face_iter().next().and_then(|f| f.surface()) else {
            continue;
        };
        let positions = mesh.positions();
        let surface_normals = mesh.normals();
        let mut points = Vec::new();
        let mut normals = Vec::new();
        for triangle in mesh.faces().triangle_iter() {
            for vertex in triangle {
                if let Some(n) = vertex.nor.and_then(|i| surface_normals.get(i)) {
                    points.push(positions[vertex.pos]);
                    normals.push(n.normalize());
                }
            }
        }
        let Some(cylinder) = face_cylinder(&points, &normals) else {
            continue;
        };
        let size = cylinder.radius.max(1.0e-9);
        let same = found.iter().position(|known| {
            known.parallel(&cylinder)
                && known.offset_from(&cylinder) < size * 1.0e-3
                && (known.radius - cylinder.radius).abs() < size * 1.0e-3
        });
        if same.is_none() {
            found.push(cylinder);
        }
    }
    found
}

fn fixed_part(text: &str) -> String {
    text.split_once(':')
        .map_or(text, |(part, _)| part)
        .to_string()
}

fn turn_onto(from: Vector3, to: Vector3) -> Matrix3 {
    let axis = from.cross(to);
    let cos = from.dot(to).clamp(-1.0, 1.0);
    if axis.magnitude() < 1.0e-12 {
        if cos > 0.0 {
            return Matrix3::identity();
        }
        let helper = if from.x.abs() < 0.9 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        };
        return Matrix3::from_axis_angle(from.cross(helper).normalize(), Rad(std::f64::consts::PI));
    }
    Matrix3::from_axis_angle(axis.normalize(), Rad(cos.acos()))
}

fn about(point: Point3, linear: Matrix3) -> Matrix4 {
    Matrix4::from_translation(point.to_vec())
        * Matrix4::from(linear)
        * Matrix4::from_translation(-point.to_vec())
}

impl Model {
    fn reference(&self, text: &str) -> Result<(String, Solid, Vec<usize>)> {
        let (part, selector) = text.split_once(':').ok_or_else(|| {
            anyhow!("write `part:faces`, like `case.main:pilot.side`; `{text}` has no `:`")
        })?;
        let solid = self.named_body(part)?;
        let groups = self.part_groups.get(part).cloned().unwrap_or_default();
        let tolerance = (geometry::bounds(&solid).diameter() * 1.0e-6).max(1.0e-7);
        let faces = select::select_faces(selector, &solid, &groups, tolerance)?;
        if faces.is_empty() {
            bail!("`{text}` matched no faces");
        }
        Ok((part.to_string(), solid, faces))
    }

    fn holes(&self, text: &str) -> Result<(String, Vec<Cylinder>)> {
        let (part, solid, faces) = self.reference(text)?;
        let found = cylinders(&solid, &faces);
        if found.is_empty() {
            bail!("`{text}` has no round faces to line up");
        }
        Ok((part, found))
    }

    pub(crate) fn op_concentric(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["moving", "fixed"], &["near", "flip"], true)?;
        let flip = match args.rest.as_slice() {
            [] => false,
            ["flip"] => true,
            _ => bail!("`concentric` takes `flip` to point the moving part the other way"),
        };
        let (part, mine) = self.holes(args.text("moving")?)?;
        let (_, theirs) = self.holes(args.text("fixed")?)?;
        let [own] = mine.as_slice() else {
            bail!(
                "`{}` has {} round faces; pick one hole on the moving part",
                args.text("moving")?,
                mine.len()
            );
        };
        let near = args
            .values
            .get("near")
            .map(|text| point3(text, &self.scope))
            .transpose()?
            .unwrap_or(own.point);
        let target = theirs
            .iter()
            .min_by(|a, b| {
                let gap = |c: &Cylinder| {
                    let d = near - c.point;
                    (d - c.axis * d.dot(c.axis)).magnitude()
                };
                gap(a).total_cmp(&gap(b))
            })
            .copied()
            .ok_or_else(|| anyhow!("no hole to line up with"))?;
        let mut goal = if own.axis.dot(target.axis) < 0.0 {
            -target.axis
        } else {
            target.axis
        };
        if flip {
            goal = -goal;
        }
        let turn = about(own.point, turn_onto(own.axis, goal));
        let moved = turn.transform_point(own.point);
        let d = target.point - moved;
        let shift = d - goal * d.dot(goal);
        let transform = Matrix4::from_translation(shift) * turn;
        let fixed = fixed_part(args.text("fixed")?);
        let features = [
            Feature::Axis(own.point, own.axis),
            Feature::Axis(target.point, target.axis),
        ];
        let said = self.mate_parts(&line.text, &part, &fixed, features, 0.0, transform)?;
        Ok(format!(
            "{said}; radius {:.3} in {:.3}",
            own.radius, target.radius
        ))
    }

    pub(crate) fn op_flush(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["moving", "fixed"], &["offset"], false)?;
        let plane = |text: &str| -> Result<(String, Plane)> {
            let (part, solid, faces) = self.reference(text)?;
            let all = select::faces(&solid);
            let planes: Vec<Plane> = faces
                .iter()
                .filter_map(|&i| match all[i].oriented_surface() {
                    Surface::Plane(p) => Some(p),
                    _ => None,
                })
                .collect();
            let first = *planes
                .first()
                .ok_or_else(|| anyhow!("`{text}` has no flat faces"))?;
            if planes.iter().any(|p| {
                p.normal().dot(first.normal()) < 1.0 - 1.0e-9
                    || (p.origin() - first.origin()).dot(first.normal()).abs() > 1.0e-6
            }) {
                bail!("`{text}` matched faces that are not on one plane");
            }
            Ok((part, first))
        };
        let (part, mine) = plane(args.text("moving")?)?;
        let (_, theirs) = plane(args.text("fixed")?)?;
        let gap = args.optional_number("offset", &self.scope)?.unwrap_or(0.0);
        let turn = about(mine.origin(), turn_onto(mine.normal(), -theirs.normal()));
        let origin = turn.transform_point(mine.origin());
        let distance = (theirs.origin() - origin).dot(theirs.normal()) + gap;
        let transform = Matrix4::from_translation(theirs.normal() * distance) * turn;
        let fixed = fixed_part(args.text("fixed")?);
        let features = [
            Feature::Plane(mine.origin(), mine.normal()),
            Feature::Plane(theirs.origin(), theirs.normal()),
        ];
        self.mate_parts(&line.text, &part, &fixed, features, gap, transform)
    }

    pub(crate) fn op_aligned(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["a", "b"], &["tol"], false)?;
        let tolerance = args.optional_number("tol", &self.scope)?.unwrap_or(0.05);
        let (_, mine) = self.holes(args.text("a")?)?;
        let (_, theirs) = self.holes(args.text("b")?)?;
        let mut worst: f64 = 0.0;
        let mut problems = Vec::new();
        for (i, own) in mine.iter().enumerate() {
            let partner = theirs
                .iter()
                .filter(|c| c.parallel(own))
                .map(|c| (own.offset_from(c), c))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            match partner {
                None => problems.push(format!(
                    "hole {} at {:.3},{:.3},{:.3} has no parallel partner",
                    i + 1,
                    own.point.x,
                    own.point.y,
                    own.point.z
                )),
                Some((offset, _)) if offset > tolerance => problems.push(format!(
                    "hole {} at {:.3},{:.3},{:.3} is {offset:.3} off its partner",
                    i + 1,
                    own.point.x,
                    own.point.y,
                    own.point.z
                )),
                Some((offset, _)) => worst = worst.max(offset),
            }
        }
        if !problems.is_empty() {
            bail!("{}", problems.join("; "));
        }
        Ok(format!(
            "{} hole(s) line up, worst {worst:.3} off",
            mine.len()
        ))
    }
}
