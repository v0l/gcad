use crate::args::{Args, label_of, positive};
use crate::interp::Scene;
use crate::mesh::Mesh;
use crate::parse::{Line, Scope, eval_point};
use crate::solid::{self, Extrude, Frame, P2, Torus};
use anyhow::{Result, anyhow, bail};
use std::sync::Arc;

pub const OPS: &[&str] = &[
    "plane", "rect", "circle", "ellipse", "ngon", "torus", "poly", "array", "extrude", "fillet",
    "repeat",
];

pub struct Solids {
    frame: Frame,
    sketch: Vec<Vec<P2>>,
    last: Option<Vec<P2>>,
    extrudes: Vec<Extrude>,
    tori: Vec<Torus>,
}

impl Default for Solids {
    fn default() -> Self {
        Solids {
            frame: Frame::named("XY").expect("world plane"),
            sketch: Vec::new(),
            last: None,
            extrudes: Vec::new(),
            tori: Vec::new(),
        }
    }
}

impl Solids {
    pub fn new_body(&mut self) {
        self.extrudes.clear();
        self.tori.clear();
        self.sketch.clear();
        self.last = None;
    }

    pub fn apply(&mut self, scene: &mut Scene, scope: &Scope, line: &Line) -> Result<()> {
        match line.op.as_str() {
            "plane" => self.plane(scope, line),
            "rect" => self.rect(scope, line),
            "circle" => self.circle(scope, line),
            "ellipse" => self.ellipse(scope, line),
            "ngon" => self.ngon(scope, line),
            "torus" => self.torus(scene, scope, line),
            "poly" => self.poly(scope, line),
            "array" => self.array(scope, line),
            "extrude" => self.extrude(scene, scope, line),
            "fillet" => self.fillet(scene, scope, line),
            "repeat" => self.repeat(scene, scope, line),
            other => bail!("`{other}` is not a solid operation"),
        }
    }

    fn plane(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        if !self.sketch.is_empty() {
            bail!(
                "`plane` must come before the profiles it holds, or after `extrude` empties them"
            );
        }
        let args = Args::new(line, &["on"], &["offset"], false)?;
        let name = args.text("on")?;
        let frame = Frame::named(name)
            .ok_or_else(|| anyhow!("gcad-mesh planes are XY, XZ or YZ, got `{name}`"))?;
        let offset = args.optional_number("offset", scope)?.unwrap_or(0.0);
        self.frame = frame.offset(offset);
        Ok(())
    }

    fn add(&mut self, points: Vec<P2>) {
        self.last = Some(points.clone());
        self.sketch.push(points);
    }

    fn rect(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let args = Args::new(line, &["w", "h"], &["at", "r"], false)?;
        let width = positive(args.number("w", scope)?, "w")?;
        let height = positive(args.number("h", scope)?, "h")?;
        let radius = args.optional_number("r", scope)?.unwrap_or(0.0);
        if radius < 0.0 || radius * 2.0 >= width.min(height) {
            bail!(
                "r={radius} must be at least 0 and under half the shorter side ({})",
                width.min(height) / 2.0
            );
        }
        let centre = args.point("at", scope)?;
        self.add(solid::rect(centre, width, height, radius));
        Ok(())
    }

    fn circle(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let args = Args::new(line, &["d"], &["at"], false)?;
        let diameter = positive(args.number("d", scope)?, "d")?;
        let centre = args.point("at", scope)?;
        self.add(solid::circle(centre, diameter));
        Ok(())
    }

    fn ellipse(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let args = Args::new(line, &["dx", "dy"], &["at"], false)?;
        let width = positive(args.number("dx", scope)?, "dx")?;
        let height = positive(args.number("dy", scope)?, "dy")?;
        let centre = args.point("at", scope)?;
        self.add(solid::ellipse(centre, width, height));
        Ok(())
    }

    fn ngon(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let args = Args::new(line, &["d", "n"], &["at", "angle"], false)?;
        let diameter = positive(args.number("d", scope)?, "d")?;
        let sides = args.number("n", scope)?;
        if sides < 3.0 || sides.fract() != 0.0 {
            bail!("n must be a whole number of at least 3, got {sides}");
        }
        let centre = args.point("at", scope)?;
        let angle = args.optional_number("angle", scope)?.unwrap_or(0.0);
        self.add(solid::ngon(centre, diameter, sides as usize, angle));
        Ok(())
    }

