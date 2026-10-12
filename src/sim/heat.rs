use super::fem::elastic::{locate, sample};
use super::fem::thermal::solve;
use super::result::short;
use super::statics::{at, prepare};
use super::{Field, Outcome, Reading, Spec};
use anyhow::{Result, bail};

pub fn run(spec: &Spec) -> Result<Outcome> {
    run_spec(spec)
}

fn run_spec(spec: &Spec) -> Result<Outcome> {
    let prepared = prepare(spec)?;
    let anchors: Vec<usize> = spec
        .thermal
        .iter()
        .flat_map(|b| match b {
            super::fem::thermal::Boundary::Temperature { faces, .. }
            | super::fem::thermal::Boundary::Convect { faces, .. } => faces.clone(),
            super::fem::thermal::Boundary::Flux { .. } => Vec::new(),
        })
        .collect();
    prepared.held(&anchors, "temperature or convect line")?;
    let mesh = &prepared.mesh;
    let conductivity = prepared.conductivity()?;
    let answer = solve(mesh, &conductivity, &spec.thermal, spec.source)?;
    let t = &answer.temperature;
    let (hot, hottest) =
        t.iter().enumerate().fold(
            (f64::NEG_INFINITY, 0),
            |b, (i, v)| if *v > b.0 { (*v, i) } else { b },
        );
    let (cold, coldest) =
        t.iter().enumerate().fold(
            (f64::INFINITY, 0),
            |b, (i, v)| if *v < b.0 { (*v, i) } else { b },
        );
    let mut readings = vec![
        Reading {
            name: "temperature".into(),
            value: hot,
            unit: "°C".into(),
            at: format!("hottest {}", at(mesh.nodes[hottest])),
        },
        Reading {
            name: "coolest".into(),
            value: cold,
            unit: "°C".into(),
            at: at(mesh.nodes[coldest]),
        },
    ];
    for (label, watts) in &answer.leaving {
        readings.push(Reading {
            name: label.clone(),
            value: *watts,
            unit: "W".into(),
            at: "heat leaving the part through these faces".into(),
        });
    }
    for (name, point) in &spec.probes {
        let Some(found) = locate(mesh, *point) else {
            bail!("probe `{name}` {} is not inside the part", at(*point));
        };
        readings.push(Reading {
            name: format!("{name}.temperature"),
            value: sample(mesh, t, found),
            unit: "°C".into(),
            at: at(*point),
        });
    }
    let into: f64 = spec
        .thermal
        .iter()
        .filter_map(|b| match b {
            super::fem::thermal::Boundary::Flux { watts, .. } => Some(*watts),
            _ => None,
        })
        .sum::<f64>()
        + spec.source;
    let out: f64 = answer.leaving.iter().map(|(_, w)| w).sum();
    let mut notes = vec![
        format!(
            "{} nodes, {} quadratic tetrahedra; {} W goes in from sources and {} W leaves through the boundaries",
            mesh.nodes.len(),
            mesh.tets.len(),
            short(into),
            short(out)
        ),
        "steady state, conduction only inside the part".into(),
    ];
    if let Some(iterations) = answer.iterations {
        notes.push(format!("solved iteratively in {iterations} steps"));
    }
    for mark in super::statics::stimuli(spec, mesh, None) {
        if mark.kind == super::statics::Stimulus::Heat && mark.text.ends_with(" W in") {
            notes.push(format!("applied {}", mark.text));
        }
    }
    if let Some(bonds) = prepared.bonds() {
        notes.push(bonds);
    }
    Ok(Outcome {
        kind: "thermal".into(),
        readings,
        notes,
        series: Vec::new(),
        checks: Vec::new(),
        aliases: Vec::new(),
        slices: Vec::new(),
        streams: Vec::new(),
        field: Some(Field {
            title: "temperature".into(),
            unit: "°C".into(),
            values: answer.temperature.clone(),
            displacement: vec![[0.0; 3]; answer.temperature.len()],
        }),
    })
}

pub fn temperatures(file: &std::path::Path) -> Result<Vec<f64>> {
    let run = crate::model::run_path(file, &[], None)?;
    if let Some((line, Err(error))) = run.steps.last() {
        bail!("{}:{}: {error:#}", file.display(), line.number);
    }
    let Some(spec) = run.model.sim.as_deref() else {
        bail!("{} is not a simulation", file.display());
    };
    if spec.kind != Some(super::Kind::Thermal) {
        bail!("{} is not a `sim thermal`", file.display());
    }
    let outcome = run_spec(spec)?;
    Ok(outcome.field.map(|f| f.values).unwrap_or_default())
}
