use super::args::{Args, label_of, positive};
use super::features::Depth;
use super::solids::loft_wires;
use super::{Combine, Model};
use crate::geometry::Profile;
use crate::parse::{Line, eval_point};
use anyhow::{Result, bail};
use monstertruck::modeling::InnerSpace;

const THREADS: &[(&str, f64, f64)] = &[
    ("M2", 2.0, 1.6),
    ("M2.5", 2.5, 2.05),
    ("M3", 3.0, 2.5),
    ("M4", 4.0, 3.3),
    ("M5", 5.0, 4.2),
    ("M6", 6.0, 5.0),
    ("M8", 8.0, 6.8),
    ("M10", 10.0, 8.5),
    ("M12", 12.0, 10.2),
];

impl Model {
    pub(crate) fn op_hole(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["d"], &["depth", "cbore", "csink", "thread"], true)?;
        let diameter = positive(args.number("d", &self.scope)?, "hole diameter")?;
        if args.rest.is_empty() {
            bail!("`hole` needs at least one x,y position after the diameter");
        }
        self.require_empty_sketch("hole")?;
        let centers = args
            .rest
            .iter()
            .map(|text| eval_point(text, &self.scope))
            .collect::<Result<Vec<_>>>()?;
        let depth = match args.values.get("depth") {
            None | Some(&"thru") => Depth::Through,
            Some(_) => Depth::Blind(positive(args.number("depth", &self.scope)?, "hole depth")?),
        };
        let thread = match args.values.get("thread") {
            None => None,
            Some(name) => {
                let (_, major, tap) = THREADS
                    .iter()
                    .find(|(known, _, _)| known.eq_ignore_ascii_case(name))
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "unknown thread `{name}`; known: {}",
                            THREADS.iter().map(|t| t.0).collect::<Vec<_>>().join(", ")
                        )
                    })?;
                if diameter > *major || diameter < tap * 0.9 {
                    bail!("a {name} thread needs a {tap} mm tap drill, the hole is {diameter}");
                }
                Some(name.to_uppercase())
            }
        };
        let frame = self.sketch_frame();
        let label = label_of(line);
        let solid = self.active("hole")?.clone();
        let bounds = crate::geometry::bounds(&solid);
        let clearance = (bounds.diameter() * 0.01).max(1.0e-3);
        let radius = diameter / 2.0;
        let corners = [bounds.min(), bounds.max()];
        let farthest = (0..8)
            .map(|i| {
                monstertruck::modeling::Point3::new(
                    corners[i & 1].x,
                    corners[(i >> 1) & 1].y,
                    corners[(i >> 2) & 1].z,
                )
            })
            .map(|corner| (frame.origin - corner).dot(frame.normal))
            .fold(0.0, f64::max);
        let end = match depth {
            Depth::Blind(d) => d,
            Depth::Through => farthest + clearance,
        };
        let mut stack: Vec<(f64, f64)> = Vec::new();
        let mut names: Vec<&str> = Vec::new();
        match (args.values.get("cbore"), args.values.get("csink")) {
            (Some(_), Some(_)) => bail!("a hole takes `cbore=` or `csink=`, not both"),
            (Some(text), None) => {
                let (bore, bore_depth) = eval_point(text, &self.scope)?;
                if bore <= diameter || bore_depth <= 0.0 || bore_depth >= end {
                    bail!("cbore=diameter,depth must be wider than the hole and shallower than it");
                }
                stack.extend([
                    (bore / 2.0, -clearance),
                    (bore / 2.0, bore_depth),
                    (radius, bore_depth),
                ]);
                names.extend(["cbore", "cbore_floor"]);
            }
            (None, Some(text)) => {
                let (sink, angle) = eval_point(text, &self.scope)?;
                if sink <= diameter || angle <= 0.0 || angle >= 180.0 {
                    bail!(
                        "csink=diameter,angle must be wider than the hole with an angle between 0 and 180"
                    );
                }
                let slope = (angle / 2.0).to_radians().tan();
                let sink_depth = (sink / 2.0 - radius) / slope;
                if sink_depth >= end {
                    bail!("the countersink is deeper than the hole");
                }
                stack.extend([
                    (sink / 2.0 + clearance * slope, -clearance),
                    (radius, sink_depth),
                ]);
                names.push("csink");
            }
            (None, None) => stack.push((radius, -clearance)),
        }
        stack.push((radius, end));
        names.push("side");
        for center in centers {
            let wires: Vec<_> = stack
                .iter()
                .map(|&(r, z)| {
                    Profile::Circle {
                        center,
                        diameter: 2.0 * r,
                    }
                    .wire(&frame.offset(-z))
                })
                .collect();
            let tool = loft_wires(&wires)?;
            let faces = crate::select::faces(&tool);
            let strips = faces.len() - 2;
            let per_strip = strips / (stack.len() - 1);
            let groups = faces
                .iter()
                .enumerate()
                .map(|(i, face)| {
                    let name = if i < strips {
                        names[i / per_strip]
                    } else if i == strips {
                        "top"
                    } else {
                        "bottom"
                    };
                    (name, face.oriented_surface())
                })
                .collect();
            self.record_inverted(&label, groups);
            self.merge(&label, tool, Combine::Remove)?;
        }
        let summary = self.describe_solid()?;
        Ok(match thread {
            Some(name) => format!("{name} tapped; {summary}"),
            None => summary,
        })
    }
}
