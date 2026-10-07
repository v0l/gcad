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
}

fn closed_polyline(points: &[Point3]) -> Wire {
    let vertices = builder::vertices(points.iter().copied());
    (0..vertices.len())
        .map(|i| builder::line(&vertices[i], &vertices[(i + 1) % vertices.len()]))
        .collect()
}

fn circle_wire(frame: &Frame, center: (f64, f64), radius: f64) -> Wire {
    let origin = frame.at(center.0, center.1);
    primitive::circle(origin + frame.x * radius, origin, frame.normal, 4)
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

    pub fn interior_point(&self) -> (f64, f64) {
        match self {
            Profile::Rect { center, .. } | Profile::Circle { center, .. } => *center,
            Profile::Polygon { points } => {
                let n = points.len() as f64;
                let (sx, sy) = points
                    .iter()
                    .fold((0.0, 0.0), |(x, y), p| (x + p.0, y + p.1));
                (sx / n, sy / n)
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
        .for_each(|shell| mesh.merge(shell.triangulation(tolerance).to_polygon()));
    mesh
}

pub fn mesh_tolerance(solid: &Solid) -> f64 {
    (bounds(solid).diameter() * 2.0e-4).max(1.0e-4)
}

pub fn volume(solid: &Solid) -> f64 {
    mesh(solid, mesh_tolerance(solid)).volume()
}
