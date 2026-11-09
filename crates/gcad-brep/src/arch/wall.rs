use super::{Built, Head, Opening, Sweep, Tier, Wall};
use crate::Model;
use crate::geometry::Profile;
use crate::import::Polygon;
use crate::parse::{Line, eval};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::{Chain, Cut, Join, Justify, Layer, P2, Pier, WallLine};
use monstertruck::modeling::*;

pub(crate) const ARC_PIECES: usize = 64;

pub(crate) fn loft(bottom: &[Vec<Point3>], top: &[Vec<Point3>]) -> Result<Solid> {
    let mut faces: Polygon = Vec::new();
    let mut polygons: Vec<Polygon> = Vec::new();
    for (b, t) in bottom.iter().zip(top) {
        let n = b.len();
        for k in 0..n {
            let j = (k + 1) % n;
            polygons.push(vec![vec![b[k], b[j], t[j], t[k]]]);
        }
        faces.push(t.clone());
    }
    polygons.push(faces);
    polygons.push(
        bottom
            .iter()
            .map(|ring| ring.iter().rev().copied().collect())
            .collect(),
    );
    let solid = planar_solid(&polygons)?;
    Ok(crate::solids::oriented(solid))
}

pub(crate) fn planar_solid(polygons: &[Polygon]) -> Result<Solid> {
    let mut points: Vec<Point3> = Vec::new();
    let mut index: std::collections::HashMap<[i64; 3], usize> = Default::default();
    let mut vertex_of = |p: Point3| -> usize {
        let key = [p.x, p.y, p.z].map(|c| (c * 1.0e6).round() as i64);
        *index.entry(key).or_insert_with(|| {
            points.push(p);
            points.len() - 1
        })
    };
    let rings: Vec<Vec<Vec<usize>>> = polygons
        .iter()
        .map(|face| {
            face.iter()
                .map(|ring| {
                    let mut ids: Vec<usize> = ring.iter().map(|p| vertex_of(*p)).collect();
                    ids.dedup();
                    while ids.len() > 1 && ids.first() == ids.last() {
                        ids.pop();
                    }
                    ids
                })
                .filter(|ids| ids.len() >= 3)
                .collect()
        })
        .collect();
    let vertices: Vec<Vertex> = points.iter().map(|p| Vertex::new(*p)).collect();
    let mut edges: std::collections::HashMap<(usize, usize), Edge> = Default::default();
    let mut edge = |a: usize, b: usize| -> Edge {
        if let Some(e) = edges.get(&(b, a)) {
            return e.inverse();
        }
        edges
            .entry((a, b))
            .or_insert_with(|| {
                Edge::new(
                    &vertices[a],
                    &vertices[b],
                    Curve::Line(Line(points[a], points[b])),
                )
            })
            .clone()
    };
    let mut faces = Vec::new();
    for face in rings.iter().filter(|f| !f.is_empty()) {
        let outer: Vec<Point3> = face[0].iter().map(|&i| points[i]).collect();
        let normal = (0..outer.len())
            .fold(Vector3::zero(), |n, k| {
                n + outer[k]
                    .to_vec()
                    .cross(outer[(k + 1) % outer.len()].to_vec())
            })
            .normalize();
        let origin = outer[0];
        let seed = if normal.x.abs() < 0.9 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        };
        let u = (seed - normal * seed.dot(normal)).normalize();
        let v = normal.cross(u);
        let wires: Vec<Wire> = face
            .iter()
            .map(|ids| {
                (0..ids.len())
                    .map(|k| edge(ids[k], ids[(k + 1) % ids.len()]))
                    .collect()
            })
            .collect();
        let plane = Plane::new(origin, origin + u, origin + v);
        faces.push(
            Face::try_new(wires, Surface::Plane(plane)).map_err(|e| anyhow!("a wall face: {e}"))?,
        );
    }
    Solid::try_new(vec![faces.into()]).map_err(|e| anyhow!("a wall piece does not close: {e}"))
}

