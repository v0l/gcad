use super::Report;
use super::thread;
use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use std::sync::OnceLock;

const TABLE: &str = include_str!("data/parts.toml");

#[derive(Deserialize)]
struct File {
    nut4032: Vec<[f64; 3]>,
    nut4035: Vec<[f64; 3]>,
    nut7040: Vec<[f64; 3]>,
    washer7089: Vec<[f64; 4]>,
    washer7093: Vec<[f64; 4]>,
    hex_head: Vec<[f64; 2]>,
    dowel_2338: Vec<f64>,
    dowel_8734: Vec<f64>,
    key6885: Vec<[f64; 6]>,
    circlip471: Vec<[f64; 5]>,
    circlip472: Vec<[f64; 5]>,
    belt: Vec<(String, f64, f64, f64)>,
    extrusion: Vec<(String, f64, f64)>,
}

fn file() -> &'static File {
    static FILE: OnceLock<File> = OnceLock::new();
    FILE.get_or_init(|| toml::from_str(TABLE).unwrap_or_else(|e| panic!("calc table parts: {e}")))
}

pub struct Kind {
    pub name: &'static str,
    pub standard: &'static str,
    pub group: &'static str,
    pub example: &'static str,
    pub modelled: bool,
}

pub const KINDS: &[Kind] = &[
    Kind { name: "iso4762", standard: "ISO 4762", group: "screw", example: "iso4762:M6x25", modelled: true },
    Kind { name: "iso4017", standard: "ISO 4017", group: "screw", example: "iso4017:M8x30", modelled: true },
    Kind { name: "iso4014", standard: "ISO 4014", group: "screw", example: "iso4014:M8x40", modelled: true },
    Kind { name: "iso7380", standard: "ISO 7380", group: "screw", example: "iso7380:M5x12", modelled: true },
    Kind { name: "iso10642", standard: "ISO 10642", group: "screw", example: "iso10642:M6x20", modelled: true },
    Kind { name: "iso4032", standard: "ISO 4032", group: "nut", example: "iso4032:M6", modelled: true },
    Kind { name: "iso4035", standard: "ISO 4035", group: "nut", example: "iso4035:M6", modelled: true },
    Kind { name: "iso7040", standard: "ISO 7040", group: "nut", example: "iso7040:M6", modelled: true },
    Kind { name: "iso7089", standard: "ISO 7089", group: "washer", example: "iso7089:M6", modelled: true },
    Kind { name: "iso7093", standard: "ISO 7093", group: "washer", example: "iso7093:M6", modelled: true },
    Kind { name: "iso2338", standard: "ISO 2338", group: "dowel", example: "iso2338:6x24", modelled: true },
    Kind { name: "iso8734", standard: "ISO 8734", group: "dowel", example: "iso8734:6x24", modelled: true },
    Kind { name: "din6885", standard: "DIN 6885", group: "key", example: "din6885:8x7x32", modelled: true },
    Kind { name: "din471", standard: "DIN 471", group: "circlip", example: "din471:25", modelled: true },
    Kind { name: "din472", standard: "DIN 472", group: "circlip", example: "din472:50", modelled: true },
    Kind { name: "gt2pulley", standard: "GT2 pulley", group: "belt", example: "gt2pulley:20x6x5", modelled: true },
    Kind { name: "htd3pulley", standard: "HTD 3M pulley", group: "belt", example: "htd3pulley:24x9x6", modelled: true },
    Kind { name: "htd5pulley", standard: "HTD 5M pulley", group: "belt", example: "htd5pulley:24x15x8", modelled: true },
    Kind { name: "gt2belt", standard: "GT2 belt", group: "belt", example: "gt2belt:6x200", modelled: false },
    Kind { name: "htd3belt", standard: "HTD 3M belt", group: "belt", example: "htd3belt:9x300", modelled: false },
    Kind { name: "htd5belt", standard: "HTD 5M belt", group: "belt", example: "htd5belt:15x500", modelled: false },
    Kind { name: "extrusion", standard: "T-slot extrusion", group: "extrusion", example: "extrusion:2020x300", modelled: false },
    Kind { name: "spec", standard: "bought part", group: "spec", example: "spec:nema17@42x42x40", modelled: false },
];

