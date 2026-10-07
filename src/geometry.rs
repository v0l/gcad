use monstertruck::meshing::prelude::*;
use monstertruck::modeling::*;

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub origin: Point3,
    pub x: Vector3,
    pub y: Vector3,
    pub normal: Vector3,
}

impl Frame {
    pub fn from_normal(origin: Point3, normal: Vector3) -> Frame {
        let normal = normal.normalize();
        let (x, y) = if normal.z.abs() < 0.999 {
            let y = (Vector3::unit_z() - normal * normal.z).normalize();
            (y.cross(normal), y)
        } else {
            let x = Vector3::unit_x();
            (x, normal.cross(x))
        };
        Frame {
            origin,
            x,
            y,
            normal,
        }
    }

    pub fn named(name: &str) -> Option<Frame> {
        let normal = match name.to_ascii_uppercase().as_str() {
            "XY" => Vector3::unit_z(),
            "XZ" => -Vector3::unit_y(),
            "YZ" => Vector3::unit_x(),
            _ => return None,
        };
        Some(Frame::from_normal(Point3::origin(), normal))
    }

    pub fn at(&self, u: f64, v: f64) -> Point3 {
        self.origin + self.x * u + self.y * v
    }

    pub fn offset(&self, distance: f64) -> Frame {
        Frame {
            origin: self.origin + self.normal * distance,
            ..*self
        }
    }
}

#[derive(Clone, Debug)]
pub enum Profile {
    Rect {
        center: (f64, f64),
        width: f64,
        height: f64,
        radius: f64,
    },
    Circle {
        center: (f64, f64),
        diameter: f64,
    },
    Polygon {
        points: Vec<(f64, f64)>,
    },
    Slot {
        center: (f64, f64),
        length: f64,
        width: f64,
        angle: f64,
    },
    Ellipse {
        center: (f64, f64),
        rx: f64,
        ry: f64,
    },
    Path {
        start: (f64, f64),
        segments: Vec<Segment>,
    },
    Spline {
        points: Vec<(f64, f64)>,
    },
    Text {
        text: String,
        size: f64,
        at: (f64, f64),
    },
}

#[derive(Clone, Debug)]
pub enum Segment {
    Line((f64, f64)),
    Arc { to: (f64, f64), via: (f64, f64) },
}

impl Frame {
    pub fn local(&self, point: Point3) -> (f64, f64) {
        let d = point - self.origin;
        (d.dot(self.x), d.dot(self.y))
    }

    pub fn matrix(&self) -> Matrix4 {
        Matrix4::from_cols(
            self.x.extend(0.0),
            self.y.extend(0.0),
            self.normal.extend(0.0),
            self.origin.to_homogeneous(),
        )
    }
}

fn slot_wire(frame: &Frame, center: (f64, f64), length: f64, width: f64, angle: f64) -> Wire {
    let (c, s) = (angle.to_radians().cos(), angle.to_radians().sin());
    let r = width / 2.0;
    let half = length / 2.0 - r;
    let at = |u: f64, v: f64| frame.at(center.0 + u * c - v * s, center.1 + u * s + v * c);
    let v = builder::vertices([at(half, -r), at(half, r), at(-half, r), at(-half, -r)]);
    vec![
        builder::circle_arc(&v[0], &v[1], at(half + r, 0.0)),
        builder::line(&v[1], &v[2]),
        builder::circle_arc(&v[2], &v[3], at(-half - r, 0.0)),
        builder::line(&v[3], &v[0]),
    ]
    .into()
}

fn ellipse_wire(frame: &Frame, center: (f64, f64), rx: f64, ry: f64) -> Wire {
    let unit: Wire = primitive::circle(
        Point3::new(1.0, 0.0, 0.0),
        Point3::origin(),
        Vector3::unit_z(),
        4,
    );
    let placement = frame.matrix()
        * Matrix4::from_translation(Vector3::new(center.0, center.1, 0.0))
        * Matrix4::from_nonuniform_scale(rx, ry, 1.0);
    builder::transformed(&unit, placement)
}

fn path_wire(frame: &Frame, start: (f64, f64), segments: &[Segment]) -> Wire {
    let ends: Vec<(f64, f64)> = std::iter::once(start)
        .chain(segments.iter().map(|segment| match segment {
            Segment::Line(to) | Segment::Arc { to, .. } => *to,
        }))
        .collect();
    let vertices = builder::vertices(ends[..segments.len()].iter().map(|&(u, v)| frame.at(u, v)));
    let count = vertices.len();
    segments
        .iter()
        .enumerate()
        .map(|(i, segment)| {
            let (from, to) = (&vertices[i], &vertices[(i + 1) % count]);
            match segment {
                Segment::Line(_) => builder::line(from, to),
                Segment::Arc { via, .. } => builder::circle_arc(from, to, frame.at(via.0, via.1)),
            }
        })
        .collect()
}

