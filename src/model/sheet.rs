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
    pub used: Vec<Vec<(f64, f64)>>,
}

#[derive(Clone, Debug)]
pub struct Bend {
    pub angle: f64,
    pub width: f64,
    pub line: (P2, P2),
}

#[derive(Clone, Debug)]
pub struct Fold {
    pub label: String,
    pub point: P2,
    pub along: P2,
    pub out: P2,
    pub at: f64,
    pub angle: f64,
}

#[derive(Clone, Debug)]
pub struct Dart {
    pub label: String,
    pub fold: usize,
    pub at: Vec<f64>,
    pub leg: f64,
    pub width: f64,
}

#[derive(Clone, Debug)]
pub struct PlacedZone {
    pub fold: usize,
    pub width: (f64, f64),
    pub motion: Matrix4,
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
    pub curved: bool,
    pub blank: Option<Solid>,
    pub folds: Vec<Fold>,
    pub flat_groups: Vec<select::GroupEntry>,
    pub derived: usize,
    pub darts: Vec<Dart>,
    pub zones: Vec<PlacedZone>,
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
        let straight = match profiles.as_slice() {
            [profile] => profile
                .as_path()
                .filter(|(_, segments)| segments.iter().all(|s| matches!(s, Segment::Line(_)))),
            _ => None,
        };
        let Some((start, segments)) = straight else {
            return self.curved_tab(line, frame, profiles, thickness);
        };
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
            used: vec![Vec::new(); outline.len()],
            outline,
            to_flat: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        });
        self.describe_solid()
    }

    fn curved_tab(
        &mut self,
        line: &Line,
        frame: Frame,
        profiles: Vec<Profile>,
        thickness: f64,
    ) -> Result<String> {
        self.sketch = profiles;
        let label = label_of(line);
        self.frame = Some(frame);
        let made = self.apply(&crate::parse::parse_line(
            line.number,
            &format!("{label}: extrude {thickness}"),
        )?);
        self.frame = Some(frame);
        made?;
        let sheet = self.sheet_mut("tab")?;
        sheet.curved = true;
        sheet.plates.push(Plate {
            frame: frame.offset(thickness),
            outline: Vec::new(),
            to_flat: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            used: Vec::new(),
        });
        self.describe_solid()
    }

    pub(crate) fn op_bend(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["from", "to"], &["angle"], false)?;
        let from = crate::parse::eval_point(args.text("from")?, &self.scope)?;
        let to = crate::parse::eval_point(args.text("to")?, &self.scope)?;
        let angle = args.optional_number("angle", &self.scope)?.unwrap_or(90.0);
        if angle == 0.0 || angle.abs() > 180.0 {
            bail!("a bend turns more than 0 and up to 180 degrees either way, got {angle}");
        }
        let tolerance = self.tolerance() * 100.0;
        let sheet = self.sheet_mut("bend")?;
        if sheet.plates.is_empty() {
            bail!("`bend` folds the flat plate a `tab` makes; draw the blank and `tab` it first");
        }
        if sheet.unfolded {
            bail!("the sheet is unfolded; bend it before `unfold`");
        }
        if !sheet.bends.is_empty() && sheet.folds.is_empty() {
            bail!(
                "this sheet already has flanges; a part is either flanged or bent from a flat blank, not both"
            );
        }
        if angle.abs() == 180.0 && sheet.radius == 0.0 {
            bail!("a 180 degree bend needs a bend radius above 0 to leave room between its sides");
        }
        let run = sub(to, from);
        let len = length(run);
        if len < tolerance {
            bail!("the two points of a bend line are the same point");
        }
        let along = mul(run, 1.0 / len);
        let mut out = (-along.1, along.0);
        let mut at = out.0 * from.0 + out.1 * from.1;
        if at.abs() < tolerance {
            bail!(
                "the bend line passes through the sketch origin; the side with the origin stays put, so draw the blank with its origin on the part that does not move"
            );
        }
        if at < 0.0 {
            out = mul(out, -1.0);
            at = -at;
        }
        if sheet.blank.is_none() {
            let (solid, groups) = (self.solid.clone(), self.groups.0.clone());
            let sheet = self.sheet_mut("bend")?;
            sheet.derived = groups.len();
            sheet.blank = solid;
            sheet.flat_groups = groups;
        }
        let sheet = self.sheet_mut("bend")?;
        sheet.folds.push(Fold {
            label: label_of(line),
            point: from,
            along,
            out,
            at,
            angle,
        });
        if let Err(error) = self.refold() {
            if let Some(sheet) = self.sheet.as_mut() {
                sheet.folds.pop();
            }
            return Err(error);
        }
        let sheet = self.sheet.as_ref().ok_or_else(|| anyhow!("no sheet"))?;
        Ok(format!(
            "{} {:.6} degrees, bend allowance {:.3}; {}",
            if angle > 0.0 { "up" } else { "down" },
            angle.abs(),
            sheet.allowance(angle),
            self.describe_solid()?
        ))
    }

    fn refold(&mut self) -> Result<()> {
        let tolerance = self.tolerance() * 100.0;
        let sheet = self.sheet.clone().ok_or_else(|| anyhow!("no sheet"))?;
        let blank = sheet
            .blank
            .clone()
            .ok_or_else(|| anyhow!("no flat blank"))?;
        let (t, r) = (sheet.thickness, sheet.radius);
        let top = sheet.plates[0].frame;
        let n3 = top.normal;
        let lift3 = |q: P2| top.at(q.0, q.1);
        let in3 = |v: P2| top.x * v.0 + top.y * v.1;
        struct Piece {
            solid: Solid,
            crossed: Vec<usize>,
        }
        let span = |solid: &Solid, fold: &Fold| {
            let mut range = (f64::INFINITY, f64::NEG_INFINITY);
            for face in select::faces(solid) {
                for edge in face.edge_iter() {
                    for point in crate::geometry::curve_samples(&edge.curve()) {
                        let (u, v) = top.local(point);
                        let s = u * fold.out.0 + v * fold.out.1;
                        range = (range.0.min(s), range.1.max(s));
                    }
                }
            }
            range
        };
        let reach = geometry_reach(&blank) + 1.0;
        let slab = |fold: &Fold, lo: f64, hi: f64| -> Result<Solid> {
            let (lo, hi) = (lo.max(-reach), hi.min(reach));
            let x = in3(fold.out);
            let frame = Frame {
                origin: top.offset(-t - 1.0).origin,
                x,
                y: n3.cross(x),
                normal: n3,
            };
            let block = Profile::Rect {
                center: ((lo + hi) / 2.0, 0.0),
                width: hi - lo,
                height: 2.0 * reach,
                radius: 0.0,
            };
            let shapes = super::solids::loops(&frame, std::slice::from_ref(&block))?;
            super::solids::prism(
                &frame,
                &shapes,
                &[0],
                std::slice::from_ref(&block),
                n3 * (t + 2.0),
                (0.0, 0.0),
            )
        };
        let remove = |solid: &Solid, fold: &Fold, from: f64, to: f64| -> Result<Solid> {
            let tool = super::solids::clear_flush(&slab(fold, from, to)?, solid);
            monstertruck::solid::difference_normalized(solid, &tool)
                .map(|cut| super::fuse::fuse_coplanar(&super::exact::exact_edges(&cut)))
                .map_err(|e| anyhow!("could not split the blank at `{}`: {e}", fold.label))
        };
        let across = |solid: &Solid, fold: &Fold, at: f64| {
            let mut width = (f64::INFINITY, f64::NEG_INFINITY);
            for face in select::faces(solid) {
                for edge in face.edge_iter() {
                    for point in crate::geometry::curve_samples(&edge.curve()) {
                        let (u, v) = top.local(point);
                        if (u * fold.out.0 + v * fold.out.1 - at).abs() < tolerance {
                            let a = u * fold.along.0 + v * fold.along.1;
                            width = (width.0.min(a), width.1.max(a));
                        }
                    }
                }
            }
            width
        };
        struct Zone {
            fold: usize,
            width: (f64, f64),
            crossed: Vec<usize>,
        }
        let mut pieces = vec![Piece {
            solid: blank.clone(),
            crossed: Vec::new(),
        }];
        let mut zones: Vec<Zone> = Vec::new();
        for (i, fold) in sheet.folds.iter().enumerate() {
            let half = sheet.allowance(fold.angle) / 2.0;
            let (near, far) = (fold.at - half, fold.at + half);
            for zone in zones.iter_mut() {
                let other = &sheet.folds[zone.fold];
                let oh = sheet.allowance(other.angle) / 2.0;
                let corners = [
                    (other.at - oh, zone.width.0),
                    (other.at - oh, zone.width.1),
                    (other.at + oh, zone.width.0),
                    (other.at + oh, zone.width.1),
                ];
                let s: Vec<f64> = corners
                    .iter()
                    .map(|&(o, a)| {
                        let p = add(mul(other.out, o), mul(other.along, a));
                        p.0 * fold.out.0 + p.1 * fold.out.1
                    })
                    .collect();
                if s.iter().all(|&x| x >= far - tolerance) {
                    zone.crossed.push(i);
                } else if !s.iter().all(|&x| x <= near + tolerance) {
                    bail!(
                        "`{}` and `{}` cross on the plate; bends must not meet inside the blank",
                        other.label,
                        fold.label
                    );
                }
            }
            let mut next = Vec::new();
            for mut piece in pieces {
                let (lo, hi) = span(&piece.solid, fold);
                if hi <= near + tolerance {
                    next.push(piece);
                    continue;
                }
                if lo >= far - tolerance {
                    piece.crossed.push(i);
                    next.push(piece);
                    continue;
                }
                if lo > near - tolerance || hi < far + tolerance {
                    bail!(
                        "the blank must run right across the strip that bends at `{}`, and it stops inside it",
                        fold.label
                    );
                }
                let before = remove(&piece.solid, fold, near, f64::INFINITY)?;
                let after = remove(&piece.solid, fold, f64::NEG_INFINITY, far)?;
                let width = across(&before, fold, near);
                let width_after = across(&after, fold, far);
                let ba = 2.0 * half;
                let strip = (width.1 - width.0) * ba * t;
                let left = crate::geometry::volume(&piece.solid).abs()
                    - crate::geometry::volume(&before).abs()
                    - crate::geometry::volume(&after).abs();
                if width.1 <= width.0
                    || (width.0 - width_after.0).abs() > tolerance
                    || (width.1 - width_after.1).abs() > tolerance
                    || (left - strip).abs() > 1.0e-2 * strip.max(1.0)
                {
                    bail!(
                        "the blank must cross a bend on straight edges square to the bend line, with no holes in the {ba:.3} wide strip that bends; at `{}` it does not",
                        fold.label
                    );
                }
                zones.push(Zone {
                    fold: i,
                    width,
                    crossed: piece.crossed.clone(),
                });
                next.push(Piece {
                    solid: before,
                    crossed: piece.crossed.clone(),
                });
                let mut crossed = piece.crossed.clone();
                crossed.push(i);
                next.push(Piece {
                    solid: after,
                    crossed,
                });
            }
            pieces = next;
        }
        let turn = |fold: &Fold| {
            let side = fold.angle.signum();
            let half = sheet.allowance(fold.angle) / 2.0;
            let o3 = in3(fold.out);
            let axis = o3.cross(n3 * side);
            let foot = lift3(mul(fold.out, fold.at - half));
            let centre = foot + n3 * if side > 0.0 { r } else { -(t + r) };
            (centre, axis, o3)
        };
        let carry = |fold: &Fold| {
            let (centre, axis, o3) = turn(fold);
            Matrix4::from_translation(centre.to_vec())
                * Matrix4::from_axis_angle(axis, Deg(fold.angle.abs()))
                * Matrix4::from_translation(-centre.to_vec())
                * Matrix4::from_translation(-o3 * sheet.allowance(fold.angle))
        };
        let motion = |crossed: &[usize]| {
            let mut order = crossed.to_vec();
            order.sort_by(|&a, &b| sheet.folds[a].at.total_cmp(&sheet.folds[b].at));
            order
                .iter()
                .fold(Matrix4::identity(), |m, &i| m * carry(&sheet.folds[i]))
        };
        let mut placed: Vec<Solid> = Vec::new();
        let mut moves: Vec<(Solid, Matrix4)> = Vec::new();
        let mut bends = Vec::new();
        let mut bend_faces: Vec<(String, Vec<Surface>)> = Vec::new();
        for piece in &pieces {
            let m = motion(&piece.crossed);
            placed.push(super::solids::placed(&piece.solid, m));
            moves.push((piece.solid.clone(), m));
        }
        let mut placed_zones = Vec::new();
        for zone in &zones {
            let fold = &sheet.folds[zone.fold];
            let m = motion(&zone.crossed);
            placed_zones.push(PlacedZone {
                fold: zone.fold,
                width: zone.width,
                motion: m,
            });
            let width = zone.width;
            let swept = bend_prism(&sheet, fold, top, width)?;
            let swept = super::solids::placed(&swept, m);
            bend_faces.push((
                fold.label.clone(),
                select::faces(&swept)
                    .iter()
                    .map(|f| f.oriented_surface())
                    .filter(|s| !matches!(s, Surface::Plane(_)))
                    .collect(),
            ));
            placed.push(swept);
            let point = |a: f64| add(mul(fold.out, fold.at), mul(fold.along, a));
            bends.push(Bend {
                angle: fold.angle,
                width: width.1 - width.0,
                line: (point(width.0), point(width.1)),
            });
        }
        let mut rest = placed;
        let mut solid = rest.remove(0);
        while !rest.is_empty() {
            let reach = crate::geometry::bounds(&solid);
            let touching = |other: &Solid| {
                let b = crate::geometry::bounds(other);
                (0..3).all(|k| {
                    b.min()[k] <= reach.max()[k] + tolerance
                        && reach.min()[k] <= b.max()[k] + tolerance
                })
            };
            let joined = (0..rest.len())
                .filter(|&k| touching(&rest[k]))
                .find_map(|k| {
                    super::weld::weld_union(&solid, &rest[k])
                        .or_else(|| monstertruck::solid::or_normalized(&solid, &rest[k]).ok())
                        .map(|joined| (k, joined))
                });
            let Some((k, joined)) = joined else {
                bail!("could not join the bent plate back together");
            };
            rest.remove(k);
            solid = joined;
        }
        self.solid = Some(super::fuse::fuse_coplanar(&solid));
        let mut groups: Vec<select::GroupEntry> = Vec::new();
        for entry in &sheet.flat_groups {
            for (piece, m) in &moves {
                let carries = select::faces(piece)
                    .iter()
                    .any(|face| select::face_on(face, &entry.surface, tolerance / 100.0));
                if carries {
                    groups.push(select::GroupEntry {
                        surface: super::bodies::moved_surface(&entry.surface, *m),
                        ..entry.clone()
                    });
                }
            }
        }
        for (label, surfaces) in bend_faces {
            for surface in surfaces {
                groups.push(select::GroupEntry {
                    label: label.clone(),
                    group: "bend".to_string(),
                    surface,
                });
            }
        }
        let tail = self.groups.0[sheet.derived.min(self.groups.0.len())..].to_vec();
        let derived = groups.len();
        groups.extend(tail);
        self.groups.0 = groups;
        let sheet = self.sheet.as_mut().ok_or_else(|| anyhow!("no sheet"))?;
        sheet.derived = derived;
        sheet.bends = bends;
        sheet.zones = placed_zones;
        let darts = sheet.darts.clone();
        for dart in &darts {
            self.press_dart(dart)?;
        }
        Ok(())
    }

    pub(crate) fn op_dart(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["bend"], &["l", "w"], true)?;
        let name = args.text("bend")?;
        let sheet = self.sheet_mut("dart")?.clone();
        let fold = sheet
            .folds
            .iter()
            .position(|f| f.label == name)
            .ok_or_else(|| {
                anyhow!(
                    "`{name}` is not a bend of this sheet; name the `bend` line to press darts into"
                )
            })?;
        if sheet.unfolded {
            bail!("the sheet is unfolded; press darts before `unfold`");
        }
        let (t, r) = (sheet.thickness, sheet.radius);
        let leg = match args.optional_number("l", &self.scope)? {
            Some(l) => positive(l, "l")?,
            None => 3.0 * (t + r),
        };
        let width = match args.optional_number("w", &self.scope)? {
            Some(w) => positive(w, "w")?,
            None => 2.0 * t + 4.0,
        };
        if width <= 2.0 * t {
            bail!(
                "a dart is wider than twice the sheet, {:.3}, to leave a groove between its walls",
                2.0 * t
            );
        }
        if sheet.folds[fold].angle.abs() > 120.0 {
            bail!("darts stiffen bends up to 120 degrees");
        }
        let at: Vec<f64> = if args.rest.is_empty() {
            vec![0.0]
        } else {
            args.rest
                .iter()
                .map(|text| crate::parse::eval(text, &self.scope))
                .collect::<Result<_>>()?
        };
        for &a in &at {
            let inside = sheet.zones.iter().any(|z| {
                z.fold == fold
                    && a - width / 2.0 >= z.width.0 - 1.0e-9
                    && a + width / 2.0 <= z.width.1 + 1.0e-9
            });
            if !inside {
                bail!("a {width:.3} wide dart at {a} runs off the `{name}` bend");
            }
        }
        let label = label_of(line);
        let sheet = self.sheet_mut("dart")?;
        sheet.darts.push(Dart {
            label,
            fold,
            at: at.clone(),
            leg,
            width,
        });
        if let Err(error) = self.refold() {
            if let Some(sheet) = self.sheet.as_mut() {
                sheet.darts.pop();
            }
            self.refold()?;
            return Err(error);
        }
        Ok(format!(
            "{} dart(s) {leg:.3} along each side, {width:.3} wide; {}",
            at.len(),
            self.describe_solid()?
        ))
    }

    fn press_dart(&mut self, dart: &Dart) -> Result<()> {
        let sheet = self.sheet.clone().ok_or_else(|| anyhow!("no sheet"))?;
        let fold = &sheet.folds[dart.fold];
        let (t, r) = (sheet.thickness, sheet.radius);
        let top = sheet.plates[0].frame;
        let side = fold.angle.signum();
        let theta = fold.angle.abs().to_radians();
        let phi = side * theta;
        let rotate = |v: P2| {
            (
                v.0 * phi.cos() - v.1 * phi.sin(),
                v.0 * phi.sin() + v.1 * phi.cos(),
            )
        };
        let s0 = fold.at - sheet.allowance(fold.angle) / 2.0;
        let h_in = if side > 0.0 { 0.0 } else { -t };
        let centre = (s0, h_in + side * r);
        let corner = (s0 + r * (theta / 2.0).tan(), h_in);
        let bisector = {
            let d = sub(corner, centre);
            mul(d, 1.0 / length(d))
        };
        let far = rotate((1.0, 0.0));
        let a = sub(corner, (dart.leg, 0.0));
        let b = add(corner, mul(far, dart.leg));
        let back = {
            let d = sub(a, b);
            mul(d, 1.0 / length(d))
        };
        let inward = (0.0, -side);
        let far_inward = rotate(inward);
        let sink_a = (t / 2.0) / (back.0 * inward.0 + back.1 * inward.1);
        let sink_b = (t / 2.0) / (-(back.0 * far_inward.0 + back.1 * far_inward.1));
        let reach = t * 1.5;
        let rib_at = |depth: f64| {
            [
                add(a, mul(back, sink_a)),
                sub(b, mul(back, sink_b)),
                add(centre, mul(bisector, r + depth * t)),
            ]
        };
        let groove_at = |deeper: f64| {
            let shift = mul(bisector, (t - deeper) / (theta / 2.0).cos());
            let (ga, gb) = (add(a, shift), add(b, shift));
            [
                add(ga, mul(back, reach)),
                sub(gb, mul(back, reach)),
                add(centre, mul(bisector, r + 1.5 * t - deeper)),
            ]
        };
        let groove = groove_at(0.0);
        let n3 = top.normal;
        let point = |p: P2, at: f64| {
            let flat = add(mul(fold.out, p.0), mul(fold.along, at));
            top.at(flat.0, flat.1) + n3 * p.1
        };
        let tent = |shape: &[P2; 3], at: f64, half: f64| -> Result<Solid> {
            tetra([
                point(shape[0], at),
                point(shape[1], at),
                point(shape[2], at - half),
                point(shape[2], at + half),
            ])
        };
        for &at in &dart.at {
            let zone = sheet
                .zones
                .iter()
                .find(|z| z.fold == dart.fold && at >= z.width.0 && at <= z.width.1)
                .ok_or_else(|| anyhow!("the dart at {at} is off its bend"))?;
            let half = dart.width / 2.0;
            let clearance = t * 1.0e-2;
            let groove = super::solids::placed(&tent(&groove, at, half)?, zone.motion);
            let shell = monstertruck::solid::difference_normalized(
                &tent(&rib_at(0.5), at, half)?,
                &tent(&groove_at(clearance), at, half - clearance)?,
            )
            .map_err(|e| anyhow!("could not shape the dart at {at}: {e}"))?;
            let shell = super::solids::placed(&shell, zone.motion);
            self.merge(&dart.label, groove, Combine::Remove)
                .map_err(|e| anyhow!("could not press the dart at {at}: {e}"))?;
            self.merge(&dart.label, shell, Combine::Add)
                .map_err(|e| anyhow!("could not press the dart at {at}: {e}"))?;
        }
        Ok(())
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
        let args = Args::new(line, &["edges", "l"], &["angle", "inset"], false)?;
        let selector = args.text("edges")?;
        let reach = positive(args.number("l", &self.scope)?, "l")?;
        let angle = args.optional_number("angle", &self.scope)?.unwrap_or(90.0);
        if !(0.0..=180.0).contains(&angle) || angle == 0.0 {
            bail!("a flange bends more than 0 and up to 180 degrees, got {angle}");
        }
        let inset = match args.values.get("inset") {
            None => (0.0, 0.0),
            Some(text) if text.contains(',') => crate::parse::eval_point(text, &self.scope)?,
            Some(_) => {
                let both = args.number("inset", &self.scope)?;
                (both, both)
            }
        };
        if inset.0 < 0.0 || inset.1 < 0.0 {
            bail!("inset cannot be negative");
        }
        let sheet = self.sheet_mut("flange")?.clone();
        if sheet.plates.is_empty() {
            bail!("`flange` needs a `tab` to bend from");
        }
        if sheet.curved || !sheet.folds.is_empty() {
            bail!(
                "`flange` works on a tab drawn with rect or poly and not yet bent; fold this blank with `bend`"
            );
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
            said.push(self.flange_one(&label, (p, i, top), reach, angle, inset)?);
        }
        Ok(format!("{}; {}", said.join("; "), self.describe_solid()?))
    }

    fn flange_one(
        &mut self,
        label: &str,
        (p, i, top): (usize, usize, bool),
        reach: f64,
        angle: f64,
        inset: (f64, f64),
    ) -> Result<String> {
        let tolerance = self.tolerance() * 100.0;
        let sheet = self.sheet.clone().ok_or_else(|| anyhow!("no sheet"))?;
        let plate = sheet.plates[p].clone();
        let (t, r) = (sheet.thickness, sheet.radius);
        if angle == 180.0 && r == 0.0 {
            bail!("a 180 degree hem needs a bend radius above 0 to leave room between its sides");
        }
        let n = plate.outline.len();
        let (a, b) = (plate.outline[i], plate.outline[(i + 1) % n]);
        let full = length(sub(b, a));
        let e = mul(sub(b, a), 1.0 / full);
        let span = (inset.0, full - inset.1);
        if span.1 - span.0 <= tolerance {
            bail!("the insets leave nothing of the {full:.3} edge to flange");
        }
        if plate.used[i]
            .iter()
            .any(|&(lo, hi)| span.0 < hi - tolerance && lo < span.1 - tolerance)
        {
            bail!(
                "that edge already has a flange there, or both sides of it are selected; pick the edge on the face the flange should rise from, like `walls.face&walls.end`"
            );
        }
        let (a, b) = (add(a, mul(e, span.0)), add(a, mul(e, span.1)));
        let width = span.1 - span.0;
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
            used: vec![vec![(0.0, width)], Vec::new(), Vec::new(), Vec::new()],
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
        sheet.plates[p].used[i].push(span);
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
        if !sheet.folds.is_empty() {
            sheet.missed += profiles.len();
            return;
        }
        if sheet.curved {
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
            && (!sheet.folds.is_empty() || !sheet.curved)
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
        if sheet.curved || sheet.blank.is_some() {
            return self.unfold_blank(&sheet);
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

impl Model {
    fn unfold_blank(&mut self, sheet: &Sheet) -> Result<String> {
        if let Some(blank) = sheet.blank.clone() {
            self.solid = Some(blank);
            let tail = self.groups.0[sheet.derived.min(self.groups.0.len())..].to_vec();
            self.groups.0 = sheet.flat_groups.clone();
            self.groups.0.extend(tail);
        }
        let solid = self.active("unfold")?.clone();
        let bounds = crate::geometry::bounds(&solid);
        let mut size = [
            bounds.max().x - bounds.min().x,
            bounds.max().y - bounds.min().y,
            bounds.max().z - bounds.min().z,
        ];
        size.sort_by(f64::total_cmp);
        let missed = sheet.missed;
        let sheet = self.sheet_mut("unfold")?;
        sheet.unfolded = true;
        let note = match missed {
            0 => String::new(),
            n => format!("; {n} cut(s) or hole(s) made after bending are not in the flat pattern"),
        };
        Ok(format!(
            "flat {:.3} x {:.3}; {}{note}",
            size[2],
            size[1],
            self.describe_solid()?
        ))
    }
}

fn bend_prism(sheet: &Sheet, fold: &Fold, top: Frame, width: (f64, f64)) -> Result<Solid> {
    let (t, r) = (sheet.thickness, sheet.radius);
    let side = fold.angle.signum();
    let phi = side * fold.angle.abs().to_radians();
    let rotate = |v: P2, by: f64| {
        (
            v.0 * by.cos() - v.1 * by.sin(),
            v.0 * by.sin() + v.1 * by.cos(),
        )
    };
    let s0 = fold.at - sheet.allowance(fold.angle) / 2.0;
    let h_in = if side > 0.0 { 0.0 } else { -t };
    let centre = (s0, h_in + side * r);
    let u0 = (0.0, -side);
    let (u1, um) = (rotate(u0, phi), rotate(u0, phi / 2.0));
    let n3 = top.normal;
    let along3 = top.x * fold.along.0 + top.y * fold.along.1;
    let point = |p: P2| {
        let flat = add(mul(fold.out, p.0), mul(fold.along, width.0));
        top.at(flat.0, flat.1) + n3 * p.1
    };
    let at = |radius: f64, u: P2| point(add(centre, mul(u, radius)));
    let outer = r + t;
    let o0 = Vertex::new(at(outer, u0));
    let o1 = Vertex::new(at(outer, u1));
    let mut edges = Vec::new();
    if r > 0.0 {
        let i0 = Vertex::new(at(r, u0));
        let i1 = Vertex::new(at(r, u1));
        edges.push(builder::line(&i0, &o0));
        edges.push(builder::circle_arc(&o0, &o1, at(outer, um)));
        edges.push(builder::line(&o1, &i1));
        edges.push(builder::circle_arc(&i1, &i0, at(r, um)));
    } else {
        let c = Vertex::new(point(centre));
        edges.push(builder::line(&c, &o0));
        edges.push(builder::circle_arc(&o0, &o1, at(outer, um)));
        edges.push(builder::line(&o1, &c));
    }
    let wire: Wire = edges.into_iter().collect();
    let face: Face =
        profile::attach_plane_normalized(vec![wire]).map_err(|e| anyhow!("bend section: {e}"))?;
    Ok(super::solids::oriented(builder::extrude(
        &face,
        along3 * (width.1 - width.0),
    )))
}

fn tetra(points: [Point3; 4]) -> Result<Solid> {
    let vertices: Vec<Vertex> = points.iter().map(|p| Vertex::new(*p)).collect();
    let pairs = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
    let lines: Vec<Edge> = pairs
        .iter()
        .map(|&(i, j)| builder::line(&vertices[i], &vertices[j]))
        .collect();
    let edge = |i: usize, j: usize| {
        let k = pairs
            .iter()
            .position(|&pair| pair == (i.min(j), i.max(j)))
            .expect("tetra edge");
        if i < j {
            lines[k].clone()
        } else {
            lines[k].inverse()
        }
    };
    let middle = Point3::from_vec(points.iter().map(|p| p.to_vec()).sum::<Vector3>() / 4.0);
    let mut faces = Vec::new();
    for [a, b, c] in [[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]] {
        let wire: Wire = vec![edge(a, b), edge(b, c), edge(c, a)].into();
        let mut face: Face =
            profile::attach_plane_normalized(vec![wire]).map_err(|e| anyhow!("dart face: {e}"))?;
        let outward =
            (points[a] + (points[b] - points[a]) / 3.0 + (points[c] - points[a]) / 3.0) - middle;
        if let Surface::Plane(plane) = face.oriented_surface()
            && plane.normal().dot(outward) < 0.0
        {
            face.invert();
        }
        faces.push(face);
    }
    Ok(super::solids::oriented(Solid::new_unchecked(vec![
        faces.into(),
    ])))
}

fn geometry_reach(solid: &Solid) -> f64 {
    crate::geometry::bounds(solid).diameter()
}

fn blank_lines(sheet: &Sheet, solid: &Solid) -> Vec<(P2, P2)> {
    let top = sheet.plates[0].frame;
    let mut lines = Vec::new();
    for face in select::faces(solid) {
        let Surface::Plane(plane) = face.oriented_surface() else {
            continue;
        };
        if plane.normal().dot(top.normal) < 1.0 - 1.0e-9
            || (plane.origin() - top.origin).dot(top.normal).abs() > 1.0e-6
        {
            continue;
        }
        for edge in face.edge_iter() {
            let points: Vec<P2> = crate::geometry::curve_samples(&edge.curve())
                .into_iter()
                .map(|p| top.local(p))
                .collect();
            lines.extend(points.windows(2).map(|pair| (pair[0], pair[1])));
        }
    }
    lines
}

fn sheet_points(model: &Model) -> Vec<Point3> {
    let Some(sheet) = model.sheet.as_ref() else {
        return Vec::new();
    };
    let base = &sheet.plates[0].frame;
    sheet.flat.iter().map(|&(u, v)| base.at(u, v)).collect()
}

pub fn flat_dxf(sheet: &Sheet, solid: Option<&Solid>) -> String {
    let mut out = String::from("0\nSECTION\n2\nENTITIES\n");
    let line = |out: &mut String, layer: &str, a: P2, b: P2| {
        out.push_str(&format!(
            "0\nLINE\n8\n{layer}\n10\n{:.6}\n20\n{:.6}\n30\n0.0\n11\n{:.6}\n21\n{:.6}\n31\n0.0\n",
            a.0, a.1, b.0, b.1
        ));
    };
    let blank = sheet
        .blank
        .as_ref()
        .or(solid.filter(|_| sheet.curved && sheet.folds.is_empty()));
    if let Some(blank) = blank {
        for (a, b) in blank_lines(sheet, blank) {
            line(&mut out, "OUTLINE", a, b);
        }
        for bend in &sheet.bends {
            line(&mut out, "BEND", bend.line.0, bend.line.1);
        }
        out.push_str("0\nENDSEC\n0\nEOF\n");
        return out;
    }
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
