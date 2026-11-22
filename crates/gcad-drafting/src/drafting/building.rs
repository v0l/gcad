use super::plan::{CUT_WEIGHT, HIDDEN_WEIGHT, PROJECTED_WEIGHT, circle, group, stroke, text};
use super::{
    Anchor, BLACK, Frame, Mark, State, TEXT, clip_line, clip_polygon, dimension, flatten, named,
    number, pair, quad, scale_of,
};
use crate::geometry;
use crate::parse::{Line, eval};
use anyhow::{Result, anyhow, bail};
use gcad_mesh::arch::layout::left;
use monstertruck::modeling::*;

type Box4 = (f64, f64, f64, f64);

pub(super) fn clip_marks(marks: Vec<Mark>, area: Box4) -> Vec<Mark> {
    marks
        .into_iter()
        .filter_map(|mark| match mark {
            Mark::Group { class, data, marks } => {
                let inner = clip_marks(marks, area);
                (!inner.is_empty()).then_some(Mark::Group {
                    class,
                    data,
                    marks: inner,
                })
            }
            Mark::Text { x, y, .. } => {
                (x >= area.0 && x <= area.2 && y >= area.1 && y <= area.3).then_some(mark)
            }
            Mark::Shape {
                loops,
                closed,
                fill,
                stroke,
                width,
                dashed,
            } => {
                let loops: Vec<Vec<(f64, f64)>> = if closed {
                    loops
                        .iter()
                        .map(|l| clip_polygon(l, area))
                        .filter(|l| l.len() > 2)
                        .collect()
                } else {
                    loops
                        .iter()
                        .flat_map(|l| clip_line(l, area))
                        .filter(|l| l.len() > 1)
                        .collect()
                };
                (!loops.is_empty()).then_some(Mark::Shape {
                    loops,
                    closed,
                    fill,
                    stroke,
                    width,
                    dashed,
                })
            }
        })
        .collect()
}

fn translate(mark: &Mark, dx: f64, dy: f64) -> Mark {
    match mark {
        Mark::Group { class, data, marks } => Mark::Group {
            class: class.clone(),
            data: data.clone(),
            marks: marks.iter().map(|m| translate(m, dx, dy)).collect(),
        },
        Mark::Text {
            x,
            y,
            size,
            text,
            anchor,
            angle,
        } => Mark::Text {
            x: x + dx,
            y: y + dy,
            size: *size,
            text: text.clone(),
            anchor: *anchor,
            angle: *angle,
        },
        Mark::Shape {
            loops,
            closed,
            fill,
            stroke,
            width,
            dashed,
        } => Mark::Shape {
            loops: loops
                .iter()
                .map(|l| l.iter().map(|p| (p.0 + dx, p.1 + dy)).collect())
                .collect(),
            closed: *closed,
            fill: *fill,
            stroke: *stroke,
            width: *width,
            dashed: *dashed,
        },
    }
}

fn unit(v: (f64, f64)) -> (f64, f64) {
    let l = v.0.hypot(v.1).max(1e-12);
    (v.0 / l, v.1 / l)
}

impl State {
    fn wall_extent(&self) -> Result<Box4> {
        let model = self.model()?;
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for wall in &model.arch.walls {
            let (lo, hi) = wall.outer_span();
            for i in 0..wall.line.segments() {
                let n = left(wall.line.dir(i));
                let (a, b) = wall.line.ends(i);
                for d in [lo, hi] {
                    for p in [a, b] {
                        let q = (p.0 + n.0 * d, p.1 + n.1 * d);
                        x0 = x0.min(q.0);
                        y0 = y0.min(q.1);
                        x1 = x1.max(q.0);
                        y1 = y1.max(q.1);
                    }
                }
            }
        }
        if x0 > x1 {
            bail!("the model has no walls to dimension");
        }
        Ok((x0, y0, x1, y1))
    }