fn spline_wire(frame: &Frame, points: &[(f64, f64)]) -> std::result::Result<Wire, String> {
    let closed: Vec<Point3> = points
        .iter()
        .chain(points.first())
        .map(|&(u, v)| frame.at(u, v))
        .collect();
    let n = closed.len();
    let degree = 3.min(n - 1);
    let knots = KnotVector::uniform_knot(degree, n - degree);
    let values: Vec<f64> = knots.iter().copied().collect();
    let parameter_points: Vec<(f64, Point3)> = closed
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            (
                values[i + 1..=i + degree].iter().sum::<f64>() / degree as f64,
                p,
            )
        })
        .collect();
    let mut curve = BsplineCurve::try_interpolate(knots, parameter_points)
        .map_err(|e| format!("spline: {e}"))?;
    let tail = curve.cut(0.5);
    let start = builder::vertex(closed[0]);
    let middle = builder::vertex(tail.front());
    Ok(vec![
        Edge::new(&start, &middle, Curve::BsplineCurve(curve)),
        Edge::new(&middle, &start, Curve::BsplineCurve(tail)),
    ]
    .into())
}

fn text_wires(
    frame: &Frame,
    text: &str,
    size: f64,
    at: (f64, f64),
) -> std::result::Result<Vec<Wire>, String> {
    let face = ttf_parser::Face::parse(epaint_default_fonts::UBUNTU_LIGHT, 0)
        .map_err(|e| format!("font: {e}"))?;
    let options = text::TextOptions {
        scale: Some(size / face.units_per_em() as f64),
        y_flip: false,
        ..Default::default()
    };
    let wires = text::text_profile(&face, text, &options).map_err(|e| format!("text: {e}"))?;
    let placement = frame.matrix() * Matrix4::from_translation(Vector3::new(at.0, at.1, 0.0));
    Ok(wires
        .iter()
        .map(|wire| builder::transformed(wire, placement))
        .collect())
}

fn closed_polyline(points: &[Point3]) -> Wire {
    let vertices = builder::vertices(points.iter().copied());
    (0..vertices.len())
        .map(|i| builder::line(&vertices[i], &vertices[(i + 1) % vertices.len()]))
        .collect()
}

fn circle_wire(frame: &Frame, center: (f64, f64), radius: f64) -> Wire {
    let origin = frame.at(center.0, center.1);
    let seam = (frame.x + frame.y) * std::f64::consts::FRAC_1_SQRT_2;
    primitive::circle(origin + seam * radius, origin, frame.normal, 4)
}

fn rounded_rect(frame: &Frame, center: (f64, f64), width: f64, height: f64, radius: f64) -> Wire {
    let (hw, hh) = (width / 2.0, height / 2.0);
    let corners = [(1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0)];
    let corner_center =
        |(sx, sy): (f64, f64)| (center.0 + sx * (hw - radius), center.1 + sy * (hh - radius));
    let points: Vec<Point3> = corners
        .iter()
        .enumerate()
        .flat_map(|(i, &(sx, sy))| {
            let (cx, cy) = corner_center((sx, sy));
            let on_horizontal = (cx, cy + sy * radius);
            let on_vertical = (cx + sx * radius, cy);
            let (start, end) = if i % 2 == 0 {
                (on_horizontal, on_vertical)
            } else {
                (on_vertical, on_horizontal)
            };
            [frame.at(start.0, start.1), frame.at(end.0, end.1)]
        })
        .collect();
    let vertices = builder::vertices(points.iter().copied());
    let count = vertices.len();
    (0..count)
        .map(|i| {
            let (from, to) = (&vertices[i], &vertices[(i + 1) % count]);
            if i % 2 == 0 {
                let (sx, sy) = corners[i / 2];
                let (cx, cy) = corner_center((sx, sy));
                let diagonal = std::f64::consts::FRAC_1_SQRT_2 * radius;
                builder::circle_arc(from, to, frame.at(cx + sx * diagonal, cy + sy * diagonal))
            } else {
                builder::line(from, to)
            }
        })
        .collect()
}

impl Profile {
    pub fn wires(&self, frame: &Frame) -> std::result::Result<Vec<Wire>, String> {
        Ok(match self {
            Profile::Slot {
                center,
                length,
                width,
                angle,
            } => vec![slot_wire(frame, *center, *length, *width, *angle)],
            Profile::Ellipse { center, rx, ry } => vec![ellipse_wire(frame, *center, *rx, *ry)],
            Profile::Path { start, segments } => vec![path_wire(frame, *start, segments)],
            Profile::Spline { points } => vec![spline_wire(frame, points)?],
            Profile::Text { text, size, at } => text_wires(frame, text, *size, *at)?,
            other => vec![other.wire(frame)],
        })
    }

