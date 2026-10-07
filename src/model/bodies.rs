use super::args::{Args, axis, label_of, point3, positive};
use super::solids::{loops, oriented, prism};
use super::{Combine, Model};
use crate::geometry::{self, Frame, Profile};
use crate::parse::Line;
use crate::select::{self, GroupEntry};
use anyhow::{Context, Result, anyhow, bail};
use monstertruck::modeling::*;
use monstertruck::topology::compress::{CompressedFace, CompressedShell, CompressedSolid};

fn about(point: Point3, transform: Matrix4) -> Matrix4 {
    Matrix4::from_translation(point.to_vec())
        * transform
        * Matrix4::from_translation(-point.to_vec())
}

fn reflection(frame: &Frame) -> Matrix4 {
    let n = frame.normal;
    let linear = Matrix3::from_cols(
        Vector3::unit_x() - n * (2.0 * n.x),
        Vector3::unit_y() - n * (2.0 * n.y),
        Vector3::unit_z() - n * (2.0 * n.z),
    );
    about(frame.origin, Matrix4::from(linear))
}

fn moved(solid: &Solid, transform: Matrix4) -> Solid {
    oriented(builder::transformed(solid, transform))
}

fn moved_surface(surface: &Surface, transform: Matrix4) -> Surface {
    let mut moved = surface.transformed(transform);
    if transform.determinant() < 0.0 {
        moved.invert();
    }
    moved
}

fn copy_flag(args: &Args<'_>) -> Result<bool> {
    match args.values.get("copy").copied() {
        None => Ok(false),
        Some("copy") => Ok(true),
        Some(other) => bail!("`{other}` is not an option; did you mean `copy`?"),
    }
}

impl Model {
    fn body_name(&self) -> String {
        if self.body.is_empty() {
            "main".to_string()
        } else {
            self.body.clone()
        }
    }

