use super::Model;
use super::args::{Args, describe_point, label_of, point3, positive};
use super::solids::{loft_wires, loops, oriented, regions};
use crate::geometry::Frame;
use crate::parse::Line;
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

#[derive(Clone, Debug)]
pub enum SweepPath {
    Polyline {
        points: Vec<Point3>,
        bend: f64,
    },
    Helix {
        frame: Frame,
        radius: f64,
        pitch: f64,
        turns: f64,
    },
}

enum Piece {
    Straight(Vector3),
    Bend {
        center: Point3,
        axis: Vector3,
        angle: f64,
    },
}

fn pieces(points: &[Point3], bend: f64) -> Result<(Vector3, Vec<Piece>)> {
    let directions: Vec<Vector3> = points.windows(2).map(|pair| pair[1] - pair[0]).collect();
    if directions.iter().any(|d| d.magnitude() < 1.0e-9) {
        bail!("path has two identical points in a row");
    }
    let corners: Vec<Option<(f64, Piece)>> = (1..points.len() - 1)
        .map(|i| {
            let (a, b) = (directions[i - 1].normalize(), directions[i].normalize());
            let angle = a.dot(b).clamp(-1.0, 1.0).acos();
            if angle < 1.0e-9 {
                return Ok(None);
            }
            if bend <= 0.0 {
                bail!(
                    "path turns at point {} ({}); give the bend radius with r=",
                    i + 1,
                    describe_point(points[i])
                );
            }
            if angle > std::f64::consts::PI - 1.0e-6 {
                bail!("path doubles back on itself at point {}", i + 1);
            }
            let setback = bend * (angle / 2.0).tan();
            let start = points[i] - a * setback;
            let inward = (b - a * a.dot(b)).normalize();
            Ok(Some((
                setback,
                Piece::Bend {
                    center: start + inward * bend,
                    axis: a.cross(b).normalize(),
                    angle,
                },
            )))
        })
        .collect::<Result<_>>()?;
    let setbacks: Vec<f64> = std::iter::once(0.0)
        .chain(
            corners
                .iter()
                .map(|corner| corner.as_ref().map_or(0.0, |(s, _)| *s)),
        )
        .chain(std::iter::once(0.0))
        .collect();
    let mut result = Vec::new();
    let mut corners = corners.into_iter();
    for (i, direction) in directions.iter().enumerate() {
        let length = direction.magnitude() - setbacks[i] - setbacks[i + 1];
        if length < -1.0e-9 {
            bail!(
                "bend radius r={bend} does not fit the segment from point {} to {}",
                i + 1,
                i + 2
            );
        }
        if length > 1.0e-9 {
            result.push(Piece::Straight(direction.normalize() * length));
        }
        if let Some(Some((_, bend))) = corners.next() {
            result.push(bend);
        }
    }
    Ok((directions[0].normalize(), result))
}

fn helix_frames(frame: &Frame, radius: f64, pitch: f64, turns: f64) -> Vec<Frame> {
    let stations = ((turns * 72.0).ceil() as usize).max(2);
    (0..=stations)
        .map(|i| {
            let t = i as f64 / stations as f64;
            let angle = t * turns * std::f64::consts::TAU;
            let radial = frame.x * angle.cos() + frame.y * angle.sin();
            let around = -frame.x * angle.sin() + frame.y * angle.cos();
            let origin = frame.origin + radial * radius + frame.normal * (pitch * turns * t);
            let tangent =
                (around * (radius * std::f64::consts::TAU) + frame.normal * pitch).normalize();
            Frame {
                origin,
                x: radial,
                y: tangent.cross(radial),
                normal: tangent,
            }
        })
        .collect()
}

fn start_frame(sketch: &Frame, origin: Point3, direction: Vector3) -> Frame {
    let projected = sketch.x - direction * sketch.x.dot(direction);
    let x = if projected.magnitude() > 1.0e-6 {
        projected.normalize()
    } else {
        (sketch.y - direction * sketch.y.dot(direction)).normalize()
    };
    Frame {
        origin,
        x,
        y: direction.cross(x),
        normal: direction,
    }
}

impl SweepPath {
    pub fn at(&self, fraction: f64) -> Result<(Point3, Vector3)> {
        match self {
            SweepPath::Helix {
                frame,
                radius,
                pitch,
                turns,
            } => {
                let angle = fraction * turns * std::f64::consts::TAU;
                let radial = frame.x * angle.cos() + frame.y * angle.sin();
                let around = -frame.x * angle.sin() + frame.y * angle.cos();
                let tangent =
                    (around * (radius * std::f64::consts::TAU) + frame.normal * *pitch).normalize();
                Ok((
                    frame.origin + radial * *radius + frame.normal * (pitch * turns * fraction),
                    tangent,
                ))
            }
            SweepPath::Polyline { points, bend } => {
                let (first, parts) = pieces(points, *bend)?;
                let length = |piece: &Piece| match piece {
                    Piece::Straight(v) => v.magnitude(),
                    Piece::Bend { angle, .. } => bend * angle,
                };
                let total: f64 = parts.iter().map(length).sum();
                let mut remaining = fraction.clamp(0.0, 1.0) * total;
                let (mut position, mut tangent) = (points[0], first);
                for (i, piece) in parts.iter().enumerate() {
                    let size = length(piece);
                    let last = i + 1 == parts.len();
                    let t = if remaining >= size && !last {
                        1.0
                    } else {
                        (remaining / size).min(1.0)
                    };
                    match piece {
                        Piece::Straight(v) => {
                            position += *v * t;
                            tangent = v.normalize();
                        }
                        Piece::Bend { axis, angle, .. } => {
                            let centre = position + axis.cross(tangent).normalize() * *bend;
                            let turn = Matrix3::from_axis_angle(*axis, Rad(angle * t));
                            position = centre + turn * (position - centre);
                            tangent = turn * tangent;
                        }
                    }
                    remaining -= size;
                    if remaining <= 0.0 {
                        break;
                    }
                }
                Ok((position, tangent))
            }
        }
    }
}

