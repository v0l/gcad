pub mod fem;
mod heat;
pub mod magnetic;
pub mod mesh;
mod result;
pub mod statics;
pub mod tet;
mod torque;
mod vibration;
pub mod wind;

pub use result::{
    Curve, Field, Movie, Outcome, Reading, Reel, Series, Slice, Stream, inputs, short, stale, stamp,
};

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
    Flow,
    Magnetic,
}

pub const KINDS: &[&str] = &[
    "torque", "static", "thermal", "modal", "buckling", "flow", "magnetic",
];

impl Kind {
    pub fn named(name: &str) -> Result<Kind> {
        match name {
            "torque" => Ok(Kind::Torque),
            "static" => Ok(Kind::Static),
            "thermal" => Ok(Kind::Thermal),
            "modal" => Ok(Kind::Modal),
            "buckling" => Ok(Kind::Buckling),
            "flow" => Ok(Kind::Flow),
            "magnetic" => Ok(Kind::Magnetic),
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
            Kind::Flow => "flow",
            Kind::Magnetic => "magnetic",
        }
    }

    pub fn meshed(self) -> bool {
        !matches!(self, Kind::Torque | Kind::Flow | Kind::Magnetic)
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
    pub joint_torques: Vec<(String, f64)>,
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
    pub contacts: Vec<(String, String, f64)>,
    pub converge: Option<f64>,
    pub leave: Vec<String>,
    pub wind: Option<wind::Wind>,
    pub air: Option<(f64, f64)>,
    pub average: Option<f64>,
    pub half: bool,
    pub ambient: Option<f64>,
    pub duct: Option<[f64; 2]>,
    pub wind_load: Option<PathBuf>,
    pub spins: Vec<Spin>,
    pub turn: Option<magnetic::Turn>,
    pub stack: Option<f64>,
    pub slice: Option<f64>,
    pub roles: Vec<magnetic::Assigned>,
}

#[derive(Clone, Debug)]
pub struct Spin {
    pub joint: String,
    pub rpm: f64,
}

#[derive(Clone, Debug)]
pub enum Warm {
    By(Vec<(Option<String>, f64)>),
    From { file: PathBuf, reference: f64 },
}

impl Spec {
    pub fn gravity(&self) -> Vector3 {
        self.gravity.unwrap_or(Vector3::new(0.0, 0.0, -9.81))
    }

