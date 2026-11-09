use super::{Flight, Piece, Stair};
use super::slab::{inside, prism};
use super::wall::loft;
use crate::Model;
use crate::parse::{Line, eval, eval_point};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::P2;
use gcad_mesh::arch::layout::{add, left, scale};
use gcad_mesh::arch::stair::{
    StairRule, Steps, check, flight_profile, hull, parse_rule, split_treads, steps,
    stringer_profile,
};
use monstertruck::modeling::*;

const WAIST: f64 = 150.0;
const COLUMN: f64 = 100.0;
const SPIRAL_TREAD: f64 = 50.0;
const STRINGER: (f64, f64, f64) = (40.0, 50.0, 300.0);
const OPEN_STRINGER: (f64, f64) = (60.0, 250.0);

struct Options {
    from: f64,
    to: f64,
    at: P2,
    dir: f64,
    shape: String,
    landing: Option<f64>,
    width: f64,
    rule: StairRule,
    rise: Option<f64>,
    tread: Option<f64>,
    risers: Option<usize>,
    stringer: Option<String>,
    nosing: f64,
    headroom: f64,
    material: String,
}

impl Flight {
    pub(crate) fn plan(&self, s: f64, u: f64) -> P2 {
        add(add(self.start, scale(self.dir, s)), scale(left(self.dir), u))
    }

    fn point(&self, s: f64, u: f64, z: f64) -> Point3 {
        let p = self.plan(s, u);
        Point3::new(p.0, p.1, self.z0 + z)
    }

    pub(crate) fn slab(&self, profile: &[P2], u0: f64, u1: f64) -> Result<Solid> {
        let ring = |u: f64| -> Vec<Vec<Point3>> {
            vec![profile.iter().map(|&(s, z)| self.point(s, u, z)).collect()]
        };
        loft(&ring(u0), &ring(u1))
    }
}

fn layout(o: &Options, steps: &Steps) -> Result<(Vec<Flight>, Vec<Piece>)> {
    let (r, t, w) = (steps.rise, steps.tread, o.width);
    let dir = (o.dir.to_radians().cos(), o.dir.to_radians().sin());
    let total = steps.treads();
    let first = |treads: usize| Flight {
        start: o.at,
        dir,
        z0: o.from,
        treads,
    };
    let deck = |ring: Vec<P2>, level: usize, kind: &'static str| Piece {
        kind,
        ring,
        top: o.from + level as f64 * r,
        depth: WAIST + r / 2.0,
    };
    let split = |turns, inside| split_treads(total, turns, inside).map_err(|e| anyhow!(e));
    Ok(match o.shape.as_str() {
        "straight" => (vec![first(total)], Vec::new()),
        "L" | "winder" => {
            let winders = if o.shape == "L" { 1 } else { 3 };
            let n = split(1, winders)?;
            let f1 = first(n[0]);
            let g = n[0] as f64 * t;
            let corner = [(g, 0.0), (g + w, 0.0), (g + w, w), (g, w)].map(|(s, u)| f1.plan(s, u));
            let pieces = if winders == 1 {
                vec![deck(corner.to_vec(), n[0] + 1, "landing")]
            } else {
                let pivot = corner[3];
                let edge = |a: f64| {
                    if a <= w {
                        f1.plan(g + a, 0.0)
                    } else {
                        f1.plan(g + w, a - w)
                    }
                };
                (0..3)
                    .map(|k| {
                        let (a, b) = (2.0 * w * k as f64 / 3.0, 2.0 * w * (k + 1) as f64 / 3.0);
                        let mut ring = vec![pivot, if k == 0 { corner[0] } else { edge(a) }];
                        if a < w && b > w {
                            ring.push(corner[1]);
                        }
                        ring.push(edge(b));
                        deck(ring, n[0] + 1 + k, "winder")
                    })
                    .collect()
            };
            let f2 = Flight {
                start: corner[2],
                dir: left(dir),
                z0: o.from + (n[0] + winders) as f64 * r,
                treads: n[1],
            };
            (vec![f1, f2], pieces)
        }
        "U" => {
            let n = split(1, 1)?;
            let f1 = first(n[0]);
            let g = n[0] as f64 * t;
            let d = o.landing.unwrap_or(w);
            let ring = [(g, 0.0), (g + d, 0.0), (g + d, 2.0 * w), (g, 2.0 * w)]
                .map(|(s, u)| f1.plan(s, u));
            let f2 = Flight {
                start: f1.plan(g, 2.0 * w),
                dir: scale(dir, -1.0),
                z0: o.from + (n[0] + 1) as f64 * r,
                treads: n[1],
            };
            (vec![f1, f2], vec![deck(ring.to_vec(), n[0] + 1, "landing")])
        }
        _ => {
            let (r0, r1) = (COLUMN / 2.0, COLUMN / 2.0 + w);
            let sweep = t / ((r0 + r1) / 2.0);
            let treads = (0..total)
                .map(|k| {
                    let a0 = o.dir.to_radians() + sweep * k as f64;
                    let at = |a: f64, rad: f64| add(o.at, (rad * a.cos(), rad * a.sin()));
                    let mut ring: Vec<P2> = (0..=6)
                        .map(|j| at(a0 + sweep * j as f64 / 6.0, r1))
                        .collect();
                    ring.push(at(a0 + sweep, r0));
                    ring.push(at(a0, r0));
                    Piece {
                        kind: "tread",
                        ring,
                        top: o.from + (k + 1) as f64 * r,
                        depth: SPIRAL_TREAD,
                    }
                })
                .collect();
            (Vec::new(), treads)
        }
    })
}

