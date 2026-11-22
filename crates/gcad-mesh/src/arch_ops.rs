use crate::arch::roof;
use crate::arch::stair::{StairRule, check, flight_profile, path_length, point_along, post_stations, split_treads, steps};
use crate::arch::layout::{dot, signed_area, sub};
use crate::arch::{Chain, Justify, Layer, P2, WallLine, bands, end_cut};
use crate::interp::Scene;
use crate::mesh::{Mesh, P3};
use crate::parse::{Line, Scope, eval, eval_point};
use crate::solid::triangulate;
use anyhow::{Context, Result, anyhow, bail};
use std::sync::Arc;

pub const OPS: &[&str] = &[
    "units", "building", "grid", "wall_type", "wall", "door", "window", "opening", "slab",
    "roof", "stair", "railing", "column",
];

const STAIR_WAIST: f64 = 150.0;

pub fn handles(line: &Line) -> bool {
    match line.op.as_str() {
        "level" => line.positional.len() >= 2,
        other => OPS.contains(&other),
    }
}

pub fn material_colour(material: &str) -> [f64; 3] {
    match material {
        "brick" | "clay-tile" => [0.62, 0.3, 0.22],
        "pir" => [0.9, 0.85, 0.5],
        "block" | "concrete" => [0.62, 0.62, 0.6],
        "plaster" => [0.9, 0.88, 0.82],
        "wood" | "oak" | "timber" => [0.55, 0.38, 0.22],
        "glass" => [0.6, 0.8, 0.9],
        "steel" => [0.6, 0.62, 0.66],
        "slate" => [0.3, 0.32, 0.36],
        _ => [0.7, 0.7, 0.7],
    }
}

#[derive(Clone, Debug)]
struct Wall {
    name: String,
    chain: Chain,
    layers: Vec<Layer>,
    justify: Justify,
    offset: f64,
    base: f64,
    top: f64,
}

impl Wall {
    fn thickness(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness).sum()
    }

    fn span(&self) -> (f64, f64) {
        let lo = self.justify.start(self.thickness()) + self.offset;
        (lo, lo + self.thickness())
    }
}

#[derive(Clone, Debug)]
struct Opening {
    name: String,
    host: String,
    at: f64,
    w: f64,
    h: f64,
    sill: f64,
    kind: String,
    hinge_right: bool,
}

#[derive(Default)]
pub struct ArchOps {
    current: String,
    owners: Vec<(String, String)>,
    repeats: Vec<(String, [f64; 3])>,
    openings: Vec<Opening>,
    grids: Vec<(String, bool, f64)>,
    types: Vec<(String, Vec<Layer>)>,
    walls: Vec<Wall>,
    done: Vec<(String, Mesh)>,
}

fn push(mesh: &mut Mesh, part: u16, p: P3) -> u32 {
    mesh.positions.push(p);
    mesh.labels.push(part);
    (mesh.positions.len() - 1) as u32
}

fn orient(mesh: &mut Mesh, first: usize) {
    let volume: f64 = mesh.triangles[first..]
        .iter()
        .map(|&t| {
            let [a, b, c] = mesh.corners(t);
            crate::mesh::dot(a, crate::mesh::cross(b, c))
        })
        .sum();
    if volume < 0.0 {
        for t in &mut mesh.triangles[first..] {
            t.swap(1, 2);
        }
    }
}

pub fn extrude(
    mesh: &mut Mesh,
    part: u16,
    outer: &[P2],
    holes: &[Vec<P2>],
    (d0, d1): (f64, f64),
    place: &dyn Fn(P2, f64) -> P3,
) {
    let first = mesh.triangles.len();
    let mut outer = outer.to_vec();
    if signed_area(&outer) < 0.0 {
        outer.reverse();
    }
    let holes: Vec<Vec<P2>> = holes
        .iter()
        .map(|h| {
            let mut h = h.clone();
            if signed_area(&h) > 0.0 {
                h.reverse();
            }
            h
        })
        .collect();
    let (points, triangles) = triangulate(&outer, &holes);
    for (d, up) in [(d0, false), (d1, true)] {
        let base = mesh.positions.len() as u32;
        for p in &points {
            push(mesh, part, place(*p, d));
        }
        for t in &triangles {
            let t = t.map(|i| i + base);
            mesh.triangles.push(if up { t } else { [t[0], t[2], t[1]] });
        }
    }
    let loops = std::iter::once(&outer).chain(holes.iter());
    for ring in loops {
        let n = ring.len();
        for k in 0..n {
            let (a, b) = (ring[k], ring[(k + 1) % n]);
            let ids = [
                push(mesh, part, place(a, d0)),
                push(mesh, part, place(b, d0)),
                push(mesh, part, place(b, d1)),
                push(mesh, part, place(a, d1)),
            ];
            mesh.triangles.push([ids[0], ids[1], ids[2]]);
            mesh.triangles.push([ids[0], ids[2], ids[3]]);
        }
    }
    orient(mesh, first);
}

