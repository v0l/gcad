use super::fem::Quadratic;
use super::fem::elastic::{Problem, locate, sample, solve};
use super::result::short;
use super::{Field, Outcome, Reading, Spec, Warm};
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

    pub fn expansion(&self, warming: Option<&Warm>) -> Result<Option<Vec<f64>>> {
        let Some(warming) = warming else {
            return Ok(None);
        };
        let warmed = |p: &Part| match warming {
            Warm::By(list) => list
                .iter()
                .any(|(part, by)| *by != 0.0 && part.as_ref().is_none_or(|n| *n == p.name)),
            Warm::From { .. } => true,
        };
        let alphas: Vec<f64> = self
            .parts
            .iter()
            .map(|p| match p.mechanics.expansion {
                Some(alpha) => Ok(alpha * 1.0e-6),
                None if !warmed(p) => Ok(0.0),
                None => bail!(
                    "`{}` is {}, which has no expansion; give it `alpha=` (µm/m·K) on its `material` line",
                    p.name,
                    p.material
                ),
            })
            .collect::<Result<_>>()?;
        let mut body = vec![0usize; self.mesh.nodes.len()];
        for (t, tet) in self.mesh.tets.iter().enumerate() {
            for &v in tet {
                body[v as usize] = self.mesh.body[t] as usize;
            }
        }
        let rise: Vec<f64> = match warming {
            Warm::By(list) => {
                let per: Vec<f64> = self
                    .parts
                    .iter()
                    .map(|p| {
                        list.iter()
                            .filter(|(part, _)| part.as_ref().is_none_or(|n| *n == p.name))
                            .map(|(_, by)| by)
                            .sum()
                    })
                    .collect();
                body.iter().map(|b| per[*b]).collect()
            }
            Warm::From { file, reference } => {
                let heat = super::heat::temperatures(file)?;
                if heat.len() != self.mesh.nodes.len() {
                    bail!(
                        "{} meshes the part differently; give it the same `part` and `mesh` lines",
                        file.display()
                    );
                }
                heat.iter().map(|t| t - reference).collect()
            }
        };
        Ok(Some(
            rise.iter()
                .zip(&body)
                .map(|(r, b)| r * alphas[*b])
                .collect(),
        ))
    }

    pub fn held(&self, faces: &[usize], what: &str) -> Result<()> {
        let count = self.parts.len();
        if count < 2 {
            return Ok(());
        }
        let mut parent: Vec<usize> = (0..count).collect();
        fn root(parent: &mut [usize], mut v: usize) -> usize {
            while parent[v] != v {
                parent[v] = parent[parent[v]];
                v = parent[v];
            }
            v
        }
        let mut body = vec![0usize; self.mesh.nodes.len()];
        for (t, tet) in self.mesh.tets.iter().enumerate() {
            for &v in tet {
                body[v as usize] = self.mesh.body[t] as usize;
            }
        }
        for tie in &self.mesh.ties {
            let (a, b) = (
                root(&mut parent, body[tie.slave as usize]),
                root(&mut parent, body[tie.masters[0] as usize]),
            );
            parent[a] = b;
        }
        let mut anchored = vec![false; count];
        for &face in faces {
            let r = root(&mut parent, (face / super::fem::STRIDE).min(count - 1));
            anchored[r] = true;
        }
        let loose: Vec<String> = (0..count)
            .filter(|&b| !anchored[root(&mut parent, b)])
            .map(|b| format!("`{}`", self.parts[b].name))
            .collect();
        if loose.is_empty() {
            return Ok(());
        }
        let one = loose.len() == 1;
        bail!(
            "{} {} free: nothing connects {} to a {what}, so the solver cannot converge; check {} flush against the parts {} should bolt to, or give {} a {what}",
            loose.join(", "),
            if one { "floats" } else { "float" },
            if one { "it" } else { "them" },
            if one { "it sits" } else { "they sit" },
            if one { "it" } else { "they" },
            if one { "it" } else { "them" },
        )
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
        let index = |name: &str| named.iter().position(|(n, _)| n == name);
        let touching = spec
            .contacts
            .iter()
            .map(|(a, b, mu)| match (index(a), index(b)) {
                (Some(i), Some(j)) => Ok((i.min(j), i.max(j), *mu)),
                _ => bail!("`contact {a} {b}` names a part the simulation does not have"),
            })
            .collect::<Result<Vec<_>>>()?;
        super::fem::merge(meshes, size * 0.1, &touching)
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
    prepared.held(
        &spec
            .supports
            .iter()
            .flat_map(|s| s.faces.iter().copied())
            .collect::<Vec<_>>(),
        "support",
    )?;
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
        expansion: prepared.expansion(spec.warming.as_ref())?,
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
    for (label, force, moment) in &answer.reactions {
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
        if moment.magnitude() > 1.0e-6 * answer.scale.max(1.0e-9) {
            readings.push(Reading {
                name: format!("{label}.moment"),
                value: moment.magnitude(),
                unit: "N·m".into(),
                at: format!(
                    "reaction moment {},{},{} about the support's centre",
                    short(moment.x),
                    short(moment.y),
                    short(moment.z)
                ),
            });
        }
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
    if let Some((touching, slipping, rounds)) = answer.contact {
        let friction = spec.contacts.iter().any(|c| c.2 > 0.0);
        notes.push(if friction {
            format!(
                "contact: {touching} node pairs pressing, {slipping} of them slipping, after {rounds} solve(s)"
            )
        } else {
            format!("frictionless contact: {touching} node pairs pressing after {rounds} solve(s)")
        });
    }
    if prepared.parts.iter().any(|p| p.mechanics.printed) {
        notes.push("printed-part stiffness and strength, along the layers".into());
    }
    let applied = answer.applied;
    let held: Vector3 = answer
        .reactions
        .iter()
        .fold(Vector3::zero(), |a, (_, r, _)| a + *r);
    if (applied + held).magnitude() > answer.scale.max(1.0e-9) * 1.0e-3 {
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

#[derive(Clone)]
pub struct FieldView {
    pub positions: Vec<Point3>,
    pub triangles: Vec<[u32; 3]>,
    pub shade: Vec<f64>,
    pub edges: Vec<[u32; 3]>,
    pub low: f64,
    pub high: f64,
    pub scale: f64,
    pub title: String,
    pub unit: String,
    pub values: Vec<f64>,
    pub moved: Vec<Vector3>,
    pub rest: Vec<Point3>,
}

pub struct Probe {
    pub value: f64,
    pub moved: Vector3,
    pub at: Point3,
}

impl FieldView {
    pub fn probe(&self, at: Point3) -> Option<Probe> {
        let (_, t, l) = self
            .triangles
            .iter()
            .map(|t| {
                let corners = t.map(|v| self.positions[v as usize]);
                let (d, l) = super::fem::closest_on_triangle(at, corners);
                (d, t, l)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))?;
        let blend = |f: &dyn Fn(usize) -> Vector3| -> Vector3 {
            t.iter()
                .zip(l)
                .fold(Vector3::zero(), |acc, (&v, w)| acc + f(v as usize) * w)
        };
        Some(Probe {
            value: t
                .iter()
                .zip(l)
                .map(|(&v, w)| self.values[v as usize] * w)
                .sum(),
            moved: blend(&|v| self.moved[v]),
            at: Point3::from_vec(blend(&|v| self.rest[v].to_vec())),
        })
    }
}

pub fn field_view(spec: &Spec, outcome: &Outcome) -> Result<FieldView> {
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
    let shade = field.values.iter().map(|v| (v - low) / span).collect();
    let mut triangles = Vec::new();
    let mut sides: std::collections::BTreeMap<(u32, u32), (u32, Vec<usize>)> =
        std::collections::BTreeMap::new();
    for (f, tag) in &mesh.boundary {
        for t in [[0, 3, 5], [3, 1, 4], [5, 4, 2], [3, 4, 5]] {
            triangles.push(t.map(|k| f[k]));
        }
        for (a, m, b) in [(0, 3, 1), (1, 4, 2), (2, 5, 0)] {
            let entry = sides
                .entry((f[a].min(f[b]), f[a].max(f[b])))
                .or_insert((f[m], Vec::new()));
            entry.1.push(*tag);
        }
    }
    let edges: Vec<[u32; 3]> = sides
        .into_iter()
        .filter(|(_, (_, tags))| tags.windows(2).any(|w| w[0] != w[1]) || tags.len() == 1)
        .map(|((a, b), (m, _))| [a, m, b])
        .collect();
    Ok(FieldView {
        positions,
        triangles,
        shade,
        edges,
        low,
        high: low + span,
        scale,
        title: field.title.clone(),
        unit: field.unit.clone(),
        values: field.values.clone(),
        moved: field
            .displacement
            .iter()
            .map(|d| Vector3::new(d[0], d[1], d[2]))
            .collect(),
        rest: mesh.nodes.clone(),
    })
}

pub fn render(spec: &Spec, outcome: &Outcome, path: &str) -> Result<()> {
    let view = field_view(spec, outcome)?;
    let positions = &view.positions;
    let (low, span, scale) = (view.low, view.high - view.low, view.scale);
    let triangles: Vec<([usize; 3], [[f64; 3]; 3])> = view
        .triangles
        .iter()
        .map(|t| {
            (
                t.map(|v| v as usize),
                t.map(|v| crate::plot::heat(view.shade[v as usize])),
            )
        })
        .collect();
    let edges: Vec<Vec<Point3>> = view
        .edges
        .iter()
        .map(|e| e.iter().map(|&v| positions[v as usize]).collect())
        .collect();
    let shown: BoundingBox<Point3> = positions.iter().copied().collect();
    let picture = crate::render::draw_field(positions, &triangles, &edges, shown);
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
        &view.unit,
    );
    let caption = if view.unit == "relative" {
        format!("{}, shape", view.title)
    } else if scale > 0.0 {
        format!(
            "{} ({}), deformation drawn {}x",
            view.title,
            view.unit,
            crate::plot::label(scale.round().max(1.0))
        )
    } else {
        format!("{} ({})", view.title, view.unit)
    };
    crate::plot::title(&mut canvas, (picture.width() as f64 / 2.0, 38.0), &caption);
    canvas.save(path)
}

pub fn buckle(spec: &Spec) -> Result<Outcome> {
    let prepared = prepare(spec)?;
    prepared.held(
        &spec
            .supports
            .iter()
            .flat_map(|s| s.faces.iter().copied())
            .collect::<Vec<_>>(),
        "support",
    )?;
    let mesh = &prepared.mesh;
    let materials = prepared.elastic()?;
    let body_force = spec.gravity.map(|g| {
        prepared
            .parts
            .iter()
            .map(|p| g * (p.density * 1.0e-6))
            .collect()
    });
    if spec.loads.is_empty() && body_force.is_none() && spec.warming.is_none() {
        bail!("buckling needs a `load`, `pressure`, `torque` or `warm` to scale");
    }
    let wanted = spec.modes.unwrap_or(3);
    let buckles = super::fem::buckling::solve(
        &Problem {
            mesh,
            materials: &materials,
            supports: &spec.supports,
            loads: &spec.loads,
            body_force,
            expansion: prepared.expansion(spec.warming.as_ref())?,
        },
        wanted,
    )?;
    let Some(first) = buckles.first() else {
        bail!("the loads do not compress the part anywhere, so it cannot buckle under them");
    };
    let mut readings = vec![Reading {
        name: "buckling".into(),
        value: first.factor,
        unit: "".into(),
        at: "times the loads given, where the part first buckles".into(),
    }];
    for (k, b) in buckles.iter().enumerate().skip(1) {
        readings.push(Reading {
            name: format!("buckling{}", k + 1),
            value: b.factor,
            unit: "".into(),
            at: "times the loads given".into(),
        });
    }
    let mut notes = vec![format!(
        "{} nodes, {} quadratic tetrahedra; linear buckling, so imperfections and yielding lower the real figure",
        mesh.nodes.len(),
        mesh.tets.len()
    )];
    if let Some(bonds) = prepared.bonds() {
        notes.push(bonds);
    }
    let displacement: Vec<[f64; 3]> = first.shape.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    let values = displacement
        .iter()
        .map(|d| (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt())
        .collect();
    Ok(Outcome {
        kind: "buckling".into(),
        readings,
        notes,
        series: Vec::new(),
        checks: Vec::new(),
        aliases: Vec::new(),
        field: Some(Field {
            title: format!("buckling mode, {} times the loads", short(first.factor)),
            unit: "relative".into(),
            values,
            displacement,
        }),
    })
}
