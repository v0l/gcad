use super::hatch::{default_pattern, hatch_marks, pattern};
use super::{
    Anchor, BLACK, BODY, Frame, Mark, State, TEXT, lighten, named, number, scale_of, scale_text,
};
use crate::drawing::section_of;
use crate::geometry;
use crate::parse::{Line, eval};
use anyhow::{Result, anyhow, bail};
use gcad_mesh::arch::P2;
use gcad_mesh::arch::layout::{left, signed_area};
use monstertruck::modeling::*;

pub(super) const CUT_WEIGHT: f64 = 0.35;
pub(super) const PROJECTED_WEIGHT: f64 = 0.18;
pub(super) const HIDDEN_WEIGHT: f64 = 0.09;

pub(super) fn group(class: &str, data: Vec<(&str, String)>, marks: Vec<Mark>) -> Mark {
    Mark::Group {
        class: class.to_string(),
        data: data.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        marks,
    }
}

pub(super) fn stroke(loops: Vec<Vec<(f64, f64)>>, closed: bool, width: f64, dashed: bool) -> Mark {
    Mark::Shape {
        loops,
        closed,
        fill: None,
        stroke: Some(BLACK),
        width,
        dashed,
    }
}

pub(super) fn text(x: f64, y: f64, size: f64, words: &str, anchor: Anchor) -> Mark {
    Mark::Text {
        x,
        y,
        size,
        text: words.to_string(),
        anchor,
        angle: 0.0,
    }
}

pub(super) fn circle(x: f64, y: f64, r: f64) -> Vec<(f64, f64)> {
    (0..24)
        .map(|k| {
            let a = std::f64::consts::TAU * k as f64 / 24.0;
            (x + r * a.cos(), y + r * a.sin())
        })
        .collect()
}

pub(super) struct Polygon {
    pub outer: Vec<P2>,
    pub holes: Vec<Vec<P2>>,
}

impl Polygon {
    pub fn area(&self) -> f64 {
        signed_area(&self.outer).abs()
            - self.holes.iter().map(|h| signed_area(h).abs()).sum::<f64>()
    }

    pub fn outer_area(&self) -> f64 {
        signed_area(&self.outer).abs()
    }

    pub fn rings(&self) -> Vec<Vec<P2>> {
        std::iter::once(self.outer.clone())
            .chain(self.holes.iter().cloned())
            .collect()
    }
}

fn inside(point: P2, ring: &[P2]) -> bool {
    let mut hit = false;
    for i in 0..ring.len() {
        let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
        if (p.1 > point.1) != (q.1 > point.1)
            && point.0 < (q.0 - p.0) * (point.1 - p.1) / (q.1 - p.1) + p.0
        {
            hit = !hit;
        }
    }
    hit
}

pub(super) fn polygons(loops: Vec<Vec<P2>>) -> Vec<Polygon> {
    let loops: Vec<Vec<P2>> = loops
        .into_iter()
        .map(|mut l| {
            if l.len() > 1 && l.first() == l.last() {
                l.pop();
            }
            l
        })
        .filter(|l| l.len() > 2)
        .collect();
    let areas: Vec<f64> = loops.iter().map(|l| signed_area(l).abs()).collect();
    let containers = |i: usize| -> Vec<usize> {
        (0..loops.len())
            .filter(|&j| j != i && areas[j] > areas[i] && inside(loops[i][0], &loops[j]))
            .collect()
    };
    let mut out: Vec<(usize, Polygon)> = Vec::new();
    for i in 0..loops.len() {
        if containers(i).len() % 2 == 0 {
            out.push((
                i,
                Polygon {
                    outer: loops[i].clone(),
                    holes: Vec::new(),
                },
            ));
        }
    }
    for i in 0..loops.len() {
        let c = containers(i);
        if c.len() % 2 == 1 {
            let owner = c
                .into_iter()
                .filter(|j| out.iter().any(|(k, _)| k == j))
                .min_by(|a, b| areas[*a].total_cmp(&areas[*b]));
            if let Some(owner) = owner {
                if let Some((_, polygon)) = out.iter_mut().find(|(k, _)| *k == owner) {
                    polygon.holes.push(loops[i].clone());
                }
            }
        }
    }
    out.into_iter().map(|(_, p)| p).collect()
}

