use super::args::{Args, label_of, positive};
use super::{Combine, Model};
use crate::geometry::{Frame, Profile, Segment, compose_2d, place_2d};
use crate::parse::Line;
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

type P2 = (f64, f64);

#[derive(Clone, Debug)]
pub struct Plate {
    pub frame: Frame,
    pub outline: Vec<P2>,
    pub to_flat: [f64; 6],
    pub used: Vec<bool>,
}

#[derive(Clone, Debug)]
pub struct Bend {
    pub angle: f64,
    pub width: f64,
    pub line: (P2, P2),
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub thickness: f64,
    pub radius: f64,
    pub k: f64,
    pub plates: Vec<Plate>,
    pub flat: Vec<P2>,
    pub bends: Vec<Bend>,
    pub cutouts: Vec<Profile>,
    pub missed: usize,
    pub unfolded: bool,
}

fn sub(a: P2, b: P2) -> P2 {
    (a.0 - b.0, a.1 - b.1)
}

fn add(a: P2, b: P2) -> P2 {
    (a.0 + b.0, a.1 + b.1)
}

fn mul(a: P2, k: f64) -> P2 {
    (a.0 * k, a.1 * k)
}

fn length(a: P2) -> f64 {
    a.0.hypot(a.1)
}

fn cross(a: P2, b: P2) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

fn area(points: &[P2]) -> f64 {
    (0..points.len())
        .map(|i| cross(points[i], points[(i + 1) % points.len()]))
        .sum::<f64>()
        / 2.0
}

fn linear(matrix: [f64; 6], v: P2) -> P2 {
    let [a, b, c, d, ..] = matrix;
    (a * v.0 + b * v.1, c * v.0 + d * v.1)
}

fn inside(points: &[P2], p: P2) -> bool {
    let mut odd = false;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            odd = !odd;
        }
    }
    odd
}

impl Sheet {
    pub fn allowance(&self, degrees: f64) -> f64 {
        degrees.abs().to_radians() * (self.radius + self.k * self.thickness)
    }

    pub fn flat_area(&self) -> f64 {
        area(&self.flat).abs()
    }

    fn splice(&mut self, from: P2, to: P2, out: P2, reach: f64, tolerance: f64) -> Result<()> {
        let n = self.flat.len();
        let at = (0..n)
            .find(|&i| {
                let (a, b) = (self.flat[i], self.flat[(i + 1) % n]);
                let d = sub(b, a);
                let len = length(d);
                let along = |p: P2| {
                    let rel = sub(p, a);
                    (cross(d, rel).abs() / len < tolerance).then(|| (rel.0 * d.0 + rel.1 * d.1) / len)
                };
                matches!((along(from), along(to)), (Some(s0), Some(s1)) if s0 > -tolerance && s1 < len + tolerance && s1 > s0)
            })
            .ok_or_else(|| anyhow!("the edge is not on the flat outline"))?;
        let mut fresh = self.flat[..=at].to_vec();
        let corners = [
            from,
            add(from, mul(out, reach)),
            add(to, mul(out, reach)),
            to,
        ];
        for p in corners
            .into_iter()
            .chain(self.flat[at + 1..].iter().copied())
        {
            if length(sub(p, *fresh.last().expect("non-empty"))) > tolerance {
                fresh.push(p);
            }
        }
        if length(sub(*fresh.last().expect("non-empty"), fresh[0])) < tolerance {
            fresh.pop();
        }
        self.flat = fresh;
        Ok(())
    }
}

fn welded_wire(points: &[Point3]) -> Wire {
    let vertices: Vec<Vertex> = points.iter().map(|p| Vertex::new(*p)).collect();
    (0..points.len())
        .map(|i| {
            let j = (i + 1) % points.len();
            builder::line(&vertices[i], &vertices[j])
        })
        .collect()
}

fn plate_solid(plate: &Plate, thickness: f64) -> Result<Solid> {
    let top: Vec<Point3> = plate
        .outline
        .iter()
        .map(|&(u, v)| plate.frame.at(u, v))
        .collect();
    let face: Face = profile::attach_plane_normalized(vec![welded_wire(&top)])
        .map_err(|e| anyhow!("sheet face: {e}"))?;
    Ok(super::solids::oriented(builder::extrude(
        &face,
        -plate.frame.normal * thickness,
    )))
}