    fn torus(&mut self, scene: &mut Scene, scope: &Scope, line: &Line) -> Result<()> {
        let args = Args::new(line, &["d", "tube"], &["at", "around", "sides"], false)?;
        let ring = positive(args.number("d", scope)?, "d")?;
        let tube = positive(args.number("tube", scope)?, "tube")?;
        if tube >= ring {
            bail!("tube {tube} must be thinner than the ring diameter {ring}");
        }
        let whole = |name: &str, default: usize| -> Result<usize> {
            match args.optional_number(name, scope)? {
                None => Ok(default),
                Some(n) if n >= 3.0 && n.fract() == 0.0 => Ok(n as usize),
                Some(n) => bail!("{name} is a whole number of at least 3, got {n}"),
            }
        };
        let around = whole("around", 24)?;
        let sides = whole("sides", 8)?;
        let centre = args.point("at", scope)?;
        self.require_solid_body(scene)?;
        if self.extrudes.iter().any(|e| e.open) {
            bail!("an `open=1` wall goes in its own body; start one with `body name`");
        }
        let label = label_of(line);
        if self.tori.iter().any(|t| t.label == label)
            || self.extrudes.iter().any(|e| e.label == label)
        {
            bail!("there is already a shape called `{label}`");
        }
        self.tori.push(Torus {
            label,
            frame: self.frame,
            centre,
            ring,
            tube,
            around,
            sides,
        });
        self.publish(scene);
        Ok(())
    }

    fn poly(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        if !line.named.is_empty() {
            bail!("gcad-mesh `poly` takes points only, without `r=` or `c=`");
        }
        let points = line
            .positional
            .iter()
            .map(|text| eval_point(text, scope))
            .collect::<Result<Vec<_>>>()?;
        if points.len() < 3 {
            bail!("`poly` needs at least three x,y points");
        }
        self.add(points);
        Ok(())
    }

    fn array(&mut self, scope: &Scope, line: &Line) -> Result<()> {
        let args = Args::new(line, &["count"], &["angle", "at", "step"], false)?;
        let count = args.number("count", scope)?;
        if count < 2.0 || count.fract() != 0.0 {
            bail!("count must be a whole number of at least 2, got {count}");
        }
        let count = count as usize;
        let last = self
            .last
            .clone()
            .ok_or_else(|| anyhow!("`array` copies the last profile and there is none"))?;
        match (
            args.optional_number("angle", scope)?,
            args.optional_point("step", scope)?,
        ) {
            (None, Some((sx, sy))) => {
                for i in 1..count {
                    let k = i as f64;
                    self.sketch
                        .push(solid::placed(&last, [1.0, 0.0, 0.0, 1.0, sx * k, sy * k]));
                }
            }
            (Some(angle), None) => {
                let (cx, cy) = args.point("at", scope)?;
                let step = if (angle.abs() - 360.0).abs() < 1.0e-9 {
                    angle / count as f64
                } else {
                    angle / (count - 1) as f64
                };
                for i in 1..count {
                    let (s, c) = (step * i as f64).to_radians().sin_cos();
                    self.sketch.push(solid::placed(
                        &last,
                        [c, -s, s, c, cx - (c * cx - s * cy), cy - (s * cx + c * cy)],
                    ));
                }
            }
            _ => bail!("`array` needs `angle=` (with optional `at=`) or `step=x,y`"),
        }
        Ok(())
    }

    fn require_solid_body(&self, scene: &Scene) -> Result<()> {
        if self.extrudes.is_empty() && self.tori.is_empty() && scene.has_mesh() {
            bail!(
                "`extrude` adds to a body of extrusions; this body already holds a sculpted mesh"
            );
        }
        Ok(())
    }

