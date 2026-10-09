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
    Spline {
        points: Vec<Point3>,
    },
}

fn spline_curve(points: &[Point3]) -> Result<BsplineCurve<Point3>> {
    let mut params = vec![0.0];
    for pair in points.windows(2) {
        params.push(params.last().expect("non-empty") + pair[0].distance(pair[1]));
    }
    let total = *params.last().expect("non-empty");
    params.iter_mut().for_each(|t| *t /= total);
    let degree = 3.min(points.len() - 1);
    let n = points.len();
    let mut knots = vec![0.0; degree + 1];
    knots
        .extend((1..n - degree).map(|j| params[j..j + degree].iter().sum::<f64>() / degree as f64));
    knots.extend(vec![1.0; degree + 1]);
    let pairs: Vec<(f64, Point3)> = params.into_iter().zip(points.iter().copied()).collect();
    BsplineCurve::try_interpolate(KnotVector::from(knots), pairs)
        .map_err(|e| anyhow!("smooth path: {e}"))
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
            SweepPath::Spline { points } => {
                let curve = spline_curve(points)?;
                let dense: Vec<Point3> = (0..=512).map(|i| curve.subs(i as f64 / 512.0)).collect();
                let lengths: Vec<f64> = std::iter::once(0.0)
                    .chain(dense.windows(2).scan(0.0, |sum, pair| {
                        *sum += pair[0].distance(pair[1]);
                        Some(*sum)
                    }))
                    .collect();
                let target = fraction.clamp(0.0, 1.0) * lengths[512];
                let k = lengths
                    .windows(2)
                    .position(|w| w[1] >= target)
                    .unwrap_or(511);
                let t = (k as f64
                    + ((target - lengths[k]) / (lengths[k + 1] - lengths[k]).max(1.0e-300))
                        .clamp(0.0, 1.0))
                    / 512.0;
                Ok((curve.subs(t), curve.der(t).normalize()))
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
        let smooth = args.rest.last() == Some(&"smooth");
        let points = args
            .rest
            .iter()
            .filter(|text| **text != "smooth")
            .map(|text| point3(text, &self.scope))
            .collect::<Result<Vec<_>>>()?;
        if points.len() < 2 {
            bail!("`path` needs at least two x,y,z points");
        }
        if smooth {
            if args.has("r") {
                bail!("a `smooth` path has no corners to bend; drop `r=`");
            }
            let curve = spline_curve(&points)?;
            let length: f64 = (0..256)
                .map(|i| {
                    curve
                        .subs(i as f64 / 256.0)
                        .distance(curve.subs((i + 1) as f64 / 256.0))
                })
                .sum();
            self.path = Some(SweepPath::Spline { points });
            return Ok(format!("smooth path {length:.3} long"));
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
        let args = Args::new(line, &[], &["mode", "twist", "scale"], false)?;
        let combine = super::args::combine_mode(&args)?;
        let path = self
            .path
            .clone()
            .ok_or_else(|| anyhow!("`sweep` needs a `path` or `helix` first"))?;
        let twist = args.optional_number("twist", &self.scope)?.unwrap_or(0.0);
        let scale = positive(
            args.optional_number("scale", &self.scope)?.unwrap_or(1.0),
            "scale",
        )?;
        let (sketch, profiles) = self.take_sketch("sweep")?;
        let label = label_of(line);
        let (start_point, _) = path.at(0.0)?;
        let (end_point, _) = path.at(1.0)?;
        let shaped = twist != 0.0 || scale != 1.0 || matches!(path, SweepPath::Spline { .. });
        let tools = match path {
            path if shaped => skinned_sweep(&path, &sketch, &profiles, twist, scale)?,
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
            SweepPath::Spline { .. } => unreachable!("smooth paths are skinned"),
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
        self.place_swept(&label, tools, start_point, end_point, combine)
    }

    pub(crate) fn place_swept(
        &mut self,
        label: &str,
        tools: Vec<Solid>,
        start_point: Point3,
        end_point: Point3,
        combine: super::Combine,
    ) -> Result<String> {
        let tolerance = self.tolerance() * 100.0;
        for tool in tools {
            let groups = select::faces(&tool)
                .iter()
                .map(|face| {
                    let surface = face.oriented_surface();
                    let on = |p: Point3| match &surface {
                        Surface::Plane(plane) => {
                            (p - plane.origin()).dot(plane.normal()).abs() < tolerance
                        }
                        _ => false,
                    };
                    let group = if on(start_point) {
                        "start"
                    } else if on(end_point) {
                        "end"
                    } else {
                        "side"
                    };
                    (group, surface)
                })
                .collect();
            self.record_for(label, groups, combine);
            self.merge(label, tool, combine)?;
        }
        self.describe_solid()
    }
}

fn skinned_sweep(
    path: &SweepPath,
    sketch: &Frame,
    profiles: &[crate::geometry::Profile],
    twist: f64,
    scale: f64,
) -> Result<Vec<Solid>> {
    let curved = !matches!(path, SweepPath::Polyline { points, .. } if points.len() == 2);
    let stations = [
        2,
        (twist.abs() / 10.0).ceil() as usize + 1,
        if curved { 33 } else { 2 },
        if scale != 1.0 { 5 } else { 2 },
    ]
    .into_iter()
    .max()
    .unwrap_or(2);
    let (origin, tangent) = path.at(0.0)?;
    let mut frame = start_frame(sketch, origin, tangent);
    let mut frames = Vec::new();
    for k in 0..stations {
        let f = k as f64 / (stations - 1) as f64;
        let (point, tangent) = path.at(f)?;
        let turn = frame.normal.cross(tangent);
        let x = if turn.magnitude() > 1.0e-12 {
            let angle = frame.normal.dot(tangent).clamp(-1.0, 1.0).acos();
            Matrix3::from_axis_angle(turn.normalize(), Rad(angle)) * frame.x
        } else {
            frame.x
        };
        let x = (x - tangent * x.dot(tangent)).normalize();
        frame = Frame {
            origin: point,
            x,
            y: tangent.cross(x),
            normal: tangent,
        };
        let spin = Matrix3::from_axis_angle(tangent, Deg(twist * f));
        let size = 1.0 + (scale - 1.0) * f;
        frames.push(Frame {
            origin: point,
            x: spin * frame.x * size,
            y: spin * frame.y * size,
            normal: tangent,
        });
    }
    skin_frames(&frames, profiles)
}

pub(crate) fn skin_frames(
    frames: &[Frame],
    profiles: &[crate::geometry::Profile],
) -> Result<Vec<Solid>> {
    let stations = frames.len();
    let shapes = loops(&frames[0], profiles)?;
    let at_station = |k: usize, index: usize| -> Result<Wire> {
        let loops = loops(&frames[k], profiles)?;
        Ok(loops[index].wire.clone())
    };
    regions(&shapes)
        .iter()
        .map(|region| {
            let skin_of = |index: usize| -> Result<Solid> {
                let wires = (0..stations)
                    .map(|k| at_station(k, index))
                    .collect::<Result<Vec<_>>>()?;
                super::skin::skin(&wires, true)
            };
            let mut solid = skin_of(region[0])?;
            for &hole in &region[1..] {
                solid = monstertruck::solid::difference_normalized(&solid, &skin_of(hole)?)
                    .map_err(|e| anyhow!("cutting a hole through the sweep: {e}"))?;
            }
            Ok(solid)
        })
        .collect()
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
