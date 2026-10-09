use super::Model;
use super::args::{Args, label_of, positive};
use super::features::{Depth, Removal};
use crate::geometry::{self, Frame, Profile, Segment};
use crate::parse::{Line, eval_point, parse_line};
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

type P2 = (f64, f64);

fn signed_area(points: &[P2]) -> f64 {
    (0..points.len())
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f64>()
        / 2.0
}

fn distance_to_segment(p: P2, a: P2, b: P2) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

fn circle_through(a: P2, b: P2, c: P2) -> Option<(P2, f64)> {
    let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    if d.abs() < 1.0e-12 {
        return None;
    }
    let s = |p: P2| p.0 * p.0 + p.1 * p.1;
    let x = (s(a) * (b.1 - c.1) + s(b) * (c.1 - a.1) + s(c) * (a.1 - b.1)) / d;
    let y = (s(a) * (c.0 - b.0) + s(b) * (a.0 - c.0) + s(c) * (b.0 - a.0)) / d;
    Some(((x, y), (a.0 - x).hypot(a.1 - y)))
}

struct Outline {
    start: P2,
    segments: Vec<Segment>,
    samples: Vec<P2>,
}

impl Outline {
    fn profile(&self) -> Profile {
        Profile::Path {
            start: self.start,
            segments: self.segments.clone(),
        }
    }

    fn area(&self) -> f64 {
        signed_area(&self.samples).abs()
    }

    fn distance(&self, p: P2) -> f64 {
        let mut from = self.start;
        let mut nearest = f64::INFINITY;
        for segment in &self.segments {
            let to = segment.end();
            let gap = match segment {
                Segment::Arc { via, .. } => match circle_through(from, *via, to) {
                    Some((c, r)) => {
                        let span = |q: P2| (q.1 - c.1).atan2(q.0 - c.0);
                        let (a0, am, a1) = (span(from), span(*via), span(to));
                        let turn = |a: f64, b: f64| (b - a).rem_euclid(std::f64::consts::TAU);
                        let ccw = turn(a0, am) < turn(a0, a1);
                        let within = if ccw {
                            turn(a0, span(p)) <= turn(a0, a1)
                        } else {
                            turn(a1, span(p)) <= turn(a1, a0)
                        };
                        if within {
                            ((p.0 - c.0).hypot(p.1 - c.1) - r).abs()
                        } else {
                            (p.0 - from.0)
                                .hypot(p.1 - from.1)
                                .min((p.0 - to.0).hypot(p.1 - to.1))
                        }
                    }
                    None => distance_to_segment(p, from, to),
                },
                _ => distance_to_segment(p, from, to),
            };
            nearest = nearest.min(gap);
            from = to;
        }
        nearest
    }
}

fn outline_of(wire: &Wire, frame: &Frame, tolerance: f64) -> Outline {
    let mut segments = Vec::new();
    let mut samples = Vec::new();
    let mut start = None;
    for edge in wire.edge_iter() {
        let curve = edge.oriented_curve();
        let (t0, t1) = curve.range_tuple();
        let points: Vec<P2> = (0..=16)
            .map(|i| frame.local(curve.subs(t0 + (t1 - t0) * i as f64 / 16.0)))
            .collect();
        start.get_or_insert(points[0]);
        samples.extend(&points[..16]);
        let (first, last) = (points[0], points[16]);
        let straight = points
            .iter()
            .all(|&p| distance_to_segment(p, first, last) < tolerance)
            && (first.0 - last.0).hypot(first.1 - last.1) > tolerance;
        if straight {
            segments.push(Segment::Line(last));
            continue;
        }
        let round = circle_through(points[0], points[5], points[11]).filter(|&(c, r)| {
            points
                .iter()
                .all(|p| ((p.0 - c.0).hypot(p.1 - c.1) - r).abs() < tolerance)
        });
        match round {
            Some(_) => segments.extend([
                Segment::Arc {
                    to: points[8],
                    via: points[4],
                },
                Segment::Arc {
                    to: last,
                    via: points[12],
                },
            ]),
            None => segments.extend(points[1..].iter().map(|&p| Segment::Line(p))),
        }
    }
    Outline {
        start: start.unwrap_or_default(),
        segments,
        samples,
    }
}

impl Model {
    fn face_outlines(&self, selector: &str) -> Result<(Frame, Vec<Outline>)> {
        let frame = self.face_frame(selector)?;
        let solid = self.active("lip")?;
        let tolerance = self.tolerance() * 100.0;
        let faces = select::faces(solid);
        let outlines = select::select_faces(selector, solid, &self.groups, self.tolerance())?
            .into_iter()
            .flat_map(|i| faces[i].boundaries())
            .map(|wire| outline_of(&wire, &frame, tolerance))
            .collect();
        Ok((frame, outlines))
    }

