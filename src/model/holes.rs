use super::args::{Args, label_of, positive};
use super::features::Depth;
use super::solids::loft_wires;
use super::{Combine, Model};
use crate::geometry::Frame;
use crate::geometry::Profile;
use crate::parse::{Line, eval_point};
use anyhow::{Result, bail};
use monstertruck::modeling::*;

struct Piece {
    top: (f64, f64),
    bottom: (f64, f64),
    side: &'static str,
    floor: Option<&'static str>,
}

impl Model {
    fn round_side(&self, selector: &str, spots: &[(f64, f64)]) -> Result<Vec<(Frame, (f64, f64))>> {
        let owner = selector.strip_suffix(".side").ok_or_else(|| {
            anyhow::anyhow!("`on=` takes the side of a round extrusion, like `rod.side`")
        })?;
        let prism = self
            .prisms
            .get(owner)
            .ok_or_else(|| anyhow::anyhow!("`{owner}` is not an extrusion"))?;
        let [Profile::Circle { center, diameter }] = prism.profiles.as_slice() else {
            bail!(
                "holes on curved faces go in the side of an extruded circle; `{owner}` is not one"
            );
        };
        let axis = prism.frame.at(center.0, center.1);
        let along = prism.frame.normal * prism.distance.signum();
        let length = prism.distance.abs();
        spots
            .iter()
            .map(|&(angle, height)| {
                if !(0.0..=length).contains(&height) {
                    bail!("height {height} is off the side of `{owner}`, which is {length} long");
                }
                let (s, c) = angle.to_radians().sin_cos();
                let outward = prism.frame.x * c + prism.frame.y * s;
                let surface = axis + along * height + outward * (diameter / 2.0);
                Ok((Frame::from_normal(surface, outward), (0.0, 0.0)))
            })
            .collect()
    }

    pub(crate) fn op_hole(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["d"],
            &["depth", "cbore", "csink", "thread", "on"],
            true,
        )?;
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
                let (major, tap, _) = super::thread::thread_named(name)?;
                if diameter > major || diameter < tap * 0.9 {
                    bail!("a {name} thread needs a {tap} mm tap drill, the hole is {diameter}");
                }
                Some(name.to_uppercase())
            }
        };
        let placements: Vec<(Frame, (f64, f64))> = match args.values.get("on") {
            None => centers.iter().map(|c| (self.sketch_frame(), *c)).collect(),
            Some(selector) => self.round_side(selector, &centers)?,
        };
        let label = label_of(line);
        let solid = self.active("hole")?.clone();
        let bounds = crate::geometry::bounds(&solid);
        let clearance = (bounds.diameter() * 0.01).max(1.0e-3);
        let radius = diameter / 2.0;
        let corners = [bounds.min(), bounds.max()];
        let farthest = placements
            .iter()
            .flat_map(|(frame, _)| {
                (0..8).map(move |i| {
                    let corner = Point3::new(
                        corners[i & 1].x,
                        corners[(i >> 1) & 1].y,
                        corners[(i >> 2) & 1].z,
                    );
                    (frame.origin - corner).dot(frame.normal)
                })
            })
            .fold(0.0, f64::max);
        let end = match depth {
            Depth::Blind(d) => d,
            Depth::Through => farthest + clearance,
        };
        let mut pieces: Vec<Piece> = Vec::new();
        match (args.values.get("cbore"), args.values.get("csink")) {
            (Some(_), Some(_)) => bail!("a hole takes `cbore=` or `csink=`, not both"),
            (Some(text), None) => {
                let (bore, bore_depth) = eval_point(text, &self.scope)?;
                if bore <= diameter || bore_depth <= 0.0 || bore_depth >= end {
                    bail!("cbore=diameter,depth must be wider than the hole and shallower than it");
                }
                pieces.push(Piece {
                    top: (bore / 2.0, -clearance),
                    bottom: (bore / 2.0, bore_depth),
                    side: "cbore",
                    floor: Some("cbore_floor"),
                });
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
                let past = (radius * 0.5 / slope).min((end - sink_depth) / 2.0);
                pieces.push(Piece {
                    top: (sink / 2.0 + clearance * slope, -clearance),
                    bottom: (radius - past * slope, sink_depth + past),
                    side: "csink",
                    floor: None,
                });
            }
            (None, None) => {}
        }
        pieces.insert(
            0,
            Piece {
                top: (radius, -clearance),
                bottom: (radius, end),
                side: "side",
                floor: matches!(depth, Depth::Blind(_)).then_some("bottom"),
            },
        );
        let mut batches: Vec<Vec<Solid>> = vec![Vec::new(); pieces.len()];
        for (frame, center) in placements {
            for (k, piece) in pieces.iter().enumerate() {
                let circle = |(r, z): (f64, f64)| {
                    Profile::Circle {
                        center,
                        diameter: 2.0 * r,
                    }
                    .wire(&frame.offset(-z))
                };
                let tool = if piece.top.0 == piece.bottom.0 {
                    let start = frame.offset(-piece.top.1);
                    let profiles = [Profile::Circle {
                        center,
                        diameter: 2.0 * piece.top.0,
                    }];
                    let shapes = super::solids::loops(&start, &profiles)?;
                    super::solids::prism(
                        &start,
                        &shapes,
                        &[0],
                        &profiles,
                        -frame.normal * (piece.bottom.1 - piece.top.1),
                        (0.0, 0.0),
                    )?
                } else {
                    loft_wires(&[circle(piece.top), circle(piece.bottom)])?
                };
                let groups = super::solids::classify(&tool, -frame.normal)
                    .into_iter()
                    .filter_map(|(group, surface)| match group {
                        "side" => Some((piece.side, surface)),
                        "end" => piece.floor.map(|floor| (floor, surface)),
                        _ => None,
                    })
                    .collect();
                self.record_inverted(&label, groups);
                batches[k].push(tool);
            }
        }
        for batch in batches {
            self.merge_all(&label, batch, Combine::Remove)?;
        }
        let summary = self.describe_solid()?;
        Ok(match thread {
            Some(name) => format!("{name} tapped; {summary}"),
            None => summary,
        })
    }
}
