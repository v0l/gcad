use crate::parse::{Line, Scope, eval, eval_point};
use anyhow::{Context, Result, anyhow, bail};
use monstertruck::modeling::{Point3, Vector3};
use std::collections::HashMap;

pub(crate) struct Args<'a> {
    line: &'a Line,
    pub(crate) values: HashMap<&'static str, &'a str>,
    pub(crate) rest: Vec<&'a str>,
}

impl<'a> Args<'a> {
    pub(crate) fn new(
        line: &'a Line,
        positional: &[&'static str],
        named: &[&'static str],
        variadic: bool,
    ) -> Result<Args<'a>> {
        let mut values = HashMap::new();
        let mut rest = Vec::new();
        line.positional
            .iter()
            .enumerate()
            .try_for_each(|(i, value)| {
                match positional.get(i) {
                    Some(name) => {
                        values.insert(*name, value.as_str());
                    }
                    None if variadic => rest.push(value.as_str()),
                    None => bail!(
                        "`{}` takes {} plain argument(s), got `{value}` extra",
                        line.op,
                        positional.len()
                    ),
                }
                Ok(())
            })?;
        line.named.iter().try_for_each(|(key, value)| {
            let name = positional
                .iter()
                .chain(named)
                .find(|name| **name == key.as_str())
                .ok_or_else(|| {
                    let all: Vec<_> = positional.iter().chain(named).collect();
                    anyhow!("`{}` has no `{key}` argument, it takes {all:?}", line.op)
                })?;
            if values.insert(*name, value.as_str()).is_some() {
                bail!("`{key}` given twice");
            }
            Ok(())
        })?;
        Ok(Args { line, values, rest })
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    pub(crate) fn text(&self, name: &str) -> Result<&'a str> {
        self.values
            .get(name)
            .copied()
            .ok_or_else(|| anyhow!("`{}` needs `{name}`", self.line.op))
    }

    pub(crate) fn number(&self, name: &str, scope: &Scope) -> Result<f64> {
        eval(self.text(name)?, scope).with_context(|| format!("argument `{name}`"))
    }

    pub(crate) fn optional_number(&self, name: &str, scope: &Scope) -> Result<Option<f64>> {
        self.values
            .get(name)
            .map(|text| eval(text, scope).with_context(|| format!("argument `{name}`")))
            .transpose()
    }

    pub(crate) fn point(&self, name: &str, scope: &Scope) -> Result<(f64, f64)> {
        self.values
            .get(name)
            .map_or(Ok((0.0, 0.0)), |text| eval_point(text, scope))
    }

    pub(crate) fn optional_point(&self, name: &str, scope: &Scope) -> Result<Option<(f64, f64)>> {
        self.values
            .get(name)
            .map(|text| eval_point(text, scope))
            .transpose()
    }
}

pub fn label_of(line: &Line) -> String {
    line.label
        .clone()
        .unwrap_or_else(|| format!("L{}", line.number))
}

pub(crate) fn positive(value: f64, what: &str) -> Result<f64> {
    if value > 0.0 {
        Ok(value)
    } else {
        bail!("{what} must be positive, got {value}")
    }
}

pub(crate) fn point3(text: &str, scope: &Scope) -> Result<Point3> {
    match crate::parse::split_top_level(text, ',').as_slice() {
        [x, y, z] => Ok(Point3::new(
            eval(x, scope)?,
            eval(y, scope)?,
            eval(z, scope)?,
        )),
        _ => bail!("`{text}` is not a 3D point, write it as x,y,z"),
    }
}

pub(crate) fn axis(text: &str) -> Result<Vector3> {
    match text.to_ascii_lowercase().as_str() {
        "x" => Ok(Vector3::unit_x()),
        "y" => Ok(Vector3::unit_y()),
        "z" => Ok(Vector3::unit_z()),
        other => bail!("axis must be x, y, z or a datum `axis`, got `{other}`"),
    }
}

pub(crate) fn describe_point(point: Point3) -> String {
    format!("{:.3},{:.3},{:.3}", point.x, point.y, point.z)
}

pub(crate) fn combine_mode(args: &Args<'_>) -> Result<super::Combine> {
    match args.values.get("mode").copied() {
        None | Some("add") => Ok(super::Combine::Add),
        Some("cut") => Ok(super::Combine::Remove),
        Some("intersect") => Ok(super::Combine::Common),
        Some(other) => bail!("mode must be add, cut or intersect, got `{other}`"),
    }
}
