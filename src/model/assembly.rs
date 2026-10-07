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
    Fixed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Feature {
    Axis(Point3, Vector3),
    Plane(Point3, Vector3),
}

impl Feature {
    pub(crate) fn moved(self, transform: Matrix4) -> Feature {
        match self {
            Feature::Axis(p, d) => Feature::Axis(
                transform.transform_point(p),
                transform.transform_vector(d).normalize(),
            ),
            Feature::Plane(p, n) => Feature::Plane(
                transform.transform_point(p),
                transform.transform_vector(n).normalize(),
            ),
        }
    }

    pub(crate) fn point(self) -> Point3 {
        match self {
            Feature::Axis(p, _) | Feature::Plane(p, _) => p,
        }
    }

    pub(crate) fn direction(self) -> Vector3 {
        match self {
            Feature::Axis(_, d) | Feature::Plane(_, d) => d,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Condition {
    Concentric,
    Flush(f64),
    Parallel,
    Angle(f64),
    Distance(f64),
}

#[derive(Clone, Debug)]
pub struct Mate {
    pub text: String,
    pub parts: [String; 2],
    pub features: [Feature; 2],
    pub condition: Condition,
}

pub(crate) fn angle_between(a: Feature, b: Feature) -> f64 {
    let dot = a.direction().dot(b.direction());
    let dot = match (a, b) {
        (Feature::Plane(..), Feature::Plane(..)) => dot,
        _ => dot.abs(),
    };
    dot.clamp(-1.0, 1.0).acos().to_degrees()
}

pub(crate) fn distance_between(a: Feature, b: Feature) -> Option<f64> {
    let tilt = 1.0e-6;
    match (a, b) {
        (Feature::Axis(pa, da), Feature::Axis(pb, db)) => {
            (da.cross(db).magnitude() < tilt).then(|| {
                let d = pb - pa;
                (d - da * d.dot(da)).magnitude()
            })
        }
        (Feature::Plane(pa, na), Feature::Plane(pb, nb)) => {
            (na.cross(nb).magnitude() < tilt).then(|| (pa - pb).dot(nb).abs())
        }
        (Feature::Axis(p, d), Feature::Plane(q, n))
        | (Feature::Plane(q, n), Feature::Axis(p, d)) => {
            (d.dot(n).abs() < tilt).then(|| (p - q).dot(n).abs())
        }
    }
}

impl Mate {
    pub fn residuals(&self, moves: [Matrix4; 2]) -> Vec<f64> {
        let [a, b] = [
            self.features[0].moved(moves[0]),
            self.features[1].moved(moves[1]),
        ];
        let (da, db) = (a.direction(), b.direction());
        let gap = b.point() - a.point();
        let across = |v: Vector3| [v.x, v.y, v.z];
        let along_zero = |d: Vector3, v: Vector3| v - d * v.dot(d);
        match (self.condition, a, b) {
            (Condition::Concentric, ..) => {
                [across(da.cross(db)), across(along_zero(da, gap))].concat()
            }
            (Condition::Flush(offset), ..) => {
                let mut r = across(da + db).to_vec();
                r.push((a.point() - b.point()).dot(db) - offset);
                r
            }
            (Condition::Parallel, Feature::Plane(..), Feature::Plane(..))
            | (Condition::Parallel, Feature::Axis(..), Feature::Axis(..)) => {
                across(da.cross(db)).to_vec()
            }
            (Condition::Parallel, ..) => vec![da.dot(db)],
            (Condition::Angle(want), ..) => vec![(angle_between(a, b) - want).to_radians()],
            (Condition::Distance(want), Feature::Axis(..), Feature::Axis(..)) => {
                let mut r = across(da.cross(db)).to_vec();
                r.push(along_zero(da, gap).magnitude() - want);
                r
            }
            (Condition::Distance(want), Feature::Plane(..), Feature::Plane(..)) => {
                let mut r = across(da.cross(db)).to_vec();
                r.push(gap.dot(db).abs() - want);
                r
            }
            (Condition::Distance(want), ..) => {
                let normal = match a {
                    Feature::Plane(..) => da,
                    Feature::Axis(..) => db,
                };
                vec![da.dot(db), gap.dot(normal).abs() - want]
            }
        }
    }

    pub fn holds(&self, moves: [Matrix4; 2]) -> bool {
        let [a, b] = [
            self.features[0].moved(moves[0]),
            self.features[1].moved(moves[1]),
        ];
        let close = 1.0e-3;
        match (self.condition, a, b) {
            (Condition::Concentric, Feature::Axis(..), Feature::Axis(..)) => {
                distance_between(a, b).is_some_and(|d| d < close)
            }
            (Condition::Flush(offset), Feature::Plane(pa, na), Feature::Plane(pb, nb)) => {
                na.dot(nb) < -1.0 + 1.0e-6 && ((pa - pb).dot(nb) - offset).abs() < close
            }
            (Condition::Parallel, ..) => {
                let degrees = angle_between(a, b);
                match (a, b) {
                    (Feature::Plane(..), Feature::Plane(..))
                    | (Feature::Axis(..), Feature::Axis(..)) => {
                        !(1.0e-3..=180.0 - 1.0e-3).contains(&degrees)
                    }
                    _ => (degrees - 90.0).abs() < 1.0e-3,
                }
            }
            (Condition::Angle(want), ..) => (angle_between(a, b) - want).abs() < 1.0e-3,
            (Condition::Distance(want), ..) => {
                distance_between(a, b).is_some_and(|d| (d - want).abs() < close)
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub file: std::path::PathBuf,
    pub body: String,
    pub vars: Vec<(String, f64)>,
}

#[derive(Clone, Debug)]
pub struct Couple {
    pub driven: String,
    pub driver: String,
    pub ratio: f64,
    pub offset: f64,
}

pub type Settled =
    std::result::Result<(Vec<f64>, std::collections::HashMap<String, Matrix4>), String>;

#[derive(Clone, Debug, Default)]
pub struct Rig {
    pub joints: Vec<Joint>,
    pub mates: Vec<Mate>,
    pub couples: Vec<Couple>,
}

impl Rig {
    pub fn driver_of(&self, joint: &str) -> Option<&Couple> {
        self.couples.iter().find(|c| c.driven == joint)
    }

    pub fn values(&self) -> Vec<f64> {
        self.joints.iter().map(|j| j.value).collect()
    }

    fn coupled(&self, values: &[f64]) -> Vec<f64> {
        let mut values = values.to_vec();
        let index = |name: &str| self.joints.iter().position(|j| j.name == name);
        for _ in 0..self.couples.len() {
            for couple in &self.couples {
                if let (Some(driven), Some(driver)) = (index(&couple.driven), index(&couple.driver))
                {
                    values[driven] = couple.ratio * values[driver] + couple.offset;
                }
            }
        }
        for (value, joint) in values.iter_mut().zip(&self.joints) {
            if joint.wraps {
                *value = (*value + 180.0).rem_euclid(360.0) - 180.0;
            }
        }
        values
    }

    fn drives(&self, joint: usize, target: usize) -> bool {
        let mut chain = self.joints[target].name.clone();
        while let Some(couple) = self.driver_of(&chain) {
            if couple.driver == self.joints[joint].name {
                return true;
            }
            chain = couple.driver.clone();
        }
        false
    }

    pub fn drive(&self, current: &[f64], index: usize, value: f64) -> Settled {
        let mut values = current.to_vec();
        values[index] = value;
        let first = match self.settle(&values) {
            Ok(done) => return Ok(done),
            Err(why) => why,
        };
        let free: Vec<usize> = (0..self.joints.len())
            .filter(|&k| {
                k != index
                    && self.joints[k].movable()
                    && self.driver_of(&self.joints[k].name).is_none()
                    && !self.drives(k, index)
            })
            .collect();
        if free.is_empty() || self.mates.is_empty() {
            return Err(first);
        }
        let step = match self.joints[index].kind {
            JointKind::Turn { .. } => 5.0,
            _ => 2.0,
        };
        let from = current[index];
        let count = ((value - from).abs() / step).ceil().max(1.0) as usize;
        let mut x: Vec<f64> = free.iter().map(|&k| current[k]).collect();
        let place = |x: &[f64], at: f64| {
            let mut values = current.to_vec();
            values[index] = at;
            free.iter().zip(x).for_each(|(&k, &v)| values[k] = v);
            self.coupled(&values)
        };
        for k in 1..=count {
            let at = from + (value - from) * k as f64 / count as f64;
            x = super::constrain::least_squares(&x, 1.0e-6, |x| {
                let moves = posed(&self.joints, &place(x, at));
                let at = |name: &str| moves.get(name).copied().unwrap_or_else(Matrix4::identity);
                self.mates
                    .iter()
                    .flat_map(|m| m.residuals([at(&m.parts[0]), at(&m.parts[1])]))
                    .collect()
            });
            self.settle(&place(&x, at))?;
        }
        self.settle(&place(&x, value))
    }

    pub fn settle(&self, values: &[f64]) -> Settled {
        let values = self.coupled(values);
        for (joint, &value) in self.joints.iter().zip(&values) {
            let (low, high) = joint.range;
            if value < low - 1.0e-9 || value > high + 1.0e-9 {
                return Err(format!(
                    "`{}` goes from {low} to {high}{}, not {value:.3}",
                    joint.name,
                    joint.unit()
                ));
            }
        }
        let moves = posed(&self.joints, &values);
        let held = broken(&self.mates, &moves);
        if !held.is_empty() {
            return Err(format!("it would pull apart {}", held.join(", ")));
        }
        Ok((values, moves))
    }
}

pub fn explode_offsets(
    joints: &[Joint],
    explode: &[(String, Vector3)],
) -> std::collections::HashMap<String, Vector3> {
    let mut offsets: std::collections::HashMap<String, Vector3> = Default::default();
    for (part, by) in explode {
        for body in subtree(joints, part) {
            *offsets.entry(body).or_insert_with(Vector3::zero) += *by;
        }
    }
    offsets
}

pub fn broken(mates: &[Mate], moves: &std::collections::HashMap<String, Matrix4>) -> Vec<String> {
    let at = |name: &str| moves.get(name).copied().unwrap_or_else(Matrix4::identity);
    mates
        .iter()
        .filter(|m| !m.holds([at(&m.parts[0]), at(&m.parts[1])]))
        .map(|m| m.text.clone())
        .collect()
}

#[derive(Clone, Debug)]
pub struct Joint {
    pub name: String,
    pub child: String,
    pub parent: String,
    pub kind: JointKind,
    pub value: f64,
    pub range: (f64, f64),
    pub wraps: bool,
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
            JointKind::Fixed => Matrix4::identity(),
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
            JointKind::Fixed => JointKind::Fixed,
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
            JointKind::Fixed => "",
        }
    }

    pub fn movable(&self) -> bool {
        self.kind != JointKind::Fixed
    }
}

impl Model {
    pub(crate) fn parent_of(&self, part: &str) -> Option<String> {
        self.joints
            .iter()
            .find(|j| j.child == part)
            .map(|j| j.parent.clone())
    }

    pub(crate) fn mate_parts(
        &mut self,
        text: &str,
        moving: &str,
        fixed: &str,
        features: [Feature; 2],
        condition: Condition,
        transform: Matrix4,
    ) -> Result<String> {
        if moving == fixed {
            bail!("a mate joins two different parts");
        }
        let mate = Mate {
            text: text.to_string(),
            parts: [moving.to_string(), fixed.to_string()],
            features,
            condition,
        };
        match self.parent_of(moving) {
            Some(parent) if parent != fixed => {
                if !mate.holds([Matrix4::identity(), Matrix4::identity()]) {
                    bail!(
                        "`{moving}` already hangs off `{parent}` and does not meet `{fixed}` this way; place it with its first mate and check the rest hold"
                    );
                }
                self.mates.push(mate);
                Ok(format!(
                    "holds; `{moving}` hangs off `{parent}`, so this ties it to `{fixed}` too"
                ))
            }
            parent => {
                if parent.is_none() && subtree(&self.joints, moving).iter().any(|p| p == fixed) {
                    bail!("`{fixed}` already hangs off `{moving}`");
                }
                let moved = self.move_subtree(moving, transform)?;
                self.mates.push(Mate {
                    features: [mate.features[0].moved(transform), mate.features[1]],
                    ..mate
                });
                if parent.is_none() {
                    self.joints.push(Joint {
                        name: format!("{moving} on {fixed}"),
                        child: moving.to_string(),
                        parent: fixed.to_string(),
                        kind: JointKind::Fixed,
                        value: 0.0,
                        range: (0.0, 0.0),
                        wraps: false,
                    });
                }
                let held = broken(&self.mates, &std::collections::HashMap::new());
                if !held.is_empty() {
                    bail!("placing `{moving}` pulls apart {}", held.join(", "));
                }
                Ok(format!("moved {}, now held by `{fixed}`", moved.join(", ")))
            }
        }
    }
}

fn commute(a: JointKind, b: JointKind) -> bool {
    let along = |through: Point3, axis: Vector3, other: JointKind| match other {
        JointKind::Turn {
            through: p,
            axis: d,
        } => {
            let gap = p - through;
            axis.cross(d).magnitude() < 1.0e-9 && (gap - axis * gap.dot(axis)).magnitude() < 1.0e-9
        }
        JointKind::Slide { along } => axis.cross(along).magnitude() < 1.0e-9,
        JointKind::Fixed => true,
    };
    match (a, b) {
        (JointKind::Turn { through, axis }, other) | (other, JointKind::Turn { through, axis }) => {
            along(through, axis, other)
        }
        _ => true,
    }
}

pub const ASSEMBLY_OPERATIONS: &[&str] = &[
    "let",
    "if",
    "include",
    "part",
    "move",
    "rotate",
    "axis",
    "joint",
    "couple",
    "pose",
    "explode",
    "interference",
    "color",
    "material",
    "measure",
    "concentric",
    "flush",
    "parallel",
    "angle",
    "distance",
    "tangent",
    "aligned",
];

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
        let delta = joint.moved(above).motion(values[i] - joint.value);
        let mine = placed.get(&joint.child).copied().unwrap_or(above);
        placed.insert(joint.child.clone(), delta * mine);
    }
    placed
}

impl Model {
    pub fn body_names(&self) -> Vec<String> {
        self.bodies
            .iter()
            .map(|(name, _)| name.clone())
            .chain(self.solid.as_ref().map(|_| self.current_body()))
            .collect()
    }

    pub fn current_body(&self) -> String {
        if self.body.is_empty() {
            "main".to_string()
        } else {
            self.body.clone()
        }
    }

    fn move_body(&mut self, name: &str, transform: Matrix4) -> Result<()> {
        let moved = |solid: &Solid| super::solids::oriented(builder::transformed(solid, transform));
        if name == self.current_body() && self.solid.is_some() {
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
        let placed = self
            .placements
            .get(name)
            .copied()
            .unwrap_or_else(Matrix4::identity);
        self.placements.insert(name.to_string(), transform * placed);
        for (part, by) in self.explode.iter_mut() {
            if part == name {
                *by = transform.transform_vector(*by);
            }
        }
        for mate in self.mates.iter_mut() {
            for k in 0..2 {
                if mate.parts[k] == name {
                    mate.features[k] = mate.features[k].moved(transform);
                }
            }
        }
        if let Some(groups) = self.part_groups.get_mut(name) {
            groups.0.iter_mut().for_each(|entry| {
                entry.surface = super::bodies::moved_surface(&entry.surface, transform)
            });
        }
        Ok(())
    }

    fn move_subtree(&mut self, root: &str, transform: Matrix4) -> Result<Vec<String>> {
        let moving = subtree(&self.joints, root);
        for body in &moving {
            self.move_body(body, transform)?;
        }
        for other in self.joints.iter_mut() {
            if moving.contains(&other.parent) {
                *other = other.moved(transform);
            }
        }
        Ok(moving)
    }

    pub(crate) fn apply_assembly(&mut self, line: &Line) -> Result<String> {
        match line.op.as_str() {
            "let" => self.op_let(line),
            "if" | "include" => self.apply_shared(line),
            "part" => self.op_part(line),
            "move" => self.op_move_part(line),
            "rotate" => self.op_rotate_part(line),
            "axis" => self.op_axis(line),
            "joint" => self.op_joint(line),
            "pose" => self.op_pose(line),
            "couple" => self.op_couple(line),
            "explode" => self.op_explode(line),
            "interference" => self.op_interference(line),
            "color" => self.op_colour_part(line),
            "material" => self.op_material(line),
            "measure" => self.op_measure(line),
            "concentric" => self.op_concentric(line),
            "flush" => self.op_flush(line),
            "parallel" | "angle" | "distance" | "tangent" => self.op_orient(line),
            "aligned" => self.op_aligned(line),
            other if super::OPERATIONS.contains(&other) => bail!(
                "`{other}` makes geometry, which belongs in a part (.lcad) file; bring the part in with `part name file.lcad`"
            ),
            other => bail!(
                "unknown assembly operation `{other}`; assemblies use {}",
                ASSEMBLY_OPERATIONS.join(", ")
            ),
        }
    }

    fn op_part(&mut self, line: &Line) -> Result<String> {
        let [name, file] = line.positional.as_slice() else {
            bail!("write `part name file.lcad [body=b] [variable=value ...]`");
        };
        if name.contains('.') {
            bail!("part names cannot contain `.`");
        }
        if self.depth >= 16 {
            bail!("parts are nested more than 16 deep");
        }
        let path = self.relative(file);
        let mut only = None;
        let mut vars = Vec::new();
        for (key, text) in &line.named {
            if key == "body" {
                only = Some(text.clone());
            } else {
                vars.push((key.clone(), crate::parse::eval(text, &self.scope)?));
            }
        }
        let loaded = super::load_model(&path, &vars, self.depth + 1)?;
        let names = loaded.body_names();
        let picked: Vec<String> = match &only {
            Some(body) if !names.contains(body) => {
                bail!("{} has no body `{body}`; it has {names:?}", path.display())
            }
            Some(body) => vec![body.clone()],
            None => names.clone(),
        };
        if picked.is_empty() {
            bail!("{} builds no solid", path.display());
        }
        let rename = |body: &str| {
            if picked.len() == 1 {
                name.clone()
            } else {
                format!("{name}.{body}")
            }
        };
        let taken = self.body_names();
        let mut added = Vec::new();
        for body in &picked {
            let new = rename(body);
            if taken.contains(&new) {
                bail!("there is already a part called `{new}`");
            }
            let solid = loaded.named_body(body)?;
            if let Some(colour) = loaded.colours.get(body) {
                self.colours.insert(new.clone(), *colour);
            }
            let source = loaded.sources.get(body).cloned().unwrap_or_else(|| Source {
                file: path.clone(),
                body: body.clone(),
                vars: vars.clone(),
            });
            self.sources.insert(new.clone(), source);
            if let Some(placed) = loaded.placements.get(body) {
                self.placements.insert(new.clone(), *placed);
            }
            if let Some(material) = loaded.materials.get(body) {
                self.materials.insert(new.clone(), material.clone());
            }
            let groups = loaded
                .part_groups
                .get(body)
                .cloned()
                .unwrap_or_else(|| loaded.groups.clone());
            self.part_groups.insert(new.clone(), groups);
            self.bodies.push((new.clone(), solid));
            added.push(new);
        }
        for joint in &loaded.joints {
            if picked.contains(&joint.child) && picked.contains(&joint.parent) {
                self.joints.push(Joint {
                    name: format!("{name}.{}", joint.name),
                    child: rename(&joint.child),
                    parent: rename(&joint.parent),
                    ..joint.clone()
                });
            }
        }
        for couple in &loaded.couples {
            let ours = |joint: &str| {
                loaded.joints.iter().any(|j| {
                    j.name == joint && picked.contains(&j.child) && picked.contains(&j.parent)
                })
            };
            if ours(&couple.driven) && ours(&couple.driver) {
                self.couples.push(Couple {
                    driven: format!("{name}.{}", couple.driven),
                    driver: format!("{name}.{}", couple.driver),
                    ..couple.clone()
                });
            }
        }
        for (part, by) in &loaded.explode {
            if picked.contains(part) {
                self.explode.push((rename(part), *by));
            }
        }
        for mate in &loaded.mates {
            if mate.parts.iter().all(|p| picked.contains(p)) {
                self.mates.push(Mate {
                    text: format!("{name}: {}", mate.text),
                    parts: mate.parts.clone().map(|p| rename(&p)),
                    ..mate.clone()
                });
            }
        }
        Ok(format!("{} from {}", added.join(", "), path.display()))
    }

    fn part_transform(&mut self, line: &Line, transform: Matrix4) -> Result<String> {
        let name = line
            .positional
            .first()
            .ok_or_else(|| anyhow!("name the part to move"))?;
        if !self.body_names().contains(name) {
            bail!("no part called `{name}`; parts are {:?}", self.body_names());
        }
        let moved = self.move_subtree(name, transform)?;
        Ok(format!("moved {}", moved.join(", ")))
    }

    fn op_explode(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["part", "by"], &[], false)?;
        let name = args.text("part")?.to_string();
        if !self.body_names().contains(&name) {
            bail!("no part called `{name}`; parts are {:?}", self.body_names());
        }
        let by = point3(args.text("by")?, &self.scope)?.to_vec();
        self.explode.push((name.clone(), by));
        let moving = subtree(&self.joints, &name);
        Ok(format!(
            "exploded views move {} by {:.3},{:.3},{:.3}",
            moving.join(", "),
            by.x,
            by.y,
            by.z
        ))
    }

    fn op_move_part(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["part", "by"], &[], false)?;
        let by = point3(args.text("by")?, &self.scope)?;
        self.part_transform(line, Matrix4::from_translation(by.to_vec()))
    }

    fn op_rotate_part(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["part", "angle"], &["axis", "about"], false)?;
        let angle = args.number("angle", &self.scope)?;
        let named = args
            .values
            .get("axis")
            .and_then(|name| self.axes.get(*name))
            .copied();
        let (through, direction) = match (named, args.values.get("about")) {
            (Some(_), Some(_)) => {
                bail!("a datum axis already passes through a point; drop `about=`")
            }
            (Some(axis), None) => axis,
            (None, about) => (
                about
                    .map(|text| point3(text, &self.scope))
                    .transpose()?
                    .unwrap_or_else(Point3::origin),
                super::args::axis(args.values.get("axis").copied().unwrap_or("z"))?,
            ),
        };
        let transform = Matrix4::from_translation(through.to_vec())
            * Matrix4::from_axis_angle(direction, Deg(angle))
            * Matrix4::from_translation(-through.to_vec());
        self.part_transform(line, transform)
    }

