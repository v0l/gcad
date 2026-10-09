use super::args::{Args, axis, label_of, point3, positive};
use super::solids::{loops, oriented, prism};
use super::{Combine, Model};
use crate::geometry::{self, Frame, Profile};
use crate::parse::Line;
use crate::select::{self, GroupEntry};
use anyhow::{Context, Result, anyhow, bail};
use monstertruck::modeling::*;

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

pub(crate) fn moved_surface(surface: &Surface, transform: Matrix4) -> Surface {
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

    pub fn named_body(&self, name: &str) -> Result<Solid> {
        if name == self.body_name() && self.solid.is_some() {
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

    pub(crate) fn op_color(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["colour"], &[], false)?;
        let colour = self.colour_of(args.text("colour")?)?;
        let name = self.body_name();
        self.colours.insert(name.clone(), colour);
        Ok(format!(
            "body `{name}` is {:.2},{:.2},{:.2}",
            colour[0], colour[1], colour[2]
        ))
    }

    pub(crate) fn op_material(&mut self, line: &Line) -> Result<String> {
        let name = if self.assembly {
            let part = line
                .positional
                .first()
                .ok_or_else(|| anyhow!("write `material part name [density=g/cm³]`"))?
                .clone();
            if !self.body_names().contains(&part) {
                bail!("no part called `{part}`; parts are {:?}", self.body_names());
            }
            part
        } else {
            self.body_name()
        };
        let wanted: &[&str] = if self.assembly { &["part"] } else { &[] };
        let args = Args::new(line, wanted, &["density"], true)?;
        let label = match args.rest.as_slice() {
            [] => None,
            [one] => Some(one.to_string()),
            _ => bail!("give one material name, like `material steel`"),
        };
        let density = match (args.optional_number("density", &self.scope)?, &label) {
            (Some(d), _) if d <= 0.0 => bail!("density must be above zero, not {d}"),
            (Some(d), _) => d,
            (None, Some(label)) => density_of(label).ok_or_else(|| {
                anyhow!(
                    "no density known for `{label}`; give `density=` in g/cm³, or use one of {}",
                    MATERIALS
                        .iter()
                        .map(|(n, _)| *n)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?,
            (None, None) => bail!("write `material steel` or `material name density=1.2`"),
        };
        let label = label.unwrap_or_else(|| format!("{density} g/cm³"));
        self.materials.insert(
            name.clone(),
            Material {
                name: label.clone(),
                density,
            },
        );
        Ok(format!("`{name}` is {label}, {density} g/cm³"))
    }

    pub fn named_solids(&self) -> Vec<(String, &Solid)> {
        self.bodies
            .iter()
            .map(|(n, s)| (n.clone(), s))
            .chain(self.solid.as_ref().map(|s| (self.body_name(), s)))
            .collect()
    }

    pub(crate) fn colour_of(&self, text: &str) -> Result<[f64; 3]> {
        let named = |name: &str| -> Option<[f64; 3]> {
            Some(match name {
                "red" => [0.85, 0.1, 0.1],
                "green" => [0.1, 0.6, 0.2],
                "blue" => [0.1, 0.3, 0.85],
                "yellow" => [0.95, 0.8, 0.1],
                "orange" => [0.95, 0.5, 0.1],
                "purple" => [0.5, 0.2, 0.7],
                "cyan" => [0.1, 0.7, 0.8],
                "magenta" => [0.85, 0.2, 0.6],
                "black" => [0.05, 0.05, 0.05],
                "white" => [0.95, 0.95, 0.95],
                "grey" | "gray" => [0.5, 0.5, 0.5],
                "silver" => [0.75, 0.75, 0.78],
                _ => return None,
            })
        };
        let colour = if let Some(hex) = text.strip_prefix('#') {
            let channel = |i: usize| {
                hex.get(i..i + 2)
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                    .map(|v| v as f64 / 255.0)
            };
            match (hex.len(), channel(0), channel(2), channel(4)) {
                (6, Some(r), Some(g), Some(b)) => [r, g, b],
                _ => bail!("`{text}` is not a colour; write #rrggbb"),
            }
        } else if text.contains(',') {
            let p = point3(text, &self.scope)?;
            let rgb = [p.x, p.y, p.z];
            if rgb.iter().any(|c| !(0.0..=1.0).contains(c)) {
                bail!("colour parts go from 0 to 1, got {text}");
            }
            rgb
        } else {
            named(text).ok_or_else(|| {
                anyhow!("`{text}` is not a colour; use a name like red or grey, #rrggbb, or r,g,b from 0 to 1")
            })?
        };
        Ok(colour)
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
        let args = Args::new(line, &["file"], &["solid"], false)?;
        let file = std::path::PathBuf::from(args.text("file")?);
        let path = match &self.dir {
            Some(dir) if file.is_relative() => dir.join(&file),
            _ => file,
        };
        let stl = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("stl"));
        let pick = match args.optional_number("solid", &self.scope)? {
            Some(n) if n.fract() == 0.0 && n >= 1.0 => Some(n as usize - 1),
            Some(_) => bail!("`solid` must be a whole number from 1"),
            None => None,
        };
        let found = if stl {
            vec![(String::new(), super::import::stl_solid(&path)?)]
        } else {
            read_step(&path, pick).with_context(|| format!("importing {}", path.display()))?
        };
        let listing = || {
            found
                .iter()
                .enumerate()
                .map(|(k, (name, solid))| {
                    let b = geometry::bounds(solid);
                    let (lo, hi) = (b.min(), b.max());
                    format!(
                        "{}: `{name}` {:.1} x {:.1} x {:.1}",
                        k + 1,
                        hi.x - lo.x,
                        hi.y - lo.y,
                        hi.z - lo.z
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        };
        let solid = match (pick, found.as_slice()) {
            (_, [(_, only)]) => only.clone(),
            (Some(_), _) => bail!(
                "`solid` must be 1 to {}; the file holds {}",
                found.len(),
                listing()
            ),
            (None, _) => bail!(
                "the file holds {} solids, pick one with `solid=`: {}",
                found.len(),
                listing()
            ),
        };
        let label = label_of(line);
        let groups = select::faces(&solid)
            .iter()
            .map(|face| ("faces", face.oriented_surface()))
            .collect();
        self.record(&label, groups);
        self.merge(&label, solid, Combine::Add)?;
        self.describe_solid()
    }
}

fn read_step(path: &std::path::Path, pick: Option<usize>) -> Result<Vec<(String, Solid)>> {
    use monstertruck::step::load::Table;
    use monstertruck::step::load::step_geometry::{Curve3D, Surface as StepSurface};
    let text = std::fs::read(path)?;
    let table = Table::from_step_bytes(&text).map_err(|error| anyhow!("{error}"))?;
    let mut breps: Vec<_> = table.manifold_solid_brep.iter().collect();
    if breps.is_empty() {
        bail!("the file has no solids");
    }
    breps.sort_by_key(|(id, _)| **id);
    let chosen: Vec<_> = match pick {
        Some(index) if index < breps.len() => vec![breps[index]],
        _ => breps,
    };
    chosen
        .into_iter()
        .map(|(_, holder)| {
            let compressed = table
                .to_compressed_trimmed_solid(holder)
                .map_err(|error| anyhow!("{error}"))?;
            let solid = monstertruck::healing::extract_healed_trimmed_solid(compressed, 1.0e-3)
                .map_err(|error| anyhow!("{error}"))?
                .erase_trims()
                .try_mapped(
                    |point| Some(*point),
                    |curve: &Curve3D| Curve::try_from(curve).ok(),
                    |surface: &StepSurface| Surface::try_from(surface).ok(),
                )
                .ok_or_else(|| anyhow!("the file uses a curve or surface gcad cannot read"))?;
            Ok((holder.label.clone(), oriented(solid)))
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub density: f64,
}

pub const MATERIALS: &[(&str, f64)] = &[
    ("steel", 7.85),
    ("stainless", 8.0),
    ("aluminium", 2.7),
    ("aluminum", 2.7),
    ("brass", 8.5),
    ("copper", 8.96),
    ("titanium", 4.43),
    ("pla", 1.24),
    ("petg", 1.27),
    ("abs", 1.04),
    ("asa", 1.07),
    ("nylon", 1.14),
    ("tpu", 1.21),
    ("polycarbonate", 1.2),
    ("acrylic", 1.18),
    ("resin", 1.2),
    ("wood", 0.6),
];

pub fn density_of(name: &str) -> Option<f64> {
    let name = name.to_ascii_lowercase();
    MATERIALS.iter().find(|(n, _)| *n == name).map(|(_, d)| *d)
}