    pub(super) fn op_dim_auto(&mut self, line: &Line) -> Result<String> {
        let kind = line
            .positional
            .get(1)
            .cloned()
            .ok_or_else(|| anyhow!("write `dim auto walls|openings|grid side=-y`"))?;
        let frame = self.frame()?.clone();
        if frame.up_is_z {
            bail!("`dim auto` goes on a plan");
        }
        let (horizontal, far) = match named(line, "side").unwrap_or("-y") {
            "-y" => (true, false),
            "+y" => (true, true),
            "-x" => (false, false),
            "+x" => (false, true),
            other => bail!("a dimension side is -y, +y, -x or +x, not `{other}`"),
        };
        let extent = self.wall_extent()?;
        let (lo, hi, edge) = if horizontal {
            (extent.0, extent.2, if far { extent.3 } else { extent.1 })
        } else {
            (extent.1, extent.3, if far { extent.2 } else { extent.0 })
        };
        let along = |p: (f64, f64)| if horizontal { p.0 } else { p.1 };
        let across = |p: (f64, f64)| if horizontal { p.1 } else { p.0 };
        let mut stops = vec![lo, hi];
        let model = self.model()?;
        match kind.as_str() {
            "walls" => {
                for wall in &model.arch.walls {
                    let (s0, s1) = wall.outer_span();
                    for i in 0..wall.line.segments() {
                        let d = wall.line.dir(i);
                        if along(d).abs() > 1e-6 {
                            continue;
                        }
                        let n = left(d);
                        let (a, _) = wall.line.ends(i);
                        for t in [s0, s1] {
                            stops.push(along((a.0 + n.0 * t, a.1 + n.1 * t)));
                        }
                    }
                }
            }
            "openings" => {
                for opening in &model.arch.openings {
                    let Some(wall) = model.arch.wall(&opening.host) else {
                        continue;
                    };
                    let (i, p, d) = wall.line.locate(opening.at);
                    if across(d).abs() > 1e-6 || (across(p) - edge).abs() > 600.0 {
                        continue;
                    }
                    for s in [-opening.w / 2.0, opening.w / 2.0] {
                        stops.push(along((p.0 + d.0 * s, p.1 + d.1 * s)));
                    }
                    let _ = i;
                }
            }
            "grid" => {
                for grid in model.arch.grids.iter().filter(|g| g.along_x == horizontal) {
                    stops.push(grid.offset);
                }
            }
            other => bail!("`dim auto` takes walls, openings or grid, not `{other}`"),
        }
        stops.retain(|s| *s >= lo - 0.5 && *s <= hi + 0.5);
        stops.sort_by(f64::total_cmp);
        stops.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        let base = match named(line, "offset") {
            Some(t) => eval(t, &self.scope)?,
            None => match kind.as_str() {
                "walls" => 16.0,
                "openings" => 32.0,
                _ => 40.0,
            },
        };
        let paper = |u: f64| {
            if horizontal {
                frame.paper((u, edge))
            } else {
                frame.paper((edge, u))
            }
        };
        let push = |sign: f64| {
            if horizontal {
                (0.0, if far { -sign } else { sign })
            } else {
                (if far { sign } else { -sign }, 0.0)
            }
        };
        let mut inner = Vec::new();
        for (k, pair) in stops.windows(2).enumerate() {
            let (pa, pb) = (paper(pair[0]), paper(pair[1]));
            let length = (pb.0 - pa.0).hypot(pb.1 - pa.1);
            let words = format!("{:.0}", pair[1] - pair[0]);
            dimension(
                &mut inner,
                pa,
                pb,
                (push(base).0, push(base).1),
                &words,
                length < 12.0 && k % 2 == 1,
            );
        }
        if stops.len() > 2 && kind == "walls" {
            dimension(
                &mut inner,
                paper(lo),
                paper(hi),
                (push(base + 8.0).0, push(base + 8.0).1),
                &format!("{:.0}", hi - lo),
                false,
            );
        }
        let count = stops.len().saturating_sub(1);
        self.sheet()?.marks.push(group(
            "dim",
            vec![("kind", kind.clone()), ("layer", "A-DIMS".into())],
            inner,
        ));
        Ok(format!(
            "{kind} chain of {count} from {:.0} to {:.0}",
            lo, hi
        ))
    }

