use super::args::{Args, positive};
use super::{Model, Pen};
use crate::geometry::{Frame, Profile, Segment};
use crate::parse::{Line, eval, eval_point};
use crate::select;
use anyhow::{Context, Result, anyhow, bail};
use monstertruck::modeling::*;

pub(crate) fn describe_frame(frame: &Frame) -> String {
    let f = |v: Vector3| {
        let v = v.map(|c| if c.abs() < 5.0e-4 { 0.0 } else { c });
        format!("({:.3}, {:.3}, {:.3})", v.x, v.y, v.z)
    };
    format!(
        "plane at {} normal {} x {} y {}",
        f(frame.origin.to_vec()),
        f(frame.normal),
        f(frame.x),
        f(frame.y)
    )
}

fn rotate(frame: Frame, axis: Vector3, degrees: f64) -> Frame {
    let rotation = Matrix3::from_axis_angle(axis.normalize(), Deg(degrees));
    Frame {
        origin: frame.origin,
        x: rotation * frame.x,
        y: rotation * frame.y,
        normal: rotation * frame.normal,
    }
}

fn arc_middle(
    from: (f64, f64),
    to: (f64, f64),
    centre: (f64, f64),
    clockwise: bool,
) -> Result<(f64, f64)> {
    let (a, b) = (
        (from.0 - centre.0, from.1 - centre.1),
        (to.0 - centre.0, to.1 - centre.1),
    );
    let (ra, rb) = (
        (a.0 * a.0 + a.1 * a.1).sqrt(),
        (b.0 * b.0 + b.1 * b.1).sqrt(),
    );
    if (ra - rb).abs() > 1.0e-6 * ra.max(1.0) {
        bail!("the arc's ends are {ra:.3} and {rb:.3} from its centre; they must be the same");
    }
    let start = a.1.atan2(a.0);
    let mut sweep = b.1.atan2(b.0) - start;
    if clockwise {
        sweep = -((-sweep).rem_euclid(std::f64::consts::TAU));
    } else {
        sweep = sweep.rem_euclid(std::f64::consts::TAU);
    }
    if sweep.abs() < 1.0e-9 {
        bail!("the arc's ends are the same point");
    }
    let middle = start + sweep / 2.0;
    Ok((centre.0 + ra * middle.cos(), centre.1 + ra * middle.sin()))
}

fn cornered(points: &[(f64, f64)], size: f64, round: bool) -> Result<Profile> {
    let n = points.len();
    let unit = |from: (f64, f64), to: (f64, f64)| {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let length = (dx * dx + dy * dy).sqrt();
        ((dx / length, dy / length), length)
    };
    let corners: Vec<((f64, f64), (f64, f64), Option<(f64, f64)>, f64)> = (0..n)
        .map(|i| {
            let (v, previous, next) = (points[i], points[(i + n - 1) % n], points[(i + 1) % n]);
            let ((ax, ay), _) = unit(v, previous);
            let ((bx, by), _) = unit(v, next);
            let angle = (ax * bx + ay * by).clamp(-1.0, 1.0).acos();
            let setback = if round {
                size / (angle / 2.0).tan()
            } else {
                size
            };
            let enter = (v.0 + ax * setback, v.1 + ay * setback);
            let leave = (v.0 + bx * setback, v.1 + by * setback);
            let via = round.then(|| {
                let (mx, my) = (ax + bx, ay + by);
                let m = (mx * mx + my * my).sqrt();
                let to_centre = size / (angle / 2.0).sin();
                let centre = (v.0 + mx / m * to_centre, v.1 + my / m * to_centre);
                (centre.0 - mx / m * size, centre.1 - my / m * size)
            });
            (enter, leave, via, setback)
        })
        .collect();
    for i in 0..n {
        let (_, length) = unit(points[i], points[(i + 1) % n]);
        if corners[i].3 + corners[(i + 1) % n].3 > length + 1.0e-9 {
            bail!(
                "corner size {size} does not fit the side from point {} to {}",
                i + 1,
                (i + 1) % n + 1
            );
        }
    }
    let start = corners[0].1;
    let mut segments = Vec::new();
    let mut cursor = start;
    for i in 1..=n {
        let (enter, leave, via, _) = corners[i % n];
        if (enter.0 - cursor.0).hypot(enter.1 - cursor.1) > 1.0e-9 {
            segments.push(Segment::Line(enter));
        }
        cursor = leave;
        segments.push(match via {
            Some(via) => Segment::Arc { to: leave, via },
            None => Segment::Line(leave),
        });
    }
    Ok(Profile::Path { start, segments })
}