const ROUND_SIDES: usize = 48;

fn round_ring(diameter: f64) -> Vec<P2> {
    let n = ROUND_SIDES as f64;
    let r = diameter / 2.0 * (std::f64::consts::TAU / (n * (std::f64::consts::TAU / n).sin())).sqrt();
    (0..ROUND_SIDES)
        .map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n;
            (r * a.cos(), r * a.sin())
        })
        .collect()
}

fn member(mesh: &mut Mesh, part: u16, ring: &[P2], from: P3, to: P3) {
    use crate::mesh::{cross, normalized, scale, sub};
    let d = sub(to, from);
    let len = crate::mesh::length(d);
    let along = normalized(d);
    let up = [0.0, 0.0, 1.0];
    let lift = sub(up, scale(along, crate::mesh::dot(up, along)));
    let y = normalized(lift);
    let x = cross(y, along);
    extrude(mesh, part, ring, &[], (0.0, len), &|p, t| {
        std::array::from_fn(|k| from[k] + along[k] * t + x[k] * p.0 + y[k] * p.1)
    });
}

pub fn column_box(mesh: &mut Mesh, part: u16, ring: &[P2], z0: f64, z1: f64) {
    extrude(mesh, part, ring, &[], (z0, z1), &|p, z| [p.0, p.1, z]);
}

fn number(text: &str, scope: &Scope, key: &str) -> Result<f64> {
    eval(text, scope).with_context(|| format!("`{key}={text}`"))
}

impl ArchOps {
    fn plan_point(&self, word: &str, scope: &Scope) -> Result<P2> {
        if !word.contains(',') && !scope.contains_key(&format!("{word}.x")) {
            bail!("no grid crossing or point `{word}`");
        }
        eval_point(word, scope)
    }

    pub fn handles(&self, line: &Line) -> bool {
        match line.op.as_str() {
            "level" => line.positional.len() >= 2,
            "repeat" | "array" => line
                .named
                .iter()
                .any(|(k, v)| k == "of" && self.owners.iter().any(|(_, b)| b == v)),
            other => OPS.contains(&other),
        }
    }

    pub fn apply(&mut self, scope: &mut Scope, line: &Line) -> Result<()> {
        match line.op.as_str() {
            "units" => match line.positional.as_slice() {
                [unit] if unit == "mm" && line.named.is_empty() => Ok(()),
                _ => bail!("only `units mm` is supported; lengths are millimetres"),
            },
            "building" => {
                if let [name] = line.positional.as_slice() {
                    self.current = name.clone();
                }
                Ok(())
            }
            "repeat" | "array" => self.repeat(scope, line),
            "level" => self.level(scope, line),
            "grid" => self.grid(scope, line),
            "wall_type" => self.wall_type(scope, line),
            "wall" => self.wall(scope, line),
            "door" | "window" | "opening" => self.opening(scope, line),
            "slab" => self.slab(scope, line),
            "roof" => self.roof(scope, line),
            "stair" => self.stair(scope, line),
            "railing" => self.railing(scope, line),
            "column" => self.column(scope, line),
            other => bail!("`{other}` is not a building operation"),
        }
    }

    fn level(&mut self, scope: &mut Scope, line: &Line) -> Result<()> {
        let (name, height) = match line.positional.as_slice() {
            [name, height] | [name, height, _] => (name, height),
            _ => bail!("write `level L1 2700 \"First\"`"),
        };
        let z = eval(height, scope).with_context(|| format!("height of `{name}`"))?;
        scope.insert(name.clone(), z);
        Ok(())
    }

    fn grid(&mut self, scope: &mut Scope, line: &Line) -> Result<()> {
        let [name] = line.positional.as_slice() else {
            bail!("write `grid A x=0` or `grid 1 y=0`");
        };
        let (axis, text) = match line.named.as_slice() {
            [(axis, text)] if axis == "x" || axis == "y" => (axis.as_str(), text),
            _ => bail!("a grid line takes one of x= or y="),
        };
        let offset = eval(text, scope)?;
        let along_x = axis == "x";
        for (other, other_x, other_offset) in self.grids.iter().filter(|g| g.1 != along_x) {
            let (x, y) = if along_x {
                ((name, offset), (other, *other_offset))
            } else {
                ((other, *other_offset), (name, offset))
            };
            let crossing = format!("{}{}", x.0, y.0);
            scope.insert(format!("{crossing}.x"), x.1);
            scope.insert(format!("{crossing}.y"), y.1);
            let _ = other_x;
        }
        self.grids.push((name.clone(), along_x, offset));
        Ok(())
    }