    pub fn named(&self) -> Result<Vec<(String, &monstertruck::modeling::Solid)>> {
        Ok(self
            .target()?
            .named_solids()
            .into_iter()
            .filter(|(name, _)| !self.leave.iter().any(|p| matches(p, name)))
            .collect())
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

pub fn movie_path(spec: &Path) -> PathBuf {
    spec.with_extension("frames.bin")
}

pub fn plan(spec: &Spec) -> Result<String> {
    match spec.kind {
        Some(Kind::Torque) => torque::plan(spec),
        Some(Kind::Flow) => wind::plan(spec),
        Some(Kind::Magnetic) => magnetic::plan(spec),
        Some(_) => mesh::plan(spec),
        None => bail!("the file has no `sim` line"),
    }
}

pub fn simulate(spec: &Spec) -> Result<Outcome> {
    let mut outcome = solve(spec)?;
    if let Some(within) = spec.converge {
        let size = match spec.kind {
            Some(Kind::Flow) => wind::cell(spec)?,
            Some(Kind::Magnetic) => magnetic::size(spec)?,
            _ => mesh::size(spec)?,
        };
        let mut coarse = spec.clone();
        coarse.mesh = Some(size * 1.5);
        coarse.converge = None;
        let rough = solve(&coarse)?;
        outcome
            .notes
            .push(settled(&rough, &outcome, size * 1.5, within));
    }
    if spec.kind.is_some_and(Kind::meshed) {
        outcome.notes.extend(directional_notes(spec)?);
    }
    outcome.check(&spec.expects);
    Ok(outcome)
}

fn directional_notes(spec: &Spec) -> Result<Vec<String>> {
    let model = spec.target()?;
    let mut notes = Vec::new();
    for (name, _) in spec.named()? {
        let Some(material) = model.materials.get(&name) else {
            continue;
        };
        let Some(axes) = material.axes else { continue };
        let m = &material.mechanics;
        let used: Vec<String> = [
            m.young.map(|v| format!("E {} GPa", crate::plot::label(v))),
            m.poisson.map(|v| format!("nu {}", crate::plot::label(v))),
            m.strength
                .map(|v| format!("yield {} MPa", crate::plot::label(v))),
        ]
        .into_iter()
        .flatten()
        .collect();
        notes.push(format!(
            "`{name}` is {} with properties along three axes ({}); the solver is isotropic and takes the weakest: {}",
            material.name,
            axes.summary(),
            used.join(", ")
        ));
    }
    Ok(notes)
}

fn settled(rough: &Outcome, fine: &Outcome, coarse: f64, within: f64) -> String {
    let mut moved = Vec::new();
    let mut steady = 0;
    for reading in &fine.readings {
        let Some(before) = rough.reading(&reading.name) else {
            continue;
        };
        let largest = fine
            .readings
            .iter()
            .filter(|r| r.unit == reading.unit)
            .map(|r| r.value.abs())
            .fold(0.0, f64::max);
        let scale = reading.value.abs().max(before.value.abs());
        if scale <= largest * 1.0e-6 {
            continue;
        }
        let change = (reading.value - before.value).abs() / scale * 100.0;
        if change > within {
            moved.push(format!("`{}` moved {:.0}%", reading.name, change));
        } else {
            steady += 1;
        }
    }
    let size = crate::plot::label(coarse);
    if moved.is_empty() {
        format!("mesh check: every reading is within {within}% of a solve on mesh {size}")
    } else {
        format!(
            "mesh check against mesh {size}: {}, so {} not settled; refine the mesh, or `probe` away from sharp corners ({steady} other readings within {within}%)",
            moved.join(", "),
            if moved.len() == 1 {
                "it has"
            } else {
                "they have"
            }
        )
    }
}

fn solve(spec: &Spec) -> Result<Outcome> {
    let outcome = match spec.kind {
        Some(Kind::Torque) => torque::run(spec)?,
        Some(Kind::Static) => statics::run(spec)?,
        Some(Kind::Thermal) => heat::run(spec)?,
        Some(Kind::Modal) => vibration::run(spec)?,
        Some(Kind::Buckling) => statics::buckle(spec)?,
        Some(Kind::Flow) => wind::run(spec)?,
        Some(Kind::Magnetic) => magnetic::run(spec)?,
        None => bail!("the file has no `sim` line"),
    };
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
        (Some(Kind::Flow), Some(outcome)) => {
            statics::draw(&wind::field_view(spec, &outcome)?, output)
        }
        (Some(Kind::Magnetic), Some(outcome)) => {
            statics::draw(&magnetic::field_view(spec, Some(&outcome))?, output)
        }
        (Some(kind), _) if kind.meshed() => mesh::render(&mesh::bodies(spec)?, output),
        _ => bail!("there is no result to draw yet; run `gcad sim` first"),
    }
}

pub struct Picture {
    pub kind: Kind,
    pub outcome: Option<Outcome>,
    pub field: Option<statics::FieldView>,
    pub stale: Option<String>,
}

pub fn picture(spec: &Spec, vars: &[(String, f64)]) -> Option<Picture> {
    let origin = spec.origin.as_ref()?;
    let kind = spec.kind?;
    let outcome = Outcome::read(&result_path(origin))
        .ok()
        .filter(|o| o.kind == kind.name());
    let shown = outcome.as_ref().and_then(|outcome| match kind {
        Kind::Flow => wind::field_view(spec, outcome).ok(),
        Kind::Magnetic => magnetic::field_view(spec, Some(outcome)).ok(),
        Kind::Torque => None,
        _ => statics::field_view(spec, outcome).ok(),
    });
    let field = shown.or_else(|| match kind {
        Kind::Flow => wind::setup_view(spec).ok(),
        Kind::Magnetic => magnetic::field_view(spec, None).ok(),
        Kind::Torque => None,
        _ => statics::setup_view(spec).ok(),
    });
    Some(Picture {
        kind,
        stale: stale(origin, vars),
        outcome,
        field,
    })
}

pub fn save(spec: &Path, vars: &[(String, f64)], outcome: &Outcome) -> Result<PathBuf> {
    let out = result_path(spec);
    crate::host::write(
        &out,
        serde_json::to_string_pretty(&outcome.to_json(&stamp(spec, vars)))?.as_bytes(),
    )?;
    let frames = movie_path(spec);
    match &outcome.movie {
        Some(movie) => movie.write(&frames)?,
        None if crate::host::is_file(&frames) => std::fs::remove_file(&frames)?,
        None => {}
    }
    Ok(out)
}

pub fn matches(pattern: &str, name: &str) -> bool {
    let pieces: Vec<&str> = pattern.split('*').collect();
    if pieces.len() == 1 {
        return pattern == name;
    }
    let (first, last) = (pieces[0], pieces[pieces.len() - 1]);
    if !name.starts_with(first) || name.len() < first.len() + last.len() || !name.ends_with(last) {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for piece in &pieces[1..pieces.len() - 1] {
        match rest.find(piece) {
            Some(at) => rest = &rest[at + piece.len()..],
            None => return false,
        }
    }
    true
}