impl Model {
    pub(crate) fn op_sheet(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["t"], &["r", "k"], false)?;
        let thickness = positive(args.number("t", &self.scope)?, "t")?;
        let radius = args.optional_number("r", &self.scope)?.unwrap_or(thickness);
        if radius < 0.0 {
            bail!("the bend radius cannot be negative");
        }
        let k = args.optional_number("k", &self.scope)?.unwrap_or(0.44);
        if !(0.0..=0.5).contains(&k) {
            bail!("the K-factor is between 0 and 0.5, got {k}");
        }
        if self.sheet.as_ref().is_some_and(|s| !s.plates.is_empty()) {
            bail!("this part already has a sheet; set `sheet` before its `tab`");
        }
        self.sheet = Some(Sheet {
            thickness,
            radius,
            k,
            ..Default::default()
        });
        Ok(format!(
            "sheet {thickness} thick, bends {radius} inside radius, K {k}; a 90 degree bend takes {:.3}",
            self.sheet.as_ref().map_or(0.0, |s| s.allowance(90.0))
        ))
    }

    fn sheet_mut(&mut self, op: &str) -> Result<&mut Sheet> {
        self.sheet
            .as_mut()
            .ok_or_else(|| anyhow!("`{op}` needs `sheet t` first to set the thickness"))
    }

    pub(crate) fn op_tab(&mut self, line: &Line) -> Result<String> {
        Args::new(line, &[], &[], false)?;
        let sheet = self.sheet_mut("tab")?;
        if !sheet.plates.is_empty() {
            bail!("a sheet has one `tab`; add to it with `flange`");
        }
        let thickness = sheet.thickness;
        if self.solid.is_some() {
            bail!("`tab` starts a sheet metal part, so it must come before any other solid");
        }
        let (frame, profiles) = self.take_sketch("tab")?;
        let [profile] = profiles.as_slice() else {
            bail!("`tab` takes one outline, drawn with rect or poly");
        };
        let (start, segments) = profile
            .as_path()
            .filter(|(_, segments)| segments.iter().all(|s| matches!(s, Segment::Line(_))))
            .ok_or_else(|| anyhow!("`tab` takes a rect without rounded corners or a poly"))?;
        let mut outline: Vec<P2> = std::iter::once(start)
            .chain(segments.iter().map(Segment::end))
            .collect();
        if length(sub(outline[0], *outline.last().expect("non-empty"))) < 1.0e-9 {
            outline.pop();
        }
        if area(&outline) < 0.0 {
            outline.reverse();
        }
        let top = frame.offset(thickness);
        self.sketch = vec![Profile::Polygon {
            points: outline.clone(),
        }];
        let label = label_of(line);
        self.frame = Some(frame);
        let made = self.apply(&crate::parse::parse_line(
            line.number,
            &format!("{label}: extrude {thickness}"),
        )?);
        self.frame = Some(frame);
        made?;
        let sheet = self.sheet_mut("tab")?;
        sheet.flat = outline.clone();
        sheet.plates.push(Plate {
            frame: top,
            used: vec![false; outline.len()],
            outline,
            to_flat: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        });
        self.describe_solid()
    }

    fn sheet_edge(&self, a: Point3, b: Point3) -> Result<(usize, usize, bool)> {
        let sheet = self.sheet.as_ref().ok_or_else(|| anyhow!("no sheet"))?;
        let tolerance = self.tolerance() * 100.0;
        for (p, plate) in sheet.plates.iter().enumerate() {
            let height = |q: Point3| (q - plate.frame.origin).dot(plate.frame.normal);
            let top = height(a).abs() < tolerance && height(b).abs() < tolerance;
            let bottom = (height(a) + sheet.thickness).abs() < tolerance
                && (height(b) + sheet.thickness).abs() < tolerance;
            if !top && !bottom {
                continue;
            }
            let (la, lb) = (plate.frame.local(a), plate.frame.local(b));
            let n = plate.outline.len();
            for i in 0..n {
                let (oa, ob) = (plate.outline[i], plate.outline[(i + 1) % n]);
                let same = |x: P2, y: P2| length(sub(x, y)) < tolerance;
                if (same(la, oa) && same(lb, ob)) || (same(la, ob) && same(lb, oa)) {
                    return Ok((p, i, top));
                }
            }
        }
        bail!(
            "a flange goes on a straight outside edge of the sheet's top or bottom face, and this edge is not one"
        )
    }

    pub(crate) fn op_flange(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["edges", "l"], &["angle"], false)?;
        let selector = args.text("edges")?;
        let reach = positive(args.number("l", &self.scope)?, "l")?;
        let angle = args.optional_number("angle", &self.scope)?.unwrap_or(90.0);
        if !(0.0..180.0).contains(&angle) || angle == 0.0 {
            bail!("a flange bends between 0 and 180 degrees, got {angle}");
        }
        let sheet = self.sheet_mut("flange")?.clone();
        if sheet.plates.is_empty() {
            bail!("`flange` needs a `tab` to bend from");
        }
        if sheet.unfolded {
            bail!("the sheet is unfolded; flange it before `unfold`");
        }
        let solid = self.active("flange")?.clone();
        let edges = select::select_edges(selector, &solid, &self.groups, self.tolerance())?;
        if edges.is_empty() {
            bail!("`{selector}` matched no edges");
        }
        let ends: Vec<(Point3, Point3)> = edges
            .iter()
            .map(|edge| (edge.front().point(), edge.back().point()))
            .collect();
        let label = label_of(line);
        let mut said = Vec::new();
        for (a, b) in ends {
            let (p, i, top) = self.sheet_edge(a, b)?;
            said.push(self.flange_one(&label, p, i, top, reach, angle)?);
        }
        Ok(format!("{}; {}", said.join("; "), self.describe_solid()?))
    }

    fn flange_one(
        &mut self,
        label: &str,
        p: usize,
        i: usize,
        top: bool,
        reach: f64,
        angle: f64,
    ) -> Result<String> {
        let tolerance = self.tolerance() * 100.0;
        let sheet = self.sheet.clone().ok_or_else(|| anyhow!("no sheet"))?;
        let plate = sheet.plates[p].clone();
        if plate.used[i] {
            bail!(
                "that edge already has a flange, or both sides of it are selected; pick the edge on the face the flange should rise from, like `walls.face&walls.end`"
            );
        }
        let (t, r) = (sheet.thickness, sheet.radius);
        let n = plate.outline.len();
        let (a, b) = (plate.outline[i], plate.outline[(i + 1) % n]);
        let width = length(sub(b, a));
        let e = mul(sub(b, a), 1.0 / width);
        let o = (e.1, -e.0);
        let f = &plate.frame;
        let lift = |q: P2| f.at(q.0, q.1);
        let (e3, o3, n3) = (f.x * e.0 + f.y * e.1, f.x * o.0 + f.y * o.1, f.normal);
        let side = if top { 1.0 } else { -1.0 };
        let axis_dir = o3.cross(n3 * side);
        let centre = lift(a) + n3 * if top { r } else { -(t + r) };
        let turn = Matrix4::from_translation(centre.to_vec())
            * Matrix4::from_axis_angle(axis_dir, Deg(angle))
            * Matrix4::from_translation(-centre.to_vec());
        let rotate = |q: Point3| turn.transform_point(q);
        let rotate_v = |v: Vector3| turn.transform_vector(v);
        let section = [lift(a), lift(b), lift(b) - n3 * t, lift(a) - n3 * t];
        let mut face: Face = profile::attach_plane_normalized(vec![welded_wire(&section)])
            .map_err(|e| anyhow!("sheet face: {e}"))?;
        if let Surface::Plane(plane) = face.oriented_surface()
            && plane.normal().dot(o3) < 0.0
        {
            face.invert();
        }
        let division = ((angle / 90.0).ceil() as usize).max(1);
        let bend = super::solids::oriented(builder::revolve(
            &face,
            centre,
            axis_dir,
            builder::SweepAngle::Partial(Rad(angle.to_radians())),
            division,
        ));
        let normal = rotate_v(n3);
        let flange = Plate {
            frame: Frame {
                origin: rotate(lift(b)),
                x: -e3,
                y: rotate_v(o3),
                normal,
            },
            outline: vec![(0.0, 0.0), (width, 0.0), (width, reach), (0.0, reach)],
            to_flat: [0.0; 6],
            used: vec![true, false, false, false],
        };
        let flange_solid = plate_solid(&flange, t)?;
        let tool = super::weld::weld_union(&bend, &flange_solid)
            .ok_or_else(|| anyhow!("could not join the bend to its flange"))?;
        let existing = self.active("flange")?.clone();
        match super::weld::weld_union(&existing, &tool) {
            Some(joined) => {
                self.solid = Some(super::fuse::fuse_coplanar(&joined));
                self.tools
                    .entry(label.to_string())
                    .or_default()
                    .push((Combine::Add, tool.clone()));
            }
            None => self.merge(label, tool.clone(), Combine::Add)?,
        }
        let facing = normal * side;
        let free = flange.frame.y;
        let groups: Vec<(&str, Surface)> = select::faces(&tool)
            .iter()
            .map(|face| {
                let surface = face.oriented_surface();
                let group = match &surface {
                    Surface::Plane(plane) => {
                        let n = plane.normal();
                        if n.dot(facing) > 1.0 - 1.0e-9 {
                            "face"
                        } else if n.dot(facing) < -1.0 + 1.0e-9 {
                            "back"
                        } else if n.dot(free) > 1.0 - 1.0e-9 {
                            "end"
                        } else {
                            "side"
                        }
                    }
                    _ => "bend",
                };
                (group, surface)
            })
            .collect();
        self.record(label, groups);
        let allowance = sheet.allowance(angle);
        let local: [f64; 6] = {
            let origin = add(b, mul(o, allowance));
            [-e.0, o.0, -e.1, o.1, origin.0, origin.1]
        };
        let to_flat = compose_2d(plate.to_flat, local);
        let (from, to) = (place_2d(plate.to_flat, a), place_2d(plate.to_flat, b));
        let out = linear(plate.to_flat, o);
        let sheet = self.sheet_mut("flange")?;
        sheet.splice(from, to, out, allowance + reach, tolerance)?;
        sheet.bends.push(Bend {
            angle: angle * side,
            width,
            line: (
                add(from, mul(out, allowance / 2.0)),
                add(to, mul(out, allowance / 2.0)),
            ),
        });
        sheet.plates[p].used[i] = true;
        sheet.plates.push(Plate { to_flat, ..flange });
        Ok(format!(
            "{} {angle} degrees, {reach} long, bend allowance {allowance:.3}",
            if top { "up" } else { "down" }
        ))
    }

    pub(crate) fn sheet_cutouts(&mut self, frame: &Frame, profiles: &[Profile], through: bool) {
        let tolerance = self.tolerance() * 100.0;
        let Some(sheet) = self.sheet.as_mut() else {
            return;
        };
        if sheet.plates.is_empty() || sheet.unfolded {
            return;
        }
        for profile in profiles {
            let samples: Vec<Point3> = profile
                .wires(frame)
                .unwrap_or_default()
                .iter()
                .flat_map(|wire| wire.edge_iter().collect::<Vec<_>>())
                .flat_map(|edge| crate::geometry::curve_samples(&edge.curve()))
                .collect();
            let found: Vec<Profile> = sheet
                .plates
                .iter()
                .filter_map(|plate| {
                    let pf = &plate.frame;
                    let parallel = frame.normal.dot(pf.normal).abs() > 1.0 - 1.0e-9;
                    let height = (frame.origin - pf.origin).dot(pf.normal);
                    let middle = pf.origin - pf.normal * (sheet.thickness / 2.0);
                    let on = if through {
                        (middle - frame.origin).dot(frame.normal) < 0.0
                    } else {
                        height.abs() < tolerance || (height + sheet.thickness).abs() < tolerance
                    };
                    let within = !samples.is_empty()
                        && samples.iter().all(|&q| inside(&plate.outline, pf.local(q)));
                    (parallel && on && within).then(|| {
                        let origin = pf.local(frame.origin);
                        let to_plate = [
                            pf.x.dot(frame.x),
                            pf.x.dot(frame.y),
                            pf.y.dot(frame.x),
                            pf.y.dot(frame.y),
                            origin.0,
                            origin.1,
                        ];
                        Profile::Placed {
                            inner: Box::new(profile.clone()),
                            matrix: compose_2d(plate.to_flat, to_plate),
                        }
                    })
                })
                .collect();
            if found.is_empty() {
                sheet.missed += 1;
            }
            sheet.cutouts.extend(found);
        }
    }

    pub(crate) fn sheet_cut(&mut self) {
        if let Some(sheet) = self.sheet.as_mut()
            && !sheet.plates.is_empty()
            && !sheet.unfolded
        {
            sheet.missed += 1;
        }
    }

    pub(crate) fn op_unfold(&mut self, line: &Line) -> Result<String> {
        Args::new(line, &[], &[], false)?;
        let sheet = self.sheet_mut("unfold")?.clone();
        if sheet.plates.is_empty() {
            bail!("`unfold` needs a `tab`");
        }
        if sheet.unfolded {
            bail!("the sheet is already unfolded");
        }
        let base = &sheet.plates[0];
        let floor = base.frame.offset(-sheet.thickness);
        let mut profiles = vec![Profile::Polygon {
            points: sheet.flat.clone(),
        }];
        profiles.extend(sheet.cutouts.iter().cloned());
        self.solid = None;
        self.sketch = profiles;
        let label = label_of(line);
        let saved = self.frame;
        self.frame = Some(floor);
        let made = self.apply(&crate::parse::parse_line(
            line.number,
            &format!("{label}: extrude {}", sheet.thickness),
        )?);
        self.frame = saved;
        made?;
        let sheet = self.sheet_mut("unfold")?;
        sheet.unfolded = true;
        let missed = sheet.missed;
        let summary = self.describe_solid()?;
        let bounds: BoundingBox<Point3> = sheet_points(self).into_iter().collect();
        let size = bounds.max() - bounds.min();
        let note = match missed {
            0 => String::new(),
            n => format!(
                "; {n} cut(s) or hole(s) made off the flat plates are not in the flat pattern"
            ),
        };
        Ok(format!(
            "flat {:.3} x {:.3}; {summary}{note}",
            size.x.max(size.y).max(size.z),
            {
                let mut s = [size.x, size.y, size.z];
                s.sort_by(f64::total_cmp);
                s[1]
            }
        ))
    }
}

