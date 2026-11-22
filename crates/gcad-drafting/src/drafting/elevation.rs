use super::hatch::{default_pattern, hatch_marks, pattern};
use super::plan::{PROJECTED_WEIGHT, group};
use super::{BLACK, BODY, Frame, Mark, State, direction, lighten, named, pair, quad, scale_of};
use crate::geometry;
use crate::parse::Line;
use anyhow::{Result, bail};
use monstertruck::modeling::*;
use std::collections::HashMap;

type Key = [i64; 3];

const PIXEL: f64 = 0.5;
const SMOOTH: f64 = 0.978;
const HATCH_STRETCH: f64 = 6.0;
const HATCH_MIN_AREA: f64 = 150.0;

struct Tri {
    body: usize,
    keys: [Key; 3],
    uv: [(f64, f64); 3],
    depth: [f64; 3],
    normal: Vector3,
}

struct Body {
    name: String,
    colour: [f64; 3],
    material: String,
    layer: &'static str,
    wall: bool,
}

fn key(p: Point3) -> Key {
    [p.x, p.y, p.z].map(|c| (c * 1000.0).round() as i64)
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn straighten(points: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let n = points.len();
    if n < 4 {
        return points;
    }
    (0..n)
        .filter(|&i| {
            let (p, c, q) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
            let cross = (c.0 - p.0) * (q.1 - c.1) - (c.1 - p.1) * (q.0 - c.0);
            let scale = (c.0 - p.0).hypot(c.1 - p.1) * (q.0 - c.0).hypot(q.1 - c.1);
            cross.abs() > 1.0e-4 * scale.max(1.0e-12)
        })
        .map(|i| points[i])
        .collect()
}

fn ring_area(points: &[(f64, f64)]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (p, q) = (points[i], points[(i + 1) % n]);
            p.0 * q.1 - q.0 * p.1
        })
        .sum::<f64>()
        / 2.0
}

