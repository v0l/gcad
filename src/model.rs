use crate::geometry::{self, Frame, Profile};
use crate::parse::{Line, Scope, eval, eval_point};
use crate::select::{self, Groups};
use anyhow::{Context, Result, anyhow, bail};
use monstertruck::modeling::*;
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct Model {
    pub scope: Scope,
    pub frame: Option<Frame>,
    pub sketch: Vec<Profile>,
    pub solid: Option<Solid>,
    pub groups: Groups,
    pub path: Option<SweepPath>,
}

#[derive(Clone, Debug)]
pub struct SweepPath {
    pub points: Vec<Point3>,
    pub bend: f64,
}

enum Segment {
    Straight(Vector3),
    Bend {
        center: Point3,
        axis: Vector3,
        angle: f64,
    },
}

fn segments(path: &SweepPath) -> Result<(Vector3, Vec<Segment>)> {
    let points = &path.points;
    let directions: Vec<Vector3> = points.windows(2).map(|pair| pair[1] - pair[0]).collect();
    if directions.iter().any(|d| d.magnitude() < 1.0e-9) {
        bail!("path has two identical points in a row");
    }
    let corners: Vec<Option<(f64, Segment)>> = (1..points.len() - 1)
        .map(|i| {
            let (a, b) = (directions[i - 1].normalize(), directions[i].normalize());
            let angle = a.dot(b).clamp(-1.0, 1.0).acos();
            if angle < 1.0e-9 {
                return Ok(None);
            }
            if path.bend <= 0.0 {
                bail!(
                    "path turns at point {} ({}); give the bend radius with r=",
                    i + 1,
                    describe(points[i])
                );
            }
            if angle > std::f64::consts::PI - 1.0e-6 {
                bail!("path doubles back on itself at point {}", i + 1);
            }
            let setback = path.bend * (angle / 2.0).tan();
            let start = points[i] - a * setback;
            let inward = (b - a * a.dot(b)).normalize();
            Ok(Some((
                setback,
                Segment::Bend {
                    center: start + inward * path.bend,
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
    let setback = |i: usize| setbacks[i];
    let mut result = Vec::new();
    let mut corners = corners.into_iter();
    for (i, direction) in directions.iter().enumerate() {
        let length = direction.magnitude() - setback(i) - setback(i + 1);
        if length < -1.0e-9 {
            bail!(
                "bend radius r={} does not fit the segment from point {} to {}",
                path.bend,
                i + 1,
                i + 2
            );
        }
        if length > 1.0e-9 {
            result.push(Segment::Straight(direction.normalize() * length));
        }
        if let Some(Some((_, bend))) = corners.next() {
            result.push(bend);
        }
    }
    Ok((directions[0].normalize(), result))
}

fn describe(point: Point3) -> String {
    format!("{:.3},{:.3},{:.3}", point.x, point.y, point.z)
}

struct Args<'a> {
    line: &'a Line,
    values: HashMap<&'static str, &'a str>,
    rest: Vec<&'a str>,
}

impl<'a> Args<'a> {
    fn new(
        line: &'a Line,
        positional: &[&'static str],
        named: &[&'static str],
        variadic: bool,
    ) -> Result<Args<'a>> {
        let mut values = HashMap::new();
        let mut rest = Vec::new();
        line.positional
            .iter()
            .enumerate()
            .try_for_each(|(i, value)| {
                match positional.get(i) {
                    Some(name) => {
                        values.insert(*name, value.as_str());
                    }
                    None if variadic => rest.push(value.as_str()),
                    None => bail!(
                        "`{}` takes {} plain argument(s), got `{value}` extra",
                        line.op,
                        positional.len()
                    ),
                }
                Ok(())
            })?;
        line.named.iter().try_for_each(|(key, value)| {
            let name = positional
                .iter()
                .chain(named)
                .find(|name| **name == key.as_str())
                .ok_or_else(|| {
                    let all: Vec<_> = positional.iter().chain(named).collect();
                    anyhow!("`{}` has no `{key}` argument, it takes {all:?}", line.op)
                })?;
            if values.insert(*name, value.as_str()).is_some() {
                bail!("`{key}` given twice");
            }
            Ok(())
        })?;
        Ok(Args { line, values, rest })
    }

    fn text(&self, name: &str) -> Result<&'a str> {
        self.values
            .get(name)
            .copied()
            .ok_or_else(|| anyhow!("`{}` needs `{name}`", self.line.op))
    }

    fn number(&self, name: &str, scope: &Scope) -> Result<f64> {
        eval(self.text(name)?, scope).with_context(|| format!("argument `{name}`"))
    }

    fn optional_number(&self, name: &str, scope: &Scope) -> Result<Option<f64>> {
        self.values
            .get(name)
            .map(|text| eval(text, scope).with_context(|| format!("argument `{name}`")))
            .transpose()
    }

    fn point(&self, name: &str, scope: &Scope) -> Result<(f64, f64)> {
        self.values
            .get(name)
            .map_or(Ok((0.0, 0.0)), |text| eval_point(text, scope))
    }
}

pub fn label_of(line: &Line) -> String {
    line.label
        .clone()
        .unwrap_or_else(|| format!("L{}", line.number))
}

fn positive(value: f64, what: &str) -> Result<f64> {
    if value > 0.0 {
        Ok(value)
    } else {
        bail!("{what} must be positive, got {value}")
    }
}

enum Depth {
    Blind(f64),
    Through,
}

fn outline(profile: &Profile) -> Vec<(f64, f64)> {
    match profile {
        Profile::Rect {
            center,
            width,
            height,
            ..
        } => {
            let (hw, hh) = (width / 2.0, height / 2.0);
            vec![
                (center.0 - hw, center.1 - hh),
                (center.0 + hw, center.1 - hh),
                (center.0 + hw, center.1 + hh),
                (center.0 - hw, center.1 + hh),
            ]
        }
        Profile::Circle { center, diameter } => (0..32)
            .map(|i| {
                let angle = i as f64 / 32.0 * std::f64::consts::TAU;
                (
                    center.0 + diameter / 2.0 * angle.cos(),
                    center.1 + diameter / 2.0 * angle.sin(),
                )
            })
            .collect(),
        Profile::Polygon { points } => points.clone(),
    }
}

fn contains(polygon: &[(f64, f64)], (x, y): (f64, f64)) -> bool {
    (0..polygon.len()).fold(false, |inside, i| {
        let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
        if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
            !inside
        } else {
            inside
        }
    })
}

fn regions(profiles: &[Profile]) -> Vec<Vec<&Profile>> {
    let outlines: Vec<_> = profiles.iter().map(outline).collect();
    let inside_of = |i: usize| -> Option<usize> {
        (0..profiles.len())
            .find(|&j| j != i && contains(&outlines[j], profiles[i].interior_point()))
    };
    let parents: Vec<Option<usize>> = (0..profiles.len()).map(inside_of).collect();
    (0..profiles.len())
        .filter(|&i| parents[i].is_none())
        .map(|outer| {
            std::iter::once(&profiles[outer])
                .chain(
                    (0..profiles.len())
                        .filter(|&i| parents[i] == Some(outer))
                        .map(|i| &profiles[i]),
                )
                .collect()
        })
        .collect()
}

fn prism(
    frame: &Frame,
    region: &[&Profile],
    direction: Vector3,
    insets: (f64, f64),
) -> Result<Solid> {
    let mut solid: Solid = if insets.0 == 0.0 && insets.1 == 0.0 {
        let wires: Vec<Wire> = region.iter().map(|profile| profile.wire(frame)).collect();
        profile::solid_from_planar_profile(wires, direction)
            .map_err(|error| anyhow!("cannot extrude the sketch: {error}"))?
    } else {
        tapered(frame, region, direction, insets)?
    };
    if geometry::volume(&solid) < 0.0 {
        solid.not();
    }
    Ok(solid)
}

fn tapered(
    frame: &Frame,
    region: &[&Profile],
    direction: Vector3,
    (start, end): (f64, f64),
) -> Result<Solid> {
    let [profile] = region else {
        bail!("draft works on a profile without holes; cut the holes in a separate line");
    };
    let top_frame = Frame {
        origin: frame.origin + direction,
        ..*frame
    };
    let bottom = profile
        .inset(start)
        .map_err(|error| anyhow!(error))?
        .wire(frame);
    let top = profile
        .inset(end)
        .map_err(|error| anyhow!(error))?
        .wire(&top_frame);
    let mut shell: Shell = builder::try_wire_homotopy(&bottom, &top)
        .map_err(|error| anyhow!("cannot loft the draft: {error}"))?;
    let cap = |wire: Wire| -> Result<Face> {
        builder::try_attach_plane(&[wire]).map_err(|error| anyhow!("cannot cap the draft: {error}"))
    };
    shell.push(cap(bottom.inverse())?);
    shell.push(cap(top)?);
    Solid::try_new(vec![shell]).map_err(|error| anyhow!("drafted solid is not closed: {error}"))
}

fn classify(solid: &Solid, direction: Vector3) -> Vec<(&'static str, Surface)> {
    let along = direction.normalize();
    select::faces(solid)
        .iter()
        .map(|face| {
            let surface = face.oriented_surface();
            let group = match &surface {
                Surface::Plane(plane) if plane.normal().dot(along) < -0.999 => "start",
                Surface::Plane(plane) if plane.normal().dot(along) > 0.999 => "end",
                _ => "side",
            };
            (group, surface)
        })
        .collect()
}

impl Model {
    pub fn tolerance(&self) -> f64 {
        self.solid.as_ref().map_or(1.0e-6, |solid| {
            (geometry::bounds(solid).diameter() * 1.0e-6).max(1.0e-7)
        })
    }

    pub fn apply(&mut self, line: &Line) -> Result<String> {
        match line.op.as_str() {
            "let" => self.op_let(line),
            "plane" => self.op_plane(line),
            "rect" => self.op_rect(line),
            "circle" => self.op_circle(line),
            "poly" => self.op_poly(line),
            "extrude" => self.op_extrude(line),
            "cut" => self.op_cut(line),
            "revolve" => self.op_revolve(line),
            "path" => self.op_path(line),
            "sweep" => self.op_sweep(line),
            "hole" => self.op_hole(line),
            "fillet" => self.op_blend(line, FilletProfile::Round),
            "chamfer" => self.op_blend(line, FilletProfile::Chamfer),
            other => bail!(
                "unknown operation `{other}`; operations are let, plane, rect, circle, poly, extrude, cut, revolve, path, sweep, hole, fillet, chamfer"
            ),
        }
    }

    fn op_let(&mut self, line: &Line) -> Result<String> {
        if !line.positional.is_empty() {
            bail!("`let` takes name=value pairs only");
        }
        line.named.iter().try_for_each(|(name, text)| {
            let value = eval(text, &self.scope).with_context(|| format!("`{name}`"))?;
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

    fn sketch_frame(&self) -> Frame {
        self.frame
            .unwrap_or_else(|| Frame::named("XY").expect("XY is a named plane"))
    }

    fn op_plane(&mut self, line: &Line) -> Result<String> {
        if !self.sketch.is_empty() {
            bail!(
                "the sketch has {} profile(s); extrude or cut it before moving the plane",
                self.sketch.len()
            );
        }
        let args = Args::new(line, &["on"], &["offset"], false)?;
        let on = args.text("on")?;
        let base = match Frame::named(on) {
            Some(frame) => frame,
            None => self.face_frame(on)?,
        };
        let frame = base.offset(args.optional_number("offset", &self.scope)?.unwrap_or(0.0));
        self.frame = Some(frame);
        Ok(describe_frame(&frame))
    }

    fn face_frame(&self, selector: &str) -> Result<Frame> {
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
        self.sketch.push(profile);
        Ok(format!("sketch has {} profile(s)", self.sketch.len()))
    }

    fn op_rect(&mut self, line: &Line) -> Result<String> {
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

    fn op_circle(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["d"], &["at"], false)?;
        let diameter = positive(args.number("d", &self.scope)?, "d")?;
        let center = args.point("at", &self.scope)?;
        self.add_profile(Profile::Circle { center, diameter })
    }

    fn op_poly(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &[], true)?;
        let points = args
            .rest
            .iter()
            .map(|text| eval_point(text, &self.scope))
            .collect::<Result<Vec<_>>>()?;
        if points.len() < 3 {
            bail!("`poly` needs at least three x,y points");
        }
        self.add_profile(Profile::Polygon { points })
    }

    fn take_sketch(&mut self, op: &str) -> Result<(Frame, Vec<Profile>)> {
        if self.sketch.is_empty() {
            bail!("`{op}` needs a sketch; add rect, circle or poly first");
        }
        Ok((self.sketch_frame(), std::mem::take(&mut self.sketch)))
    }

    fn on_existing_face(&self, frame: &Frame, normal: Vector3) -> bool {
        self.solid.as_ref().is_some_and(|solid| {
            select::faces(solid)
                .iter()
                .any(|face| match face.oriented_surface() {
                    Surface::Plane(plane) => {
                        plane.normal().dot(normal) > 1.0 - 1.0e-9
                            && (plane.origin() - frame.origin).dot(normal).abs() < self.tolerance()
                    }
                    _ => false,
                })
        })
    }

    fn op_extrude(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["d"], &["draft"], false)?;
        let distance = args.number("d", &self.scope)?;
        if distance == 0.0 {
            bail!("extrude distance is zero");
        }
        let taper = args
            .optional_number("draft", &self.scope)?
            .unwrap_or(0.0)
            .to_radians()
            .tan();
        let (frame, profiles) = self.take_sketch("extrude")?;
        let normal = frame.normal * distance.signum();
        let overlap = if self.on_existing_face(&frame, normal) {
            (distance.abs() * 0.02).max(1.0e-3)
        } else {
            0.0
        };
        let start = frame.offset(-overlap * distance.signum());
        let direction = normal * (distance.abs() + overlap);
        let label = label_of(line);
        let mut solid = self.solid.take();
        for region in regions(&profiles) {
            let insets = (-overlap * taper, distance.abs() * taper);
            let tool = prism(&start, &region, direction, insets)?;
            classify(&tool, direction)
                .into_iter()
                .for_each(|(group, surface)| self.groups.record(&label, group, surface));
            solid = Some(match solid {
                None => tool,
                Some(existing) => monstertruck::solid::or_normalized(&existing, &tool)
                    .map_err(|error| anyhow!("union failed: {error}"))?,
            });
        }
        self.solid = solid;
        self.describe_solid()
    }

    #[allow(clippy::too_many_arguments)]
    fn remove(
        &mut self,
        line: &Line,
        profiles: Vec<Profile>,
        frame: Frame,
        depth: Depth,
        taper: f64,
        side_group: &str,
        end_group: &str,
    ) -> Result<String> {
        let existing = self
            .solid
            .take()
            .ok_or_else(|| anyhow!("`{}` needs a solid to cut", line.op))?;
        let bounds = geometry::bounds(&existing);
        let clearance = (bounds.diameter() * 0.01).max(1.0e-3);
        let distance = match depth {
            Depth::Blind(d) => d,
            Depth::Through => {
                let corners = [bounds.min(), bounds.max()];
                let farthest = (0..8)
                    .map(|i| {
                        Point3::new(
                            corners[i & 1].x,
                            corners[(i >> 1) & 1].y,
                            corners[(i >> 2) & 1].z,
                        )
                    })
                    .map(|corner| (frame.origin - corner).dot(frame.normal))
                    .fold(0.0, f64::max);
                farthest + clearance
            }
        };
        let start = frame.offset(clearance);
        let direction = -frame.normal * (distance + clearance);
        let label = label_of(line);
        let mut solid = existing;
        for region in regions(&profiles) {
            let tool = prism(
                &start,
                &region,
                direction,
                (-clearance * taper, distance * taper),
            )?;
            classify(&tool, direction)
                .into_iter()
                .for_each(|(group, mut surface)| {
                    surface.invert();
                    let name = match group {
                        "end" => end_group,
                        "side" => side_group,
                        other => other,
                    };
                    self.groups.record(&label, name, surface);
                });
            solid = monstertruck::solid::difference_normalized(&solid, &tool)
                .map_err(|error| anyhow!("cut failed: {error}"))?;
        }
        self.solid = Some(solid);
        self.describe_solid()
    }

    fn op_cut(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["d"], &["draft"], false)?;
        let depth = match args.text("d")? {
            "thru" => Depth::Through,
            _ => Depth::Blind(positive(args.number("d", &self.scope)?, "cut depth")?),
        };
        let taper = args
            .optional_number("draft", &self.scope)?
            .unwrap_or(0.0)
            .to_radians()
            .tan();
        let (frame, profiles) = self.take_sketch("cut")?;
        self.remove(line, profiles, frame, depth, taper, "side", "end")
    }

    fn op_revolve(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["angle"], &["axis"], false)?;
        let angle = args.number("angle", &self.scope)?;
        if angle == 0.0 || angle.abs() > 360.0 {
            bail!("revolve angle must be within -360..360 and not zero, got {angle}");
        }
        let (frame, profiles) = self.take_sketch("revolve")?;
        let axis = match args.values.get("axis").copied().unwrap_or("y") {
            "x" => frame.x,
            "y" => frame.y,
            other => bail!("axis must be x or y of the plane, got `{other}`"),
        };
        let sweep = if angle.abs() == 360.0 {
            builder::SweepAngle::Closed
        } else {
            builder::SweepAngle::Partial(Rad(angle.to_radians()))
        };
        let division =
            ((angle.abs() / 90.0).ceil() as usize).max(if angle.abs() == 360.0 { 4 } else { 1 });
        let label = label_of(line);
        let mut solid = self.solid.take();
        for region in regions(&profiles) {
            let wires: Vec<Wire> = region.iter().map(|profile| profile.wire(&frame)).collect();
            let face: Face = profile::attach_plane_normalized(wires)
                .map_err(|error| anyhow!("cannot face the sketch: {error}"))?;
            let mut tool: Solid = builder::revolve(&face, frame.origin, axis, sweep, division);
            if geometry::volume(&tool) < 0.0 {
                tool.not();
            }
            select::faces(&tool).iter().for_each(|face| {
                let surface = face.oriented_surface();
                let group = if matches!(surface, Surface::Plane(_)) {
                    "caps"
                } else {
                    "side"
                };
                self.groups.record(&label, group, surface);
            });
            solid = Some(match solid {
                None => tool,
                Some(existing) => monstertruck::solid::or_normalized(&existing, &tool)
                    .map_err(|error| anyhow!("union failed: {error}"))?,
            });
        }
        self.solid = solid;
        self.describe_solid()
    }

    fn op_path(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &["r"], true)?;
        let points = args
            .rest
            .iter()
            .map(|text| {
                let parts = text.split(',').collect::<Vec<_>>();
                match parts.as_slice() {
                    [x, y, z] => Ok(Point3::new(
                        eval(x, &self.scope)?,
                        eval(y, &self.scope)?,
                        eval(z, &self.scope)?,
                    )),
                    _ => bail!("`{text}` is not a path point, write it as x,y,z"),
                }
            })
            .collect::<Result<Vec<_>>>()?;
        if points.len() < 2 {
            bail!("`path` needs at least two x,y,z points");
        }
        let bend = args.optional_number("r", &self.scope)?.unwrap_or(0.0);
        let path = SweepPath { points, bend };
        let (_, pieces) = segments(&path)?;
        let bends = pieces
            .iter()
            .filter(|piece| matches!(piece, Segment::Bend { .. }))
            .count();
        self.path = Some(path);
        Ok(format!(
            "path of {} straight and {bends} bent segment(s)",
            pieces.len() - bends
        ))
    }

    fn op_sweep(&mut self, line: &Line) -> Result<String> {
        Args::new(line, &[], &[], false)?;
        let path = self
            .path
            .clone()
            .ok_or_else(|| anyhow!("`sweep` needs a `path` first"))?;
        let (frame, profiles) = self.take_sketch("sweep")?;
        let (start_direction, pieces) = segments(&path)?;
        let x = {
            let projected = frame.x - start_direction * frame.x.dot(start_direction);
            if projected.magnitude() > 1.0e-6 {
                projected.normalize()
            } else {
                (frame.y - start_direction * frame.y.dot(start_direction)).normalize()
            }
        };
        let start = Frame {
            origin: path.points[0],
            x,
            y: start_direction.cross(x),
            normal: start_direction,
        };
        let label = label_of(line);
        let mut solid = self.solid.take();
        for region in regions(&profiles) {
            let wires: Vec<Wire> = region.iter().map(|profile| profile.wire(&start)).collect();
            let mut face: Face = profile::attach_plane_normalized(wires)
                .map_err(|error| anyhow!("cannot face the sketch: {error}"))?;
            if let Surface::Plane(plane) = face.oriented_surface()
                && plane.normal().dot(start_direction) < 0.0
            {
                face.invert();
            }
            let mut faces: Vec<Face> = vec![face.inverse()];
            for piece in &pieces {
                let swept: Solid = match piece {
                    Segment::Straight(vector) => builder::extrude(&face, *vector),
                    Segment::Bend {
                        center,
                        axis,
                        angle,
                    } => {
                        let division =
                            ((angle / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
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
            let mut tool = Solid::try_new(vec![shell])
                .map_err(|error| anyhow!("swept solid is not closed: {error}"))?;
            if geometry::volume(&tool) < 0.0 {
                tool.not();
            }
            let tool_faces = select::faces(&tool);
            tool_faces.iter().enumerate().for_each(|(i, face)| {
                let group = if i == 0 {
                    "start"
                } else if i + 1 == tool_faces.len() {
                    "end"
                } else {
                    "side"
                };
                self.groups.record(&label, group, face.oriented_surface());
            });
            solid = Some(match solid {
                None => tool,
                Some(existing) => monstertruck::solid::or_normalized(&existing, &tool)
                    .map_err(|error| anyhow!("union failed: {error}"))?,
            });
        }
        self.solid = solid;
        self.describe_solid()
    }

    fn op_hole(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["d"], &["depth"], true)?;
        let diameter = positive(args.number("d", &self.scope)?, "hole diameter")?;
        if args.rest.is_empty() {
            bail!("`hole` needs at least one x,y position after the diameter");
        }
        if !self.sketch.is_empty() {
            bail!(
                "the sketch has {} profile(s); extrude or cut it before `hole`",
                self.sketch.len()
            );
        }
        let profiles = args
            .rest
            .iter()
            .map(|text| {
                eval_point(text, &self.scope).map(|center| Profile::Circle { center, diameter })
            })
            .collect::<Result<Vec<_>>>()?;
        let depth = match args.values.get("depth") {
            None | Some(&"thru") => Depth::Through,
            Some(_) => Depth::Blind(positive(args.number("depth", &self.scope)?, "hole depth")?),
        };
        let frame = self.sketch_frame();
        self.remove(line, profiles, frame, depth, 0.0, "side", "bottom")
    }

    fn op_blend(&mut self, line: &Line, profile: FilletProfile) -> Result<String> {
        let args = Args::new(line, &["size", "edges"], &[], false)?;
        let size = positive(args.number("size", &self.scope)?, "size")?;
        let selector = args.text("edges")?;
        let solid = self
            .solid
            .as_ref()
            .ok_or_else(|| anyhow!("`{}` needs a solid", line.op))?;
        let tolerance = self.tolerance();
        let edges = select::select_edges(selector, solid, &self.groups, tolerance)?;
        if edges.is_empty() {
            bail!("`{selector}` matched no edges");
        }
        let before: Vec<Surface> = select::faces(solid)
            .iter()
            .map(|face| face.oriented_surface())
            .collect();
        let options = FilletOptions::constant(size).with_profile(profile);
        let shells = solid
            .boundaries()
            .iter()
            .map(|shell| {
                let mut shell = shell.clone();
                let owned: Vec<Edge> = edges
                    .iter()
                    .filter(|edge| shell.edge_iter().any(|own| own.is_same(edge)))
                    .cloned()
                    .collect();
                if !owned.is_empty() {
                    fillet_edges(&mut shell, &owned, Some(&options)).map_err(|error| {
                        anyhow!("{} of {} edge(s) failed: {error}", line.op, owned.len())
                    })?;
                }
                Ok(shell)
            })
            .collect::<Result<Vec<_>>>()?;
        let result = Solid::try_new(shells)
            .map_err(|error| anyhow!("{} left an invalid solid: {error}", line.op))?;
        let label = label_of(line);
        select::faces(&result)
            .iter()
            .filter(|face| {
                !before
                    .iter()
                    .any(|surface| select::face_on(face, surface, tolerance))
            })
            .for_each(|face| self.groups.record(&label, "faces", face.oriented_surface()));
        self.solid = Some(result);
        Ok(format!(
            "{} edge(s); {}",
            edges.len(),
            self.describe_solid()?
        ))
    }

    pub fn describe_solid(&self) -> Result<String> {
        let solid = self.solid.as_ref().ok_or_else(|| anyhow!("no solid"))?;
        let bounds = geometry::bounds(solid);
        let tidy = |v: f64| if v.abs() < 5.0e-4 { 0.0 } else { v };
        let (min, max) = (bounds.min().map(tidy), bounds.max().map(tidy));
        Ok(format!(
            "{} faces, volume {:.3}, bbox [{:.3}, {:.3}, {:.3}]..[{:.3}, {:.3}, {:.3}]",
            select::faces(solid).len(),
            geometry::volume(solid),
            min.x,
            min.y,
            min.z,
            max.x,
            max.y,
            max.z
        ))
    }
}

fn describe_frame(frame: &Frame) -> String {
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

pub struct Run {
    pub model: Model,
    pub steps: Vec<(Line, Result<String>)>,
}

pub fn run(lines: &[Line]) -> Run {
    let mut model = Model::default();
    let mut steps = Vec::new();
    for line in lines {
        let result = model.apply(line);
        let failed = result.is_err();
        steps.push((line.clone(), result));
        if failed {
            break;
        }
    }
    Run { model, steps }
}