pub fn kind(name: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.name == name)
}

pub fn knows(spec: &str) -> bool {
    spec.split_once(':').is_some_and(|(k, _)| kind(k).is_some())
}

#[derive(Debug, Clone)]
pub struct Part {
    pub spec: String,
    pub title: String,
    pub material: Option<&'static str>,
    pub modelled: bool,
    pub rows: Vec<(String, String)>,
    pub source: String,
    pub envelope: [f64; 3],
    pub alias: Option<String>,
}

fn numbers(text: &str, count: usize, form: &str) -> Result<Vec<f64>> {
    let values: Option<Vec<f64>> = text
        .split(['x', 'X'])
        .map(|p| p.parse::<f64>().ok().filter(|v| *v > 0.0))
        .collect();
    values
        .filter(|v| v.len() == count)
        .ok_or_else(|| anyhow!("write it as {form}, with {count} positive numbers"))
}

fn hex_corners(across_flats: f64) -> f64 {
    across_flats / (std::f64::consts::PI / 6.0).cos()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-9
}

fn row(rows: &[Vec<(String, String)>]) -> Vec<(String, String)> {
    rows.concat()
}

fn kv(k: &str, v: String) -> Vec<(String, String)> {
    vec![(k.to_string(), v)]
}

fn thread_size(size: &str, kind: &str, with_length: bool) -> Result<(thread::Thread, f64)> {
    let (name, length) = match size.split_once(['x', 'X']) {
        Some((n, l)) if with_length => (
            n,
            l.parse::<f64>()
                .map_err(|_| anyhow!("`{l}` is not a length in `{kind}:{size}`"))?,
        ),
        Some(_) => bail!("`{kind}` takes a size alone, like {kind}:M6"),
        None if with_length => bail!("`{kind}` needs a length, like {kind}:M6x25"),
        None => (size, 0.0),
    };
    if with_length && length <= 0.0 {
        bail!("the length in `{kind}:{size}` must be above zero");
    }
    let t = thread::find(name)?;
    if t.family != thread::Family::Metric {
        bail!("`{kind}` is a metric part; `{name}` is not a metric size");
    }
    Ok((t, length))
}

fn pick<const N: usize>(rows: &[[f64; N]], major: f64, kind: &str) -> Result<[f64; N]> {
    rows.iter()
        .find(|r| near(r[0], major))
        .copied()
        .ok_or_else(|| {
            anyhow!(
                "{kind} is listed for M{}; M{major} is not in the table",
                rows.iter().map(|r| r[0].to_string()).collect::<Vec<_>>().join(", M")
            )
        })
}

fn finish(
    k: &Kind,
    spec: &str,
    size: &str,
    material: Option<&'static str>,
    rows: Vec<(String, String)>,
    source: Vec<String>,
    envelope: [f64; 3],
) -> Part {
    let mut lines = source;
    if let Some(m) = material {
        lines.push(format!("material {m}"));
    }
    Part {
        spec: spec.to_string(),
        title: format!("{} {}", k.standard, size),
        material,
        modelled: k.modelled,
        rows,
        source: lines.join("\n"),
        envelope,
        alias: None,
    }
}

