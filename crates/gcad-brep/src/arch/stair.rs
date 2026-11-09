use super::{Flight, Stair};
use super::slab::inside;
use super::wall::loft;
use crate::Model;
use crate::parse::{Line, eval, eval_point};
use anyhow::{Context, Result, anyhow, bail};
use gcad_mesh::arch::P2;
use gcad_mesh::arch::layout::{add, left, scale};
use gcad_mesh::arch::stair::{
    StairRule, check, flight_profile, parse_rule, steps, stringer_profile,
};
use monstertruck::modeling::*;

const WAIST: f64 = 150.0;
const STRINGER: (f64, f64, f64) = (40.0, 50.0, 300.0);
const OPEN_STRINGER: (f64, f64) = (60.0, 250.0);

struct Options {
    from: f64,
    to: f64,
    at: P2,
    dir: f64,
    shape: String,
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

    fn slab(&self, profile: &[P2], u0: f64, u1: f64) -> Result<Solid> {
        let ring = |u: f64| -> Vec<Vec<Point3>> {
            vec![profile.iter().map(|&(s, z)| self.point(s, u, z)).collect()]
        };
        loft(&ring(u0), &ring(u1))
    }
}

impl Model {
    fn stair_options(&self, line: &Line) -> Result<Options> {
        let mut o = Options {
            from: f64::NAN,
            to: f64::NAN,
            at: (0.0, 0.0),
            dir: 0.0,
            shape: "straight".into(),
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
                    "`stair` takes from=, to=, at=, dir=, shape=, width=, rise=, tread=, rule=, max_rise=, min_tread=, risers=, stringer=, nosing=, headroom= and material=, not `{other}=`"
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
        if o.shape != "straight" {
            bail!("stair shapes other than straight are not built yet, not `{}`", o.shape);
        }
        Ok(o)
    }

    fn headroom_clash(&self, stair: &Stair, headroom: f64) -> Option<String> {
        let footprint: Vec<P2> = stair
            .flights
            .iter()
            .flat_map(|f| {
                let going = f.treads as f64 * stair.steps.tread;
                [(0.0, 0.0), (going, 0.0), (going, stair.width), (0.0, stair.width)]
                    .map(|(s, u)| f.plan(s, u))
            })
            .collect();
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
        let dir = (o.dir.to_radians().cos(), o.dir.to_radians().sin());
        let stair = Stair {
            name: name.clone(),
            base: o.from,
            width: o.width,
            steps,
            flights: vec![Flight {
                start: o.at,
                dir,
                z0: o.from,
                treads: steps.treads(),
            }],
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
        let summary = format!(
            "stair {name}: {} risers of {:.1}, {} treads of {:.1}, going {:.0}, pitch {:.1}°",
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
