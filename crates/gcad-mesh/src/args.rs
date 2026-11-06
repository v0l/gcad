use crate::mesh::P3;
use crate::parse::{Line, Scope, eval, eval_point};
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;

pub struct Args<'a> {
    line: &'a Line,
    pub values: HashMap<&'static str, &'a str>,
    pub rest: Vec<&'a str>,
}

impl<'a> Args<'a> {
    pub fn new(
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

    pub fn has(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    pub fn text(&self, name: &str) -> Result<&'a str> {
        self.values
            .get(name)
            .copied()
            .ok_or_else(|| anyhow!("`{}` needs `{name}`", self.line.op))
    }

    pub fn number(&self, name: &str, scope: &Scope) -> Result<f64> {
        eval(self.text(name)?, scope).with_context(|| format!("argument `{name}`"))
    }

    pub fn optional_number(&self, name: &str, scope: &Scope) -> Result<Option<f64>> {
        self.values
            .get(name)
            .map(|text| eval(text, scope).with_context(|| format!("argument `{name}`")))
            .transpose()
    }

    pub fn point(&self, name: &str, scope: &Scope) -> Result<(f64, f64)> {
        self.values
            .get(name)
            .map_or(Ok((0.0, 0.0)), |text| eval_point(text, scope))
    }

    pub fn optional_point(&self, name: &str, scope: &Scope) -> Result<Option<(f64, f64)>> {
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

pub fn positive(value: f64, what: &str) -> Result<f64> {
    if value > 0.0 {
        Ok(value)
    } else {
        bail!("{what} must be positive, got {value}")
    }
}

pub fn point3(text: &str, scope: &Scope) -> Result<P3> {
    match crate::parse::split_top_level(text, ',').as_slice() {
        [x, y, z] => Ok([eval(x, scope)?, eval(y, scope)?, eval(z, scope)?]),
        _ => bail!("`{text}` is not a 3D point, write it as x,y,z"),
    }
}

pub fn direction(text: &str, scope: &Scope) -> Result<P3> {
    let (sign, name) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, text),
    };
    match name.to_ascii_lowercase().as_str() {
        "x" | "y" | "z" => Ok(crate::mesh::scale(axis(name)?, sign)),
        _ => point3(text, scope),
    }
}

pub fn axis(text: &str) -> Result<P3> {
    match text.to_ascii_lowercase().as_str() {
        "x" => Ok([1.0, 0.0, 0.0]),
        "y" => Ok([0.0, 1.0, 0.0]),
        "z" => Ok([0.0, 0.0, 1.0]),
        other => bail!("axis must be x, y, z or a datum `axis`, got `{other}`"),
    }
}

pub fn colour_text(text: &str) -> Option<[f64; 3]> {
    if let Some(hex) = text.strip_prefix('#') {
        let channel = |i: usize| {
            hex.get(i..i + 2)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .map(|v| v as f64 / 255.0)
        };
        return match (hex.len(), channel(0), channel(2), channel(4)) {
            (6, Some(r), Some(g), Some(b)) => Some([r, g, b]),
            _ => None,
        };
    }
    Some(match text {
        "red" => [0.85, 0.1, 0.1],
        "green" => [0.1, 0.6, 0.2],
        "blue" => [0.1, 0.3, 0.85],
        "yellow" => [0.95, 0.8, 0.1],
        "orange" => [0.95, 0.5, 0.1],
        "purple" => [0.5, 0.2, 0.7],
        "cyan" => [0.1, 0.7, 0.8],
        "magenta" => [0.85, 0.2, 0.6],
        "black" => [0.05, 0.05, 0.05],
        "white" => [0.95, 0.95, 0.95],
        "grey" | "gray" => [0.5, 0.5, 0.5],
        "silver" => [0.75, 0.75, 0.78],
        _ => return None,
    })
}

pub fn colour_of(text: &str, scope: &Scope) -> Result<[f64; 3]> {
    let colour = if text.starts_with('#') {
        colour_text(text).ok_or_else(|| anyhow!("`{text}` is not a colour; write #rrggbb"))?
    } else if text.contains(',') {
        let p = point3(text, scope)?;
        if p.iter().any(|c| !(0.0..=1.0).contains(c)) {
            bail!("colour parts go from 0 to 1, got {text}");
        }
        p
    } else {
        colour_text(text).ok_or_else(|| {
            anyhow!("`{text}` is not a colour; use a name like red or grey, #rrggbb, or r,g,b from 0 to 1")
        })?
    };
    Ok(colour)
}
