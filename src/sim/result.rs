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
pub struct Field {
    pub title: String,
    pub unit: String,
    pub values: Vec<f64>,
    pub displacement: Vec<[f64; 3]>,
}

#[derive(Clone, Debug, Default)]
pub struct Slice {
    pub title: String,
    pub width: usize,
    pub spacing: f64,
    pub across: String,
    pub origin: [f64; 3],
    pub right: [f64; 3],
    pub down: [f64; 3],
    pub speed: Vec<f64>,
    pub along: Vec<f64>,
    pub side: Vec<f64>,
    pub snapshot: Vec<f64>,
}

impl Slice {
    pub fn height(&self) -> usize {
        self.speed.len() / self.width.max(1)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stream {
    pub points: Vec<[f64; 3]>,
    pub speed: Vec<f64>,
    pub turbulence: f64,
    pub pressure: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Movie {
    pub top: f64,
    pub seconds: f64,
    pub planes: Vec<Reel>,
    pub streams: Vec<Vec<Stream>>,
}

#[derive(Clone, Debug, Default)]
pub struct Reel {
    pub width: usize,
    pub height: usize,
    pub frames: Vec<Vec<u8>>,
}

const MOVIE: &[u8; 8] = b"GCADFRM1";
const LINES: &[u8; 4] = b"LINE";

impl Movie {
    pub fn write(&self, path: &Path) -> Result<()> {
        let mut out = Vec::new();
        out.extend_from_slice(MOVIE);
        out.extend_from_slice(&(self.planes.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.top as f32).to_le_bytes());
        out.extend_from_slice(&(self.seconds as f32).to_le_bytes());
        for reel in &self.planes {
            for v in [reel.width, reel.height, reel.frames.len()] {
                out.extend_from_slice(&(v as u32).to_le_bytes());
            }
            for frame in &reel.frames {
                out.extend_from_slice(frame);
            }
        }
        let points = self.streams.iter().flatten().flat_map(|s| s.points.iter());
        let (low, high) = points.fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(l, h), p| {
            (
                [0, 1, 2].map(|k| l[k].min(p[k])),
                [0, 1, 2].map(|k| h[k].max(p[k])),
            )
        });
        if low[0].is_finite() {
            out.extend_from_slice(LINES);
            for v in low.iter().chain(&high) {
                out.extend_from_slice(&(*v as f32).to_le_bytes());
            }
            out.extend_from_slice(&(self.streams.len() as u32).to_le_bytes());
            let span = [0, 1, 2].map(|k| (high[k] - low[k]).max(1.0e-9));
            for frame in &self.streams {
                out.extend_from_slice(&(frame.len() as u32).to_le_bytes());
                for line in frame {
                    let count = line.points.len().min(u16::MAX as usize);
                    out.extend_from_slice(&(count as u16).to_le_bytes());
                    for (p, speed) in line.points.iter().zip(&line.speed).take(count) {
                        for k in 0..3 {
                            let q = ((p[k] - low[k]) / span[k] * 65535.0).round() as u16;
                            out.extend_from_slice(&q.to_le_bytes());
                        }
                        out.push((speed / self.top * 254.0).round().clamp(0.0, 254.0) as u8);
                    }
                }
            }
        }
        std::fs::write(path, out)?;
        Ok(())
    }

    pub fn read(path: &Path) -> Result<Movie> {
        let bytes = std::fs::read(path)?;
        if bytes.len() < 20 || &bytes[..8] != MOVIE {
            return Err(anyhow!("{} is not a gcad frames file", path.display()));
        }
        let mut at = 8;
        let word = |at: &mut usize| {
            let v = u32::from_le_bytes(bytes[*at..*at + 4].try_into().unwrap_or([0; 4]));
            *at += 4;
            v
        };
        let count = word(&mut at) as usize;
        let top = f32::from_bits(word(&mut at)) as f64;
        let seconds = f32::from_bits(word(&mut at)) as f64;
        let mut planes = Vec::new();
        for _ in 0..count {
            if at + 12 > bytes.len() {
                return Err(anyhow!("{} is cut short", path.display()));
            }
            let (width, height, frames) = (
                word(&mut at) as usize,
                word(&mut at) as usize,
                word(&mut at) as usize,
            );
            let size = width * height;
            let end = at + size * frames;
            if end > bytes.len() {
                return Err(anyhow!("{} is cut short", path.display()));
            }
            planes.push(Reel {
                width,
                height,
                frames: bytes[at..end]
                    .chunks(size.max(1))
                    .map(<[u8]>::to_vec)
                    .collect(),
            });
            at = end;
        }
        let mut streams = Vec::new();
        if at + 4 <= bytes.len() && &bytes[at..at + 4] == LINES {
            at += 4;
            let float = |at: &mut usize| f32::from_bits(word(at)) as f64;
            let low = [float(&mut at), float(&mut at), float(&mut at)];
            let high = [float(&mut at), float(&mut at), float(&mut at)];
            let frames = word(&mut at) as usize;
            let short = || anyhow!("{} is cut short", path.display());
            for _ in 0..frames {
                if at + 4 > bytes.len() {
                    return Err(short());
                }
                let lines = word(&mut at) as usize;
                let mut frame = Vec::with_capacity(lines);
                for _ in 0..lines {
                    if at + 2 > bytes.len() {
                        return Err(short());
                    }
                    let count = u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize;
                    at += 2;
                    if at + count * 7 > bytes.len() {
                        return Err(short());
                    }
                    let mut line = Stream::default();
                    for _ in 0..count {
                        let q = |k: usize| {
                            u16::from_le_bytes([bytes[at + 2 * k], bytes[at + 2 * k + 1]]) as f64
                                / 65535.0
                        };
                        line.points
                            .push([0, 1, 2].map(|k| low[k] + q(k) * (high[k] - low[k])));
                        line.speed.push(bytes[at + 6] as f64 / 254.0 * top);
                        at += 7;
                    }
                    frame.push(line);
                }
                streams.push(frame);
            }
        }
        Ok(Movie {
            top,
            seconds,
            planes,
            streams,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub kind: String,
    pub readings: Vec<Reading>,
    pub notes: Vec<String>,
    pub series: Vec<Series>,
    pub checks: Vec<(String, f64, bool)>,
    pub aliases: Vec<(String, String)>,
    pub field: Option<Field>,
    pub slices: Vec<Slice>,
    pub streams: Vec<Stream>,
    pub movie: Option<Movie>,
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
            "field": self.field.as_ref().map(|f| json!({
                "title": f.title, "unit": f.unit, "values": f.values,
                "displacement": f.displacement,
            })),
            "slices": self.slices.iter().map(|s| json!({
                "title": s.title, "width": s.width, "spacing": s.spacing, "across": s.across,
                "origin": s.origin, "right": s.right, "down": s.down,
                "speed": s.speed, "along": s.along, "side": s.side, "snapshot": s.snapshot,
            })).collect::<Vec<_>>(),
            "streams": self.streams.iter().map(|s| json!({
                "points": s.points, "speed": s.speed,
                "turbulence": s.turbulence, "pressure": s.pressure,
            })).collect::<Vec<_>>(),
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
        let triple = |v: &Value| -> [f64; 3] {
            let n = numbers(v);
            [0, 1, 2].map(|k| n.get(k).copied().unwrap_or(0.0))
        };
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
            movie: None,
            slices: list(&value["slices"])
                .iter()
                .map(|s| Slice {
                    title: text(s, "title"),
                    width: s["width"].as_u64().unwrap_or(1) as usize,
                    spacing: s["spacing"].as_f64().unwrap_or(1.0),
                    across: text(s, "across"),
                    origin: triple(&s["origin"]),
                    right: triple(&s["right"]),
                    down: triple(&s["down"]),
                    speed: numbers(&s["speed"]),
                    along: numbers(&s["along"]),
                    side: numbers(&s["side"]),
                    snapshot: numbers(&s["snapshot"]),
                })
                .collect(),
            streams: list(&value["streams"])
                .iter()
                .map(|s| Stream {
                    points: list(&s["points"]).iter().map(triple).collect(),
                    speed: numbers(&s["speed"]),
                    turbulence: s["turbulence"].as_f64().unwrap_or(0.0),
                    pressure: s["pressure"].as_f64().unwrap_or(0.0),
                })
                .collect(),
            field: value["field"].as_object().map(|_| {
                let f = &value["field"];
                Field {
                    title: text(f, "title"),
                    unit: text(f, "unit"),
                    values: numbers(&f["values"]),
                    displacement: list(&f["displacement"])
                        .iter()
                        .map(|d| {
                            let n = numbers(d);
                            [n[0], n[1], n[2]]
                        })
                        .collect(),
                }
            }),
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
    if value.abs() < 1.0e-9 {
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
    "gcad", "gasm", "gsim", "step", "stp", "svg", "dxf", "stl", "ttf", "otf", "toml", "xml",
    "fcmat",
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
        for library in crate::model::materials::discover(Some(&dir)) {
            if !found.contains(&library) {
                found.push(library);
            }
        }
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
