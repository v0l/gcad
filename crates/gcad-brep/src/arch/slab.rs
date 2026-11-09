use super::Slab;
use super::wall::{ccw, layer_list, loft, piece_of};
use crate::Model;
use crate::parse::{Line, eval, eval_point};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::layout::{dot, length, signed_area, sub, unit};
use gcad_mesh::arch::{Layer, P2};
use monstertruck::modeling::*;

pub(crate) fn inside(point: P2, polygon: &[P2]) -> bool {
    let n = polygon.len();
    let mut result = false;
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        if (a.1 > point.1) != (b.1 > point.1)
            && point.0 < (b.0 - a.0) * (point.1 - a.1) / (b.1 - a.1) + a.0
        {
            result = !result;
        }
    }
    result
}

fn centroid(ring: &[P2]) -> P2 {
    let n = ring.len() as f64;
    let (x, y) = ring.iter().fold((0.0, 0.0), |(x, y), p| (x + p.0, y + p.1));
    (x / n, y / n)
}

pub(crate) fn slab_layers(model: &mut Model, text: &str) -> Result<Vec<Layer>> {
    let words: Vec<&str> = text.split('+').collect();
    layer_list(model, &words)
}

impl Slab {
    fn rise(&self, p: P2) -> f64 {
        match self.fall {
            Some((to, dir)) => self.slope * dot(sub(p, to), dir),
            None => 0.0,
        }
    }

    pub(crate) fn layer_names(&self) -> Vec<(String, usize)> {
        let mut used = Vec::new();
        let mut out = Vec::new();
        for (l, layer) in self.layers.iter().enumerate() {
            if layer.material == "air" {
                continue;
            }
            let name = piece_of(&format!("{}.{}", self.name, layer.material), &used);
            used.push(name.clone());
            out.push((name, l));
        }
        out
    }

    pub(crate) fn bands(&self) -> Vec<(f64, f64)> {
        let total: f64 = self.layers.iter().map(|l| l.thickness).sum();
        let mut at = self.top - total;
        self.layers
            .iter()
            .map(|l| {
                let band = (at, at + l.thickness);
                at += l.thickness;
                band
            })
            .collect()
    }

    pub(crate) fn layer_solid(&self, band: (f64, f64)) -> Result<Solid> {
        let ring = |z: f64| -> Vec<Vec<Point3>> {
            let lift = |ring: &[P2]| -> Vec<Point3> {
                ring.iter()
                    .map(|&p| Point3::new(p.0, p.1, z + self.rise(p)))
                    .collect()
            };
            let mut rings = vec![lift(&ccw(self.outline.clone()))];
            for hole in &self.holes {
                let mut h = ccw(hole.clone());
                h.reverse();
                rings.push(lift(&h));
            }
            rings
        };
        loft(&ring(band.0), &ring(band.1))
    }
}

pub(crate) fn prism(outer: &[P2], holes: &[Vec<P2>], z0: f64, z1: f64) -> Result<Solid> {
    let ring = |z: f64| -> Vec<Vec<Point3>> {
        let lift = |ring: &[P2]| -> Vec<Point3> {
            ring.iter().map(|&p| Point3::new(p.0, p.1, z)).collect()
        };
        let mut rings = vec![lift(&ccw(outer.to_vec()))];
        for hole in holes {
            let mut h = ccw(hole.clone());
            h.reverse();
            rings.push(lift(&h));
        }
        rings
    };
    loft(&ring(z0), &ring(z1))
}

fn flat(ring: Vec<(P2, f64)>) -> Vec<P2> {
    ring.into_iter().map(|(p, _)| p).collect()
}

impl Model {
    fn closed_wall(&self, name: &str, op: &str) -> Result<super::Wall> {
        let wall = self
            .arch
            .wall(name)
            .ok_or_else(|| anyhow!("`{op}` follows a wall, and there is no wall `{name}`"))?;
        Ok(wall.clone())
    }

    pub(crate) fn inner_ring(&self, wall: &super::Wall) -> Result<Vec<P2>> {
        if !wall.line.closed {
            bail!(
                "`{}` is not a closed loop of walls, so it has no inside",
                wall.name
            );
        }
        let (lo, hi) = wall.outer_span();
        let a = flat(wall.line.offset(lo, None, None));
        let b = flat(wall.line.offset(hi, None, None));
        Ok(if signed_area(&a).abs() < signed_area(&b).abs() {
            a
        } else {
            b
        })
    }

