use super::wall::{ccw, head_outline, outline};
use super::{Fill, Head, Opening, Wall};
use crate::Model;
use crate::parse::{Line, eval};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::P2;
use gcad_mesh::arch::layout::{left, signed_area};
use monstertruck::modeling::*;

pub(crate) struct Station {
    origin: P2,
    dir: P2,
    base: f64,
}

impl Station {
    fn at(&self, s: f64, d: f64, z: f64) -> Point3 {
        let n = left(self.dir);
        Point3::new(
            self.origin.0 + self.dir.0 * s + n.0 * d,
            self.origin.1 + self.dir.1 * s + n.1 * d,
            self.base + z,
        )
    }

    fn matrix(&self, s: f64, d: f64, z: f64) -> Matrix4 {
        let n = left(self.dir);
        let o = self.at(s, d, z);
        Matrix4::from_cols(
            Vector4::new(self.dir.0, self.dir.1, 0.0, 0.0),
            Vector4::new(n.0, n.1, 0.0, 0.0),
            Vector4::new(0.0, 0.0, 1.0, 0.0),
            Vector4::new(o.x, o.y, o.z, 1.0),
        )
    }

    fn prism(&self, outer: &[P2], holes: &[Vec<P2>], d0: f64, d1: f64) -> Result<Solid> {
        let mut rings = vec![ccw(outer.to_vec())];
        rings.extend(holes.iter().map(|h| {
            let mut h = ccw(h.clone());
            h.reverse();
            h
        }));
        let lift = |d: f64| -> Vec<Vec<Point3>> {
            rings
                .iter()
                .map(|ring| ring.iter().map(|&(s, z)| self.at(s, d, z)).collect())
                .collect()
        };
        super::wall::loft(&lift(d0), &lift(d1))
    }
}

fn rect(s0: f64, s1: f64, z0: f64, z1: f64) -> Vec<P2> {
    vec![(s0, z0), (s1, z0), (s1, z1), (s0, z1)]
}

impl Model {
    fn station(&self, wall: &Wall, opening: &Opening) -> Result<Station> {
        let line = &wall.line;
        let (i, _, dir) = line.locate(opening.at);
        let starts = line.starts();
        if !matches!(opening.fill, Fill::Cut(_)) {
            let (s0, s1) = (opening.at - opening.w / 2.0, opening.at + opening.w / 2.0);
            if s0 < starts[i] - 1.0e-6 || s1 > starts[i + 1] + 1.0e-6 {
                bail!(
                    "`{}` crosses a corner of `{}`; openings sit on one straight run",
                    opening.name,
                    wall.name
                );
            }
        }
        let p = line.points[i];
        Ok(Station {
            origin: (p.0 - dir.0 * starts[i], p.1 - dir.1 * starts[i]),
            dir,
            base: wall.base,
        })
    }

    fn reach(wall: &Wall) -> (f64, f64) {
        let (lo, hi) = wall.outer_span();
        let pier = wall.pilasters.iter().map(|p| p.1).fold(0.0, f64::max);
        let lean = (wall.top - wall.base) * wall.batter.to_radians().tan().abs();
        let slack = (hi - lo) + 10.0;
        (lo - slack - lean, hi + slack + pier + lean)
    }

    pub(crate) fn opening_tools(&self, wall: &Wall, opening: &Opening) -> Result<Vec<Solid>> {
        let station = self.station(wall, opening)?;
        let (d0, d1) = Self::reach(wall);
        match &opening.fill {
            Fill::Cut(profiles) => profiles
                .iter()
                .map(|p| station.prism(&outline(p)?, &[], d0, d1))
                .collect(),
            _ => Ok(vec![station.prism(
                &self.opening_outline(opening),
                &[],
                d0,
                d1,
            )?]),
        }
    }