    fn rim(&self, selector: &str, op: &str) -> Result<(Frame, Outline, Outline)> {
        let (frame, mut outlines) = self.face_outlines(selector)?;
        if outlines.len() != 2 {
            bail!(
                "`{op}` takes the flat rim of a hollow part, a face with one outline and one hole; `{selector}` has {} outlines",
                outlines.len()
            );
        }
        outlines.sort_by(|a, b| b.area().total_cmp(&a.area()));
        let inner = outlines.pop().expect("two");
        let outer = outlines.pop().expect("two");
        Ok((frame, outer, inner))
    }

    fn wall(outer: &Outline, inner: &Outline) -> f64 {
        inner
            .samples
            .iter()
            .map(|&p| outer.distance(p))
            .fold(f64::INFINITY, f64::min)
    }

    pub(crate) fn op_lip(&mut self, line: &Line, groove: bool) -> Result<String> {
        let op = if groove { "groove" } else { "lip" };
        let args = Args::new(line, &["faces", "h"], &["w", "gap"], false)?;
        let selector = args.text("faces")?;
        let height = positive(args.number("h", &self.scope)?, "h")?;
        let (frame, outer, inner) = self.rim(selector, op)?;
        let wall = Self::wall(&outer, &inner);
        let width = match args.optional_number("w", &self.scope)? {
            Some(w) => positive(w, "w")?,
            None => wall / 2.0,
        };
        let gap = match args.optional_number("gap", &self.scope)? {
            Some(g) if g < 0.0 => bail!("gap cannot be negative"),
            Some(g) => g,
            None if groove => 0.2,
            None => 0.0,
        };
        if width + gap >= wall {
            bail!(
                "the {op} is {:.3} wide but the wall is only {wall:.3}",
                width + gap
            );
        }
        let clear = wall / 4.0;
        let grow = |outline: &Outline, by: f64| -> Result<Profile> {
            outline
                .profile()
                .inset(-by)
                .map_err(|e| anyhow!("{op}: {e}"))
        };
        let (profiles, depth) = if groove {
            (
                vec![grow(&inner, width + gap)?, grow(&inner, -clear)?],
                height + gap,
            )
        } else {
            (
                vec![grow(&outer, clear)?, grow(&inner, width + gap)?],
                height,
            )
        };
        let label = label_of(line);
        let summary = self.remove(
            &label,
            Removal {
                frame,
                profiles,
                depth: Depth::Blind(depth),
                taper: 0.0,
                side: "side",
                end: "step",
            },
        )?;
        Ok(format!(
            "{op} {:.3} wide, {depth:.3} deep on a {wall:.3} wall; {summary}",
            width + gap
        ))
    }

    fn wall_under(&self, frame: &Frame, spots: &[P2]) -> Result<f64> {
        let solid = self.active("vent")?;
        let mesh = geometry::mesh(solid, geometry::mesh_tolerance(solid));
        let size = geometry::bounds(solid).diameter();
        let lift = size * 1.0e-3;
        spots
            .iter()
            .map(|&(u, v)| {
                let origin = frame.at(u, v) + frame.normal * lift;
                let mut hits: Vec<f64> = geometry::ray_hits(&mesh, origin, -frame.normal)
                    .into_iter()
                    .map(|(t, _)| t)
                    .filter(|t| *t > 0.0)
                    .collect();
                hits.sort_by(f64::total_cmp);
                hits.dedup_by(|a, b| (*a - *b).abs() < size * 1.0e-7);
                match hits.as_slice() {
                    [enter, exit, ..] if (enter - lift).abs() < lift * 0.5 => Ok(exit - lift),
                    _ => bail!(
                        "there is no wall under ({u}, {v}) on the workplane; put the plane on the face to vent"
                    ),
                }
            })
            .try_fold(0.0, |most: f64, wall| wall.map(|w| most.max(w)))
    }