    pub fn wire(&self, frame: &Frame) -> Wire {
        match self {
            Profile::Rect {
                center,
                width,
                height,
                radius,
            } if *radius > 0.0 => rounded_rect(frame, *center, *width, *height, *radius),
            Profile::Rect {
                center,
                width,
                height,
                ..
            } => {
                let (hw, hh) = (width / 2.0, height / 2.0);
                let points = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
                    .map(|(u, v)| frame.at(center.0 + u, center.1 + v));
                closed_polyline(&points)
            }
            Profile::Circle { center, diameter } => circle_wire(frame, *center, diameter / 2.0),
            Profile::Polygon { points } => {
                let points: Vec<Point3> = points.iter().map(|&(u, v)| frame.at(u, v)).collect();
                closed_polyline(&points)
            }
            other => other
                .wires(frame)
                .ok()
                .and_then(|mut wires| (wires.len() == 1).then(|| wires.remove(0)))
                .unwrap_or_default(),
        }
    }

    pub fn inset(&self, delta: f64) -> std::result::Result<Profile, String> {
        match self {
            Profile::Rect {
                center,
                width,
                height,
                radius,
            } => {
                let rounded = *radius > 0.0;
                let (width, height, radius) = (
                    width - 2.0 * delta,
                    height - 2.0 * delta,
                    if rounded { radius - delta } else { 0.0 },
                );
                if width <= 0.0 || height <= 0.0 || (rounded && radius <= 0.0) {
                    return Err("the draft closes the rectangle or its corner radius".to_string());
                }
                Ok(Profile::Rect {
                    center: *center,
                    width,
                    height,
                    radius,
                })
            }
            Profile::Circle { center, diameter } => {
                let diameter = diameter - 2.0 * delta;
                if diameter <= 0.0 {
                    return Err("the draft closes the circle".to_string());
                }
                Ok(Profile::Circle {
                    center: *center,
                    diameter,
                })
            }
            Profile::Slot {
                center,
                length,
                width,
                angle,
            } => {
                let (length, width) = (length - 2.0 * delta, width - 2.0 * delta);
                if width <= 0.0 || length <= width {
                    return Err("the draft closes the slot".to_string());
                }
                Ok(Profile::Slot {
                    center: *center,
                    length,
                    width,
                    angle: *angle,
                })
            }
            Profile::Ellipse { .. }
            | Profile::Path { .. }
            | Profile::Spline { .. }
            | Profile::Text { .. } => Err("draft works on rect, circle, poly and slot".to_string()),
            Profile::Polygon { points } => {
                let n = points.len();
                let area: f64 = (0..n)
                    .map(|i| {
                        points[i].0 * points[(i + 1) % n].1 - points[(i + 1) % n].0 * points[i].1
                    })
                    .sum();
                let inward = if area > 0.0 { 1.0 } else { -1.0 } * delta;
                let shifted_line = |i: usize| {
                    let (a, b) = (points[i], points[(i + 1) % n]);
                    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                    let length = (dx * dx + dy * dy).sqrt();
                    let (nx, ny) = (-dy / length * inward, dx / length * inward);
                    ((a.0 + nx, a.1 + ny), (dx, dy))
                };
                let moved = (0..n)
                    .map(|i| {
                        let (p, d) = shifted_line((i + n - 1) % n);
                        let (q, e) = shifted_line(i);
                        let cross = d.0 * e.1 - d.1 * e.0;
                        if cross.abs() < 1.0e-12 {
                            return Err("polygon has a straight vertex".to_string());
                        }
                        let t = ((q.0 - p.0) * e.1 - (q.1 - p.1) * e.0) / cross;
                        Ok((p.0 + d.0 * t, p.1 + d.1 * t))
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(Profile::Polygon { points: moved })
            }
        }
    }
}

pub fn bounds(solid: &Solid) -> BoundingBox<Point3> {
    solid
        .boundaries()
        .iter()
        .flat_map(|shell| shell.vertex_iter())
        .map(|vertex| vertex.point())
        .collect()
}

pub fn mesh(solid: &Solid, tolerance: f64) -> PolygonMesh {
    let mut mesh = PolygonMesh::default();
    solid
        .boundaries()
        .iter()
        .for_each(|shell| mesh.merge(shell.robust_triangulation(tolerance).to_polygon()));
    mesh
}

pub fn mesh_tolerance(solid: &Solid) -> f64 {
    (bounds(solid).diameter() * 2.0e-4).max(1.0e-4)
}

pub fn volume(solid: &Solid) -> f64 {
    volume_at(solid, mesh_tolerance(solid))
}

pub fn volume_at(solid: &Solid, tolerance: f64) -> f64 {
    mesh(solid, tolerance).volume()
}
