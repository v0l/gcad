mod result;
mod torque;

pub use result::{Curve, Outcome, Reading, Series, inputs, stale, stamp};

use crate::model::Model;
use anyhow::{Result, bail};
use monstertruck::modeling::{Point3, Vector3};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Torque,
}

pub const KINDS: &[&str] = &["torque", "static", "thermal", "modal"];

impl Kind {
    pub fn named(name: &str) -> Result<Kind> {
        match name {
            "torque" => Ok(Kind::Torque),
            "static" | "thermal" | "modal" => {
                bail!("`sim {name}` is planned but not built yet; `torque` works today")
            }
            other => bail!(
                "unknown simulation `{other}`; kinds are {}",
                KINDS.join(", ")
            ),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Torque => "torque",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Payload {
    pub part: String,
    pub grams: f64,
    pub at: Option<Point3>,
}

#[derive(Clone, Debug)]
pub struct Sweep {
    pub joints: Vec<usize>,
    pub steps: usize,
}

#[derive(Clone, Debug)]
pub struct Expect {
    pub text: String,
    pub reading: String,
    pub below: bool,
    pub limit: f64,
}

#[derive(Clone, Default)]
pub struct Spec {
    pub kind: Option<Kind>,
    pub target: Option<Box<Model>>,
    pub file: Option<PathBuf>,
    pub gravity: Option<Vector3>,
    pub payloads: Vec<Payload>,
    pub sweeps: Vec<Sweep>,
    pub expects: Vec<Expect>,
}

impl Spec {
    pub fn gravity(&self) -> Vector3 {
        self.gravity.unwrap_or(Vector3::new(0.0, 0.0, -9.81))
    }

    pub fn target(&self) -> Result<&Model> {
        match &self.target {
            Some(model) => Ok(model),
            None => bail!("the simulation has no `part` line"),
        }
    }
}

pub fn is_sim(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gsim"))
}

pub fn result_path(spec: &Path) -> PathBuf {
    spec.with_extension("result.json")
}

pub fn plan(spec: &Spec) -> Result<String> {
    match spec.kind {
        Some(Kind::Torque) => torque::plan(spec),
        None => bail!("the file has no `sim` line"),
    }
}

pub fn simulate(spec: &Spec) -> Result<Outcome> {
    let mut outcome = match spec.kind {
        Some(Kind::Torque) => torque::run(spec)?,
        None => bail!("the file has no `sim` line"),
    };
    outcome.check(&spec.expects);
    Ok(outcome)
}