pub fn resolve(spec: &str) -> Result<Part> {
    let (name, size) = spec
        .split_once(':')
        .ok_or_else(|| anyhow!("a part is written kind:size, like iso4762:M6x25"))?;
    let k = kind(name).ok_or_else(|| {
        anyhow!(
            "unknown part `{name}`; known: {}",
            KINDS.iter().map(|k| k.name).collect::<Vec<_>>().join(", ")
        )
    })?;
    let f = file();
    match name {
        "iso4762" | "iso4017" | "iso4014" | "iso7380" | "iso10642" => {
            let (t, l) = thread_size(size, name, true)?;
            let head = |standard: &str| {
                thread::heads(&t)
                    .into_iter()
                    .find(|h| h.standard == standard)
                    .ok_or_else(|| anyhow!("{} is not listed for {}", k.standard, t.name))
            };
            let rows = match name {
                "iso4017" | "iso4014" => {
                    let h = pick(&f.hex_head.iter().map(|r| [r[0], r[1]]).collect::<Vec<_>>(), t.major, k.standard)?;
                    let across = pick(&f.nut4032, t.major, k.standard)?[1];
                    row(&[
                        kv("head", format!("{across} mm across flats, {} high", h[1])),
                        kv("thread", format!("{} pitch {} mm", t.name, t.pitch)),
                    ])
                }
                _ => {
                    let h = head(k.standard)?;
                    row(&[
                        kv("head", format!("{} mm across, {} high", h.head_dia, h.head_height)),
                        kv("thread", format!("{} pitch {} mm", t.name, t.pitch)),
                    ])
                }
            };
            let mut part = finish(
                k,
                spec,
                &format!("{}x{}", t.name, l),
                Some("steel"),
                row(&[rows, kv("length", format!("{l} mm"))]),
                Vec::new(),
                [t.major * 2.0, t.major * 2.0, l],
            );
            part.alias = match name {
                "iso4762" => Some(format!("shcs:{}x{l}", t.name)),
                "iso4017" | "iso4014" => Some(format!("hex:{}x{l}", t.name)),
                "iso10642" => Some(format!("fhcs:{}x{l}", t.name)),
                _ => None,
            };
            if name != "iso7380" {
                let across = match name {
                    "iso4017" | "iso4014" => pick(&f.nut4032, t.major, k.standard)?[1],
                    _ => head(k.standard)?.head_dia,
                };
                part.envelope = [across, across, l];
            }
            if name == "iso7380" {
                let h = head(k.standard)?;
                part.source = [
                    format!("circle {}", h.head_dia),
                    format!("head: extrude {}", h.head_height),
                    "plane XY".into(),
                    format!("circle {}", t.major),
                    format!("shank: extrude -{l}"),
                    "material steel".into(),
                ]
                .join("\n");
                part.envelope = [h.head_dia, h.head_dia, l + h.head_height];
            }
            Ok(part)
        }
        "iso4032" | "iso4035" | "iso7040" => {
            let (t, _) = thread_size(size, name, false)?;
            let table = match name {
                "iso4032" => &f.nut4032,
                "iso4035" => &f.nut4035,
                _ => &f.nut7040,
            };
            let r = pick(table, t.major, k.standard)?;
            let rows = row(&[
                kv("across flats", format!("{} mm", r[1])),
                kv("height", format!("{} mm", r[2])),
                kv("thread", format!("{} pitch {} mm", t.name, t.pitch)),
            ]);
            let source = vec![
                format!("ngon {} 6", hex_corners(r[1])),
                format!("nut: extrude {}", r[2]),
                "plane nut.end".into(),
                format!("circle {}", t.major),
                "bore: cut thru".into(),
            ];
            let corners = hex_corners(r[1]);
            Ok(finish(k, spec, &t.name, Some("steel"), rows, source, [corners, r[1], r[2]]))
        }
        "iso7089" | "iso7093" => {
            let (t, _) = thread_size(size, name, false)?;
            let table = if name == "iso7089" { &f.washer7089 } else { &f.washer7093 };
            let r = pick(table, t.major, k.standard)?;
            let rows = row(&[
                kv("inner", format!("{} mm", r[1])),
                kv("outer", format!("{} mm", r[2])),
                kv("thickness", format!("{} mm", r[3])),
            ]);
            let source = vec![
                format!("circle {}", r[2]),
                format!("washer: extrude {}", r[3]),
                "plane washer.end".into(),
                format!("circle {}", r[1]),
                "bore: cut thru".into(),
            ];
            Ok(finish(k, spec, &t.name, Some("steel"), rows, source, [r[2], r[2], r[3]]))
        }
        "iso2338" | "iso8734" => {
            let v = numbers(size, 2, &format!("{name}:DxL, like {}", k.example.split(':').nth(1).unwrap_or("")))?;
            let (d, l) = (v[0], v[1]);
            let sizes = if name == "iso2338" { &f.dowel_2338 } else { &f.dowel_8734 };
            if !sizes.iter().any(|s| near(*s, d)) {
                bail!(
                    "{} has diameters {}; {d} is not one",
                    k.standard,
                    sizes.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ")
                );
            }
            if l <= d {
                bail!("a {d} dowel must be longer than its diameter");
            }
            let rows = row(&[
                kv("diameter", format!("{d} mm")),
                kv("length", format!("{l} mm")),
                kv("fit", "m6 (ISO 2338) or h8 (ISO 8734)".into()),
            ]);
            let source = vec![
                format!("circle {d}"),
                format!("pin: extrude {l}"),
                format!("chamfer {} pin.start&pin.side|pin.end&pin.side", d * 0.1),
            ];
            Ok(finish(k, spec, &format!("{d}x{l}"), Some("steel"), rows, source, [d, d, l]))
        }
        "din6885" => {
            let v = numbers(size, 3, "din6885:bxhxL, like din6885:8x7x32")?;
            let (b, h, l) = (v[0], v[1], v[2]);
            let r = f
                .key6885
                .iter()
                .find(|r| near(r[2], b) && near(r[3], h))
                .ok_or_else(|| {
                    anyhow!(
                        "DIN 6885 keys are {}",
                        f.key6885
                            .iter()
                            .map(|r| format!("{}x{}", r[2], r[3]))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })?;
            if l < b * 1.5 {
                bail!("a {b} wide key is at least {} long", b * 1.5);
            }
            let rows = row(&[
                kv("shaft", format!("over {} up to {} mm", r[0], r[1])),
                kv("shaft keyway depth t1", format!("{} mm", r[4])),
                kv("hub keyway depth t2", format!("{} mm", r[5])),
            ]);
            let source = vec![
                format!("rect {l} {b}"),
                format!("key: extrude {h}"),
            ];
            Ok(finish(k, spec, &format!("{b}x{h}x{l}"), Some("steel"), rows, source, [l, b, h]))
        }
        "din471" | "din472" => {
            let d = numbers(size, 1, &format!("{name}:d, like {}", k.example.split(':').nth(1).unwrap_or("")))?[0];
            let table = if name == "din471" { &f.circlip471 } else { &f.circlip472 };
            let r = table.iter().find(|r| near(r[0], d)).ok_or_else(|| {
                anyhow!(
                    "{} is listed for {} mm",
                    k.standard,
                    table.iter().map(|r| r[0].to_string()).collect::<Vec<_>>().join(", ")
                )
            })?;
            let (inner, outer) = if name == "din471" {
                (r[2], r[2] + 2.0 * r[3])
            } else {
                (r[2] - 2.0 * r[3], r[2])
            };
            let rows = row(&[
                kv("groove diameter", format!("{} mm", r[4])),
                kv("thickness", format!("{} mm", r[1])),
                kv("free diameter d3", format!("{} mm", r[2])),
                kv("radial width", format!("{} mm", r[3])),
            ]);
            let source = vec![
                format!("circle {outer}"),
                format!("clip: extrude {}", r[1]),
                "plane clip.end".into(),
                format!("circle {inner}"),
                "bore: cut thru".into(),
            ];
            Ok(finish(k, spec, &format!("{d}x{}", r[1]), Some("steel"), rows, source, [outer, outer, r[1]]))
        }
        "gt2pulley" | "htd3pulley" | "htd5pulley" => {
            let v = numbers(size, 3, &format!("{name}:teethxwidthxbore, like {}", k.example.split(':').nth(1).unwrap_or("")))?;
            let (teeth, width, bore) = (v[0], v[1], v[2]);
            let belt = belt_of(name.trim_end_matches("pulley"))?;
            let pd = teeth * belt.1 / std::f64::consts::PI;
            let od = pd - 2.0 * belt.2;
            if teeth.fract() != 0.0 || teeth < 10.0 {
                bail!("a pulley has a whole number of at least 10 teeth");
            }
            if bore >= od - 2.0 {
                bail!("a {bore} bore leaves no wall in a {od:.2} outside diameter pulley");
            }
            let long = width + 6.0;
            let rows = row(&[
                kv("pitch diameter", format!("{pd:.3} mm")),
                kv("outside diameter", format!("{od:.3} mm")),
                kv("belt width", format!("{width} mm")),
                kv("length", format!("{long} mm with hub")),
                kv("bore", format!("{bore} mm")),
            ]);
            let source = vec![
                format!("circle {od}"),
                format!("pulley: extrude {long}"),
                "plane pulley.end".into(),
                format!("circle {bore}"),
                "bore: cut thru".into(),
            ];
            Ok(finish(k, spec, &format!("{}T x {width} bore {bore}", teeth as u32), Some("aluminium"), rows, source, [od, od, long]))
        }
        "gt2belt" | "htd3belt" | "htd5belt" => {
            let v = numbers(size, 2, &format!("{name}:widthxlength, like {}", k.example.split(':').nth(1).unwrap_or("")))?;
            let (width, length) = (v[0], v[1]);
            let belt = belt_of(name.trim_end_matches("belt"))?;
            if (length / belt.1).fract().abs() > 1.0e-9 {
                bail!("a {} belt is a whole number of {} mm teeth; {length} mm is not", k.standard, belt.1);
            }
            let pitch_dia = length / std::f64::consts::PI;
            let outer = pitch_dia + belt.3 / 2.0 + belt.3 / 2.0;
            let inner = pitch_dia - belt.3;
            let rows = row(&[
                kv("teeth", format!("{}", (length / belt.1) as u32)),
                kv("pitch length", format!("{length} mm")),
                kv("width", format!("{width} mm")),
                kv("thickness", format!("{} mm", belt.3)),
                kv("envelope", format!("ring {outer:.1} across, {width} wide")),
            ]);
            let source = vec![
                format!("circle {outer}"),
                format!("belt: extrude {width}"),
                "plane belt.end".into(),
                format!("circle {inner}"),
                "bore: cut thru".into(),
                "material nylon".into(),
            ];
            let mut part = finish(k, spec, &format!("{width}x{length}"), None, rows, source, [outer, outer, width]);
            part.material = Some("nylon");
            Ok(part)
        }
        "extrusion" => {
            let (profile, length) = size
                .split_once(['x', 'X'])
                .ok_or_else(|| anyhow!("write extrusion:2020x300, the profile then the length in mm"))?;
            let e = f.extrusion.iter().find(|e| e.0 == profile).ok_or_else(|| {
                anyhow!(
                    "extrusion profiles are {}",
                    f.extrusion.iter().map(|e| e.0.as_str()).collect::<Vec<_>>().join(", ")
                )
            })?;
            let length: f64 = length
                .parse()
                .ok()
                .filter(|l: &f64| *l > 0.0)
                .ok_or_else(|| anyhow!("`{length}` is not a length"))?;
            let rows = row(&[
                kv("section", format!("{0} x {0} mm", e.1)),
                kv("centre bore", format!("{} mm", e.2)),
                kv("length", format!("{length} mm")),
                kv("model", "clearance envelope, no slots".into()),
            ]);
            let source = vec![
                format!("rect {0} {0}", e.1),
                format!("profile: extrude {length}"),
                format!("plane profile.end"),
                format!("circle {}", e.2),
                "bore: cut thru".into(),
            ];
            Ok(finish(k, spec, &format!("{profile} x {length}"), Some("aluminium"), rows, source, [e.1, e.1, length]))
        }
        _ => {
            let (label, dims) = size
                .split_once('@')
                .ok_or_else(|| anyhow!("write spec:name@WxDxH, like spec:nema17@42x42x40"))?;
            if label.is_empty() {
                bail!("a spec part needs a name before the @");
            }
            let v = numbers(dims, 3, "spec:name@WxDxH")?;
            let rows = row(&[
                kv("name", label.to_string()),
                kv("clearance envelope", format!("{} x {} x {} mm", v[0], v[1], v[2])),
                kv("model", "bought part, not modelled".into()),
            ]);
            let source = vec![
                format!("rect {} {}", v[0], v[1]),
                format!("body: extrude {}", v[2]),
            ];
            Ok(finish(k, spec, &format!("{label} {}x{}x{}", v[0], v[1], v[2]), None, rows, source, [v[0], v[1], v[2]]))
        }
    }
}

fn belt_of(name: &str) -> Result<(String, f64, f64, f64)> {
    file()
        .belt
        .iter()
        .find(|b| b.0 == name)
        .cloned()
        .ok_or_else(|| anyhow!("no belt profile `{name}`"))
}

pub fn list(kind_name: Option<&str>) -> Result<String> {
    let f = file();
    let Some(name) = kind_name else {
        let mut out = vec!["spec kind   standard          group      model     example".to_string()];
        for k in KINDS {
            out.push(format!(
                "{:<11} {:<17} {:<10} {:<9} {}",
                k.name,
                k.standard,
                k.group,
                if k.modelled { "modelled" } else { "envelope" },
                k.example
            ));
        }
        out.push("use a spec string as `part x iso4762:M6x25` in an assembly".into());
        return Ok(out.join("\n"));
    };
    let k = kind(name).ok_or_else(|| {
        anyhow!("unknown part `{name}`; known: {}", KINDS.iter().map(|k| k.name).collect::<Vec<_>>().join(", "))
    })?;
    let sizes: Vec<String> = match name {
        "iso4762" | "iso7380" | "iso10642" => thread::all()
            .iter()
            .filter(|t| t.family == thread::Family::Metric && t.coarse)
            .filter(|t| thread::heads(t).iter().any(|h| h.standard == k.standard))
            .map(|t| t.name.clone())
            .collect(),
        "iso4017" | "iso4014" => f.hex_head.iter().map(|r| format!("M{}", r[0])).collect(),
        "iso4032" => f.nut4032.iter().map(|r| format!("M{}", r[0])).collect(),
        "iso4035" => f.nut4035.iter().map(|r| format!("M{}", r[0])).collect(),
        "iso7040" => f.nut7040.iter().map(|r| format!("M{}", r[0])).collect(),
        "iso7089" => f.washer7089.iter().map(|r| format!("M{}", r[0])).collect(),
        "iso7093" => f.washer7093.iter().map(|r| format!("M{}", r[0])).collect(),
        "iso2338" => f.dowel_2338.iter().map(|d| format!("{d}")).collect(),
        "iso8734" => f.dowel_8734.iter().map(|d| format!("{d}")).collect(),
        "din6885" => f.key6885.iter().map(|r| format!("{}x{}", r[2], r[3])).collect(),
        "din471" => f.circlip471.iter().map(|r| format!("{}", r[0])).collect(),
        "din472" => f.circlip472.iter().map(|r| format!("{}", r[0])).collect(),
        "extrusion" => f.extrusion.iter().map(|e| e.0.clone()).collect(),
        _ => vec![k.example.split(':').nth(1).unwrap_or("").to_string()],
    };
    Ok(format!("{} ({}), sizes: {}\nexample: {}", k.standard, k.group, sizes.join(" "), k.example))
}

pub fn report(part: &Part) -> Report {
    let mut rows = vec![
        ("spec".to_string(), part.spec.clone()),
        ("title".to_string(), part.title.clone()),
        (
            "model".to_string(),
            if part.modelled { "modelled".into() } else { "clearance envelope only".into() },
        ),
    ];
    rows.extend(part.rows.iter().cloned());
    rows.push((
        "envelope".to_string(),
        format!("{:.2} x {:.2} x {:.2} mm", part.envelope[0], part.envelope[1], part.envelope[2]),
    ));
    rows.push((
        "bom line".to_string(),
        format!("1, {}, {}", part.title, part.material.unwrap_or("no material")),
    ));
    Report {
        title: format!("part {}", part.spec),
        rows,
        why: format!("Write `part name {}` in an assembly; the BOM lists it as {}.", part.spec, part.title),
        formulas: vec!["pulley outside diameter = teeth x pitch / pi - 2 x pitch line differential".into()],
        sources: vec![
            "tables in crates/gcad-calc/src/data/parts.toml, each cited in its header".into(),
        ],
    }
}

pub fn show(spec: &str) -> Result<String> {
    Ok(report(&resolve(spec)?).render(false))
}
