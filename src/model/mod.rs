mod args;
mod assembly;
mod blends;
mod bodies;
mod constrain;
mod features;
mod holes;
mod import;
pub mod mate;
mod measure;
mod path;
mod pattern;
mod rib;
mod round;
mod sketch;
mod skin;
mod solids;
mod weld;
mod wrap;

use crate::geometry::{self, Frame, Profile, Segment};
use crate::parse::{Line, Scope};
use crate::select::{self, Groups};
use anyhow::{Result, anyhow, bail};
use monstertruck::meshing::prelude::*;
use monstertruck::modeling::*;
use std::collections::HashMap;
use std::path::PathBuf;

pub use args::label_of;
pub use assembly::{
    ASSEMBLY_OPERATIONS, Couple, Joint, JointKind, Mate, Rig, Source, broken, explode_offsets,
    posed, subtree,
};
pub use measure::{MassProperties, mass_properties};
pub use path::SweepPath;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Combine {
    Add,
    Remove,
    Common,
}

#[derive(Clone, Debug)]
pub struct Pen {
    pub start: (f64, f64),
    pub cursor: (f64, f64),
    pub segments: Vec<Segment>,
}

#[derive(Clone, Debug)]
pub struct Prism {
    pub frame: Frame,
    pub profiles: Vec<Profile>,
    pub distance: f64,
    pub alone: Option<Vec<monstertruck::topology::FaceId<Surface>>>,
}

#[derive(Clone, Default)]
pub struct Model {
    pub scope: Scope,
    pub frame: Option<Frame>,
    pub sketch: Vec<Profile>,
    pub pen: Option<Pen>,
    pub sections: Vec<(Frame, Vec<Profile>)>,
    pub solid: Option<Solid>,
    pub bodies: Vec<(String, Solid)>,
    pub body: String,
    pub groups: Groups,
    pub path: Option<SweepPath>,
    pub tools: HashMap<String, Vec<(Combine, Solid)>>,
    pub prisms: HashMap<String, Prism>,
    pub dir: Option<PathBuf>,
    pub fixed: Scope,
    pub depth: usize,
    pub points: Vec<constrain::SketchPoint>,
    pub constraints: Vec<constrain::Constraint>,
    pub construction: Vec<(Frame, Profile)>,
    pub axes: HashMap<String, (Point3, Vector3)>,
    pub colours: HashMap<String, [f64; 3]>,
    pub materials: HashMap<String, bodies::Material>,
    pub sources: HashMap<String, assembly::Source>,
    pub placements: HashMap<String, Matrix4>,
    pub explode: Vec<(String, Vector3)>,
    pub revolves: HashMap<String, features::Revolve>,
    pub joints: Vec<assembly::Joint>,
    pub assembly: bool,
    pub part_groups: HashMap<String, select::Groups>,
    pub mates: Vec<assembly::Mate>,
    pub couples: Vec<assembly::Couple>,
    pub cache: Cache,
}

#[derive(Clone, Default)]
pub struct Cache(std::sync::Arc<std::sync::Mutex<CacheEntries>>);

#[derive(Default)]
struct CacheEntries {
    parts: HashMap<String, std::sync::Arc<Model>>,
    cylinders: HashMap<String, Vec<mate::Cylinder>>,
}

impl Cache {
    pub(crate) fn part(
        &self,
        key: String,
        load: impl FnOnce() -> Result<Model>,
    ) -> Result<std::sync::Arc<Model>> {
        if let Some(found) = self.0.lock().ok().and_then(|c| c.parts.get(&key).cloned()) {
            return Ok(found);
        }
        let loaded = std::sync::Arc::new(load()?);
        if let Ok(mut entries) = self.0.lock() {
            entries.parts.insert(key, loaded.clone());
        }
        Ok(loaded)
    }

    pub(crate) fn cylinders(
        &self,
        key: String,
        fit: impl FnOnce() -> Vec<mate::Cylinder>,
    ) -> Vec<mate::Cylinder> {
        if let Some(found) = self
            .0
            .lock()
            .ok()
            .and_then(|c| c.cylinders.get(&key).cloned())
        {
            return found;
        }
        let fitted = fit();
        if let Ok(mut entries) = self.0.lock() {
            entries.cylinders.insert(key, fitted.clone());
        }
        fitted
    }
}

