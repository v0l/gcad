use super::Model;
use super::args::{Args, point3};
use crate::geometry;
use crate::parse::Line;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JointKind {
    Turn { through: Point3, axis: Vector3 },
    Slide { along: Vector3 },
}

#[derive(Clone, Debug)]
pub struct Joint {
    pub name: String,
    pub child: String,
    pub parent: String,
    pub kind: JointKind,
    pub value: f64,
    pub range: (f64, f64),
}

impl Joint {
    pub fn motion(&self, amount: f64) -> Matrix4 {
        match self.kind {
            JointKind::Turn { through, axis } => {
                Matrix4::from_translation(through.to_vec())
                    * Matrix4::from_axis_angle(axis, Deg(amount))
                    * Matrix4::from_translation(-through.to_vec())
            }
            JointKind::Slide { along } => Matrix4::from_translation(along * amount),
        }
    }

    pub fn moved(&self, transform: Matrix4) -> Joint {
        let kind = match self.kind {
            JointKind::Turn { through, axis } => JointKind::Turn {
                through: transform.transform_point(through),
                axis: transform.transform_vector(axis).normalize(),
            },
            JointKind::Slide { along } => JointKind::Slide {
                along: transform.transform_vector(along).normalize(),
            },
        };
        Joint {
            kind,
            ..self.clone()
        }
    }

    pub fn unit(&self) -> &'static str {
        match self.kind {
            JointKind::Turn { .. } => "°",
            JointKind::Slide { .. } => " mm",
        }
    }
}

pub fn subtree(joints: &[Joint], root: &str) -> Vec<String> {
    let mut found = vec![root.to_string()];
    let mut index = 0;
    while index < found.len() {
        let parent = found[index].clone();
        let children: Vec<String> = joints
            .iter()
            .filter(|j| j.parent == parent && !found.contains(&j.child))
            .map(|j| j.child.clone())
            .collect();
        found.extend(children);
        index += 1;
    }
    found
}

pub fn posed(joints: &[Joint], values: &[f64]) -> std::collections::HashMap<String, Matrix4> {
    let mut placed: std::collections::HashMap<String, Matrix4> = Default::default();
    let mut pending: Vec<usize> = (0..joints.len()).collect();
    while !pending.is_empty() {
        let ready = pending
            .iter()
            .position(|&i| !pending.iter().any(|&k| joints[k].child == joints[i].parent))
            .unwrap_or(0);
        let i = pending.remove(ready);
        let joint = &joints[i];
        let above = placed
            .get(&joint.parent)
            .copied()
            .unwrap_or_else(Matrix4::identity);
        let moved = joint.moved(above);
        let delta = moved.motion(values[i] - joint.value);
        placed.insert(joint.child.clone(), delta * above);
    }
    placed
}

impl Model {
    pub(crate) fn body_names(&self) -> Vec<String> {
        self.bodies
            .iter()
            .map(|(name, _)| name.clone())
            .chain(self.solid.as_ref().map(|_| self.current_body()))
            .collect()
    }

    pub(crate) fn current_body(&self) -> String {
        if self.body.is_empty() {
            "main".to_string()
        } else {
            self.body.clone()
        }
    }

    fn move_body(&mut self, name: &str, transform: Matrix4) -> Result<()> {
        let moved = |solid: &Solid| super::solids::oriented(builder::transformed(solid, transform));
        if name == self.current_body() {
            let solid = self
                .solid
                .as_ref()
                .ok_or_else(|| anyhow!("body `{name}` has no solid yet"))?;
            self.solid = Some(moved(solid));
            return Ok(());
        }
        let names = self.body_names();
        let (_, solid) = self
            .bodies
            .iter_mut()
            .find(|(n, _)| n == name)
            .ok_or_else(|| anyhow!("no body called `{name}`; bodies are {names:?}"))?;
        *solid = moved(solid);
        Ok(())
    }

    fn set_joint(&mut self, index: usize, value: f64) -> Result<()> {
        let joint = self.joints[index].clone();
        let (low, high) = joint.range;
        if value < low - 1.0e-9 || value > high + 1.0e-9 {
            bail!(
                "`{}` goes from {low} to {high}{}, not {value}",
                joint.name,
                joint.unit()
            );
        }
        let transform = joint.motion(value - joint.value);
        let moving = subtree(&self.joints, &joint.child);
        for body in &moving {
            self.move_body(body, transform)?;
        }
        for other in self.joints.iter_mut() {
            if moving.contains(&other.parent) {
                *other = other.moved(transform);
            }
        }
        self.joints[index].value = value;
        Ok(())
    }

