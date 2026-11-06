use crate::args::{Args, direction, point3, positive};
use crate::interp::Scene;
use crate::mesh::alpha::Alpha;
use crate::mesh::brush::Falloff;
use crate::mesh::stroke::{Aim, Brush, KINDS, Kind, Stroke};
use crate::mesh::{self, Mesh, P3, Region, Selection};
use crate::parse::{Line, Scope, eval};
use anyhow::{Result, anyhow, bail};
use std::sync::Arc;

fn axis_of(text: &str) -> Result<usize> {
    match text.to_ascii_lowercase().as_str() {
        "x" => Ok(0),
        "y" => Ok(1),
        "z" => Ok(2),
        other => bail!("mirror takes x, y or z, got `{other}`"),
    }
}

fn on_off(text: &str, what: &str) -> Result<bool> {
    match text {
        "on" | "yes" | "true" => Ok(true),
        "off" | "no" | "false" => Ok(false),
        other => bail!("`{what}` is on or off, got `{other}`"),
    }
}

pub fn falloff_of(text: &str, scope: &Scope) -> Result<Falloff> {
    Falloff::parse(text, |v| eval(v, scope))
}

fn fraction(value: f64, what: &str, lo: f64) -> Result<f64> {
    if (lo..=1.0).contains(&value) {
        Ok(value)
    } else {
        bail!("`{what}` goes from {lo} to 1, got {value}")
    }
}

const BRUSH_ARGS: &[&str] = &[
    "type", "size", "strength", "falloff", "focal", "edge", "spacing", "lazy", "alpha",
    "geodesic", "normal", "mirror", "detail", "pinch",
];

fn sphere_regions(scope: &Scope, args: &Args<'_>) -> Result<Vec<Region>> {
    let mut regions = Vec::new();
    if let Some(text) = args.values.get("sphere") {
        let c = point3(text, scope)?;
        let r = positive(args.number("r", scope)?, "r")?;
        regions.push(Region::Sphere(c, r));
    }
    if let Some(text) = args.values.get("box") {
        let v: Vec<f64> = crate::parse::split_top_level(text, ',')
            .iter()
            .map(|p| eval(p, scope))
            .collect::<Result<_>>()?;
        let [a, b, c, d, e, f] = v[..] else {
            bail!("`box` is x0,y0,z0,x1,y1,z1");
        };
        regions.push(Region::Box(
            [a.min(d), b.min(e), c.min(f)],
            [a.max(d), b.max(e), c.max(f)],
        ));
    }
    if let Some(text) = args.values.get("part") {
        for name in crate::parse::split_top_level(text, ',') {
            regions.push(Region::Part(name.trim().to_string()));
        }
    }
    Ok(regions)
}

