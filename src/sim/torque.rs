use super::{Curve, Outcome, Reading, Series, Spec};
use crate::geometry;
use crate::model::{JointKind, Model, Rig, posed};
use anyhow::{Result, bail};
use monstertruck::meshing::prelude::*;
use rayon::prelude::*;

struct Lump {
    part: String,
    kg: f64,
    centroid: Point3,
}

fn body_lumps(model: &Model) -> (Vec<Lump>, Vec<String>) {
    let named = model.named_solids();
    let measured: Vec<(String, f64, Point3)> = named
        .par_iter()
        .map(|(name, solid)| {
            let mesh = geometry::mesh(solid, geometry::mesh_tolerance(solid) * 0.5);
            let positions = mesh.positions();
            let (mut volume, mut moment) = (0.0, Vector3::zero());
            for triangle in mesh.faces().triangle_iter() {
                let [a, b, c] = triangle.map(|v| positions[v.pos].to_vec());
                let six = a.dot(b.cross(c));
                volume += six / 6.0;
                moment += (a + b + c) * (six / 24.0);
            }
            (name.clone(), volume, Point3::from_vec(moment / volume))
        })
        .collect();
    let mut bare = Vec::new();
    let lumps = measured
        .into_iter()
        .filter_map(
            |(name, volume, centroid)| match model.materials.get(&name) {
                Some(material) => Some(Lump {
                    part: name,
                    kg: volume.abs() * material.density / 1.0e6,
                    centroid,
                }),
                None => {
                    bare.push(name);
                    None
                }
            },
        )
        .collect();
    (lumps, bare)
}

fn lumps(spec: &Spec, model: &Model) -> Result<(Vec<Lump>, Vec<String>)> {
    let (mut lumps, bare) = body_lumps(model);
    let mut notes = Vec::new();
    if !bare.is_empty() {
        notes.push(format!("no material, so no weight: {}", bare.join(", ")));
    }
    for payload in &spec.payloads {
        let at = match payload.at {
            Some(at) => at,
            None => {
                let own: Vec<&Lump> = lumps.iter().filter(|l| l.part == payload.part).collect();
                let kg: f64 = own.iter().map(|l| l.kg).sum();
                if kg <= 0.0 {
                    bail!(
                        "`{}` has no mass to put a payload at its centre; give `at=`",
                        payload.part
                    );
                }
                Point3::from_vec(
                    own.iter()
                        .map(|l| l.centroid.to_vec() * l.kg)
                        .sum::<Vector3>()
                        / kg,
                )
            }
        };
        lumps.push(Lump {
            part: payload.part.clone(),
            kg: payload.grams / 1000.0,
            centroid: at,
        });
    }
    Ok((lumps, notes))
}

fn energy(
    rig: &Rig,
    lumps: &[Lump],
    pushes: &[(usize, f64)],
    gravity: Vector3,
    values: &[f64],
) -> f64 {
    let moves = posed(&rig.joints, values);
    let weight: f64 = lumps
        .iter()
        .map(|lump| {
            let at = moves
                .get(&lump.part)
                .map_or(lump.centroid, |m| m.transform_point(lump.centroid));
            -lump.kg * gravity.dot(at.to_vec()) / 1000.0
        })
        .sum();
    weight
        - pushes
            .iter()
            .map(|&(joint, load)| load * values[joint])
            .sum::<f64>()
}

#[derive(Clone, Copy)]
struct Root {
    driver: usize,
    ratio: f64,
    efficiency: f64,
    back: f64,
    drag: f64,
    held: f64,
}

impl Root {
    fn lossy(&self) -> bool {
        self.efficiency < 1.0 || self.back < 1.0 || self.drag > 0.0
    }
}

fn chain(rig: &Rig, joint: usize) -> Vec<usize> {
    let index = |name: &str| rig.joints.iter().position(|j| j.name == name);
    let mut path = vec![joint];
    while path.len() <= rig.couples.len() {
        let at = *path.last().expect("non-empty");
        match rig
            .driver_of(&rig.joints[at].name)
            .and_then(|c| index(&c.driver))
        {
            Some(driver) => path.push(driver),
            None => break,
        }
    }
    path
}

