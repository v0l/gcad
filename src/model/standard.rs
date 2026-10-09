use super::fastener::{Metric, named};
use anyhow::{Result, anyhow, bail};

pub const KINDS: &[(&str, &str)] = &[
    ("shcs", "ISO 4762"),
    ("fhcs", "ISO 10642"),
    ("hex", "ISO 4017"),
    ("nut", "ISO 4032"),
    ("washer", "ISO 7089"),
    ("insert", "heat-set insert"),
];

pub struct Standard {
    pub title: String,
    pub source: String,
}

pub fn is_standard(spec: &str) -> bool {
    spec.split_once(':')
        .is_some_and(|(kind, _)| KINDS.iter().any(|(k, _)| *k == kind))
}

fn sized(size: &str, kind: &str) -> Result<(&'static Metric, Option<f64>)> {
    let (thread, length) = match size.split_once(['x', 'X']) {
        Some((thread, length)) => (
            thread,
            Some(
                length
                    .parse::<f64>()
                    .map_err(|_| anyhow!("`{length}` is not a length in `{kind}:{size}`"))?,
            ),
        ),
        None => (size, None),
    };
    Ok((named(thread)?, length))
}

fn hex_corners(across_flats: f64) -> f64 {
    across_flats / (std::f64::consts::PI / 6.0).cos()
}

pub fn standard(spec: &str) -> Result<Standard> {
    let (kind, size) = spec
        .split_once(':')
        .ok_or_else(|| anyhow!("a standard part is written kind:size, like shcs:M3x10"))?;
    let standard = KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, s)| *s)
        .ok_or_else(|| {
            anyhow!(
                "unknown standard part `{kind}`; known: {}",
                KINDS.iter().map(|k| k.0).collect::<Vec<_>>().join(", ")
            )
        })?;
    let (m, length) = sized(size, kind)?;
    let screw = matches!(kind, "shcs" | "fhcs" | "hex");
    let length = match (screw, length) {
        (true, Some(l)) if l > 0.0 => Some(l),
        (true, _) => bail!("a screw needs a length, like {kind}:{}x10", m.name),
        (false, Some(_)) => bail!("`{kind}` takes a size alone, like {kind}:{}", m.name),
        (false, None) => None,
    };
    let d = m.major;
    let lines: Vec<String> = match kind {
        "shcs" => {
            let (head, k) = m.socket_head;
            let (key, depth) = m.socket_key;
            let l = length.unwrap_or_default();
            vec![
                format!("circle {head}"),
                format!("head: extrude {k}"),
                "plane head.end".into(),
                format!("ngon {} 6", hex_corners(key)),
                format!("socket: cut {depth}"),
                "plane XY".into(),
                format!("circle {d}"),
                format!("shank: extrude -{l}"),
            ]
        }
        "fhcs" => {
            let h = m.flat_head_height();
            let l = length.unwrap_or_default();
            if l <= h {
                bail!(
                    "a {} flat head screw is longer than its {h:.2} head",
                    m.name
                );
            }
            let (key, depth) = m.flat_key;
            vec![
                format!("circle {}", m.flat_head),
                format!("head: extrude -{h} draft=45"),
                "plane XY".into(),
                format!("ngon {} 6", hex_corners(key)),
                format!("socket: cut {depth}"),
                format!("plane XY offset=-{h}"),
                format!("circle {d}"),
                format!("shank: extrude -{}", l - h),
            ]
        }
        "hex" => {
            let (across, k) = m.hex_head;
            let l = length.unwrap_or_default();
            vec![
                format!("ngon {} 6", hex_corners(across)),
                format!("head: extrude {k}"),
                format!("circle {d}"),
                format!("shank: extrude -{l}"),
            ]
        }
        "nut" => {
            let (across, height) = m.nut;
            vec![
                format!("ngon {} 6", hex_corners(across)),
                format!("nut: extrude {height}"),
                "plane nut.end".into(),
                format!("circle {d}"),
                "bore: cut thru".into(),
            ]
        }
        "washer" => {
            let (outer, thick) = m.washer;
            vec![
                format!("circle {outer}"),
                format!("washer: extrude {thick}"),
                "plane washer.end".into(),
                format!("circle {}", m.clearance[0]),
                "bore: cut thru".into(),
            ]
        }
        _ => {
            let (_, long, outer) = m
                .insert
                .ok_or_else(|| anyhow!("there is no heat-set insert listed for {}", m.name))?;
            vec![
                format!("circle {outer}"),
                format!("insert: extrude -{long}"),
                "plane XY".into(),
                format!("circle {d}"),
                "bore: cut thru".into(),
            ]
        }
    };
    let material = if kind == "insert" { "brass" } else { "steel" };
    let title = match length {
        Some(l) => format!("{standard} {}x{l}", m.name),
        None => format!("{standard} {}", m.name),
    };
    let source = lines
        .into_iter()
        .chain([format!("material {material}")])
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Standard { title, source })
}
