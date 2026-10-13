use super::{Kind, Nested, Spec, Warm};
use crate::model::{Material, Model};
use anyhow::{Context, Result, anyhow, bail};
use monstertruck::modeling::*;
use monstertruck::topology::compress::CompressedSolid;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

type Body = CompressedSolid<Point3, Curve, Surface>;

#[derive(Serialize, Deserialize)]
struct Job {
    gcad: String,
    spec: Spec,
    bodies: Vec<(String, Body)>,
    materials: Vec<(String, Material)>,
    warm: Option<Box<Warmed>>,
    stamp: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize)]
struct Warmed {
    file: PathBuf,
    reference: f64,
    job: Job,
}

pub struct Unpacked {
    pub spec: Spec,
    pub stamp: Vec<(String, String)>,
}

pub fn remote_kind(spec: &Spec) -> Result<Kind> {
    match spec.kind {
        Some(Kind::Torque) => {
            bail!("a torque sweep runs in seconds and needs the whole assembly; run it locally")
        }
        Some(kind) => Ok(kind),
        None => bail!("the file has no `sim` line"),
    }
}

fn job_of(spec: &Spec, stamp: Vec<(String, String)>) -> Result<Job> {
    remote_kind(spec)?;
    let model = spec.target()?;
    let mut spec = spec.clone();
    spec.target = None;
    if let Some(file) = &spec.wind_load
        && spec.wind_samples.is_none()
    {
        spec.wind_samples = Some(super::wind::surface_samples(file)?);
    }
    let warm = match spec.warming.take() {
        Some(Warm::From { file, reference }) => {
            let thermal = super::heat::thermal_spec(&file)?;
            Some(Box::new(Warmed {
                job: job_of(&thermal, Vec::new())
                    .with_context(|| format!("packing {}", file.display()))?,
                file,
                reference,
            }))
        }
        Some(Warm::Solved { .. }) => bail!("the warming is already solved"),
        other => {
            spec.warming = other;
            None
        }
    };
    let mut materials: Vec<(String, Material)> = model
        .materials
        .iter()
        .map(|(n, m)| (n.clone(), m.clone()))
        .collect();
    materials.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Job {
        gcad: env!("CARGO_PKG_VERSION").to_string(),
        spec,
        bodies: model
            .named_solids()
            .into_iter()
            .map(|(name, solid)| (name, solid.compress()))
            .collect(),
        materials,
        warm,
        stamp,
    })
}

pub fn pack(spec: &Spec, stamp: Vec<(String, String)>) -> Result<Vec<u8>> {
    let job = job_of(spec, stamp)?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    ciborium::into_writer(&job, &mut gz)?;
    Ok(gz.finish()?)
}

fn spec_of(job: Job) -> Result<Spec> {
    if job.gcad != env!("CARGO_PKG_VERSION") {
        bail!(
            "the job was packed by gcad {} and this is gcad {}",
            job.gcad,
            env!("CARGO_PKG_VERSION")
        );
    }
    let mut spec = job.spec;
    remote_kind(&spec)?;
    if let Some(warmed) = job.warm {
        spec.warming = Some(Warm::Solved {
            file: warmed.file,
            spec: Nested(Box::new(spec_of(warmed.job)?)),
            reference: warmed.reference,
        });
    }
    let mut model = Model::default();
    for (name, body) in job.bodies {
        let solid =
            Solid::extract(body).map_err(|e| anyhow!("body `{name}` does not rebuild: {e}"))?;
        model.bodies.push((name, solid));
    }
    model.materials = job.materials.into_iter().collect();
    spec.target = Some(Box::new(model));
    Ok(spec)
}

pub fn unpack(bytes: &[u8]) -> Result<Unpacked> {
    let job: Job = ciborium::from_reader(flate2::read::GzDecoder::new(bytes)).map_err(|e| {
        anyhow!(
            "gcad {} cannot read the job ({e}); it needs the version that packed it",
            env!("CARGO_PKG_VERSION")
        )
    })?;
    let stamp = job.stamp.clone();
    Ok(Unpacked {
        spec: spec_of(job)?,
        stamp,
    })
}
