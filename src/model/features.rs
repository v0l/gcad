use super::args::{Args, combine_mode, label_of, positive};
use super::solids::{classify, loft_wires, loops, oriented, prism, rational, regions, tapered};
use super::{Combine, Model, Prism};
use crate::geometry::{self, Frame, Profile, Segment};
use crate::parse::Line;
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

pub(crate) enum Depth {
    Blind(f64),
    Through,
}

pub(crate) struct Removal<'a> {
    pub(crate) frame: Frame,
    pub(crate) profiles: Vec<Profile>,
    pub(crate) depth: Depth,
    pub(crate) taper: f64,
    pub(crate) side: &'a str,
    pub(crate) end: &'a str,
}

#[derive(Clone, Debug)]
pub struct Revolve {
    pub frame: Frame,
    pub profiles: Vec<Profile>,
    pub through: Point3,
    pub axis: Vector3,
    pub angle: f64,
    pub alone: Option<Vec<monstertruck::topology::FaceId<Surface>>>,
}

fn reversed(segments: &[Segment], ends: &[(f64, f64)]) -> Vec<Segment> {
    segments
        .iter()
        .enumerate()
        .rev()
        .map(|(i, segment)| match segment {
            Segment::Line(_) => Segment::Line(ends[i]),
            Segment::Arc { via, .. } => Segment::Arc {
                to: ends[i],
                via: *via,
            },
            Segment::Cubic { c1, c2, .. } => Segment::Cubic {
                to: ends[i],
                c1: *c2,
                c2: *c1,
            },
        })
        .collect()
}

fn wall_profiles(
    start: (f64, f64),
    segments: &[Segment],
    walled: &[bool],
    inner_start: (f64, f64),
    inner: &[Segment],
) -> Vec<Profile> {
    let n = segments.len();
    let outer_ends: Vec<(f64, f64)> = std::iter::once(start)
        .chain(segments.iter().map(Segment::end))
        .collect();
    let inner_ends: Vec<(f64, f64)> = std::iter::once(inner_start)
        .chain(inner.iter().map(Segment::end))
        .collect();
    if walled.iter().all(|w| *w) {
        return vec![
            Profile::Path {
                start,
                segments: segments.to_vec(),
            },
            Profile::Path {
                start: inner_start,
                segments: inner.to_vec(),
            },
        ];
    }
    let first_shared = (0..n).find(|&i| !walled[i]).expect("one is shared");
    let mut runs = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    for k in 1..=n {
        let i = (first_shared + k) % n;
        if walled[i] {
            current.push(i);
        } else if !current.is_empty() {
            runs.push(std::mem::take(&mut current));
        }
    }
    let apart = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).hypot(a.1 - b.1) > 1.0e-9;
    runs.into_iter()
        .map(|run| {
            let (a, b) = (run[0], *run.last().expect("non-empty"));
            let mut path: Vec<Segment> = run.iter().map(|&i| segments[i].clone()).collect();
            let (outer_end, inner_end) = (outer_ends[b + 1], inner_ends[b + 1]);
            if apart(outer_end, inner_end) {
                path.push(Segment::Line(inner_end));
            }
            let inner_run: Vec<Segment> = run.iter().map(|&i| inner[i].clone()).collect();
            let inner_run_ends: Vec<(f64, f64)> = std::iter::once(inner_ends[a])
                .chain(inner_run.iter().map(Segment::end))
                .collect();
            path.extend(reversed(&inner_run, &inner_run_ends));
            if apart(inner_ends[a], outer_ends[a]) {
                path.push(Segment::Line(outer_ends[a]));
            }
            Profile::Path {
                start: outer_ends[a],
                segments: path,
            }
        })
        .collect()
}

fn outer_profiles(frame: &Frame, profiles: &[Profile]) -> Result<Vec<Profile>> {
    let shapes = loops(frame, profiles)?;
    Ok(regions(&shapes)
        .iter()
        .map(|region| profiles[shapes[region[0]].profile].clone())
        .collect())
}

fn face_ids(solid: &Solid) -> Vec<monstertruck::topology::FaceId<Surface>> {
    select::faces(solid).iter().map(Face::id).collect()
}

fn revolved(
    frame: &Frame,
    profiles: &[Profile],
    through: Point3,
    axis: Vector3,
    angle: f64,
) -> Result<Vec<Solid>> {
    let sweep = if angle.abs() == 360.0 {
        builder::SweepAngle::Closed
    } else {
        builder::SweepAngle::Partial(Rad(angle.to_radians()))
    };
    let division =
        ((angle.abs() / 90.0).ceil() as usize).max(if angle.abs() == 360.0 { 4 } else { 1 });
    let shapes = loops(frame, profiles)?;
    regions(&shapes)
        .iter()
        .map(|region| {
            let wires: Vec<Wire> = region.iter().map(|&i| shapes[i].wire.clone()).collect();
            let face: Face = profile::attach_plane_normalized(wires)
                .map_err(|error| anyhow!("cannot face the sketch: {error}"))?;
            Ok(oriented(rational(
                builder::revolve(&face, through, axis, sweep, division),
                angle.abs().to_radians() / division as f64,
            )))
        })
        .collect()
}

