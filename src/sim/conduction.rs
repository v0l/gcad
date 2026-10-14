use super::fem::thermal::Boundary;
use super::skin::{self, Patch};
use super::statics::FieldView;
use super::{Field, Outcome, Reading, Spec};
use anyhow::{Result, bail};
use monstertruck::modeling::*;
use sims_thermal::{Block, Boundary as Surface, Grid, Model, Prepared, Solution, lines};

const MM: f64 = 1.0e-3;
const TIDY: f64 = 1.0e6;
const GROWTH: f64 = 1.25;
const NODES: usize = 3_000_000;
const DIRECT: usize = 250_000;
const VOXEL_BLOCKS: usize = 6000;
const HELD: f64 = 1.0e-6;

#[derive(Clone)]
struct Environment {
    label: String,
    temperature: f64,
    surfaces: Vec<usize>,
}

enum Piece {
    Box(Block),
    Round {
        triangles: Vec<[[f64; 3]; 3]>,
        low: [f64; 3],
        high: [f64; 3],
        conductivity: f64,
    },
}

struct Setup {
    model: Model,
    environments: Vec<Environment>,
    patches: Vec<Patch>,
    round: Vec<String>,
    fine: f64,
}

fn tidy(mm: f64) -> f64 {
    (mm * TIDY).round() / TIDY * MM
}

fn flat(patch: &Patch) -> Option<(usize, f64)> {
    let first = patch.points.first()?;
    for axis in 0..3 {
        let coordinate = first[axis];
        let level = patch
            .points
            .iter()
            .all(|p| (p[axis] - coordinate).abs() < 1.0e-6);
        if level {
            return Some((axis, coordinate));
        }
    }
    None
}

fn pieces(spec: &Spec, patches: &[Patch]) -> Result<Vec<(String, Piece)>> {
    let model = spec.target()?;
    let mut out = Vec::new();
    for (name, _) in spec.named()? {
        let Some(k) = model
            .materials
            .get(&name)
            .and_then(|m| m.mechanics.conductivity)
        else {
            bail!(
                "give `{name}` a material with a conductivity, like `material concrete` or `material name density=0.03 k=0.035`"
            );
        };
        let own: Vec<&Patch> = patches.iter().filter(|p| p.body == name).collect();
        let bounds: BoundingBox<Point3> =
            own.iter().flat_map(|p| p.points.iter().copied()).collect();
        let (low, high) = (bounds.min(), bounds.max());
        let low = [low.x, low.y, low.z].map(tidy);
        let high = [high.x, high.y, high.z].map(tidy);
        let box_volume: f64 = (0..3).map(|a| high[a] - low[a]).product::<f64>() / (MM * MM * MM);
        let volume = skin::volume(&own).abs();
        if (volume - box_volume).abs() <= 1.0e-4 * box_volume {
            out.push((
                name,
                Piece::Box(Block {
                    low,
                    high,
                    conductivity: k,
                }),
            ));
        } else {
            let triangles = own
                .iter()
                .flat_map(|p| {
                    p.triangles
                        .iter()
                        .map(|t| t.map(|v| [p.points[v].x, p.points[v].y, p.points[v].z]))
                })
                .collect();
            out.push((
                name,
                Piece::Round {
                    triangles,
                    low,
                    high,
                    conductivity: k,
                },
            ));
        }
    }
    Ok(out)
}