    fn layers(&self, scope: &Scope, words: &[&str]) -> Result<Vec<Layer>> {
        if words.is_empty() {
            bail!("a wall type needs layers like `brick:100 block:100`");
        }
        words
            .iter()
            .map(|word| {
                let (material, thickness) = word
                    .split_once(':')
                    .ok_or_else(|| anyhow!("`{word}` should be material:thickness"))?;
                let thickness = eval(thickness, scope)?;
                if !(thickness > 0.0 && thickness.is_finite()) {
                    bail!("`{word}`: a layer thickness must be above zero");
                }
                Ok(Layer {
                    material: material.to_string(),
                    thickness,
                })
            })
            .collect()
    }

    fn wall_type(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let [name, layers @ ..] = line.positional.as_slice() else {
            bail!("write `wall_type ext brick:102 block:100`");
        };
        let words: Vec<&str> = layers.iter().map(String::as_str).collect();
        let layers = self.layers(scope, &words)?;
        self.types.retain(|(n, _)| n != name);
        self.types.push((name.clone(), layers));
        Ok(())
    }

    fn wall(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let name = line
            .label
            .clone()
            .ok_or_else(|| anyhow!("a wall needs a label, like `w1: wall 0,0 4000,0 type=ext`"))?;
        if self.walls.iter().any(|w| w.name == name) {
            bail!("there is already a wall `{name}`");
        }
        self.owners.push((name.clone(), self.current.clone()));
        let mut words: Vec<&str> = line.positional.iter().map(String::as_str).collect();
        let closed = words.contains(&"closed");
        words.retain(|w| *w != "closed");
        if words.first() == Some(&"arc") || words.first() == Some(&"profile") {
            bail!("gcad-mesh walls follow straight points");
        }
        let points = words
            .iter()
            .map(|p| self.plan_point(p, scope))
            .collect::<Result<Vec<_>>>()?;
        let chain = Chain::new(points, closed).map_err(|e| anyhow!(e))?;
        let mut wall = Wall {
            name: name.clone(),
            chain,
            layers: Vec::new(),
            justify: Justify::Centre,
            offset: 0.0,
            base: 0.0,
            top: f64::NAN,
        };
        for (key, text) in &line.named {
            match key.as_str() {
                "type" => {
                    wall.layers = self
                        .types
                        .iter()
                        .find(|(n, _)| n == text)
                        .map(|(_, l)| l.clone())
                        .ok_or_else(|| anyhow!("no wall type `{text}`"))?
                }
                "base" => wall.base = number(text, scope, key)?,
                "top" => wall.top = number(text, scope, key)?,
                "offset" => wall.offset = number(text, scope, key)?,
                "justify" => {
                    wall.justify = Justify::parse(text)
                        .ok_or_else(|| anyhow!("justify is outside, centre or inside"))?
                }
                other => bail!(
                    "gcad-mesh `wall` takes type=, base=, top=, justify= and offset=, not `{other}=`"
                ),
            }
        }
        if wall.top.is_nan() {
            bail!("a wall needs `top=`, a height or a level like `top=L1`");
        }
        if wall.layers.is_empty() {
            bail!("a wall needs `type=`, a `wall_type` name");
        }
        if wall.top <= wall.base {
            bail!("the wall's top is not above its base");
        }
        self.walls.push(wall);
        Ok(())
    }

    fn opening(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let op = line.op.as_str();
        let name = line
            .label
            .clone()
            .ok_or_else(|| anyhow!("`{op}` needs a label, like `d1: door on=w1 at=2000 w=900 h=2100`"))?;
        let mut o = Opening {
            name,
            host: String::new(),
            at: f64::NAN,
            w: f64::NAN,
            h: f64::NAN,
            sill: if op == "window" { 900.0 } else { 0.0 },
            kind: op.to_string(),
            hinge_right: false,
        };
        for (key, text) in &line.named {
            match key.as_str() {
                "on" => o.host = text.clone(),
                "at" => o.at = number(text, scope, key)?,
                "w" => o.w = number(text, scope, key)?,
                "h" => o.h = number(text, scope, key)?,
                "sill" => o.sill = number(text, scope, key)?,
                "hinge" => match text.as_str() {
                    "left" => o.hinge_right = false,
                    "right" => o.hinge_right = true,
                    other => bail!("hinge is left or right, not `{other}`"),
                },
                "mark" | "swing" | "frame" | "leaf" | "panes" | "mullion" | "glazing"
                | "depth" | "threshold" | "head" => {}
                other => bail!("`{op}` has no `{other}=` argument"),
            }
        }
        for (value, what) in [(o.at, "at"), (o.w, "w"), (o.h, "h")] {
            if value.is_nan() {
                bail!("`{op}` needs `{what}=`");
            }
        }
        let wall = self
            .walls
            .iter()
            .find(|w| w.name == o.host)
            .ok_or_else(|| anyhow!("`{}` is on `{}`, which is not a wall", o.name, o.host))?;
        let starts = wall.chain.starts();
        let (i, _, _) = wall.chain.locate(o.at);
        let (s0, s1) = (o.at - o.w / 2.0, o.at + o.w / 2.0);
        if o.w <= 0.0 || o.h <= 0.0 {
            bail!("an opening needs w and h above zero");
        }
        if s0 < starts[i] - 1.0e-6 || s1 > starts[i + 1] + 1.0e-6 {
            bail!("`{}` must sit on one straight run of `{}`", o.name, wall.name);
        }
        if o.sill < -1.0e-6 || o.sill + o.h > wall.top - wall.base + 1.0e-6 {
            bail!("`{}` reaches above the top of `{}`", o.name, wall.name);
        }
        self.owners.push((o.name.clone(), self.current.clone()));
        self.openings.push(o);
        Ok(())
    }

