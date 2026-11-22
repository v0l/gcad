use super::Model;
use super::args::{Args, point3};
use crate::parse::{Line, eval, label, split_top_level};
use anyhow::{Result, anyhow, bail};
use gcad_cloud::colour::{ColourBy, colours};
use gcad_cloud::deviation::{Stats, Surface, stats};
use gcad_cloud::measure::{self, Feature};
use rayon::prelude::*;

const DEFAULT_FINENESS: f64 = 0.25;

#[derive(Clone, Debug)]
pub struct Inspection {
    pub scan: String,
    pub target: String,
    pub into: String,
    pub tolerance: f64,
    pub triangles: usize,
    pub stats: Stats,
}

#[derive(Clone, Debug)]
pub struct CheckRecord {
    pub line: usize,
    pub text: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct MeasureRecord {
    pub name: String,
    pub what: String,
    pub distance: f64,
    pub signed: Option<f64>,
}

impl Model {
    pub(crate) fn cloud_deviation(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &["to", "max", "into", "fine"], false)?;
        let name = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&name, "deviation")?;
        let target = args.text("to")?.to_string();
        let tolerance = args.number("max", &self.scope)?;
        if !(tolerance > 0.0) {
            bail!("`max` must be positive, got {tolerance}");
        }
        let fineness = args.optional_number("fine", &self.scope)?.unwrap_or(DEFAULT_FINENESS);
        if !(fineness > 0.0) {
            bail!("`fine` must be positive, got {fineness}");
        }
        let into = args.values.get("into").copied().unwrap_or("dev").to_string();
        let surface = Surface::new(self.cad_triangles(&target, fineness)?)?;
        let values: Vec<f32> = cloud
            .positions
            .par_iter()
            .map(|p| surface.signed_distance(*p) as f32)
            .collect();
        let found = stats(&values, tolerance);
        let mut out = (*cloud).clone();
        out.deviation = Some(values);
        out.deviation_limit = tolerance;
        let all: Vec<usize> = (0..out.len()).collect();
        out.colours = Some(colours(&out, ColourBy::Deviation, &all));
        self.publish_cloud(&name, out);
        for (key, value) in [
            ("mean", found.mean),
            ("std", found.std),
            ("min", found.min),
            ("max", found.max),
            ("rms", found.rms),
            ("within", found.within),
            ("tolerance", tolerance),
        ] {
            self.scope.insert(format!("{into}.{key}"), value);
        }
        self.inspections.retain(|i| i.into != into);
        self.inspections.push(Inspection {
            scan: name,
            target: target.clone(),
            into,
            tolerance,
            triangles: surface.triangle_count(),
            stats: found,
        });
        Ok(format!(
            "deviation to {target}: mean {}, std {}, min {}, max {}, {}% within {}; signed distance to {} triangles, chord tolerance {} of the default",
            label(found.mean),
            label(found.std),
            label(found.min),
            label(found.max),
            label(found.within),
            label(tolerance),
            surface.triangle_count(),
            label(fineness)
        ))
    }

    pub(crate) fn cloud_offset(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &["by", "box", "dir"], false)?;
        let name = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&name, "cloud_offset")?;
        let by = args.number("by", &self.scope)?;
        let region = match args.values.get("box") {
            Some(text) => {
                let parts = split_top_level(text, ',');
                if parts.len() != 6 {
                    bail!("`box` is x0,y0,z0,x1,y1,z1");
                }
                let v = parts.iter().map(|p| eval(p, &self.scope)).collect::<Result<Vec<_>>>()?;
                Some((
                    std::array::from_fn::<f64, 3, _>(|k| v[k].min(v[k + 3])),
                    std::array::from_fn::<f64, 3, _>(|k| v[k].max(v[k + 3])),
                ))
            }
            None => None,
        };
        let direction = match args.values.get("dir") {
            Some(text) => {
                let d = point3(text, &self.scope)?;
                let length = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
                if length < 1.0e-12 {
                    bail!("`dir` must not be zero");
                }
                Some([d.x / length, d.y / length, d.z / length])
            }
            None => None,
        };
        if direction.is_none() && cloud.normals.is_none() {
            bail!("`cloud_offset` moves points along their normals; run `normals` first or give dir=x,y,z");
        }
        let mut out = (*cloud).clone();
        let mut moved = 0usize;
        for i in 0..out.len() {
            let p = out.positions[i];
            if let Some((lo, hi)) = region {
                if (0..3).any(|k| p[k] < lo[k] || p[k] > hi[k]) {
                    continue;
                }
            }
            let n = match direction {
                Some(d) => d,
                None => cloud.normals.as_ref().map_or([0.0; 3], |n| n[i].map(|v| v as f64)),
            };
            (0..3).for_each(|k| out.positions[i][k] += by * n[k]);
            moved += 1;
        }
        out.deviation = None;
        out.deviation_limit = 0.0;
        let _ = self.replace_cloud_text(&name, out);
        Ok(format!("moved {moved} of {} points by {}", cloud.len(), label(by)))
    }

    fn replace_cloud_text(&mut self, name: &str, cloud: gcad_cloud::Cloud) -> String {
        let report = cloud.summary();
        self.publish_cloud(name, cloud);
        report
    }

    fn feature_of(&self, text: &str) -> Result<Feature> {
        if let Some(shape) = self.primitives.get(text) {
            return Ok(Feature::Shape(*shape));
        }
        if text.contains(',') {
            let p = point3(text, &self.scope)?;
            return Ok(Feature::Point([p.x, p.y, p.z]));
        }
        let known: Vec<_> = self.primitives.keys().map(String::as_str).collect();
        Err(anyhow!(
            "`{text}` is not a fitted feature or a point x,y,z; fitted features are {known:?}"
        ))
    }

    pub(crate) fn cloud_measure(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &["into"], true)?;
        self.named_cloud(args.text("cloud")?, "measure")?;
        let into = args.values.get("into").copied().unwrap_or("m").to_string();
        match args.rest.as_slice() {
            ["diameter", one] => {
                let value = measure::diameter(&self.feature_of(one)?)?;
                self.scope.insert(format!("{into}.diameter"), value);
                self.scope.insert(format!("{into}.distance"), value);
                self.measures.push(MeasureRecord {
                    name: into,
                    what: format!("diameter of {one}"),
                    distance: value,
                    signed: None,
                });
                Ok(format!("diameter {}", label(value)))
            }
            [a, b] => {
                let found = measure::distance(&self.feature_of(a)?, &self.feature_of(b)?)?;
                self.scope.insert(format!("{into}.distance"), found.distance);
                if let Some(signed) = found.signed {
                    self.scope.insert(format!("{into}.signed"), signed);
                }
                self.measures.push(MeasureRecord {
                    name: into,
                    what: format!("{} {a} {b}", found.what),
                    distance: found.distance,
                    signed: found.signed,
                });
                Ok(format!("{} {}", found.what, label(found.distance)))
            }
            _ => bail!("`measure scan a b` or `measure scan diameter a`; a and b are fitted features or points x,y,z"),
        }
    }

    pub(crate) fn op_check(&mut self, line: &Line) -> Result<String> {
        let mut text = line.positional.join("");
        for (k, v) in &line.named {
            text.push_str(&format!("{k}={v}"));
        }
        if text.trim().is_empty() {
            bail!("write `check d.max < 0.3`");
        }
        let value = eval(&text, &self.scope)?;
        let passed = value != 0.0;
        let mut seen: Vec<String> = Vec::new();
        for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.')) {
            if self.scope.contains_key(word) && !seen.iter().any(|s| s == word) {
                seen.push(word.to_string());
            }
        }
        let detail = seen
            .iter()
            .map(|w| format!("{w} = {}", label(self.scope[w])))
            .collect::<Vec<_>>()
            .join(", ");
        self.checks.push(CheckRecord {
            line: line.number,
            text: text.clone(),
            passed,
            detail: detail.clone(),
        });
        if passed {
            return Ok(format!("pass: {text} ({detail})"));
        }
        if self.fixed.contains_key("_soft_checks") {
            return Ok(format!("FAIL: {text} ({detail})"));
        }
        bail!("check failed: {text} ({detail})")
    }
}
