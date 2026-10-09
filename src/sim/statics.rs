use super::fem::Quadratic;
use super::fem::elastic::{Problem, locate, sample, solve};
use super::result::short;
use super::{Field, Outcome, Reading, Spec};
use crate::model::Mechanics;
use anyhow::{Result, bail};
use monstertruck::modeling::*;

pub struct Part {
    pub name: String,
    pub material: String,
    pub mechanics: Mechanics,
    pub density: f64,
}

pub struct Prepared {
    pub mesh: Quadratic,
    pub parts: Vec<Part>,
}

impl Prepared {
    pub fn elastic(&self) -> Result<Vec<(f64, f64)>> {
        self.parts
            .iter()
            .map(|p| match (p.mechanics.young, p.mechanics.poisson) {
                (Some(e), Some(nu)) => Ok((e * 1000.0, nu)),
                _ => bail!(
                    "`{}` is {}, which has no stiffness; give it `E=` (GPa) and `nu=` on its `material` line",
                    p.name,
                    p.material
                ),
            })
            .collect()
    }

    pub fn conductivity(&self) -> Result<Vec<f64>> {
        self.parts
            .iter()
            .map(|p| match p.mechanics.conductivity {
                Some(k) => Ok(k),
                None => bail!(
                    "`{}` is {}, which has no conductivity; give it `k=` (W/m·K) on its `material` line",
                    p.name,
                    p.material
                ),
            })
            .collect()
    }

    pub fn bonds(&self) -> Option<String> {
        let pairs: Vec<String> = self
            .mesh
            .bonds
            .iter()
            .map(|(a, b, n)| {
                format!(
                    "{} to {} at {n} nodes",
                    self.parts[*a].name, self.parts[*b].name
                )
            })
            .collect();
        (!pairs.is_empty()).then(|| format!("bonded {}", pairs.join(", ")))
    }
}

pub fn prepare(spec: &Spec) -> Result<Prepared> {
    let model = spec.target()?;
    let size = super::mesh::size(spec)?;
    let named = model.named_solids();
    if named.is_empty() {
        bail!("the part has no solid");
    }
    let mut parts = Vec::new();
    for (name, _) in &named {
        let Some(material) = model.materials.get(name) else {
            bail!(
                "give `{name}` a material, like `material aluminium`, so it has a stiffness and conductivity"
            );
        };
        parts.push(Part {
            name: name.clone(),
            material: material.name.clone(),
            mechanics: material.mechanics,
            density: material.density,
        });
    }
    use rayon::prelude::*;
    let meshes: Vec<(String, Quadratic)> = named
        .par_iter()
        .map(|(name, solid)| {
            let tets = super::tet::mesh_solid(solid, size)?;
            Ok((name.clone(), Quadratic::from(&tets)))
        })
        .collect::<Result<_>>()?;
    let mesh = if meshes.len() == 1 {
        meshes
            .into_iter()
            .next()
            .map(|(n, mut q)| {
                q.bodies = vec![n];
                q
            })
            .expect("one mesh")
    } else {
        super::fem::merge(meshes, size * 0.1)
    };
    Ok(Prepared { mesh, parts })
}

pub fn at(p: Point3) -> String {
    format!(
        "at {},{},{}",
        crate::plot::label((p.x * 100.0).round() / 100.0),
        crate::plot::label((p.y * 100.0).round() / 100.0),
        crate::plot::label((p.z * 100.0).round() / 100.0)
    )
}

