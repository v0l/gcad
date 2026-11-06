use crate::args::{Args, axis, colour_of, point3, positive};
use crate::interp::Scene;
use crate::parse::{Line, Scope};
use anyhow::{Result, bail};
use std::collections::HashMap;

pub fn copy_flag(args: &Args<'_>) -> Result<bool> {
    match args.values.get("copy").copied() {
        None => Ok(false),
        Some("copy") => Ok(true),
        Some(other) => bail!("`{other}` is not an option; did you mean `copy`?"),
    }
}

pub fn opacity_of(args: &Args, scope: &Scope) -> Result<f64> {
    let opacity = args.optional_number("opacity", scope)?.unwrap_or(1.0);
    if !(opacity > 0.0 && opacity <= 1.0) {
        bail!("opacity must be above 0 and at most 1, got {opacity}");
    }
    Ok(opacity)
}

impl Scene {
    pub fn op_move(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["by", "copy"], &[], false)?;
        let by = point3(args.text("by")?, scope)?;
        if copy_flag(&args)? {
            bail!("`move copy` copies solids; a mesh moves its selection");
        }
        self.mesh_move(by)
    }

    pub fn op_rotate(
        &mut self,
        scope: &Scope,
        line: &Line,
        datum: impl Fn(&str) -> Option<(crate::mesh::P3, crate::mesh::P3)>,
    ) -> Result<String> {
        let args = Args::new(line, &["angle", "copy"], &["axis", "about"], false)?;
        let copy = copy_flag(&args)?;
        let angle = args.number("angle", scope)?;
        let named = args.values.get("axis").and_then(|name| datum(name));
        let direction = match named {
            Some((_, direction)) => direction,
            None => axis(args.values.get("axis").copied().unwrap_or("z"))?,
        };
        let centre = match (args.values.get("about"), named) {
            (Some(_), Some(_)) => {
                bail!("a datum axis already passes through a point; drop `about=`")
            }
            (Some(text), None) => Some(point3(text, scope)?),
            (None, Some((through, _))) => Some(through),
            (None, None) => None,
        };
        if copy {
            bail!("`rotate copy` copies solids; a mesh turns its selection");
        }
        self.mesh_rotate(direction, angle, centre)
    }

    pub fn op_scale(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["factor"], &["about"], false)?;
        let text = args.text("factor")?;
        let factors = if text.contains(',') {
            point3(text, scope)?
        } else {
            [args.number("factor", scope)?; 3]
        };
        factors
            .iter()
            .try_for_each(|f| positive(*f, "scale factor").map(|_| ()))?;
        let given = args
            .values
            .get("about")
            .map(|text| point3(text, scope))
            .transpose()?;
        self.mesh_scale(factors, given)
    }

    pub fn op_mirror(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["on"], &["offset", "of"], false)?;
        let on = args.text("on")?;
        let axis = match on.to_ascii_lowercase().as_str() {
            "x" | "yz" => 0,
            "y" | "xz" => 1,
            "z" | "xy" => 2,
            other => {
                bail!("a mesh mirrors across x, y or z (the YZ, XZ or XY plane), got `{other}`")
            }
        };
        let at = args.optional_number("offset", scope)?.unwrap_or(0.0);
        self.mesh_mirror(axis, at, args.values.get("of").copied())
    }

    pub fn op_color(
        &mut self,
        scope: &Scope,
        line: &Line,
        colours: &mut HashMap<String, [f64; 3]>,
    ) -> Result<String> {
        let args = Args::new(
            line,
            &["colour"],
            &["opacity", "part", "gloss", "soft"],
            false,
        )?;
        let colour = colour_of(args.text("colour")?, scope)?;
        opacity_of(&args, scope)?;
        let gloss = args.optional_number("gloss", scope)?;
        if let Some(g) = gloss
            && !(0.0..=1.0).contains(&g)
        {
            bail!("gloss goes from 0 (matte) to 1 (wet), got {g}");
        }
        let soft = args.optional_number("soft", scope)?;
        if let Some(s) = soft
            && s < 0.0
        {
            bail!("soft must be 0 or more, got {s}");
        }
        if let Some(part) = args.values.get("part") {
            return self.colour_mesh_part(part, colour, gloss, soft);
        }
        if gloss.is_some() || soft.is_some() {
            bail!("`gloss=` and `soft=` go with `part=` on a mesh body");
        }
        let name = self.body_name();
        colours.insert(name.clone(), colour);
        Ok(format!(
            "body `{name}` is {:.2},{:.2},{:.2}",
            colour[0], colour[1], colour[2]
        ))
    }
}

pub fn guard(line: &Line, scope: &Scope) -> Result<std::result::Result<Line, String>> {
    let text = line.text.trim_start();
    let text = match &line.label {
        Some(label) => text.trim_start_matches(&format!("{label}:")).trim_start(),
        None => text,
    };
    let rest = text
        .strip_prefix("if")
        .ok_or_else(|| anyhow::anyhow!("`if` must start the line"))?
        .trim_start();
    let (condition, inner) = rest
        .split_once(char::is_whitespace)
        .ok_or_else(|| anyhow::anyhow!("write `if <condition> <operation ...>`"))?;
    let holds = crate::parse::eval(condition, scope)?;
    if holds == 0.0 {
        return Ok(Err(condition.to_string()));
    }
    let mut inner = crate::parse::parse_line(line.number, inner.trim())?;
    if inner.label.is_none() {
        inner.label = line.label.clone();
    }
    Ok(Ok(inner))
}