    pub(super) fn op_tag(&mut self, line: &Line) -> Result<String> {
        let kind = line
            .positional
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("write `tag door|window|room|wall|stair|column`"))?;
        let frame = self.frame()?.clone();
        if frame.up_is_z {
            bail!("tags go on a plan");
        }
        let level = self.plan_level;
        let model = self.model()?;
        let mut marks = Vec::new();
        let at_wall = |wall: &crate::model::arch::Wall, s: f64, d: f64| {
            let (_, p, dir) = wall.line.locate(s);
            let n = left(dir);
            frame.paper((p.0 + n.0 * d, p.1 + n.1 * d))
        };
        match kind.as_str() {
            "door" | "window" => {
                for o in &model.arch.openings {
                    let is_door = matches!(o.fill, crate::model::arch::Fill::Door { .. });
                    let is_window = matches!(o.fill, crate::model::arch::Fill::Window { .. });
                    if (kind == "door" && !is_door) || (kind == "window" && !is_window) {
                        continue;
                    }
                    let Some(wall) = model.arch.wall(&o.host) else {
                        continue;
                    };
                    let (lo, _) = wall.outer_span();
                    let (x, y) = at_wall(wall, o.at, lo - 450.0);
                    let words = o.mark.clone().unwrap_or_else(|| o.name.clone());
                    marks.push(group(
                        "tag",
                        vec![("kind", kind.clone()), ("target", o.name.clone())],
                        vec![
                            stroke(vec![circle(x, y, 3.2)], true, HIDDEN_WEIGHT * 1.5, false),
                            text(x, y + 0.9, TEXT * 0.8, &words, Anchor::Middle),
                        ],
                    ));
                }
            }
            "wall" => {
                for wall in &model.arch.walls {
                    let (lo, _) = wall.outer_span();
                    let longest = (0..wall.line.segments())
                        .max_by(|a, b| {
                            let l = |i: usize| {
                                let (p, q) = wall.line.ends(i);
                                (q.0 - p.0).hypot(q.1 - p.1)
                            };
                            l(*a).total_cmp(&l(*b))
                        })
                        .unwrap_or(0);
                    let starts = wall.line.starts();
                    let mid = (starts[longest] + starts[longest + 1]) / 2.0;
                    let (x, y) = at_wall(wall, mid, lo - 450.0);
                    let name = model
                        .arch
                        .types
                        .iter()
                        .find(|(_, layers)| wall.tiers.first().is_some_and(|t| *layers == t.layers))
                        .map_or_else(|| wall.name.clone(), |(n, _)| n.clone());
                    marks.push(group(
                        "tag",
                        vec![("kind", "wall".into()), ("target", wall.name.clone())],
                        vec![text(x, y, TEXT, &name, Anchor::Middle)],
                    ));
                }
            }
            "room" => {
                for room in model
                    .arch
                    .rooms
                    .iter()
                    .filter(|r| (r.level - level).abs() < 1.0)
                {
                    let [x0, y0, x1, y1] = room.rect;
                    let (x, y) = frame.paper(((x0 + x1) / 2.0, (y0 + y1) / 2.0));
                    marks.push(group(
                        "tag",
                        vec![
                            ("kind", "room".into()),
                            ("target", room.name.clone()),
                            ("area", format!("{:.2}", room.area())),
                        ],
                        vec![
                            text(x, y - 1.0, TEXT, &room.title, Anchor::Middle),
                            text(
                                x,
                                y + 2.6,
                                TEXT * 0.85,
                                &format!("{:.2} m²", room.area()),
                                Anchor::Middle,
                            ),
                        ],
                    ));
                }
            }
            "stair" => {
                for stair in &model.arch.stairs {
                    if let Some(f) = stair.flights.first() {
                        let (x, y) =
                            frame.paper(f.plan(stair.steps.tread * 1.5, stair.width + 250.0));
                        marks.push(group(
                            "tag",
                            vec![("kind", "stair".into()), ("target", stair.name.clone())],
                            vec![text(
                                x,
                                y,
                                TEXT,
                                &format!("{} {} R", stair.name, stair.steps.risers),
                                Anchor::Start,
                            )],
                        ));
                    }
                }
            }
            "column" => {
                for column in &model.arch.columns {
                    if let Ok(s) = model.named_body(&column.name) {
                        let b = geometry::bounds(&s);
                        let (x, y) =
                            frame.paper(((b.min().x + b.max().x) / 2.0, b.max().y + 250.0));
                        marks.push(group(
                            "tag",
                            vec![("kind", "column".into()), ("target", column.name.clone())],
                            vec![text(x, y, TEXT, &column.name, Anchor::Middle)],
                        ));
                    }
                }
            }
            other => bail!("`tag` takes door, window, room, wall, stair or column, not `{other}`"),
        }
        let count = marks.len();
        let mut placed: Vec<Mark> = Vec::new();
        let fixed: Vec<super::annotate::Rect> = flatten(&self.sheet()?.marks)
            .into_iter()
            .filter(|m| matches!(m, Mark::Text { .. }))
            .flat_map(super::annotate::mark_rects)
            .collect();
        for mark in marks {
            let mut taken = fixed.clone();
            taken.extend(placed.iter().flat_map(super::annotate::mark_rects));
            let own = super::annotate::mark_rects(&mark);
            let mut best = mark.clone();
            'search: for r in [0.0, 3.0, 6.0, 9.0, 12.0, 16.0, 22.0, 30.0] {
                for (dx, dy) in [
                    (r, 0.0),
                    (-r, 0.0),
                    (0.0, r),
                    (0.0, -r),
                    (r, r),
                    (-r, -r),
                    (r, -r),
                    (-r, r),
                ] {
                    let clear = own.iter().all(|rect| {
                        let moved = (
                            rect.0 + dx - 0.5,
                            rect.1 + dy - 0.5,
                            rect.2 + dx + 0.5,
                            rect.3 + dy + 0.5,
                        );
                        taken.iter().all(|t| !super::annotate::overlaps(moved, *t))
                    });
                    if clear {
                        best = translate(&mark, dx, dy);
                        break 'search;
                    }
                }
            }
            placed.push(best);
        }
        self.sheet()?.marks.extend(placed);
        Ok(format!("{count} {kind} tags"))
    }

    pub(super) fn op_viewport(&mut self, line: &Line) -> Result<String> {
        let mut base = self.last_view.clone().ok_or_else(|| {
            anyhow!("draw a `plan`, `rcp`, `elevation` or `section` to detail first")
        })?;
        let window = named(line, "window").ok_or_else(|| {
            anyhow!("write `viewport window=u0,v0,u1,v1 scale=1:5 label=\"name\"`")
        })?;
        quad(window, &self.scope)?;
        scale_of(named(line, "scale").unwrap_or("1:1"))?;
        base.named
            .retain(|(k, _)| !["window", "scale", "label", "place"].contains(&k.as_str()));
        for key in ["window", "scale", "label", "place"] {
            if let Some(v) = named(line, key) {
                base.named.push((key.to_string(), v.to_string()));
            }
        }
        let keep = self.last_view.clone();
        let result = self.apply_drawing(&base);
        self.last_view = keep;
        result
    }

    fn apply_drawing(&mut self, line: &Line) -> Result<String> {
        match line.op.as_str() {
            "plan" => self.op_plan(line),
            "rcp" => self.op_rcp(line),
            "elevation" => self.op_elevation(line),
            "section" => self.op_building_section(line),
            other => bail!("cannot detail a `{other}`"),
        }
    }

    pub(super) fn is_building_section(line: &Line) -> bool {
        line.positional.len() == 3
            && line.positional[1].contains(',')
            && line.positional[2].contains(',')
    }

    pub(super) fn op_building_section(&mut self, line: &Line) -> Result<String> {
        let name = line.positional[0].to_uppercase();
        let a = pair(&line.positional[1], &self.scope)?;
        let b = pair(&line.positional[2], &self.scope)?;
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        if length < 1.0 {
            bail!("a section line needs two different points");
        }
        let (dx, dy) = unit((b.0 - a.0, b.1 - a.1));
        let normal = Vector3::new(-dy, dx, 0.0);
        let at = normal.x * a.0 + normal.y * a.1;
        let project =
            move |p: monstertruck::modeling::Point3| ((p.x - a.0) * dx + (p.y - a.1) * dy, p.z);
        let model = self.model()?;
        let top = model
            .arch
            .levels
            .iter()
            .map(|l| l.z)
            .fold(0.0_f64, f64::max);
        let bottom = model
            .arch
            .levels
            .iter()
            .map(|l| l.z)
            .fold(0.0_f64, f64::min);
        let window = match named(line, "window") {
            Some(t) => {
                let (p, q, r, s) = quad(t, &self.scope)?;
                (p.min(r), q.min(s), p.max(r), q.max(s))
            }
            None => (-800.0, bottom - 600.0, length + 800.0, top + 800.0),
        };
        let scale = match named(line, "scale") {
            Some(s) => scale_of(s)?,
            None => self.sheet()?.scale,
        };
        let (w, h) = ((window.2 - window.0) / scale, (window.3 - window.1) / scale);
        let place = named(line, "place")
            .map(|t| pair(t, &self.scope))
            .transpose()?;
        let (x, y) = self.place(w, h, place)?;
        let frame = Frame {
            right: Vector3::new(dx, dy, 0.0),
            up: Vector3::unit_z(),
            scale,
            origin: (x, y),
            window,
            up_is_z: true,
        };
        let cut = self.cut_polygons(normal, at, &project, true)?;
        let mut marks = self.poche_marks(&frame, &cut);
        let model = self.model()?;
        let mut names = Vec::new();
        for room in &model.arch.rooms {
            let [x0, y0, x1, y1] = room.rect;
            let (mut t0, mut t1) = (0.0_f64, length);
            for (p, d, lo, hi) in [(a.0, dx, x0, x1), (a.1, dy, y0, y1)] {
                if d.abs() < 1e-9 {
                    if p < lo || p > hi {
                        t1 = -1.0;
                    }
                } else {
                    let (u, v) = ((lo - p) / d, (hi - p) / d);
                    t0 = t0.max(u.min(v));
                    t1 = t1.min(u.max(v));
                }
            }
            if t1 - t0 > 1.0 {
                let (px, py) = frame.paper(((t0 + t1) / 2.0, room.level + 1100.0));
                names.push(group(
                    "roomname",
                    vec![("room", room.name.clone()), ("layer", "A-ANNO".into())],
                    vec![
                        text(px, py, TEXT, &room.title, Anchor::Middle),
                        text(
                            px,
                            py + 3.4,
                            TEXT * 0.8,
                            &format!("{:.2} m²", room.area()),
                            Anchor::Middle,
                        ),
                    ],
                ));
            }
        }
        marks.extend(names);
        let (bx0, by0, _, _) = frame.box_paper();
        marks.push(group(
            "sectionmark",
            vec![("section", name.clone()), ("layer", "A-ANNO".into())],
            vec![
                stroke(
                    vec![circle(bx0 - 7.0, by0 + 4.0, 5.5)],
                    true,
                    PROJECTED_WEIGHT,
                    false,
                ),
                text(bx0 - 7.0, by0 + 5.2, TEXT * 0.9, &name, Anchor::Middle),
            ],
        ));
        let _ = (CUT_WEIGHT, BLACK);
        let count = cut.len();
        let label = format!("section {name}");
        let mut shown = line.clone();
        if named(&shown, "label").is_none() {
            shown.named.push(("label".into(), label));
        }
        self.finish_plan(&shown, frame, marks, "", "section")?;
        self.last_view = Some(line.clone());
        let levels: Vec<(f64, String)> = self
            .model()?
            .arch
            .levels
            .iter()
            .map(|l| (l.z, l.title.clone()))
            .collect();
        for (z, title) in levels {
            let level = Line {
                number: line.number,
                text: String::new(),
                label: None,
                op: "level".into(),
                positional: vec![number(z), title],
                named: Vec::new(),
            };
            self.op_level(&level)?;
        }
        Ok(format!("section {name}, {count} cut polygons"))
    }

    pub(super) fn op_drawinglist(&mut self, line: &Line) -> Result<String> {
        let at = named(line, "place")
            .map(|t| pair(t, &self.scope))
            .transpose()?;
        let index = self.drawing.sheets.len().saturating_sub(1);
        if self.drawing.sheets.is_empty() {
            bail!("start a `sheet` before `drawinglist`");
        }
        self.lists.push((index, at));
        Ok("drawing list".into())
    }

    pub(super) fn fill_lists(&mut self) {
        let rows: Vec<Vec<String>> = self
            .drawing
            .sheets
            .iter()
            .map(|s| {
                vec![
                    s.number.clone(),
                    s.name.clone(),
                    super::scale_text(s.scale),
                    format!("{} x {}", number(s.width), number(s.height)),
                ]
            })
            .collect();
        let table = crate::model::arch::Table {
            header: ["number", "title", "scale", "size"]
                .map(String::from)
                .to_vec(),
            rows,
        };
        for (index, at) in std::mem::take(&mut self.lists) {
            let (x, top) = match at {
                Some((x, y)) => (x, self.drawing.sheets[index].height - y),
                None => (super::MARGIN + 22.0, super::MARGIN + 14.0),
            };
            let marks = super::table_marks("drawing list", &table, x, top);
            self.drawing.sheets[index].marks.extend(marks);
        }
    }
}