    fn repeat(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let mut of = None;
        let (mut count, mut step) = (f64::NAN, None);
        for (key, text) in &line.named {
            match key.as_str() {
                "of" => of = Some(text.clone()),
                "count" => count = number(text, scope, key)?,
                "step" => step = Some(eval_point(text, scope)?),
                other => bail!("a building `repeat` takes of=, count= and step=, not `{other}=`"),
            }
        }
        let of = of.ok_or_else(|| anyhow!("`repeat` needs `of=`"))?;
        if count < 2.0 || count.fract() != 0.0 {
            bail!("count must be a whole number of at least 2, got {count}");
        }
        let (sx, sy) = step.ok_or_else(|| anyhow!("`repeat` needs `step=x,y`"))?;
        if self.repeats.iter().all(|(b, _)| *b != of) {
            self.repeats.push((of.clone(), [0.0; 3]));
        }
        for k in 1..count as usize {
            self.repeats.push((of.clone(), [sx * k as f64, sy * k as f64, 0.0]));
        }
        Ok(())
    }

    fn free(&mut self, name: &str) -> Result<()> {
        if self.walls.iter().any(|w| w.name == name) || self.done.iter().any(|(n, _)| n == name) {
            bail!("there is already something called `{name}`");
        }
        self.owners.push((name.to_string(), self.current.clone()));
        Ok(())
    }

    fn slab(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let name = line
            .label
            .clone()
            .ok_or_else(|| anyhow!("a slab needs a label, like `s1: slab L1 A1 B1 B2 A2 type=concrete:200`"))?;
        self.free(&name)?;
        let [level, points @ ..] = line.positional.as_slice() else {
            bail!("write `s1: slab L1 A1 B1 B2 A2 type=concrete:200`");
        };
        let top = eval(level, scope).with_context(|| format!("the slab level `{level}`"))?;
        let ring = points
            .iter()
            .map(|p| self.plan_point(p, scope))
            .collect::<Result<Vec<_>>>()?;
        if ring.len() < 3 || signed_area(&ring).abs() < 1.0e-6 {
            bail!("an outline needs three or more points around an area");
        }
        let mut layers = None;
        for (key, text) in &line.named {
            match key.as_str() {
                "type" => {
                    let words: Vec<&str> = text.split('+').collect();
                    layers = Some(self.layers(scope, &words)?)
                }
                other => bail!("gcad-mesh `slab` takes type=, not `{other}=`"),
            }
        }
        let layers = layers.ok_or_else(|| anyhow!("a slab needs `type=concrete:200`"))?;
        let total: f64 = layers.iter().map(|l| l.thickness).sum();
        let mut mesh = Mesh::default();
        let mut at = top - total;
        for layer in &layers {
            let part = mesh.part_index(&layer.material);
            column_box(&mut mesh, part, &ring, at, at + layer.thickness);
            at += layer.thickness;
        }
        self.done.push((name, mesh));
        Ok(())
    }