pub fn run(spec: &Spec) -> Result<Outcome> {
    let prepared = prepare(spec)?;
    let mesh = &prepared.mesh;
    let materials = prepared.elastic()?;
    let body_force = spec.gravity.map(|g| {
        prepared
            .parts
            .iter()
            .map(|p| g * (p.density * 1.0e-6))
            .collect()
    });
    let answer = solve(&Problem {
        mesh,
        materials: &materials,
        supports: &spec.supports,
        loads: &spec.loads,
        body_force,
    })?;
    let mut readings = Vec::new();
    let mut notes = Vec::new();
    let (peak, where_peak) = answer
        .von_mises
        .iter()
        .enumerate()
        .fold(
            (0.0, 0),
            |best, (i, v)| if *v > best.0 { (*v, i) } else { best },
        );
    readings.push(Reading {
        name: "stress".into(),
        value: peak,
        unit: "MPa".into(),
        at: format!("peak von Mises {}", at(mesh.nodes[where_peak])),
    });
    let (moved, where_moved) =
        answer
            .displacement
            .iter()
            .enumerate()
            .fold((0.0, 0), |best, (i, d)| {
                let m = d.magnitude();
                if m > best.0 { (m, i) } else { best }
            });
    readings.push(Reading {
        name: "displacement".into(),
        value: moved,
        unit: "mm".into(),
        at: at(mesh.nodes[where_moved]),
    });
    let mut peaks = vec![0.0f64; prepared.parts.len()];
    for (t, tet) in mesh.tets.iter().enumerate() {
        let b = mesh.body[t] as usize;
        for &v in tet {
            peaks[b] = peaks[b].max(answer.von_mises[v as usize]);
        }
    }
    let safety = prepared
        .parts
        .iter()
        .zip(&peaks)
        .filter_map(|(p, &peak)| Some((p.mechanics.strength? / peak.max(1.0e-300), p)))
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((factor, part)) = safety
        && peak > 0.0
    {
        readings.push(Reading {
            name: "safety".into(),
            value: factor,
            unit: "".into(),
            at: if prepared.parts.len() > 1 {
                format!("in `{}`, yield over its peak stress", part.name)
            } else {
                format!(
                    "yield {} MPa over the peak stress",
                    short(part.mechanics.strength.unwrap_or_default())
                )
            },
        });
    }
    for (label, force) in &answer.reactions {
        readings.push(Reading {
            name: label.clone(),
            value: force.magnitude(),
            unit: "N".into(),
            at: format!(
                "reaction {},{},{}",
                short(force.x),
                short(force.y),
                short(force.z)
            ),
        });
    }
    for (name, point) in &spec.probes {
        let Some(found) = locate(mesh, *point) else {
            bail!("probe `{name}` {} is not inside the part", at(*point));
        };
        readings.push(Reading {
            name: format!("{name}.stress"),
            value: sample(mesh, &answer.von_mises, found),
            unit: "MPa".into(),
            at: format!("von Mises {}", at(*point)),
        });
        let parts: Vec<f64> = (0..3)
            .map(|k| {
                let values: Vec<f64> = answer.displacement.iter().map(|d| d[k]).collect();
                sample(mesh, &values, found)
            })
            .collect();
        readings.push(Reading {
            name: format!("{name}.displacement"),
            value: Vector3::new(parts[0], parts[1], parts[2]).magnitude(),
            unit: "mm".into(),
            at: at(*point),
        });
    }
    notes.push(format!(
        "{} nodes, {} quadratic tetrahedra; the solver took {} iterations to a residual of {:.1e}",
        mesh.nodes.len(),
        mesh.tets.len(),
        answer.iterations,
        answer.residual
    ));
    if answer.residual > 1.0e-6 {
        notes.push("the solver did not converge; the results are unreliable".into());
    }
    notes.push(
        "peak stresses at sharp inside corners and at the edges of fixed faces keep rising as the mesh gets finer; `probe` a point away from them to read the stress there".into(),
    );
    if let Some(bonds) = prepared.bonds() {
        notes.push(bonds);
    }
    if prepared.parts.iter().any(|p| p.mechanics.printed) {
        notes.push("printed-part stiffness and strength, along the layers".into());
    }
    let applied = answer.applied;
    let held: Vector3 = answer
        .reactions
        .iter()
        .fold(Vector3::zero(), |a, (_, r)| a + *r);
    if (applied + held).magnitude() > applied.magnitude().max(1.0e-9) * 1.0e-3 {
        notes.push(format!(
            "the supports hold {} N of the {} N applied; some of the load goes into a sliding support's free direction",
            short(held.magnitude()),
            short(applied.magnitude())
        ));
    }
    Ok(Outcome {
        kind: "static".into(),
        readings,
        notes,
        series: Vec::new(),
        checks: Vec::new(),
        aliases: Vec::new(),
        field: Some(Field {
            title: "von Mises stress".into(),
            unit: "MPa".into(),
            values: answer.von_mises,
            displacement: answer
                .displacement
                .iter()
                .map(|d| [d.x, d.y, d.z])
                .collect(),
        }),
    })
}