pub const OPERATIONS: &[&str] = &[
    "let",
    "wrap",
    "material",
    "plane",
    "rect",
    "circle",
    "poly",
    "ngon",
    "gear",
    "offset",
    "slot",
    "ellipse",
    "pen",
    "line",
    "arc",
    "close",
    "spline",
    "text",
    "section",
    "loft",
    "extrude",
    "cut",
    "revolve",
    "path",
    "helix",
    "sweep",
    "hole",
    "fillet",
    "chamfer",
    "shell",
    "draft",
    "push",
    "mirror",
    "repeat",
    "move",
    "rotate",
    "scale",
    "split",
    "body",
    "combine",
    "place",
    "measure",
    "import",
    "if",
    "include",
    "point",
    "dist",
    "horizontal",
    "vertical",
    "angle",
    "reflect",
    "array",
    "axis",
];

const PROFILE_OPS: &[&str] = &[
    "rect", "circle", "poly", "ngon", "gear", "offset", "slot", "ellipse", "close", "spline",
    "text",
];

impl Model {
    pub fn tolerance(&self) -> f64 {
        self.solid.as_ref().map_or(1.0e-6, |solid| {
            (geometry::bounds(solid).diameter() * 1.0e-6).max(1.0e-7)
        })
    }

    pub fn solids(&self) -> Vec<&Solid> {
        self.bodies
            .iter()
            .map(|(_, solid)| solid)
            .chain(self.solid.as_ref())
            .collect()
    }

    pub fn parts(&self) -> Vec<(&Solid, Option<[f64; 3]>)> {
        let current = if self.body.is_empty() {
            "main"
        } else {
            self.body.as_str()
        };
        self.bodies
            .iter()
            .map(|(name, solid)| (solid, self.colours.get(name).copied()))
            .chain(
                self.solid
                    .as_ref()
                    .map(|solid| (solid, self.colours.get(current).copied())),
            )
            .collect()
    }

    pub fn exploded_parts(&self, scale: f64) -> Vec<(Solid, Option<[f64; 3]>)> {
        let offsets = assembly::explode_offsets(&self.joints, &self.explode, &Default::default());
        self.named_solids()
            .into_iter()
            .map(|(name, solid)| {
                let colour = self.colours.get(&name).copied();
                match offsets.get(&name) {
                    Some(by) if scale != 0.0 => (
                        builder::transformed(solid, Matrix4::from_translation(by * scale)),
                        colour,
                    ),
                    _ => (solid.clone(), colour),
                }
            })
            .collect()
    }

    pub(crate) fn apply_shared(&mut self, line: &Line) -> Result<String> {
        match line.op.as_str() {
            "if" => self.op_if(line),
            _ => self.op_include(line),
        }
    }

    pub fn apply(&mut self, line: &Line) -> Result<String> {
        if self.assembly {
            return self.apply_assembly(line);
        }
        if PROFILE_OPS.contains(&line.op.as_str())
            && line.positional.iter().any(|w| w == "construct")
        {
            let mut plain = line.clone();
            plain.positional.retain(|w| w != "construct");
            let count = self.sketch.len();
            self.apply_op(&plain)?;
            let frame = self.sketch_frame();
            let added: Vec<Profile> = self.sketch.drain(count..).collect();
            self.construction
                .extend(added.into_iter().map(|p| (frame, p)));
            return Ok(format!(
                "construction; sketch has {} profile(s)",
                self.sketch.len()
            ));
        }
        self.apply_op(line)
    }