    fn roof(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let name = line
            .label
            .clone()
            .ok_or_else(|| anyhow!("a roof needs a label"))?;
        self.free(&name)?;
        let outline = match line.positional.as_slice() {
            [host] if self.walls.iter().any(|w| w.name == *host) => {
                let wall = self.walls.iter().find(|w| w.name == *host).expect("a wall");
                if !wall.chain.closed {
                    bail!("`{host}` is not a closed loop of walls");
                }
                let (lo, hi) = wall.span();
                let ring = |d: f64| -> Vec<P2> {
                    wall.chain.offset(d, None, None).into_iter().map(|(p, _)| p).collect()
                };
                let (a, b) = (ring(lo), ring(hi));
                if signed_area(&a).abs() > signed_area(&b).abs() { a } else { b }
            }
            [points @ .., last] if last == "closed" => points
                .iter()
                .map(|p| self.plan_point(p, scope))
                .collect::<Result<Vec<_>>>()?,
            _ => bail!("write `r1: roof outer type=gable pitch=45` or `r1: roof x,y x,y x,y closed ...`"),
        };
        let outline = roof::ccw_ring(outline);
        let (mut kind, mut pitch, mut overhang, mut thickness, mut base) =
            (String::new(), f64::NAN, 0.0, 200.0, f64::NAN);
        let mut material = "clay-tile".to_string();
        let mut ridge = None;
        for (key, text) in &line.named {
            match key.as_str() {
                "type" => kind = text.clone(),
                "pitch" => pitch = number(text, scope, key)?,
                "overhang" => overhang = number(text, scope, key)?,
                "thickness" => thickness = number(text, scope, key)?,
                "base" => base = number(text, scope, key)?,
                "material" => material = text.clone(),
                "ridge" => {
                    ridge = Some(match text.as_str() {
                        "x" => (1.0, 0.0),
                        "y" => (0.0, 1.0),
                        other => bail!("ridge is x or y, not `{other}`"),
                    })
                }
                other => bail!("gcad-mesh `roof` takes type=, pitch=, overhang=, thickness=, base=, ridge= and material=, not `{other}=`"),
            }
        }
        if !roof::ROOF_KINDS.contains(&kind.as_str()) {
            bail!("a roof needs `type=`, one of {}", roof::ROOF_KINDS.join(", "));
        }
        if base.is_nan() {
            bail!("a roof needs `base=`, the level of its eaves");
        }
        if kind == "flat" && pitch.is_nan() {
            pitch = 0.0;
        } else if !(pitch > 0.0 && pitch < 90.0) {
            bail!("a {kind} roof needs `pitch=` above 0 and under 90");
        }
        if !(thickness > 0.0) || overhang < 0.0 {
            bail!("a roof's thickness must be above zero and its overhang not below zero");
        }
        let ridge = ridge.unwrap_or_else(|| roof::longest_edge(&outline));
        let planes = roof::roof_planes(&kind, &outline, base, &vec![pitch; outline.len()], ridge)
            .map_err(|e| anyhow!(e))?;
        let eaves = roof::offset_ring(&outline, overhang);
        let shell = roof::holed_shell(&eaves, &planes, thickness, &[]).map_err(|e| anyhow!(e))?;
        let mut mesh = Mesh::default();
        let part = mesh.part_index(&material);
        let ids: Vec<u32> = shell.points.iter().map(|&p| push(&mut mesh, part, p)).collect();
        let first = mesh.triangles.len();
        for face in &shell.faces {
            for k in 1..face.len() - 1 {
                mesh.triangles.push([ids[face[0]], ids[face[k]], ids[face[k + 1]]]);
            }
        }
        orient(&mut mesh, first);
        self.done.push((name, mesh));
        Ok(())
    }

    fn stair(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let name = line
            .label
            .clone()
            .ok_or_else(|| anyhow!("a stair needs a label"))?;
        self.free(&name)?;
        let (mut from, mut to, mut at, mut dir, mut width) = (f64::NAN, f64::NAN, None, 0.0, 900.0);
        let mut rule = StairRule::default();
        let (mut risers, mut tread) = (None, None);
        let mut material = "oak".to_string();
        let mut shape = "straight".to_string();
        for (key, text) in &line.named {
            match key.as_str() {
                "from" => from = number(text, scope, key)?,
                "to" => to = number(text, scope, key)?,
                "at" => at = Some(eval_point(text, scope)?),
                "dir" => dir = number(text, scope, key)?,
                "width" => width = number(text, scope, key)?,
                "max_rise" => rule.max_rise = number(text, scope, key)?,
                "min_tread" => rule.min_tread = number(text, scope, key)?,
                "risers" => risers = Some(number(text, scope, key)? as usize),
                "tread" if text != "auto" => tread = Some(number(text, scope, key)?),
                "tread" => {}
                "material" => material = text.clone(),
                "shape" if text == "straight" || text == "L" => shape = text.clone(),
                other => bail!("gcad-mesh `stair` takes from=, to=, at=, dir=, width=, risers=, tread=, max_rise=, min_tread= and material= for a straight flight, not `{other}=`"),
            }
        }
        if from.is_nan() || to.is_nan() {
            bail!("a stair needs `from=` and `to=`");
        }
        let at = at.ok_or_else(|| anyhow!("a stair needs `at=x,y`"))?;
        if width <= 0.0 {
            bail!("a stair width must be above zero");
        }
        let s = steps(to - from, &rule, risers, tread).map_err(|e| anyhow!(e))?;
        check(&s, &rule).map_err(|e| anyhow!(e))?;
        let total = s.treads();
        let counts = match shape.as_str() {
            "straight" => vec![total],
            _ => split_treads(total, 1, 1).map_err(|e| anyhow!(e))?,
        };
        let (sin, cos) = dir.to_radians().sin_cos();
        let forward = (cos, sin);
        let sideways = (-sin, cos);
        let plan = |start: P2, d: P2, s: f64, u: f64| -> P2 {
            let side = (-d.1, d.0);
            (start.0 + d.0 * s + side.0 * u, start.1 + d.1 * s + side.1 * u)
        };
        let _ = sideways;
        let mut mesh = Mesh::default();
        let part = mesh.part_index(&material);
        let mut flight = |start: P2, d: P2, z0: f64, treads: usize, mesh: &mut Mesh| {
            let profile = flight_profile(&s, treads, 0.0, STAIR_WAIST);
            extrude(mesh, part, &profile, &[], (0.0, width), &|p, u| {
                let q = plan(start, d, p.0, u);
                [q.0, q.1, z0 + p.1]
            });
        };
        flight(at, forward, from, counts[0], &mut mesh);
        if counts.len() == 2 {
            let g = counts[0] as f64 * s.tread;
            let corner: Vec<P2> = [(g, 0.0), (g + width, 0.0), (g + width, width), (g, width)]
                .iter()
                .map(|&(a, b)| plan(at, forward, a, b))
                .collect();
            let top = from + (counts[0] + 1) as f64 * s.rise;
            column_box(&mut mesh, part, &corner, top - (STAIR_WAIST + s.rise / 2.0), top);
            let turned = (-forward.1, forward.0);
            flight(corner[2], turned, top, counts[1], &mut mesh);
        }
        self.done.push((name, mesh));
        Ok(())
    }