    fn extrude(&mut self, scene: &mut Scene, scope: &Scope, line: &Line) -> Result<()> {
        let args = Args::new(line, &["d"], &["scale", "open"], false)?;
        let depth = args.number("d", scope)?;
        let scale = match args.optional_number("scale", scope)? {
            Some(scale) if scale >= 0.0 => scale,
            Some(scale) => bail!("scale is 0 (a point) or more, got {scale}"),
            None => 1.0,
        };
        if depth.abs() < 1.0e-9 {
            bail!("`extrude` needs a distance other than 0");
        }
        if self.sketch.is_empty() {
            bail!("`extrude` needs a sketch; add rect, circle or poly first");
        }
        self.require_solid_body(scene)?;
        let label = label_of(line);
        if self.extrudes.iter().any(|e| e.label == label) {
            bail!("there is already an extrude called `{label}`");
        }
        let regions = solid::regions(&self.sketch);
        if scale < 1.0e-9 && regions.iter().any(|r| !r.holes.is_empty()) {
            bail!("`scale=0` makes a cone from a profile without holes");
        }
        let open = match args.optional_number("open", scope)? {
            None => false,
            Some(v) if v == 0.0 => false,
            Some(v) if v == 1.0 => true,
            Some(v) => bail!("give `open=1` for a wall without caps, got open={v}"),
        };
        if open && regions.iter().any(|r| !r.holes.is_empty()) {
            bail!("an `open=1` wall is one loop: give a profile without holes");
        }
        let shapes = self.extrudes.len() + self.tori.len();
        if (open && shapes > 0) || self.extrudes.iter().any(|e| e.open) {
            bail!("an `open=1` wall goes in its own body; start one with `body name`");
        }
        self.sketch.clear();
        self.last = None;
        self.extrudes.push(Extrude {
            label,
            frame: self.frame,
            regions,
            depth,
            start_round: 0.0,
            end_round: 0.0,
            copies: Vec::new(),
            scale,
            open,
        });
        self.publish(scene);
        Ok(())
    }

    fn fillet(&mut self, scene: &mut Scene, scope: &Scope, line: &Line) -> Result<()> {
        let args = Args::new(line, &["r", "edges"], &[], false)?;
        let radius = positive(args.number("r", scope)?, "r")?;
        let selector = args.text("edges")?;
        let mut label: Option<&str> = None;
        let mut groups: Vec<&str> = Vec::new();
        for piece in selector.split('&') {
            let (name, group) = piece.split_once('.').ok_or_else(|| {
                anyhow!("gcad-mesh fillets edges written `label.end&label.side`, got `{selector}`")
            })?;
            if label.is_some_and(|l| l != name) {
                bail!("a fillet selector names one extrude, got `{selector}`");
            }
            label = Some(name);
            groups.push(group);
        }
        let label = label.ok_or_else(|| anyhow!("`fillet` needs an edge selector"))?;
        let extrude = self
            .extrudes
            .iter_mut()
            .find(|e| e.label == label)
            .ok_or_else(|| anyhow!("no extrude called `{label}` in this body"))?;
        if extrude.open {
            bail!("`{label}` is an `open=1` wall with no rim edges to fillet");
        }
        if !groups.contains(&"side") {
            bail!("gcad-mesh fillets only the rim of an extrude: `{label}.end&{label}.side`");
        }
        match (groups.contains(&"start"), groups.contains(&"end")) {
            (false, true) => extrude.end_round = radius,
            (true, false) => extrude.start_round = radius,
            _ => bail!("a fillet rounds `{label}.start` or `{label}.end` against its side"),
        }
        if extrude.start_round + extrude.end_round > extrude.depth.abs() * (1.0 + 1.0e-9) {
            bail!(
                "fillet r={radius} does not fit in an extrude {} thick",
                extrude.depth.abs()
            );
        }
        self.publish(scene);
        Ok(())
    }

    fn repeat(&mut self, scene: &mut Scene, scope: &Scope, line: &Line) -> Result<()> {
        let args = Args::new(line, &["of"], &["count", "step"], false)?;
        let of = args.text("of")?;
        let count = args.number("count", scope)?;
        if count < 2.0 || count.fract() != 0.0 {
            bail!("count must be a whole number of at least 2, got {count}");
        }
        let (sx, sy) = args
            .optional_point("step", scope)?
            .ok_or_else(|| anyhow!("`repeat` needs `step=x,y`"))?;
        let step = self.frame.plane_vector(sx, sy);
        let extrude = self
            .extrudes
            .iter_mut()
            .find(|e| e.label == of)
            .ok_or_else(|| anyhow!("`{of}` made no extrusion to repeat in this body"))?;
        for k in 1..count as usize {
            extrude.copies.push(crate::mesh::scale(step, k as f64));
        }
        self.publish(scene);
        Ok(())
    }

    fn publish(&self, scene: &mut Scene) {
        let mut mesh = Mesh::default();
        for extrude in &self.extrudes {
            solid::extrude_into(&mut mesh, extrude);
        }
        for torus in &self.tori {
            solid::torus_into(&mut mesh, torus);
        }
        scene.mesh_body.mesh = Some(Arc::new(mesh));
        let name = scene.body_name();
        if !scene.solid_bodies.contains(&name) {
            scene.solid_bodies.push(name);
        }
    }
}
