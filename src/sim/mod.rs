pub mod fem;
mod heat;
pub mod mesh;
mod result;
pub mod statics;
pub mod tet;
mod torque;
mod vibration;

pub use result::{Curve, Field, Outcome, Reading, Series, inputs, stale, stamp};

use crate::model::Model;
use anyhow::{Result, bail};
use monstertruck::modeling::{Point3, Vector3};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Torque,
    Static,
    Thermal,
    Modal,
    Buckling,
}

pub const KINDS: &[&str] = &["torque", "static", "thermal", "modal", "buckling"];

impl Kind {
    pub fn named(name: &str) -> Result<Kind> {
        match name {
            "torque" => Ok(Kind::Torque),
            "static" => Ok(Kind::Static),
            "thermal" => Ok(Kind::Thermal),
            "modal" => Ok(Kind::Modal),
            "buckling" => Ok(Kind::Buckling),
            other => bail!(
                "unknown simulation `{other}`; kinds are {}",
                KINDS.join(", ")
            ),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Torque => "torque",
            Kind::Static => "static",
            Kind::Thermal => "thermal",
            Kind::Modal => "modal",
            Kind::Buckling => "buckling",
        }
    }

    pub fn meshed(self) -> bool {
        self != Kind::Torque
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
    pub origin: Option<PathBuf>,
    pub kind: Option<Kind>,
    pub target: Option<Box<Model>>,
    pub file: Option<PathBuf>,
    pub gravity: Option<Vector3>,
    pub payloads: Vec<Payload>,
    pub sweeps: Vec<Sweep>,
    pub expects: Vec<Expect>,
    pub mesh: Option<f64>,
    pub supports: Vec<fem::elastic::Support>,
    pub loads: Vec<fem::elastic::Load>,
    pub probes: Vec<(String, Point3)>,
    pub thermal: Vec<fem::thermal::Boundary>,
    pub source: f64,
    pub modes: Option<usize>,
    pub warming: Option<Warm>,
    pub contacts: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub enum Warm {
    By(f64),
    From { file: PathBuf, reference: f64 },
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
        Some(_) => mesh::plan(spec),
        None => bail!("the file has no `sim` line"),
    }
}

pub fn simulate(spec: &Spec) -> Result<Outcome> {
    let mut outcome = match spec.kind {
        Some(Kind::Torque) => torque::run(spec)?,
        Some(Kind::Static) => statics::run(spec)?,
        Some(Kind::Thermal) => heat::run(spec)?,
        Some(Kind::Modal) => vibration::run(spec)?,
        Some(Kind::Buckling) => statics::buckle(spec)?,
        None => bail!("the file has no `sim` line"),
    };
    outcome.check(&spec.expects);
    Ok(outcome)
}

pub fn render(spec: &Spec, result: &Path, output: &str) -> Result<()> {
    let outcome = Outcome::read(result)
        .ok()
        .filter(|o| Some(o.kind.as_str()) == spec.kind.map(Kind::name));
    match (spec.kind, outcome) {
        (Some(Kind::Torque), Some(outcome)) => crate::plot::chart(&outcome, output),
        (Some(Kind::Static | Kind::Thermal | Kind::Modal | Kind::Buckling), Some(outcome)) => {
            statics::render(spec, &outcome, output)
        }
        (Some(kind), _) if kind.meshed() => mesh::render(&mesh::bodies(spec)?, output),
        _ => bail!("there is no result to draw yet; run `gcad sim` first"),
    }
}

pub fn current_field(spec: &Spec) -> Option<statics::FieldView> {
    let origin = spec.origin.as_ref()?;
    if !spec.kind.is_some_and(Kind::meshed) || stale(origin, &[]).is_some() {
        return None;
    }
    let outcome = Outcome::read(&result_path(origin)).ok()?;
    statics::field_view(spec, &outcome).ok()
}