fn sheet_points(model: &Model) -> Vec<Point3> {
    let Some(sheet) = model.sheet.as_ref() else {
        return Vec::new();
    };
    let base = &sheet.plates[0].frame;
    sheet.flat.iter().map(|&(u, v)| base.at(u, v)).collect()
}

pub fn flat_dxf(sheet: &Sheet) -> String {
    let mut out = String::from("0\nSECTION\n2\nENTITIES\n");
    let line = |out: &mut String, layer: &str, a: P2, b: P2| {
        out.push_str(&format!(
            "0\nLINE\n8\n{layer}\n10\n{:.6}\n20\n{:.6}\n30\n0.0\n11\n{:.6}\n21\n{:.6}\n31\n0.0\n",
            a.0, a.1, b.0, b.1
        ));
    };
    let n = sheet.flat.len();
    for i in 0..n {
        line(&mut out, "OUTLINE", sheet.flat[i], sheet.flat[(i + 1) % n]);
    }
    let flat = Frame::named("XY").expect("XY");
    for cutout in &sheet.cutouts {
        for wire in cutout.wires(&flat).unwrap_or_default() {
            for edge in wire.edge_iter() {
                let points = crate::geometry::curve_samples(&edge.curve());
                for pair in points.windows(2) {
                    line(
                        &mut out,
                        "OUTLINE",
                        (pair[0].x, pair[0].y),
                        (pair[1].x, pair[1].y),
                    );
                }
            }
        }
    }
    for bend in &sheet.bends {
        line(&mut out, "BEND", bend.line.0, bend.line.1);
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");
    out
}