    pub(crate) fn named_body(&self, name: &str) -> Result<Solid> {
        if name == self.body_name() {
            return self
                .solid
                .clone()
                .ok_or_else(|| anyhow!("body `{name}` has no solid yet"));
        }
        self.bodies
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s.clone())
            .ok_or_else(|| {
                anyhow!(
                    "no body called `{name}`; bodies are {:?}",
                    self.bodies
                        .iter()
                        .map(|(n, _)| n.clone())
                        .chain(std::iter::once(self.body_name()))
                        .collect::<Vec<_>>()
                )
            })
    }

    pub(crate) fn op_combine(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["into", "from"], &["mode"], false)?;
        let combine = super::args::combine_mode(&args)?;
        let (into, from) = (
            args.text("into")?.to_string(),
            args.text("from")?.to_string(),
        );
        if into == from {
            bail!("combine two different bodies");
        }
        let (a, b) = (self.named_body(&into)?, self.named_body(&from)?);
        let result = match combine {
            Combine::Add => monstertruck::solid::or_normalized(&a, &b),
            Combine::Remove => monstertruck::solid::difference_normalized(&a, &b),
            Combine::Common => monstertruck::solid::and_normalized(&a, &b),
        }
        .map_err(|error| anyhow!("combine failed: {error}"))?;
        let active = self.body_name();
        let mut others: Vec<(String, Solid)> = self
            .bodies
            .drain(..)
            .filter(|(n, _)| *n != into && *n != from)
            .collect();
        if active != into
            && active != from
            && let Some(solid) = self.solid.take()
        {
            others.push((active, solid));
        }
        self.bodies = others;
        self.solid = Some(result);
        self.body = into;
        self.describe_solid()
    }

    pub(crate) fn op_place(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["body"], &["on"], false)?;
        let name = args.text("body")?.to_string();
        let selector = args.text("on")?;
        let tolerance = self.tolerance();
        let target = self
            .bodies
            .iter()
            .map(|(n, s)| (n.clone(), s.clone()))
            .chain(self.solid.clone().map(|s| (self.body_name(), s)))
            .filter(|(n, _)| *n != name)
            .find_map(|(_, solid)| {
                let faces = select::faces(&solid);
                select::select_faces(selector, &solid, &self.groups, tolerance)
                    .ok()?
                    .into_iter()
                    .find_map(|i| match faces[i].oriented_surface() {
                        Surface::Plane(plane) => Some(plane),
                        _ => None,
                    })
            })
            .ok_or_else(|| anyhow!("`{selector}` matched no flat face on the other bodies"))?;
        let normal = target.normal();
        let level = normal.dot(target.origin().to_vec());
        let body = self.named_body(&name)?;
        let lowest = body
            .boundaries()
            .iter()
            .flat_map(|shell| shell.vertex_iter())
            .map(|v| normal.dot(v.point().to_vec()))
            .fold(f64::INFINITY, f64::min);
        let shifted = moved(&body, Matrix4::from_translation(normal * (level - lowest)));
        if name == self.body_name() {
            self.solid = Some(shifted);
        } else if let Some(entry) = self.bodies.iter_mut().find(|(n, _)| *n == name) {
            entry.1 = shifted;
        }
        Ok(format!(
            "moved `{name}` {:.3} onto the face",
            level - lowest
        ))
    }

    fn transform_all(&mut self, transform: Matrix4) -> Result<()> {
        let solid = self
            .solid
            .take()
            .ok_or_else(|| anyhow!("there is no solid to move"))?;
        self.solid = Some(moved(&solid, transform));
        self.groups
            .0
            .iter_mut()
            .for_each(|entry| entry.surface = moved_surface(&entry.surface, transform));
        self.tools
            .values_mut()
            .flatten()
            .for_each(|(_, tool)| *tool = moved(tool, transform));
        Ok(())
    }

    fn copy_groups(&mut self, label: Option<&str>, transform: Matrix4, as_label: &str) {
        let copies: Vec<GroupEntry> = self
            .groups
            .0
            .iter()
            .filter(|entry| label.is_none_or(|l| entry.label == l))
            .map(|entry| GroupEntry {
                label: as_label.to_string(),
                group: entry.group.clone(),
                surface: moved_surface(&entry.surface, transform),
            })
            .collect();
        self.groups.0.extend(copies);
    }

    pub(crate) fn op_mirror(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["on"], &["offset", "of"], false)?;
        if let Some(of) = args.values.get("of") {
            let frame = self
                .plane_from(args.text("on")?)?
                .offset(args.optional_number("offset", &self.scope)?.unwrap_or(0.0));
            let transform = reflection(&frame);
            let tools = self
                .tools
                .get(*of)
                .cloned()
                .ok_or_else(|| anyhow!("`{of}` made no material to mirror"))?;
            let label = label_of(line);
            self.copy_groups(Some(of), transform, of);
            for (combine, tool) in tools {
                self.merge(&label, moved(&tool, transform), combine)?;
            }
            return self.describe_solid();
        }
        let frame = self
            .plane_from(args.text("on")?)?
            .offset(args.optional_number("offset", &self.scope)?.unwrap_or(0.0));
        let transform = reflection(&frame);
        let copy = moved(self.active("mirror")?, transform);
        let label = label_of(line);
        self.copy_groups(None, transform, &label);
        let originals: Vec<GroupEntry> = self
            .groups
            .0
            .iter()
            .filter(|e| e.label != label)
            .cloned()
            .collect();
        originals.iter().for_each(|entry| {
            self.groups.record(
                &entry.label,
                &entry.group,
                moved_surface(&entry.surface, transform),
            )
        });
        self.merge(&label, copy, Combine::Add)?;
        self.describe_solid()
    }

    pub(crate) fn op_repeat(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["of"], &["count", "step", "angle", "along"], false)?;
        let of = args.text("of")?;
        let tools = self.tools.get(of).cloned().ok_or_else(|| {
            let mut known: Vec<&String> = self.tools.keys().collect();
            known.sort();
            anyhow!("`{of}` made no material to repeat; lines that did: {known:?}")
        })?;
        let count = args.number("count", &self.scope)?;
        if count < 2.0 || count.fract() != 0.0 {
            bail!("count must be a whole number of at least 2, got {count}");
        }
        let count = count as usize;
        let frame = self.sketch_frame();
        let along = match args.values.get("along").copied() {
            None => None,
            Some("path") => {
                let path = self
                    .path
                    .clone()
                    .ok_or_else(|| anyhow!("`along=path` needs a `path` or `helix` first"))?;
                let open = !matches!(&path, super::SweepPath::Polyline { points, .. } if (points[0] - points[points.len() - 1]).magnitude() < 1.0e-9);
                let steps = if open { count - 1 } else { count };
                let (start, start_tangent) = path.at(0.0)?;
                let stations = (1..count)
                    .map(|k| {
                        let (point, tangent) = path.at(k as f64 / steps as f64)?;
                        let turn = start_tangent.cross(tangent);
                        let rotation = if turn.magnitude() < 1.0e-12 {
                            Matrix4::identity()
                        } else {
                            Matrix4::from_axis_angle(
                                turn.normalize(),
                                Rad(start_tangent.dot(tangent).clamp(-1.0, 1.0).acos()),
                            )
                        };
                        Ok(Matrix4::from_translation(point.to_vec())
                            * rotation
                            * Matrix4::from_translation(-start.to_vec()))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Some(stations)
            }
            Some(other) => bail!("`along=` takes `path`, got `{other}`"),
        };
        let place: Box<dyn Fn(usize) -> Matrix4> = match (
            args.optional_point("step", &self.scope)?,
            args.optional_number("angle", &self.scope)?,
        ) {
            _ if along.is_some() => {
                if args.has("step") || args.has("angle") {
                    bail!("give `along=`, `step=` or `angle=`, only one");
                }
                let stations = along.expect("checked");
                Box::new(move |k| stations[k - 1])
            }
            (Some((x, y)), None) => {
                let step = frame.x * x + frame.y * y;
                Box::new(move |k| Matrix4::from_translation(step * k as f64))
            }
            (None, Some(angle)) => {
                let delta = if (angle.abs() - 360.0).abs() < 1.0e-9 {
                    angle / count as f64
                } else {
                    angle / (count - 1) as f64
                };
                Box::new(move |k| {
                    about(
                        frame.origin,
                        Matrix4::from_axis_angle(frame.normal, Deg(delta * k as f64)),
                    )
                })
            }
            _ => bail!(
                "give `step=x,y` for a row, `angle=` for a circle about the workplane normal, or `along=path`"
            ),
        };
        let label = label_of(line);
        for k in 1..count {
            let transform = place(k);
            self.copy_groups(Some(of), transform, of);
            for (combine, tool) in &tools {
                self.merge(&label, moved(tool, transform), *combine)?;
            }
        }
        self.describe_solid()
    }

    fn place_or_copy(&mut self, line: &Line, transform: Matrix4, copy: bool) -> Result<String> {
        if copy {
            let duplicate = moved(self.active(&line.op)?, transform);
            let label = label_of(line);
            self.copy_groups(None, transform, &label);
            self.merge(&label, duplicate, Combine::Add)?;
        } else {
            self.transform_all(transform)?;
        }
        self.describe_solid()
    }

    pub(crate) fn op_move(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["by", "copy"], &[], false)?;
        let by = point3(args.text("by")?, &self.scope)?;
        let copy = copy_flag(&args)?;
        self.place_or_copy(line, Matrix4::from_translation(by.to_vec()), copy)
    }

    pub(crate) fn op_rotate(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["angle", "copy"], &["axis", "about"], false)?;
        let copy = copy_flag(&args)?;
        let angle = args.number("angle", &self.scope)?;
        let named = args
            .values
            .get("axis")
            .and_then(|name| self.axes.get(*name))
            .copied();
        let direction = match named {
            Some((_, direction)) => direction,
            None => axis(args.values.get("axis").copied().unwrap_or("z"))?,
        };
        let center = match (args.values.get("about"), named) {
            (Some(_), Some(_)) => {
                bail!("a datum axis already passes through a point; drop `about=`")
            }
            (Some(text), None) => point3(text, &self.scope)?,
            (None, Some((through, _))) => through,
            (None, None) => Point3::origin(),
        };
        self.place_or_copy(
            line,
            about(center, Matrix4::from_axis_angle(direction, Deg(angle))),
            copy,
        )
    }

    pub(crate) fn op_scale(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["factor"], &["about"], false)?;
        let text = args.text("factor")?;
        let factors = if text.contains(',') {
            let p = point3(text, &self.scope)?;
            [p.x, p.y, p.z]
        } else {
            [args.number("factor", &self.scope)?; 3]
        };
        factors
            .iter()
            .try_for_each(|f| positive(*f, "scale factor").map(|_| ()))?;
        let center = args
            .values
            .get("about")
            .map(|text| point3(text, &self.scope))
            .transpose()?
            .unwrap_or_else(Point3::origin);
        self.transform_all(about(
            center,
            Matrix4::from_nonuniform_scale(factors[0], factors[1], factors[2]),
        ))?;
        self.describe_solid()
    }

    pub(crate) fn op_split(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["on"], &["offset", "keep"], false)?;
        let frame = self
            .plane_from(args.text("on")?)?
            .offset(args.optional_number("offset", &self.scope)?.unwrap_or(0.0));
        let side = match args.values.get("keep").copied() {
            Some("below") => -1.0,
            Some("above") => 1.0,
            _ => {
                bail!("`split` needs keep=below or keep=above (below is against the plane normal)")
            }
        };
        let solid = self.active("split")?;
        let bounds = geometry::bounds(solid);
        let size = bounds.diameter() * 4.0;
        let (u, v) = frame.local(bounds.center());
        let block = Profile::Rect {
            center: (u, v),
            width: size,
            height: size,
            radius: 0.0,
        };
        let shapes = loops(&frame, std::slice::from_ref(&block))?;
        let tool = prism(
            &frame,
            &shapes,
            &[0],
            std::slice::from_ref(&block),
            frame.normal * (side * size),
            (0.0, 0.0),
        )?;
        let label = label_of(line);
        let mut cut_face = Surface::Plane(Plane::new(
            frame.origin,
            frame.origin + frame.x,
            frame.origin + frame.y,
        ));
        if side < 0.0 {
            cut_face.invert();
        }
        self.groups.record(&label, "cut", cut_face);
        self.merge(&label, tool, Combine::Common)?;
        self.describe_solid()
    }

    pub(crate) fn op_body(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["name"], &[], false)?;
        self.require_empty_sketch("body")?;
        let name = args.text("name")?.to_string();
        if self.body == name || self.bodies.iter().any(|(n, _)| *n == name) {
            bail!("there is already a body called `{name}`");
        }
        if let Some(solid) = self.solid.take() {
            let previous = if self.body.is_empty() {
                "main".to_string()
            } else {
                self.body.clone()
            };
            self.bodies.push((previous, solid));
        }
        self.body = name;
        Ok(format!(
            "{} finished bod{}; new body `{}`",
            self.bodies.len(),
            if self.bodies.len() == 1 { "y" } else { "ies" },
            self.body
        ))
    }

    pub(crate) fn op_import(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["file"], &[], false)?;
        let file = std::path::PathBuf::from(args.text("file")?);
        let path = match &self.dir {
            Some(dir) if file.is_relative() => dir.join(&file),
            _ => file,
        };
        let solids = read_step(&path).with_context(|| format!("importing {}", path.display()))?;
        let label = label_of(line);
        for solid in solids {
            let groups = select::faces(&solid)
                .iter()
                .map(|face| ("faces", face.oriented_surface()))
                .collect();
            self.record(&label, groups);
            self.merge(&label, solid, Combine::Add)?;
        }
        self.describe_solid()
    }
}