impl Model {
    fn stair_options(&self, line: &Line) -> Result<Options> {
        let mut o = Options {
            from: f64::NAN,
            to: f64::NAN,
            at: (0.0, 0.0),
            dir: 0.0,
            shape: "straight".into(),
            landing: None,
            width: 900.0,
            rule: StairRule::default(),
            rise: None,
            tread: None,
            risers: None,
            stringer: None,
            nosing: 0.0,
            headroom: 2000.0,
            material: "oak".into(),
        };
        let mut at = None;
        for (key, text) in &line.named {
            let value = || eval(text, &self.scope).with_context(|| format!("`{key}={text}`"));
            let auto = text == "auto";
            match key.as_str() {
                "from" => o.from = value()?,
                "to" => o.to = value()?,
                "at" => at = Some(eval_point(text, &self.scope)?),
                "dir" => o.dir = value()?,
                "shape" => o.shape = text.clone(),
                "landing" => o.landing = Some(value()?),
                "width" => o.width = value()?,
                "rise" if auto => o.rise = None,
                "rise" => o.rise = Some(value()?),
                "tread" if auto => o.tread = None,
                "tread" => o.tread = Some(value()?),
                "rule" => o.rule.sum = parse_rule(text).map_err(|e| anyhow!(e))?,
                "max_rise" => o.rule.max_rise = value()?,
                "min_tread" => o.rule.min_tread = value()?,
                "risers" => {
                    let n = value()?;
                    if n.fract() != 0.0 || n < 2.0 {
                        bail!("risers= is a whole number of 2 or more");
                    }
                    o.risers = Some(n as usize);
                }
                "stringer" => match text.as_str() {
                    "closed" | "open" | "mono" => o.stringer = Some(text.clone()),
                    other => bail!("a stringer is closed, open or mono, not `{other}`"),
                },
                "nosing" => o.nosing = value()?,
                "headroom" => o.headroom = value()?,
                "material" => o.material = text.clone(),
                other => bail!(
                    "`stair` takes from=, to=, at=, dir=, shape=, landing=, width=, rise=, tread=, rule=, max_rise=, min_tread=, risers=, stringer=, nosing=, headroom= and material=, not `{other}=`"
                ),
            }
        }
        if o.from.is_nan() || o.to.is_nan() {
            bail!("a stair needs `from=` and `to=`, the levels it climbs between");
        }
        o.at = at.ok_or_else(|| anyhow!("a stair needs `at=x,y`, the foot of its first flight"))?;
        if o.width <= 0.0 {
            bail!("a stair width must be above zero");
        }
        if o.nosing < 0.0 {
            bail!("nosing= cannot be negative");
        }
        if o.landing.is_some_and(|l| l <= 0.0) {
            bail!("a landing must be above zero deep");
        }
        if !["straight", "L", "U", "winder", "spiral"].contains(&o.shape.as_str()) {
            bail!("a stair is shape=straight, L, U, winder or spiral, not `{}`", o.shape);
        }
        Ok(o)
    }

