use crate::arch::layout::{dot, left, signed_area, sub};
use crate::arch::{Chain, Justify, Layer, P2, WallLine, bands, end_cut};
use crate::interp::Scene;
use crate::mesh::{Mesh, P3};
use crate::parse::{Line, Scope, eval, eval_point};
use crate::solid::triangulate;
use anyhow::{Context, Result, anyhow, bail};
use std::sync::Arc;

pub const OPS: &[&str] = &["units", "building", "grid", "wall_type", "wall"];

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

#[derive(Default)]
pub struct ArchOps {
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
                let along = |p: P2| dot(sub(p, origin), dir) + starts[i];
                let _ = along;
                let ring = [right[i].0, right[j].0, leftside[j].0, leftside[i].0];
                extrude(&mut mesh, part, &ring, &[], (wall.base, wall.top), &|p, z| {
                    [p.0, p.1, z]
                });
            }
        }
        let _ = left((1.0, 0.0));
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