    fn apply_op(&mut self, line: &Line) -> Result<String> {
        match line.op.as_str() {
            "let" => self.op_let(line),
            "plane" => self.op_plane(line),
            "rect" => self.op_rect(line),
            "circle" => self.op_circle(line),
            "poly" => self.op_poly(line),
            "ngon" => self.op_ngon(line),
            "gear" => self.op_gear(line),
            "offset" => self.op_offset(line),
            "slot" => self.op_slot(line),
            "ellipse" => self.op_ellipse(line),
            "pen" => self.op_pen(line),
            "line" => self.op_line(line),
            "arc" => self.op_arc(line),
            "close" => self.op_close(line),
            "spline" => self.op_spline(line),
            "text" => self.op_text(line),
            "section" => self.op_section(line),
            "loft" => self.op_loft(line),
            "extrude" => self.op_extrude(line),
            "cut" => self.op_cut(line),
            "revolve" => self.op_revolve(line),
            "path" => self.op_path(line),
            "helix" => self.op_helix(line),
            "sweep" => self.op_sweep(line),
            "hole" => self.op_hole(line),
            "fillet" => self.op_blend(line, FilletProfile::Round),
            "chamfer" => self.op_blend(line, FilletProfile::Chamfer),
            "shell" => self.op_shell(line),
            "draft" => self.op_draft(line),
            "push" => self.op_push(line),
            "mirror" => self.op_mirror(line),
            "repeat" => self.op_repeat(line),
            "move" => self.op_move(line),
            "rotate" => self.op_rotate(line),
            "scale" => self.op_scale(line),
            "split" => self.op_split(line),
            "body" => self.op_body(line),
            "combine" => self.op_combine(line),
            "place" => self.op_place(line),
            "measure" => self.op_measure(line),
            "if" => self.op_if(line),
            "include" => self.op_include(line),
            "point" => self.op_point(line),
            "dist" | "horizontal" | "vertical" | "angle" | "coincident" | "parallel"
            | "perpendicular" | "equal" | "midpoint" | "online" => self.op_constrain(line),
            "reflect" => self.op_reflect(line),
            "array" => self.op_array(line),
            "axis" => self.op_axis(line),
            "color" => self.op_color(line),
            "material" => self.op_material(line),
            "dxf" | "svg" => self.op_drawing_file(line),
            "thicken" => self.op_thicken(line),
            "wrap" => self.op_wrap(line),
            "rib" => self.op_rib(line),
            "joint" | "pose" | "couple" | "explode" | "interference" | "part" | "concentric"
            | "flush" | "aligned" | "distance" | "tangent" => {
                bail!(
                    "`{}` belongs in an assembly (.gasm) file, which brings parts in with `part name file.gcad`",
                    line.op
                )
            }
            "import" => self.op_import(line),
            other => bail!(
                "unknown operation `{other}`; operations are {}",
                OPERATIONS.join(", ")
            ),
        }
    }

    pub(crate) fn active(&self, op: &str) -> Result<&Solid> {
        self.solid
            .as_ref()
            .ok_or_else(|| anyhow!("`{op}` needs a solid"))
    }

    pub(crate) fn merge_all(
        &mut self,
        label: &str,
        tools: Vec<Solid>,
        combine: Combine,
    ) -> Result<()> {
        let apart = |a: &BoundingBox<Point3>, b: &BoundingBox<Point3>| {
            (0..3).any(|k| a.max()[k] < b.min()[k] || b.max()[k] < a.min()[k])
        };
        let boxes: Vec<BoundingBox<Point3>> = tools.iter().map(geometry::bounds).collect();
        let disjoint =
            (0..boxes.len()).all(|i| (i + 1..boxes.len()).all(|j| apart(&boxes[i], &boxes[j])));
        if tools.len() > 1 && disjoint && self.solid.is_some() {
            let faces: Vec<Face> = tools.iter().flat_map(select::faces).collect();
            let together = Solid::new_unchecked(vec![faces.into()]);
            let before = self.solid.clone();
            if self.merge(label, together, combine).is_ok() {
                return Ok(());
            }
            self.solid = before;
        }
        tools
            .into_iter()
            .try_for_each(|tool| self.merge(label, tool, combine))
    }

    pub(crate) fn merge(&mut self, label: &str, tool: Solid, combine: Combine) -> Result<()> {
        let result = match (self.solid.as_ref(), combine) {
            (None, Combine::Add) => tool.clone(),
            (None, _) => bail!("there is no solid to cut or intersect"),
            (Some(existing), Combine::Add) => {
                match monstertruck::solid::or_normalized(existing, &tool) {
                    Ok(solid) => solid,
                    Err(error) => weld::weld_union(existing, &tool)
                        .ok_or_else(|| anyhow!("union failed: {error}"))?,
                }
            }
            (Some(existing), Combine::Remove) => {
                let tool = solids::clear_flush(&tool, existing);
                monstertruck::solid::difference_normalized(existing, &tool)
                    .map_err(|e| anyhow!("cut failed: {e}"))?
            }
            (Some(existing), Combine::Common) => {
                let tool = solids::clear_flush(&tool, existing);
                monstertruck::solid::and_normalized(existing, &tool)
                    .map_err(|e| anyhow!("intersection failed: {e}"))?
            }
        };
        self.solid = Some(result);
        self.tools
            .entry(label.to_string())
            .or_default()
            .push((combine, tool));
        Ok(())
    }