impl Model {
    pub(crate) fn op_let(&mut self, line: &Line) -> Result<String> {
        if !line.positional.is_empty() {
            bail!("`let` takes name=value pairs only");
        }
        line.named.iter().try_for_each(|(name, text)| {
            let value = match self.fixed.get(name) {
                Some(fixed) => *fixed,
                None => eval(text, &self.scope).with_context(|| format!("`{name}`"))?,
            };
            self.scope.insert(name.clone(), value);
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(line
            .named
            .iter()
            .map(|(name, _)| format!("{name}={}", self.scope[name]))
            .collect::<Vec<_>>()
            .join(" "))
    }

    pub(crate) fn sketch_frame(&self) -> Frame {
        self.frame
            .unwrap_or_else(|| Frame::named("XY").expect("XY is a named plane"))
    }

    pub(crate) fn require_empty_sketch(&self, op: &str) -> Result<()> {
        if self.pen.is_some() {
            bail!("a `pen` path is still open; `close` it before `{op}`");
        }
        if !self.sketch.is_empty() {
            bail!(
                "the sketch has {} profile(s); extrude or cut it before `{op}`",
                self.sketch.len()
            );
        }
        Ok(())
    }

    pub(crate) fn plane_from(&self, on: &str) -> Result<Frame> {
        match Frame::named(on) {
            Some(frame) => Ok(frame),
            None => self.face_frame(on),
        }
    }

    pub(crate) fn op_plane(&mut self, line: &Line) -> Result<String> {
        self.require_empty_sketch("plane")
            .map_err(|e| anyhow!("{e}; extrude or cut it before moving the plane"))?;
        let args = Args::new(line, &["on"], &["offset", "rx", "ry", "rz"], true)?;
        let mut frame = match args.rest.as_slice() {
            [] => self.plane_from(args.text("on")?)?,
            [second, third] => {
                let points = [args.text("on")?, second, third]
                    .map(|text| super::args::point3(text, &self.scope));
                let [a, b, c] = [
                    points[0].as_ref().map_err(|e| anyhow!("{e:#}"))?,
                    points[1].as_ref().map_err(|e| anyhow!("{e:#}"))?,
                    points[2].as_ref().map_err(|e| anyhow!("{e:#}"))?,
                ];
                let x = *b - *a;
                let normal = x.cross(*c - *a);
                if normal.magnitude() < 1.0e-12 {
                    bail!("the three points are in a line");
                }
                let (x, normal) = (x.normalize(), normal.normalize());
                Frame {
                    origin: *a,
                    x,
                    y: normal.cross(x),
                    normal,
                }
            }
            _ => bail!("`plane` takes a name, a face selector or three x,y,z points"),
        };
        frame = frame.offset(args.optional_number("offset", &self.scope)?.unwrap_or(0.0));
        for (name, axis) in [("rx", frame.x), ("ry", frame.y), ("rz", frame.normal)] {
            if let Some(degrees) = args.optional_number(name, &self.scope)? {
                frame = rotate(frame, axis, degrees);
            }
        }
        self.frame = Some(frame);
        Ok(describe_frame(&frame))
    }

    pub(crate) fn face_frame(&self, selector: &str) -> Result<Frame> {
        let solid = self.solid.as_ref().ok_or_else(|| {
            anyhow!("`{selector}` is not XY, XZ or YZ and there is no solid to select a face on")
        })?;
        let indices = select::select_faces(selector, solid, &self.groups, self.tolerance())?;
        let faces = select::faces(solid);
        let planes: Vec<Plane> = indices
            .iter()
            .map(|&i| match faces[i].oriented_surface() {
                Surface::Plane(plane) => Ok(plane),
                _ => bail!("`{selector}` includes a curved face; a plane needs flat faces"),
            })
            .collect::<Result<_>>()?;
        let first = planes
            .first()
            .ok_or_else(|| anyhow!("`{selector}` matched no faces"))?;
        let normal = first.normal();
        let offset = first.origin().to_vec().dot(normal);
        if planes.iter().any(|plane| {
            plane.normal().dot(normal) < 1.0 - 1.0e-9
                || (plane.origin().to_vec().dot(normal) - offset).abs() > self.tolerance()
        }) {
            bail!(
                "`{selector}` matched {} faces that are not coplanar",
                planes.len()
            );
        }
        Ok(Frame::from_normal(
            Point3::from_vec(normal * offset),
            normal,
        ))
    }

    fn add_profile(&mut self, profile: Profile) -> Result<String> {
        if self.pen.is_some() {
            bail!("a `pen` path is still open; `close` it first");
        }
        self.sketch.push(profile);
        Ok(format!("sketch has {} profile(s)", self.sketch.len()))
    }

    pub(crate) fn op_rect(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["w", "h"], &["at", "r"], false)?;
        let width = positive(args.number("w", &self.scope)?, "w")?;
        let height = positive(args.number("h", &self.scope)?, "h")?;
        let radius = args.optional_number("r", &self.scope)?.unwrap_or(0.0);
        if radius < 0.0 || radius * 2.0 >= width.min(height) {
            bail!(
                "r={radius} must be at least 0 and under half the shorter side ({})",
                width.min(height) / 2.0
            );
        }
        let center = args.point("at", &self.scope)?;
        self.add_profile(Profile::Rect {
            center,
            width,
            height,
            radius,
        })
    }

    pub(crate) fn op_circle(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["d"], &["at"], false)?;
        let diameter = positive(args.number("d", &self.scope)?, "d")?;
        let center = args.point("at", &self.scope)?;
        self.add_profile(Profile::Circle { center, diameter })
    }