    pub(crate) fn op_joint(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["name", "child", "parent", "kind"],
            &["about", "along", "min", "max", "at"],
            false,
        )?;
        let name = args.text("name")?.to_string();
        if self.joints.iter().any(|j| j.name == name) {
            bail!("there is already a joint called `{name}`");
        }
        let (child, parent) = (
            args.text("child")?.to_string(),
            args.text("parent")?.to_string(),
        );
        let names = self.body_names();
        for body in [&child, &parent] {
            if !names.contains(body) {
                bail!("no body called `{body}`; bodies are {names:?}");
            }
        }
        if child == parent {
            bail!("a joint joins two different bodies");
        }
        if subtree(&self.joints, &child).contains(&parent) {
            bail!("`{parent}` already hangs off `{child}`; joints must form a tree");
        }
        if self.joints.iter().any(|j| j.child == child) {
            bail!("`{child}` already has a joint to its parent");
        }
        let kind = match args.text("kind")? {
            "turn" => {
                let about = args
                    .values
                    .get("about")
                    .ok_or_else(|| anyhow!("a `turn` joint needs `about=` a datum `axis`"))?;
                let (through, axis) = self.axes.get(*about).copied().ok_or_else(|| {
                    anyhow!("no datum axis `{about}`; make one with `axis {about} x,y,z x,y,z`")
                })?;
                JointKind::Turn { through, axis }
            }
            "slide" => {
                let along = args
                    .values
                    .get("along")
                    .map(|text| point3(text, &self.scope))
                    .transpose()?
                    .ok_or_else(|| anyhow!("a `slide` joint needs `along=x,y,z`"))?
                    .to_vec();
                if along.magnitude() < 1.0e-12 {
                    bail!("`along=` must not be zero");
                }
                JointKind::Slide {
                    along: along.normalize(),
                }
            }
            other => bail!("a joint is `turn` or `slide`, not `{other}`"),
        };
        let (default_low, default_high) = match kind {
            JointKind::Turn { .. } => (-180.0, 180.0),
            JointKind::Slide { .. } => (-100.0, 100.0),
        };
        let low = args
            .optional_number("min", &self.scope)?
            .unwrap_or(default_low);
        let high = args
            .optional_number("max", &self.scope)?
            .unwrap_or(default_high);
        if low > high {
            bail!("min={low} is above max={high}");
        }
        let at = args
            .optional_number("at", &self.scope)?
            .unwrap_or(0.0_f64.clamp(low, high));
        self.joints.push(Joint {
            name: name.clone(),
            child,
            parent,
            kind,
            value: 0.0_f64.clamp(low, high),
            range: (low, high),
        });
        let index = self.joints.len() - 1;
        self.set_joint(index, at)?;
        let joint = &self.joints[index];
        Ok(format!(
            "{name}: `{}` {} on `{}` from {low} to {high}{}, at {at}{}",
            joint.child,
            if matches!(joint.kind, JointKind::Turn { .. }) {
                "turns"
            } else {
                "slides"
            },
            joint.parent,
            joint.unit(),
            joint.unit()
        ))
    }

    pub(crate) fn op_pose(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["joint", "value"], &[], false)?;
        let name = args.text("joint")?;
        let index = self
            .joints
            .iter()
            .position(|j| j.name == name)
            .ok_or_else(|| anyhow!("no joint called `{name}`"))?;
        let value = args.number("value", &self.scope)?;
        self.set_joint(index, value)?;
        Ok(format!("{name} at {value}{}", self.joints[index].unit()))
    }

    fn overlaps(&self) -> Vec<(String, String, f64)> {
        let named: Vec<(String, &Solid)> = self
            .bodies
            .iter()
            .map(|(n, s)| (n.clone(), s))
            .chain(self.solid.as_ref().map(|s| (self.current_body(), s)))
            .collect();
        let boxes: Vec<BoundingBox<Point3>> =
            named.iter().map(|(_, s)| geometry::bounds(s)).collect();
        let mut found = Vec::new();
        for i in 0..named.len() {
            for j in i + 1..named.len() {
                let (a, b) = (&boxes[i], &boxes[j]);
                let apart = (0..3).any(|k| a.max()[k] < b.min()[k] || b.max()[k] < a.min()[k]);
                if apart {
                    continue;
                }
                let shared = geometry::overlap_volume(named[i].1, named[j].1, 96);
                if shared > 1.0e-6 * geometry::volume(named[i].1).abs().max(1.0) {
                    found.push((named[i].0.clone(), named[j].0.clone(), shared));
                }
            }
        }
        found
    }

    pub(crate) fn op_interference(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &["joint", "steps"], true)?;
        let strict = match args.rest.as_slice() {
            [] => false,
            ["none"] => true,
            _ => bail!(
                "`interference` takes `none` to fail on any overlap, and `joint=` `steps=` to sweep a joint"
            ),
        };
        let report = |found: &[(String, String, f64)]| {
            found
                .iter()
                .map(|(a, b, v)| format!("{a} and {b} share about {v:.3}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let Some(name) = args.values.get("joint").copied() else {
            let found = self.overlaps();
            if found.is_empty() {
                return Ok(format!("{} bodies, no overlaps", self.body_names().len()));
            }
            if strict {
                bail!("bodies overlap: {}", report(&found));
            }
            return Ok(report(&found));
        };
        let index = self
            .joints
            .iter()
            .position(|j| j.name == name)
            .ok_or_else(|| anyhow!("no joint called `{name}`"))?;
        let steps = args.optional_number("steps", &self.scope)?.unwrap_or(8.0);
        if steps < 1.0 || steps.fract() != 0.0 {
            bail!("steps must be a whole number of at least 1");
        }
        let (low, high) = self.joints[index].range;
        let mut trial = self.clone();
        let mut hits = Vec::new();
        for k in 0..=steps as usize {
            let value = low + (high - low) * k as f64 / steps;
            trial.set_joint(index, value)?;
            let found = trial.overlaps();
            if !found.is_empty() {
                hits.push(format!(
                    "at {value:.1}{}: {}",
                    self.joints[index].unit(),
                    report(&found)
                ));
            }
        }
        if hits.is_empty() {
            return Ok(format!(
                "{name} sweeps {low} to {high} in {steps} steps with no overlaps"
            ));
        }
        if strict {
            bail!("{}", hits.join("; "));
        }
        Ok(hits.join("; "))
    }
}
