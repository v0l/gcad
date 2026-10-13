pub mod field;
pub mod plane;
pub mod steel;

use super::{Curve, Field, Outcome, Reading, Series, Spec, statics::FieldView};
use anyhow::{Result, bail};
use monstertruck::modeling::*;
use plane::{Gap, Layout, Plane, Shape, Stuff, Toward};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Turn {
    pub joint: String,
    pub sweep: f64,
    pub steps: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Pointing {
    Radial(f64),
    Along(Vector3),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Role {
    Magnet {
        br: f64,
        mu: f64,
        toward: Pointing,
    },
    Iron {
        curve: Option<String>,
        mu: f64,
    },
    Coil {
        turns: f64,
        current: f64,
        axis: Option<Vector3>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Assigned {
    pub label: String,
    pub patterns: Vec<String>,
    pub role: Role,
}

struct Frame {
    through: Point3,
    axis: Vector3,
    x: Vector3,
    y: Vector3,
    at: f64,
}

impl Frame {
    fn flat(&self, p: Point3) -> [f64; 2] {
        let v = p - self.through;
        [v.dot(self.x) * 1.0e-3, v.dot(self.y) * 1.0e-3]
    }

    fn lift(&self, p: [f64; 2]) -> Point3 {
        self.through + self.axis * self.at + self.x * (p[0] * 1.0e3) + self.y * (p[1] * 1.0e3)
    }
}

struct Coil {
    label: String,
    current: f64,
    shapes: Vec<(usize, f64)>,
}

struct Setup {
    frame: Frame,
    shapes: Vec<Shape>,
    parts: Vec<String>,
    coils: Vec<Coil>,
    irons: Vec<(String, Vec<usize>)>,
    layout: Layout,
    length: f64,
    sweep: f64,
    steps: usize,
    pitch: i64,
    left_out: Vec<String>,
}

fn regions(loops: Vec<Vec<[f64; 2]>>) -> Vec<Vec<Vec<[f64; 2]>>> {
    let area = |l: &[[f64; 2]]| {
        (0..l.len())
            .map(|k| {
                let (a, b) = (l[k], l[(k + 1) % l.len()]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            .abs()
            / 2.0
    };
    let depth: Vec<usize> = loops
        .iter()
        .enumerate()
        .map(|(i, l)| {
            loops
                .iter()
                .enumerate()
                .filter(|(j, other)| *j != i && plane::inside(l[0], std::slice::from_ref(other)))
                .count()
        })
        .collect();
    let mut out: Vec<(usize, Vec<Vec<[f64; 2]>>)> = Vec::new();
    for (i, l) in loops.iter().enumerate() {
        if depth[i].is_multiple_of(2) {
            out.push((i, vec![l.clone()]));
        }
    }
    for (i, l) in loops.iter().enumerate() {
        if depth[i] % 2 == 1 {
            let owner = out
                .iter_mut()
                .filter(|(k, _)| plane::inside(l[0], std::slice::from_ref(&loops[*k])))
                .min_by(|a, b| area(&loops[a.0]).total_cmp(&area(&loops[b.0])));
            if let Some((_, region)) = owner {
                region.push(l.clone());
            }
        }
    }
    out.into_iter().map(|(_, r)| r).collect()
}

fn canonical(mut ring: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let n = ring.len();
    let twice: f64 = (0..n)
        .map(|k| {
            let (a, b) = (ring[k], ring[(k + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    if twice < 0.0 {
        ring.reverse();
    }
    let first = (0..n)
        .min_by(|&i, &j| {
            ring[i][0]
                .total_cmp(&ring[j][0])
                .then(ring[i][1].total_cmp(&ring[j][1]))
        })
        .unwrap_or(0);
    ring.rotate_left(first);
    ring
}

fn joint_frame(spec: &Spec) -> Result<(Frame, Vec<String>, Turn)> {
    let Some(turn) = spec.turn.clone() else {
        bail!("a magnetic simulation needs `turn joint`, the joint the rotor turns on");
    };
    let rig = spec.target()?.rig();
    let Some(joint) = rig.joints.iter().find(|j| j.name == turn.joint) else {
        bail!("no joint called `{}`", turn.joint);
    };
    let crate::model::JointKind::Turn { through, axis } = joint.kind else {
        bail!("`{}` is not a `turn` joint", turn.joint);
    };
    let axis = axis.normalize();
    let seed = if axis.x.abs() < 0.9 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    };
    let x = (seed - axis * seed.dot(axis)).normalize();
    let y = axis.cross(x);
    let carried = crate::model::subtree(&rig.joints, &joint.child);
    Ok((
        Frame {
            through,
            axis,
            x,
            y,
            at: 0.0,
        },
        carried,
        turn,
    ))
}

fn carried_by(carried: &[String], name: &str) -> bool {
    carried.iter().any(|c| {
        name == c
            || name
                .strip_prefix(c.as_str())
                .is_some_and(|r| r.starts_with('.'))
    })
}

fn setup(spec: &Spec) -> Result<Setup> {
    let (mut frame, carried, turn) = joint_frame(spec)?;
    let named = spec.named()?;
    let mut chosen: Vec<(usize, &Assigned)> = Vec::new();
    for assigned in &spec.roles {
        let mut hit = false;
        for (k, (name, _)) in named.iter().enumerate() {
            if assigned.patterns.iter().any(|p| super::matches(p, name)) {
                if chosen.iter().any(|(c, _)| *c == k) {
                    bail!("`{name}` is given two roles");
                }
                chosen.push((k, assigned));
                hit = true;
            }
        }
        if !hit {
            bail!(
                "`{}` matches no part; parts are {:?}",
                assigned.patterns.join(" "),
                named.iter().map(|(n, _)| n).collect::<Vec<_>>()
            );
        }
    }
    let left_out: Vec<String> = named
        .iter()
        .enumerate()
        .filter(|(k, _)| !chosen.iter().any(|(c, _)| c == k))
        .map(|(_, (n, _))| n.clone())
        .collect();
    let along = |p: Point3| (p - frame.through).dot(frame.axis);
    let mut turning_span = (f64::INFINITY, f64::NEG_INFINITY);
    let mut all_span = (f64::INFINITY, f64::NEG_INFINITY);
    let meshes: Vec<(Vec<Point3>, Vec<[usize; 3]>)> = chosen
        .iter()
        .map(|(k, _)| {
            crate::geometry::welded(&crate::geometry::mesh(
                named[*k].1,
                crate::geometry::mesh_tolerance(named[*k].1),
            ))
        })
        .collect();
    for ((k, _), (points, _)) in chosen.iter().zip(&meshes) {
        for &p in points {
            let a = along(p);
            all_span = (all_span.0.min(a), all_span.1.max(a));
            if carried_by(&carried, &named[*k].0) {
                turning_span = (turning_span.0.min(a), turning_span.1.max(a));
            }
        }
    }
    if !turning_span.0.is_finite() {
        bail!(
            "nothing that `{}` turns has a magnetic role; give the rotor's parts `iron` or `magnet`",
            turn.joint
        );
    }
    frame.at = match spec.slice {
        Some(at) => at,
        None => 0.5 * (turning_span.0 + turning_span.1),
    };
    let length = spec.stack.unwrap_or(turning_span.1 - turning_span.0);
    if length <= 0.0 {
        bail!("the stack length must be positive");
    }
    let plane_offset = frame.through.to_vec().dot(frame.axis) + frame.at;
    let mut shapes = Vec::new();
    let mut parts = Vec::new();
    let mut coils: Vec<Coil> = Vec::new();
    let mut irons: Vec<(String, Vec<usize>)> = Vec::new();
    for (k, assigned) in &chosen {
        let (name, solid) = &named[*k];
        let turning = carried_by(&carried, name);
        let loops: Vec<Vec<[f64; 2]>> =
            crate::drawing::section_loops(solid, frame.axis, plane_offset)
                .into_iter()
                .map(|l| {
                    let mut flat: Vec<[f64; 2]> = l.iter().map(|&p| frame.flat(p)).collect();
                    if flat.len() > 1 && flat[0] == flat[flat.len() - 1] {
                        flat.pop();
                    }
                    flat
                })
                .filter(|l| l.len() >= 3)
                .map(canonical)
                .collect();
        let mut loops = loops;
        loops.sort_by(|a, b| {
            a[0][0]
                .total_cmp(&b[0][0])
                .then(a[0][1].total_cmp(&b[0][1]))
        });
        if loops.is_empty() {
            bail!(
                "the slice {} mm along `{}` misses `{name}`; move it with `slice at=`",
                crate::plot::label(frame.at),
                turn.joint
            );
        }
        match &assigned.role {
            Role::Magnet { br, mu, toward } => {
                let toward = match toward {
                    Pointing::Radial(sign) => Toward::Radial(*sign),
                    Pointing::Along(d) => {
                        let v = [d.dot(frame.x), d.dot(frame.y)];
                        let n = v[0].hypot(v[1]);
                        if n < 1.0e-9 {
                            bail!(
                                "`{name}` is magnetized along the turning axis, which a slice cannot show"
                            );
                        }
                        Toward::Along([v[0] / n, v[1] / n])
                    }
                };
                for region in regions(loops) {
                    shapes.push(Shape {
                        name: name.clone(),
                        loops: region,
                        stuff: Stuff::Magnet {
                            br: *br,
                            mu: *mu,
                            toward: toward.clone(),
                        },
                        turning,
                    });
                    parts.push(name.clone());
                }
            }
            Role::Iron { curve, mu } => {
                let stuff = match curve {
                    Some(c) => Stuff::Steel(
                        steel::named(c).ok_or_else(|| anyhow::anyhow!("no steel called `{c}`"))?,
                    ),
                    None => Stuff::Iron { mu: *mu },
                };
                let mut indices = Vec::new();
                for region in regions(loops) {
                    indices.push(shapes.len());
                    shapes.push(Shape {
                        name: name.clone(),
                        loops: region,
                        stuff: stuff.clone(),
                        turning,
                    });
                    parts.push(name.clone());
                }
                irons.push((name.clone(), indices));
            }
            Role::Coil {
                turns,
                current,
                axis,
            } => {
                let pieces = regions(loops);
                let centre = {
                    let all: Vec<[f64; 2]> = pieces.iter().flat_map(|r| r[0].clone()).collect();
                    let n = all.len() as f64;
                    all.iter()
                        .fold([0.0, 0.0], |m, p| [m[0] + p[0] / n, m[1] + p[1] / n])
                };
                let winding = match axis {
                    Some(a) => [a.dot(frame.x), a.dot(frame.y)],
                    None => {
                        let r = centre[0].hypot(centre[1]).max(1.0e-12);
                        [centre[0] / r, centre[1] / r]
                    }
                };
                let coil = match coils.iter().position(|c| c.label == assigned.label) {
                    Some(c) => c,
                    None => {
                        coils.push(Coil {
                            label: assigned.label.clone(),
                            current: *current,
                            shapes: Vec::new(),
                        });
                        coils.len() - 1
                    }
                };
                let single = pieces.len() == 1;
                for region in pieces {
                    let n = region[0].len() as f64;
                    let middle = region[0]
                        .iter()
                        .fold([0.0, 0.0], |m, p| [m[0] + p[0] / n, m[1] + p[1] / n]);
                    let offset = [middle[0] - centre[0], middle[1] - centre[1]];
                    let against = winding[0] * offset[1] - winding[1] * offset[0] < 0.0;
                    let sign = if !single && against { -1.0 } else { 1.0 };
                    coils[coil].shapes.push((shapes.len(), sign * turns));
                    shapes.push(Shape {
                        name: name.clone(),
                        loops: region,
                        stuff: Stuff::Coil {
                            amps: sign * turns * current,
                        },
                        turning,
                    });
                    parts.push(name.clone());
                }
            }
        }
    }
    let radius = |s: &Shape, pick: fn(f64, f64) -> f64, start: f64| {
        s.loops
            .iter()
            .flatten()
            .map(|p| p[0].hypot(p[1]))
            .fold(start, pick)
    };
    let inner = shapes
        .iter()
        .filter(|s| s.turning)
        .map(|s| radius(s, f64::max, 0.0))
        .fold(0.0, f64::max);
    let outer = shapes
        .iter()
        .filter(|s| !s.turning)
        .map(|s| radius(s, f64::min, f64::INFINITY))
        .fold(f64::INFINITY, f64::min);
    if !outer.is_finite() {
        bail!("nothing stands still around the rotor; give the stator's parts a role");
    }
    if outer <= inner * 1.0005 {
        bail!(
            "the turning parts reach {} mm from the axis and the still ones come within {} mm, so there is no airgap to turn in",
            crate::plot::label(inner * 1.0e3),
            crate::plot::label(outer * 1.0e3)
        );
    }
    let width = outer - inner;
    let reach = shapes
        .iter()
        .map(|s| radius(s, f64::max, 0.0))
        .fold(0.0, f64::max);
    let size = spec
        .mesh
        .map(|m| m * 1.0e-3)
        .unwrap_or_else(|| (0.5 * width).max(reach / 200.0));
    let middle = 0.5 * (inner + outer);
    let fewest = ((std::f64::consts::TAU * middle / (0.5 * width)).ceil() as usize).max(90);
    let (sweep, steps) = if turn.steps > 0 {
        (turn.sweep, turn.steps)
    } else {
        (0.0, 0)
    };
    let (nodes, pitch) = if steps == 0 {
        (fewest, 0)
    } else {
        let step = sweep / steps as f64;
        let per_turn = 360.0 / step;
        let mut found = None;
        for k in 1..=4000usize {
            let n = per_turn * k as f64;
            if (n - n.round()).abs() < 1.0e-6 && n.round() as usize >= fewest {
                found = Some((n.round() as usize, k as i64));
                break;
            }
        }
        found.ok_or_else(|| {
            anyhow::anyhow!(
                "a sweep of {sweep}° in {steps} steps does not divide a turn into whole steps; pick a sweep and steps whose step divides 360°"
            )
        })?
    };
    let layout = Layout {
        shapes: shapes.clone(),
        size,
        outer: 1.5 * reach,
        gap: Some(Gap {
            inner,
            outer,
            nodes,
        }),
    };
    Ok(Setup {
        frame,
        shapes,
        parts,
        coils,
        irons,
        layout,
        length: length * 1.0e-3,
        sweep,
        steps,
        pitch,
        left_out,
    })
}

pub fn size(spec: &Spec) -> Result<f64> {
    Ok(setup(spec)?.layout.size * 1.0e3)
}

pub fn plan(spec: &Spec) -> Result<String> {
    let setup = setup(spec)?;
    let plane = plane::mesh(&setup.layout)?;
    let gap = setup.layout.gap.expect("a gap");
    Ok(format!(
        "slice {} mm along the axis, {} regions, {} nodes and {} triangles of about {} mm\nairgap from {} to {} mm, {} nodes round it; stack {} mm\n{} poses",
        crate::plot::label(setup.frame.at),
        setup.shapes.len(),
        plane.points.len(),
        plane.triangles.len(),
        crate::plot::label(setup.layout.size * 1.0e3),
        crate::plot::label(gap.inner * 1.0e3),
        crate::plot::label(gap.outer * 1.0e3),
        gap.nodes,
        crate::plot::label(setup.length * 1.0e3),
        setup.steps + 1
    ))
}

fn reading(name: &str, value: f64, unit: &str, at: &str) -> Reading {
    Reading {
        name: name.into(),
        value,
        unit: unit.into(),
        at: at.into(),
    }
}

struct Pose {
    torque: f64,
    fluxes: Vec<f64>,
    gap: f64,
    peaks: Vec<f64>,
    shown: Option<Vec<f64>>,
}

fn node_values(plane: &Plane, field: &field::Field) -> Vec<f64> {
    let mut sums = vec![(0.0, 0.0); plane.points.len()];
    for (t, b) in plane.triangles.iter().zip(&field.flux) {
        let [p, q, r] = t.map(|v| field.points[v]);
        let area = 0.5 * ((q[0] - p[0]) * (r[1] - p[1]) - (r[0] - p[0]) * (q[1] - p[1]));
        let size = b[0].hypot(b[1]);
        for &v in t {
            sums[v].0 += area * size;
            sums[v].1 += area;
        }
    }
    sums.iter().map(|(s, a)| s / a.max(1.0e-30)).collect()
}

pub fn run(spec: &Spec) -> Result<Outcome> {
    let setup = setup(spec)?;
    let plane = plane::mesh(&setup.layout)?;
    let poses = setup.steps + 1;
    let step_angle = if setup.steps > 0 {
        setup.sweep / setup.steps as f64
    } else {
        0.0
    };
    let areas = field::areas(&plane, setup.shapes.len());
    use rayon::prelude::*;
    let solved: Vec<Pose> = (0..poses)
        .into_par_iter()
        .map(|k| -> Result<_> {
            let field = field::solve(&plane, &setup.shapes, k as i64 * setup.pitch)?;
            let torque = field::torque(&plane, &field) * setup.length;
            let fluxes: Vec<f64> = setup
                .coils
                .iter()
                .map(|c| {
                    c.shapes
                        .iter()
                        .map(|&(s, factor)| factor * field::linkage(&plane, &field, s, areas[s]))
                        .sum::<f64>()
                        * setup.length
                })
                .collect();
            let gap = plane
                .band
                .iter()
                .map(|&e| field.flux[e][0].hypot(field.flux[e][1]))
                .fold(0.0, f64::max);
            let peaks: Vec<f64> = setup
                .irons
                .iter()
                .map(|(_, indices)| {
                    plane
                        .region
                        .iter()
                        .zip(&field.flux)
                        .filter(|(r, _)| r.is_some_and(|r| indices.contains(&r)))
                        .map(|(_, b)| b[0].hypot(b[1]))
                        .fold(0.0, f64::max)
                })
                .collect();
            let shown = (k == 0).then(|| node_values(&plane, &field));
            Ok(Pose {
                torque,
                fluxes,
                gap,
                peaks,
                shown,
            })
        })
        .collect::<Result<_>>()?;
    let torques: Vec<f64> = solved.iter().map(|s| s.torque).collect();
    let mean = torques.iter().sum::<f64>() / poses as f64;
    let high = torques.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let low = torques.iter().copied().fold(f64::INFINITY, f64::min);
    let idle = setup.coils.iter().all(|c| c.current == 0.0);
    let joint = spec
        .turn
        .as_ref()
        .map(|t| t.joint.clone())
        .unwrap_or_default();
    let mut readings = vec![reading(
        "torque",
        mean,
        "N·m",
        if poses > 1 {
            "on the rotor about the joint axis, mean over the sweep"
        } else {
            "on the rotor about the joint axis"
        },
    )];
    if poses > 1 {
        readings.push(reading("torque.max", high, "N·m", ""));
        readings.push(reading("torque.min", low, "N·m", ""));
        readings.push(reading(
            if idle { "cogging" } else { "ripple" },
            high - low,
            "N·m",
            "peak to peak over the sweep",
        ));
    }
    readings.push(reading(
        "airgap",
        solved.iter().map(|s| s.gap).fold(0.0, f64::max),
        "T",
        "peak flux density in the airgap",
    ));
    for (k, (name, _)) in setup.irons.iter().enumerate() {
        readings.push(reading(
            &format!("{name}.b"),
            solved.iter().map(|s| s.peaks[k]).fold(0.0, f64::max),
            "T",
            "peak flux density",
        ));
    }
    let radians = step_angle.to_radians();
    let omega = 1000.0 * std::f64::consts::TAU / 60.0;
    for (k, coil) in setup.coils.iter().enumerate() {
        readings.push(reading(
            &format!("{}.flux", coil.label),
            solved[0].fluxes[k],
            "Wb",
            "flux linkage at the first pose",
        ));
        if poses > 2 {
            let emf = (1..poses - 1)
                .map(|i| {
                    ((solved[i + 1].fluxes[k] - solved[i - 1].fluxes[k]) / (2.0 * radians) * omega)
                        .abs()
                })
                .fold(0.0, f64::max);
            readings.push(reading(
                &format!("{}.emf", coil.label),
                emf,
                "V",
                "peak at 1000 rpm, from the flux linkage over the sweep",
            ));
        }
    }
    let mut series = Vec::new();
    if poses > 1 {
        let x: Vec<f64> = (0..poses).map(|k| k as f64 * step_angle).collect();
        let mut curves = vec![Curve {
            name: "torque".into(),
            unit: "N·m".into(),
            values: torques.clone(),
        }];
        for (k, coil) in setup.coils.iter().enumerate() {
            curves.push(Curve {
                name: format!("{}.flux", coil.label),
                unit: "Wb".into(),
                values: solved.iter().map(|s| s.fluxes[k]).collect(),
            });
        }
        series.push(Series {
            title: format!("{joint} turned"),
            axis: "°".into(),
            x,
            curves,
        });
    }
    let gap = setup.layout.gap.expect("a gap");
    let mut notes = vec![format!(
        "a 2D slice {} mm along the axis, times a {} mm stack: {} triangles, the airgap from {} to {} mm split into {} steps round",
        crate::plot::label(setup.frame.at),
        crate::plot::label(setup.length * 1.0e3),
        plane.triangles.len(),
        crate::plot::label(gap.inner * 1.0e3),
        crate::plot::label(gap.outer * 1.0e3),
        gap.nodes
    )];
    notes.push(
        "a slice leaves out end effects: the coil ends, the field fringing past the stack and any skew"
            .into(),
    );
    if !setup.left_out.is_empty() {
        notes.push(format!(
            "{} have no magnetic role and count as air",
            setup.left_out.join(", ")
        ));
    }
    let _ = &setup.parts;
    Ok(Outcome {
        kind: "magnetic".into(),
        readings,
        notes,
        series,
        field: solved[0].shown.clone().map(|values| Field {
            title: "flux density".into(),
            unit: "T".into(),
            values,
            displacement: Vec::new(),
        }),
        ..Default::default()
    })
}

pub fn field_view(spec: &Spec, outcome: Option<&Outcome>) -> Result<FieldView> {
    let setup = setup(spec)?;
    let plane = plane::mesh(&setup.layout)?;
    let positions: Vec<Point3> = plane.points.iter().map(|&p| setup.frame.lift(p)).collect();
    let (values, title, unit) = match outcome.and_then(|o| o.field.as_ref()) {
        Some(field) if field.values.len() == positions.len() => (
            field.values.clone(),
            field.title.clone(),
            field.unit.clone(),
        ),
        Some(_) => bail!("the result does not match the slice; rerun `gcad sim`"),
        None => {
            let mut v = vec![0.0; positions.len()];
            for (t, r) in plane.triangles.iter().zip(&plane.region) {
                for &n in t {
                    v[n] = r.map_or(0.0, |r| r as f64 + 1.0);
                }
            }
            (v, "the slice".to_string(), String::new())
        }
    };
    let mut edges = Vec::new();
    let mut sides: std::collections::HashMap<(usize, usize), Option<usize>> = Default::default();
    for (t, r) in plane.triangles.iter().zip(&plane.region) {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let key = (a.min(b), a.max(b));
            match sides.remove(&key) {
                Some(other) if other != *r => edges.push([a as u32, a as u32, b as u32]),
                Some(_) => {}
                None => {
                    sides.insert(key, *r);
                }
            }
        }
    }
    for ((a, b), r) in sides {
        if r.is_some() {
            edges.push([a as u32, a as u32, b as u32]);
        }
    }
    let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let low = values.iter().copied().fold(f64::INFINITY, f64::min);
    let span = (high - low).max(1.0e-12);
    Ok(FieldView {
        shade: values.iter().map(|v| (v - low) / span).collect(),
        triangles: plane
            .triangles
            .iter()
            .map(|t| t.map(|v| v as u32))
            .collect(),
        edges,
        low,
        high: low + span,
        scale: 0.0,
        title,
        unit,
        values,
        moved: vec![Vector3::zero(); positions.len()],
        rest: positions.clone(),
        positions,
        marks: Vec::new(),
        index: Default::default(),
        slices: Vec::new(),
        streams: Vec::new(),
        travel: Default::default(),
        movie: None,
    })
}