    fn footprint(stair: &Stair) -> Vec<P2> {
        let mut points: Vec<P2> = stair
            .flights
            .iter()
            .flat_map(|f| {
                let going = f.treads as f64 * stair.steps.tread;
                [(0.0, 0.0), (going, 0.0), (going, stair.width), (0.0, stair.width)]
                    .map(|(s, u)| f.plan(s, u))
            })
            .collect();
        points.extend(stair.pieces.iter().flat_map(|p| p.ring.iter().copied()));
        hull(points)
    }

    fn headroom_clash(&self, stair: &Stair, headroom: f64) -> Option<String> {
        let footprint = Self::footprint(stair);
        for slab in &self.arch.slabs {
            let over = slab.holes.iter().any(|h| {
                h.iter().any(|&p| inside(p, &footprint)) || footprint.iter().any(|&p| inside(p, h))
            });
            if !over {
                continue;
            }
            let under = slab.bands().first().map_or(slab.top, |b| b.0);
            for f in &stair.flights {
                let Some(from) = stair.steps.low_headroom_from(under - f.z0, headroom) else {
                    continue;
                };
                let going = f.treads as f64 * stair.steps.tread;
                let pieces = ((going - from) / 10.0).ceil().max(1.0) as usize;
                for k in 0..=pieces {
                    let s = (from + (going - from) * k as f64 / pieces as f64).min(going - 1.0);
                    for u in [1.0, stair.width / 2.0, stair.width - 1.0] {
                        let p = f.plan(s, u);
                        if inside(p, &slab.outline) && !slab.holes.iter().any(|h| inside(p, h)) {
                            let clear = under - f.z0 - stair.steps.nosing_at(s);
                            return Some(format!(
                                "the headroom under `{}` is {:.0} at {:.0},{:.0}, less than {headroom:.0}",
                                slab.name, clear, p.0, p.1
                            ));
                        }
                    }
                }
            }
        }
        None
    }

    pub(crate) fn op_stair_cut(&mut self, line: &Line) -> Result<String> {
        let [stair, host] = line.positional.as_slice() else {
            bail!("write `stair_cut st1 s1 headroom=2000`");
        };
        let mut headroom = 2000.0;
        for (key, text) in &line.named {
            match key.as_str() {
                "headroom" => {
                    headroom = eval(text, &self.scope).with_context(|| format!("`headroom={text}`"))?
                }
                other => bail!("`stair_cut` takes headroom=, not `{other}=`"),
            }
        }
        let stair = self
            .arch
            .stairs
            .iter()
            .find(|s| s.name == *stair)
            .cloned()
            .ok_or_else(|| anyhow!("no stair `{stair}`"))?;
        let index = self
            .arch
            .slabs
            .iter()
            .position(|s| s.name == *host)
            .ok_or_else(|| anyhow!("no slab `{host}`"))?;
        let slab = self.arch.slabs[index].clone();
        let under = slab.bands().first().map_or(slab.top, |b| b.0);
        let mut points: Vec<P2> = Vec::new();
        for f in &stair.flights {
            if let Some(from) = stair.steps.low_headroom_from(under - f.z0, headroom) {
                let going = f.treads as f64 * stair.steps.tread;
                points.extend(
                    [(from, 0.0), (going, 0.0), (going, stair.width), (from, stair.width)]
                        .map(|(s, u)| f.plan(s, u)),
                );
            }
        }
        for piece in &stair.pieces {
            if piece.top + headroom > under {
                points.extend(piece.ring.iter().copied());
            }
        }
        if points.is_empty() {
            return Ok(format!(
                "stair {} has {} clear under {host} everywhere, no cut needed",
                stair.name,
                crate::parse::label(headroom)
            ));
        }
        let mut ring = hull(points);
        if ring.iter().any(|&p| !inside(p, &slab.outline)) {
            bail!("the cut for `{}` runs outside the edge of `{host}`", stair.name);
        }
        let mut holes = Vec::new();
        for h in slab.holes {
            if h.iter().any(|&p| inside(p, &ring)) || ring.iter().any(|&p| inside(p, &h)) {
                ring = hull(ring.into_iter().chain(h).collect());
            } else {
                holes.push(h);
            }
        }
        let area = gcad_mesh::arch::layout::signed_area(&ring).abs();
        holes.push(ring);
        self.arch.slabs[index].holes = holes;
        self.build_slab(host)?;
        Ok(format!(
            "stair cut in {host} for {}: {} m² open, {} headroom",
            stair.name,
            crate::parse::label(area / 1.0e6),
            crate::parse::label(headroom)
        ))
    }