fn read_step(path: &std::path::Path) -> Result<Vec<Solid>> {
    use monstertruck::step::load::Table;
    let text = std::fs::read(path)?;
    let table = Table::from_step_bytes(&text).map_err(|error| anyhow!("{error}"))?;
    let solids = table
        .manifold_solid_brep
        .values()
        .map(|holder| {
            let compressed = table
                .to_compressed_solid(holder)
                .map_err(|error| anyhow!("{error}"))?;
            let boundaries = compressed
                .boundaries
                .into_iter()
                .map(|shell| {
                    let edges = shell
                        .edges
                        .into_iter()
                        .map(|edge| {
                            Curve::try_from(&edge.curve)
                                .map(|curve| monstertruck::topology::compress::CompressedEdge {
                                    vertices: edge.vertices,
                                    curve,
                                })
                                .map_err(|error| anyhow!("unsupported curve: {error:?}"))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let faces = shell
                        .faces
                        .into_iter()
                        .map(|face| {
                            Surface::try_from(&face.surface)
                                .map(|surface| CompressedFace {
                                    boundaries: face.boundaries,
                                    orientation: face.orientation,
                                    surface,
                                })
                                .map_err(|error| anyhow!("unsupported surface: {error:?}"))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Ok(CompressedShell {
                        vertices: shell.vertices,
                        edges,
                        faces,
                        vertex_stable_ids: None,
                        edge_stable_ids: None,
                        face_stable_ids: None,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let solid = Solid::extract(CompressedSolid {
                boundaries,
                id_allocator: None,
                attributes: None,
            })
            .map_err(|error| anyhow!("{error}"))?;
            Ok(oriented(solid))
        })
        .collect::<Result<Vec<_>>>()?;
    if solids.is_empty() {
        bail!("the file has no solids");
    }
    Ok(solids)
}