fn clearance(solid: &Solid) -> f64 {
    (geometry::bounds(solid).diameter() * 0.01).max(1.0e-3)
}

fn farthest_along(solid: &Solid, origin: Point3, direction: Vector3) -> f64 {
    let bounds = geometry::bounds(solid);
    let corners = [bounds.min(), bounds.max()];
    (0..8)
        .map(|i| {
            Point3::new(
                corners[i & 1].x,
                corners[(i >> 1) & 1].y,
                corners[(i >> 2) & 1].z,
            )
        })
        .map(|corner| (corner - origin).dot(direction))
        .fold(0.0, f64::max)
}

pub(crate) fn sink_face(solid: &Solid, face: &Face, distance: f64, selector: &str) -> Result<()> {
    let Surface::Plane(plane) = face.oriented_surface() else {
        bail!("`push` moves flat faces; `{selector}` includes a curved one");
    };
    let shift = plane.normal() * distance;
    let boundary: Vec<Edge> = face.edge_iter().collect();
    let corners: Vec<Vertex> = face.vertex_iter().collect();
    let touches = |edge: &Edge| {
        corners
            .iter()
            .any(|v| v == edge.front() || v == edge.back())
    };
    let shells = solid.boundaries();
    let neighbours: Vec<&Face> = shells
        .iter()
        .flat_map(|shell| shell.face_iter())
        .filter(|other| other.id() != face.id() && other.edge_iter().any(|edge| touches(&edge)))
        .collect();
    if neighbours.iter().any(|other| !matches!(other.surface(), Surface::Plane(p) if p.normal().dot(shift).abs() < 1.0e-9 * shift.magnitude())) {
        bail!("pushing a face in needs flat side faces square to it; `{selector}` has others");
    }
    let sides: Vec<Edge> = shells
        .iter()
        .flat_map(|shell| shell.edge_iter())
        .filter(|edge| touches(edge) && !boundary.iter().any(|own| own.is_same(edge)))
        .collect();
    if sides
        .iter()
        .any(|edge| !matches!(edge.curve(), Curve::Line(_)))
    {
        bail!("pushing a face in needs straight side edges");
    }
    let translation = Matrix4::from_translation(shift);
    face.set_surface(face.surface().transformed(translation));
    corners.iter().for_each(|v| v.set_point(v.point() + shift));
    boundary
        .iter()
        .for_each(|edge| edge.set_curve(edge.curve().transformed(translation)));
    sides.iter().for_each(|edge| {
        edge.set_curve(Curve::Line(Line(
            edge.absolute_front().point(),
            edge.absolute_back().point(),
        )))
    });
    Ok(())
}

fn taper_of(args: &Args<'_>, model: &Model) -> Result<f64> {
    Ok(args
        .optional_number("draft", &model.scope)?
        .unwrap_or(0.0)
        .to_radians()
        .tan())
}

impl Model {
    fn backed(&self, frame: &Frame, profiles: &[Profile], below: Vector3) -> Result<bool> {
        let Some(solid) = self.solid.as_ref() else {
            return Ok(false);
        };
        let mesh = geometry::mesh(solid, geometry::mesh_tolerance(solid));
        let direction = Vector3::new(0.5773, 0.5774, 0.5775).normalize();
        let inside = |p: Point3| {
            geometry::ray_hits(&mesh, p, direction)
                .iter()
                .filter(|(t, _)| *t > 0.0)
                .count()
                % 2
                == 1
        };
        let outline: Vec<(f64, f64)> = loops(frame, profiles)?
            .iter()
            .flat_map(|l| l.outline.clone())
            .collect();
        Ok(outline.iter().all(|&(u, v)| inside(frame.at(u, v) + below)))
    }

    pub(crate) fn on_existing_face(&self, origin: Point3, normal: Vector3) -> bool {
        self.solid.as_ref().is_some_and(|solid| {
            select::faces(solid)
                .iter()
                .any(|face| match face.oriented_surface() {
                    Surface::Plane(plane) => {
                        plane.normal().dot(normal) > 1.0 - 1.0e-9
                            && (plane.origin() - origin).dot(normal).abs() < self.tolerance()
                    }
                    _ => false,
                })
        })
    }

    fn next_face(&self, frame: &Frame, profiles: &[Profile]) -> Result<Frame> {
        let solid = self
            .solid
            .as_ref()
            .ok_or_else(|| anyhow!("`extrude next` needs a solid to grow into"))?;
        let outline: Vec<(f64, f64)> = loops(frame, profiles)?
            .iter()
            .flat_map(|l| l.outline.clone())
            .collect();
        let n = outline.len() as f64;
        let (u, v) = outline
            .iter()
            .fold((0.0, 0.0), |(su, sv), (u, v)| (su + u / n, sv + v / n));
        let centre = frame.at(u, v);
        let mesh = geometry::mesh(solid, geometry::mesh_tolerance(solid));
        let tolerance = self.tolerance() * 10.0;
        let nearest = |direction: Vector3| {
            geometry::ray_hits(&mesh, centre, direction)
                .into_iter()
                .filter(|(t, n)| *t > tolerance && n.dot(direction) < 0.0)
                .min_by(|a, b| a.0.total_cmp(&b.0))
        };
        let (t, direction) = nearest(frame.normal)
            .map(|(t, _)| (t, frame.normal))
            .or_else(|| nearest(-frame.normal).map(|(t, _)| (t, -frame.normal)))
            .ok_or_else(|| anyhow!("no face of the solid lies ahead of or behind the sketch"))?;
        let hit = centre + direction * t;
        select::faces(solid)
            .iter()
            .find_map(|face| match face.oriented_surface() {
                Surface::Plane(plane)
                    if plane.normal().dot(direction) < -1.0e-9
                        && (hit - plane.origin()).dot(plane.normal()).abs() < tolerance =>
                {
                    Some(Frame::from_normal(hit, plane.normal()))
                }
                _ => None,
            })
            .ok_or_else(|| anyhow!("the next face is curved; `extrude next` stops at flat faces"))
    }

