use super::fem::Quadratic;
use super::fem::elastic::{Problem, locate, sample, solve};
use super::result::short;
use super::{Field, Outcome, Reading, Spec, Warm};
use crate::model::Mechanics;
use anyhow::{Context, Result, bail};
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
    pub original: Vec<usize>,
}

impl Prepared {
    pub fn elastic(&self) -> Result<Vec<(f64, f64)>> {
        let missing: Vec<String> = self
            .parts
            .iter()
            .filter(|p| p.mechanics.young.is_none() || p.mechanics.poisson.is_none())
            .map(|p| format!("`{}` ({})", p.name, p.material))
            .collect();
        match missing.as_slice() {
            [] => {}
            [one] => bail!(
                "{one} has no stiffness; give it `E=` (GPa) and `nu=` on its `material` line, or in the simulation with `material part name E= nu=`"
            ),
            many => bail!(
                "{} parts have no stiffness: {}; give each `E=` (GPa) and `nu=` on its `material` line, or in the simulation with `material part name E= nu=`",
                many.len(),
                many.join(", ")
            ),
        }
        Ok(self
            .parts
            .iter()
            .map(|p| {
                (
                    p.mechanics.young.unwrap_or(0.0) * 1000.0,
                    p.mechanics.poisson.unwrap_or(0.0),
                )
            })
            .collect())
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
            let Some(body) = self
                .original
                .iter()
                .position(|&o| o == face / super::fem::STRIDE)
            else {
                continue;
            };
            let r = root(&mut parent, body);
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

pub fn stiff(spec: &Spec) -> Result<()> {
    let model = spec.target()?;
    let parts: Vec<Part> = spec
        .named()?
        .into_iter()
        .filter_map(|(name, _)| {
            let m = model.materials.get(&name)?;
            Some(Part {
                name,
                material: m.name.clone(),
                mechanics: m.mechanics,
                density: m.density,
            })
        })
        .collect();
    Prepared {
        mesh: Quadratic::empty(),
        parts,
        original: Vec::new(),
    }
    .elastic()
    .map(|_| ())
}

pub fn prepare(spec: &Spec) -> Result<Prepared> {
    let model = spec.target()?;
    let size = super::mesh::size(spec)?;
    let named = spec.named()?;
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
    let meshed: Vec<(String, Quadratic, f64)> = named
        .par_iter()
        .map(|(name, solid)| {
            let (tets, used) =
                super::mesh::mesh_part(solid, size).with_context(|| format!("meshing `{name}`"))?;
            Ok((name.clone(), Quadratic::from(&tets), used))
        })
        .collect::<Result<_>>()?;
    let gaps: Vec<f64> = meshed.iter().map(|m| m.2 * 0.1).collect();
    let meshes: Vec<(String, Quadratic)> = meshed.into_iter().map(|(n, q, _)| (n, q)).collect();
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
        super::fem::merge(meshes, &gaps, &touching)
    };
    let mut mesh = mesh;
    let everything: Vec<String> = model.named_solids().into_iter().map(|(n, _)| n).collect();
    let original: Vec<usize> = named
        .iter()
        .map(|(n, _)| everything.iter().position(|m| m == n).unwrap_or(0))
        .collect();
    let stride = super::fem::STRIDE;
    for (_, tag) in &mut mesh.boundary {
        *tag = original[*tag / stride] * stride + *tag % stride;
    }
    mesh.faces = mesh
        .boundary
        .iter()
        .map(|(_, tag)| tag + 1)
        .max()
        .unwrap_or(0)
        .max(mesh.faces);
    Ok(Prepared {
        mesh,
        parts,
        original,
    })
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
    stiff(spec)?;
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
    let wind = spec
        .wind_load
        .as_deref()
        .map(super::wind::surface_pressure)
        .transpose()?;
    let answer = solve(&Problem {
        mesh,
        materials: &materials,
        supports: &spec.supports,
        loads: &spec.loads,
        body_force,
        expansion: prepared.expansion(spec.warming.as_ref())?,
        wind: wind.as_ref(),
    })?;
    let mut readings = Vec::new();
    let mut notes = Vec::new();
    if let (Some(wind), Some(file)) = (&wind, &spec.wind_load) {
        let f = super::fem::elastic::wind_force(mesh, wind);
        notes.push(format!(
            "wind from {}: {} N ({}) of surface pressure on the parts",
            file.display(),
            short(f.magnitude()),
            triple(f)
        ));
    }
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
    for mark in stimuli(spec, mesh, None) {
        if mark.kind == Stimulus::Push {
            notes.push(format!("applied {}", mark.text));
        }
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
    pub marks: Vec<Mark>,
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
    stiff(spec)?;
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
        marks: stimuli(spec, mesh, Some(outcome))
            .into_iter()
            .map(|mut m| {
                if !m.nodes.is_empty() {
                    let shift = m
                        .nodes
                        .iter()
                        .map(|&v| positions_shift(&field.displacement[v as usize], scale))
                        .sum::<Vector3>()
                        / m.nodes.len() as f64;
                    m.at += shift;
                }
                m
            })
            .collect(),
    })
}

