use super::Expect;
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reading {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Curve {
    pub name: String,
    pub unit: String,
    pub values: Vec<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Series {
    pub title: String,
    pub axis: String,
    pub x: Vec<f64>,
    pub curves: Vec<Curve>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Field {
    pub title: String,
    pub unit: String,
    pub values: Vec<f64>,
    pub displacement: Vec<[f64; 3]>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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
    #[serde(skip)]
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

    pub fn packed(&self, stamp: &[(String, String)]) -> Result<Vec<u8>> {
        let header = bitcode::serialize(&Header {
            gcad: env!("CARGO_PKG_VERSION").to_string(),
            stamp: stamp.to_vec(),
        })?;
        let body = bitcode::serialize(self)?;
        let mut out = Vec::with_capacity(RESULT.len() + 8 + header.len() + body.len());
        out.extend_from_slice(RESULT);
        out.extend_from_slice(&(header.len() as u64).to_le_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(&body);
        Ok(out)
    }

    pub fn unpacked(bytes: &[u8]) -> Result<Outcome> {
        let (header, body) = split(bytes)?;
        bitcode::deserialize(body).map_err(|_| {
            anyhow!(
                "written by gcad {} in another layout; rerun `gcad sim`",
                header.gcad
            )
        })
    }

    pub fn read(path: &Path) -> Result<Outcome> {
        let bytes = crate::host::read(path)
            .map_err(|e| anyhow!("reading {}: {e}; run `gcad sim` first", path.display()))?;
        Outcome::unpacked(&bytes).map_err(|e| anyhow!("reading {}: {e}", path.display()))
    }
}

const RESULT: &[u8; 8] = b"GCADRES1";

#[derive(Serialize, Deserialize)]
struct Header {
    gcad: String,
    stamp: Vec<(String, String)>,
}

fn split(bytes: &[u8]) -> Result<(Header, &[u8])> {
    if bytes.len() < RESULT.len() + 8 || &bytes[..RESULT.len()] != RESULT {
        bail!("not a gcad result; rerun `gcad sim`");
    }
    let start = RESULT.len() + 8;
    let length = u64::from_le_bytes(bytes[RESULT.len()..start].try_into()?) as usize;
    let Some(header) = bytes.get(start..start + length) else {
        bail!("the result is cut short; rerun `gcad sim`");
    };
    let header: Header = bitcode::deserialize(header)
        .map_err(|_| anyhow!("written by another gcad; rerun `gcad sim`"))?;
    Ok((header, &bytes[start + length..]))
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
        let Ok(text) = crate::host::read_to_string(&file) else {
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
                && crate::host::is_file(&path)
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
            let bytes = crate::host::read(&file).unwrap_or_default();
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
    let Ok(bytes) = crate::host::read(&path) else {
        return Some(format!("no result yet; run `gcad sim {}`", spec.display()));
    };
    let had = match split(&bytes) {
        Ok((header, _)) => header.stamp,
        Err(why) => return Some(format!("{}: {why}", path.display())),
    };
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
