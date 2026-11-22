use super::{Anchor, BLACK, BODY, Mark, State};
use crate::drawing::section_of;
use crate::geometry;
use crate::parse::{Line, eval};
use anyhow::{Result, anyhow, bail};
use gcad_cloud::colour::diverging;
use monstertruck::modeling::Vector3;

const MARKER: f64 = 0.35;
const FLAG: f64 = 0.9;
const GAP: f64 = 14.0;

fn named<'a>(line: &'a Line, key: &str) -> Option<&'a str> {
    line.named
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn rgb(c: [u8; 3]) -> [f64; 3] {
    c.map(|v| v as f64 / 255.0)
}

fn square(x: f64, y: f64, half: f64) -> Vec<(f64, f64)> {
    vec![
        (x - half, y - half),
        (x + half, y - half),
        (x + half, y + half),
        (x - half, y + half),
    ]
}

impl State {
    pub(super) fn op_scansection(&mut self, line: &Line) -> Result<String> {
        let scan_name = line
            .positional
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("write `scansection scan to=part axis=z at=10`"))?;
        let part = named(line, "to")
            .ok_or_else(|| anyhow!("`scansection` needs `to=` the part to compare with"))?;
        let axis = match named(line, "axis").unwrap_or("z") {
            "x" => 0,
            "y" => 1,
            "z" => 2,
            other => bail!("`axis` is x, y or z, not `{other}`"),
        };
        let at = eval(
            named(line, "at").ok_or_else(|| anyhow!("`scansection` needs `at=`"))?,
            &self.scope,
        )?;
        let thickness = match named(line, "thickness") {
            Some(t) => eval(t, &self.scope)?,
            None => 1.0,
        };
        let scale = self.sheet()?.scale;
        let (cloud, solid) = {
            let model = self.model()?;
            let cloud = model
                .cloud(&scan_name)
                .cloned()
                .ok_or_else(|| anyhow!("`{scan_name}` is not a point cloud in the part"))?;
            let solid = model
                .named_solids()
                .into_iter()
                .find(|(n, _)| n == part)
                .map(|(_, s)| s.clone())
                .ok_or_else(|| anyhow!("`{part}` is not a solid in the part"))?;
            (cloud, solid)
        };
        let tolerance = match named(line, "tol") {
            Some(t) => eval(t, &self.scope)?,
            None => gcad_cloud::colour::deviation_limit(&cloud).max(1.0e-9),
        };
        let (iu, iv) = match axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let normal = [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()][axis];
        let mesh = geometry::mesh(&solid, geometry::mesh_tolerance(&solid));
        let loops: Vec<Vec<(f64, f64)>> = section_of(&mesh, &solid, normal, at)
            .into_iter()
            .map(|l| {
                l.into_iter()
                    .map(|p| {
                        let c = [p.x, p.y, p.z];
                        (c[iu], c[iv])
                    })
                    .collect()
            })
            .collect();
        if loops.is_empty() {
            bail!("the plane at {at} misses `{part}`");
        }
        let slab: Vec<usize> = (0..cloud.len())
            .filter(|&i| (cloud.positions[i][axis] - at).abs() <= thickness / 2.0)
            .collect();
        let mut extent = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for &(u, v) in loops.iter().flatten() {
            extent = (
                extent.0.min(u),
                extent.1.min(v),
                extent.2.max(u),
                extent.3.max(v),
            );
        }
        let (w, h) = ((extent.2 - extent.0) / scale, (extent.3 - extent.1) / scale);
        let first = self.place(w, h, None)?;
        let second = self.place(w, h, None)?;
        let place = |origin: (f64, f64), (u, v): (f64, f64)| {
            (
                origin.0 + (u - extent.0) / scale,
                origin.1 - (v - extent.1) / scale,
            )
        };
        let colour = self.model()?.colours.get(part).copied().unwrap_or(BODY);
        let outline = |origin: (f64, f64)| -> Vec<Vec<(f64, f64)>> {
            loops
                .iter()
                .map(|l| l.iter().map(|&p| place(origin, p)).collect())
                .collect()
        };
        let mut marks = vec![
            Mark::Shape {
                loops: outline(first),
                closed: true,
                fill: Some(colour),
                stroke: Some(BLACK),
                width: 0.2,
                dashed: false,
            },
            Mark::Shape {
                loops: outline(second),
                closed: true,
                fill: None,
                stroke: Some(BLACK),
                width: 0.15,
                dashed: true,
            },
        ];
        let mut flagged = 0usize;
        let mut worst = 0.0f64;
        let mut drawn: Vec<(f64, f64, Option<f32>)> = Vec::new();
        for &i in &slab {
            let p = cloud.positions[i];
            let dev = cloud.deviation.as_ref().map(|d| d[i]);
            drawn.push((p[iu], p[iv], dev));
        }
        let step = (drawn.len() / 4000).max(1);
        let limit = tolerance.max(1.0e-9) as f32;
        for &(u, v, dev) in drawn.iter().step_by(step) {
            let (x, y) = place(second, (u, v));
            let c = match dev {
                Some(d) => rgb(diverging(0.5 + 0.5 * (d / limit).clamp(-1.0, 1.0))),
                None => [0.3, 0.3, 0.3],
            };
            marks.push(Mark::Shape {
                loops: vec![square(x, y, MARKER)],
                closed: true,
                fill: Some(c),
                stroke: None,
                width: 0.0,
                dashed: false,
            });
        }
        for &(u, v, dev) in &drawn {
            let Some(d) = dev else { continue };
            worst = worst.max(d.abs() as f64);
            if d.abs() as f64 > tolerance {
                flagged += 1;
                let (x, y) = place(second, (u, v));
                marks.push(Mark::Shape {
                    loops: vec![square(x, y, FLAG)],
                    closed: false,
                    fill: None,
                    stroke: Some(if d > 0.0 {
                        [0.85, 0.1, 0.1]
                    } else {
                        [0.1, 0.2, 0.85]
                    }),
                    width: 0.15,
                    dashed: false,
                });
            }
        }
        let axis_name = ["x", "y", "z"][axis];
        for (origin, text) in [
            (first, format!("CAD section {axis_name}={at}")),
            (
                second,
                format!(
                    "scan section {axis_name}={at}, {} points, {flagged} beyond {}",
                    slab.len(),
                    gcad_parse::label(tolerance)
                ),
            ),
        ] {
            marks.push(Mark::Text {
                x: origin.0,
                y: origin.1 - h - 4.0,
                size: 3.0,
                text,
                anchor: Anchor::Start,
                angle: 0.0,
            });
        }
        let _ = GAP;
        self.sheet()?.marks.extend(marks);
        if let Some(name) = line.label.as_deref() {
            self.scope
                .insert(format!("{name}.points"), slab.len() as f64);
            self.scope.insert(format!("{name}.flagged"), flagged as f64);
            self.scope.insert(format!("{name}.worst"), worst);
        }
        Ok(format!(
            "{} scan points in the slab, {flagged} beyond {}, worst {}",
            slab.len(),
            gcad_parse::label(tolerance),
            gcad_parse::label(worst)
        ))
    }
}