    fn op_if(&mut self, line: &Line) -> Result<String> {
        let text = line.text.trim_start();
        let text = match &line.label {
            Some(label) => text.trim_start_matches(&format!("{label}:")).trim_start(),
            None => text,
        };
        let rest = text
            .strip_prefix("if")
            .ok_or_else(|| anyhow!("`if` must start the line"))?
            .trim_start();
        let (condition, inner) = rest
            .split_once(char::is_whitespace)
            .ok_or_else(|| anyhow!("write `if <condition> <operation ...>`"))?;
        let holds = crate::parse::eval(condition, &self.scope)?;
        if holds == 0.0 {
            return Ok(format!("skipped, `{condition}` is false"));
        }
        let mut inner = crate::parse::parse_line(line.number, inner.trim())?;
        if inner.label.is_none() {
            inner.label = line.label.clone();
        }
        self.apply(&inner)
    }

    fn op_include(&mut self, line: &Line) -> Result<String> {
        let file = line
            .positional
            .first()
            .ok_or_else(|| anyhow!("`include` needs a file"))?;
        if line.positional.len() > 1 {
            bail!("`include` takes one file and name=value pairs");
        }
        if self.depth >= 16 {
            bail!("includes are nested more than 16 deep");
        }
        let path = match &self.dir {
            Some(dir) if std::path::Path::new(file).is_relative() => dir.join(file),
            _ => PathBuf::from(file),
        };
        let source = std::fs::read_to_string(&path)
            .map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
        let lines = crate::parse::parse_program(&source)?;
        let (saved_fixed, saved_scope, saved_dir) =
            (self.fixed.clone(), self.scope.clone(), self.dir.clone());
        for (name, text) in &line.named {
            let value = crate::parse::eval(text, &saved_scope)?;
            self.fixed.insert(name.clone(), value);
            self.scope.insert(name.clone(), value);
        }
        self.dir = path.parent().map(std::path::Path::to_path_buf);
        self.depth += 1;
        let result = lines.iter().try_for_each(|inner| {
            self.apply(inner)
                .map(|_| ())
                .map_err(|e| anyhow!("{}:{}: {e:#}", path.display(), inner.number))
        });
        self.depth -= 1;
        (self.fixed, self.scope, self.dir) = (saved_fixed, saved_scope, saved_dir);
        result?;
        Ok(format!(
            "ran {} line(s) from {}; {}",
            lines.len(),
            path.display(),
            self.describe_solid().unwrap_or_default()
        ))
    }

    pub fn describe_solid(&self) -> Result<String> {
        let solid = self.solid.as_ref().ok_or_else(|| anyhow!("no solid"))?;
        let mesh = geometry::mesh(solid, geometry::mesh_tolerance(solid));
        let bounds: BoundingBox<Point3> = mesh.positions().iter().copied().collect();
        let tidy = |v: f64| if v.abs() < 5.0e-4 { 0.0 } else { v };
        let (min, max) = (bounds.min().map(tidy), bounds.max().map(tidy));
        let others = match self.bodies.len() {
            0 => String::new(),
            n => format!(" ({n} other bodies)"),
        };
        Ok(format!(
            "{} faces, volume {:.3}, bbox [{:.3}, {:.3}, {:.3}]..[{:.3}, {:.3}, {:.3}]{others}",
            select::faces(solid).len(),
            mesh.volume(),
            min.x,
            min.y,
            min.z,
            max.x,
            max.y,
            max.z
        ))
    }
}

#[derive(Clone)]
pub struct Snapshot {
    pub line: Line,
    pub result: std::result::Result<String, String>,
    pub model: Model,
}

fn start(dir: Option<PathBuf>) -> Model {
    Model {
        dir,
        ..Default::default()
    }
}

pub fn is_assembly(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gasm"))
}