    fn railing(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let name = line
            .label
            .clone()
            .ok_or_else(|| anyhow!("a railing needs a label, like `rl1: railing path 0,0,0 3000,0,0 height=900`"))?;
        self.free(&name)?;
        let (mut height, mut post, mut top) = (900.0, Some((50.0, 1500.0)), Some(("round".to_string(), 40.0)));
        let (mut glass, mut top_given) = (None, false);
        let mut material = "steel".to_string();
        for (key, text) in &line.named {
            match key.as_str() {
                "height" => height = number(text, scope, key)?,
                "post" if text == "none" => post = None,
                "post" => {
                    let (size, spacing) = text
                        .split_once('@')
                        .ok_or_else(|| anyhow!("`post=` is a size and spacing like `50@1500`, or none"))?;
                    post = Some((eval(size, scope)?, eval(spacing, scope)?));
                }
                "top" => {
                    top_given = true;
                    top = if text == "none" {
                        None
                    } else {
                        let (kind, size) = text
                            .split_once(':')
                            .filter(|(k, _)| *k == "round" || *k == "rect")
                            .ok_or_else(|| anyhow!("`top=` is round:D, rect:S or none"))?;
                        Some((kind.to_string(), eval(size, scope)?))
                    }
                }
                "glass" => glass = Some(number(text, scope, key)?),
                "material" => material = text.clone(),
                other => bail!("gcad-mesh `railing` takes height=, post=, top=, glass= and material= on a path, not `{other}=`"),
            }
        }
        if glass.is_some() && !top_given {
            top = None;
        }
        if height <= 0.0 || post.is_some_and(|(s, g)| s <= 0.0 || g <= 0.0) {
            bail!("a railing's height, post size and spacing must be above zero");
        }
        let points = match line.positional.as_slice() {
            [kind, points @ ..] if kind == "path" => points
                .iter()
                .map(|p| crate::args::point3(p, scope))
                .collect::<Result<Vec<_>>>()?,
            _ => bail!("write `rl1: railing path x,y,z x,y,z ... height=900`"),
        };
        if points.len() < 2 || path_length(&points) < 1.0e-6 {
            bail!("a railing path needs two or more different points");
        }
        let top_size = top.as_ref().map_or(0.0, |t| t.1);
        let total = path_length(&points);
        let mut mesh = Mesh::default();
        if let Some((size, spacing)) = post {
            let part = mesh.part_index("post");
            let _ = &material;
            let h = size / 2.0;
            let ring = [(-h, -h), (h, -h), (h, h), (-h, h)];
            for s in post_stations(total, spacing) {
                let p = point_along(&points, s);
                column_box(&mut mesh, part, &ring.map(|(a, b)| (a + p[0], b + p[1])), p[2], p[2] + height - top_size);
            }
        }
        if let Some(t) = glass {
            let part = mesh.part_index("glass");
            for w in points.windows(2) {
                let run = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
                if run < 1.0e-6 {
                    continue;
                }
                let d = ((w[1][0] - w[0][0]) / run, (w[1][1] - w[0][1]) / run);
                let rise = w[1][2] - w[0][2];
                let profile = [(0.0, 0.0), (run, rise), (run, rise + height - top_size), (0.0, height - top_size)];
                extrude(&mut mesh, part, &profile, &[], (-t / 2.0, t / 2.0), &|p, u| {
                    [w[0][0] + d.0 * p.0 - d.1 * u, w[0][1] + d.1 * p.0 + d.0 * u, w[0][2] + p.1]
                });
            }
        }
        if let Some((kind, size)) = &top {
            let part = mesh.part_index(&material);
            let radius = size / 2.0;
            let ring = if kind == "round" { round_ring(*size) } else { vec![(-radius, -radius), (radius, -radius), (radius, radius), (-radius, radius)] };
            for w in points.windows(2) {
                let run = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
                let cos = if run < 1.0e-9 { 1.0 } else { run / path_length(w) };
                if path_length(w) < 1.0e-6 {
                    continue;
                }
                let lift = height - radius * cos;
                member(&mut mesh, part, &ring, [w[0][0], w[0][1], w[0][2] + lift], [w[1][0], w[1][1], w[1][2] + lift]);
            }
        }
        self.done.push((name, mesh));
        Ok(())
    }