struct Piece {
    body: String,
    material: String,
    layer: &'static str,
}

impl State {
    pub(super) fn level_of(&self, line: &Line) -> Result<(String, f64, String)> {
        let name = line
            .positional
            .first()
            .ok_or_else(|| anyhow!("write `plan L0 cut=1200 scale=1:50`, naming a level"))?;
        let level = self
            .model()?
            .arch
            .levels
            .iter()
            .find(|l| l.name == *name)
            .ok_or_else(|| anyhow!("there is no level `{name}`; declare it with `level`"))?;
        Ok((level.name.clone(), level.z, level.title.clone()))
    }

    fn cut_pieces(&self, slabs: bool) -> Result<Vec<Piece>> {
        let arch = &self.model()?.arch;
        let mut pieces = Vec::new();
        if slabs {
            for stair in &arch.stairs {
                for built in arch.built.iter().filter(|b| b.owner == stair.name) {
                    for body in &built.bodies {
                        let material = self
                            .model()?
                            .materials
                            .get(body)
                            .map_or_else(|| "timber".to_string(), |m| m.name.clone());
                        pieces.push(Piece {
                            body: body.clone(),
                            material,
                            layer: "A-STRS",
                        });
                    }
                }
            }
            for slab in &arch.slabs {
                for (body, l) in slab.layer_names() {
                    pieces.push(Piece {
                        body,
                        material: slab.layers[l].material.clone(),
                        layer: "A-FLOR",
                    });
                }
            }
        }
        for wall in &arch.walls {
            for (body, t, l) in wall.layer_names() {
                pieces.push(Piece {
                    body,
                    material: wall.tiers[t].layers[l].material.clone(),
                    layer: "A-WALL",
                });
            }
        }
        for column in &arch.columns {
            pieces.push(Piece {
                body: column.name.clone(),
                material: column.material.clone(),
                layer: "A-COLS",
            });
        }
        Ok(pieces)
    }

