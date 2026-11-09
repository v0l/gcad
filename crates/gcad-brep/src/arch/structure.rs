use crate::Model;
use crate::member::straight_member;
use crate::parse::{Line, eval};
use anyhow::{Context, Result, anyhow, bail};
use monstertruck::modeling::*;

struct Upright {
    kind: String,
    size: String,
    base: f64,
    top: f64,
    material: String,
}

impl Model {
    fn upright(&self, line: &Line, words: &[String], op: &str) -> Result<Upright> {
        let (kind, size) = match words {
            [kind, size] => (kind.clone(), size.clone()),
            _ => bail!(
                "write `{op} rect 300x300`, `round 300` or a member section like `tube 100x100x5`"
            ),
        };
        let mut upright = Upright {
            kind,
            size,
            base: 0.0,
            top: f64::NAN,
            material: "concrete".into(),
        };
        for (key, text) in &line.named {
            let value = || eval(text, &self.scope).with_context(|| format!("`{key}={text}`"));
            match key.as_str() {
                "base" => upright.base = value()?,
                "top" => upright.top = value()?,
                "material" => upright.material = text.clone(),
                other => bail!("`{op}` takes base=, top= and material=, not `{other}=`"),
            }
        }
        if upright.top.is_nan() {
            bail!("a column needs `top=`, a height or a level like `top=L1`");
        }
        if upright.top <= upright.base {
            bail!(
                "the column's top {} is not above its base {}",
                upright.top,
                upright.base
            );
        }
        Ok(upright)
    }

    fn put_upright(&mut self, owner: &str, name: &str, at: (f64, f64), u: &Upright) -> Result<()> {
        let solid = straight_member(
            &u.kind,
            &u.size,
            Point3::new(at.0, at.1, u.base),
            Point3::new(at.0, at.1, u.top),
            Vector3::unit_y(),
            false,
        )
        .with_context(|| format!("building `{name}`"))?;
        self.put_body(name.to_string(), solid, &u.material)?;
        self.note_built(owner, name);
        Ok(())
    }

    pub(crate) fn op_column(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a column needs a label, like `c1: column A1 rect 300x300 top=L1`")
        })?;
        self.check_free_name(&name)?;
        let [at, section @ ..] = line.positional.as_slice() else {
            bail!("write `c1: column A1 rect 300x300 base=L0 top=L1`");
        };
        let point = self.plan_point(at)?;
        let upright = self.upright(line, section, "column")?;
        self.ensure_library()?;
        self.put_upright(&name, &name, point, &upright)?;
        Ok(format!(
            "column {name} at {at}: {} {}, {} high",
            upright.kind,
            upright.size,
            crate::parse::label(upright.top - upright.base)
        ))
    }

    pub(crate) fn op_column_grid(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a column grid needs a label, like `cg: column_grid rect 200x200 top=L1`")
        })?;
        self.check_free_name(&name)?;
        let upright = self.upright(line, &line.positional, "column_grid")?;
        let crossings: Vec<(String, (f64, f64))> = self
            .arch
            .grids
            .iter()
            .filter(|g| g.along_x)
            .flat_map(|x| {
                self.arch
                    .grids
                    .iter()
                    .filter(|g| !g.along_x)
                    .map(move |y| (format!("{}{}", x.name, y.name), (x.offset, y.offset)))
            })
            .collect();
        if crossings.is_empty() {
            bail!("a column grid needs x and y `grid` lines to stand on");
        }
        self.ensure_library()?;
        for (crossing, at) in &crossings {
            self.put_upright(&name, &format!("{name}.{crossing}"), *at, &upright)?;
        }
        Ok(format!(
            "column grid {name}: {} columns, {} {}",
            crossings.len(),
            upright.kind,
            upright.size
        ))
    }
}