pub(crate) fn ccw(mut ring: Vec<P2>) -> Vec<P2> {
    if gcad_mesh::arch::layout::signed_area(&ring) < 0.0 {
        ring.reverse();
    }
    ring
}

pub(crate) fn outline(profile: &Profile) -> Result<Vec<P2>> {
    let points = match profile {
        Profile::Circle { center, diameter } => (0..ARC_PIECES)
            .map(|k| {
                let t = std::f64::consts::TAU * k as f64 / ARC_PIECES as f64;
                (
                    center.0 + diameter / 2.0 * t.cos(),
                    center.1 + diameter / 2.0 * t.sin(),
                )
            })
            .collect(),
        other => {
            let (start, segments) = other
                .as_path()
                .ok_or_else(|| anyhow!("walls follow rect, poly, circle and pen paths"))?;
            let mut points = vec![start];
            for segment in segments {
                let from = *points.last().expect("a start");
                match segment {
                    crate::geometry::Segment::Line(to) => points.push(to),
                    crate::geometry::Segment::Arc { to, via } => {
                        let arc = gcad_mesh::arch::arc_points(from, via, to, ARC_PIECES / 4)
                            .ok_or_else(|| anyhow!("an arc in the profile is straight"))?;
                        points.extend(arc.into_iter().skip(1));
                    }
                    crate::geometry::Segment::Cubic { .. } => {
                        bail!("walls do not follow curves; use lines and arcs")
                    }
                }
            }
            points
        }
    };
    let mut points: Vec<P2> = points;
    if points.len() > 1 {
        let (a, b) = (points[0], points[points.len() - 1]);
        if (a.0 - b.0).hypot(a.1 - b.1) < 1.0e-9 {
            points.pop();
        }
    }
    Ok(ccw(points))
}

pub(crate) fn piece_of(name: &str, used: &[String]) -> String {
    let mut candidate = name.to_string();
    let mut k = 2;
    while used.contains(&candidate) {
        candidate = format!("{name}.{k}");
        k += 1;
    }
    candidate
}

impl Wall {
    pub(crate) fn thickness(&self) -> f64 {
        self.tiers
            .iter()
            .map(|t| t.layers.iter().map(|l| l.thickness).sum::<f64>())
            .fold(0.0, f64::max)
    }

    pub(crate) fn span(&self, tier: &Tier) -> (f64, f64) {
        let total: f64 = tier.layers.iter().map(|l| l.thickness).sum();
        let lo = self.justify.start(total) + self.offset;
        (lo, lo + total)
    }

    pub(crate) fn outer_span(&self) -> (f64, f64) {
        let tier = self
            .tiers
            .iter()
            .max_by(|a, b| {
                let t = |t: &Tier| t.layers.iter().map(|l| l.thickness).sum::<f64>();
                t(a).total_cmp(&t(b))
            })
            .expect("a wall has a tier");
        self.span(tier)
    }

    pub(crate) fn top_at(&self, s: f64) -> f64 {
        let length = self.line.length();
        let (a, b) = self.top_offset;
        self.top + a + (b - a) * (s / length).clamp(0.0, 1.0)
    }

    pub(crate) fn layer_names(&self) -> Vec<(String, usize, usize)> {
        let mut used = Vec::new();
        let mut out = Vec::new();
        for (t, tier) in self.tiers.iter().enumerate() {
            for (l, layer) in tier.layers.iter().enumerate() {
                if layer.material == "air" {
                    continue;
                }
                let name = piece_of(&format!("{}.{}", self.name, layer.material), &used);
                used.push(name.clone());
                out.push((name, t, l));
            }
        }
        out
    }