    pub(crate) fn op_stair(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a stair needs a label, like `st1: stair from=L0 to=L1 at=0,0 dir=0`")
        })?;
        self.check_free_name(&name)?;
        if !line.positional.is_empty() {
            bail!("`stair` takes only named values, like `from=L0 to=L1 at=0,0`");
        }
        let o = self.stair_options(line)?;
        let height = o.to - o.from;
        let risers = o.risers.or_else(|| {
            o.rise
                .filter(|r| *r > 0.0)
                .map(|r| ((height / r - 1.0e-9).ceil() as usize).max(2))
        });
        let steps = steps(height, &o.rule, risers, o.tread).map_err(|e| anyhow!(e))?;
        check(&steps, &o.rule).map_err(|e| anyhow!(e))?;
        let (flights, pieces) = layout(&o, &steps)?;
        let stair = Stair {
            name: name.clone(),
            base: o.from,
            width: o.width,
            steps,
            flights,
            pieces,
        };
        if let Some(problem) = self.headroom_clash(&stair, o.headroom) {
            bail!("{problem}");
        }
        self.ensure_library()?;
        let mut stringers = 0;
        for (k, f) in stair.flights.iter().enumerate() {
            let profile = flight_profile(&steps, f.treads, o.nosing, WAIST);
            let solid = f
                .slab(&profile, 0.0, o.width)
                .with_context(|| format!("building `{name}`"))?;
            let body = format!("{name}.flight.{}", k + 1);
            self.put_body(body.clone(), solid, &o.material)?;
            self.note_built(&name, &body);
            let waist = steps.rise + WAIST / steps.pitch().to_radians().cos();
            let (above, depth, spans) = match o.stringer.as_deref() {
                Some("closed") => {
                    let (t, above, depth) = STRINGER;
                    (above, depth, vec![(-t, 0.0), (o.width, o.width + t)])
                }
                Some("open") => {
                    let (t, depth) = OPEN_STRINGER;
                    let inset = o.width / 6.0;
                    (
                        -waist,
                        waist + depth,
                        vec![(inset, inset + t), (o.width - inset - t, o.width - inset)],
                    )
                }
                Some(_) => {
                    let (t, depth) = OPEN_STRINGER;
                    let mid = o.width / 2.0;
                    (-waist, waist + depth, vec![(mid - t, mid + t)])
                }
                None => (0.0, 0.0, Vec::new()),
            };
            let top = o.to - f.z0;
            let profile = stringer_profile(&steps, f.treads, above, depth, top);
            for (u0, u1) in spans {
                stringers += 1;
                let solid = f
                    .slab(&profile, u0, u1)
                    .with_context(|| format!("building `{name}`"))?;
                let body = format!("{name}.stringer.{stringers}");
                self.put_body(body.clone(), solid, &o.material)?;
                self.note_built(&name, &body);
            }
        }
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for piece in &stair.pieces {
            let k = match counts.iter_mut().find(|(kind, _)| *kind == piece.kind) {
                Some(slot) => {
                    slot.1 += 1;
                    slot.1
                }
                None => {
                    counts.push((piece.kind, 1));
                    1
                }
            };
            let solid = prism(&piece.ring, &[], piece.top - piece.depth, piece.top)
                .with_context(|| format!("building `{name}`"))?;
            let body = format!("{name}.{}.{k}", piece.kind);
            self.put_body(body.clone(), solid, &o.material)?;
            self.note_built(&name, &body);
        }
        if o.shape == "spiral" {
            let ring: Vec<P2> = (0..32)
                .map(|k| {
                    let a = std::f64::consts::TAU * (k as f64 + 0.5) / 32.0;
                    add(o.at, (COLUMN / 2.0 * a.cos(), COLUMN / 2.0 * a.sin()))
                })
                .collect();
            let solid = prism(&ring, &[], o.from, o.to)
                .with_context(|| format!("building `{name}`"))?;
            let body = format!("{name}.column");
            self.put_body(body.clone(), solid, "steel")?;
            self.note_built(&name, &body);
        }
        let summary = format!(
            "stair {name}: {}{} risers of {:.1}, {} treads of {:.1}, going {:.0}, pitch {:.1}°",
            if o.shape == "straight" { String::new() } else { format!("{}, ", o.shape) },
            steps.risers,
            steps.rise,
            steps.treads(),
            steps.tread,
            steps.going(),
            steps.pitch()
        );
        self.arch.stairs.push(stair);
        Ok(summary)
    }
}
