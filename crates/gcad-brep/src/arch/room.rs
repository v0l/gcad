use super::{Room, Slab};
use crate::Model;
use crate::parse::{Line, eval, eval_point};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::P2;

const EPS: f64 = 1.0e-6;

type Segment = (P2, P2);

fn ray_hit(p: P2, dir: P2, s: Segment) -> Option<f64> {
    let (a, b) = s;
    let e = (b.0 - a.0, b.1 - a.1);
    let den = dir.0 * e.1 - dir.1 * e.0;
    if den.abs() < 1.0e-12 {
        return None;
    }
    let w = (a.0 - p.0, a.1 - p.1);
    let t = (w.0 * e.1 - w.1 * e.0) / den;
    let u = (w.0 * dir.1 - w.1 * dir.0) / den;
    (t > EPS && (-EPS..=1.0 + EPS).contains(&u)).then_some(t)
}

fn covered(segments: &[Segment], along_x: bool, at: f64, from: f64, to: f64) -> bool {
    let (c, k) = if along_x { (1, 0) } else { (0, 1) };
    let get = |p: P2, i: usize| if i == 0 { p.0 } else { p.1 };
    let mut spans: Vec<(f64, f64)> = segments
        .iter()
        .filter(|(a, b)| (get(*a, c) - at).abs() < EPS && (get(*b, c) - at).abs() < EPS)
        .map(|(a, b)| {
            let (u, v) = (get(*a, k), get(*b, k));
            (u.min(v), u.max(v))
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut reach = from;
    for (u, v) in spans {
        if u > reach + EPS {
            break;
        }
        reach = reach.max(v);
    }
    reach >= to - EPS
}

fn crosses_inside(s: Segment, rect: [f64; 4]) -> bool {
    let [x0, y0, x1, y1] = rect;
    (0..=8).any(|k| {
        let t = k as f64 / 8.0;
        let p = (s.0.0 + (s.1.0 - s.0.0) * t, s.0.1 + (s.1.1 - s.0.1) * t);
        p.0 > x0 + EPS && p.0 < x1 - EPS && p.1 > y0 + EPS && p.1 < y1 - EPS
    })
}

impl Model {
    fn wall_faces_at(&self, level: f64) -> Vec<Segment> {
        let lines = self.wall_lines();
        let mut out = Vec::new();
        for (index, wall) in self.arch.walls.iter().enumerate() {
            if !(wall.base <= level + EPS && level < wall.top - EPS) {
                continue;
            }
            let (lo, hi) = wall.outer_span();
            let (start, end) = self.end_cuts(&lines, index);
            let side = |d: f64| -> Vec<P2> {
                wall.line
                    .offset(d, start, end)
                    .into_iter()
                    .map(|(p, _)| p)
                    .collect()
            };
            let (a, b) = (side(lo), side(hi));
            for ring in [&a, &b] {
                let n = ring.len();
                let pairs = if wall.line.closed { n } else { n - 1 };
                out.extend((0..pairs).map(|k| (ring[k], ring[(k + 1) % n])));
            }
            if !wall.line.closed {
                out.push((a[0], b[0]));
                out.push((a[a.len() - 1], b[b.len() - 1]));
            }
        }
        out
    }

    pub(crate) fn op_room(&mut self, line: &Line) -> Result<String> {
        let name = match (line.label.as_ref(), line.positional.as_slice()) {
            (Some(label), []) => label.clone(),
            (None, [name]) => name.clone(),
            _ => bail!("write `room hall at=2000,3000` or `hall: room at=2000,3000`"),
        };
        if self.arch.rooms.iter().any(|r| r.name == name) {
            bail!("there is already a room `{name}`");
        }
        let mut at = None;
        let mut level = 0.0;
        for (key, text) in &line.named {
            match key.as_str() {
                "at" => at = Some(eval_point(text, &self.scope)?),
                "level" => {
                    level = eval(text, &self.scope).with_context(|| format!("`level={text}`"))?
                }
                other => bail!("`room` takes at= and level=, not `{other}=`"),
            }
        }
        let p = at.ok_or_else(|| anyhow!("a room needs `at=x,y`, a point inside it"))?;
        let faces = self.wall_faces_at(level);
        let reach = |dir: P2| {
            faces
                .iter()
                .filter_map(|s| ray_hit(p, dir, *s))
                .min_by(f64::total_cmp)
        };
        let lost = || {
            anyhow!(
                "no walls close room `{name}` around {},{}",
                crate::parse::label(p.0),
                crate::parse::label(p.1)
            )
        };
        let x1 = p.0 + reach((1.0, 0.0)).ok_or_else(lost)?;
        let x0 = p.0 - reach((-1.0, 0.0)).ok_or_else(lost)?;
        let y1 = p.1 + reach((0.0, 1.0)).ok_or_else(lost)?;
        let y0 = p.1 - reach((0.0, -1.0)).ok_or_else(lost)?;
        let rect = [x0, y0, x1, y1];
        let closed = covered(&faces, true, y0, x0, x1)
            && covered(&faces, true, y1, x0, x1)
            && covered(&faces, false, x0, y0, y1)
            && covered(&faces, false, x1, y0, y1)
            && !faces.iter().any(|s| crosses_inside(*s, rect));
        if !closed {
            bail!(
                "room `{name}` is not a rectangle of wall faces; only rectangular rooms are found for now"
            );
        }
        self.arch.rooms.push(Room {
            name: name.clone(),
            level,
            rect,
        });
        Ok(format!(
            "room {name}: {} x {}, {} m²",
            crate::parse::label(x1 - x0),
            crate::parse::label(y1 - y0),
            crate::parse::label((x1 - x0) * (y1 - y0) / 1.0e6)
        ))
    }

    pub(crate) fn op_room_cover(&mut self, line: &Line) -> Result<String> {
        let ceiling = line.op == "ceiling";
        let name = line.label.clone().ok_or_else(|| {
            anyhow!(
                "a {} needs a label, like `c1: {} room=hall type=gypsum:13`",
                line.op,
                line.op
            )
        })?;
        self.check_free_name(&name)?;
        if !line.positional.is_empty() {
            bail!("`{}` takes room=, type= and h=", line.op);
        }
        let mut room = None;
        let mut layers = None;
        let mut h = None;
        for (key, text) in &line.named {
            match key.as_str() {
                "room" => room = Some(text.clone()),
                "type" => layers = Some(super::slab::slab_layers(self, text)?),
                "h" if ceiling => {
                    h = Some(eval(text, &self.scope).with_context(|| format!("`h={text}`"))?)
                }
                other => bail!(
                    "`{}` takes room={}, type=, not `{other}=`",
                    line.op,
                    if ceiling { ", h=" } else { "" }
                ),
            }
        }
        let room = room.ok_or_else(|| anyhow!("`{}` needs `room=`, a room name", line.op))?;
        let room = self
            .arch
            .rooms
            .iter()
            .find(|r| r.name == room)
            .cloned()
            .ok_or_else(|| anyhow!("no room `{room}`"))?;
        let layers = layers.ok_or_else(|| {
            anyhow!(
                "`{}` needs `type=`, layers bottom to top like `gypsum:13`",
                line.op
            )
        })?;
        let total: f64 = layers.iter().map(|l| l.thickness).sum();
        let bottom = match (ceiling, h) {
            (true, Some(h)) => room.level + h,
            (true, None) => bail!("a ceiling needs `h=`, its height above the room's level"),
            (false, _) => room.level,
        };
        let [x0, y0, x1, y1] = room.rect;
        self.arch.slabs.push(Slab {
            name: name.clone(),
            outline: vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)],
            holes: Vec::new(),
            layers,
            top: bottom + total,
            slope: 0.0,
            fall: None,
        });
        self.build_slab(&name)?;
        Ok(format!(
            "{} {name} in {}, {} thick at {}",
            line.op.replace('_', " "),
            room.name,
            crate::parse::label(total),
            crate::parse::label(bottom)
        ))
    }
}
