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

impl Model {
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
        let args = Args::new(line, &["on"], &["offset"], false)?;
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
        let args = Args::new(line, &["of"], &["count", "step", "angle"], false)?;
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
        let place: Box<dyn Fn(usize) -> Matrix4> = match (
            args.optional_point("step", &self.scope)?,
            args.optional_number("angle", &self.scope)?,
        ) {
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
                "give `step=x,y` for a row or `angle=` for a circle about the workplane normal"
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

    pub(crate) fn op_move(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["by"], &[], false)?;
        let by = point3(args.text("by")?, &self.scope)?;
        self.transform_all(Matrix4::from_translation(by.to_vec()))?;
        self.describe_solid()
    }

    pub(crate) fn op_rotate(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["angle"], &["axis", "about"], false)?;
        let angle = args.number("angle", &self.scope)?;
        let direction = axis(args.values.get("axis").copied().unwrap_or("z"))?;
        let center = args
            .values
            .get("about")
            .map(|text| point3(text, &self.scope))
            .transpose()?
            .unwrap_or_else(Point3::origin);
        self.transform_all(about(
            center,
            Matrix4::from_axis_angle(direction, Deg(angle)),
        ))?;
        self.describe_solid()
    }

    pub(crate) fn op_scale(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["factor"], &["about"], false)?;
        let factor = positive(args.number("factor", &self.scope)?, "scale factor")?;
        let center = args
            .values
            .get("about")
            .map(|text| point3(text, &self.scope))
            .transpose()?
            .unwrap_or_else(Point3::origin);
        self.transform_all(about(center, Matrix4::from_scale(factor)))?;
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
