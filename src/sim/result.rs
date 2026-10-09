use super::Expect;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Reading {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub at: String,
}

#[derive(Clone, Debug)]
pub struct Curve {
    pub name: String,
    pub unit: String,
    pub values: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct Series {
    pub title: String,
    pub axis: String,
    pub x: Vec<f64>,
    pub curves: Vec<Curve>,
}

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub kind: String,
    pub readings: Vec<Reading>,
    pub notes: Vec<String>,
    pub series: Vec<Series>,
    pub checks: Vec<(String, f64, bool)>,
    pub aliases: Vec<(String, String)>,
}

impl Outcome {
    pub fn reading(&self, name: &str) -> Option<&Reading> {
        let name = self
            .aliases
            .iter()
            .find(|(alias, _)| alias == name)
            .map_or(name, |(_, real)| real.as_str());
        self.readings.iter().find(|r| r.name == name)
    }

    pub(super) fn check(&mut self, expects: &[Expect]) {
        self.checks = expects
            .iter()
            .map(|e| {
                let value = self.reading(&e.reading).map_or(f64::NAN, |r| r.value);
                let pass = if e.below {
                    value < e.limit
                } else {
                    value > e.limit
                };
                (e.text.clone(), value, pass)
            })
            .collect();
    }

    pub fn failed(&self) -> Vec<String> {
        self.checks
            .iter()
            .filter(|(_, _, pass)| !pass)
            .map(|(text, value, _)| format!("{text} (it is {})", short(*value)))
            .collect()
    }

    pub fn table(&self) -> String {
        let width = self
            .readings
            .iter()
            .map(|r| r.name.len())
            .max()
            .unwrap_or(0);
        let mut out = String::new();
        for r in &self.readings {
            out += &format!(
                "{:<width$}  {:>10} {:<4} {}\n",
                r.name,
                short(r.value),
                r.unit,
                r.at
            );
        }
        for note in &self.notes {
            out += &format!("note: {note}\n");
        }
        for (text, value, pass) in &self.checks {
            let verdict = if *pass { "ok" } else { "FAILED" };
            out += &format!("expect {text}  {verdict} ({})\n", short(*value));
        }
        out
    }