    fn layer_solid(
        &self,
        tier: &Tier,
        band: (f64, f64),
        cuts: (Option<Cut>, Option<Cut>),
    ) -> Result<Solid> {
        let piers: Vec<Pier> = if band.1 >= self.span(tier).1 - 1.0e-9 {
            self.pilasters
                .iter()
                .map(|&(width, depth, at)| Pier { at, width, depth })
                .collect()
        } else {
            Vec::new()
        };
        let lean = self.batter.to_radians().tan();
        let ring = |lo: f64, hi: f64, z: &dyn Fn(f64) -> f64, shift: f64| {
            let (outer, hole) = self
                .line
                .band(lo + shift, hi + shift, cuts.0, cuts.1, &piers);
            let lift = |ring: Vec<(P2, f64)>| -> Vec<Point3> {
                ring.into_iter()
                    .map(|((x, y), s)| Point3::new(x, y, z(s)))
                    .collect()
            };
            let mut rings = vec![lift(outer)];
            rings.extend(hole.map(|h| {
                let mut h = lift(h);
                h.reverse();
                h
            }));
            rings
        };
        let (z0, z1) = (self.base + tier.from, self.base + tier.to);
        let bottom = ring(band.0, band.1, &|_| z0, z0 * 0.0 + (z0 - self.base) * lean);
        let topped = tier.to >= self.top - self.base - 1.0e-9;
        let top_z = |s: f64| if topped { self.top_at(s) } else { z1 };
        let top = ring(band.0, band.1, &top_z, (z1 - self.base) * lean);
        let fix = |rings: Vec<Vec<Point3>>| -> Vec<Vec<Point3>> {
            rings
                .into_iter()
                .enumerate()
                .map(|(k, ring)| {
                    let flat: Vec<P2> = ring.iter().map(|p| (p.x, p.y)).collect();
                    let area = gcad_mesh::arch::layout::signed_area(&flat);
                    if (k == 0) == (area < 0.0) {
                        ring.into_iter().rev().collect()
                    } else {
                        ring
                    }
                })
                .collect()
        };
        let (bottom, top) = (fix(bottom), fix(top));
        loft(&bottom, &top)
    }
}

pub(crate) fn layer_list(model: &mut Model, words: &[&str]) -> Result<Vec<Layer>> {
    if words.is_empty() {
        bail!("a wall type needs layers outside to inside, like `brick:102 air:50 block:100`");
    }
    model.ensure_library()?;
    words
        .iter()
        .map(|word| {
            let (material, thickness) = word
                .split_once(':')
                .ok_or_else(|| anyhow!("`{word}` should be material:thickness"))?;
            let thickness = eval(thickness, &model.scope)
                .with_context(|| format!("the thickness of `{word}`"))?;
            if !(thickness > 0.0 && thickness.is_finite()) {
                bail!("`{word}`: a layer thickness must be above zero");
            }
            let known = material == "air"
                || model
                    .library
                    .contains_key(&crate::materials::slug(material))
                || crate::bodies::stock_of(material).is_some();
            if !known {
                bail!("no material `{material}` in the material library");
            }
            Ok(Layer {
                material: material.to_string(),
                thickness,
            })
        })
        .collect()
}

impl Model {
    pub(crate) fn op_wall_type(&mut self, line: &Line) -> Result<String> {
        let [name, layers @ ..] = line.positional.as_slice() else {
            bail!("write `wall_type ext brick:102 air:50 pir:100 block:100`");
        };
        if !line.named.is_empty() {
            bail!("`wall_type` takes a name and material:thickness layers");
        }
        let words: Vec<&str> = layers.iter().map(String::as_str).collect();
        let layers = layer_list(self, &words)?;
        let total: f64 = layers.iter().map(|l| l.thickness).sum();
        self.arch.types.retain(|(n, _)| n != name);
        self.arch.types.push((name.clone(), layers.clone()));
        Ok(format!(
            "wall type {name}: {} layers, {} thick",
            layers.len(),
            crate::parse::label(total)
        ))
    }

