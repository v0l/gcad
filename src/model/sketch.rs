use super::args::{Args, positive};
use super::{Model, Pen};
use crate::geometry::{Frame, Profile, Segment, compose_2d};
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
    type Corner = ((f64, f64), (f64, f64), Option<(f64, f64)>, f64);
    let corners: Vec<Corner> = (0..n)
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

    pub(crate) fn op_axis(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["name", "from", "to"], &[], false)?;
        let name = args.text("name")?;
        if ["x", "y", "z"].contains(&name) {
            bail!("`{name}` is already an axis; pick another name");
        }
        let from = super::args::point3(args.text("from")?, &self.scope)?;
        let to = super::args::point3(args.text("to")?, &self.scope)?;
        let direction = to - from;
        if direction.magnitude() < 1.0e-12 {
            bail!("the axis needs two different points");
        }
        self.axes
            .insert(name.to_string(), (from, direction.normalize()));
        Ok(format!(
            "axis {name} through {} along {}",
            super::args::describe_point(from),
            super::args::describe_point(Point3::from_vec(direction.normalize()))
        ))
    }

    fn edge_plane(&self, selector: &str, degrees: f64) -> Result<Frame> {
        let solid = self.active("plane")?;
        let (first_set, _) = selector
            .split_once('&')
            .filter(|_| !selector.contains('|'))
            .ok_or_else(|| {
                anyhow!(
                    "write the edge as `a&b`; the plane starts on face `a` and turns toward `b`"
                )
            })?;
        let edges = select::select_edges(selector, solid, &self.groups, self.tolerance())?;
        let [edge] = edges.as_slice() else {
            bail!(
                "`{selector}` matched {} edges; the plane needs one",
                edges.len()
            );
        };
        let faces = select::faces(solid);
        let on_first = select::select_faces(first_set, solid, &self.groups, self.tolerance())?;
        let owners: Vec<usize> = (0..faces.len())
            .filter(|&i| faces[i].edge_iter().any(|e| e.is_same(edge)))
            .collect();
        let flat = |i: usize| match faces[i].oriented_surface() {
            Surface::Plane(plane) => Ok(plane.normal()),
            _ => bail!("the plane turns about an edge between flat faces"),
        };
        let (Some(&a), Some(&b)) = (
            owners.iter().find(|i| on_first.contains(i)),
            owners.iter().find(|i| !on_first.contains(i)),
        ) else {
            bail!("`{selector}` is not an edge of face `{first_set}`");
        };
        let (na, nb) = (flat(a)?, flat(b)?);
        let (p, q) = (edge.front().point(), edge.back().point());
        let curve = edge.curve();
        let (t0, t1) = curve.range_tuple();
        if (curve.subs((t0 + t1) / 2.0) - p.midpoint(q)).magnitude() > self.tolerance() * 10.0 {
            bail!("the plane turns about a straight edge; `{selector}` is curved");
        }
        let along = (q - p).normalize();
        let mut toward = along.cross(na);
        if toward.dot(nb) < 0.0 {
            toward = -toward;
        }
        let (s, c) = degrees.to_radians().sin_cos();
        let normal = na * c + toward * s;
        Ok(Frame {
            origin: p.midpoint(q),
            x: along,
            y: normal.cross(along),
            normal,
        })
    }

    pub(crate) fn op_plane(&mut self, line: &Line) -> Result<String> {
        self.require_empty_sketch("plane")?;
        if let Some(selector) = line
            .named
            .iter()
            .find(|(k, _)| k == "edge")
            .map(|(_, v)| v.clone())
        {
            let args = Args::new(line, &[], &["edge", "angle"], false)?;
            let degrees = args.optional_number("angle", &self.scope)?.unwrap_or(0.0);
            let frame = self.edge_plane(&selector, degrees)?;
            self.frame = Some(frame);
            return Ok(describe_frame(&frame));
        }
        if line.positional.first().map(String::as_str) == Some("path") {
            let args = Args::new(line, &["path"], &["at"], false)?;
            let fraction = args.optional_number("at", &self.scope)?.unwrap_or(0.0);
            if !(0.0..=1.0).contains(&fraction) {
                bail!("`at=` is a fraction of the path from 0 to 1, got {fraction}");
            }
            let path = self
                .path
                .as_ref()
                .ok_or_else(|| anyhow!("`plane path` needs a `path` or `helix` first"))?;
            let (origin, tangent) = path.at(fraction)?;
            let frame = Frame::from_normal(origin, tangent);
            self.frame = Some(frame);
            return Ok(describe_frame(&frame));
        }
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

    pub(crate) fn op_gear(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["teeth", "module"],
            &["at", "angle", "pressure", "backlash"],
            false,
        )?;
        let teeth = args.number("teeth", &self.scope)?;
        if teeth < 6.0 || teeth.fract() != 0.0 {
            bail!("teeth must be a whole number of at least 6, got {teeth}");
        }
        let module = positive(args.number("module", &self.scope)?, "module")?;
        let pressure = args
            .optional_number("pressure", &self.scope)?
            .unwrap_or(20.0);
        if !(5.0..=35.0).contains(&pressure) {
            bail!("pressure must be between 5 and 35 degrees, got {pressure}");
        }
        let backlash = args
            .optional_number("backlash", &self.scope)?
            .unwrap_or(0.0);
        let shape = GearShape::new(teeth as usize, module, pressure.to_radians(), backlash)?;
        let (cx, cy) = args.point("at", &self.scope)?;
        let start = args
            .optional_number("angle", &self.scope)?
            .unwrap_or(0.0)
            .to_radians();
        let (first, segments) = shape.outline(start, (cx, cy));
        let message = self.add_profile(Profile::Path {
            start: first,
            segments,
        })?;
        Ok(format!(
            "{message}; pitch diameter {:.3}, tip {:.3}, root {:.3}",
            shape.pitch * 2.0,
            shape.tip * 2.0,
            shape.root * 2.0
        ))
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
        let args = Args::new(line, &[], &[], true)?;
        let trim = match args.rest.as_slice() {
            [] => false,
            ["trim"] => true,
            _ => bail!("`close` takes nothing, or `trim` to cut the path where it crosses itself"),
        };
        if trim {
            let pen = self
                .pen
                .as_ref()
                .ok_or_else(|| anyhow!("`close` needs a `pen x,y` first"))?;
            let (start, segments) = trimmed(pen.start, &pen.segments)?;
            self.pen = None;
            return self.add_profile(Profile::Path { start, segments });
        }
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

    pub(crate) fn op_reflect(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &[], true)?;
        let (origin, (dx, dy)) = match args.rest.as_slice() {
            ["x"] => ((0.0, 0.0), (1.0, 0.0)),
            ["y"] => ((0.0, 0.0), (0.0, 1.0)),
            [a, b] => {
                let (a, b) = (eval_point(a, &self.scope)?, eval_point(b, &self.scope)?);
                let length = (b.0 - a.0).hypot(b.1 - a.1);
                if length < 1.0e-12 {
                    bail!("the two points of the mirror line are the same");
                }
                (a, ((b.0 - a.0) / length, (b.1 - a.1) / length))
            }
            _ => bail!("`reflect` takes `x`, `y` or two x,y points on the mirror line"),
        };
        let (a, b, d) = (2.0 * dx * dx - 1.0, 2.0 * dx * dy, 2.0 * dy * dy - 1.0);
        let matrix = [
            a,
            b,
            b,
            d,
            origin.0 - (a * origin.0 + b * origin.1),
            origin.1 - (b * origin.0 + d * origin.1),
        ];
        let last = self.last_profile("reflect")?;
        self.add_profile(placed(last, matrix))
    }

    pub(crate) fn op_array(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["count"], &["angle", "at", "step"], false)?;
        let count = args.number("count", &self.scope)?;
        if count < 2.0 || count.fract() != 0.0 {
            bail!("count must be a whole number of at least 2, got {count}");
        }
        let count = count as usize;
        let last = self.last_profile("array")?;
        let matrices: Vec<[f64; 6]> = match (
            args.optional_number("angle", &self.scope)?,
            args.optional_point("step", &self.scope)?,
        ) {
            (Some(angle), None) => {
                let (cx, cy) = args.point("at", &self.scope)?;
                let step = if (angle.abs() - 360.0).abs() < 1.0e-9 {
                    angle / count as f64
                } else {
                    angle / (count - 1) as f64
                };
                (1..count)
                    .map(|i| {
                        let (s, c) = (step * i as f64).to_radians().sin_cos();
                        [c, -s, s, c, cx - (c * cx - s * cy), cy - (s * cx + c * cy)]
                    })
                    .collect()
            }
            (None, Some((sx, sy))) => (1..count)
                .map(|i| [1.0, 0.0, 0.0, 1.0, sx * i as f64, sy * i as f64])
                .collect(),
            _ => bail!("`array` needs `angle=` (with optional `at=`) or `step=x,y`"),
        };
        for matrix in matrices {
            self.sketch.push(placed(last.clone(), matrix));
        }
        Ok(format!("sketch has {} profile(s)", self.sketch.len()))
    }

    pub(crate) fn op_drawing_file(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["file"], &["at", "scale"], false)?;
        let path = self.relative(args.text("file")?);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let profiles = if line.op == "dxf" {
            super::import::dxf_profiles(&text)?
        } else {
            super::import::svg_profiles(&text)?
        };
        let (x, y) = args.point("at", &self.scope)?;
        let scale = positive(
            args.optional_number("scale", &self.scope)?.unwrap_or(1.0),
            "scale",
        )?;
        let count = profiles.len();
        for profile in profiles {
            let placed = if scale == 1.0 && x == 0.0 && y == 0.0 {
                profile
            } else {
                placed(profile, [scale, 0.0, 0.0, scale, x, y])
            };
            self.add_profile(placed)?;
        }
        Ok(format!(
            "{count} outline(s) from {}; sketch has {} profile(s)",
            path.display(),
            self.sketch.len()
        ))
    }

    pub(crate) fn relative(&self, file: &str) -> std::path::PathBuf {
        let file = std::path::PathBuf::from(file);
        match &self.dir {
            Some(dir) if file.is_relative() => dir.join(&file),
            _ => file,
        }
    }

    fn last_profile(&self, op: &str) -> Result<Profile> {
        if self.pen.is_some() {
            bail!("a `pen` path is still open; `close` it before `{op}`");
        }
        self.sketch
            .last()
            .cloned()
            .ok_or_else(|| anyhow!("`{op}` copies the last profile and there is none"))
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

fn placed(profile: Profile, matrix: [f64; 6]) -> Profile {
    match profile {
        Profile::Placed { inner, matrix: own } => Profile::Placed {
            inner,
            matrix: compose_2d(matrix, own),
        },
        other => Profile::Placed {
            inner: Box::new(other),
            matrix,
        },
    }
}

fn crossing(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> Option<(f64, f64)> {
    let (r, s) = ((b.0 - a.0, b.1 - a.1), (d.0 - c.0, d.1 - c.1));
    let denominator = r.0 * s.1 - r.1 * s.0;
    if denominator.abs() < 1.0e-12 {
        return None;
    }
    let (qp0, qp1) = (c.0 - a.0, c.1 - a.1);
    let t = (qp0 * s.1 - qp1 * s.0) / denominator;
    let u = (qp0 * r.1 - qp1 * r.0) / denominator;
    let eps = 1.0e-9;
    ((-eps..=1.0 + eps).contains(&t) && (-eps..=1.0 + eps).contains(&u))
        .then_some((a.0 + r.0 * t, a.1 + r.1 * t))
}

fn trimmed(start: (f64, f64), segments: &[Segment]) -> Result<((f64, f64), Vec<Segment>)> {
    let ends: Vec<(f64, f64)> = std::iter::once(start)
        .chain(segments.iter().map(Segment::end))
        .collect();
    if segments.iter().any(|s| !matches!(s, Segment::Line(_))) {
        bail!("`close trim` works on paths of `line`s");
    }
    let n = segments.len();
    for last in (1..n).rev() {
        for first in 0..last.saturating_sub(1) {
            if let Some(meet) = crossing(ends[first], ends[first + 1], ends[last], ends[last + 1]) {
                let away = |p: &(f64, f64)| (p.0 - meet.0).hypot(p.1 - meet.1) > 1.0e-9;
                let kept: Vec<Segment> = ends[first + 1..=last]
                    .iter()
                    .filter(|p| away(p))
                    .chain(std::iter::once(&meet))
                    .map(|p| Segment::Line(*p))
                    .collect();
                if kept.len() < 3 {
                    bail!("the trimmed path has too few sides");
                }
                return Ok((meet, kept));
            }
        }
    }
    bail!("the path never crosses itself, so there is nothing to trim; use `close`")
}

struct GearShape {
    teeth: usize,
    pitch: f64,
    base: f64,
    tip: f64,
    root: f64,
    half_at_pitch: f64,
    pressure: f64,
}

fn involute(angle: f64) -> f64 {
    angle.tan() - angle
}

impl GearShape {
    fn new(teeth: usize, module: f64, pressure: f64, backlash: f64) -> Result<Self> {
        let pitch = module * teeth as f64 / 2.0;
        let shape = Self {
            teeth,
            pitch,
            base: pitch * pressure.cos(),
            tip: pitch + module,
            root: pitch - 1.25 * module,
            half_at_pitch: (std::f64::consts::PI * module / 2.0 - backlash) / (2.0 * pitch),
            pressure,
        };
        if shape.half(shape.tip) <= 0.0 {
            bail!("backlash {backlash} leaves the teeth pointed");
        }
        if shape.half(shape.root.max(shape.base)) >= std::f64::consts::PI / teeth as f64 {
            bail!("the teeth meet at the root; use more teeth or less pressure");
        }
        Ok(shape)
    }

    fn half(&self, radius: f64) -> f64 {
        let at = (self.base / radius.max(self.base)).acos();
        self.half_at_pitch + involute(self.pressure) - involute(at)
    }

    fn roll(&self, radius: f64) -> f64 {
        ((radius / self.base).powi(2) - 1.0).max(0.0).sqrt()
    }

    fn flank(&self, centre: f64, side: f64, rolls: [f64; 4], at: (f64, f64)) -> Segment {
        let lead = self.half_at_pitch + involute(self.pressure);
        let [p0, b1, b2, p3] = rolls.map(|t| {
            let r = self.base * (1.0 + t * t).sqrt();
            let a = centre + side * (lead - (t - t.atan()));
            (at.0 + r * a.cos(), at.1 + r * a.sin())
        });
        let first = |i: usize, p: [(f64, f64); 4]| {
            let pick = |q: (f64, f64)| if i == 0 { q.0 } else { q.1 };
            let [p0, b1, b2, p3] = p.map(pick);
            let near = 27.0 * b1 - 8.0 * p0 - p3;
            let far = 27.0 * b2 - p0 - 8.0 * p3;
            ((2.0 * near - far) / 18.0, (2.0 * far - near) / 18.0)
        };
        let points = [p0, b1, b2, p3];
        let (x, y) = (first(0, points), first(1, points));
        Segment::Cubic {
            to: p3,
            c1: (x.0, y.0),
            c2: (x.1, y.1),
        }
    }

    fn outline(&self, start: f64, at: (f64, f64)) -> ((f64, f64), Vec<Segment>) {
        let low = self.root.max(self.base);
        let (foot_roll, tip_roll) = (self.roll(low), self.roll(self.tip));
        let middle = foot_roll + (tip_roll - foot_roll) * 0.4;
        let inner = [0.0, 1.0, 2.0, 3.0].map(|k| {
            (foot_roll * foot_roll + (middle * middle - foot_roll * foot_roll) * k / 3.0).sqrt()
        });
        let outer = [0.0, 1.0, 2.0, 3.0].map(|k| middle + (tip_roll - middle) * k / 3.0);
        let reversed = |r: [f64; 4]| [r[3], r[2], r[1], r[0]];
        let gap = std::f64::consts::PI / self.teeth as f64;
        let polar = |r: f64, a: f64| (at.0 + r * a.cos(), at.1 + r * a.sin());
        let foot = self.half(low);
        let first = polar(self.root, start - foot);
        let segments = (0..self.teeth)
            .flat_map(|k| {
                let c = start + 2.0 * gap * k as f64;
                let tip = self.half(self.tip);
                let radial =
                    |a: f64, r: f64| (self.root < self.base).then(|| Segment::Line(polar(r, a)));
                radial(c - foot, low)
                    .into_iter()
                    .chain([
                        self.flank(c, -1.0, inner, at),
                        self.flank(c, -1.0, outer, at),
                        Segment::Arc {
                            to: polar(self.tip, c + tip),
                            via: polar(self.tip, c),
                        },
                        self.flank(c, 1.0, reversed(outer), at),
                        self.flank(c, 1.0, reversed(inner), at),
                    ])
                    .chain(radial(c + foot, self.root))
                    .chain(std::iter::once(Segment::Arc {
                        to: polar(self.root, c + 2.0 * gap - foot),
                        via: polar(self.root, c + gap),
                    }))
                    .collect::<Vec<_>>()
            })
            .collect();
        (first, segments)
    }
}