fn positions_shift(d: &[f64; 3], scale: f64) -> Vector3 {
    Vector3::new(d[0], d[1], d[2]) * scale
}

pub fn render(spec: &Spec, outcome: &Outcome, path: &str) -> Result<()> {
    draw(&field_view(spec, outcome)?, path)
}

pub fn draw(view: &FieldView, path: &str) -> Result<()> {
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
    let mut edges: Vec<Vec<Point3>> = view
        .edges
        .iter()
        .map(|e| e.iter().map(|&v| positions[v as usize]).collect())
        .collect();
    let shown: BoundingBox<Point3> = positions.iter().copied().collect();
    let length = shown.diameter() * 0.2;
    for mark in &view.marks {
        let Some(toward) = mark.toward.filter(|_| !mark.nodes.is_empty()) else {
            continue;
        };
        let tail = mark.at - toward * length;
        let side = if toward.x.abs() < 0.9 {
            toward.cross(Vector3::unit_x())
        } else {
            toward.cross(Vector3::unit_y())
        }
        .normalize();
        let barb = |s: f64| mark.at - toward * (length * 0.2) + side * (length * 0.1 * s);
        edges.push(vec![tail, mark.at]);
        edges.push(vec![barb(1.0), mark.at, barb(-1.0)]);
    }
    let picture = crate::render::draw_field(positions, &triangles, &edges, shown);
    let listed: Vec<&str> = view.marks.iter().map(|m| m.text.as_str()).collect();
    let below = 22 * listed.len() as u32 + if listed.is_empty() { 0 } else { 16 };
    let mut canvas = crate::plot::Canvas::new(picture.width() + 260, picture.height() + 60 + below);
    canvas.paste(&picture, (0, 60));
    for (k, text) in listed.iter().enumerate() {
        canvas.text(
            (24.0, (picture.height() + 60 + 24 + 22 * k as u32) as f64),
            14.0,
            text,
            crate::plot::Anchor::Left,
            [40, 40, 40],
        );
    }
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
    if spec.loads.is_empty()
        && body_force.is_none()
        && spec.warming.is_none()
        && spec.wind_load.is_none()
    {
        bail!("buckling needs a `load`, `pressure`, `torque`, `warm` or `wind` to scale");
    }
    let wind = spec
        .wind_load
        .as_deref()
        .map(super::wind::surface_pressure)
        .transpose()?;
    let wanted = spec.modes.unwrap_or(3);
    let buckles = super::fem::buckling::solve(
        &Problem {
            mesh,
            materials: &materials,
            supports: &spec.supports,
            loads: &spec.loads,
            body_force,
            expansion: prepared.expansion(spec.warming.as_ref())?,
            wind: wind.as_ref(),
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
    for mark in stimuli(spec, mesh, None) {
        if mark.kind == Stimulus::Push {
            notes.push(format!("applied {}", mark.text));
        }
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stimulus {
    Push,
    Hold,
    Heat,
}

#[derive(Clone, Debug)]
pub struct Mark {
    pub kind: Stimulus,
    pub text: String,
    pub nodes: Vec<u32>,
    pub outline: Vec<[u32; 3]>,
    pub spot: Option<u32>,
    pub at: Point3,
    pub toward: Option<Vector3>,
}

struct Patch {
    nodes: Vec<u32>,
    outline: Vec<[u32; 3]>,
    spot: Option<u32>,
    at: Point3,
    normal: Vector3,
    area: f64,
}

fn patch(mesh: &Quadratic, faces: &[usize]) -> Patch {
    let mut nodes = Vec::new();
    let mut centre = Vector3::zero();
    let mut normal = Vector3::zero();
    let mut area = 0.0;
    let mut sides: std::collections::BTreeMap<(u32, u32), (u32, usize)> =
        std::collections::BTreeMap::new();
    for (f, tag) in &mesh.boundary {
        if !faces.contains(tag) {
            continue;
        }
        nodes.extend_from_slice(f);
        for (a, m, b) in [(0, 3, 1), (1, 4, 2), (2, 5, 0)] {
            sides
                .entry((f[a].min(f[b]), f[a].max(f[b])))
                .or_insert((f[m], 0))
                .1 += 1;
        }
        let [a, b, c] = [f[0], f[1], f[2]].map(|v| mesh.nodes[v as usize]);
        let n = (b - a).cross(c - a) * 0.5;
        let da = n.magnitude();
        area += da;
        normal += n;
        centre += (a.to_vec() + b.to_vec() + c.to_vec()) / 3.0 * da;
    }
    nodes.sort_unstable();
    nodes.dedup();
    let at = Point3::from_vec(centre / area.max(1.0e-300));
    let spot = nodes.iter().copied().min_by(|&p, &q| {
        (mesh.nodes[p as usize] - at)
            .magnitude2()
            .total_cmp(&(mesh.nodes[q as usize] - at).magnitude2())
    });
    let outline = sides
        .into_iter()
        .filter(|(_, (_, count))| *count == 1)
        .map(|((a, b), (m, _))| [a, m, b])
        .collect();
    Patch {
        nodes,
        outline,
        spot,
        at,
        normal,
        area,
    }
}

fn triple(v: Vector3) -> String {
    format!("{}, {}, {}", short(v.x), short(v.y), short(v.z))
}

pub fn stimuli(spec: &Spec, mesh: &Quadratic, outcome: Option<&Outcome>) -> Vec<Mark> {
    use super::fem::elastic::{Hold, Push};
    use super::fem::thermal::Boundary;
    let mut marks = Vec::new();
    for load in &spec.loads {
        let Patch {
            nodes,
            outline,
            spot,
            at,
            normal,
            area,
        } = patch(mesh, &load.faces);
        let (text, toward) = match load.push {
            Push::Force(f) => (
                format!("{}: {} N ({})", load.label, short(f.magnitude()), triple(f)),
                f,
            ),
            Push::Pressure(p) => {
                let total = -normal * p;
                (
                    format!(
                        "{}: {} MPa over {} mm², {} N in all",
                        load.label,
                        short(p),
                        short(area),
                        short(total.magnitude())
                    ),
                    total,
                )
            }
            Push::Torque { axis, moment, .. } => (
                format!(
                    "{}: {} N·m about ({})",
                    load.label,
                    short(moment * 1.0e-3),
                    triple(axis)
                ),
                axis * moment.signum(),
            ),
        };
        marks.push(Mark {
            kind: Stimulus::Push,
            text,
            nodes,
            outline,
            spot,
            at,
            toward: (toward.magnitude() > 0.0).then(|| toward.normalize()),
        });
    }
    for support in &spec.supports {
        let Patch {
            nodes,
            outline,
            spot,
            at,
            ..
        } = patch(mesh, &support.faces);
        let how = match support.hold {
            Hold::Fixed => "fixed",
            Hold::Slide => "slides on its face",
            Hold::Pin { .. } => "pinned, free to turn",
        };
        let reaction = outcome
            .and_then(|o| o.reading(&support.label))
            .map(|r| format!(", holds {} N", short(r.value)))
            .unwrap_or_default();
        marks.push(Mark {
            kind: Stimulus::Hold,
            text: format!("{}: {how}{reaction}", support.label),
            nodes,
            outline,
            spot,
            at,
            toward: None,
        });
    }
    for boundary in &spec.thermal {
        let (faces, text) = match boundary {
            Boundary::Temperature {
                label,
                faces,
                value,
            } => (faces, format!("{label}: held at {} °C", short(*value))),
            Boundary::Flux {
                label,
                faces,
                watts,
            } => (faces, format!("{label}: {} W in", short(*watts))),
            Boundary::Convect {
                label,
                faces,
                film,
                ambient,
            } => (
                faces,
                format!(
                    "{label}: air at {} °C, h {} W/m²·K",
                    short(*ambient),
                    short(*film)
                ),
            ),
        };
        let Patch {
            nodes,
            outline,
            spot,
            at,
            normal,
            ..
        } = patch(mesh, faces);
        let toward = matches!(boundary, Boundary::Flux { .. })
            .then(|| -normal)
            .filter(|n| n.magnitude() > 0.0)
            .map(|n| n.normalize());
        marks.push(Mark {
            kind: Stimulus::Heat,
            text,
            nodes,
            outline,
            spot,
            at,
            toward,
        });
    }
    if let Some(g) = spec.gravity
        && g.magnitude() > 0.0
    {
        marks.push(Mark {
            kind: Stimulus::Push,
            text: format!("gravity: {} m/s² ({})", short(g.magnitude()), triple(g)),
            nodes: Vec::new(),
            outline: Vec::new(),
            spot: None,
            at: Point3::origin(),
            toward: None,
        });
    }
    marks
}