fn start_for(path: &std::path::Path, vars: &[(String, f64)], depth: usize) -> Model {
    let mut model = start(path.parent().map(std::path::Path::to_path_buf));
    model.fixed = vars.iter().cloned().collect();
    model.scope = model.fixed.clone();
    model.assembly = is_assembly(path);
    model.depth = depth;
    model
}

fn read_lines(path: &std::path::Path) -> Result<Vec<Line>> {
    let source =
        std::fs::read_to_string(path).map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
    crate::parse::parse_program(&source).map_err(|e| anyhow!("{}: {e:#}", path.display()))
}

pub(crate) fn load_model(
    path: &std::path::Path,
    vars: &[(String, f64)],
    depth: usize,
    cache: &Cache,
) -> Result<Model> {
    let mut start = start_for(path, vars, depth);
    start.cache = cache.clone();
    let run = run_model(start, &read_lines(path)?);
    if let Some((line, Err(error))) = run.steps.last() {
        bail!("{}:{}: {error:#}", path.display(), line.number);
    }
    Ok(run.model)
}

pub fn run_path(
    path: &std::path::Path,
    vars: &[(String, f64)],
    until: Option<usize>,
) -> Result<Run> {
    let lines: Vec<Line> = read_lines(path)?
        .into_iter()
        .filter(|line| until.is_none_or(|last| line.number <= last))
        .collect();
    Ok(run_model(start_for(path, vars, 0), &lines))
}

pub fn snapshots_path(
    path: &std::path::Path,
    vars: &[(String, f64)],
) -> Result<(Vec<Line>, Vec<Snapshot>)> {
    let lines = read_lines(path)?;
    let snapshots = snapshots_from(start_for(path, vars, 0), &lines);
    Ok((lines, snapshots))
}

pub fn run_snapshots_in(dir: Option<PathBuf>, lines: &[Line]) -> Vec<Snapshot> {
    run_snapshots_full(dir, &[], lines)
}

pub fn run_snapshots_full(
    dir: Option<PathBuf>,
    vars: &[(String, f64)],
    lines: &[Line],
) -> Vec<Snapshot> {
    let mut model = start(dir);
    model.fixed = vars.iter().cloned().collect();
    model.scope = model.fixed.clone();
    snapshots_from(model, lines)
}

fn snapshots_from(mut model: Model, lines: &[Line]) -> Vec<Snapshot> {
    if model.assembly {
        model.prefetch(lines);
    }
    let mut snapshots = Vec::new();
    for line in lines {
        let before = model.clone();
        let result = model.apply(line).map_err(|error| format!("{error:#}"));
        let failed = result.is_err();
        if failed {
            model = before;
        }
        snapshots.push(Snapshot {
            line: line.clone(),
            result,
            model: model.clone(),
        });
        if failed {
            break;
        }
    }
    snapshots
}

pub fn run_snapshots(lines: &[Line]) -> Vec<Snapshot> {
    run_snapshots_in(None, lines)
}

pub struct Run {
    pub model: Model,
    pub steps: Vec<(Line, Result<String>)>,
    pub took: Vec<std::time::Duration>,
}

pub fn run_in(dir: Option<PathBuf>, lines: &[Line]) -> Run {
    run_model(start(dir), lines)
}

fn run_model(mut model: Model, lines: &[Line]) -> Run {
    if model.assembly {
        model.prefetch(lines);
    }
    let mut steps = Vec::new();
    let mut took = Vec::new();
    for line in lines {
        let before = model.clone();
        let clock = std::time::Instant::now();
        let result = model.apply(line);
        took.push(clock.elapsed());
        let failed = result.is_err();
        if failed {
            model = before;
        }
        steps.push((line.clone(), result));
        if failed {
            break;
        }
    }
    Run { model, steps, took }
}

pub fn run(lines: &[Line]) -> Run {
    run_in(None, lines)
}

pub fn run_with(vars: &[(String, f64)], lines: &[Line]) -> Run {
    run_full(None, vars, lines)
}

pub fn run_assembly(dir: Option<PathBuf>, lines: &[Line]) -> Run {
    let mut model = start(dir);
    model.assembly = true;
    run_model(model, lines)
}

pub fn run_full(dir: Option<PathBuf>, vars: &[(String, f64)], lines: &[Line]) -> Run {
    let mut model = start(dir);
    model.fixed = vars.iter().cloned().collect();
    model.scope = model.fixed.clone();
    run_model(model, lines)
}