    pub(crate) fn op_foundation(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a foundation needs a label, like `fs: foundation strip outer w=600 d=900`")
        })?;
        self.check_free_name(&name)?;
        let mut w = None;
        let mut l = None;
        let mut d = None;
        let mut top = None;
        let mut material = "concrete".to_string();
        for (key, text) in &line.named {
            let value = || eval(text, &self.scope).with_context(|| format!("`{key}={text}`"));
            match key.as_str() {
                "w" => w = Some(value()?),
                "l" => l = Some(value()?),
                "d" => d = Some(value()?),
                "top" => top = Some(value()?),
                "material" => material = text.clone(),
                other => bail!("`foundation` takes w=, l=, d=, top= and material=, not `{other}=`"),
            }
        }
        let d = d.ok_or_else(|| anyhow!("a foundation needs `d=`, its depth"))?;
        let w = w.ok_or_else(|| anyhow!("a foundation needs `w=`, its width"))?;
        if d <= 0.0 || w <= 0.0 || l.is_some_and(|l| l <= 0.0) {
            bail!("a foundation's w=, l= and d= must be above zero");
        }
        let (solid, top, what) = match line.positional.as_slice() {
            [kind, host] if kind == "strip" => {
                if l.is_some() {
                    bail!("a strip foundation runs the length of its wall; it takes no `l=`");
                }
                let wall = self.closed_wall(host, "foundation strip")?;
                let (lo, hi) = wall.outer_span();
                let mid = (lo + hi) / 2.0;
                let (outer, hole) = wall
                    .line
                    .band(mid - w / 2.0, mid + w / 2.0, None, None, &[]);
                let holes: Vec<Vec<P2>> = hole.map(flat).into_iter().collect();
                let top = top.unwrap_or(wall.base);
                (
                    prism(&flat(outer), &holes, top - d, top)?,
                    top,
                    format!("under {host}"),
                )
            }
            [kind, at] if kind == "pad" => {
                let c = self.plan_point(at)?;
                let l = l.unwrap_or(w);
                let ring = vec![
                    (c.0 - w / 2.0, c.1 - l / 2.0),
                    (c.0 + w / 2.0, c.1 - l / 2.0),
                    (c.0 + w / 2.0, c.1 + l / 2.0),
                    (c.0 - w / 2.0, c.1 + l / 2.0),
                ];
                let top = top.unwrap_or(0.0);
                (prism(&ring, &[], top - d, top)?, top, format!("at {at}"))
            }
            _ => bail!("write `foundation strip WALL w= d=` or `foundation pad x,y w= l= d=`"),
        };
        self.ensure_library()?;
        self.put_body(name.clone(), solid, &material)?;
        self.note_built(&name, &name);
        Ok(format!(
            "foundation {name} {what}, {} deep, top at {}",
            crate::parse::label(d),
            crate::parse::label(top)
        ))
    }

    pub(crate) fn op_ground_slab(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a ground slab needs a label, like `g: ground_slab outer type=concrete:150`")
        })?;
        self.check_free_name(&name)?;
        let [host] = line.positional.as_slice() else {
            bail!("write `g: ground_slab outer type=concrete:150+pir:100`");
        };
        let wall = self.closed_wall(host, "ground_slab")?;
        let outline = self.inner_ring(&wall)?;
        let mut layers = None;
        let mut top = wall.base;
        for (key, text) in &line.named {
            match key.as_str() {
                "type" => layers = Some(slab_layers(self, text)?),
                "top" => top = eval(text, &self.scope).with_context(|| format!("`top={text}`"))?,
                other => bail!("`ground_slab` takes type= and top=, not `{other}=`"),
            }
        }
        let layers = layers.ok_or_else(|| {
            anyhow!("a ground slab needs `type=`, layers bottom to top like `concrete:150+pir:100`")
        })?;
        let area = signed_area(&outline).abs();
        self.arch.slabs.push(Slab {
            name: name.clone(),
            outline,
            holes: Vec::new(),
            layers,
            top,
            slope: 0.0,
            fall: None,
        });
        self.build_slab(&name)?;
        Ok(format!(
            "ground slab {name} inside {host}: {} m², top at {}",
            crate::parse::label(area / 1.0e6),
            crate::parse::label(top)
        ))
    }

    pub(crate) fn plan_ring(&self, words: &[String]) -> Result<Vec<P2>> {
        let mut ring: Vec<P2> = Vec::new();
        for word in words {
            let p = self.plan_point(word)?;
            if ring.last().is_none_or(|q| length(sub(p, *q)) > 1.0e-6) {
                ring.push(p);
            }
        }
        if ring.len() > 1 && length(sub(ring[0], ring[ring.len() - 1])) < 1.0e-6 {
            ring.pop();
        }
        if ring.len() < 3 || signed_area(&ring).abs() < 1.0e-6 {
            bail!("an outline needs three or more points around an area");
        }
        Ok(ring)
    }

    pub(crate) fn op_slab(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a slab needs a label, like `s1: slab L1 A1 B1 B2 A2 type=concrete:200`")
        })?;
        self.check_free_name(&name)?;
        let [level, points @ ..] = line.positional.as_slice() else {
            bail!("write `s1: slab L1 A1 B1 B2 A2 type=concrete:200`");
        };
        let top = eval(level, &self.scope).with_context(|| format!("the slab level `{level}`"))?;
        let outline = self.plan_ring(points)?;
        let mut layers = None;
        let mut slope = 0.0;
        let mut falls_to = None;
        for (key, text) in &line.named {
            match key.as_str() {
                "type" => layers = Some(slab_layers(self, text)?),
                "slope" => {
                    slope = eval(text, &self.scope).with_context(|| format!("`slope={text}`"))?
                }
                "falls_to" => falls_to = Some(eval_point(text, &self.scope)?),
                other => bail!("`slab` takes type=, slope= and falls_to=, not `{other}=`"),
            }
        }
        let layers = layers.ok_or_else(|| {
            anyhow!("a slab needs `type=`, layers bottom to top like `concrete:200+screed:50`")
        })?;
        let fall = match falls_to {
            Some(to) if slope != 0.0 => {
                let away = sub(centroid(&outline), to);
                if length(away) < 1.0e-6 {
                    bail!("`falls_to` is the middle of the slab, so it has no direction to fall");
                }
                Some((to, unit(away)))
            }
            Some(_) => bail!("`falls_to=` needs a `slope=`, like `slope=0.02`"),
            None if slope != 0.0 => bail!("`slope=` needs `falls_to=x,y`, the low point"),
            None => None,
        };
        let slab = Slab {
            name: name.clone(),
            outline,
            holes: Vec::new(),
            layers,
            top,
            slope,
            fall,
        };
        let total: f64 = slab.layers.iter().map(|l| l.thickness).sum();
        let area = signed_area(&slab.outline).abs();
        self.arch.slabs.push(slab);
        self.build_slab(&name)?;
        Ok(format!(
            "slab {name}: {} m², {} thick, top at {}",
            crate::parse::label(area / 1.0e6),
            crate::parse::label(total),
            crate::parse::label(top)
        ))
    }

    pub(crate) fn op_slab_opening(&mut self, line: &Line) -> Result<String> {
        let [host, points @ ..] = line.positional.as_slice() else {
            bail!("write `slab_opening s1 x,y x,y x,y x,y`");
        };
        if !line.named.is_empty() {
            bail!("`slab_opening` takes a slab and the points of the hole");
        }
        let ring = self.plan_ring(points)?;
        let index = self
            .arch
            .slabs
            .iter()
            .position(|s| s.name == *host)
            .ok_or_else(|| anyhow!("no slab `{host}`"))?;
        let slab = &self.arch.slabs[index];
        if ring.iter().any(|&p| !inside(p, &slab.outline)) {
            bail!("the opening runs outside the edge of `{host}`");
        }
        if slab
            .holes
            .iter()
            .any(|h| h.iter().any(|&p| inside(p, &ring)) || ring.iter().any(|&p| inside(p, h)))
        {
            bail!("the opening overlaps another opening in `{host}`");
        }
        let area = signed_area(&ring).abs();
        self.arch.slabs[index].holes.push(ring);
        self.build_slab(host)?;
        Ok(format!(
            "opening in {host}: {} m²",
            crate::parse::label(area / 1.0e6)
        ))
    }

    pub(crate) fn check_free_name(&self, name: &str) -> Result<()> {
        if self.arch.wall(name).is_some()
            || self.arch.slabs.iter().any(|s| s.name == name)
            || self.body_names().iter().any(|b| b == name)
        {
            bail!("there is already something called `{name}`");
        }
        Ok(())
    }

    pub(crate) fn build_slab(&mut self, name: &str) -> Result<()> {
        let slab = self
            .arch
            .slabs
            .iter()
            .find(|s| s.name == name)
            .cloned()
            .expect("a slab");
        let bands = slab.bands();
        for (body, l) in slab.layer_names() {
            let solid = slab
                .layer_solid(bands[l])
                .with_context(|| format!("building `{name}`"))?;
            self.put_body(body.clone(), solid, &slab.layers[l].material)?;
            self.note_built(name, &body);
        }
        Ok(())
    }
}