    fn put_body(&mut self, name: String, solid: Solid, material: &str) -> Result<()> {
        match self.bodies.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = solid,
            None => self.bodies.push((name.clone(), solid)),
        }
        let line = Line {
            number: 0,
            text: format!("material {material}"),
            label: None,
            op: "material".into(),
            positional: vec![material.to_string()],
            named: Vec::new(),
        };
        self.material_for(&line, name.clone(), &[])?;
        if material == "glass" {
            self.opacities.insert(name, 0.35);
        }
        Ok(())
    }

    pub(crate) fn build_fill(&mut self, wall: &Wall, opening: &Opening) -> Result<()> {
        let station = self.station(wall, opening)?;
        let (lo, hi) = wall.outer_span();
        let (s0, s1) = (opening.at - opening.w / 2.0, opening.at + opening.w / 2.0);
        let (z0, z1) = (opening.sill, opening.sill + opening.h);
        let name = &opening.name;
        let mut made: Vec<(String, Solid, &str)> = Vec::new();
        match &opening.fill {
            Fill::Hole | Fill::Cut(_) => {}
            Fill::Door {
                swing,
                inward,
                frame: (fw, fd),
                leaf,
                threshold,
            } => {
                let (fw, fd) = (*fw, *fd);
                let (b0, b1) = if *inward {
                    (hi - fd, hi)
                } else {
                    (lo, lo + fd)
                };
                let frame = vec![
                    (s0, z0),
                    (s0 + fw, z0),
                    (s0 + fw, z1 - fw),
                    (s1 - fw, z1 - fw),
                    (s1 - fw, z0),
                    (s1, z0),
                    (s1, z1),
                    (s0, z1),
                ];
                made.push((
                    format!("{name}.frame"),
                    station.prism(&frame, &[], b0, b1)?,
                    "wood",
                ));
                let count = match swing.as_str() {
                    "double" | "slide" => 2,
                    "fold" => 4,
                    _ => 1,
                };
                let bottom = z0 + threshold;
                let width = (opening.w - 2.0 * fw) / count as f64;
                let mid = (b0 + b1) / 2.0;
                for k in 0..count {
                    let a = s0 + fw + width * k as f64;
                    let solid = station.prism(
                        &rect(a, a + width, bottom, z1 - fw),
                        &[],
                        mid - leaf / 2.0,
                        mid + leaf / 2.0,
                    )?;
                    let label = if k == 0 {
                        format!("{name}.leaf")
                    } else {
                        format!("{name}.leaf.{}", k + 1)
                    };
                    made.push((label, solid, "wood"));
                }
                if *threshold > 0.0 {
                    made.push((
                        format!("{name}.threshold"),
                        station.prism(&rect(s0 + fw, s1 - fw, z0, bottom), &[], lo, hi)?,
                        "wood",
                    ));
                }
            }
            Fill::Window {
                panes: (cols, rows),
                frame: fw,
                depth,
                mullion,
                glass,
            } => {
                let fw = *fw;
                let mid = (lo + hi) / 2.0;
                let (b0, b1) = (mid - depth / 2.0, mid + depth / 2.0);
                let outer = self.opening_outline(opening);
                let inner = head_outline(opening.head, s0 + fw, s1 - fw, z0 + fw, z1 - fw);
                made.push((
                    format!("{name}.frame"),
                    station.prism(&outer, std::slice::from_ref(&inner), b0, b1)?,
                    "wood",
                ));
                let (g0, g1) = (mid - glass / 2.0, mid + glass / 2.0);
                let (cols, rows) = if opening.head == Head::Flat {
                    (*cols, *rows)
                } else {
                    (1, 1)
                };
                let pane_w = (opening.w - 2.0 * fw - (cols - 1) as f64 * mullion) / cols as f64;
                let pane_h = (opening.h - 2.0 * fw - (rows - 1) as f64 * mullion) / rows as f64;
                if pane_w <= 0.0 || pane_h <= 0.0 {
                    bail!(
                        "`{name}` has no room for {cols}x{rows} panes with its frame and mullions"
                    );
                }
                let mut pane = 0;
                for r in 0..rows {
                    for c in 0..cols {
                        let a = s0 + fw + c as f64 * (pane_w + mullion);
                        let b = z0 + fw + r as f64 * (pane_h + mullion);
                        let shape = if cols == 1 && rows == 1 {
                            inner.clone()
                        } else {
                            rect(a, a + pane_w, b, b + pane_h)
                        };
                        pane += 1;
                        made.push((
                            format!("{name}.glass.{pane}"),
                            station.prism(&shape, &[], g0, g1)?,
                            "glass",
                        ));
                    }
                }
                let mut bars = Vec::new();
                for c in 1..cols {
                    let a = s0 + fw + c as f64 * pane_w + (c - 1) as f64 * mullion;
                    bars.push(rect(a, a + mullion, z0 + fw, z1 - fw));
                }
                for r in 1..rows {
                    let b = z0 + fw + r as f64 * pane_h + (r - 1) as f64 * mullion;
                    for c in 0..cols {
                        let a = s0 + fw + c as f64 * (pane_w + mullion);
                        bars.push(rect(a, a + pane_w, b, b + mullion));
                    }
                }
                for (k, bar) in bars.iter().enumerate() {
                    let label = if k == 0 {
                        format!("{name}.mullion")
                    } else {
                        format!("{name}.mullion.{}", k + 1)
                    };
                    made.push((label, station.prism(bar, &[], b0, b1)?, "wood"));
                }
            }
            Fill::Garage { panels } => {
                let height = opening.h / *panels as f64;
                let b1 = hi;
                for k in 0..*panels {
                    let b = z0 + height * k as f64;
                    made.push((
                        format!("{name}.panel.{}", k + 1),
                        station.prism(&rect(s0, s1, b, b + height), &[], b1 - 40.0, b1)?,
                        "steel",
                    ));
                }
            }
            Fill::Part { file, vars } => {
                let part = crate::load_model(file, vars, self.depth + 1, &self.cache)
                    .with_context(|| format!("building `{name}` from {}", file.display()))?;
                let place = station.matrix(opening.at, (lo + hi) / 2.0, opening.sill);
                for (body, solid) in part.named_solids() {
                    let moved = crate::bodies::moved(solid, place);
                    let material = part
                        .materials
                        .get(&body)
                        .map_or("wood".to_string(), |m| m.name.clone());
                    let label = format!("{name}.{body}");
                    match self.bodies.iter_mut().find(|(n, _)| *n == label) {
                        Some(slot) => slot.1 = moved,
                        None => self.bodies.push((label.clone(), moved)),
                    }
                    if crate::bodies::stock_of(&material).is_some()
                        || self
                            .library
                            .contains_key(&crate::materials::slug(&material))
                    {
                        self.put_material(&label, &material)?;
                    }
                    self.note_built(name, &label);
                }
            }
        }
        for (label, solid, material) in made {
            self.put_body(label.clone(), solid, material)?;
            self.note_built(name, &label);
        }
        Ok(())
    }

    fn put_material(&mut self, body: &str, material: &str) -> Result<()> {
        let line = Line {
            number: 0,
            text: String::new(),
            label: None,
            op: "material".into(),
            positional: vec![material.to_string()],
            named: Vec::new(),
        };
        self.material_for(&line, body.to_string(), &[])?;
        Ok(())
    }

    fn note_built(&mut self, owner: &str, body: &str) {
        match self.arch.built.iter_mut().find(|b| b.owner == owner) {
            Some(built) => {
                if !built.bodies.iter().any(|b| b == body) {
                    built.bodies.push(body.to_string());
                }
            }
            None => self.arch.built.push(super::Built {
                owner: owner.to_string(),
                key: String::new(),
                holes: Vec::new(),
                bodies: vec![body.to_string()],
            }),
        }
    }

    fn check_opening(&self, opening: &Opening) -> Result<()> {
        let wall = self.arch.wall(&opening.host).ok_or_else(|| {
            anyhow!(
                "`{}` is on `{}`, which is not a wall; walls are {}",
                opening.name,
                opening.host,
                self.arch
                    .walls
                    .iter()
                    .map(|w| w.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        if self.arch.openings.iter().any(|o| o.name == opening.name)
            || self.body_names().contains(&opening.name)
        {
            bail!("there is already something called `{}`", opening.name);
        }
        if matches!(opening.fill, Fill::Cut(_)) {
            return Ok(());
        }
        let length = wall.line.length();
        let (s0, s1) = (opening.at - opening.w / 2.0, opening.at + opening.w / 2.0);
        if s0 < -1.0e-6 || s1 > length + 1.0e-6 {
            bail!(
                "`{}` runs from {} to {} along `{}`, past the end of the wall (0 to {})",
                opening.name,
                crate::parse::label(s0),
                crate::parse::label(s1),
                wall.name,
                crate::parse::label(length)
            );
        }
        let top = wall.top_at(s0).min(wall.top_at(s1)) - wall.base;
        if opening.sill < -1.0e-6 || opening.sill + opening.h > top + 1.0e-6 {
            bail!(
                "`{}` reaches {} above the base of `{}`, above the top of the wall at {}",
                opening.name,
                crate::parse::label(opening.sill + opening.h),
                wall.name,
                crate::parse::label(top)
            );
        }
        for other in self
            .arch
            .openings
            .iter()
            .filter(|o| o.host == opening.host && !matches!(o.fill, Fill::Cut(_)))
        {
            let (a0, a1) = (other.at - other.w / 2.0, other.at + other.w / 2.0);
            let (b0, b1) = (other.sill, other.sill + other.h);
            let apart = s1 <= a0 + 1.0e-6
                || a1 <= s0 + 1.0e-6
                || opening.sill + opening.h <= b0 + 1.0e-6
                || b1 <= opening.sill + 1.0e-6;
            if !apart {
                bail!(
                    "`{}` overlaps `{}` in `{}`",
                    opening.name,
                    other.name,
                    wall.name
                );
            }
        }
        Ok(())
    }

    fn host_opening(&mut self, opening: Opening) -> Result<String> {
        self.check_opening(&opening)?;
        let text = format!(
            "{} in {} at {}, {} x {}{}",
            opening.name,
            opening.host,
            crate::parse::label(opening.at),
            crate::parse::label(opening.w),
            crate::parse::label(opening.h),
            opening
                .mark
                .as_ref()
                .map(|m| format!(", mark {m}"))
                .unwrap_or_default()
        );
        self.arch.openings.push(opening);
        self.rebuild_walls()?;
        Ok(text)
    }

    pub(crate) fn op_opening(&mut self, line: &Line) -> Result<String> {
        let name = crate::args::label_of(line);
        let mut words: Vec<&str> = line.positional.iter().map(String::as_str).collect();
        let mut opening = Opening {
            name,
            host: String::new(),
            at: f64::NAN,
            w: f64::NAN,
            h: f64::NAN,
            sill: if line.op == "window" { 900.0 } else { 0.0 },
            head: Head::Flat,
            mark: None,
            fill: Fill::Hole,
        };
        let mut swing = "left".to_string();
        let mut frame = None;
        let mut leaf = 44.0;
        let mut threshold = 0.0;
        let mut panes = (1, 1);
        let mut mullion = 40.0;
        let mut glazing = 24.0;
        let mut depth = 70.0;
        let mut panels = 4;
        let mut part = None;
        let mut vars = Vec::new();
        let op = line.op.as_str();
        for (key, text) in &line.named {
            let number =
                |model: &Model| eval(text, &model.scope).with_context(|| format!("`{key}={text}`"));
            match (op, key.as_str()) {
                (_, "on") => opening.host = text.clone(),
                (_, "at") => opening.at = number(self)?,
                (_, "w") => opening.w = number(self)?,
                (_, "h") => opening.h = number(self)?,
                (_, "sill") => opening.sill = number(self)?,
                (_, "mark") => opening.mark = Some(text.clone()),
                ("window" | "opening", "head") => {
                    opening.head = match text.as_str() {
                        "flat" => Head::Flat,
                        "round" => Head::Round,
                        "arch" => Head::Arch,
                        "segment" => Head::Segment,
                        other => bail!("head is flat, arch, segment or round, not `{other}`"),
                    }
                }
                ("door", "swing") => match text.as_str() {
                    "left" | "right" | "double" | "slide" | "fold" => swing = text.clone(),
                    other => bail!("swing is left, right, double, slide or fold, not `{other}`"),
                },
                ("door", "hinge") => {
                    if !matches!(text.as_str(), "left" | "right") {
                        bail!("hinge is left or right, not `{text}`");
                    }
                }
                ("door", "frame") => {
                    let (w, d) = text.split_once('x').ok_or_else(|| {
                        anyhow!("write a door frame as face x depth, like `frame=70x45`")
                    })?;
                    frame = Some((eval(w, &self.scope)?, eval(d, &self.scope)?));
                }
                ("window", "frame") => frame = Some((number(self)?, 0.0)),
                ("window", "depth") => depth = number(self)?,
                ("door", "leaf") => leaf = number(self)?,
                ("door", "threshold") => threshold = number(self)?,
                ("door" | "window", "part") => part = Some(self.relative(text)),
                ("window", "mullion") => mullion = number(self)?,
                ("window", "panes") => {
                    let (c, r) = text.split_once('x').ok_or_else(|| {
                        anyhow!("write panes as columns x rows, like `panes=2x1`")
                    })?;
                    let count = |t: &str| -> Result<usize> {
                        let v = eval(t, &self.scope)?;
                        if v < 1.0 || v.fract() != 0.0 || v > 50.0 {
                            bail!("pane counts are whole numbers from 1");
                        }
                        Ok(v as usize)
                    };
                    panes = (count(c)?, count(r)?);
                }
                ("window", "glazing") => {
                    glazing = match text.as_str() {
                        "single" => 6.0,
                        "double" => 24.0,
                        "triple" => 36.0,
                        other => bail!("glazing is single, double or triple, not `{other}`"),
                    }
                }
                ("garage_door", "panels") => {
                    let v = number(self)?;
                    if v < 1.0 || v.fract() != 0.0 {
                        bail!("panels is a whole number from 1");
                    }
                    panels = v as usize;
                }
                ("door" | "window", _)
                    if part.is_some() || line.named.iter().any(|(k, _)| k == "part") =>
                {
                    vars.push((key.clone(), number(self)?));
                }
                (_, other) => bail!("`{op}` has no `{other}=` argument"),
            }
        }
        let inward = match words.as_slice() {
            [] => true,
            ["in"] if op == "door" => true,
            ["out"] if op == "door" => false,
            _ => bail!(
                "`{op}` takes name=value arguments{}",
                if op == "door" { " and in or out" } else { "" }
            ),
        };
        words.clear();
        for (value, what) in [(opening.at, "at"), (opening.w, "w"), (opening.h, "h")] {
            if value.is_nan() {
                bail!("`{op}` needs `{what}=`");
            }
        }
        if opening.host.is_empty() {
            bail!("`{op}` needs `on=`, the wall it sits in");
        }
        if opening.w <= 0.0 || opening.h <= 0.0 {
            bail!("an opening needs w and h above zero");
        }
        opening.fill = match (op, part) {
            (_, Some(file)) => {
                let mut all = vec![("w".to_string(), opening.w), ("h".to_string(), opening.h)];
                all.extend(vars);
                Fill::Part { file, vars: all }
            }
            ("door", None) => {
                let frame = frame.unwrap_or((70.0, 45.0));
                if 2.0 * frame.0 >= opening.w || frame.0 >= opening.h {
                    bail!("the frame leaves no room for the leaf");
                }
                Fill::Door {
                    swing,
                    inward,
                    frame,
                    leaf,
                    threshold,
                }
            }
            ("window", None) => Fill::Window {
                panes,
                frame: frame.map_or(60.0, |f| f.0),
                depth,
                mullion,
                glass: glazing,
            },
            ("garage_door", None) => Fill::Garage { panels },
            _ => Fill::Hole,
        };
        self.host_opening(opening)
    }

    pub(crate) fn op_wall_cut(&mut self, line: &Line) -> Result<String> {
        let [host, shape, rest @ ..] = line.positional.as_slice() else {
            bail!(
                "write `wall_cut w1 rect 1000 500 at=2000,2450`, along the wall and up from its base"
            );
        };
        if !super::super::PROFILE_OPS.contains(&shape.as_str()) {
            bail!("`wall_cut` takes a profile like rect, circle or poly, not `{shape}`");
        }
        let inner = Line {
            number: line.number,
            text: line.text.clone(),
            label: None,
            op: shape.clone(),
            positional: rest.to_vec(),
            named: line.named.clone(),
        };
        let saved = std::mem::take(&mut self.sketch);
        let made = self.apply_op(&inner);
        let profiles = std::mem::replace(&mut self.sketch, saved);
        made?;
        let mut area = 0.0;
        for p in &profiles {
            area += signed_area(&outline(p)?).abs();
        }
        let name = crate::args::label_of(line);
        let opening = Opening {
            name: name.clone(),
            host: host.clone(),
            at: 0.0,
            w: 0.0,
            h: 0.0,
            sill: 0.0,
            head: Head::Flat,
            mark: None,
            fill: Fill::Cut(profiles),
        };
        self.host_opening(opening)?;
        Ok(format!(
            "{name} cuts {} mm² through every layer of {host}",
            crate::parse::label(area)
        ))
    }
}