fn reflect_drag(rig: &Rig, roots: &mut [Root]) {
    let reflected: Vec<(f64, f64)> = (0..roots.len())
        .map(|i| {
            let path = chain(rig, i);
            (0..roots.len())
                .filter(|&k| rig.joints[k].drag > 0.0 && roots[k].driver == roots[i].driver)
                .map(|k| {
                    let lever = (roots[k].ratio / roots[i].ratio * per_unit(rig, i)
                        / per_unit(rig, k))
                    .abs();
                    let through = if path.contains(&k) {
                        roots[i].efficiency / roots[k].efficiency
                    } else {
                        1.0
                    };
                    let drag = rig.joints[k].drag * lever;
                    (drag / through, drag)
                })
                .fold((0.0, 0.0), |(a, b), (c, d)| (a + c, b + d))
        })
        .collect();
    for (root, (drag, held)) in roots.iter_mut().zip(reflected) {
        root.drag = drag;
        root.held = held;
    }
}

fn root(rig: &Rig, joint: usize) -> Root {
    let index = |name: &str| rig.joints.iter().position(|j| j.name == name);
    let mut root = Root {
        driver: joint,
        ratio: 1.0,
        efficiency: 1.0,
        back: 1.0,
        drag: 0.0,
        held: 0.0,
    };
    for _ in 0..rig.couples.len() {
        let Some((driver, couple)) = rig
            .driver_of(&rig.joints[root.driver].name)
            .and_then(|c| Some((index(&c.driver)?, c)))
        else {
            break;
        };
        root.driver = driver;
        root.ratio *= couple.ratio;
        root.efficiency *= couple.efficiency;
        root.back *= couple.back;
    }
    root
}

fn reach(rig: &Rig, base: &[f64], targets: &[(usize, f64)]) -> Option<Vec<f64>> {
    let mut wanted = base.to_vec();
    targets.iter().for_each(|&(i, v)| wanted[i] = v);
    if let Ok((values, _)) = rig.settle(&wanted) {
        return Some(values);
    }
    let mut now = base.to_vec();
    for &(i, v) in targets {
        now = rig.drive(&now, i, v).ok()?.0;
    }
    Some(now)
}

fn per_unit(rig: &Rig, joint: usize) -> f64 {
    match rig.joints[joint].kind {
        JointKind::Turn { .. } => 180.0 / std::f64::consts::PI,
        _ => 1000.0,
    }
}

fn unit(rig: &Rig, joint: usize) -> &'static str {
    match rig.joints[joint].kind {
        JointKind::Turn { .. } => "N·m",
        _ => "N",
    }
}

struct Setup {
    rig: Rig,
    lumps: Vec<Lump>,
    pushes: Vec<(usize, f64)>,
    gravity: Vector3,
    reported: Vec<usize>,
    roots: Vec<Root>,
    aliases: Vec<(String, String)>,
}

impl Setup {
    fn loads(&self, values: &[f64]) -> Vec<f64> {
        let step = 0.01;
        let derivative = |i: usize| -> f64 {
            let at = values[i];
            let side = |v: f64| {
                reach(&self.rig, values, &[(i, v)]).map(|vals| {
                    (
                        v,
                        energy(&self.rig, &self.lumps, &self.pushes, self.gravity, &vals),
                    )
                })
            };
            let here = energy(&self.rig, &self.lumps, &self.pushes, self.gravity, values);
            let one_sided = |direction: f64| -> Option<f64> {
                let (_, near) = side(at + direction * step)?;
                let (_, far) = side(at + direction * 2.0 * step)?;
                Some(direction * (4.0 * near - far - 3.0 * here) / (2.0 * step))
            };
            match (side(at - step), side(at + step)) {
                (Some(a), Some(b)) => (b.1 - a.1) / (b.0 - a.0),
                (Some(_), None) => one_sided(-1.0).unwrap_or(f64::NAN),
                (None, Some(_)) => one_sided(1.0).unwrap_or(f64::NAN),
                (None, None) => f64::NAN,
            }
        };
        let mut raw: std::collections::HashMap<usize, f64> = Default::default();
        self.reported
            .iter()
            .map(|&joint| {
                let Root {
                    driver,
                    ratio,
                    efficiency,
                    drag,
                    ..
                } = self.roots[joint];
                let q = *raw.entry(driver).or_insert_with(|| derivative(driver));
                let load = q / ratio * per_unit(&self.rig, joint) / efficiency;
                let load = if load.abs() < 1.0e-9 { 0.0 } else { load };
                if load < 0.0 { load - drag } else { load + drag }
            })
            .collect()
    }

    fn sweep_values(&self, joint: usize, steps: usize) -> Vec<f64> {
        let j = &self.rig.joints[joint];
        let (low, high) = if j.wraps || !j.range.0.is_finite() || !j.range.1.is_finite() {
            (-180.0, 180.0)
        } else {
            j.range
        };
        (0..=steps)
            .map(|k| low + (high - low) * k as f64 / steps as f64)
            .collect()
    }

