use crate::args::{Args, colour_of, positive};
use crate::interp::{Scene, Shaded};
use crate::mesh::fur::FurSpec;
use crate::parse::{Line, Scope};
use anyhow::{Result, bail};
use std::collections::HashMap;
use std::sync::Arc;

impl Scene {
    pub fn op_fur(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &[],
            &[
                "part",
                "length",
                "density",
                "cards",
                "width",
                "clump",
                "droop",
                "seed",
                "colour",
                "color",
                "triangles",
            ],
            false,
        )?;
        let length = positive(args.number("length", scope)?, "length")?;
        let width = match args.optional_number("width", scope)? {
            Some(w) => positive(w, "width")?,
            None => length * 0.25,
        };
        let clump = args.optional_number("clump", scope)?.unwrap_or(0.3);
        if !(0.0..=1.0).contains(&clump) {
            bail!("clump goes from 0 (every strand on its own) to 1 (tufts), got {clump}");
        }
        let droop = args.optional_number("droop", scope)?.unwrap_or(0.0);
        if droop < 0.0 {
            bail!("droop must be 0 or more, got {droop}");
        }
        let seed = args.optional_number("seed", scope)?.unwrap_or(1.0);
        if seed < 0.0 || seed.fract() != 0.0 {
            bail!("`seed` is a whole number, got {seed}");
        }
        let colour = match args.values.get("colour").or(args.values.get("color")) {
            Some(text) => Some(colour_of(text, scope)?),
            None => None,
        };
        let mesh = self.sculpted("fur")?.clone();
        let mut spec = FurSpec {
            part: args.values.get("part").map(|p| p.to_string()),
            length,
            width,
            count: 0,
            clump,
            droop,
            seed: seed as u32,
            colour,
        };
        let area = spec.area(&mesh)?;
        let wanted = match (
            args.optional_number("cards", scope)?,
            args.optional_number("density", scope)?,
        ) {
            (Some(n), None) => {
                if n < 1.0 || n.fract() != 0.0 {
                    bail!("`cards` is a whole number of cards, got {n}");
                }
                n as usize
            }
            (None, Some(d)) => (positive(d, "density")? * area).round().max(1.0) as usize,
            _ => bail!("give `density=` (cards per unit of surface) or `cards=` (a count)"),
        };
        let mut note = String::new();
        spec.count = wanted;
        if let Some(budget) = args.optional_number("triangles", scope)? {
            if budget < 2.0 || budget.fract() != 0.0 {
                bail!("`triangles` is a whole number of at least 2, got {budget}");
            }
            let fits = budget as usize / 2;
            if wanted > fits {
                spec.count = fits;
                note = format!("; {wanted} cards would pass the {budget} triangle budget");
            }
        }
        let triangles = spec.triangles();
        let what = spec.part.clone().unwrap_or_else(|| "the mesh".to_string());
        self.mesh_body.detail.fur.push(spec.clone());
        Ok(format!(
            "{} hair cards on {what} ({triangles} triangles, {area:.3} of surface){note}",
            spec.count
        ))
    }

    pub fn fur_shading(&self, colours: &HashMap<String, [f64; 3]>) -> Vec<Shaded> {
        let tinted = self.mesh_tints(colours);
        let mut out = Vec::new();
        for (name, mesh, tints) in tinted {
            let Some(detail) = self.detail_of(&name) else {
                continue;
            };
            for spec in &detail.fur {
                if let Ok(cards) = spec.grow(&mesh) {
                    let (blades, vertex) = cards.shaded(spec.tint(&mesh, &tints));
                    out.push((format!("{name}.fur"), Arc::new(blades), vertex, Vec::new()));
                }
            }
        }
        out
    }
}