impl State {
    fn elevation_bodies(&self) -> Result<Vec<Body>> {
        let model = self.model()?;
        let arch = &model.arch;
        let mut layers: HashMap<String, (&'static str, bool)> = HashMap::new();
        for wall in &arch.walls {
            for (body, _, _) in wall.layer_names() {
                layers.insert(body, ("A-WALL", true));
            }
        }
        for opening in &arch.openings {
            let layer = match opening.fill {
                crate::model::arch::Fill::Door { .. } => "A-DOOR",
                crate::model::arch::Fill::Window { .. } => "A-GLAZ",
                _ => "A-WALL",
            };
            for (body, _) in model.named_solids() {
                if body.starts_with(&format!("{}.", opening.name)) {
                    layers.insert(body, (layer, false));
                }
            }
        }
        for slab in &arch.slabs {
            for (body, _) in slab.layer_names() {
                layers.insert(body, ("A-FLOR", false));
            }
        }
        for roof in &arch.roofs {
            layers.insert(roof.name.clone(), ("A-ROOF", false));
        }
        for column in &arch.columns {
            layers.insert(column.name.clone(), ("A-COLS", false));
        }
        for stair in &arch.stairs {
            for built in arch.built.iter().filter(|b| b.owner == stair.name) {
                for body in &built.bodies {
                    layers.insert(body.clone(), ("A-STRS", false));
                }
            }
        }
        Ok(model
            .named_solids()
            .into_iter()
            .map(|(name, _)| {
                let (layer, wall) = layers.get(&name).copied().unwrap_or(("A-GENR", false));
                Body {
                    colour: model.colours.get(&name).copied().unwrap_or(BODY),
                    material: model
                        .materials
                        .get(&name)
                        .map(|m| m.name.clone())
                        .unwrap_or_default(),
                    layer,
                    wall,
                    name,
                }
            })
            .collect())
    }

    pub(super) fn op_elevation(&mut self, line: &Line) -> Result<String> {
        let from = named(line, "from").unwrap_or("-y").to_string();
        let (toward, up) = direction(&from)?;
        if up.z.abs() < 0.5 {
            bail!("an elevation looks from -y, +y, -x or +x");
        }
        let scale = match named(line, "scale") {
            Some(s) => scale_of(s)?,
            None => self.sheet()?.scale,
        };
        let frame0 = Frame {
            right: toward.cross(up),
            up,
            scale,
            origin: (0.0, 0.0),
            window: (0.0, 0.0, 0.0, 0.0),
            up_is_z: true,
        };
        let bodies = self.elevation_bodies()?;
        let (tris, extent) = {
            let model = self.model()?;
            let solids: Vec<(usize, Solid)> = model
                .named_solids()
                .into_iter()
                .enumerate()
                .map(|(i, (_, s))| (i, s.clone()))
                .collect();
            let meshes = crate::host::each(solids.iter().collect(), |(i, s)| {
                (*i, geometry::mesh(s, geometry::mesh_tolerance(s)))
            });
            let mut tris = Vec::new();
            let mut extent = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for (body, mesh) in meshes.into_iter().flatten() {
                let positions = mesh.positions();
                for p in positions {
                    let (u, v) = frame0.uv(*p);
                    extent = (
                        extent.0.min(u),
                        extent.1.min(v),
                        extent.2.max(u),
                        extent.3.max(v),
                    );
                }
                for tri in mesh.faces().triangle_iter() {
                    let [a, b, c] = tri.map(|v| positions[v.pos]);
                    let n = (b - a).cross(c - a);
                    let len = n.magnitude();
                    if len < 1.0e-9 {
                        continue;
                    }
                    let normal = n / len;
                    if normal.dot(toward) > -1.0e-3 {
                        continue;
                    }
                    let pts = [a, b, c];
                    tris.push(Tri {
                        body,
                        keys: pts.map(key),
                        uv: pts.map(|p| frame0.uv(p)),
                        depth: pts.map(|p| p.to_vec().dot(toward)),
                        normal,
                    });
                }
            }
            (tris, extent)
        };
        if extent.0 > extent.2 {
            bail!("the model has nothing to draw");
        }
        let window = match named(line, "window") {
            Some(text) => {
                let (a, b, c, d) = quad(text, &self.scope)?;
                (a.min(c), b.min(d), a.max(c), b.max(d))
            }
            None => (
                extent.0 - 800.0,
                extent.1 - 300.0,
                extent.2 + 800.0,
                extent.3 + 800.0,
            ),
        };
        let (w, h) = ((window.2 - window.0) / scale, (window.3 - window.1) / scale);
        let place = named(line, "place")
            .map(|t| pair(t, &self.scope))
            .transpose()?;
        let (x, y) = self.place(w, h, place)?;
        let frame = Frame {
            origin: (x, y),
            window,
            ..frame0
        };
        let hatches = self.hatches.clone();
        let marks = face_marks(&bodies, &tris, &frame, &hatches);
        let label = named(line, "label")
            .map(|t| t.trim_matches('"').to_string())
            .or_else(|| line.label.clone())
            .unwrap_or_else(|| format!("elevation {from}"));
        let mut shown = line.clone();
        shown.named.retain(|(k, _)| k != "label");
        shown.named.push(("label".into(), label));
        self.finish_plan(&shown, frame.clone(), marks, "", "elevation")?;
        self.last_view = Some(line.clone());
        let levels: Vec<(f64, String)> = self
            .model()?
            .arch
            .levels
            .iter()
            .map(|l| (l.z, l.title.clone()))
            .collect();
        let count = levels.len();
        if named(line, "levels").is_none_or(|t| t != "0") && named(line, "window").is_none() {
            for (z, title) in levels {
                let level = Line {
                    number: line.number,
                    text: String::new(),
                    label: None,
                    op: "level".into(),
                    positional: vec![super::number(z), title],
                    named: Vec::new(),
                };
                self.op_level(&level)?;
            }
        }
        Ok(format!("elevation from {from}, {count} levels"))
    }
}

fn face_marks(
    bodies: &[Body],
    tris: &[Tri],
    frame: &Frame,
    hatches: &std::collections::BTreeMap<String, String>,
) -> Vec<Mark> {
    let (bx0, by0, bx1, by1) = frame.box_paper();
    let (cols, rows) = (
        (((bx1 - bx0) / PIXEL).ceil() as usize).max(1),
        (((by1 - by0) / PIXEL).ceil() as usize).max(1),
    );
    let mut zbuf = vec![f64::INFINITY; cols * rows];
    let mut ids = vec![u32::MAX; cols * rows];
    let paper: Vec<[(f64, f64); 3]> = tris.iter().map(|t| t.uv.map(|p| frame.paper(p))).collect();
    for (i, p) in paper.iter().enumerate() {
        let (x0, x1) = (
            p.iter().map(|q| q.0).fold(f64::MAX, f64::min),
            p.iter().map(|q| q.0).fold(f64::MIN, f64::max),
        );
        let (y0, y1) = (
            p.iter().map(|q| q.1).fold(f64::MAX, f64::min),
            p.iter().map(|q| q.1).fold(f64::MIN, f64::max),
        );
        if x1 < bx0 || x0 > bx1 || y1 < by0 || y0 > by1 {
            continue;
        }
        let px0 = (((x0 - bx0) / PIXEL).floor().max(0.0)) as usize;
        let px1 = (((x1 - bx0) / PIXEL).ceil().min(cols as f64 - 1.0)).max(0.0) as usize;
        let py0 = (((y0 - by0) / PIXEL).floor().max(0.0)) as usize;
        let py1 = (((y1 - by0) / PIXEL).ceil().min(rows as f64 - 1.0)).max(0.0) as usize;
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        if area.abs() < 1.0e-12 {
            continue;
        }
        for py in py0..=py1 {
            for px in px0..=px1 {
                let (sx, sy) = (
                    bx0 + (px as f64 + 0.5) * PIXEL,
                    by0 + (py as f64 + 0.5) * PIXEL,
                );
                let w0 = ((p[1].0 - sx) * (p[2].1 - sy) - (p[2].0 - sx) * (p[1].1 - sy)) / area;
                let w1 = ((p[2].0 - sx) * (p[0].1 - sy) - (p[0].0 - sx) * (p[2].1 - sy)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1.0e-9 || w1 < -1.0e-9 || w2 < -1.0e-9 {
                    continue;
                }
                let d = w0 * tris[i].depth[0] + w1 * tris[i].depth[1] + w2 * tris[i].depth[2];
                let at = py * cols + px;
                if d < zbuf[at] {
                    zbuf[at] = d;
                    ids[at] = i as u32;
                }
            }
        }
    }
    let mut visible = vec![false; tris.len()];
    for id in &ids {
        if *id != u32::MAX {
            visible[*id as usize] = true;
        }
    }
    let seen: Vec<usize> = (0..tris.len()).filter(|i| visible[*i]).collect();
    let mut parent: Vec<usize> = (0..tris.len()).collect();
    let mut edges: HashMap<(Key, Key), usize> = HashMap::new();
    for &i in &seen {
        let k = tris[i].keys;
        for e in 0..3 {
            let (a, b) = (k[e], k[(e + 1) % 3]);
            let ordered = if a < b { (a, b) } else { (b, a) };
            match edges.get(&ordered) {
                Some(&j)
                    if tris[j].body == tris[i].body
                        && tris[j].normal.dot(tris[i].normal) > SMOOTH =>
                {
                    let (ra, rb) = (find(&mut parent, i), find(&mut parent, j));
                    parent[ra] = rb;
                }
                Some(_) => {}
                None => {
                    edges.insert(ordered, i);
                }
            }
        }
    }
    let mut members: HashMap<usize, Vec<usize>> = HashMap::new();
    for &i in &seen {
        let root = find(&mut parent, i);
        members.entry(root).or_default().push(i);
    }
    let mut groups: Vec<(f64, usize, Vec<Vec<(f64, f64)>>)> = Vec::new();
    for tris_in in members.values() {
        let mut directed: HashMap<(Key, Key), bool> = HashMap::new();
        let mut at: HashMap<Key, (f64, f64)> = HashMap::new();
        for &i in tris_in {
            for e in 0..3 {
                let (a, b) = (tris[i].keys[e], tris[i].keys[(e + 1) % 3]);
                at.entry(a).or_insert(paper[i][e]);
                if directed.remove(&(b, a)).is_none() {
                    directed.insert((a, b), true);
                }
            }
        }
        let mut next: HashMap<Key, Vec<Key>> = HashMap::new();
        let mut starts: Vec<(Key, Key)> = directed.keys().copied().collect();
        starts.sort();
        for (a, b) in &starts {
            next.entry(*a).or_default().push(*b);
        }
        let mut loops = Vec::new();
        for (a, b) in starts {
            if !next.get(&a).is_some_and(|v| v.contains(&b)) {
                continue;
            }
            let mut ring = vec![at[&a]];
            let mut from = a;
            let mut to = b;
            loop {
                if let Some(v) = next.get_mut(&from) {
                    if let Some(pos) = v.iter().position(|k| *k == to) {
                        v.swap_remove(pos);
                    }
                }
                if to == a {
                    break;
                }
                ring.push(at[&to]);
                let Some(follow) = next.get(&to).and_then(|v| v.first().copied()) else {
                    break;
                };
                from = to;
                to = follow;
            }
            let ring = straighten(ring);
            if ring.len() > 2 && ring_area(&ring).abs() > 0.02 {
                loops.push(ring);
            }
        }
        if loops.is_empty() {
            continue;
        }
        let depth = tris_in
            .iter()
            .map(|&i| tris[i].depth.iter().sum::<f64>() / 3.0)
            .sum::<f64>()
            / tris_in.len() as f64;
        groups.push((depth, tris[tris_in[0]].body, loops));
    }
    groups.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut marks = Vec::new();
    for (_, body, loops) in groups {
        let info = &bodies[body];
        let fill = lighten(info.colour, 0.45);
        marks.push(group(
            "face",
            vec![
                ("body", info.name.clone()),
                ("material", info.material.clone()),
                ("layer", info.layer.to_string()),
            ],
            vec![Mark::Shape {
                loops: loops.clone(),
                closed: true,
                fill: Some(fill),
                stroke: Some(BLACK),
                width: PROJECTED_WEIGHT,
                dashed: false,
            }],
        ));
        if info.wall {
            let area: f64 = loops.iter().map(|l| ring_area(l).abs()).sum();
            let name = hatches
                .get(&info.material)
                .cloned()
                .unwrap_or_else(|| default_pattern(&info.material).to_string());
            if area >= HATCH_MIN_AREA {
                if let Some(p) = pattern(&name) {
                    let strokes = hatch_marks(&loops, p, HATCH_STRETCH);
                    if !strokes.is_empty() {
                        marks.push(group(
                            &format!("hatch {name}"),
                            vec![
                                ("material", info.material.clone()),
                                ("layer", info.layer.to_string()),
                            ],
                            strokes,
                        ));
                    }
                }
            }
        }
    }
    marks
}