pub fn render(spec: &Spec, outcome: &Outcome, path: &str) -> Result<()> {
    let Some(field) = &outcome.field else {
        bail!("the result has no field to draw; rerun `gcad sim`");
    };
    let prepared = prepare(spec)?;
    let mesh = &prepared.mesh;
    if mesh.nodes.len() != field.values.len() || field.displacement.len() != field.values.len() {
        bail!("the result does not match the mesh; rerun `gcad sim`");
    }
    let bounds: BoundingBox<Point3> = mesh.nodes.iter().copied().collect();
    let largest = field
        .displacement
        .iter()
        .map(|d| Vector3::new(d[0], d[1], d[2]).magnitude())
        .fold(0.0, f64::max);
    let scale = if largest > 0.0 {
        bounds.diameter() * 0.05 / largest
    } else {
        0.0
    };
    let positions: Vec<Point3> = mesh
        .nodes
        .iter()
        .zip(&field.displacement)
        .map(|(p, d)| p + Vector3::new(d[0], d[1], d[2]) * scale)
        .collect();
    let high = field
        .values
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let low = if field.unit == "°C" {
        field.values.iter().copied().fold(f64::INFINITY, f64::min)
    } else {
        0.0
    };
    let span = (high - low).max(1.0e-12);
    let colour = |v: u32| crate::plot::heat((field.values[v as usize] - low) / span);
    let mut triangles = Vec::new();
    let mut edges = Vec::new();
    for (f, _) in &mesh.boundary {
        for t in [[0, 3, 5], [3, 1, 4], [5, 4, 2], [3, 4, 5]] {
            let tri = t.map(|k| f[k]);
            triangles.push((tri.map(|v| v as usize), tri.map(colour)));
        }
        for (a, m, b) in [(0, 3, 1), (1, 4, 2), (2, 5, 0)] {
            if f[a] < f[b] {
                edges.push(vec![
                    positions[f[a] as usize],
                    positions[f[m] as usize],
                    positions[f[b] as usize],
                ]);
            }
        }
    }
    let shown: BoundingBox<Point3> = positions.iter().copied().collect();
    let edges: Vec<Vec<Point3>> = if mesh.boundary.len() > 6000 {
        Vec::new()
    } else {
        edges
    };
    let picture = crate::render::draw_triangles(&positions, &triangles, &edges, shown);
    let mut canvas = crate::plot::Canvas::new(picture.width() + 260, picture.height() + 60);
    canvas.paste(&picture, (0, 60));
    crate::plot::colour_bar(
        &mut canvas,
        (
            picture.width() as f64 + 40.0,
            160.0,
            picture.height() as f64 - 60.0,
        ),
        low,
        low + span,
        &field.unit,
    );
    let caption = if field.unit == "relative" {
        format!("{}, shape", field.title)
    } else if scale > 0.0 {
        format!(
            "{} ({}), deformation drawn {}x",
            field.title,
            field.unit,
            crate::plot::label(scale.round().max(1.0))
        )
    } else {
        format!("{} ({})", field.title, field.unit)
    };
    crate::plot::title(&mut canvas, (picture.width() as f64 / 2.0, 38.0), &caption);
    canvas.save(path)
}