impl Scene {
    fn brush_named(&self, name: &str) -> Result<Brush> {
        self.mesh_body
            .brushes
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, b)| b.clone())
            .ok_or_else(|| {
                let known: Vec<&str> = self
                    .mesh_body
                    .brushes
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect();
                anyhow!(
                    "there is no brush `{name}`; define it first with `brush {name} type=draw size=0.5`{}",
                    if known.is_empty() {
                        String::new()
                    } else {
                        format!(" (brushes so far: {})", known.join(", "))
                    }
                )
            })
    }

    pub fn op_brush(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["name"], BRUSH_ARGS, false)?;
        let name = args.text("name")?.to_string();
        let existing = self
            .mesh_body
            .brushes
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, b)| b.clone());
        let mut brush = match (existing, args.values.get("type")) {
            (_, Some(text)) => {
                let kind = Kind::parse(text)
                    .ok_or_else(|| anyhow!("brush type is {KINDS}, got `{text}`"))?;
                let size = match args.optional_number("size", scope)? {
                    Some(s) => positive(s, "size")?,
                    None => bail!("a new brush needs `size=`, its radius in model units"),
                };
                Brush::new(kind, size)
            }
            (Some(b), None) => b,
            (None, None) => bail!(
                "a new brush needs `type=` ({KINDS}) and `size=`, like `brush {name} type=clay size=0.4`"
            ),
        };
        if let Some(size) = args.optional_number("size", scope)? {
            let old = brush.size;
            brush.size = positive(size, "size")?;
            if !args.has("strength") && !args.has("type") {
                let default = brush.kind.default_strength(old);
                if (brush.strength - default).abs() < 1.0e-12 {
                    brush.strength = brush.kind.default_strength(brush.size);
                }
            }
        }
        if let Some(s) = args.optional_number("strength", scope)? {
            brush.strength = s;
        }
        if let Some(text) = args.values.get("falloff") {
            brush.profile.falloff = falloff_of(text, scope)?;
        }
        if let Some(v) = args.optional_number("focal", scope)? {
            brush.profile.focal = fraction(v, "focal", -1.0)?;
        }
        if let Some(v) = args.optional_number("edge", scope)? {
            brush.profile.edge = fraction(v, "edge", 0.0)?;
        }
        if let Some(v) = args.optional_number("spacing", scope)? {
            if !(0.01..=4.0).contains(&v) {
                bail!("`spacing` is a share of the brush size from 0.01 to 4, got {v}");
            }
            brush.spacing = v;
        }
        if let Some(v) = args.optional_number("lazy", scope)? {
            brush.lazy = fraction(v, "lazy", 0.0)?;
        }
        if let Some(v) = args.optional_number("pinch", scope)? {
            brush.pinch = fraction(v, "pinch", 0.0)?;
        }
        if let Some(text) = args.values.get("alpha") {
            brush.alpha = match *text {
                "none" => None,
                text => Some(Alpha::parse(text, |v| eval(v, scope))?),
            };
        }
        if let Some(text) = args.values.get("geodesic") {
            brush.geodesic = on_off(text, "geodesic")?;
        }
        if let Some(text) = args.values.get("mirror") {
            brush.mirror = match *text {
                "none" | "off" => None,
                t => Some(axis_of(t)?),
            };
        }
        if let Some(text) = args.values.get("normal") {
            brush.aim = match *text {
                "auto" | "area" => Aim::Area,
                "vertex" => Aim::Vertex,
                t => {
                    let n = direction(t, scope)?;
                    if mesh::length(n) < 1.0e-12 {
                        bail!("`normal` must not be zero");
                    }
                    Aim::Fixed(n)
                }
            };
        }
        if let Some(text) = args.values.get("detail") {
            brush.detail = match *text {
                "off" | "none" => None,
                t => Some(positive(eval(t, scope)?, "detail")?),
            };
        }
        let summary = format!(
            "brush `{name}`: {:?} size {:.3} strength {:.3}",
            brush.kind, brush.size, brush.strength
        )
        .to_lowercase();
        self.mesh_body.brushes.push((name, brush));
        Ok(summary)
    }

    fn effective(&self, mut brush: Brush) -> Brush {
        if brush.detail.is_none() {
            brush.detail = self.mesh_body.dyntopo;
        }
        brush
    }

    fn landing(&mut self, op: &str, at: P3) -> Result<f64> {
        let mesh = self.sculpted(op)?;
        let surface = mesh::grid::Surface::new(mesh);
        Ok(surface.closest(at).map_or(f64::INFINITY, |h| h.distance))
    }

    fn run_stroke(&mut self, op: &str, brush: Brush, stroke: Stroke) -> Result<String> {
        let off = self.landing(op, stroke.path[0])?;
        let brush = self.effective(brush);
        let outcome = self.sculpted(op)?.stroke(&brush, &stroke);
        let mesh = self.mesh_body.mesh.as_ref().expect("sculpted");
        let mut text = format!(
            "moved {} vertices, the most by {:.3}",
            outcome.moved, outcome.most
        );
        if outcome.added > 0 {
            text += &format!(", {} new vertices, {} triangles", outcome.added, mesh.triangles.len());
        }
        if off > brush.size * 0.5 && off.is_finite() {
            text += &format!("; started {off:.2} off the surface, put on the closest point");
        }
        if outcome.moved == 0 {
            text += "; nothing moved: check the mask, or that the brush is larger than the edges here";
        } else if let Some(note) = self.coarse_brush(&brush, &stroke) {
            text += &note;
        }
        Ok(text)
    }

    fn coarse_brush(&self, brush: &Brush, stroke: &Stroke) -> Option<String> {
        if brush.detail.is_some() {
            return None;
        }
        let mesh = self.mesh_body.mesh.as_ref()?;
        let regions = vec![Region::Sphere(stroke.path[0], brush.size)];
        let selection = Selection::new(mesh, regions, 0.0, None).ok()?;
        let (mean, _) = mesh.edge_lengths(&selection);
        (mean > brush.size * 0.5).then(|| {
            format!(
                "; edges here are {mean:.2} long for a {:.2} brush, so `dyntopo on detail={:.2}` or `refine` first",
                brush.size,
                brush.size / 4.0
            )
        })
    }

    pub fn op_dab(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["brush"], &["at", "to", "pressure"], false)?;
        let brush = self.brush_named(args.text("brush")?)?;
        let at = point3(args.text("at")?, scope)?;
        let pressure = args
            .optional_number("pressure", scope)?
            .map(|p| vec![p])
            .unwrap_or_default();
        let to = args
            .values
            .get("to")
            .map(|t| point3(t, scope))
            .transpose()?;
        self.run_stroke(
            "dab",
            brush,
            Stroke {
                path: vec![at],
                pressure,
                to,
            },
        )
    }

    pub fn op_stroke(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["brush"], &["from", "to", "path", "pressure"], true)?;
        let brush = self.brush_named(args.text("brush")?)?;
        let mut path: Vec<P3> = Vec::new();
        if let Some(text) = args.values.get("from") {
            path.push(point3(text, scope)?);
        }
        if let Some(text) = args.values.get("path") {
            path.push(point3(text, scope)?);
        }
        for text in &args.rest {
            path.push(point3(text, scope)?);
        }
        let to = args
            .values
            .get("to")
            .map(|t| point3(t, scope))
            .transpose()?;
        let snake_or_move = matches!(brush.kind, Kind::Move | Kind::SnakeHook);
        let to = match to {
            Some(t) if !snake_or_move => {
                path.push(t);
                None
            }
            other => other,
        };
        if path.is_empty() || (path.len() < 2 && to.is_none()) {
            bail!(
                "a stroke needs a path: `stroke b from=x,y,z to=x,y,z` or points `stroke b x,y,z x,y,z ...`"
            );
        }
        let pressure: Vec<f64> = match args.values.get("pressure") {
            Some(text) => crate::parse::split_top_level(text, ',')
                .iter()
                .map(|v| eval(v, scope))
                .collect::<Result<_>>()?,
            None => Vec::new(),
        };
        if pressure.iter().any(|p| !(0.0..=4.0).contains(p)) {
            bail!("pressure values go from 0 to 4, got {pressure:?}");
        }
        self.run_stroke("stroke", brush, Stroke { path, pressure, to })
    }

    pub fn op_mask(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["action", "amount"],
            &["sphere", "r", "box", "part", "by", "axis", "from", "to", "falloff", "strength"],
            false,
        )?;
        let mesh = self.sculpted("mask")?;
        let action = args.values.get("action").copied();
        let amount = args.optional_number("amount", scope)?;
        match action {
            Some("clear") => {
                mesh.mask.clear();
            }
            Some("invert") => mesh.mask_invert(),
            Some("blur") => {
                let passes = amount.unwrap_or(1.0);
                if passes < 1.0 || passes.fract() != 0.0 || passes > 100.0 {
                    bail!("`mask blur` takes a whole number of passes up to 100, got {passes}");
                }
                mesh.mask_blur(passes as usize);
            }
            Some("grow") | Some("shrink") => {
                let by = positive(
                    amount.ok_or_else(|| anyhow!("say how far, like `mask grow 0.5`"))?,
                    "distance",
                )?;
                if action == Some("grow") {
                    mesh.mask_grow(by);
                } else {
                    mesh.mask_shrink(by);
                }
            }
            Some(other) => bail!(
                "`mask` takes sphere=/box=/part=/by=, or clear, invert, blur N, grow D, shrink D; got `{other}`"
            ),
            None => {
                let strength = args.optional_number("strength", scope)?.unwrap_or(1.0);
                let strength = fraction(strength, "strength", 0.0)?;
                let values: Vec<f64> = match args.values.get("by").copied() {
                    Some("height") => {
                        let axis = direction(args.values.get("axis").copied().unwrap_or("z"), scope)?;
                        let from = args.number("from", scope)?;
                        let to = args.number("to", scope)?;
                        mesh.height_values(mesh::normalized(axis), from, to)
                    }
                    Some(kind @ ("cavity" | "curvature")) => {
                        let r = args.optional_number("r", scope)?.unwrap_or(1.0);
                        let r = positive(r, "r")?;
                        mesh.curvature()
                            .into_iter()
                            .map(|k| {
                                let k = if kind == "cavity" { -k } else { k.abs() };
                                (k * r).clamp(0.0, 1.0)
                            })
                            .collect()
                    }
                    Some(other) => bail!("`mask by=` is height, cavity or curvature, got `{other}`"),
                    None => {
                        let regions = sphere_regions(scope, &args)?;
                        if regions.is_empty() {
                            bail!(
                                "say what to mask: `sphere=x,y,z r=`, `box=`, `part=`, `by=height|cavity|curvature`, or clear, invert, blur, grow, shrink"
                            );
                        }
                        let falloff = args.optional_number("falloff", scope)?.unwrap_or(0.0);
                        let weighting = mesh::Weighting {
                            masked: false,
                            ..Default::default()
                        };
                        let plain = Mesh {
                            mask: Vec::new(),
                            ..mesh.clone()
                        };
                        Selection::weighted(&plain, regions, falloff, None, weighting)?.weights
                    }
                };
                mesh.mask_with(&values, strength);
            }
        }
        let mesh = self.mesh_body.mesh.as_ref().expect("sculpted");
        Ok(format!(
            "{} of {} vertices masked",
            mesh.mask_count(),
            mesh.positions.len()
        ))
    }

    fn part_ids(&mut self, op: &str, names: &[&str]) -> Result<Vec<u16>> {
        let mesh = self.sculpted(op)?;
        names
            .iter()
            .flat_map(|n| crate::parse::split_top_level(n, ','))
            .map(|name| {
                let name = name.trim();
                mesh.parts
                    .iter()
                    .position(|p| p == name)
                    .map(|i| i as u16)
                    .ok_or_else(|| {
                        anyhow!(
                            "the mesh has no part `{name}`; its parts are {}",
                            mesh.parts.join(", ")
                        )
                    })
            })
            .collect()
    }

    pub fn op_hide(&mut self, _scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &[], true)?;
        if args.rest.is_empty() && line.op != "show" {
            bail!("say which parts, like `{} head,ear_l`", line.op);
        }
        let ids = self.part_ids(&line.op, &args.rest)?;
        let mesh = self.sculpted(&line.op)?;
        match line.op.as_str() {
            "hide" => {
                for id in ids {
                    if !mesh.hidden.contains(&id) {
                        mesh.hidden.push(id);
                    }
                }
            }
            "isolate" => {
                mesh.hidden = (0..mesh.parts.len() as u16)
                    .filter(|i| !ids.contains(i))
                    .collect();
            }
            _ if ids.is_empty() => mesh.hidden.clear(),
            _ => mesh.hidden.retain(|h| !ids.contains(h)),
        }
        let hidden: Vec<&str> = mesh
            .hidden
            .iter()
            .map(|&i| mesh.parts[i as usize].as_str())
            .collect();
        Ok(if hidden.is_empty() {
            "every part can be sculpted".to_string()
        } else {
            format!("held still: {}", hidden.join(", "))
        })
    }

    pub fn op_group(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["name"], &["sphere", "r", "box", "part"], false)?;
        let name = args.text("name")?.to_string();
        let regions = sphere_regions(scope, &args)?;
        if regions.is_empty() {
            bail!("say where the group is: `group {name} sphere=x,y,z r=` or `box=`");
        }
        let mesh = self.sculpted("group")?;
        let weighting = mesh::Weighting {
            masked: false,
            ..Default::default()
        };
        let selection = Selection::weighted(mesh, regions, 0.0, None, weighting)?;
        let id = mesh.part_index(&name);
        let mut count = 0;
        for (label, w) in mesh.labels.iter_mut().zip(&selection.weights) {
            if *w >= 1.0 {
                *label = id;
                count += 1;
            }
        }
        Ok(format!("{count} vertices in group `{name}`"))
    }

    pub fn op_join(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["a", "b"], &["blend", "falloff"], false)?;
        let blend = positive(args.number("blend", scope)?, "blend")?;
        let profile = match args.values.get("falloff") {
            Some(text) => falloff_of(text, scope)?,
            None => Falloff::Smooth,
        };
        let ids = self.part_ids("join", &[args.text("a")?, args.text("b")?])?;
        let [a, b] = ids[..] else {
            bail!("`join` takes two parts");
        };
        let mesh = self.sculpted("join")?;
        let before = mesh.volume();
        let moved = mesh.join_parts(a, b, blend, &profile);
        Ok(format!(
            "blended {moved} vertices across the seam; volume {:.3} (was {before:.3})",
            mesh.volume()
        ))
    }

    pub fn op_dyntopo(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["state"], &["detail"], false)?;
        let on = on_off(args.text("state")?, "dyntopo")?;
        self.mesh_body.dyntopo = if on {
            let detail = args
                .optional_number("detail", scope)?
                .ok_or_else(|| anyhow!("`dyntopo on` needs `detail=`, the edge length to keep under the brush"))?;
            Some(positive(detail, "detail")?)
        } else {
            None
        };
        Ok(match self.mesh_body.dyntopo {
            Some(d) => format!("brushes refine to {d:.3} edges where they work"),
            None => "brushes keep the triangles they find".to_string(),
        })
    }

    pub fn op_subdivide(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["levels"], &[], false)?;
        let levels = args.optional_number("levels", scope)?.unwrap_or(1.0);
        if !(1.0..=4.0).contains(&levels) || levels.fract() != 0.0 {
            bail!("`subdivide` takes 1 to 4 levels, got {levels}");
        }
        let mesh = self.sculpted("subdivide")?;
        let before = mesh.triangles.len();
        mesh.subdivide(levels as usize);
        if mesh.triangles.len() > 8_000_000 {
            bail!(
                "`subdivide {levels}` makes {} triangles; subdivide less or `refine` a region",
                mesh.triangles.len()
            );
        }
        Ok(format!(
            "{} triangles (was {before}); volume {:.3}",
            mesh.triangles.len(),
            mesh.volume()
        ))
    }

    pub fn op_level(&mut self, _scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["name"], &[], false)?;
        let name = args.text("name")?.to_string();
        let mesh = Arc::new(self.sculpted("level")?.clone());
        let tris = mesh.triangles.len();
        self.mesh_body.levels.retain(|(n, _)| *n != name);
        self.mesh_body.levels.push((name.clone(), mesh));
        Ok(format!("level `{name}` keeps this {tris}-triangle surface"))
    }

    pub fn op_project(&mut self, _scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["level"], &[], false)?;
        let name = args.text("level")?;
        let target = self
            .mesh_body
            .levels
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, m)| m.clone())
            .ok_or_else(|| {
                anyhow!(
                    "there is no level `{name}`; keep one first with `level {name}`{}",
                    if self.mesh_body.levels.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " (levels: {})",
                            self.mesh_body
                                .levels
                                .iter()
                                .map(|(n, _)| n.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                )
            })?;
        let selection = self.selection_for_brush("project")?;
        let (moved, most) = self.sculpted("project")?.project_onto(&target, &selection);
        Ok(format!(
            "projected {moved} vertices onto level `{name}`, the most by {most:.3}"
        ))
    }

    pub fn op_layer(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["name"], &["amount"], false)?;
        let name = args.text("name")?.to_string();
        let mesh = self.sculpted("layer")?;
        match args.optional_number("amount", scope)? {
            None => {
                mesh.start_layer(&name)?;
                Ok(format!(
                    "layer `{name}` records the sculpting from here; dial it with `layer {name} amount=0.5`"
                ))
            }
            Some(amount) => {
                if !(-4.0..=4.0).contains(&amount) {
                    bail!("a layer amount goes from -4 to 4, got {amount}");
                }
                let most = mesh.set_layer(&name, amount)?;
                Ok(format!("layer `{name}` at {amount}, moved the most by {most:.3}"))
            }
        }
    }

    pub fn op_finish(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &["decimate", "max_error", "keep", "focus", "weight"], false)?;
        let mesh = self.sculpted("finish")?;
        let before = mesh.triangles.len();
        let target = match args.values.get("decimate") {
            Some(text) => Some(match text.strip_suffix('%') {
                Some(share) => {
                    let share = eval(share, scope)?;
                    if !(share > 0.0 && share < 100.0) {
                        bail!("keep between 0% and 100% of the triangles, got {share}%");
                    }
                    (before as f64 * share / 100.0).round() as usize
                }
                None => {
                    let n = eval(text, scope)?;
                    if n < 4.0 || n.fract() != 0.0 {
                        bail!("`finish decimate=` is a whole number of triangles or a share like 10%");
                    }
                    n as usize
                }
            }),
            None => None,
        };
        let max_error = args
            .optional_number("max_error", scope)?
            .map(|e| positive(e, "max_error"))
            .transpose()?;
        if target.is_none() && max_error.is_none() {
            bail!("say how far to go: `finish decimate=20000`, `finish decimate=10%` or `finish max_error=0.01`");
        }
        let keep = args.values.get("keep").copied().unwrap_or("parts");
        for k in keep.split(',') {
            if !matches!(k.trim(), "parts" | "creases" | "colours" | "colors") {
                bail!("`keep` lists parts, creases or colours, got `{k}`");
            }
        }
        if self.mesh_body.rig.skin.is_some() {
            bail!("`finish` goes before `skin`: decimate first, then bind the skin to the final triangles");
        }
        let focus: Vec<&str> = args
            .values
            .get("focus")
            .map(|t| vec![*t])
            .unwrap_or_default();
        let weight = args.optional_number("weight", scope)?.unwrap_or(10.0);
        if !(1.0..=1000.0).contains(&weight) {
            bail!("`weight` goes from 1 to 1000, got {weight}");
        }
        if args.has("weight") && focus.is_empty() {
            bail!("`weight=` goes with `focus=parts`, the parts to keep finest");
        }
        let ids = self.part_ids("finish", &focus)?;
        let mesh = self.sculpted("finish")?;
        let importance: Vec<f64> = if ids.is_empty() {
            Vec::new()
        } else {
            mesh.labels
                .iter()
                .map(|l| if ids.contains(l) { weight } else { 1.0 })
                .collect()
        };
        let reference = mesh.clone();
        Ok(mesh.finish_weighted(&reference, target, max_error, &importance))
    }

    fn selection_for_brush(&mut self, op: &str) -> Result<Selection> {
        let mesh = self.sculpted(op)?.clone();
        match &self.mesh_body.selection {
            Some(s) => s.again(&mesh),
            None => Selection::new(&mesh, vec![Region::All], 0.0, None),
        }
    }
}