impl Model {
    pub(crate) fn op_path(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &["r"], true)?;
        let points = args
            .rest
            .iter()
            .map(|text| point3(text, &self.scope))
            .collect::<Result<Vec<_>>>()?;
        if points.len() < 2 {
            bail!("`path` needs at least two x,y,z points");
        }
        let bend = args.optional_number("r", &self.scope)?.unwrap_or(0.0);
        let (_, parts) = pieces(&points, bend)?;
        let bends = parts
            .iter()
            .filter(|piece| matches!(piece, Piece::Bend { .. }))
            .count();
        self.path = Some(SweepPath::Polyline { points, bend });
        Ok(format!(
            "path of {} straight and {bends} bent segment(s)",
            parts.len() - bends
        ))
    }

    pub(crate) fn op_helix(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &["r", "pitch", "turns", "at"], false)?;
        let radius = positive(args.number("r", &self.scope)?, "r")?;
        let pitch = positive(args.number("pitch", &self.scope)?, "pitch")?;
        let turns = positive(args.number("turns", &self.scope)?, "turns")?;
        let plane = self.sketch_frame();
        let (u, v) = args.point("at", &self.scope)?;
        let frame = Frame {
            origin: plane.at(u, v),
            ..plane
        };
        self.path = Some(SweepPath::Helix {
            frame,
            radius,
            pitch,
            turns,
        });
        Ok(format!(
            "helix of {turns} turn(s), {:.3} long",
            turns * ((radius * std::f64::consts::TAU).powi(2) + pitch * pitch).sqrt()
        ))
    }

    pub(crate) fn op_sweep(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &["mode"], false)?;
        let combine = super::args::combine_mode(&args)?;
        let path = self
            .path
            .clone()
            .ok_or_else(|| anyhow!("`sweep` needs a `path` or `helix` first"))?;
        let (sketch, profiles) = self.take_sketch("sweep")?;
        let label = label_of(line);
        let tools = match path {
            SweepPath::Polyline { points, bend } => {
                let (direction, parts) = pieces(&points, bend)?;
                let frame = start_frame(&sketch, points[0], direction);
                let shapes = loops(&frame, &profiles)?;
                regions(&shapes)
                    .iter()
                    .map(|region| {
                        let wires: Vec<Wire> =
                            region.iter().map(|&i| shapes[i].wire.clone()).collect();
                        sweep_face(wires, direction, &parts)
                    })
                    .collect::<Result<Vec<_>>>()?
            }
            SweepPath::Helix {
                frame,
                radius,
                pitch,
                turns,
            } => {
                let frames = helix_frames(&frame, radius, pitch, turns);
                let shapes = loops(&frames[0], &profiles)?;
                if shapes.len() != 1 {
                    bail!("a helix sweep takes one profile without holes");
                }
                let wires = frames
                    .iter()
                    .map(|f| profiles[0].wires(f).map(|mut w| w.remove(0)))
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| anyhow!(e))?;
                vec![loft_wires(&wires)?]
            }
        };
        for tool in tools {
            let faces = select::faces(&tool);
            let count = faces.len();
            let groups = faces
                .iter()
                .enumerate()
                .map(|(i, face)| {
                    (
                        if i == 0 {
                            "start"
                        } else if i + 1 == count {
                            "end"
                        } else {
                            "side"
                        },
                        face.oriented_surface(),
                    )
                })
                .collect();
            self.record_for(&label, groups, combine);
            self.merge(&label, tool, combine)?;
        }
        self.describe_solid()
    }
}

fn sweep_face(wires: Vec<Wire>, direction: Vector3, parts: &[Piece]) -> Result<Solid> {
    let mut face: Face = profile::attach_plane_normalized(wires)
        .map_err(|error| anyhow!("cannot face the sketch: {error}"))?;
    if let Surface::Plane(plane) = face.oriented_surface()
        && plane.normal().dot(direction) < 0.0
    {
        face.invert();
    }
    let mut faces: Vec<Face> = vec![face.inverse()];
    for part in parts {
        let swept: Solid = match part {
            Piece::Straight(vector) => builder::extrude(&face, *vector),
            Piece::Bend {
                center,
                axis,
                angle,
            } => {
                let division = ((angle / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
                builder::revolve(
                    &face,
                    *center,
                    *axis,
                    builder::SweepAngle::Partial(Rad(*angle)),
                    division,
                )
            }
        };
        let shell = &swept.boundaries()[0];
        let count = shell.len();
        faces.extend(shell.face_iter().skip(1).take(count - 2).cloned());
        face = shell[count - 1].clone();
    }
    faces.push(face);
    let shell: Shell = faces.into();
    let solid = Solid::try_new(vec![shell])
        .map_err(|error| anyhow!("swept solid is not closed: {error}"))?;
    Ok(oriented(solid))
}