    fn column(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let name = line
            .label
            .clone()
            .ok_or_else(|| anyhow!("a column needs a label, like `c1: column A1 rect 300x300 top=L1`"))?;
        self.free(&name)?;
        let [at, kind, size] = line.positional.as_slice() else {
            bail!("write `c1: column A1 rect 300x300 base=L0 top=L1`");
        };
        let at = self.plan_point(at, scope)?;
        let (mut base, mut top) = (0.0, f64::NAN);
        let mut material = "concrete".to_string();
        for (key, text) in &line.named {
            match key.as_str() {
                "base" => base = number(text, scope, key)?,
                "top" => top = number(text, scope, key)?,
                "material" => material = text.clone(),
                other => bail!("`column` takes base=, top= and material=, not `{other}=`"),
            }
        }
        if top.is_nan() || top <= base {
            bail!("a column needs `top=` above its base");
        }
        let ring: Vec<P2> = match kind.as_str() {
            "rect" | "bar" => {
                let (w, h) = size
                    .split_once('x')
                    .ok_or_else(|| anyhow!("write a rect column as `rect 300x300`"))?;
                let (w, h) = (eval(w, scope)? / 2.0, eval(h, scope)? / 2.0);
                vec![(-w, -h), (w, -h), (w, h), (-w, h)]
            }
            "round" | "rod" => round_ring(eval(size, scope)?),
            other => bail!("gcad-mesh columns are rect or round, not `{other}`"),
        };
        let ring: Vec<P2> = ring.into_iter().map(|p| (p.0 + at.0, p.1 + at.1)).collect();
        let mut mesh = Mesh::default();
        let part = mesh.part_index(&material);
        column_box(&mut mesh, part, &ring, base, top);
        self.done.push((name, mesh));
        Ok(())
    }

    fn fills(&self, wall: &Wall) -> Vec<(String, Mesh, Option<P3>)> {
        let starts = wall.chain.starts();
        let (lo, hi) = wall.span();
        let mut out = Vec::new();
        for o in self.openings.iter().filter(|o| o.host == wall.name) {
            if o.kind == "opening" {
                continue;
            }
            let (i, _, dir) = wall.chain.locate(o.at);
            let origin = wall.chain.points[i];
            let n = (-dir.1, dir.0);
            let place = |p: P2, d: f64| -> P3 {
                let s = p.0 - starts[i];
                [
                    origin.0 + dir.0 * s + n.0 * d,
                    origin.1 + dir.1 * s + n.1 * d,
                    wall.base + p.1,
                ]
            };
            let (s0, s1) = (o.at - o.w / 2.0, o.at + o.w / 2.0);
            let (z0, z1) = (o.sill, o.sill + o.h);
            let rect = |a: f64, b: f64, c: f64, d: f64| [(a, c), (b, c), (b, d), (a, d)];
            let make = |ring: &[P2], holes: &[Vec<P2>], depth: (f64, f64), material: &str| {
                let mut mesh = Mesh::default();
                let part = mesh.part_index(material);
                extrude(&mut mesh, part, ring, holes, depth, &place);
                mesh
            };
            if o.kind == "door" {
                let (fw, fd, leaf) = (70.0, 45.0, 44.0);
                let (b0, b1) = (hi - fd, hi);
                let frame = [
                    (s0, z0), (s0 + fw, z0), (s0 + fw, z1 - fw), (s1 - fw, z1 - fw),
                    (s1 - fw, z0), (s1, z0), (s1, z1), (s0, z1),
                ];
                out.push((format!("{}.frame", o.name), make(&frame, &[], (b0, b1), "wood"), None));
                let mid = (b0 + b1) / 2.0;
                let hinge_s = if o.hinge_right { s1 - fw } else { s0 + fw };
                out.push((
                    format!("{}.leaf", o.name),
                    make(&rect(s0 + fw, s1 - fw, z0, z1 - fw), &[], (mid - leaf / 2.0, mid + leaf / 2.0), "wood"),
                    Some(place((hinge_s, z0), mid)),
                ));
            } else {
                let fw = 60.0;
                let mid = (lo + hi) / 2.0;
                let outer = rect(s0, s1, z0, z1);
                let inner = rect(s0 + fw, s1 - fw, z0 + fw, z1 - fw);
                out.push((
                    format!("{}.frame", o.name),
                    make(&outer, &[inner.to_vec()], (mid - 35.0, mid + 35.0), "wood"),
                    None,
                ));
                out.push((
                    format!("{}.glass", o.name),
                    make(&inner, &[], (mid - 12.0, mid + 12.0), "glass"),
                    None,
                ));
            }
        }
        out
    }