    fn tiers_of(&mut self, text: &str, height: f64) -> Result<Vec<Tier>> {
        let named = |model: &Model, name: &str| {
            model
                .arch
                .types
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, l)| l.clone())
        };
        if !text.contains(':') {
            let layers = named(self, text).ok_or_else(|| {
                anyhow!(
                    "no wall type `{text}`; types are {}",
                    self.arch
                        .types
                        .iter()
                        .map(|(n, _)| n.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            return Ok(vec![Tier {
                layers,
                from: 0.0,
                to: height,
            }]);
        }
        let mut tiers = Vec::new();
        for part in text.split(',') {
            let (name, range) = part
                .split_once(':')
                .ok_or_else(|| anyhow!("`{part}` should be type:from..to"))?;
            let (from, to) = range
                .split_once("..")
                .ok_or_else(|| anyhow!("`{part}` should be type:from..to"))?;
            let layers = named(self, name).ok_or_else(|| anyhow!("no wall type `{name}`"))?;
            let (from, to) = (eval(from, &self.scope)?, eval(to, &self.scope)?);
            if to <= from {
                bail!("`{part}` must rise");
            }
            tiers.push(Tier {
                layers,
                from: from.max(0.0),
                to: to.min(height),
            });
        }
        tiers.sort_by(|a, b| a.from.total_cmp(&b.from));
        for pair in tiers.windows(2) {
            if (pair[0].to - pair[1].from).abs() > 1.0e-9 {
                bail!("stacked wall types must meet with no gap or overlap");
            }
        }
        Ok(tiers)
    }

    pub(crate) fn op_wall(&mut self, line: &Line) -> Result<String> {
        let name = line
            .label
            .clone()
            .ok_or_else(|| anyhow!("a wall needs a label, like `w1: wall 0,0 4000,0 type=ext`"))?;
        if self.arch.wall(&name).is_some() || self.body_names().iter().any(|b| *b == name) {
            bail!("there is already a wall or body `{name}`");
        }
        let mut words: Vec<&str> = line.positional.iter().map(String::as_str).collect();
        let closed = words.contains(&"closed");
        words.retain(|w| *w != "closed");
        let chain = match words.as_slice() {
            ["profile"] => {
                let (_, profiles) = self.take_sketch("wall profile")?;
                let [profile] = profiles.as_slice() else {
                    bail!("`wall profile` takes one closed profile");
                };
                Chain::new(super::wall::outline(profile)?, true)
            }
            ["arc", a, b, c] => {
                let (a, b, c) = (
                    self.plan_point(a)?,
                    self.plan_point(b)?,
                    self.plan_point(c)?,
                );
                let points = gcad_mesh::arch::arc_points(a, b, c, ARC_PIECES)
                    .ok_or_else(|| anyhow!("the three points of an arc wall are in a line"))?;
                Chain::new(points, false)
            }
            ["arc", ..] => bail!("write `wall arc start through end`"),
            points => Chain::new(
                points
                    .iter()
                    .map(|p| self.plan_point(p))
                    .collect::<Result<_>>()?,
                closed,
            ),
        }
        .map_err(|e| anyhow!(e))?;
        let mut kind = None;
        let mut wall = Wall {
            name: name.clone(),
            line: chain,
            tiers: Vec::new(),
            justify: Justify::Centre,
            offset: 0.0,
            base: 0.0,
            top: f64::NAN,
            top_offset: (0.0, 0.0),
            batter: 0.0,
            pilasters: Vec::new(),
        };
        for (key, text) in &line.named {
            let number =
                |model: &Model| eval(text, &model.scope).with_context(|| format!("`{key}={text}`"));
            match key.as_str() {
                "type" => kind = Some(text.clone()),
                "base" => wall.base = number(self)?,
                "top" => wall.top = number(self)?,
                "offset" => wall.offset = number(self)?,
                "batter" => wall.batter = number(self)?,
                "justify" => {
                    wall.justify = Justify::parse(text).ok_or_else(|| {
                        anyhow!("justify is outside, centre or inside, not `{text}`")
                    })?
                }
                "top_offset" => wall.top_offset = crate::parse::eval_point(text, &self.scope)?,
                "pilaster" => {
                    for one in text.split(';') {
                        let (size, at) = one
                            .split_once('@')
                            .ok_or_else(|| anyhow!("write `pilaster=300x200@2000`"))?;
                        let (w, d) = size
                            .split_once('x')
                            .ok_or_else(|| anyhow!("write `pilaster=300x200@2000`"))?;
                        wall.pilasters.push((
                            eval(w, &self.scope)?,
                            eval(d, &self.scope)?,
                            eval(at, &self.scope)?,
                        ));
                    }
                }
                other => bail!(
                    "`wall` takes type=, base=, top=, justify=, offset=, top_offset=, batter= and pilaster=, not `{other}=`"
                ),
            }
        }
        if wall.top.is_nan() {
            bail!("a wall needs `top=`, a height or a level like `top=L1`");
        }
        let height = wall.top - wall.base;
        if height <= 0.0 {
            bail!(
                "the wall's top {} is not above its base {}",
                wall.top,
                wall.base
            );
        }
        if wall.batter.abs() >= 45.0 {
            bail!("batter is an angle in degrees under 45");
        }
        let kind = kind.ok_or_else(|| anyhow!("a wall needs `type=`, a `wall_type` name"))?;
        wall.tiers = self.tiers_of(&kind, height)?;
        let length = wall.line.length();
        for (w, _, at) in &wall.pilasters {
            if at - w / 2.0 < 0.0 || at + w / 2.0 > length {
                bail!("a pilaster at {at} runs past the end of `{name}`");
            }
        }
        let thickness = wall.thickness();
        self.arch.walls.push(wall);
        let made = self.rebuild_walls()?;
        Ok(format!(
            "wall {name}: {} long, {} thick, {} layer bodies{made}",
            crate::parse::label(length),
            crate::parse::label(thickness),
            self.arch
                .built
                .iter()
                .find(|b| b.owner == name)
                .map_or(0, |b| b.bodies.len())
        ))
    }

    pub(crate) fn op_wall_join(&mut self, line: &Line) -> Result<String> {
        let [a, b, how] = line.positional.as_slice() else {
            bail!("write `wall_join w1 w2 mitre|butt|tee`");
        };
        let how = Join::parse(how).ok_or_else(|| anyhow!("a join is mitre, butt or tee"))?;
        let index = |name: &str| {
            self.arch
                .walls
                .iter()
                .position(|w| w.name == name)
                .ok_or_else(|| anyhow!("no wall `{name}`"))
        };
        let (i, j) = (index(a)?, index(b)?);
        let lines = self.wall_lines();
        if !gcad_mesh::arch::meets(&lines, i, j) {
            bail!("`{a}` and `{b}` do not meet");
        }
        self.arch
            .joins
            .retain(|(x, y, _)| !(x == a && y == b || x == b && y == a));
        self.arch.joins.push((a.clone(), b.clone(), how));
        let made = self.rebuild_walls()?;
        Ok(format!("{a} and {b} join {how:?}{made}").to_lowercase())
    }

    pub(crate) fn wall_lines(&self) -> Vec<WallLine> {
        self.arch
            .walls
            .iter()
            .map(|w| {
                let (lo, hi) = w.outer_span();
                WallLine {
                    chain: w.line.clone(),
                    lo,
                    hi,
                }
            })
            .collect()
    }

    pub(crate) fn end_cuts(&self, lines: &[WallLine], index: usize) -> (Option<Cut>, Option<Cut>) {
        let forced: Vec<(usize, usize, Join)> = self
            .arch
            .joins
            .iter()
            .filter_map(|(a, b, how)| {
                let at = |n: &str| self.arch.walls.iter().position(|w| w.name == n);
                Some((at(a)?, at(b)?, *how))
            })
            .collect();
        (
            gcad_mesh::arch::end_cut(lines, index, false, &forced),
            gcad_mesh::arch::end_cut(lines, index, true, &forced),
        )
    }

    pub(crate) fn rebuild_walls(&mut self) -> Result<String> {
        let lines = self.wall_lines();
        let mut changed = Vec::new();
        for index in 0..self.arch.walls.len() {
            let wall = self.arch.walls[index].clone();
            let cuts = self.end_cuts(&lines, index);
            let openings: Vec<Opening> = self
                .arch
                .openings
                .iter()
                .filter(|o| o.host == wall.name)
                .cloned()
                .collect();
            let key = format!("{wall:?} {cuts:?}");
            let holes: Vec<String> = openings.iter().map(|o| format!("{o:?}")).collect();
            let previous = self
                .arch
                .built
                .iter()
                .find(|b| b.owner == wall.name)
                .cloned();
            let from = match &previous {
                Some(b) if b.key == key && holes.starts_with(&b.holes) => {
                    if b.holes.len() == holes.len() {
                        continue;
                    }
                    b.holes.len()
                }
                _ => 0,
            };
            let names = wall.layer_names();
            let mut solids: Vec<Solid> = if from == 0 {
                names
                    .iter()
                    .map(|(_, t, l)| {
                        let tier = &wall.tiers[*t];
                        let bands = gcad_mesh::arch::bands(&tier.layers, wall.justify, wall.offset);
                        wall.layer_solid(tier, bands[*l], cuts)
                            .with_context(|| format!("building `{}`", wall.name))
                    })
                    .collect::<Result<_>>()?
            } else {
                names
                    .iter()
                    .map(|(n, _, _)| self.named_body(n))
                    .collect::<Result<_>>()?
            };
            for opening in &openings[from..] {
                let tools = self.opening_tools(&wall, opening)?;
                for solid in solids.iter_mut() {
                    for tool in &tools {
                        let b = crate::geometry::bounds(solid);
                        let t = crate::geometry::bounds(tool);
                        let apart =
                            (0..3).any(|k| b.max()[k] < t.min()[k] || t.max()[k] < b.min()[k]);
                        if apart {
                            continue;
                        }
                        let cut = monstertruck::solid::difference_normalized(solid, tool).map_err(
                            |e| {
                                anyhow!(
                                    "cutting `{}` out of `{}` failed: {e}",
                                    opening.name,
                                    wall.name
                                )
                            },
                        )?;
                        *solid = crate::fuse::fuse_coplanar(&crate::exact::exact_edges(&cut));
                    }
                }
            }
            let bodies: Vec<String> = names.iter().map(|(n, _, _)| n.clone()).collect();
            if let Some(old) = &previous {
                for body in &old.bodies {
                    if !bodies.contains(body) {
                        self.bodies.retain(|(n, _)| n != body);
                        self.materials.remove(body);
                    }
                }
            }
            for ((name, t, l), solid) in names.iter().zip(solids) {
                match self.bodies.iter_mut().find(|(n, _)| n == name) {
                    Some(slot) => slot.1 = solid,
                    None => self.bodies.push((name.clone(), solid)),
                }
                let material = wall.tiers[*t].layers[*l].material.clone();
                if self.materials.get(name).is_none_or(|m| m.name != material) {
                    let line = Line {
                        number: 0,
                        text: format!("material {material}"),
                        label: None,
                        op: "material".into(),
                        positional: vec![material],
                        named: Vec::new(),
                    };
                    self.material_for(&line, name.clone(), &[])?;
                }
            }
            self.arch.built.retain(|b| b.owner != wall.name);
            self.arch.built.push(Built {
                owner: wall.name.clone(),
                key,
                holes,
                bodies,
            });
            changed.push(wall.name.clone());
            let fills: Vec<Opening> = openings.clone();
            for opening in fills {
                self.build_fill(&wall, &opening)?;
            }
        }
        self.rebuild_sweeps()?;
        Ok(match changed.as_slice() {
            [] => String::new(),
            [_] => String::new(),
            many => format!("; rebuilt {}", many.join(", ")),
        })
    }

    pub(crate) fn op_wall_sweep(&mut self, line: &Line) -> Result<String> {
        let name = crate::args::label_of(line);
        let (face, size) = match line.positional.as_slice() {
            [face, shape, size] if shape == "rect" => (face.clone(), size.clone()),
            _ => bail!("write `skirt: wall_sweep w1.inside rect 15x100 at=0`"),
        };
        let (host, side) = face
            .split_once('.')
            .ok_or_else(|| anyhow!("sweep along `wall.inside`, `wall.outside` or `wall.top`"))?;
        if self.arch.wall(host).is_none() {
            bail!("no wall `{host}`");
        }
        if !matches!(side, "inside" | "outside" | "top") {
            bail!("a sweep runs along `{host}.inside`, `{host}.outside` or `{host}.top`");
        }
        let (w, h) = size
            .split_once('x')
            .ok_or_else(|| anyhow!("write the profile as `rect WIDTHxHEIGHT`"))?;
        let (width, height) = (eval(w, &self.scope)?, eval(h, &self.scope)?);
        let mut at = 0.0;
        let mut material = String::new();
        for (key, text) in &line.named {
            match key.as_str() {
                "at" => at = eval(text, &self.scope)?,
                "material" => material = text.clone(),
                other => bail!("`wall_sweep` takes at= and material=, not `{other}=`"),
            }
        }
        if !(width > 0.0 && height > 0.0) {
            bail!("a sweep profile needs a size above zero");
        }
        self.arch.sweeps.retain(|s| s.name != name);
        self.arch.sweeps.push(Sweep {
            name: name.clone(),
            host: host.to_string(),
            face: side.to_string(),
            width,
            height,
            at,
            material,
        });
        self.rebuild_sweeps()?;
        Ok(format!("{name} runs along {face}"))
    }

    fn rebuild_sweeps(&mut self) -> Result<()> {
        let lines = self.wall_lines();
        for sweep in self.arch.sweeps.clone() {
            let index = self
                .arch
                .walls
                .iter()
                .position(|w| w.name == sweep.host)
                .expect("checked host");
            let wall = self.arch.walls[index].clone();
            let (lo, hi) = wall.outer_span();
            let cuts = self.end_cuts(&lines, index);
            let (band, z0, z1) = match sweep.face.as_str() {
                "inside" => (
                    (hi, hi + sweep.width),
                    wall.base + sweep.at,
                    wall.base + sweep.at + sweep.height,
                ),
                "outside" => (
                    (lo - sweep.width, lo),
                    wall.base + sweep.at,
                    wall.base + sweep.at + sweep.height,
                ),
                _ => {
                    let mid = (lo + hi) / 2.0;
                    (
                        (mid - sweep.width / 2.0, mid + sweep.width / 2.0),
                        wall.top + sweep.at,
                        wall.top + sweep.at + sweep.height,
                    )
                }
            };
            let tier = Tier {
                layers: vec![Layer {
                    material: "sweep".into(),
                    thickness: band.1 - band.0,
                }],
                from: z0 - wall.base,
                to: z1 - wall.base,
            };
            let flat = Wall {
                top: z1,
                top_offset: (0.0, 0.0),
                batter: 0.0,
                pilasters: Vec::new(),
                tiers: vec![tier.clone()],
                ..wall
            };
            let solid = flat.layer_solid(&tier, band, cuts)?;
            match self.bodies.iter_mut().find(|(n, _)| *n == sweep.name) {
                Some(slot) => slot.1 = solid,
                None => self.bodies.push((sweep.name.clone(), solid)),
            }
            if !sweep.material.is_empty() {
                let line = Line {
                    number: 0,
                    text: String::new(),
                    label: None,
                    op: "material".into(),
                    positional: vec![sweep.material.clone()],
                    named: Vec::new(),
                };
                self.material_for(&line, sweep.name.clone(), &[])?;
            }
            self.arch.built.retain(|b| b.owner != sweep.name);
            self.arch.built.push(Built {
                owner: sweep.name.clone(),
                key: String::new(),
                holes: Vec::new(),
                bodies: vec![sweep.name.clone()],
            });
        }
        Ok(())
    }

    pub(crate) fn wall_faces(&self, selector: &str) -> Option<Result<Vec<Face>>> {
        let mut parts = selector.split('.');
        let wall = self.arch.wall(parts.next()?)?;
        let rest: Vec<&str> = parts.collect();
        Some(self.faces_of_wall(wall, &rest))
    }

    fn faces_of_wall(&self, wall: &Wall, rest: &[&str]) -> Result<Vec<Face>> {
        let names = wall.layer_names();
        let (pick, group): (Vec<&(String, usize, usize)>, &str) = match rest {
            [group] => (names.iter().collect(), group),
            [layer, group] => (
                names
                    .iter()
                    .filter(|(n, _, _)| n.strip_prefix(&format!("{}.", wall.name)) == Some(layer))
                    .collect(),
                group,
            ),
            _ => bail!(
                "wall faces are `{0}.outside`, `{0}.inside`, `{0}.top`, `{0}.bottom`, `{0}.ends`, or per layer `{0}.brick.outside`",
                wall.name
            ),
        };
        if pick.is_empty() {
            bail!("`{}` has no layer `{}`", wall.name, rest[0]);
        }
        let mut found = Vec::new();
        for (name, t, l) in pick {
            let tier = &wall.tiers[*t];
            let (lo, hi) = if rest.len() == 2 {
                gcad_mesh::arch::bands(&tier.layers, wall.justify, wall.offset)[*l]
            } else {
                wall.span(tier)
            };
            let solid = self.named_body(name)?;
            for face in crate::select::faces(&solid) {
                let Surface::Plane(plane) = face.oriented_surface() else {
                    continue;
                };
                let normal = plane.normal();
                let centre = {
                    let points = crate::select::sample_points(&face);
                    let sum = points.iter().fold(Vector3::zero(), |s, p| s + p.to_vec());
                    Point3::from_vec(sum / points.len().max(1) as f64)
                };
                let keep = match group {
                    "top" => normal.z > 0.5,
                    "bottom" => normal.z < -0.5,
                    "outside" | "inside" | "ends" if normal.z.abs() < 0.5 => {
                        match wall.line.project((centre.x, centre.y)) {
                            Some((i, _, off)) => {
                                let dir = wall.line.dir(i);
                                let n = gcad_mesh::arch::layout::left(dir);
                                let across = normal.x * n.0 + normal.y * n.1;
                                let along = normal.x * dir.0 + normal.y * dir.1;
                                match group {
                                    "outside" => across < -0.999 && (off - lo).abs() < 1.0e-6,
                                    "inside" => across > 0.999 && (off - hi).abs() < 1.0e-6,
                                    _ => along.abs() > 0.5 && !wall.line.closed,
                                }
                            }
                            None => group == "ends",
                        }
                    }
                    "outside" | "inside" | "ends" => false,
                    other => bail!(
                        "`{other}` is not a wall face; use outside, inside, top, bottom or ends"
                    ),
                };
                if keep {
                    found.push(face);
                }
            }
        }
        if found.is_empty() {
            bail!("`{}.{}` matched no faces", wall.name, rest.join("."));
        }
        Ok(found)
    }

    pub(crate) fn opening_outline(&self, opening: &Opening) -> Vec<P2> {
        let (s0, s1) = (opening.at - opening.w / 2.0, opening.at + opening.w / 2.0);
        let (z0, z1) = (opening.sill, opening.sill + opening.h);
        head_outline(opening.head, s0, s1, z0, z1)
    }
}

pub(crate) fn head_outline(head: Head, s0: f64, s1: f64, z0: f64, z1: f64) -> Vec<P2> {
    let w = s1 - s0;
    let rise = match head {
        Head::Flat => 0.0,
        Head::Round => w / 2.0,
        Head::Arch => w / 4.0,
        Head::Segment => w / 8.0,
    }
    .min(z1 - z0);
    if rise <= 0.0 {
        return vec![(s0, z0), (s1, z0), (s1, z1), (s0, z1)];
    }
    let spring = z1 - rise;
    let arc = gcad_mesh::arch::arc_points(
        (s1, spring),
        ((s0 + s1) / 2.0, z1),
        (s0, spring),
        ARC_PIECES,
    )
    .expect("a rising arc");
    let mut out = vec![(s0, z0), (s1, z0)];
    out.extend(arc);
    out
}