    pub(crate) fn op_vent(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["l", "w"],
            &["count", "step", "angle", "depth"],
            true,
        )?;
        let length = positive(args.number("l", &self.scope)?, "l")?;
        let width = positive(args.number("w", &self.scope)?, "w")?;
        if length <= width {
            bail!("vent length {length} must be longer than its width {width}");
        }
        self.require_empty_sketch("vent")?;
        if args.rest.is_empty() {
            bail!("`vent` needs at least one x,y position after its length and width");
        }
        let count = args.optional_number("count", &self.scope)?.unwrap_or(1.0);
        if count < 1.0 || count.fract() != 0.0 {
            bail!("count must be a whole number of at least 1");
        }
        let step = match args.values.get("step") {
            Some(text) => eval_point(text, &self.scope)?,
            None if count > 1.0 => bail!("give `step=x,y` to space {count} vents"),
            None => (0.0, 0.0),
        };
        let angle = args.optional_number("angle", &self.scope)?.unwrap_or(0.0);
        let centres: Vec<P2> = args
            .rest
            .iter()
            .map(|text| eval_point(text, &self.scope))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flat_map(|(x, y)| {
                (0..count as usize).map(move |k| (x + step.0 * k as f64, y + step.1 * k as f64))
            })
            .collect();
        let frame = self.sketch_frame();
        let depth = match args.optional_number("depth", &self.scope)? {
            Some(d) => positive(d, "depth")?,
            None => self.wall_under(&frame, &centres)? * 1.02,
        };
        let profiles = centres
            .iter()
            .map(|&center| Profile::Slot {
                center,
                length,
                width,
                angle,
            })
            .collect();
        let summary = self.remove(
            &label_of(line),
            Removal {
                frame,
                profiles,
                depth: Depth::Blind(depth),
                taper: 0.0,
                side: "side",
                end: "end",
            },
        )?;
        Ok(format!(
            "{} vent(s) {depth:.3} deep; {summary}",
            centres.len()
        ))
    }

    fn run_inner(&mut self, number: usize, text: &str) -> Result<String> {
        let inner = parse_line(number, text)?;
        self.apply(&inner)
    }

    fn relabel(&mut self, from: &str, to: &str, groups: &[(&str, &str)]) {
        for entry in self.groups.0.iter_mut().filter(|e| e.label == from) {
            entry.label = to.to_string();
            if let Some((_, renamed)) = groups.iter().find(|(old, _)| *old == entry.group) {
                entry.group = renamed.to_string();
            }
        }
        self.groups
            .0
            .retain(|e| !(e.label == to && e.group.starts_with('-')));
        if let Some(tools) = self.tools.remove(from) {
            self.tools.entry(to.to_string()).or_default().extend(tools);
        }
        self.prisms.remove(from);
    }

    pub(crate) fn op_boss(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["d", "h"],
            &["hole", "fit", "depth", "ribs", "rib", "angle"],
            true,
        )?;
        let diameter = positive(args.number("d", &self.scope)?, "d")?;
        let height = positive(args.number("h", &self.scope)?, "h")?;
        self.require_empty_sketch("boss")?;
        let spots: Vec<P2> = args
            .rest
            .iter()
            .map(|text| eval_point(text, &self.scope))
            .collect::<Result<Vec<_>>>()?;
        if spots.is_empty() {
            bail!("`boss` needs at least one x,y position after its diameter and height");
        }
        let ribs = args.optional_number("ribs", &self.scope)?.unwrap_or(0.0);
        if ribs < 0.0 || ribs.fract() != 0.0 {
            bail!("ribs must be a whole number");
        }
        let rib = positive(
            args.optional_number("rib", &self.scope)?
                .unwrap_or(diameter / 5.0),
            "rib",
        )?;
        let turn = args.optional_number("angle", &self.scope)?.unwrap_or(0.0);
        let frame = self.sketch_frame();
        let label = label_of(line);
        let number = line.number;
        let saved = self.frame;
        let result = (|| -> Result<()> {
            for &(x, y) in &spots {
                self.sketch.push(Profile::Circle {
                    center: (x, y),
                    diameter,
                });
            }
            self.run_inner(number, &format!("{label}__boss: extrude {height}"))?;
            self.relabel(&format!("{label}__boss"), &label, &[("start", "-start")]);
            for &(x, y) in &spots {
                for k in 0..ribs as usize {
                    let a = (turn + 360.0 * k as f64 / ribs).to_radians();
                    let radial = frame.x * a.cos() + frame.y * a.sin();
                    let centre = frame.at(x, y);
                    let upright = Frame {
                        origin: centre,
                        x: radial,
                        y: frame.normal,
                        normal: radial.cross(frame.normal),
                    };
                    let r = diameter / 2.0;
                    let sink = self
                        .wall_under(&frame, &[(x + 1.5 * r * a.cos(), y + 1.5 * r * a.sin())])
                        .map_or(0.0, |wall| wall * 0.25);
                    self.frame = Some(upright);
                    self.sketch.push(Profile::Polygon {
                        points: vec![
                            (r * 0.8, -sink),
                            (r * 2.0, -sink),
                            (r * 2.0, 0.0),
                            (r * 0.8, height * 0.75),
                        ],
                    });
                    self.run_inner(number, &format!("{label}__rib: extrude {rib} both"))?;
                    self.relabel(
                        &format!("{label}__rib"),
                        &label,
                        &[("side", "ribs"), ("start", "ribs"), ("end", "ribs")],
                    );
                }
            }
            if let Some(hole) = args.values.get("hole") {
                self.frame = Some(frame.offset(height));
                let depth = match args.optional_number("depth", &self.scope)? {
                    Some(d) => d,
                    None => {
                        let insert = args.values.get("fit") == Some(&"insert");
                        match super::fastener::is_metric(hole) {
                            true if insert => super::fastener::named(hole)?
                                .insert
                                .map_or(height, |(_, long, _)| (long + 1.0).min(height)),
                            _ => height,
                        }
                    }
                };
                let fit = args
                    .values
                    .get("fit")
                    .map(|f| format!(" fit={f}"))
                    .unwrap_or_default();
                let points: Vec<String> = spots.iter().map(|(x, y)| format!("{x},{y}")).collect();
                self.run_inner(
                    number,
                    &format!(
                        "{label}__hole: hole {hole} {} depth={depth}{fit}",
                        points.join(" ")
                    ),
                )?;
                self.relabel(
                    &format!("{label}__hole"),
                    &label,
                    &[("side", "hole"), ("bottom", "hole_bottom")],
                );
            }
            Ok(())
        })();
        self.frame = saved;
        self.sketch.clear();
        result?;
        Ok(format!(
            "{} boss(es) {diameter} across, {height} tall; {}",
            spots.len(),
            self.describe_solid()?
        ))
    }

    pub(crate) fn op_snap(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["l", "t", "w"], &["hook", "angle", "dir"], true)?;
        let length = positive(args.number("l", &self.scope)?, "l")?;
        let thick = positive(args.number("t", &self.scope)?, "t")?;
        let width = positive(args.number("w", &self.scope)?, "w")?;
        let hook = positive(
            args.optional_number("hook", &self.scope)?.unwrap_or(thick),
            "hook",
        )?;
        let lead = args.optional_number("angle", &self.scope)?.unwrap_or(30.0);
        if !(5.0..85.0).contains(&lead) {
            bail!("the lead-in angle must be between 5 and 85 degrees, got {lead}");
        }
        let rise = hook / lead.to_radians().tan();
        if rise >= length {
            bail!("the hook's lead-in is {rise:.3} long, longer than the {length} arm");
        }
        self.require_empty_sketch("snap")?;
        let spots: Vec<P2> = args
            .rest
            .iter()
            .map(|text| eval_point(text, &self.scope))
            .collect::<Result<Vec<_>>>()?;
        if spots.is_empty() {
            bail!("`snap` needs at least one x,y position after its length, thickness and width");
        }
        let dir = args
            .optional_number("dir", &self.scope)?
            .unwrap_or(0.0)
            .to_radians();
        let frame = self.sketch_frame();
        let label = label_of(line);
        let saved = self.frame;
        let result = (|| -> Result<()> {
            for &(x, y) in &spots {
                let sink = self
                    .wall_under(&frame, &[(x, y)])
                    .map_or(0.0, |wall| (wall * 0.5).min(thick));
                let out = frame.x * dir.cos() + frame.y * dir.sin();
                self.frame = Some(Frame {
                    origin: frame.at(x, y),
                    x: out,
                    y: frame.normal,
                    normal: out.cross(frame.normal),
                });
                self.sketch.push(Profile::Polygon {
                    points: vec![
                        (-thick, -sink),
                        (0.0, -sink),
                        (0.0, length - rise),
                        (hook, length - rise),
                        (0.0, length),
                        (-thick, length),
                    ],
                });
                self.run_inner(line.number, &format!("{label}__snap: extrude {width} both"))?;
                self.relabel(
                    &format!("{label}__snap"),
                    &label,
                    &[("side", "faces"), ("start", "faces"), ("end", "faces")],
                );
            }
            Ok(())
        })();
        self.frame = saved;
        self.sketch.clear();
        result?;
        Ok(format!(
            "{} snap hook(s), {hook} catch; {}",
            spots.len(),
            self.describe_solid()?
        ))
    }
}
