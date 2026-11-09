use super::Flight;
use crate::Model;
use crate::member::straight_member;
use crate::parse::{Line, eval};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::stair::{parse_ratio, ramp_parts, rung_count};
use monstertruck::modeling::*;

const RAMP_SLAB: f64 = 200.0;
const RAIL: &str = "60x20";
const RUNG: &str = "25";

fn spacing_of(text: &str, scope: &crate::parse::Scope, key: &str) -> Result<f64> {
    let value = text
        .strip_prefix('@')
        .ok_or_else(|| anyhow!("`{key}=` is a spacing like `@300`"))?;
    let spacing = eval(value, scope).with_context(|| format!("`{key}={text}`"))?;
    if spacing <= 0.0 {
        bail!("`{key}=` spacing must be above zero");
    }
    Ok(spacing)
}

impl Model {
    pub(crate) fn op_ramp(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a ramp needs a label, like `rp1: ramp from=0,0,0 to=12000,0,600 slope=1:12`")
        })?;
        self.check_free_name(&name)?;
        if !line.positional.is_empty() {
            bail!("`ramp` takes only named values, like `from=0,0,0 to=12000,0,600`");
        }
        let (mut from, mut to) = (None, None);
        let (mut slope, mut width, mut landing, mut every) = (1.0 / 12.0, 1200.0, 1500.0, 0.0);
        let mut material = "concrete".to_string();
        for (key, text) in &line.named {
            let value = || eval(text, &self.scope).with_context(|| format!("`{key}={text}`"));
            match key.as_str() {
                "from" => from = Some(crate::args::point3(text, &self.scope)?),
                "to" => to = Some(crate::args::point3(text, &self.scope)?),
                "slope" => slope = parse_ratio(text).map_err(|e| anyhow!(e))?,
                "width" => width = value()?,
                "landing" => landing = value()?,
                "every" => every = value()?,
                "material" => material = text.clone(),
                other => bail!(
                    "`ramp` takes from=, to=, slope=, width=, landing=, every= and material=, not `{other}=`"
                ),
            }
        }
        let (from, to) = match (from, to) {
            (Some(a), Some(b)) => (a, b),
            _ => bail!("a ramp needs `from=x,y,z` and `to=x,y,z`"),
        };
        if width <= 0.0 || landing <= 0.0 {
            bail!("a ramp's width= and landing= must be above zero");
        }
        let plan = (to.x - from.x, to.y - from.y);
        let length = plan.0.hypot(plan.1);
        let parts = ramp_parts(length, to.z - from.z, slope, landing, every)
            .map_err(|e| anyhow!(e))?;
        let dir = if length > 0.0 {
            (plan.0 / length, plan.1 / length)
        } else {
            (1.0, 0.0)
        };
        let axis = Flight {
            start: (from.x, from.y),
            dir,
            z0: from.z,
            treads: 0,
        };
        self.ensure_library()?;
        let (mut runs, mut landings) = (0, 0);
        for part in &parts {
            let profile = vec![
                (part.s0, part.z0 - RAMP_SLAB),
                (part.s1, part.z1 - RAMP_SLAB),
                (part.s1, part.z1),
                (part.s0, part.z0),
            ];
            let solid = axis
                .slab(&profile, -width / 2.0, width / 2.0)
                .with_context(|| format!("building `{name}`"))?;
            let body = if part.landing {
                landings += 1;
                format!("{name}.landing.{landings}")
            } else {
                runs += 1;
                format!("{name}.run.{runs}")
            };
            self.put_body(body.clone(), solid, &material)?;
            self.note_built(&name, &body);
        }
        let sloped = parts.iter().filter(|p| !p.landing).map(|p| p.s1 - p.s0).sum::<f64>();
        Ok(format!(
            "ramp {name}: {runs} run{}, {landings} landing{}, slope 1:{:.1}, {} long",
            if runs == 1 { "" } else { "s" },
            if landings == 1 { "" } else { "s" },
            sloped / (to.z - from.z).abs().max(1.0e-9),
            crate::parse::label(length)
        ))
    }

    pub(crate) fn op_ladder(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a ladder needs a label, like `la: ladder 0,0,0 0,0,3000 width=400 rungs=@300`")
        })?;
        self.check_free_name(&name)?;
        let [a, b] = line.positional.as_slice() else {
            bail!("write `la: ladder x,y,z x,y,z width=400 rungs=@300`");
        };
        let (mut width, mut spacing) = (400.0, 300.0);
        let mut material = "steel".to_string();
        for (key, text) in &line.named {
            match key.as_str() {
                "width" => {
                    width = eval(text, &self.scope).with_context(|| format!("`width={text}`"))?
                }
                "rungs" => spacing = spacing_of(text, &self.scope, "rungs")?,
                "material" => material = text.clone(),
                other => bail!("`ladder` takes width=, rungs= and material=, not `{other}=`"),
            }
        }
        if width <= 0.0 {
            bail!("a ladder width must be above zero");
        }
        let (p, q) = (
            crate::args::point3(a, &self.scope)?,
            crate::args::point3(b, &self.scope)?,
        );
        let along = q - p;
        let length = along.magnitude();
        if length < 1.0e-6 {
            bail!("a ladder needs two different ends");
        }
        let across = {
            let c = Vector3::unit_z().cross(along / length);
            if c.magnitude() < 1.0e-6 {
                Vector3::unit_x()
            } else {
                c.normalize()
            }
        };
        let face = across.cross(along / length);
        self.ensure_library()?;
        let half = across * (width / 2.0);
        for (k, side) in [-1.0, 1.0].into_iter().enumerate() {
            let solid = straight_member("bar", RAIL, p + half * side, q + half * side, face, false)
                .with_context(|| format!("building `{name}`"))?;
            let body = format!("{name}.rail.{}", k + 1);
            self.put_body(body.clone(), solid, &material)?;
            self.note_built(&name, &body);
        }
        let rungs = rung_count(length, spacing);
        for k in 1..=rungs {
            let at = p + along * (k as f64 * spacing / length);
            let solid = straight_member("rod", RUNG, at - half, at + half, along / length, false)
                .with_context(|| format!("building `{name}`"))?;
            let body = format!("{name}.rung.{k}");
            self.put_body(body.clone(), solid, &material)?;
            self.note_built(&name, &body);
        }
        Ok(format!(
            "ladder {name}: {rungs} rungs at {}, {} long, {} wide",
            crate::parse::label(spacing),
            crate::parse::label(length),
            crate::parse::label(width)
        ))
    }
}