    pub(super) fn cut_polygons(
        &self,
        normal: Vector3,
        at: f64,
        project: &(dyn Fn(monstertruck::modeling::Point3) -> P2 + Sync),
        slabs: bool,
    ) -> Result<Vec<(String, &'static str, String, Polygon)>> {
        let model = self.model()?;
        let pieces = self.cut_pieces(slabs)?;
        let solids: Vec<(String, &'static str, String, monstertruck::modeling::Solid)> = pieces
            .into_iter()
            .filter_map(|p| {
                let solid = model.named_body(&p.body).ok()?;
                let b = geometry::bounds(&solid);
                let along: Vec<f64> = (0..8)
                    .map(|i| {
                        let pick =
                            |bit: usize, lo: f64, hi: f64| if i & bit == 0 { lo } else { hi };
                        monstertruck::modeling::Point3::new(
                            pick(1, b.min().x, b.max().x),
                            pick(2, b.min().y, b.max().y),
                            pick(4, b.min().z, b.max().z),
                        )
                        .to_vec()
                        .dot(normal)
                    })
                    .collect();
                let lo = along.iter().cloned().fold(f64::MAX, f64::min);
                let hi = along.iter().cloned().fold(f64::MIN, f64::max);
                (lo <= at && at <= hi).then_some((p.body, p.layer, p.material, solid))
            })
            .collect();
        let loops: Vec<Vec<Vec<P2>>> =
            crate::host::each(solids.iter().collect(), |(_, _, _, s)| {
                let mesh = geometry::mesh(s, geometry::mesh_tolerance(s));
                section_of(&mesh, s, normal, at)
                    .into_iter()
                    .map(|l| l.into_iter().map(project).collect())
                    .collect()
            })
            .into_iter()
            .map(|l| l.unwrap_or_default())
            .collect();
        let mut out = Vec::new();
        for ((body, layer, material, _), loops) in solids.into_iter().zip(loops) {
            for polygon in polygons(loops) {
                out.push((body.clone(), layer, material.clone(), polygon));
            }
        }
        Ok(out)
    }

    pub(super) fn poche_marks(
        &self,
        frame: &Frame,
        cut: &[(String, &'static str, String, Polygon)],
    ) -> Vec<Mark> {
        let mut marks = Vec::new();
        let model = self.model().ok();
        for (body, layer, material, polygon) in cut {
            let colour = model
                .and_then(|m| m.colours.get(body).copied())
                .unwrap_or(BODY);
            let rings: Vec<Vec<(f64, f64)>> = polygon
                .rings()
                .iter()
                .map(|r| r.iter().map(|p| frame.paper(*p)).collect())
                .collect();
            let area = format!("{:.0}", polygon.area());
            marks.push(group(
                "poche",
                vec![
                    ("body", body.clone()),
                    ("material", material.clone()),
                    ("layer", layer.to_string()),
                    ("area", area.clone()),
                ],
                vec![Mark::Shape {
                    loops: rings.clone(),
                    closed: true,
                    fill: Some(lighten(colour, 0.55)),
                    stroke: Some(BLACK),
                    width: CUT_WEIGHT,
                    dashed: false,
                }],
            ));
            marks.push(self.hatch_group(material, &rings, &area, layer));
        }
        marks
    }

    pub(super) fn op_hatch(&mut self, line: &Line) -> Result<String> {
        let material = line
            .positional
            .first()
            .ok_or_else(|| anyhow!("write `hatch brick pattern=brick`"))?
            .clone();
        let name = named(line, "pattern").unwrap_or(default_pattern(&material));
        if pattern(name).is_none() {
            bail!(
                "`{name}` is not a hatch pattern; use {}",
                super::hatch::PATTERNS
                    .iter()
                    .map(|p| p.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        self.hatches.insert(material.clone(), name.to_string());
        let mut replaced = 0;
        let mut marks = std::mem::take(&mut self.sheet()?.marks);
        for i in 0..marks.len().saturating_sub(1) {
            let (rings, area, layer) = match &marks[i] {
                Mark::Group { class, data, marks } if class == "poche" => {
                    let get = |k: &str| data.iter().find(|(d, _)| d == k).map(|(_, v)| v.clone());
                    if get("material").as_deref() != Some(material.as_str()) {
                        continue;
                    }
                    let rings = match marks.first() {
                        Some(Mark::Shape { loops, .. }) => loops.clone(),
                        _ => continue,
                    };
                    (
                        rings,
                        get("area").unwrap_or_default(),
                        get("layer").unwrap_or_default(),
                    )
                }
                _ => continue,
            };
            if matches!(&marks[i + 1], Mark::Group { class, .. } if class.starts_with("hatch ")) {
                marks[i + 1] = self.hatch_group(&material, &rings, &area, &layer);
                replaced += 1;
            }
        }
        self.sheet()?.marks = marks;
        Ok(format!("{material} hatched {name}, {replaced} areas"))
    }

    pub(super) fn hatch_group(
        &self,
        material: &str,
        rings: &[Vec<(f64, f64)>],
        area: &str,
        layer: &str,
    ) -> Mark {
        let name = self
            .hatches
            .get(material)
            .cloned()
            .unwrap_or_else(|| default_pattern(material).to_string());
        let marks = pattern(&name)
            .map(|p| clip_hatch(rings, p))
            .unwrap_or_default();
        group(
            &format!("hatch {name}"),
            vec![
                ("material", material.to_string()),
                ("area", area.to_string()),
                ("layer", layer.to_string()),
            ],
            marks,
        )
    }

    fn open_plan(
        &mut self,
        line: &Line,
        cut_default: f64,
        kind: &str,
    ) -> Result<(f64, f64, Frame, String)> {
        let (_, z, title) = self.level_of(line)?;
        let cut = match named(line, "cut") {
            Some(t) => eval(t, &self.scope)?,
            None => cut_default,
        };
        let scale = match named(line, "scale") {
            Some(s) => scale_of(s)?,
            None => self.sheet()?.scale,
        };
        let pts = {
            let model = self.model()?;
            let mut pts: Vec<P2> = Vec::new();
            for wall in &model.arch.walls {
                let (lo, hi) = wall.outer_span();
                for i in 0..wall.line.segments() {
                    let n = left(wall.line.dir(i));
                    let (a, b) = wall.line.ends(i);
                    for d in [lo, hi] {
                        for p in [a, b] {
                            pts.push((p.0 + n.0 * d, p.1 + n.1 * d));
                        }
                    }
                }
            }
            for c in &model.arch.columns {
                if let Ok(s) = model.named_body(&c.name) {
                    let b = geometry::bounds(&s);
                    pts.push((b.min().x, b.min().y));
                    pts.push((b.max().x, b.max().y));
                }
            }
            for r in &model.arch.rooms {
                pts.push((r.rect[0], r.rect[1]));
                pts.push((r.rect[2], r.rect[3]));
            }
            for stair in &model.arch.stairs {
                for f in &stair.flights {
                    let length = f.treads as f64 * stair.steps.tread;
                    for (s, u) in [
                        (0.0, 0.0),
                        (length, 0.0),
                        (length, stair.width),
                        (0.0, stair.width),
                    ] {
                        pts.push(f.plan(s, u));
                    }
                }
            }
            pts
        };
        if pts.is_empty() {
            bail!("the model has no walls or columns to draw a {kind} of");
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in &pts {
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        }
        self.plan_level = z;
        let margin = 1500.0;
        let window = match named(line, "window") {
            Some(t) => {
                let (a, b, c, d) = super::quad(t, &self.scope)?;
                (a.min(c), b.min(d), a.max(c), b.max(d))
            }
            None => (x0 - margin, y0 - margin, x1 + margin, y1 + margin),
        };
        let (w, h) = ((window.2 - window.0) / scale, (window.3 - window.1) / scale);
        let at = named(line, "place")
            .map(|t| super::pair(t, &self.scope))
            .transpose()?;
        let (x, y) = self.place(w, h, at)?;
        let frame = Frame {
            right: Vector3::unit_x(),
            up: Vector3::unit_y(),
            scale,
            origin: (x, y),
            window,
            up_is_z: false,
        };
        Ok((z, z + cut, frame, title))
    }

    pub(super) fn finish_plan(
        &mut self,
        line: &Line,
        frame: Frame,
        marks: Vec<Mark>,
        title: &str,
        kind: &str,
    ) -> Result<String> {
        let (bx0, by0, _, _) = frame.box_paper();
        let label = match named(line, "label") {
            Some(t) => t.trim_matches('"').to_string(),
            None => line.label.clone().unwrap_or_else(|| {
                let level = line.positional.first().cloned().unwrap_or_default();
                if title.is_empty() {
                    format!("{kind} {level}")
                } else {
                    format!("{kind} {level} {title}")
                }
            }),
        };
        let mut marks = marks;
        if named(line, "window").is_some() {
            marks = super::building::clip_marks(marks, frame.box_paper());
        }
        if !label.is_empty() {
            marks.push(text(
                bx0,
                by0 - 4.0,
                3.5,
                &format!("{label} {}", scale_text(frame.scale)),
                Anchor::Start,
            ));
        }
        self.view_geometry = None;
        self.last_view = Some(line.clone());
        self.view_box = Some(frame.box_paper());
        self.sheet()?.marks.extend(marks);
        self.frame = Some(frame);
        Ok(format!(
            "{kind} of {}",
            line.positional.first().cloned().unwrap_or_default()
        ))
    }

    pub(super) fn op_plan(&mut self, line: &Line) -> Result<String> {
        let (level_z, cut_z, frame, title) = self.open_plan(line, 1200.0, "plan")?;
        let cut = self.cut_polygons(Vector3::unit_z(), cut_z, &|p| (p.x, p.y), false)?;
        let mut marks = self.poche_marks(&frame, &cut);
        marks.extend(self.opening_marks(&frame, cut_z));
        marks.extend(self.stair_marks(&frame, level_z, cut_z));
        marks.extend(self.slab_edges(&frame, level_z));
        if named(line, "grids").is_some_and(|t| t != "0") {
            marks.extend(self.grid_marks(&frame));
        }
        if named(line, "levels").is_some_and(|t| t != "0") {
            marks.extend(self.level_note(&frame, level_z, &title));
        }
        let count = cut.len();
        self.finish_plan(line, frame, marks, &title, "plan")?;
        Ok(format!("{count} poche polygons"))
    }

    pub(super) fn op_rcp(&mut self, line: &Line) -> Result<String> {
        let (level_z, cut_z, frame, title) =
            self.open_plan(line, 2400.0, "reflected ceiling plan")?;
        let cut = self.cut_polygons(Vector3::unit_z(), cut_z, &|p| (p.x, p.y), false)?;
        let mut marks = Vec::new();
        for (body, layer, _, polygon) in &cut {
            let rings: Vec<Vec<(f64, f64)>> = polygon
                .rings()
                .iter()
                .map(|r| r.iter().map(|p| frame.paper(*p)).collect())
                .collect();
            marks.push(group(
                "wall",
                vec![("body", body.clone()), ("layer", layer.to_string())],
                vec![Mark::Shape {
                    loops: rings,
                    closed: true,
                    fill: Some([0.35, 0.35, 0.35]),
                    stroke: Some(BLACK),
                    width: PROJECTED_WEIGHT,
                    dashed: false,
                }],
            ));
        }
        let rooms: Vec<_> = self
            .model()?
            .arch
            .rooms
            .iter()
            .filter(|r| (r.level - level_z).abs() < 1.0)
            .cloned()
            .collect();
        if rooms.is_empty() {
            let innermost = cut
                .iter()
                .filter(|(_, _, _, p)| !p.holes.is_empty())
                .min_by(|a, b| a.3.outer_area().total_cmp(&b.3.outer_area()));
            if let Some((_, _, _, polygon)) = innermost {
                for hole in &polygon.holes {
                    let ring: Vec<(f64, f64)> = hole.iter().map(|p| frame.paper(*p)).collect();
                    let area = signed_area(hole).abs() / 1.0e6;
                    marks.push(group(
                        "ceiling",
                        vec![("area", format!("{area:.2}")), ("layer", "A-CLNG".into())],
                        vec![stroke(vec![ring], true, PROJECTED_WEIGHT, false)],
                    ));
                }
            }
        }
        for room in &rooms {
            let [x0, y0, x1, y1] = room.rect;
            let (a, b) = (frame.paper((x0, y0)), frame.paper((x1, y1)));
            let mut inner = vec![
                stroke(
                    vec![vec![a, (b.0, a.1), b, (a.0, b.1)]],
                    true,
                    PROJECTED_WEIGHT,
                    false,
                ),
                stroke(
                    vec![vec![a, b], vec![(a.0, b.1), (b.0, a.1)]],
                    false,
                    HIDDEN_WEIGHT,
                    false,
                ),
            ];
            let c = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            inner.push(text(c.0, c.1 - 1.0, TEXT, &room.title, Anchor::Middle));
            inner.push(text(
                c.0,
                c.1 + 3.0,
                TEXT * 0.8,
                &format!("CH {}", number(room.height)),
                Anchor::Middle,
            ));
            marks.push(group(
                "ceiling",
                vec![("room", room.name.clone()), ("layer", "A-CLNG".into())],
                inner,
            ));
        }
        self.finish_plan(line, frame, marks, &title, "reflected ceiling plan")?;
        Ok(format!("{} ceilings", rooms.len()))
    }

    fn opening_marks(&self, frame: &Frame, cut_z: f64) -> Vec<Mark> {
        let Ok(model) = self.model() else {
            return Vec::new();
        };
        let mut marks = Vec::new();
        for opening in &model.arch.openings {
            let Some(wall) = model.arch.wall(&opening.host) else {
                continue;
            };
            if cut_z < wall.base || cut_z > wall.top {
                continue;
            }
            let (lo, hi) = wall.outer_span();
            let (s0, s1) = (opening.at - opening.w / 2.0, opening.at + opening.w / 2.0);
            let (i, _, dir) = wall.line.locate(opening.at);
            let starts = wall.line.starts();
            let origin = wall.line.points[i];
            let n = left(dir);
            let at = |s: f64, d: f64| -> (f64, f64) {
                let t = s - starts[i];
                frame.paper((
                    origin.0 + dir.0 * t + n.0 * d,
                    origin.1 + dir.1 * t + n.1 * d,
                ))
            };
            let local = cut_z - wall.base;
            let dashed = local < opening.sill || local > opening.sill + opening.h;
            match &opening.fill {
                crate::model::arch::Fill::Door {
                    swing,
                    inward,
                    frame: (fw, _),
                    ..
                } => {
                    let hinge_left = swing != "right";
                    let leaf = opening.w - 2.0 * fw;
                    let (hs, away) = if hinge_left {
                        (s0 + fw, 1.0)
                    } else {
                        (s1 - fw, -1.0)
                    };
                    let face = if *inward { hi } else { lo };
                    let side = if *inward { 1.0 } else { -1.0 };
                    let hinge = (hs, face);
                    let open_end = (hs, face + side * leaf);
                    let arc: Vec<(f64, f64)> = (0..=16)
                        .map(|k| {
                            let a = std::f64::consts::FRAC_PI_2 * k as f64 / 16.0;
                            at(hs + away * leaf * a.cos(), face + side * leaf * a.sin())
                        })
                        .collect();
                    let thickness = 40.0;
                    let leaf_rect = vec![
                        at(hinge.0, hinge.1),
                        at(open_end.0, open_end.1),
                        at(open_end.0 + away * thickness, open_end.1),
                        at(hinge.0 + away * thickness, hinge.1),
                    ];
                    let jambs = vec![vec![at(s0, lo), at(s0, hi)], vec![at(s1, lo), at(s1, hi)]];
                    marks.push(group(
                        "swing",
                        vec![
                            ("door", opening.name.clone()),
                            ("mark", opening.mark.clone().unwrap_or_default()),
                            ("layer", "A-DOOR".into()),
                        ],
                        vec![
                            stroke(jambs, false, PROJECTED_WEIGHT, false),
                            stroke(vec![leaf_rect], true, PROJECTED_WEIGHT, false),
                            stroke(vec![arc], false, HIDDEN_WEIGHT, false),
                        ],
                    ));
                }
                crate::model::arch::Fill::Window { .. } => {
                    let mid = (lo + hi) / 2.0;
                    let inner = vec![
                        vec![at(s0, lo), at(s1, lo)],
                        vec![at(s0, hi), at(s1, hi)],
                        vec![at(s0, lo), at(s0, hi)],
                        vec![at(s1, lo), at(s1, hi)],
                    ];
                    marks.push(group(
                        "window",
                        vec![
                            ("window", opening.name.clone()),
                            ("mark", opening.mark.clone().unwrap_or_default()),
                            ("layer", "A-GLAZ".into()),
                        ],
                        vec![
                            stroke(inner, false, PROJECTED_WEIGHT, dashed),
                            stroke(
                                vec![vec![at(s0, mid), at(s1, mid)]],
                                false,
                                CUT_WEIGHT * 0.7,
                                false,
                            ),
                        ],
                    ));
                }
                _ => {}
            }
        }
        marks
    }

    fn stair_marks(&self, frame: &Frame, level_z: f64, cut_z: f64) -> Vec<Mark> {
        let Ok(model) = self.model() else {
            return Vec::new();
        };
        let mut marks = Vec::new();
        for stair in &model.arch.stairs {
            let top = stair.base + stair.steps.risers as f64 * stair.steps.rise;
            let up = (stair.base - level_z).abs() < 1.0;
            let down = (top - level_z).abs() < 1.0;
            if !up && !down {
                continue;
            }
            let (rise, tread) = (stair.steps.rise, stair.steps.tread);
            let mut inner = Vec::new();
            let mut breaks = Vec::new();
            for flight in &stair.flights {
                let length = flight.treads as f64 * tread;
                let p = |s: f64, u: f64| frame.paper(flight.plan(s, u));
                inner.push(stroke(
                    vec![vec![
                        p(0.0, 0.0),
                        p(length, 0.0),
                        p(length, stair.width),
                        p(0.0, stair.width),
                    ]],
                    true,
                    PROJECTED_WEIGHT,
                    false,
                ));
                let mut solid = Vec::new();
                let mut dashed = Vec::new();
                let mut broken_at = None;
                for k in 1..flight.treads {
                    let rise_here = flight.z0 + (k + 1) as f64 * rise;
                    let line = vec![p(k as f64 * tread, 0.0), p(k as f64 * tread, stair.width)];
                    if up && rise_here > cut_z {
                        if broken_at.is_none() {
                            broken_at = Some(k as f64 * tread);
                        }
                        dashed.push(line);
                    } else {
                        solid.push(line);
                    }
                }
                if !solid.is_empty() {
                    inner.push(stroke(solid, false, HIDDEN_WEIGHT * 1.5, false));
                }
                if !dashed.is_empty() {
                    inner.push(stroke(dashed, false, HIDDEN_WEIGHT, true));
                }
                let mid = stair.width / 2.0;
                let (a, b) = (p(tread * 0.5, mid), p(length - tread * 0.5, mid));
                let (from, to) = if down { (b, a) } else { (a, b) };
                inner.push(stroke(vec![vec![from, to]], false, PROJECTED_WEIGHT, false));
                let (dx, dy) = (to.0 - from.0, to.1 - from.1);
                let len = dx.hypot(dy).max(1e-9);
                let (ux, uy) = (dx / len, dy / len);
                inner.push(Mark::Shape {
                    loops: vec![vec![
                        to,
                        (to.0 - ux * 3.0 - uy * 1.2, to.1 - uy * 3.0 + ux * 1.2),
                        (to.0 - ux * 3.0 + uy * 1.2, to.1 - uy * 3.0 - ux * 1.2),
                    ]],
                    closed: true,
                    fill: Some(BLACK),
                    stroke: None,
                    width: 0.0,
                    dashed: false,
                });
                let tail = if down { b } else { a };
                inner.push(text(
                    tail.0 + 1.5,
                    tail.1 - 1.5,
                    TEXT,
                    if down { "DN" } else { "UP" },
                    Anchor::Start,
                ));
                if let Some(s) = broken_at {
                    let w = stair.width;
                    let zig = vec![
                        p(s, -100.0),
                        p(s, w * 0.4),
                        p(s + tread * 0.3, w * 0.5),
                        p(s - tread * 0.3, w * 0.6),
                        p(s, w * 0.7),
                        p(s, w + 100.0),
                    ];
                    breaks.push(stroke(vec![zig], false, PROJECTED_WEIGHT, false));
                }
            }
            marks.push(group(
                "stair",
                vec![("stair", stair.name.clone()), ("layer", "A-STRS".into())],
                inner,
            ));
            if !breaks.is_empty() {
                marks.push(group("break", vec![("layer", "A-STRS".into())], breaks));
            }
        }
        marks
    }

    fn slab_edges(&self, frame: &Frame, level_z: f64) -> Vec<Mark> {
        let Ok(model) = self.model() else {
            return Vec::new();
        };
        model
            .arch
            .slabs
            .iter()
            .filter(|s| (s.top - level_z).abs() < 500.0)
            .map(|s| {
                let mut rings = vec![
                    s.outline
                        .iter()
                        .map(|p| frame.paper(*p))
                        .collect::<Vec<_>>(),
                ];
                rings.extend(
                    s.holes
                        .iter()
                        .map(|h| h.iter().map(|p| frame.paper(*p)).collect::<Vec<_>>()),
                );
                group(
                    "slab",
                    vec![("slab", s.name.clone()), ("layer", "A-FLOR".into())],
                    vec![stroke(rings, true, HIDDEN_WEIGHT, true)],
                )
            })
            .collect()
    }

    pub(super) fn grid_marks(&self, frame: &Frame) -> Vec<Mark> {
        let Ok(model) = self.model() else {
            return Vec::new();
        };
        let (wx0, wy0, wx1, wy1) = frame.window;
        let mut marks = Vec::new();
        for grid in &model.arch.grids {
            let (a, b) = if grid.along_x {
                ((grid.offset, wy0 + 600.0), (grid.offset, wy1 - 600.0))
            } else {
                ((wx0 + 600.0, grid.offset), (wx1 - 600.0, grid.offset))
            };
            let (pa, pb) = (frame.paper(a), frame.paper(b));
            let end = if grid.along_x { pb } else { pa };
            let r = 4.0;
            let centre = if grid.along_x {
                (end.0, end.1 - r)
            } else {
                (end.0 - r, end.1)
            };
            let mut inner = vec![
                stroke(vec![vec![pa, pb]], false, HIDDEN_WEIGHT, true),
                stroke(
                    vec![circle(centre.0, centre.1, r)],
                    true,
                    PROJECTED_WEIGHT,
                    false,
                ),
            ];
            inner.push(text(
                centre.0,
                centre.1 + 1.2,
                3.5,
                &grid.name,
                Anchor::Middle,
            ));
            marks.push(group(
                "grid",
                vec![("grid", grid.name.clone()), ("layer", "A-GRID".into())],
                inner,
            ));
        }
        marks
    }

    pub(super) fn op_northarrow(&mut self, line: &Line) -> Result<String> {
        let frame = self.frame()?.clone();
        let north = self
            .model()?
            .arch
            .building
            .as_ref()
            .map_or(0.0, |b| b.north);
        let (_, by0, bx1, _) = frame.box_paper();
        let (cx, cy) = match named(line, "place") {
            Some(t) => super::pair(t, &self.scope)?,
            None => (bx1 - 14.0, by0 + 22.0),
        };
        let r = 9.0;
        let a = (90.0 + north).to_radians();
        let (ux, uy) = (a.cos(), -a.sin());
        let (vx, vy) = (-uy, ux);
        let tip = (cx + ux * r, cy + uy * r);
        let tail = (cx - ux * r, cy - uy * r);
        let half = (
            cx - ux * r * 0.2 + vx * r * 0.35,
            cy - uy * r * 0.2 + vy * r * 0.35,
        );
        let other = (
            cx - ux * r * 0.2 - vx * r * 0.35,
            cy - uy * r * 0.2 - vy * r * 0.35,
        );
        let marks = vec![group(
            "northarrow",
            vec![("north", number(north)), ("layer", "A-ANNO".into())],
            vec![
                stroke(vec![circle(cx, cy, r)], true, PROJECTED_WEIGHT, false),
                Mark::Shape {
                    loops: vec![vec![tip, half, tail, other]],
                    closed: true,
                    fill: Some(BLACK),
                    stroke: Some(BLACK),
                    width: HIDDEN_WEIGHT,
                    dashed: false,
                },
                text(tip.0, tip.1 - 2.0, 4.0, "N", Anchor::Middle),
            ],
        )];
        self.sheet()?.marks.extend(marks);
        Ok(format!("north {}", number(north)))
    }

    pub(super) fn op_scalebar(&mut self, line: &Line) -> Result<String> {
        let frame = self.frame()?.clone();
        let (bx0, _, _, by1) = frame.box_paper();
        let (x, y) = match named(line, "place") {
            Some(t) => super::pair(t, &self.scope)?,
            None => (bx0 + 6.0, by1 - 8.0),
        };
        let metre = 1000.0 / frame.scale;
        let ticks = match named(line, "length") {
            Some(t) => eval(t, &self.scope)?.round().max(1.0) as usize,
            None => 5,
        };
        let mut inner = Vec::new();
        for k in 0..ticks {
            let (a, b) = (x + k as f64 * metre, x + (k + 1) as f64 * metre);
            inner.push(Mark::Shape {
                loops: vec![vec![(a, y), (b, y), (b, y + 1.6), (a, y + 1.6)]],
                closed: true,
                fill: if k % 2 == 0 {
                    Some(BLACK)
                } else {
                    Some([1.0, 1.0, 1.0])
                },
                stroke: Some(BLACK),
                width: HIDDEN_WEIGHT * 1.5,
                dashed: false,
            });
        }
        for k in 0..=ticks {
            inner.push(text(
                x + k as f64 * metre,
                y - 1.0,
                TEXT * 0.8,
                &k.to_string(),
                Anchor::Middle,
            ));
        }
        inner.push(text(
            x + ticks as f64 * metre + 4.0,
            y + 1.4,
            TEXT * 0.8,
            "m",
            Anchor::Start,
        ));
        self.sheet()?
            .marks
            .push(group("scalebar", vec![("layer", "A-ANNO".into())], inner));
        Ok(format!("{ticks} m at {}", scale_text(frame.scale)))
    }

    fn level_note(&self, frame: &Frame, level_z: f64, title: &str) -> Vec<Mark> {
        let (bx0, _, _, by1) = frame.box_paper();
        let (x, y) = (bx0 + 6.0, by1 - 4.0);
        let words = format!(
            "{}{} {title}",
            if level_z >= 0.0 { "+" } else { "" },
            number(level_z.round())
        );
        vec![group(
            "levelmark",
            vec![("layer", "A-ANNO".into())],
            vec![
                Mark::Shape {
                    loops: vec![vec![(x, y), (x - 1.6, y - 2.4), (x + 1.6, y - 2.4)]],
                    closed: true,
                    fill: Some(BLACK),
                    stroke: None,
                    width: 0.0,
                    dashed: false,
                },
                text(x + 3.0, y - 1.0, TEXT, words.trim(), Anchor::Start),
            ],
        )]
    }
}

fn clip_hatch(rings: &[Vec<(f64, f64)>], pattern: &super::hatch::Pattern) -> Vec<Mark> {
    hatch_marks(rings, pattern)
}