fn columns(triangles: &[[[f64; 3]; 3]], x: &[f64], y: &[f64], conductivity: f64) -> Vec<Block> {
    use rayon::prelude::*;
    let (jx, jy) = (1.234_567e-7, 2.345_678e-7);
    let cells: Vec<(usize, usize)> = (0..x.len() - 1)
        .flat_map(|i| (0..y.len() - 1).map(move |j| (i, j)))
        .collect();
    cells
        .into_par_iter()
        .flat_map_iter(|(i, j)| {
            let (px, py) = (
                (x[i] + x[i + 1]) / 2.0 / MM + jx,
                (y[j] + y[j + 1]) / 2.0 / MM + jy,
            );
            let mut hits: Vec<f64> = triangles
                .iter()
                .filter_map(|t| {
                    let [a, b, c] = *t;
                    let d = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                    if d.abs() < 1.0e-14 {
                        return None;
                    }
                    let u = ((px - a[0]) * (c[1] - a[1]) - (py - a[1]) * (c[0] - a[0])) / d;
                    let v = ((b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0])) / d;
                    (u >= 0.0 && v >= 0.0 && u + v <= 1.0)
                        .then_some(a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]))
                })
                .collect();
            hits.sort_by(f64::total_cmp);
            hits.as_chunks::<2>()
                .0
                .iter()
                .map(|pair| Block {
                    low: [x[i], y[j], tidy(pair[0])],
                    high: [x[i + 1], y[j + 1], tidy(pair[1])],
                    conductivity,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn merged(mut blocks: Vec<Block>) -> Vec<Block> {
    blocks.sort_by(|a, b| {
        (a.low[0], a.low[2], a.high[2], a.low[1])
            .partial_cmp(&(b.low[0], b.low[2], b.high[2], b.low[1]))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut out: Vec<Block> = Vec::new();
    for b in blocks {
        match out.last_mut() {
            Some(last)
                if last.low[0] == b.low[0]
                    && last.high[0] == b.high[0]
                    && last.low[2] == b.low[2]
                    && last.high[2] == b.high[2]
                    && (last.high[1] - b.low[1]).abs() < 1.0e-12 =>
            {
                last.high[1] = b.high[1];
            }
            _ => out.push(b),
        }
    }
    out.sort_by(|a, b| {
        (a.low[1], a.high[1], a.low[2], a.high[2], a.low[0])
            .partial_cmp(&(b.low[1], b.high[1], b.low[2], b.high[2], b.low[0]))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut wide: Vec<Block> = Vec::new();
    for b in out {
        match wide.last_mut() {
            Some(last)
                if last.low[1] == b.low[1]
                    && last.high[1] == b.high[1]
                    && last.low[2] == b.low[2]
                    && last.high[2] == b.high[2]
                    && (last.high[0] - b.low[0]).abs() < 1.0e-12 =>
            {
                last.high[0] = b.high[0];
            }
            _ => wide.push(b),
        }
    }
    wide
}

fn environments(spec: &Spec) -> Result<Vec<Environment>> {
    let mut out: Vec<Environment> = Vec::new();
    for (index, boundary) in spec.thermal.iter().enumerate() {
        let (label, temperature) = match boundary {
            Boundary::Temperature { label, value, .. } => (label, *value),
            Boundary::Convect { label, ambient, .. } => (label, *ambient),
            Boundary::Flux { label, .. } => bail!(
                "`{label}` puts heat into faces; a conduction simulation takes `temperature` and `convect` lines"
            ),
        };
        match out.iter_mut().find(|e| &e.label == label) {
            Some(e) if (e.temperature - temperature).abs() > 1.0e-9 => bail!(
                "`{label}` is at {} °C and {temperature} °C; give each temperature its own label",
                e.temperature
            ),
            Some(e) => e.surfaces.push(index),
            None => out.push(Environment {
                label: label.clone(),
                temperature,
                surfaces: vec![index],
            }),
        }
    }
    if out.len() < 2 {
        bail!(
            "a conduction simulation needs `temperature` or `convect` lines on at least two sides, each with its own label"
        );
    }
    Ok(out)
}

fn surfaces(spec: &Spec, patches: &[Patch], environments: &[Environment]) -> Result<Vec<Surface>> {
    let mut out = Vec::new();
    for (environment, e) in environments.iter().enumerate() {
        for &index in &e.surfaces {
            let (faces, resistance) = match &spec.thermal[index] {
                Boundary::Temperature { faces, .. } => (faces, HELD),
                Boundary::Convect { faces, film, .. } => (faces, 1.0 / film),
                Boundary::Flux { .. } => unreachable!("refused above"),
            };
            for tag in faces {
                let Some(patch) = patches.iter().find(|p| p.tag == *tag) else {
                    bail!("`{}` names a face the part does not have", e.label);
                };
                let Some((axis, at)) = flat(patch) else {
                    bail!(
                        "`{}` reaches a face of `{}` that is not flat and square to X, Y or Z; a conduction grid takes boundaries on flat faces of that kind",
                        e.label,
                        patch.body
                    );
                };
                let bounds = patch.bounds();
                let (mut low, mut high) = (
                    [bounds.min().x, bounds.min().y, bounds.min().z].map(tidy),
                    [bounds.max().x, bounds.max().y, bounds.max().z].map(tidy),
                );
                low[axis] = tidy(at);
                high[axis] = tidy(at);
                out.push(Surface {
                    low,
                    high,
                    environment,
                    resistance,
                });
            }
        }
    }
    Ok(out)
}

fn stops(pieces: &[(String, Piece)], surfaces: &[Surface], fine: f64, axis: usize) -> Vec<f64> {
    let mut stops = Vec::new();
    for (_, piece) in pieces {
        match piece {
            Piece::Box(b) => stops.extend([b.low[axis], b.high[axis]]),
            Piece::Round { low, high, .. } => {
                stops.extend([low[axis], high[axis]]);
                let span = high[axis] - low[axis];
                let step = (span / 16.0).max(fine * 2.0);
                let mut at = low[axis] + step;
                while at < high[axis] - step * 0.5 {
                    stops.push(tidy(at / MM));
                    at += step;
                }
            }
        }
    }
    for s in surfaces {
        stops.extend([s.low[axis], s.high[axis]]);
    }
    stops
}

fn grid_for(pieces: &[(String, Piece)], surfaces: &[Surface], fine: f64) -> [Vec<f64>; 3] {
    [0, 1, 2].map(|axis| lines(&stops(pieces, surfaces, fine, axis), fine, GROWTH))
}

fn smallest_gap(pieces: &[(String, Piece)], surfaces: &[Surface]) -> f64 {
    (0..3)
        .flat_map(|axis| {
            let mut s: Vec<f64> = stops(pieces, surfaces, f64::INFINITY, axis);
            s.sort_by(f64::total_cmp);
            s.dedup_by(|a, b| (*a - *b).abs() < 1.0e-9);
            s.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>()
        })
        .fold(f64::INFINITY, f64::min)
}

fn nodes_of(grid: &[Vec<f64>; 3]) -> usize {
    grid.iter().map(Vec::len).product()
}

fn setup(spec: &Spec) -> Result<Setup> {
    let patches = skin::patches(spec, 0.0)?;
    let environments = environments(spec)?;
    let surfaces = surfaces(spec, &patches, &environments)?;
    let parts = pieces(spec, &patches)?;
    let diameter = {
        let bounds: BoundingBox<Point3> = patches
            .iter()
            .flat_map(|p| p.points.iter().copied())
            .collect();
        bounds.diameter() * MM
    };
    let gap = smallest_gap(&parts, &surfaces);
    let mut fine = match spec.mesh {
        Some(size) => size * MM,
        None => (gap / 4.0).max(diameter / 1500.0),
    };
    let mut grid = grid_for(&parts, &surfaces, fine);
    while nodes_of(&grid) > NODES {
        if spec.mesh.is_some() {
            bail!(
                "a {} mm grid has {} nodes; use a coarser `mesh`",
                crate::plot::label(fine / MM),
                nodes_of(&grid)
            );
        }
        fine *= 1.6;
        grid = grid_for(&parts, &surfaces, fine);
    }
    let mut blocks = Vec::new();
    let mut round = Vec::new();
    for (name, piece) in &parts {
        match piece {
            Piece::Box(b) => blocks.push(*b),
            Piece::Round {
                triangles,
                conductivity,
                ..
            } => {
                round.push(name.clone());
                blocks.extend(merged(columns(
                    triangles,
                    &grid[0],
                    &grid[1],
                    *conductivity,
                )));
            }
        }
    }
    if blocks.len() > VOXEL_BLOCKS * (round.len() + 1) + parts.len() {
        bail!(
            "the round parts need {} blocks on this grid; use a coarser `mesh`",
            blocks.len()
        );
    }
    let [x, y, z] = grid;
    let model = Model {
        grid: Grid { x, y, z },
        blocks,
        boundaries: surfaces,
    };
    Ok(Setup {
        model,
        environments,
        patches,
        round,
        fine: fine / MM,
    })
}

pub fn size(spec: &Spec) -> Result<f64> {
    Ok(setup(spec)?.fine)
}

pub fn plan(spec: &Spec) -> Result<String> {
    let s = setup(spec)?;
    let [nx, ny, nz] = s.model.grid.nodes();
    Ok(format!(
        "{nx} x {ny} x {nz} = {} nodes, finest spacing {} mm, {} blocks ({} round part(s) voxelised: {})\n{} environment(s): {}",
        nx * ny * nz,
        crate::plot::label(s.fine),
        s.model.blocks.len(),
        s.round.len(),
        if s.round.is_empty() {
            "none".into()
        } else {
            s.round.join(", ")
        },
        s.environments.len(),
        s.environments
            .iter()
            .map(|e| format!("{} at {} °C", e.label, crate::plot::label(e.temperature)))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn solve(prepared: &Prepared, nodes: usize, temperatures: &[f64]) -> Result<Solution> {
    if nodes <= DIRECT {
        prepared.solve(temperatures).map_err(|e| anyhow::anyhow!(e))
    } else {
        prepared
            .solve_iterative(temperatures, super::fem::gpu(), 1.0e-9)
            .map_err(|e| anyhow::anyhow!(e))
    }
}

fn reading(name: String, value: f64, unit: &str, at: String) -> Reading {
    Reading {
        name,
        value,
        unit: unit.into(),
        at,
    }
}

fn on_surface(setup: &Setup, solution: &Solution, environment: usize) -> Option<(f64, [f64; 3])> {
    let g = &setup.model.grid;
    let [nx, ny, nz] = g.nodes();
    let mut coolest: Option<(f64, [f64; 3])> = None;
    for b in setup
        .model
        .boundaries
        .iter()
        .filter(|b| b.environment == environment)
    {
        let within =
            |axis: usize, v: f64| v >= b.low[axis] - 1.0e-12 && v <= b.high[axis] + 1.0e-12;
        for i in (0..nx).filter(|&i| within(0, g.x[i])) {
            for j in (0..ny).filter(|&j| within(1, g.y[j])) {
                for k in (0..nz).filter(|&k| within(2, g.z[k])) {
                    let t = solution.temperature[(i * ny + j) * nz + k];
                    if t.is_nan() {
                        continue;
                    }
                    if coolest.is_none_or(|c| t < c.0) {
                        coolest = Some((t, [g.x[i], g.y[j], g.z[k]]));
                    }
                }
            }
        }
    }
    coolest
}

pub fn run(spec: &Spec) -> Result<Outcome> {
    let s = setup(spec)?;
    let nodes = nodes_of(&[
        s.model.grid.x.clone(),
        s.model.grid.y.clone(),
        s.model.grid.z.clone(),
    ]);
    let prepared = s.model.prepare().map_err(|e| anyhow::anyhow!(e))?;
    let temperatures: Vec<f64> = s.environments.iter().map(|e| e.temperature).collect();
    let solution = solve(&prepared, nodes, &temperatures)?;
    let mut readings = Vec::new();
    for (e, env) in s.environments.iter().enumerate() {
        readings.push(reading(
            env.label.clone(),
            -solution.flows[e],
            "W",
            format!("heat leaving the construction through `{}`", env.label),
        ));
    }
    let count = s.environments.len();
    if count >= 2 {
        let stage = crate::progress::stage("coupling coefficients", Some(count), "solves");
        let mut columns: Vec<Vec<f64>> = Vec::new();
        for j in 0..count {
            let mut unit = vec![0.0; count];
            unit[j] = 1.0;
            let answer = solve(&prepared, nodes, &unit)?;
            columns.push(
                answer
                    .flows
                    .iter()
                    .enumerate()
                    .map(|(i, f)| if i == j { *f } else { -*f })
                    .collect(),
            );
            stage.advance(1.0);
        }
        for (i, a) in s.environments.iter().enumerate() {
            for (j, b) in s.environments.iter().enumerate().skip(i + 1) {
                let value = (columns[j][i] + columns[i][j]) / 2.0;
                let (a, b) = (&a.label, &b.label);
                readings.push(reading(
                    format!("L.{a}.{b}"),
                    value,
                    "W/K",
                    format!("coupling coefficient between `{a}` and `{b}`"),
                ));
                if count == 2 {
                    readings.push(reading(
                        "coupling".into(),
                        value,
                        "W/K",
                        "heat flow per kelvin between the two sides, ISO 10211".into(),
                    ));
                }
            }
        }
    }
    let hottest = (0..count).max_by(|a, b| {
        s.environments[*a]
            .temperature
            .total_cmp(&s.environments[*b].temperature)
    });
    let coldest = (0..count).min_by(|a, b| {
        s.environments[*a]
            .temperature
            .total_cmp(&s.environments[*b].temperature)
    });
    if let (Some(hot), Some(cold)) = (hottest, coldest) {
        let (warm, chill) = (
            s.environments[hot].temperature,
            s.environments[cold].temperature,
        );
        if let Some((t, at)) = on_surface(&s, &solution, hot) {
            readings.push(reading(
                "coolest".into(),
                t,
                "°C",
                format!(
                    "coolest point of `{}` at {:.0},{:.0},{:.0}",
                    s.environments[hot].label,
                    at[0] / MM,
                    at[1] / MM,
                    at[2] / MM
                ),
            ));
            if warm - chill > 1.0e-9 {
                readings.push(reading(
                    "fRsi".into(),
                    (t - chill) / (warm - chill),
                    "",
                    format!(
                        "temperature factor of `{}` against `{}`; mould risk below about 0.7",
                        s.environments[hot].label, s.environments[cold].label
                    ),
                ));
            }
        }
    }
    let temperature_at =
        |p: Point3| prepared.temperature(&solution, [p.x * MM, p.y * MM, p.z * MM]);
    for (name, point) in &spec.probes {
        match temperature_at(*point) {
            Some(t) => readings.push(reading(
                format!("{name}.temperature"),
                t,
                "°C",
                format!("{:.0},{:.0},{:.0}", point.x, point.y, point.z),
            )),
            None => bail!("probe `{name}` is outside the construction"),
        }
    }
    let values: Vec<f64> = s
        .patches
        .iter()
        .flat_map(|p| p.points.iter())
        .map(|p| temperature_at(*p).unwrap_or(f64::NAN))
        .collect();
    let (low, high) = values
        .iter()
        .filter(|v| !v.is_nan())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
            (l.min(*v), h.max(*v))
        });
    let values: Vec<f64> = values
        .into_iter()
        .map(|v| if v.is_nan() { (low + high) / 2.0 } else { v })
        .collect();
    let [nx, ny, nz] = s.model.grid.nodes();
    let mut notes = vec![format!(
        "{} nodes ({nx} x {ny} x {nz}), finest spacing {} mm, {} blocks; steady conduction on a finite-volume grid, solved {}",
        nx * ny * nz,
        crate::plot::label(s.fine),
        s.model.blocks.len(),
        if nx * ny * nz <= DIRECT {
            "directly"
        } else {
            "iteratively"
        }
    )];
    if !s.round.is_empty() {
        notes.push(format!(
            "{} {} round and staircased onto the grid",
            s.round
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", "),
            if s.round.len() == 1 { "is" } else { "are" }
        ));
    }
    notes.push(
        "a boundary covers the rectangle around its face, and later parts replace earlier ones where they overlap"
            .into(),
    );
    Ok(Outcome {
        kind: "conduction".into(),
        readings,
        notes,
        field: Some(Field {
            title: "surface temperature".into(),
            unit: "°C".into(),
            values,
            displacement: Vec::new(),
        }),
        ..Default::default()
    })
}

pub fn field_view(spec: &Spec, outcome: Option<&Outcome>) -> Result<FieldView> {
    let patches = skin::patches(spec, 0.0)?;
    let total: usize = patches.iter().map(|p| p.points.len()).sum();
    let (values, title, unit) = match outcome.and_then(|o| o.field.as_ref()) {
        Some(field) if field.values.len() == total => (
            field.values.clone(),
            field.title.clone(),
            field.unit.clone(),
        ),
        Some(_) => bail!("the result does not match the part; rerun `gcad sim`"),
        None => (vec![0.0; total], "the part".into(), String::new()),
    };
    let mut positions: Vec<Point3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut edges: Vec<[u32; 3]> = Vec::new();
    for patch in &patches {
        let base = positions.len();
        positions.extend(patch.points.iter().copied());
        triangles.extend(patch.triangles.iter().map(|t| t.map(|v| (base + v) as u32)));
        edges.extend(
            super::wind::creases(&patch.points, &patch.triangles)
                .into_iter()
                .map(|[a, b]| [a, a, b].map(|v| (base + v) as u32)),
        );
    }
    let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let low = values.iter().copied().fold(f64::INFINITY, f64::min);
    let span = (high - low).max(1.0e-12);
    Ok(FieldView {
        shade: values.iter().map(|v| (v - low) / span).collect(),
        triangles,
        edges,
        low,
        high: low + span,
        scale: 0.0,
        title,
        unit,
        moved: vec![Vector3::zero(); positions.len()],
        rest: positions.clone(),
        values,
        positions,
        marks: Vec::new(),
        index: Default::default(),
        slices: Vec::new(),
        streams: Vec::new(),
        travel: Default::default(),
        movie: None,
    })
}
