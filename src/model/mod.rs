mod args;
mod blends;
mod bodies;
mod features;
mod holes;
mod path;
mod round;
mod sketch;
mod solids;
mod weld;

use crate::geometry::{self, Frame, Profile, Segment};
use crate::parse::{Line, Scope};
use crate::select::{self, Groups};
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;
use std::collections::HashMap;
use std::path::PathBuf;

pub use args::label_of;
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
}

pub const OPERATIONS: &[&str] = &[
    "let", "plane", "rect", "circle", "poly", "ngon", "slot", "ellipse", "pen", "line", "arc",
    "close", "spline", "text", "section", "loft", "extrude", "cut", "revolve", "path", "helix",
    "sweep", "hole", "fillet", "chamfer", "shell", "draft", "push", "mirror", "repeat", "move",
    "rotate", "scale", "split", "body", "import",
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

    pub fn apply(&mut self, line: &Line) -> Result<String> {
        match line.op.as_str() {
            "let" => self.op_let(line),
            "plane" => self.op_plane(line),
            "rect" => self.op_rect(line),
            "circle" => self.op_circle(line),
            "poly" => self.op_poly(line),
            "ngon" => self.op_ngon(line),
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

    pub(crate) fn merge(&mut self, label: &str, tool: Solid, combine: Combine) -> Result<()> {
        let result = match (self.solid.take(), combine) {
            (None, Combine::Add) => tool.clone(),
            (None, _) => bail!("there is no solid to cut or intersect"),
            (Some(existing), Combine::Add) => {
                match monstertruck::solid::or_normalized(&existing, &tool) {
                    Ok(solid) => solid,
                    Err(error) => weld::weld_union(&existing, &tool)
                        .ok_or_else(|| anyhow!("union failed: {error}"))?,
                }
            }
            (Some(existing), Combine::Remove) => {
                monstertruck::solid::difference_normalized(&existing, &tool)
                    .map_err(|e| anyhow!("cut failed: {e}"))?
            }
            (Some(existing), Combine::Common) => {
                monstertruck::solid::and_normalized(&existing, &tool)
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

    pub fn describe_solid(&self) -> Result<String> {
        let solid = self.solid.as_ref().ok_or_else(|| anyhow!("no solid"))?;
        let bounds = geometry::bounds(solid);
        let tidy = |v: f64| if v.abs() < 5.0e-4 { 0.0 } else { v };
        let (min, max) = (bounds.min().map(tidy), bounds.max().map(tidy));
        let others = match self.bodies.len() {
            0 => String::new(),
            n => format!(" ({n} other bodies)"),
        };
        Ok(format!(
            "{} faces, volume {:.3}, bbox [{:.3}, {:.3}, {:.3}]..[{:.3}, {:.3}, {:.3}]{others}",
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

pub fn run_snapshots_in(dir: Option<PathBuf>, lines: &[Line]) -> Vec<Snapshot> {
    let mut model = start(dir);
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
}

pub fn run_in(dir: Option<PathBuf>, lines: &[Line]) -> Run {
    run_model(start(dir), lines)
}

fn run_model(mut model: Model, lines: &[Line]) -> Run {
    let mut steps = Vec::new();
    for line in lines {
        let before = model.clone();
        let result = model.apply(line);
        let failed = result.is_err();
        if failed {
            model = before;
        }
        steps.push((line.clone(), result));
        if failed {
            break;
        }
    }
    Run { model, steps }
}

pub fn run(lines: &[Line]) -> Run {
    run_in(None, lines)
}

pub fn run_with(vars: &[(String, f64)], lines: &[Line]) -> Run {
    run_full(None, vars, lines)
}

pub fn run_full(dir: Option<PathBuf>, vars: &[(String, f64)], lines: &[Line]) -> Run {
    let mut model = start(dir);
    model.fixed = vars.iter().cloned().collect();
    model.scope = model.fixed.clone();
    run_model(model, lines)
}
