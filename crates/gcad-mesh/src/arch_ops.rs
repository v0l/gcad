use crate::arch::roof;
use crate::arch::stair::{StairRule, check, flight_profile, steps};
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
    "roof", "stair",
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
}

#[derive(Default)]
pub struct ArchOps {
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

    pub fn apply(&mut self, scope: &mut Scope, line: &Line) -> Result<()> {
        match line.op.as_str() {
            "units" => match line.positional.as_slice() {
                [unit] if unit == "mm" && line.named.is_empty() => Ok(()),
                _ => bail!("only `units mm` is supported; lengths are millimetres"),
            },
            "building" => Ok(()),
            "level" => self.level(scope, line),
            "grid" => self.grid(scope, line),
            "wall_type" => self.wall_type(scope, line),
            "wall" => self.wall(scope, line),
            "door" | "window" | "opening" => self.opening(scope, line),
            "slab" => self.slab(scope, line),
            "roof" => self.roof(scope, line),
            "stair" => self.stair(scope, line),
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
        };
        for (key, text) in &line.named {
            match key.as_str() {
                "on" => o.host = text.clone(),
                "at" => o.at = number(text, scope, key)?,
                "w" => o.w = number(text, scope, key)?,
                "h" => o.h = number(text, scope, key)?,
                "sill" => o.sill = number(text, scope, key)?,
                "mark" | "swing" | "hinge" | "frame" | "leaf" | "panes" | "mullion" | "glazing"
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
        self.openings.push(o);
        Ok(())
    }

    fn free(&self, name: &str) -> Result<()> {
        if self.walls.iter().any(|w| w.name == name) || self.done.iter().any(|(n, _)| n == name) {
            bail!("there is already something called `{name}`");
        }
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
                "shape" if text == "straight" => {}
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
        let profile = flight_profile(&s, s.treads(), 0.0, STAIR_WAIST);
        let (sin, cos) = dir.to_radians().sin_cos();
        let mut mesh = Mesh::default();
        let part = mesh.part_index(&material);
        extrude(&mut mesh, part, &profile, &[], (0.0, width), &|p, u| {
            [at.0 + cos * p.0 - sin * u, at.1 + sin * p.0 + cos * u, from + p.1]
        });
        self.done.push((name, mesh));
        Ok(())
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
        let mut built: Vec<(String, Mesh)> = (0..self.walls.len())
            .map(|i| (self.walls[i].name.clone(), self.wall_mesh(i, &lines)))
            .collect();
        built.extend(self.done);
        for (name, mesh) in built {
            for part in &mesh.parts {
                scene
                    .part_colours
                    .insert((name.clone(), part.clone()), material_colour(part));
            }
            scene.solid_bodies.push(name.clone());
            scene.mesh_bodies.push((name, Arc::new(mesh)));
        }
        Ok(())
    }
}