    fn op_colour_part(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["part", "colour"], &[], false)?;
        let name = args.text("part")?.to_string();
        if !self.body_names().contains(&name) {
            bail!("no part called `{name}`; parts are {:?}", self.body_names());
        }
        let colour = self.colour_of(args.text("colour")?)?;
        self.colours.insert(name.clone(), colour);
        Ok(format!(
            "`{name}` is {:.2},{:.2},{:.2}",
            colour[0], colour[1], colour[2]
        ))
    }

    pub fn rig(&self) -> Rig {
        Rig {
            joints: self.joints.clone(),
            mates: self.mates.clone(),
            couples: self.couples.clone(),
        }
    }

    fn set_joint(&mut self, index: usize, value: f64) -> Result<()> {
        let joint = self.joints[index].clone();
        let rig = self.rig();
        if let Some(couple) = rig.driver_of(&joint.name) {
            bail!(
                "`{}` is driven by `{}`; move that instead",
                joint.name,
                couple.driver
            );
        }
        let (values, _) = rig.drive(&rig.values(), index, value).map_err(|why| {
            anyhow!(
                "`{}` cannot move to {value}{}: {why}",
                joint.name,
                joint.unit()
            )
        })?;
        for (k, &target) in values.iter().enumerate() {
            let joint = self.joints[k].clone();
            if target != joint.value {
                self.move_subtree(&joint.child, joint.motion(target - joint.value))?;
                self.joints[k].value = target;
            }
        }
        Ok(())
    }

    pub(crate) fn op_couple(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["driven", "driver"], &["ratio"], false)?;
        let (driven, driver) = (args.text("driven")?, args.text("driver")?);
        let find = |name: &str| {
            self.joints
                .iter()
                .find(|j| j.name == name && j.movable())
                .cloned()
                .ok_or_else(|| anyhow!("no joint called `{name}` to couple"))
        };
        let (follower, leader) = (find(driven)?, find(driver)?);
        if driven == driver {
            bail!("a joint cannot drive itself");
        }
        if let Some(couple) = self.couples.iter().find(|c| c.driven == driven) {
            bail!("`{driven}` is already driven by `{}`", couple.driver);
        }
        let mut chain = driver.to_string();
        while let Some(couple) = self.couples.iter().find(|c| c.driven == chain) {
            if couple.driver == driven {
                bail!("`{driven}` already drives `{driver}`");
            }
            chain = couple.driver.clone();
        }
        let ratio = args.optional_number("ratio", &self.scope)?.unwrap_or(1.0);
        self.couples.push(Couple {
            driven: driven.to_string(),
            driver: driver.to_string(),
            ratio,
            offset: follower.value - ratio * leader.value,
        });
        Ok(format!(
            "`{driven}` moves {ratio}{} for each {} `{driver}` moves",
            follower.unit(),
            leader.unit().trim()
        ))
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
        let siblings: Vec<Joint> = self
            .joints
            .iter()
            .filter(|j| j.child == child)
            .cloned()
            .collect();
        if let Some(joint) = siblings.iter().find(|j| !j.movable()) {
            bail!(
                "`{child}` is already held by `{}`; give a part its joints before mating it",
                joint.name
            );
        }
        if let Some(joint) = siblings.iter().find(|j| j.parent != parent) {
            bail!(
                "`{child}` already moves on `{}` through `{}`; all of a part's joints go to one parent",
                joint.parent,
                joint.name
            );
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
        if let Some(other) = siblings.iter().find(|j| !commute(j.kind, kind)) {
            bail!(
                "`{child}` already moves through `{}`, and the two motions would depend on their order; a part's joints can be slides, and turns about one axis with slides along it",
                other.name
            );
        }
        let wraps = matches!(kind, JointKind::Turn { .. }) && !args.has("min") && !args.has("max");
        let (default_low, default_high) = match kind {
            JointKind::Turn { .. } => (-180.0, 180.0),
            _ => (-100.0, 100.0),
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
            wraps,
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
        let before: Vec<f64> = self.joints.iter().map(|j| j.value).collect();
        self.set_joint(index, value)?;
        let followed: Vec<String> = self
            .joints
            .iter()
            .zip(&before)
            .enumerate()
            .filter(|(k, (joint, old))| *k != index && (joint.value - *old).abs() > 1.0e-9)
            .map(|(_, (joint, _))| format!("{} {:.3}{}", joint.name, joint.value, joint.unit()))
            .collect();
        let unit = self.joints[index].unit();
        Ok(if followed.is_empty() {
            format!("{name} at {value}{unit}")
        } else {
            format!("{name} at {value}{unit}; {} follow", followed.join(", "))
        })
    }

    fn overlaps(&self) -> Vec<(String, String, f64)> {
        let named: Vec<(String, &Solid)> = self
            .bodies
            .iter()
            .map(|(n, s)| (n.clone(), s))
            .chain(self.solid.as_ref().map(|s| (self.current_body(), s)))
            .collect();
        let meshes: Vec<geometry::Meshed> = named
            .iter()
            .map(|(_, s)| geometry::Meshed::new(s))
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
                let shared = geometry::overlap_of(&meshes[i], &meshes[j], 96);
                if shared > 1.0e-6 * meshes[i].volume().abs().max(1.0) {
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
