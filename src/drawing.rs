use crate::export::Colour;
use crate::geometry;
use monstertruck::mesh::PolygonMesh;
use monstertruck::modeling::*;
use std::collections::HashMap;
use std::fmt::Write as _;

struct View {
    name: String,
    right: Vector3,
    up: Vector3,
    cut: Option<(Vector3, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    pub axis: usize,
    pub at: f64,
}

impl std::str::FromStr for Section {
    type Err = String;

    fn from_str(text: &str) -> std::result::Result<Section, String> {
        let (axis, at) = text
            .split_once('=')
            .ok_or("write the section plane as x=0, y=0 or z=0")?;
        let axis = match axis {
            "x" | "X" => 0,
            "y" | "Y" => 1,
            "z" | "Z" => 2,
            other => return Err(format!("a section cuts across x, y or z, not `{other}`")),
        };
        let at = at.parse().map_err(|_| format!("`{at}` is not a number"))?;
        Ok(Section { axis, at })
    }
}

fn keep(cut: Option<(Vector3, f64)>, p: Point3) -> f64 {
    cut.map_or(1.0, |(toward, at)| at - p.to_vec().dot(toward))
}

fn clipped(cut: Option<(Vector3, f64)>, line: &[Point3]) -> Vec<Vec<Point3>> {
    let mut pieces = Vec::new();
    let mut current: Vec<Point3> = Vec::new();
    for pair in line.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (da, db) = (keep(cut, a), keep(cut, b));
        let cross =
            |from: Point3, to: Point3, df: f64, dt: f64| from + (to - from) * (df / (df - dt));
        match (da >= 0.0, db >= 0.0) {
            (true, true) => {
                if current.is_empty() {
                    current.push(a);
                }
                current.push(b);
            }
            (true, false) => {
                if current.is_empty() {
                    current.push(a);
                }
                current.push(cross(a, b, da, db));
                pieces.push(std::mem::take(&mut current));
            }
            (false, true) => {
                current.push(cross(a, b, da, db));
                current.push(b);
            }
            (false, false) => {}
        }
    }
    if current.len() > 1 {
        pieces.push(current);
    }
    pieces
}

pub fn section_outlines(solid: &Solid, section: Section) -> Vec<Vec<Point3>> {
    let mut normal = Vector3::zero();
    normal[section.axis] = 1.0;
    section_loops(solid, normal, section.at)
}

fn section_loops(solid: &Solid, normal: Vector3, at: f64) -> Vec<Vec<Point3>> {
    section_of(
        &geometry::mesh(solid, geometry::mesh_tolerance(solid)),
        solid,
        normal,
        at,
    )
}