    pub(crate) fn op_extrude(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["d", "how"],
            &["draft", "mode", "upto", "offset", "thin"],
            false,
        )?;
        let combine = match args.values.get("mode").copied() {
            None | Some("add") => Combine::Add,
            Some("intersect") => Combine::Common,
            Some("cut") => Combine::Remove,
            Some(other) => bail!("mode must be add, cut or intersect, got `{other}`"),
        };
        let both = match args.values.get("how").copied() {
            None => false,
            Some("both") => true,
            Some(other) => bail!("`{other}` is not an extrude option; did you mean `both`?"),
        };
        let taper = taper_of(&args, self)?;
        let was_empty = self.solid.is_none() && combine == Combine::Add;
        let (frame, mut profiles) = self.take_sketch("extrude")?;
        if let Some(wall) = args.optional_number("thin", &self.scope)? {
            let wall = positive(wall, "thin")?;
            profiles = profiles
                .iter()
                .map(|p| p.inset(wall).map(|inner| [p.clone(), inner]))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|error| anyhow!("thin: {error}"))?
                .concat();
        }
        let next = args.values.get("d") == Some(&"next");
        let target = match (args.values.get("upto"), next) {
            (Some(_), true) => bail!("give `next` or `upto=`, not both"),
            (Some(selector), false) => {
                if args.has("d") {
                    bail!("give either a distance or `upto=`, not both");
                }
                Some((self.face_frame(selector)?, format!("`{selector}`")))
            }
            (None, true) => Some((
                self.next_face(&frame, &profiles)?,
                "the next face".to_string(),
            )),
            (None, false) => None,
        };
        let (distance, end_overlap) = match target {
            Some((face, name)) => {
                let target =
                    face.offset(args.optional_number("offset", &self.scope)?.unwrap_or(0.0));
                let distance = (target.origin - frame.origin).dot(target.normal)
                    / frame.normal.dot(target.normal);
                if distance.abs() < 1.0e-9 {
                    bail!("{name} is on the sketch plane");
                }
                let into_material = target.normal.dot(frame.normal * distance.signum()) < 0.0;
                (
                    distance,
                    if into_material {
                        (distance.abs() * 0.02).max(1.0e-3)
                    } else {
                        0.0
                    },
                )
            }
            None => (args.number("d", &self.scope)?, 0.0),
        };
        if distance == 0.0 {
            bail!("extrude distance is zero");
        }
        let normal = frame.normal * distance.signum();
        let base = if both {
            frame.offset(-distance / 2.0)
        } else {
            frame
        };
        let pad = (distance.abs() * 0.02).max(1.0e-3);
        let starts_on_face =
            combine == Combine::Add && !both && self.on_existing_face(frame.origin, normal);
        let backed = starts_on_face && self.backed(&frame, &profiles, -normal * pad * 0.5)?;
        let overlaps: Vec<f64> = match combine {
            Combine::Add if backed => vec![0.0, pad],
            Combine::Add if starts_on_face => vec![0.0, pad],
            Combine::Common if self.on_existing_face(base.origin, -normal) => vec![pad],
            _ => vec![0.0],
        };
        let end_overlap = match combine {
            Combine::Common
                if self.on_existing_face(base.origin + normal * distance.abs(), normal) =>
            {
                end_overlap.max(pad)
            }
            _ => end_overlap,
        };
        let label = label_of(line);
        let before = self.solid.clone();
        let mut outcome = Err(anyhow!("nothing to extrude"));
        let mut used = 0.0;
        for &start_overlap in &overlaps {
            self.solid = before.clone();
            let start = base.offset(-start_overlap * distance.signum());
            let direction = normal * (distance.abs() + start_overlap + end_overlap);
            let shapes = loops(&start, &profiles)?;
            let insets = (
                -start_overlap * taper,
                (distance.abs() + end_overlap) * taper,
            );
            let tools = regions(&shapes)
                .iter()
                .map(|region| prism(&start, &shapes, region, &profiles, direction, insets))
                .collect::<Result<Vec<_>>>()?;
            outcome = self.merge_all(&label, tools.clone(), combine);
            if outcome.is_ok() {
                for tool in &tools {
                    let groups = classify(tool, direction);
                    match combine {
                        Combine::Remove => self.record_inverted(&label, groups),
                        _ => self.record(&label, groups),
                    }
                }
                used = start_overlap;
                break;
            }
        }
        if let Err(error) = outcome {
            self.solid = before;
            return Err(error);
        }
        self.prisms.insert(
            label,
            Prism {
                frame: base,
                profiles,
                distance,
                alone: was_empty
                    .then(|| self.solid.as_ref().map(face_ids))
                    .flatten(),
            },
        );
        let summary = self.describe_solid()?;
        Ok(if starts_on_face && !backed && used > 0.0 {
            format!(
                "{summary}; the kernel could not join it flush, so where it overhangs the face it starts {used:.3} below it"
            )
        } else {
            summary
        })
    }

    pub(crate) fn remove(&mut self, label: &str, removal: Removal<'_>) -> Result<String> {
        let existing = self.active("cut")?.clone();
        let clearance = clearance(&existing);
        let distance = match removal.depth {
            Depth::Blind(d) => d,
            Depth::Through => {
                farthest_along(&existing, removal.frame.origin, -removal.frame.normal) + clearance
            }
        };
        let start = removal.frame.offset(clearance);
        let direction = -removal.frame.normal * (distance + clearance);
        let shapes = loops(&start, &removal.profiles)?;
        let mut tools = Vec::new();
        for region in regions(&shapes) {
            let insets = (-clearance * removal.taper, distance * removal.taper);
            let tool = prism(
                &start,
                &shapes,
                &region,
                &removal.profiles,
                direction,
                insets,
            )?;
            let groups = classify(&tool, direction)
                .into_iter()
                .map(|(group, surface)| {
                    let name = match group {
                        "end" => removal.end,
                        "side" => removal.side,
                        other => other,
                    };
                    (name, surface)
                })
                .collect();
            self.record_inverted(label, groups);
            tools.push(tool);
        }
        self.merge_all(label, tools, Combine::Remove)?;
        self.describe_solid()
    }

    pub(crate) fn op_cut(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["d"], &["draft"], false)?;
        let depth = match args.text("d")? {
            "thru" => Depth::Through,
            _ => Depth::Blind(positive(args.number("d", &self.scope)?, "cut depth")?),
        };
        let taper = taper_of(&args, self)?;
        let (frame, profiles) = self.take_sketch("cut")?;
        self.remove(
            &label_of(line),
            Removal {
                frame,
                profiles,
                depth,
                taper,
                side: "side",
                end: "end",
            },
        )
    }

    pub(crate) fn op_revolve(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["angle"], &["axis", "mode"], false)?;
        let combine = combine_mode(&args)?;
        let angle = args.number("angle", &self.scope)?;
        if angle == 0.0 || angle.abs() > 360.0 {
            bail!("revolve angle must be within -360..360 and not zero, got {angle}");
        }
        let (frame, profiles) = self.take_sketch("revolve")?;
        let (through, axis) = match args.values.get("axis").copied().unwrap_or("y") {
            "x" => (frame.origin, frame.x),
            "y" => (frame.origin, frame.y),
            name => self.axes.get(name).copied().ok_or_else(|| {
                anyhow!("axis must be x or y of the plane or a datum `axis`, got `{name}`")
            })?,
        };
        let label = label_of(line);
        let was_empty = self.solid.is_none();
        let mut revolve = Revolve {
            frame,
            profiles,
            through,
            axis,
            angle,
            alone: None,
        };
        for tool in revolved(&revolve.frame, &revolve.profiles, through, axis, angle)? {
            let groups = select::faces(&tool)
                .iter()
                .map(|face| {
                    let surface = face.oriented_surface();
                    (
                        if matches!(surface, Surface::Plane(_)) {
                            "caps"
                        } else {
                            "side"
                        },
                        surface,
                    )
                })
                .collect();
            self.record_for(&label, groups, combine);
            self.merge(&label, tool, combine)?;
        }
        if combine == Combine::Add {
            if was_empty {
                revolve.alone = self.solid.as_ref().map(face_ids);
            }
            self.revolves.insert(label, revolve);
        }
        self.describe_solid()
    }

    pub(crate) fn op_loft(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["how"], &["mode"], false)?;
        let combine = combine_mode(&args)?;
        let smooth = match args.values.get("how").copied() {
            None => false,
            Some("smooth") => true,
            Some(other) => bail!("`{other}` is not a loft option; did you mean `smooth`?"),
        };
        if self.sections.len() < 2 {
            bail!(
                "`loft` needs at least two `section` lines, it has {}",
                self.sections.len()
            );
        }
        let sections = std::mem::take(&mut self.sections);
        let wires = sections
            .iter()
            .enumerate()
            .map(|(i, (frame, profiles))| {
                let mut shapes = loops(frame, profiles)?;
                if shapes.len() != 1 {
                    bail!(
                        "section {} has {} loops; each section takes one profile without holes",
                        i + 1,
                        shapes.len()
                    );
                }
                Ok(shapes.remove(0).wire)
            })
            .collect::<Result<Vec<_>>>()?;
        let counts: Vec<usize> = wires.iter().map(|wire| wire.len()).collect();
        let tool = if smooth || counts.windows(2).any(|pair| pair[0] != pair[1]) {
            super::skin::skin(&wires, smooth)?
        } else {
            loft_wires(&wires)?
        };
        let label = label_of(line);
        let faces = select::faces(&tool);
        let count = faces.len();
        let groups = faces
            .iter()
            .enumerate()
            .map(|(i, face)| {
                (
                    if i + 2 == count {
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
        self.describe_solid()
    }

    pub(crate) fn op_shell(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["t"], &["open"], false)?;
        let thickness = positive(args.number("t", &self.scope)?, "wall thickness")?;
        let open: Vec<&str> = args
            .values
            .get("open")
            .map(|text| text.split(',').collect())
            .unwrap_or_default();
        let owners: Vec<&str> = open
            .iter()
            .map(|selector| {
                selector
                    .split_once('.')
                    .map_or(*selector, |(label, _)| label)
            })
            .collect();
        let label = match owners.first() {
            Some(first) => {
                if owners.iter().any(|o| o != first) {
                    bail!("every `open=` face must belong to the same feature");
                }
                first.to_string()
            }
            None => {
                let mut labels: Vec<&String> =
                    self.prisms.keys().chain(self.revolves.keys()).collect();
                labels.sort();
                labels.first().map(|l| (*l).clone()).ok_or_else(|| {
                    anyhow!("`shell` hollows an extrusion or revolve and there is none")
                })?
            }
        };
        if self.revolves.contains_key(&label) {
            return self.shell_revolve(line, &label, thickness, &open);
        }
        let mut ends = Vec::new();
        for selector in &open {
            match selector.split_once('.') {
                Some((_, "end")) => ends.push(true),
                Some((_, "start")) => ends.push(false),
                _ => bail!("`open=` takes an extrusion's `label.end` or `label.start`"),
            }
        }
        let (open_end, open_start) = (ends.contains(&true), ends.contains(&false));
        let prism_of = self.prisms.get(&label).cloned().ok_or_else(|| anyhow!("`{label}` is not an extrusion; `shell` hollows the extrusion that owns the open face"))?;
        let solid = self.active("shell")?.clone();
        let clearance = clearance(&solid);
        let length = prism_of.distance.abs();
        let along = prism_of.frame.normal * prism_of.distance.signum();
        let from = if open_start { -clearance } else { thickness };
        let to = if open_end {
            length + clearance
        } else {
            length - thickness
        };
        if to <= from {
            bail!("a {thickness} wall leaves no room inside a {length} long extrusion");
        }
        let start = Frame {
            origin: prism_of.frame.origin + along * from,
            ..prism_of.frame
        };
        let inner: Vec<Profile> = prism_of
            .profiles
            .iter()
            .map(|p| p.inset(thickness))
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| anyhow!("shell: {e}"))?;
        let shapes = loops(&start, &inner)?;
        let here = label_of(line);
        for region in regions(&shapes) {
            let tool = prism(
                &start,
                &shapes,
                &region,
                &inner,
                along * (to - from),
                (0.0, 0.0),
            )?;
            let groups = classify(&tool, along)
                .into_iter()
                .map(|(group, s)| (if group == "start" { "floor" } else { "inside" }, s))
                .collect();
            self.record_inverted(&here, groups);
            self.merge(&here, tool, Combine::Remove)?;
        }
        self.describe_solid()
    }

    fn shell_revolve(
        &mut self,
        line: &Line,
        label: &str,
        thickness: f64,
        open: &[&str],
    ) -> Result<String> {
        let revolve = self.revolves[label].clone();
        if revolve.angle.abs() != 360.0 {
            bail!(
                "`shell` hollows full revolves; `{label}` turns {} degrees",
                revolve.angle
            );
        }
        let [profile] = revolve.profiles.as_slice() else {
            bail!("`shell` hollows a revolve of one profile");
        };
        let (start, segments) = profile
            .as_path()
            .ok_or_else(|| anyhow!("`shell` hollows revolves of rect, poly and pen paths"))?;
        let solid = self.active("shell")?.clone();
        let clearance = clearance(&solid);
        let tolerance = self.tolerance() * 10.0;
        let frame = revolve.frame;
        let axis_point = frame.local(revolve.through);
        let axis_direction = (revolve.axis.dot(frame.x), revolve.axis.dot(frame.y));
        let off_axis = |p: (f64, f64)| {
            ((p.0 - axis_point.0) * axis_direction.1 - (p.1 - axis_point.1) * axis_direction.0)
                .abs()
        };
        let faces = select::faces(&solid);
        let open_planes: Vec<Plane> = open
            .iter()
            .map(|selector| select::select_faces(selector, &solid, &self.groups, self.tolerance()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .filter_map(|i| match faces[i].oriented_surface() {
                Surface::Plane(plane) => Some(plane),
                _ => None,
            })
            .collect();
        if !open.is_empty() && open_planes.is_empty() {
            bail!("`open=` must pick flat faces of `{label}`");
        }
        let ends: Vec<(f64, f64)> = std::iter::once(start)
            .chain(segments.iter().map(Segment::end))
            .collect();
        let distances: Vec<f64> = segments
            .iter()
            .enumerate()
            .map(|(i, segment)| {
                let (a, b) = (ends[i], ends[i + 1]);
                let straight = matches!(segment, Segment::Line(_));
                if straight && off_axis(a) < tolerance && off_axis(b) < tolerance {
                    return 0.0;
                }
                let middle = frame.at((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                let opened = straight
                    && open_planes.iter().any(|plane| {
                        plane.normal().cross(revolve.axis).magnitude() < 1.0e-9
                            && (middle - plane.origin()).dot(plane.normal()).abs() < tolerance
                    });
                if opened { -clearance } else { thickness }
            })
            .collect();
        if revolve.alone.as_ref() == Some(&face_ids(&solid)) {
            let walled: Vec<bool> = distances.iter().map(|d| *d > 0.0).collect();
            let shared: Vec<f64> = distances.iter().map(|d| d.max(0.0)).collect();
            let (inner_start, inner) = crate::offset::offset_path(start, &segments, &shared)
                .map_err(|e| anyhow!("shell: {e}"))?;
            let walls = wall_profiles(start, &segments, &walled, inner_start, &inner);
            let here = label_of(line);
            self.solid = None;
            for tool in revolved(&frame, &walls, revolve.through, revolve.axis, 360.0)? {
                let groups = select::faces(&tool)
                    .iter()
                    .map(|face| ("inside", face.oriented_surface()))
                    .collect();
                self.record(&here, groups);
                self.merge(&here, tool, Combine::Add)?;
            }
            return self.describe_solid();
        }
        if distances.contains(&0.0) {
            bail!(
                "`{label}` touches its axis, so it can only be shelled while it is the whole solid; shell it before adding other features"
            );
        }
        let (inner_start, inner) = crate::offset::offset_path(start, &segments, &distances)
            .map_err(|e| anyhow!("shell: {e}"))?;
        let cavity = vec![Profile::Path {
            start: inner_start,
            segments: inner,
        }];
        let here = label_of(line);
        for tool in revolved(&frame, &cavity, revolve.through, revolve.axis, 360.0)? {
            let groups = select::faces(&tool)
                .iter()
                .map(|face| ("inside", face.oriented_surface()))
                .collect();
            self.record_inverted(&here, groups);
            self.merge(&here, tool, Combine::Remove)?;
        }
        self.describe_solid()
    }

    fn prism_part(&self, selector: &str) -> Option<(String, Prism, &'static str)> {
        let (owner, group) = selector.split_once('.')?;
        let group = ["start", "end", "side"].into_iter().find(|g| *g == group)?;
        let prism_of = self.prisms.get(owner)?.clone();
        Some((owner.to_string(), prism_of, group))
    }

    fn still_alone(&self, prism_of: &Prism) -> bool {
        prism_of.alone.is_some()
            && prism_of.alone.as_ref() == self.solid.as_ref().map(face_ids).as_ref()
    }

    fn draft_prism(
        &mut self,
        line: &Line,
        selector: &str,
        angle: f64,
        neutral: &Frame,
    ) -> Result<String> {
        let Some((owner, prism_of, "side")) = self.prism_part(selector) else {
            bail!(
                "`draft` tilts flat faces, or every side of an extrusion (`label.side`); `{selector}` is neither"
            );
        };
        let along = prism_of.frame.normal * prism_of.distance.signum();
        let length = prism_of.distance.abs();
        if neutral.normal.cross(along).magnitude() > 1.0e-9 {
            bail!("the neutral face must be square to `{owner}`");
        }
        let at = (neutral.origin - prism_of.frame.origin).dot(along);
        let (hinge, away) = if at.abs() < self.tolerance() * 10.0 {
            (prism_of.frame.origin, along)
        } else if (at - length).abs() < self.tolerance() * 10.0 {
            (prism_of.frame.origin + along * length, -along)
        } else {
            bail!("the neutral face must be the start or end of `{owner}`");
        };
        let outers = outer_profiles(&prism_of.frame, &prism_of.profiles)?;
        let [profile] = outers.as_slice() else {
            bail!("drafting curved sides works on an extrusion of one profile");
        };
        if prism_of.profiles.len() > 1 {
            bail!("drafting curved sides works on a profile without holes");
        }
        let slope = angle.tan();
        let label = label_of(line);
        if !self.still_alone(&prism_of) {
            bail!(
                "drafting curved sides needs `{owner}` to be the whole solid; draft before adding other features, or use `extrude ... draft=`"
            );
        }
        self.solid = None;
        let tool = tapered(
            &Frame {
                origin: hinge,
                ..prism_of.frame
            },
            profile,
            away * length,
            (0.0, length * slope),
        )?;
        let groups = select::faces(&tool)
            .iter()
            .filter(|face| !matches!(face.oriented_surface(), Surface::Plane(_)))
            .map(|face| ("faces", face.oriented_surface()))
            .collect();
        self.record(&label, groups);
        self.merge(&label, tool, Combine::Add)?;
        self.describe_solid()
    }

    pub(crate) fn op_thicken(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["faces", "t"], &[], false)?;
        let selector = args.text("faces")?;
        let thickness = positive(args.number("t", &self.scope)?, "thickness")?;
        match self.prism_part(selector) {
            Some((owner, prism_of, "side")) => {
                let shapes = loops(&prism_of.frame, &prism_of.profiles)?;
                let mut grown = Vec::new();
                for region in regions(&shapes) {
                    for (k, &i) in region.iter().enumerate() {
                        let profile = &prism_of.profiles[shapes[i].profile];
                        let delta = if k == 0 { -thickness } else { thickness };
                        grown.push(
                            profile
                                .inset(delta)
                                .map_err(|e| anyhow!("thicken `{owner}`: {e}"))?,
                        );
                    }
                }
                let along = prism_of.frame.normal * prism_of.distance.signum();
                let alone = self.still_alone(&prism_of);
                let label = label_of(line);
                if alone {
                    self.solid = None;
                }
                let pad = if !alone && self.on_existing_face(prism_of.frame.origin, along) {
                    (prism_of.distance.abs() * 0.02).max(1.0e-3)
                } else {
                    0.0
                };
                let start = Frame {
                    origin: prism_of.frame.origin - along * pad,
                    ..prism_of.frame
                };
                let shapes = loops(&start, &grown)?;
                for region in regions(&shapes) {
                    let tool = prism(
                        &start,
                        &shapes,
                        &region,
                        &grown,
                        along * (prism_of.distance.abs() + pad),
                        (0.0, 0.0),
                    )?;
                    let groups = classify(&tool, along)
                        .into_iter()
                        .filter(|(g, _)| *g == "side")
                        .collect();
                    self.record(&label, groups);
                    self.merge(&label, tool, Combine::Add)?;
                }
                if alone {
                    let mut updated = prism_of;
                    updated.profiles = grown;
                    updated.alone = self.solid.as_ref().map(face_ids);
                    self.prisms.insert(owner, updated);
                }
                self.describe_solid()
            }
            _ => {
                let solid = self.active("thicken")?.clone();
                let indices =
                    select::select_faces(selector, &solid, &self.groups, self.tolerance())?;
                let faces = select::faces(&solid);
                if indices
                    .iter()
                    .any(|&i| !matches!(faces[i].oriented_surface(), Surface::Plane(_)))
                {
                    bail!(
                        "`thicken` grows flat faces, or every side of an extrusion (`label.side`)"
                    );
                }
                self.op_push(&Line {
                    op: "push".to_string(),
                    positional: vec![selector.to_string(), thickness.to_string()],
                    named: Vec::new(),
                    ..line.clone()
                })
            }
        }
    }

    pub(crate) fn op_push(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["faces", "d"], &[], false)?;
        let selector = args.text("faces")?;
        let distance = args.number("d", &self.scope)?;
        if distance == 0.0 {
            bail!("push distance is zero");
        }
        let solid = self.active("push")?.clone();
        let indices = select::select_faces(selector, &solid, &self.groups, self.tolerance())?;
        if indices.is_empty() {
            bail!("`{selector}` matched no faces");
        }
        let label = label_of(line);
        if distance > 0.0 {
            let faces = select::faces(&solid);
            for index in indices {
                let face = &faces[index];
                let Surface::Plane(plane) = face.oriented_surface() else {
                    bail!("`push` moves flat faces; `{selector}` includes a curved one");
                };
                let tool = oriented(builder::extrude(
                    &builder::clone(face),
                    plane.normal() * distance,
                ));
                let groups = classify(&tool, plane.normal());
                self.record(&label, groups);
                self.merge(&label, tool, Combine::Add)?;
            }
        } else {
            let moved = builder::clone(&solid);
            let faces = select::faces(&moved);
            let sunk = indices
                .iter()
                .try_for_each(|&index| sink_face(&moved, &faces[index], distance, selector));
            match sunk {
                Ok(()) => self.solid = Some(moved),
                Err(error) => {
                    let Some((owner, prism_of, group)) = self.prism_part(selector) else {
                        return Err(error);
                    };
                    if group == "side" {
                        return Err(error);
                    }
                    let clearance = clearance(&solid);
                    let along = prism_of.frame.normal * prism_of.distance.signum();
                    let (plane, inward) = if group == "end" {
                        (
                            prism_of.frame.origin + along * prism_of.distance.abs(),
                            -along,
                        )
                    } else {
                        (prism_of.frame.origin, along)
                    };
                    let start = Frame {
                        origin: plane - inward * clearance,
                        ..prism_of.frame
                    };
                    let grown = outer_profiles(&prism_of.frame, &prism_of.profiles)?
                        .iter()
                        .map(|p| p.inset(-clearance))
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .map_err(|e| anyhow!("push `{owner}`: {e}"))?;
                    let shapes = loops(&start, &grown)?;
                    for region in regions(&shapes) {
                        let tool = prism(
                            &start,
                            &shapes,
                            &region,
                            &grown,
                            inward * (clearance - distance),
                            (0.0, 0.0),
                        )?;
                        let groups = classify(&tool, inward)
                            .into_iter()
                            .filter(|(group, _)| *group == "end")
                            .collect();
                        self.record_inverted(&label, groups);
                        self.merge(&label, tool, Combine::Remove)?;
                    }
                }
            }
        }
        self.describe_solid()
    }

    pub(crate) fn op_draft(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["angle", "faces"], &["neutral"], false)?;
        let angle = args.number("angle", &self.scope)?.to_radians();
        let selector = args.text("faces")?;
        let neutral = self.face_frame(args.text("neutral")?)?;
        let original = self.active("draft")?;
        let indices = select::select_faces(selector, original, &self.groups, self.tolerance())?;
        if indices.is_empty() {
            bail!("`{selector}` matched no faces");
        }
        let solid = builder::clone(original);
        let faces = select::faces(&solid);
        let pull = -neutral.normal;
        let curved = indices
            .iter()
            .any(|&index| !matches!(faces[index].oriented_surface(), Surface::Plane(_)));
        if curved {
            return self.draft_prism(line, selector, angle, &neutral);
        }
        let changes = indices
            .iter()
            .map(|&index| {
                let Surface::Plane(plane) = faces[index].oriented_surface() else {
                    bail!("`draft` tilts flat faces; `{selector}` includes a curved one");
                };
                let n = plane.normal();
                if n.dot(pull).abs() > 1.0e-9 {
                    bail!("draft faces must be square to the neutral plane");
                }
                let on_face = plane.origin();
                let hinge = on_face
                    + pull
                        * ((neutral.origin - on_face).dot(neutral.normal)
                            / pull.dot(neutral.normal));
                Ok((index, hinge, n * angle.cos() + pull * angle.sin()))
            })
            .collect::<Result<Vec<_>>>()?;
        replane(&solid, &changes)?;
        let label = label_of(line);
        changes.iter().for_each(|&(index, _, _)| {
            self.groups
                .record(&label, "faces", faces[index].oriented_surface())
        });
        self.solid = Some(solid);
        self.describe_solid()
    }
}

fn plane_through(point: Point3, normal: Vector3) -> Plane {
    let helper = if normal.x.abs() < 0.9 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    };
    let x = normal.cross(helper).normalize();
    let y = normal.cross(x);
    Plane::new(point, point + x, point + y)
}