    pub(crate) fn op_poly(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &["r", "c"], true)?;
        let points = args
            .rest
            .iter()
            .map(|text| eval_point(text, &self.scope))
            .collect::<Result<Vec<_>>>()?;
        if points.len() < 3 {
            bail!("`poly` needs at least three x,y points");
        }
        let corner = match (
            args.optional_number("r", &self.scope)?,
            args.optional_number("c", &self.scope)?,
        ) {
            (Some(_), Some(_)) => bail!("give `r=` or `c=`, not both"),
            (Some(r), None) => Some((positive(r, "r")?, true)),
            (None, Some(c)) => Some((positive(c, "c")?, false)),
            (None, None) => None,
        };
        match corner {
            None => self.add_profile(Profile::Polygon { points }),
            Some((size, round)) => {
                let path = cornered(&points, size, round)?;
                self.add_profile(path)
            }
        }
    }

    pub(crate) fn op_ngon(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["d", "n"], &["at", "angle"], false)?;
        let radius = positive(args.number("d", &self.scope)?, "d")? / 2.0;
        let sides = args.number("n", &self.scope)?;
        if sides < 3.0 || sides.fract() != 0.0 {
            bail!("n must be a whole number of at least 3, got {sides}");
        }
        let (cx, cy) = args.point("at", &self.scope)?;
        let start = args
            .optional_number("angle", &self.scope)?
            .unwrap_or(0.0)
            .to_radians();
        let points = (0..sides as usize)
            .map(|i| {
                let a = start + i as f64 / sides * std::f64::consts::TAU;
                (cx + radius * a.cos(), cy + radius * a.sin())
            })
            .collect();
        self.add_profile(Profile::Polygon { points })
    }

    pub(crate) fn op_slot(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["l", "w"], &["at", "angle"], false)?;
        let length = positive(args.number("l", &self.scope)?, "l")?;
        let width = positive(args.number("w", &self.scope)?, "w")?;
        if length <= width {
            bail!("slot length {length} must be longer than its width {width}");
        }
        let center = args.point("at", &self.scope)?;
        let angle = args.optional_number("angle", &self.scope)?.unwrap_or(0.0);
        self.add_profile(Profile::Slot {
            center,
            length,
            width,
            angle,
        })
    }

    pub(crate) fn op_ellipse(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["dx", "dy"], &["at"], false)?;
        let rx = positive(args.number("dx", &self.scope)?, "dx")? / 2.0;
        let ry = positive(args.number("dy", &self.scope)?, "dy")? / 2.0;
        let center = args.point("at", &self.scope)?;
        self.add_profile(Profile::Ellipse { center, rx, ry })
    }

    pub(crate) fn op_pen(&mut self, line: &Line) -> Result<String> {
        if self.pen.is_some() {
            bail!("a `pen` path is already open; `close` it first");
        }
        let args = Args::new(line, &["at"], &[], false)?;
        let start = eval_point(args.text("at")?, &self.scope)?;
        self.pen = Some(Pen {
            start,
            cursor: start,
            segments: Vec::new(),
        });
        Ok(format!("pen at {:.3},{:.3}", start.0, start.1))
    }

    fn pen_mut(&mut self, op: &str) -> Result<&mut Pen> {
        self.pen
            .as_mut()
            .ok_or_else(|| anyhow!("`{op}` needs a `pen x,y` first"))
    }

    pub(crate) fn op_line(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["to"], &[], false)?;
        let to = eval_point(args.text("to")?, &self.scope)?;
        let pen = self.pen_mut("line")?;
        pen.segments.push(Segment::Line(to));
        pen.cursor = to;
        Ok(format!("path has {} segment(s)", pen.segments.len()))
    }

    pub(crate) fn op_arc(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["to", "turn"], &["via", "center"], false)?;
        let to = eval_point(args.text("to")?, &self.scope)?;
        let cursor = self
            .pen
            .as_ref()
            .map(|pen| pen.cursor)
            .ok_or_else(|| anyhow!("`arc` needs a `pen x,y` first"))?;
        let via = match (args.values.get("via"), args.values.get("center")) {
            (Some(text), None) => eval_point(text, &self.scope)?,
            (None, Some(text)) => {
                let centre = eval_point(text, &self.scope)?;
                let clockwise = match args.values.get("turn").copied() {
                    None | Some("ccw") => false,
                    Some("cw") => true,
                    Some(other) => bail!("the arc turns `cw` or `ccw`, got `{other}`"),
                };
                arc_middle(cursor, to, centre, clockwise)?
            }
            _ => bail!("`arc` needs `via=x,y` or `center=x,y`"),
        };
        let pen = self.pen_mut("arc")?;
        let (a, b) = (
            (via.0 - pen.cursor.0, via.1 - pen.cursor.1),
            (to.0 - pen.cursor.0, to.1 - pen.cursor.1),
        );
        if (a.0 * b.1 - a.1 * b.0).abs() < 1.0e-12 {
            bail!("the arc's via point is in line with its ends");
        }
        pen.segments.push(Segment::Arc { to, via });
        pen.cursor = to;
        Ok(format!("path has {} segment(s)", pen.segments.len()))
    }

    pub(crate) fn op_close(&mut self, line: &Line) -> Result<String> {
        Args::new(line, &[], &[], false)?;
        let mut pen = self
            .pen
            .take()
            .ok_or_else(|| anyhow!("`close` needs a `pen x,y` first"))?;
        let (dx, dy) = (pen.cursor.0 - pen.start.0, pen.cursor.1 - pen.start.1);
        if (dx * dx + dy * dy).sqrt() > 1.0e-9 {
            pen.segments.push(Segment::Line(pen.start));
        }
        if pen.segments.len() < 2 {
            bail!("a closed path needs at least two segments");
        }
        self.add_profile(Profile::Path {
            start: pen.start,
            segments: pen.segments,
        })
    }

    pub(crate) fn op_spline(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &[], true)?;
        let (closed, points): (Vec<&str>, Vec<&str>) =
            args.rest.iter().partition(|word| **word == "closed");
        if closed.is_empty() {
            bail!("only closed splines make a profile; end the points with `closed`");
        }
        let points = points
            .iter()
            .map(|text| eval_point(text, &self.scope))
            .collect::<Result<Vec<_>>>()?;
        if points.len() < 3 {
            bail!("`spline` needs at least three x,y points");
        }
        self.add_profile(Profile::Spline { points })
    }

    pub(crate) fn op_text(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["text"], &["size", "at"], false)?;
        let text = args.text("text")?.to_string();
        let size = positive(
            args.optional_number("size", &self.scope)?.unwrap_or(10.0),
            "size",
        )?;
        let at = args.point("at", &self.scope)?;
        self.add_profile(Profile::Text { text, size, at })
    }

    pub(crate) fn op_offset(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["d"], &[], false)?;
        let distance = args.number("d", &self.scope)?;
        let last = self
            .sketch
            .last()
            .ok_or_else(|| anyhow!("`offset` copies the last profile and there is none"))?;
        let grown = last
            .inset(-distance)
            .map_err(|error| anyhow!("offset: {error}"))?;
        self.add_profile(grown)
    }

    pub(crate) fn take_sketch(&mut self, op: &str) -> Result<(Frame, Vec<Profile>)> {
        if self.pen.is_some() {
            bail!("a `pen` path is still open; `close` it before `{op}`");
        }
        if self.sketch.is_empty() {
            bail!("`{op}` needs a sketch; add rect, circle, poly or another profile first");
        }
        Ok((self.sketch_frame(), std::mem::take(&mut self.sketch)))
    }

    pub(crate) fn op_section(&mut self, line: &Line) -> Result<String> {
        Args::new(line, &[], &[], false)?;
        let section = self.take_sketch("section")?;
        self.sections.push(section);
        Ok(format!("{} section(s) for `loft`", self.sections.len()))
    }
}