    pub fn to_json(&self, stamp: &[(String, String)]) -> Value {
        json!({
            "gcad": env!("CARGO_PKG_VERSION"),
            "kind": self.kind,
            "inputs": stamp.iter().map(|(f, h)| json!({"file": f, "hash": h})).collect::<Vec<_>>(),
            "readings": self.readings.iter().map(|r| json!({
                "name": r.name, "value": r.value, "unit": r.unit, "at": r.at,
            })).collect::<Vec<_>>(),
            "notes": self.notes,
            "aliases": self.aliases.iter().map(|(a, n)| json!([a, n])).collect::<Vec<_>>(),
            "expect": self.checks.iter().map(|(t, v, p)| json!({
                "text": t, "value": v, "pass": p,
            })).collect::<Vec<_>>(),
            "series": self.series.iter().map(|s| json!({
                "title": s.title, "axis": s.axis, "x": s.x,
                "curves": s.curves.iter().map(|c| json!({
                    "name": c.name, "unit": c.unit, "values": c.values,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }

    pub fn from_json(value: &Value) -> Result<Outcome> {
        let text = |v: &Value, key: &str| v[key].as_str().unwrap_or_default().to_string();
        let numbers = |v: &Value| -> Vec<f64> {
            v.as_array()
                .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect())
                .unwrap_or_default()
        };
        let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
        Ok(Outcome {
            kind: text(value, "kind"),
            readings: list(&value["readings"])
                .iter()
                .map(|r| Reading {
                    name: text(r, "name"),
                    value: r["value"].as_f64().unwrap_or(f64::NAN),
                    unit: text(r, "unit"),
                    at: text(r, "at"),
                })
                .collect(),
            notes: list(&value["notes"])
                .iter()
                .filter_map(|n| n.as_str().map(str::to_string))
                .collect(),
            series: list(&value["series"])
                .iter()
                .map(|s| Series {
                    title: text(s, "title"),
                    axis: text(s, "axis"),
                    x: numbers(&s["x"]),
                    curves: list(&s["curves"])
                        .iter()
                        .map(|c| Curve {
                            name: text(c, "name"),
                            unit: text(c, "unit"),
                            values: numbers(&c["values"]),
                        })
                        .collect(),
                })
                .collect(),
            aliases: list(&value["aliases"])
                .iter()
                .filter_map(|pair| {
                    Some((pair[0].as_str()?.to_string(), pair[1].as_str()?.to_string()))
                })
                .collect(),
            checks: list(&value["expect"])
                .iter()
                .map(|e| {
                    (
                        text(e, "text"),
                        e["value"].as_f64().unwrap_or(f64::NAN),
                        e["pass"].as_bool().unwrap_or(false),
                    )
                })
                .collect(),
        })
    }

    pub fn read(path: &Path) -> Result<Outcome> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("reading {}: {e}; run `gcad sim` first", path.display()))?;
        Outcome::from_json(&serde_json::from_str(&text)?)
    }
}

pub fn short(value: f64) -> String {
    if !value.is_finite() {
        return "none".to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs();
    let places = match magnitude {
        m if m >= 100.0 => 1,
        m if m >= 1.0 => 3,
        m if m >= 0.01 => 4,
        _ => 6,
    };
    format!("{value:.places$}")
}

const FILE_KINDS: &[&str] = &[
    "gcad", "gasm", "gsim", "step", "stp", "svg", "dxf", "stl", "ttf", "otf",
];
const READ_INTO: &[&str] = &["gcad", "gasm", "gsim"];

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn inputs(spec: &Path) -> Vec<PathBuf> {
    let mut found = vec![spec.to_path_buf()];
    let mut index = 0;
    while index < found.len() {
        let file = found[index].clone();
        index += 1;
        if !READ_INTO.contains(&extension(&file).as_str()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        for word in text.lines().flat_map(str::split_whitespace) {
            let word = word.rsplit('=').next().unwrap_or(word);
            let path = dir.join(word);
            if FILE_KINDS.contains(&extension(&path).as_str())
                && path.is_file()
                && !found.contains(&path)
            {
                found.push(path);
            }
        }
    }
    found
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub fn stamp(spec: &Path, vars: &[(String, f64)]) -> Vec<(String, String)> {
    let dir = spec.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut stamp: Vec<(String, String)> = inputs(spec)
        .into_iter()
        .map(|file| {
            let bytes = std::fs::read(&file).unwrap_or_default();
            let name = file.strip_prefix(&dir).unwrap_or(&file);
            (name.display().to_string(), format!("{:016x}", fnv(&bytes)))
        })
        .collect();
    if !vars.is_empty() {
        let set = format!("{vars:?}");
        stamp.push(("--set".to_string(), format!("{:016x}", fnv(set.as_bytes()))));
    }
    stamp.push(("gcad".to_string(), env!("CARGO_PKG_VERSION").to_string()));
    stamp
}

pub fn stale(spec: &Path, vars: &[(String, f64)]) -> Option<String> {
    let path = super::result_path(spec);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Some(format!("no result yet; run `gcad sim {}`", spec.display()));
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Some(format!("{} cannot be read", path.display()));
    };
    let had: Vec<(String, String)> = value["inputs"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|i| {
            (
                i["file"].as_str().unwrap_or_default().to_string(),
                i["hash"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let now = stamp(spec, vars);
    let changed: Vec<String> = now
        .iter()
        .filter(|entry| !had.contains(entry))
        .map(|(file, _)| file.clone())
        .chain(
            had.iter()
                .filter(|(file, _)| !now.iter().any(|(f, _)| f == file))
                .map(|(file, _)| file.clone()),
        )
        .collect();
    (!changed.is_empty()).then(|| {
        format!(
            "{} is stale ({} changed); rerun `gcad sim {}`",
            path.display(),
            changed.join(", "),
            spec.display()
        )
    })
}