fn replane(solid: &Solid, changes: &[(usize, Point3, Vector3)]) -> Result<()> {
    let faces = select::faces(solid);
    for &(index, point, normal) in changes {
        let face = &faces[index];
        let absolute = if face.orientation() { normal } else { -normal };
        face.set_surface(Surface::Plane(plane_through(point, absolute.normalize())));
    }
    let mut moved: Vec<Vertex> = Vec::new();
    changes.iter().for_each(|&(index, _, _)| {
        faces[index].vertex_iter().for_each(|v| {
            if !moved.contains(&v) {
                moved.push(v);
            }
        })
    });
    let planes = |vertex: &Vertex| -> Result<Vec<(Vector3, f64)>> {
        faces
            .iter()
            .filter(|face| face.vertex_iter().any(|v| v == *vertex))
            .map(|face| match face.surface() {
                Surface::Plane(plane) => Ok((plane.normal(), plane.normal().dot(plane.origin().to_vec()))),
                _ => bail!("a drafted face meets a curved face at a corner; draft works on flat-sided parts"),
            })
            .collect()
    };
    let corners = moved
        .iter()
        .map(|vertex| {
            let planes = planes(vertex)?;
            let n = planes.len();
            let triple = (0..n)
                .flat_map(|i| (i + 1..n).flat_map(move |j| (j + 1..n).map(move |k| (i, j, k))))
                .find(|&(i, j, k)| {
                    Matrix3::from_cols(planes[i].0, planes[j].0, planes[k].0)
                        .determinant()
                        .abs()
                        > 1.0e-6
                })
                .ok_or_else(|| anyhow!("a drafted corner is not fixed by three faces"))?;
            let (i, j, k) = triple;
            let rows = Matrix3::from_cols(planes[i].0, planes[j].0, planes[k].0).transpose();
            let inverse = rows
                .invert()
                .ok_or_else(|| anyhow!("a drafted corner is degenerate"))?;
            let point =
                Point3::from_vec(inverse * Vector3::new(planes[i].1, planes[j].1, planes[k].1));
            if planes
                .iter()
                .any(|(n, d)| (n.dot(point.to_vec()) - d).abs() > 1.0e-6 * (1.0 + d.abs()))
            {
                bail!(
                    "a drafted corner touches more than three faces that do not meet at one point"
                );
            }
            Ok(point)
        })
        .collect::<Result<Vec<_>>>()?;
    let edges: Vec<Edge> = solid
        .boundaries()
        .iter()
        .flat_map(|shell| shell.edge_iter())
        .filter(|edge| moved.contains(edge.front()) || moved.contains(edge.back()))
        .collect();
    if edges
        .iter()
        .any(|edge| !matches!(edge.curve(), Curve::Line(_)))
    {
        bail!("a drafted face has a curved edge; draft works on flat-sided parts");
    }
    moved
        .iter()
        .zip(corners)
        .for_each(|(vertex, point)| vertex.set_point(point));
    edges.iter().for_each(|edge| {
        edge.set_curve(Curve::Line(Line(
            edge.absolute_front().point(),
            edge.absolute_back().point(),
        )))
    });
    Ok(())
}