    fn describe(&self, values: &[f64], swept: &[usize]) -> String {
        if swept.is_empty() {
            return "as posed".to_string();
        }
        let parts: Vec<String> = swept
            .iter()
            .map(|&i| {
                let j = &self.rig.joints[i];
                format!("{} {}{}", j.name, crate::plot::label(values[i]), j.unit())
            })
            .collect();
        format!("at {}", parts.join(", "))
    }
}

fn setup(spec: &Spec, model: &Model) -> Result<(Setup, Vec<String>)> {
    if !model.assembly || model.joints.iter().all(|j| !j.movable()) {
        bail!("a torque simulation needs an assembly (.gasm) with `turn` or `slide` joints");
    }
    let rig = model.rig();
    let (lumps, notes) = lumps(spec, model)?;
    let mut roots: Vec<Root> = (0..rig.joints.len()).map(|i| root(&rig, i)).collect();
    reflect_drag(&rig, &mut roots);
    let pushes = spec
        .joint_torques
        .iter()
        .map(|(name, value)| {
            let joint = rig
                .joints
                .iter()
                .position(|j| &j.name == name)
                .ok_or_else(|| anyhow::anyhow!("no joint called `{name}`"))?;
            Ok((joint, value / per_unit(&rig, joint)))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut reported: Vec<usize> = Vec::new();
    let mut aliases = Vec::new();
    for i in (0..rig.joints.len()).filter(|&i| rig.joints[i].movable()) {
        let twin = reported.iter().copied().find(|&k| {
            let (a, b) = (roots[k], roots[i]);
            a.driver == b.driver
                && (a.ratio - b.ratio).abs() <= b.ratio.abs() * 1.0e-12
                && a.efficiency == b.efficiency
                && a.back == b.back
                && a.drag == b.drag
                && a.held == b.held
                && unit(&rig, k) == unit(&rig, i)
        });
        match twin {
            Some(k) => aliases.push((rig.joints[i].name.clone(), rig.joints[k].name.clone())),
            None => reported.push(i),
        }
    }
    Ok((
        Setup {
            rig,
            lumps,
            pushes,
            gravity: spec.gravity(),
            reported,
            roots,
            aliases,
        },
        notes,
    ))
}

fn grid(setup: &Setup, joints: &[usize], steps: usize) -> Vec<Vec<(usize, f64)>> {
    joints.iter().fold(vec![Vec::new()], |poses, &joint| {
        let values = setup.sweep_values(joint, steps);
        poses
            .iter()
            .flat_map(|pose| {
                values.iter().map(move |&v| {
                    let mut next = pose.clone();
                    next.push((joint, v));
                    next
                })
            })
            .collect()
    })
}

pub fn plan(spec: &Spec) -> Result<String> {
    let model = spec.target()?;
    let (setup, notes) = setup(spec, model)?;
    let poses: usize = spec
        .sweeps
        .iter()
        .map(|s| (s.steps + 1).pow(s.joints.len() as u32))
        .sum::<usize>()
        .max(1);
    let mut text = format!(
        "{} weighed bodies and payloads, {} joints, {poses} poses",
        setup.lumps.len(),
        setup.reported.len(),
    );
    for note in notes {
        text += &format!("\nnote: {note}");
    }
    Ok(text)
}

pub fn run(spec: &Spec) -> Result<Outcome> {
    let model = spec.target()?;
    let (setup, mut notes) = setup(spec, model)?;
    let base = setup.rig.values();
    let names: Vec<String> = setup
        .reported
        .iter()
        .map(|&j| setup.rig.joints[j].name.clone())
        .collect();
    let mut peaks: Vec<(f64, String)> = setup
        .loads(&base)
        .into_iter()
        .map(|t| (t, setup.describe(&base, &[])))
        .collect();
    let mut series = Vec::new();
    let mut unreachable = 0;
    for sweep in &spec.sweeps {
        let poses = grid(&setup, &sweep.joints, sweep.steps);
        let joints: Vec<String> = sweep
            .joints
            .iter()
            .map(|&j| format!("`{}`", setup.rig.joints[j].name))
            .collect();
        let sweeping = crate::progress::stage(
            format!("sweeping {}", joints.join(" and ")),
            Some(poses.len()),
            "poses",
        );
        let solved: Vec<Option<(Vec<f64>, Vec<f64>)>> = poses
            .par_iter()
            .map(|targets| {
                let found = reach(&setup.rig, &base, targets).map(|values| {
                    let loads = setup.loads(&values);
                    (values, loads)
                });
                sweeping.advance(1.0);
                found
            })
            .collect();
        drop(sweeping);
        let first = sweep.joints[0];
        let xs = setup.sweep_values(first, sweep.steps);
        let mut curves: Vec<Vec<f64>> = vec![vec![f64::NAN; xs.len()]; names.len()];
        for (targets, outcome) in poses.iter().zip(&solved) {
            let Some((values, loads)) = outcome else {
                unreachable += 1;
                continue;
            };
            let column = xs
                .iter()
                .position(|x| (x - targets[0].1).abs() < 1.0e-9)
                .unwrap_or(0);
            for (k, &load) in loads.iter().enumerate() {
                if load.is_nan() {
                    continue;
                }
                if load.abs() > peaks[k].0.abs() || peaks[k].0.is_nan() {
                    peaks[k] = (load, setup.describe(values, &sweep.joints));
                }
                let cell = &mut curves[k][column];
                let shown = if sweep.joints.len() == 1 {
                    load
                } else {
                    load.abs()
                };
                if cell.is_nan() || shown.abs() > cell.abs() {
                    *cell = shown;
                }
            }
        }
        let swept: Vec<&str> = sweep
            .joints
            .iter()
            .map(|&j| setup.rig.joints[j].name.as_str())
            .collect();
        let joint = &setup.rig.joints[first];
        series.push(Series {
            title: format!("sweep {}", swept.join(", ")),
            axis: format!("{} ({})", joint.name, joint.unit().trim()),
            x: xs,
            curves: names
                .iter()
                .zip(&setup.reported)
                .zip(curves)
                .map(|((name, &j), values)| Curve {
                    name: if sweep.joints.len() == 1 {
                        name.clone()
                    } else {
                        format!("|{name}| worst")
                    },
                    unit: unit(&setup.rig, j).to_string(),
                    values,
                })
                .collect(),
        });
    }
    if unreachable > 0 {
        notes.push(format!(
            "{unreachable} pose(s) could not be reached (a joint limit or a mate stops them) and were skipped"
        ));
    }
    let mut same: Vec<(String, Vec<String>)> = Vec::new();
    for (alias, name) in &setup.aliases {
        match same.iter_mut().find(|(n, _)| n == name) {
            Some((_, list)) => list.push(alias.clone()),
            None => same.push((name.clone(), vec![alias.clone()])),
        }
    }
    for (name, list) in &same {
        let verb = if list.len() == 1 { "carries" } else { "carry" };
        notes.push(format!(
            "{} {verb} the same load as {name}",
            list.join(", ")
        ));
    }
    let g = setup.gravity;
    let pushed = if setup.pushes.is_empty() {
        ""
    } else {
        ", joint torques"
    };
    notes.push(format!(
        "holding torque against gravity {},{},{} m/s², payloads{pushed}; the sign of a value is the way the joint must push",
        g.x, g.y, g.z
    ));
    let lossy: Vec<usize> = setup
        .reported
        .iter()
        .copied()
        .filter(|&j| setup.roots[j].lossy())
        .collect();
    if !lossy.is_empty() {
        notes.push(
            "with gear losses and joint drag a joint reads the torque to drive its load, and `.back` the torque the load turns it back with"
                .to_string(),
        );
    }
    let locked: Vec<&str> = lossy
        .iter()
        .filter(|&&j| setup.roots[j].back <= 0.0)
        .map(|&j| setup.rig.joints[j].name.as_str())
        .collect();
    if !locked.is_empty() {
        let (verb, it) = if locked.len() == 1 {
            ("locks", "it")
        } else {
            ("lock", "them")
        };
        notes.push(format!(
            "{} {verb}: the load cannot turn {it} back",
            locked.join(", ")
        ));
    }
    let mut readings = Vec::new();
    for ((name, &j), (value, at)) in names.into_iter().zip(&setup.reported).zip(peaks) {
        let unit = unit(&setup.rig, j).to_string();
        let at = if value < 0.0 {
            format!("{at}, pushing negative")
        } else {
            at
        };
        let root = setup.roots[j];
        let back = root.lossy().then(|| Reading {
            name: format!("{name}.back"),
            value: ((value.abs() - root.drag) * root.efficiency * root.back - root.held).max(0.0),
            unit: unit.clone(),
            at: at.clone(),
        });
        readings.push(Reading {
            name,
            value: value.abs(),
            unit,
            at,
        });
        readings.extend(back);
    }
    Ok(Outcome {
        kind: "torque".to_string(),
        readings,
        notes,
        series,
        checks: Vec::new(),
        aliases: setup.aliases.clone(),
        slices: Vec::new(),
        streams: Vec::new(),
        movie: None,
        field: None,
    })
}