fn section_of(mesh: &PolygonMesh, solid: &Solid, normal: Vector3, at: f64) -> Vec<Vec<Point3>> {
    let positions = mesh.positions();
    let scale = geometry::bounds(solid).diameter().max(1.0);
    let at = at + scale * 1.0e-9;
    let key = |p: Point3| [p.x, p.y, p.z].map(|c| (c / (scale * 1.0e-9)).round() as i64);
    let ordered = |p: Point3, q: Point3| {
        if (p.x, p.y, p.z) < (q.x, q.y, q.z) {
            (p, q)
        } else {
            (q, p)
        }
    };
    let crossing = |p: Point3, q: Point3| {
        let (p, q) = ordered(p, q);
        let (dp, dq) = (p.to_vec().dot(normal) - at, q.to_vec().dot(normal) - at);
        p + (q - p) * (dp / (dp - dq))
    };
    let mut links: HashMap<[i64; 3], (Point3, Vec<[i64; 3]>)> = HashMap::new();
    for triangle in mesh.faces().triangle_iter() {
        let corners = triangle.map(|v| positions[v.pos]);
        let side = corners.map(|p| p.to_vec().dot(normal) - at > 0.0);
        let points: Vec<Point3> = (0..3)
            .filter(|&i| side[i] != side[(i + 1) % 3])
            .map(|i| crossing(corners[i], corners[(i + 1) % 3]))
            .collect();
        if let [p, q] = points.as_slice() {
            let (kp, kq) = (key(*p), key(*q));
            if kp == kq {
                continue;
            }
            links.entry(kp).or_insert((*p, Vec::new())).1.push(kq);
            links.entry(kq).or_insert((*q, Vec::new())).1.push(kp);
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut loops = Vec::new();
    let starts: Vec<[i64; 3]> = links.keys().copied().collect();
    for start in starts {
        if seen.contains(&start) {
            continue;
        }
        let mut chain = vec![links[&start].0];
        seen.insert(start);
        let mut here = start;
        while let Some(next) = links[&here].1.iter().find(|k| !seen.contains(*k)).copied() {
            seen.insert(next);
            chain.push(links[&next].0);
            here = next;
        }
        if links[&here].1.contains(&start) {
            chain.push(chain[0]);
        }
        if chain.len() > 2 {
            loops.push(chain);
        }
    }
    loops
}

struct Callout {
    centers: Vec<Point3>,
    radius: f64,
    note: Option<String>,
}

fn hole_callouts(
    holes: &[crate::model::mate::Cylinder],
    toward: Vector3,
    notes: &[crate::model::HoleNote],
) -> Vec<Callout> {
    let mut found: Vec<Callout> = Vec::new();
    for cylinder in holes {
        if cylinder.axis.cross(toward).magnitude() > 1.0e-6 {
            continue;
        }
        let rounded = (cylinder.radius * 200.0).round() / 200.0;
        let note = notes
            .iter()
            .find(|n| {
                (n.diameter / 2.0 - cylinder.radius).abs() < 3.0e-3
                    && n.centers
                        .iter()
                        .any(|p| (p - cylinder.point).cross(toward).magnitude() < 1.0e-3)
            })
            .map(|n| n.note.clone());
        match found
            .iter_mut()
            .find(|c| (c.radius - rounded).abs() < 1.0e-9 && c.note == note)
        {
            Some(known) => known.centers.push(cylinder.point),
            None => found.push(Callout {
                centers: vec![cylinder.point],
                radius: rounded,
                note,
            }),
        }
    }
    found
}

impl View {
    fn toward(&self) -> Vector3 {
        self.right.cross(self.up)
    }

    fn project(&self, p: Point3) -> (f64, f64) {
        (p.to_vec().dot(self.right), p.to_vec().dot(self.up))
    }
}

fn polylines(solid: &Solid) -> Vec<Vec<Point3>> {
    solid
        .boundaries()
        .iter()
        .flat_map(|shell| shell.edge_iter().collect::<Vec<_>>())
        .fold(Vec::<Edge>::new(), |mut unique, edge| {
            if !unique.iter().any(|e| e.is_same(&edge)) {
                unique.push(edge);
            }
            unique
        })
        .iter()
        .map(|edge| geometry::curve_samples(&edge.curve()))
        .collect()
}

fn silhouettes(mesh: &PolygonMesh, solid: &Solid, toward: Vector3) -> Vec<Vec<Point3>> {
    let positions = mesh.positions();
    let scale = geometry::bounds(solid).diameter().max(1.0) * 1.0e-9;
    let key = |p: Point3| [p.x, p.y, p.z].map(|c| (c / scale).round() as i64);
    type Side = (Point3, Point3, Vec<f64>);
    let mut sides: HashMap<([i64; 3], [i64; 3]), Side> = HashMap::new();
    for triangle in mesh.faces().triangle_iter() {
        let [a, b, c] = triangle.map(|v| positions[v.pos]);
        let normal = (b - a).cross(c - a);
        if normal.magnitude() < 1.0e-14 {
            continue;
        }
        let facing = normal.normalize().dot(toward);
        for (p, q) in [(a, b), (b, c), (c, a)] {
            let (kp, kq) = (key(p), key(q));
            let id = if kp < kq { (kp, kq) } else { (kq, kp) };
            sides.entry(id).or_insert((p, q, Vec::new())).2.push(facing);
        }
    }
    sides
        .into_values()
        .filter(|(_, _, facing)| {
            facing.len() == 2
                && facing[0] * facing[1] < 0.0
                && facing.iter().all(|f| f.abs() > 1.0e-6)
        })
        .map(|(p, q, _)| vec![p, q])
        .collect()
}

pub fn drawing(parts: &[(&Solid, Option<Colour>)]) -> String {
    drawing_with(parts, None)
}

pub struct PartsList {
    pub rows: Vec<Vec<String>>,
    pub balloons: Vec<(usize, usize)>,
}

const PARTS_HEADER: [&str; 5] = ["item", "qty", "part", "material", "mass g"];

pub fn drawing_with(parts: &[(&Solid, Option<Colour>)], section: Option<Section>) -> String {
    annotated(parts, section, None, &[])
}

pub fn drawing_noted(
    parts: &[(&Solid, Option<Colour>)],
    section: Option<Section>,
    notes: &[crate::model::HoleNote],
) -> String {
    annotated(parts, section, None, notes)
}

fn view(name: &str, right: Vector3, up: Vector3) -> View {
    View {
        name: name.to_string(),
        right,
        up,
        cut: None,
    }
}

fn number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn annotated(
    parts: &[(&Solid, Option<Colour>)],
    section: Option<Section>,
    list: Option<&PartsList>,
    notes: &[crate::model::HoleNote],
) -> String {
    let mut views = vec![
        view("top", Vector3::unit_x(), Vector3::unit_y()),
        view("front", Vector3::unit_x(), Vector3::unit_z()),
        view("right", Vector3::unit_y(), Vector3::unit_z()),
        view(
            "iso",
            Vector3::new(1.0, 1.0, 0.0).normalize(),
            Vector3::new(-1.0, 1.0, 2.0).normalize(),
        ),
    ];
    if let Some(Section { axis, at }) = section {
        let like = match axis {
            0 => 2,
            1 => 1,
            _ => 0,
        };
        let (right, up) = (views[like].right, views[like].up);
        let toward = right.cross(up);
        let name = format!("section {}={}", ["x", "y", "z"][axis], number(at));
        views.push(View {
            name,
            right,
            up,
            cut: Some((toward, at * toward[axis])),
        });
    }
    let bounds: BoundingBox<Point3> = parts
        .iter()
        .flat_map(|(solid, _)| {
            let b = geometry::bounds(solid);
            [b.min(), b.max()]
        })
        .collect();
    let size = bounds.max() - bounds.min();
    let gap = size.x.max(size.y).max(size.z).max(1.0) * 0.3;
    let extent = |view: &View| {
        let corners: Vec<(f64, f64)> = (0..8)
            .map(|i| {
                let pick = |bit: usize, lo: f64, hi: f64| if i & bit == 0 { lo } else { hi };
                view.project(Point3::new(
                    pick(1, bounds.min().x, bounds.max().x),
                    pick(2, bounds.min().y, bounds.max().y),
                    pick(4, bounds.min().z, bounds.max().z),
                ))
            })
            .collect();
        let fold = |f: fn(f64, f64) -> f64, start: f64, pick: fn(&(f64, f64)) -> f64| {
            corners.iter().map(pick).fold(start, f)
        };
        (
            fold(f64::min, f64::INFINITY, |c| c.0),
            fold(f64::max, f64::NEG_INFINITY, |c| c.0),
            fold(f64::min, f64::INFINITY, |c| c.1),
            fold(f64::max, f64::NEG_INFINITY, |c| c.1),
        )
    };
    let extents: Vec<(f64, f64, f64, f64)> = views.iter().map(extent).collect();
    let shapes: Vec<(PolygonMesh, Vec<Vec<Point3>>)> = parts
        .iter()
        .map(|(solid, _)| {
            (
                geometry::mesh(solid, geometry::mesh_tolerance(solid)),
                polylines(solid),
            )
        })
        .collect();
    let holes: Vec<Vec<crate::model::mate::Cylinder>> = std::thread::scope(|scope| {
        let jobs: Vec<_> = parts
            .iter()
            .map(|(solid, _)| scope.spawn(move || crate::model::mate::holes_in(solid)))
            .collect();
        jobs.into_iter()
            .map(|job| job.join().unwrap_or_default())
            .collect()
    });
    let width = |i: usize| extents[i].1 - extents[i].0;
    let height = |i: usize| extents[i].3 - extents[i].2;
    let cells = [(0, 0), (0, 1), (1, 1), (1, 0), (2, 1)];
    let columns = if views.len() > 4 { 3 } else { 2 };
    let column: Vec<f64> = (0..columns)
        .map(|c| {
            (0..views.len())
                .filter(|&i| cells[i].0 == c)
                .map(width)
                .fold(0.0, f64::max)
        })
        .collect();
    let row: Vec<f64> = (0..2)
        .map(|r| {
            (0..views.len())
                .filter(|&i| cells[i].1 == r)
                .map(height)
                .fold(0.0, f64::max)
        })
        .collect();
    let total_w = gap * (columns as f64 + 1.0) + column.iter().sum::<f64>();
    let stroke = gap * 0.008;
    let text = gap * 0.12;
    let line_height = text * 1.7;
    let table_h = list.map_or(0.0, |l| (l.rows.len() + 1) as f64 * line_height + gap * 0.5);
    let total_h = gap * 4.0 + row[0] + row[1] + table_h;
    let mut svg = String::new();
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{total_w:.3}mm\" height=\"{total_h:.3}mm\" viewBox=\"0 0 {total_w:.3} {total_h:.3}\">"
    );
    let arrow = gap * 0.06;
    let hatch = gap * 0.1;
    let _ = writeln!(
        svg,
        "<defs><marker id=\"arrow\" viewBox=\"0 0 10 10\" refX=\"10\" refY=\"5\" markerUnits=\"userSpaceOnUse\" markerWidth=\"{arrow:.3}\" markerHeight=\"{arrow:.3}\" orient=\"auto-start-reverse\"><path d=\"M0,1 L10,5 L0,9 z\"/></marker>\
         <pattern id=\"hatch\" patternUnits=\"userSpaceOnUse\" width=\"{hatch:.3}\" height=\"{hatch:.3}\" patternTransform=\"rotate(45)\"><line x1=\"0\" y1=\"0\" x2=\"0\" y2=\"{hatch:.3}\" stroke=\"black\" stroke-width=\"{:.4}\"/></pattern></defs>",
        stroke
    );
    let _ = writeln!(svg, "<rect width=\"100%\" height=\"100%\" fill=\"white\"/>");
    let label = |svg: &mut String, x: f64, y: f64, anchor: &str, rotate: bool, words: &str| {
        let turn = if rotate {
            format!(" transform=\"rotate(-90 {x:.3} {y:.3})\"")
        } else {
            String::new()
        };
        let _ = writeln!(
            svg,
            "<text x=\"{x:.3}\" y=\"{y:.3}\" font-family=\"sans-serif\" font-size=\"{text:.3}\" text-anchor=\"{anchor}\"{turn}>{words}</text>"
        );
    };
    let dimension = |svg: &mut String,
                     from: (f64, f64),
                     to: (f64, f64),
                     offset: (f64, f64),
                     value: f64| {
        let (a, b) = (
            (from.0 + offset.0, from.1 + offset.1),
            (to.0 + offset.0, to.1 + offset.1),
        );
        let length = offset.0.hypot(offset.1).max(1.0e-12);
        let over = (
            offset.0 / length * gap * 0.05,
            offset.1 / length * gap * 0.05,
        );
        let _ = writeln!(
            svg,
            "<g class=\"dimension\" stroke=\"black\" stroke-width=\"{:.4}\" fill=\"none\"><path d=\"M{:.3},{:.3} L{:.3},{:.3} M{:.3},{:.3} L{:.3},{:.3}\"/><path d=\"M{:.3},{:.3} L{:.3},{:.3}\" marker-start=\"url(#arrow)\" marker-end=\"url(#arrow)\"/></g>",
            stroke * 0.6,
            from.0 + over.0,
            from.1 + over.1,
            a.0 + over.0,
            a.1 + over.1,
            to.0 + over.0,
            to.1 + over.1,
            b.0 + over.0,
            b.1 + over.1,
            a.0,
            a.1,
            b.0,
            b.1
        );
        let middle = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let vertical = (a.0 - b.0).abs() < 1.0e-9;
        let (x, y) = if vertical {
            (middle.0 - text * 0.4, middle.1)
        } else {
            (middle.0, middle.1 - text * 0.4)
        };
        label(svg, x, y, "middle", vertical, &number(value));
    };
    for (i, view) in views.iter().enumerate() {
        let (col, rowi) = cells[i];
        let left = gap + (0..col).map(|c| column[c] + gap).sum::<f64>();
        let top = gap + if rowi == 1 { row[0] + gap } else { 0.0 };
        let (x0, x1, y0, y1) = extents[i];
        let place = |p: Point3| {
            let (u, v) = view.project(p);
            (left + (u - x0), top + (y1 - v))
        };
        let _ = writeln!(
            svg,
            "<g id=\"{}\" fill=\"none\" stroke=\"black\" stroke-width=\"{stroke:.4}\" stroke-linecap=\"round\">",
            view.name.replace(' ', "-").replace('=', "")
        );
        for ((solid, _), (mesh, edges)) in parts.iter().zip(&shapes) {
            if let Some((toward, at)) = view.cut {
                for outline in section_of(mesh, solid, toward, at) {
                    let d: String = outline
                        .iter()
                        .enumerate()
                        .map(|(k, p)| {
                            let (x, y) = place(*p);
                            format!("{}{x:.3},{y:.3} ", if k == 0 { "M" } else { "L" })
                        })
                        .collect();
                    let _ = writeln!(
                        svg,
                        "<path class=\"cut\" fill=\"url(#hatch)\" fill-rule=\"evenodd\" d=\"{}Z\"/>",
                        d
                    );
                }
            }
            let lines = edges
                .iter()
                .cloned()
                .chain(silhouettes(mesh, solid, view.toward()))
                .flat_map(|line| clipped(view.cut, &line));
            for line in lines {
                let mut d = String::new();
                for (k, p) in line.iter().enumerate() {
                    let (x, y) = place(*p);
                    let _ = write!(d, "{}{x:.3},{y:.3} ", if k == 0 { "M" } else { "L" });
                }
                let _ = writeln!(svg, "<path d=\"{}\"/>", d.trim_end());
            }
        }
        svg.push_str("</g>\n");
        label(&mut svg, left, top - gap * 0.55, "start", false, &view.name);
        let (bottom, right) = (top + (y1 - y0), left + (x1 - x0));
        match view.name.as_str() {
            "top" => {
                dimension(
                    &mut svg,
                    (left, top),
                    (right, top),
                    (0.0, -gap * 0.3),
                    x1 - x0,
                );
                dimension(
                    &mut svg,
                    (left, bottom),
                    (left, top),
                    (-gap * 0.3, 0.0),
                    y1 - y0,
                );
            }
            "front" => {
                dimension(
                    &mut svg,
                    (left, bottom),
                    (left, top),
                    (-gap * 0.3, 0.0),
                    y1 - y0,
                );
            }
            _ => {}
        }
        if let (Some(list), "iso") = (list, view.name.as_str()) {
            let middle = place(Point3::from_vec(
                (bounds.min().to_vec() + bounds.max().to_vec()) / 2.0,
            ));
            for &(item, part) in &list.balloons {
                let Some((solid, _)) = parts.get(part) else {
                    continue;
                };
                let b = geometry::bounds(solid);
                let at = place(Point3::from_vec(
                    (b.min().to_vec() + b.max().to_vec()) / 2.0,
                ));
                let away = (at.0 - middle.0, at.1 - middle.1);
                let length = away.0.hypot(away.1).max(1.0e-9);
                let push = gap * 0.9;
                let centre = (
                    at.0 + away.0 / length * push,
                    at.1 + away.1 / length * push - gap * 0.3,
                );
                let radius = text * 0.9;
                let toward = (at.0 - centre.0, at.1 - centre.1);
                let reach = toward.0.hypot(toward.1).max(1.0e-9);
                let rim = (
                    centre.0 + toward.0 / reach * radius,
                    centre.1 + toward.1 / reach * radius,
                );
                let _ = writeln!(
                    svg,
                    "<g class=\"balloon\" stroke=\"black\" stroke-width=\"{:.4}\" fill=\"white\"><path fill=\"none\" d=\"M{:.3},{:.3} L{:.3},{:.3}\"/><circle cx=\"{:.3}\" cy=\"{:.3}\" r=\"{radius:.3}\"/></g>",
                    stroke * 0.6,
                    rim.0,
                    rim.1,
                    at.0,
                    at.1,
                    centre.0,
                    centre.1
                );
                let _ = writeln!(
                    svg,
                    "<circle cx=\"{:.3}\" cy=\"{:.3}\" r=\"{:.3}\"/>",
                    at.0,
                    at.1,
                    stroke * 1.5
                );
                label(
                    &mut svg,
                    centre.0,
                    centre.1 + text * 0.35,
                    "middle",
                    false,
                    &item.to_string(),
                );
            }
        }
        if ["top", "front", "right"].contains(&view.name.as_str()) {
            let mut taken: Vec<(f64, f64)> = Vec::new();
            for found in &holes {
                for callout in hole_callouts(found, view.toward(), notes) {
                    let lean = std::f64::consts::FRAC_1_SQRT_2;
                    let spot = |center: Point3| {
                        let (cx, cy) = place(center);
                        let edge = (cx + callout.radius * lean, cy - callout.radius * lean);
                        (edge, (edge.0 + gap * 0.25, edge.1 - gap * 0.25))
                    };
                    let clear = |end: (f64, f64)| {
                        taken.iter().all(|t| {
                            (t.0 - end.0).abs() > gap * 0.8 || (t.1 - end.1).abs() > text * 1.4
                        })
                    };
                    let (edge, end) = callout
                        .centers
                        .iter()
                        .map(|c| spot(*c))
                        .find(|(_, end)| clear(*end))
                        .unwrap_or_else(|| {
                            let (edge, mut end) = spot(callout.centers[0]);
                            while !clear(end) {
                                end.1 += text * 1.4;
                            }
                            (edge, end)
                        });
                    taken.push(end);
                    let _ = writeln!(
                        svg,
                        "<path class=\"callout\" stroke=\"black\" stroke-width=\"{:.4}\" fill=\"none\" marker-start=\"url(#arrow)\" d=\"M{:.3},{:.3} L{:.3},{:.3} L{:.3},{:.3}\"/>",
                        stroke * 0.6,
                        edge.0,
                        edge.1,
                        end.0,
                        end.1,
                        end.0 + gap * 0.1,
                        end.1
                    );
                    let count = if callout.centers.len() > 1 {
                        format!("{}× ", callout.centers.len())
                    } else {
                        String::new()
                    };
                    label(
                        &mut svg,
                        end.0 + gap * 0.12,
                        end.1 + text * 0.35,
                        "start",
                        false,
                        &format!(
                            "{count}⌀{}{}",
                            number(callout.radius * 2.0),
                            callout
                                .note
                                .as_ref()
                                .map(|note| format!(" {note}"))
                                .unwrap_or_default()
                        ),
                    );
                }
            }
        }
    }
    if let Some(list) = list {
        let char_w = text * 0.62;
        let widths: Vec<f64> = (0..PARTS_HEADER.len())
            .map(|k| {
                list.rows
                    .iter()
                    .map(|r| r.get(k).map_or(0, |c| c.chars().count()))
                    .chain([PARTS_HEADER[k].len()])
                    .max()
                    .unwrap_or(0) as f64
                    * char_w
                    + text
            })
            .collect();
        let table_w: f64 = widths.iter().sum();
        let left = total_w - gap - table_w;
        let top = total_h - gap * 0.9 - table_h + gap * 0.5;
        let rows = std::iter::once(PARTS_HEADER.map(String::from).to_vec())
            .chain(list.rows.iter().cloned());
        for (r, cells) in rows.enumerate() {
            let y = top + r as f64 * line_height;
            let _ = writeln!(
                svg,
                "<path stroke=\"black\" stroke-width=\"{:.4}\" d=\"M{left:.3},{y:.3} L{:.3},{y:.3}\"/>",
                stroke * 0.6,
                left + table_w
            );
            let mut x = left;
            for (k, cell) in cells.iter().enumerate() {
                label(
                    &mut svg,
                    x + text * 0.4,
                    y + line_height * 0.7,
                    "start",
                    false,
                    cell,
                );
                x += widths[k];
            }
        }
        let bottom = top + (list.rows.len() + 1) as f64 * line_height;
        let _ = writeln!(
            svg,
            "<path class=\"parts-list\" stroke=\"black\" stroke-width=\"{:.4}\" fill=\"none\" d=\"M{left:.3},{top:.3} L{:.3},{top:.3} L{:.3},{bottom:.3} L{left:.3},{bottom:.3} Z\"/>",
            stroke * 0.6,
            left + table_w,
            left + table_w
        );
    }
    label(
        &mut svg,
        gap,
        total_h - gap * 0.4,
        "start",
        false,
        &format!(
            "{} x {} x {} mm",
            number(size.x),
            number(size.y),
            number(size.z)
        ),
    );
    svg.push_str("</svg>\n");
    svg
}