    fn wall_lines(&self) -> Vec<WallLine> {
        self.walls
            .iter()
            .map(|w| {
                let (lo, hi) = w.span();
                WallLine {
                    chain: w.chain.clone(),
                    lo,
                    hi,
                }
            })
            .collect()
    }

    fn wall_mesh(&self, index: usize, lines: &[WallLine]) -> Mesh {
        let wall = &self.walls[index];
        let cuts = (
            end_cut(lines, index, false, &[]),
            end_cut(lines, index, true, &[]),
        );
        let starts = wall.chain.starts();
        let mut mesh = Mesh::default();
        for (layer, (lo, hi)) in wall.layers.iter().zip(bands(&wall.layers, wall.justify, wall.offset)) {
            let part = mesh.part_index(&layer.material);
            let right = wall.chain.offset(lo, cuts.0, cuts.1);
            let leftside = wall.chain.offset(hi, cuts.0, cuts.1);
            for i in 0..wall.chain.segments() {
                let j = (i + 1) % right.len();
                let dir = wall.chain.dir(i);
                let origin = wall.chain.points[i];
                let on = |edge: &[(P2, f64)], s: f64| -> P2 {
                    let (a, b) = (edge[i].0, edge[j].0);
                    let (sa, sb) = (
                        dot(sub(a, origin), dir) + starts[i],
                        dot(sub(b, origin), dir) + starts[i],
                    );
                    if s <= sa + 1.0e-9 {
                        a
                    } else if s >= sb - 1.0e-9 {
                        b
                    } else {
                        (a.0 + dir.0 * (s - sa), a.1 + dir.1 * (s - sa))
                    }
                };
                let mut gaps: Vec<&Opening> = self
                    .openings
                    .iter()
                    .filter(|o| o.host == wall.name && wall.chain.locate(o.at).0 == i)
                    .collect();
                gaps.sort_by(|a, b| a.at.total_cmp(&b.at));
                let mut boxes: Vec<(f64, f64, f64, f64)> = Vec::new();
                let mut cursor = f64::NEG_INFINITY;
                for o in gaps {
                    let (s0, s1) = (o.at - o.w / 2.0, o.at + o.w / 2.0);
                    boxes.push((cursor, s0, wall.base, wall.top));
                    if o.sill > 1.0e-6 {
                        boxes.push((s0, s1, wall.base, wall.base + o.sill));
                    }
                    if o.sill + o.h < wall.top - wall.base - 1.0e-6 {
                        boxes.push((s0, s1, wall.base + o.sill + o.h, wall.top));
                    }
                    cursor = s1;
                }
                boxes.push((cursor, f64::INFINITY, wall.base, wall.top));
                for (s0, s1, z0, z1) in boxes {
                    if s1 - s0 < 1.0e-6 {
                        continue;
                    }
                    let ring = [on(&right, s0), on(&right, s1), on(&leftside, s1), on(&leftside, s0)];
                    extrude(&mut mesh, part, &ring, &[], (z0, z1), &|p, z| [p.0, p.1, z]);
                }
            }
        }
        mesh
    }

    pub fn finish(self, scene: &mut Scene) -> Result<()> {
        let lines = self.wall_lines();
        let mut built: Vec<(String, Mesh, Option<P3>)> = (0..self.walls.len())
            .map(|i| (self.walls[i].name.clone(), self.wall_mesh(i, &lines), None))
            .collect();
        for wall in &self.walls {
            built.extend(self.fills(wall));
        }
        built.extend(self.done.into_iter().map(|(n, m)| (n, m, None)));
        for (name, mesh, pivot) in built {
            let owner = |n: &str| {
                let base = n.split('.').next().unwrap_or(n);
                self.owners.iter().find(|(o, _)| o == base).map(|(_, b)| b.clone())
            };
            let group = self
                .repeats
                .iter()
                .find(|(b, _)| Some(b) == owner(&name).as_ref())
                .map(|(b, _)| b.clone());
            if let Some(group) = group {
                scene.groups.push((name.clone(), group));
            }
            if let Some(p) = pivot {
                scene.pivots.push((name.clone(), p));
            }
            for part in &mesh.parts {
                scene
                    .part_colours
                    .insert((name.clone(), part.clone()), material_colour(part));
            }
            scene.solid_bodies.push(name.clone());
            scene.mesh_bodies.push((name, Arc::new(mesh)));
        }
        for (group, at) in &self.repeats {
            scene.instances.push((group.clone(), *at));
        }
        Ok(())
    }
}
