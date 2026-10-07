use super::args::{Args, combine_mode, label_of, positive};
use super::solids::{classify, loft_wires, loops, oriented, prism, rational, regions};
use super::{Combine, Model, Prism};
use crate::geometry::{self, Frame, Profile};
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

fn sink_face(solid: &Solid, face: &Face, distance: f64, selector: &str) -> Result<()> {
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

    pub(crate) fn op_extrude(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["d", "how"], &["draft", "mode", "upto"], false)?;
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
        let (frame, profiles) = self.take_sketch("extrude")?;
        let (distance, end_overlap) = match args.values.get("upto") {
            Some(selector) => {
                if args.has("d") {
                    bail!("give either a distance or `upto=`, not both");
                }
                let target = self.face_frame(selector)?;
                let distance = (target.origin - frame.origin).dot(target.normal)
                    / frame.normal.dot(target.normal);
                if distance.abs() < 1.0e-9 {
                    bail!("`{selector}` is on the sketch plane");
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
        let start_overlap = match combine {
            Combine::Add if !both && self.on_existing_face(frame.origin, normal) => pad,
            Combine::Common if self.on_existing_face(base.origin, -normal) => pad,
            _ => 0.0,
        };
        let end_overlap = match combine {
            Combine::Common
                if self.on_existing_face(base.origin + normal * distance.abs(), normal) =>
            {
                end_overlap.max(pad)
            }
            _ => end_overlap,
        };
        let start = base.offset(-start_overlap * distance.signum());
        let direction = normal * (distance.abs() + start_overlap + end_overlap);
        let label = label_of(line);
        let shapes = loops(&start, &profiles)?;
        for region in regions(&shapes) {
            let insets = (
                -start_overlap * taper,
                (distance.abs() + end_overlap) * taper,
            );
            let tool = prism(&start, &shapes, &region, &profiles, direction, insets)?;
            let groups = classify(&tool, direction);
            match combine {
                Combine::Remove => self.record_inverted(&label, groups),
                _ => self.record(&label, groups),
            }
            self.merge(&label, tool, combine)?;
        }
        self.prisms.insert(
            label,
            Prism {
                frame: base,
                profiles,
                distance,
            },
        );
        self.describe_solid()
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
            self.merge(label, tool, Combine::Remove)?;
        }
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
        let shapes = loops(&frame, &profiles)?;
        for region in regions(&shapes) {
            let wires: Vec<Wire> = region.iter().map(|&i| shapes[i].wire.clone()).collect();
            let face: Face = profile::attach_plane_normalized(wires)
                .map_err(|error| anyhow!("cannot face the sketch: {error}"))?;
            let tool = oriented(rational(
                builder::revolve(&face, frame.origin, axis, sweep, division),
                angle.abs().to_radians() / division as f64,
            ));
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
        self.describe_solid()
    }

    pub(crate) fn op_loft(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &["mode"], false)?;
        let combine = combine_mode(&args)?;
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
        if counts.windows(2).any(|pair| pair[0] != pair[1]) {
            bail!(
                "sections have {counts:?} edges; loft needs the same number in each (a rect and a poly of 4 points, a circle and a circle)"
            );
        }
        let tool = loft_wires(&wires)?;
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
        let open = args.values.get("open").copied();
        let owner = match open {
            Some(selector) => selector
                .split_once('.')
                .filter(|(_, group)| *group == "end" || *group == "start")
                .map(|(label, group)| (label.to_string(), Some(group == "end")))
                .ok_or_else(|| {
                    anyhow!("`open=` takes an extrusion's `label.end` or `label.start`")
                })?,
            None => {
                let mut labels: Vec<&String> = self.prisms.keys().collect();
                labels.sort();
                let label = labels
                    .first()
                    .ok_or_else(|| anyhow!("`shell` hollows an extrusion and there is none"))?;
                ((*label).clone(), None)
            }
        };
        let (label, open_end) = owner;
        let prism_of = self.prisms.get(&label).cloned().ok_or_else(|| anyhow!("`{label}` is not an extrusion; `shell` hollows the extrusion that owns the open face"))?;
        let solid = self.active("shell")?.clone();
        let clearance = clearance(&solid);
        let length = prism_of.distance.abs();
        let along = prism_of.frame.normal * prism_of.distance.signum();
        let (from, to) = match open_end {
            Some(true) => (thickness, length + clearance),
            Some(false) => (-clearance, length - thickness),
            None => (thickness, length - thickness),
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
            for index in indices {
                sink_face(&moved, &faces[index], distance, selector)?;
            }
            self.solid = Some(moved);
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
