use crate::Model;
use crate::member::straight_member;
use crate::parse::{Line, eval};
use anyhow::{Context, Result, anyhow, bail};
use monstertruck::modeling::*;

struct Upright {
    kind: String,
    size: String,
    base: f64,
    top: f64,
    material: String,
}

fn spans_across(outline: &[(f64, f64)], at: f64) -> Vec<(f64, f64)> {
    let n = outline.len();
    let mut cuts: Vec<f64> = (0..n)
        .filter_map(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % n]);
            ((a.0 > at) != (b.0 > at)).then(|| a.1 + (b.1 - a.1) * (at - a.0) / (b.0 - a.0))
        })
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

fn truss_members(
    kind: &str,
    l: f64,
    h: f64,
) -> Result<Vec<(&'static str, (f64, f64), (f64, f64))>> {
    let rafter = |u: f64| h * (1.0 - (2.0 * u / l - 1.0).abs());
    let mut members = vec![
        ("chord", (0.0, 0.0), (l, 0.0)),
        ("rafter", (0.0, 0.0), (l / 2.0, h)),
        ("rafter", (l, 0.0), (l / 2.0, h)),
    ];
    match kind {
        "king" => members.push(("post", (l / 2.0, 0.0), (l / 2.0, h))),
        "queen" => {
            let (a, b) = (l / 3.0, 2.0 * l / 3.0);
            members.push(("post", (a, 0.0), (a, rafter(a))));
            members.push(("post", (b, 0.0), (b, rafter(b))));
            members.push(("strut", (a, rafter(a)), (b, rafter(b))));
        }
        "fink" => {
            let (a, b) = (l / 3.0, 2.0 * l / 3.0);
            let (q, r) = (l / 4.0, 3.0 * l / 4.0);
            members.push(("web", (q, rafter(q)), (a, 0.0)));
            members.push(("web", (a, 0.0), (l / 2.0, h)));
            members.push(("web", (l / 2.0, h), (b, 0.0)));
            members.push(("web", (b, 0.0), (r, rafter(r))));
        }
        other => bail!("a truss is type=king, queen or fink, not `{other}`"),
    }
    Ok(members)
}

fn numbered(name: &str, part: &str, used: &mut Vec<String>) -> String {
    let mut k = 1;
    loop {
        let candidate = format!("{name}.{part}.{k}");
        if !used.contains(&candidate) {
            used.push(candidate.clone());
            return candidate;
        }
        k += 1;
    }
}

impl Model {
    fn framing_named(
        &self,
        line: &Line,
        op: &str,
        allowed: &[&str],
    ) -> Result<Vec<(String, String)>> {
        let mut out = Vec::new();
        for (key, text) in &line.named {
            if !allowed.contains(&key.as_str()) && key != "material" {
                bail!(
                    "`{op}` takes {} and material=, not `{key}=`",
                    allowed
                        .iter()
                        .map(|k| format!("{k}="))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            out.push((key.clone(), text.clone()));
        }
        Ok(out)
    }

    fn number_of(&self, named: &[(String, String)], key: &str) -> Result<Option<f64>> {
        named
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, text)| eval(text, &self.scope).with_context(|| format!("`{key}={text}`")))
            .transpose()
    }

    fn material_of(named: &[(String, String)]) -> String {
        named
            .iter()
            .find(|(k, _)| k == "material")
            .map_or("c24".to_string(), |(_, v)| v.clone())
    }

    pub(crate) fn op_beam(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a beam needs a label, like `b1: beam A1 B1 bar 200x400 z=L1`")
        })?;
        self.check_free_name(&name)?;
        let [a, b, kind, size] = line.positional.as_slice() else {
            bail!("write `b1: beam A1 B1 bar 200x400 z=L1`, the top of the beam on z");
        };
        let named = self.framing_named(line, "beam", &["z"])?;
        let z = self
            .number_of(&named, "z")?
            .ok_or_else(|| anyhow!("a beam needs `z=`, the height of its top"))?;
        let (p, q) = (self.plan_point(a)?, self.plan_point(b)?);
        let solid = straight_member(
            kind,
            size,
            Point3::new(p.0, p.1, z),
            Point3::new(q.0, q.1, z),
            Vector3::unit_z(),
            true,
        )
        .with_context(|| format!("building `{name}`"))?;
        self.ensure_library()?;
        self.put_body(name.clone(), solid, &Self::material_of(&named))?;
        self.note_built(&name, &name);
        Ok(format!(
            "beam {name}: {kind} {size}, {} long, top at {}",
            crate::parse::label((q.0 - p.0).hypot(q.1 - p.1)),
            crate::parse::label(z)
        ))
    }

    pub(crate) fn op_joist(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!(
                "joists need a label, like `j: joist A1 B1 B2 A2 bar 47x200 spacing=400 dir=y z=L1`"
            )
        })?;
        self.check_free_name(&name)?;
        let words = &line.positional;
        if words.len() < 5 {
            bail!("write `j: joist A1 B1 B2 A2 bar 47x200 spacing=400 dir=y z=L1`");
        }
        let (points, section) = words.split_at(words.len() - 2);
        let (kind, size) = (&section[0], &section[1]);
        let outline = self.plan_ring(points)?;
        let named = self.framing_named(line, "joist", &["spacing", "dir", "z"])?;
        let z = self
            .number_of(&named, "z")?
            .ok_or_else(|| anyhow!("joists need `z=`, the height of their tops"))?;
        let spacing = self
            .number_of(&named, "spacing")?
            .ok_or_else(|| anyhow!("joists need `spacing=`, centre to centre"))?;
        if spacing <= 0.0 {
            bail!("joist spacing must be above zero");
        }
        let along_y = match named
            .iter()
            .find(|(k, _)| k == "dir")
            .map(|(_, v)| v.as_str())
        {
            Some("y") | None => true,
            Some("x") => false,
            Some(other) => bail!("joists run dir=x or dir=y, not `{other}`"),
        };
        let width = size
            .split(['x', 'X'])
            .next()
            .and_then(|w| w.parse::<f64>().ok())
            .filter(|w| *w > 0.0)
            .ok_or_else(|| anyhow!("`{size}` is not a joist size like 47x200"))?;
        let swap = |p: (f64, f64)| if along_y { p } else { (p.1, p.0) };
        let local: Vec<(f64, f64)> = outline.iter().map(|&p| swap(p)).collect();
        let lo = local.iter().map(|p| p.0).fold(f64::INFINITY, f64::min) + width / 2.0;
        let hi = local.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max) - width / 2.0;
        if hi < lo {
            bail!("the outline is narrower than one joist");
        }
        let mut stations: Vec<f64> = (0..)
            .map(|k| lo + spacing * k as f64)
            .take_while(|s| *s < hi - 1.0e-6)
            .collect();
        stations.push(hi);
        let material = Self::material_of(&named);
        self.ensure_library()?;
        let mut made = 0;
        for s in stations {
            for (a, b) in spans_across(&local, s) {
                made += 1;
                let (p, q) = (swap((s, a)), swap((s, b)));
                let solid = straight_member(
                    kind,
                    size,
                    Point3::new(p.0, p.1, z),
                    Point3::new(q.0, q.1, z),
                    Vector3::unit_z(),
                    true,
                )
                .with_context(|| format!("building `{name}`"))?;
                let body = format!("{name}.{made}");
                self.put_body(body.clone(), solid, &material)?;
                self.note_built(&name, &body);
            }
        }
        Ok(format!(
            "joists {name}: {made} of {kind} {size} at {} centres, tops at {}",
            crate::parse::label(spacing),
            crate::parse::label(z)
        ))
    }

    pub(crate) fn op_truss(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a truss needs a label, like `t: truss 0,0,2700 8000,0,2700 type=fink`")
        })?;
        self.check_free_name(&name)?;
        let [a, b, size] = line.positional.as_slice() else {
            bail!(
                "write `t: truss x,y,z x,y,z type=king|queen|fink pitch=30 member=bar 47x150 spacing=600 count=10`"
            );
        };
        let named = self.framing_named(
            line,
            "truss",
            &["type", "pitch", "member", "spacing", "count"],
        )?;
        let text = |key: &str| named.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        let kind = text("type").unwrap_or_else(|| "fink".into());
        let section = text("member").unwrap_or_else(|| "bar".into());
        let pitch = self.number_of(&named, "pitch")?.unwrap_or(30.0);
        if !(pitch > 0.0 && pitch < 80.0) {
            bail!("a truss pitch is degrees between 0 and 80");
        }
        let count = self.number_of(&named, "count")?.unwrap_or(1.0);
        if count < 1.0 || count.fract() != 0.0 {
            bail!("count must be a positive whole number");
        }
        let spacing = self.number_of(&named, "spacing")?.unwrap_or(600.0);
        if count > 1.0 && spacing <= 0.0 {
            bail!("truss spacing must be above zero");
        }
        let (p, q) = (
            crate::args::point3(a, &self.scope)?,
            crate::args::point3(b, &self.scope)?,
        );
        if (p.z - q.z).abs() > 1.0e-6 {
            bail!("a truss bears on two points at the same height");
        }
        let span = q - p;
        let l = span.magnitude();
        if l < 1.0e-6 {
            bail!("a truss needs two different bearing points");
        }
        let s = span.normalize();
        let across = Vector3::unit_z().cross(s);
        let h = l / 2.0 * pitch.to_radians().tan();
        let members = truss_members(&kind, l, h)?;
        let material = Self::material_of(&named);
        self.ensure_library()?;
        for k in 0..count as usize {
            let origin = p + across * (spacing * k as f64);
            let at = |(u, v): (f64, f64)| origin + s * u + Vector3::unit_z() * v;
            let mut used = Vec::new();
            for (part, from, to) in &members {
                let (from, to) = (at(*from), at(*to));
                let up = across.cross((to - from).normalize());
                let solid = straight_member(&section, size, from, to, up, false)
                    .with_context(|| format!("building `{name}`"))?;
                let body = numbered(&format!("{name}.{}", k + 1), part, &mut used);
                self.put_body(body.clone(), solid, &material)?;
                self.note_built(&name, &body);
            }
        }
        Ok(format!(
            "truss {name}: {count} {kind} trusses of {} members, {} span, {} rise",
            members.len(),
            crate::parse::label(l),
            crate::parse::label(h)
        ))
    }

    fn upright(&self, line: &Line, words: &[String], op: &str) -> Result<Upright> {
        let (kind, size) = match words {
            [kind, size] => (kind.clone(), size.clone()),
            _ => bail!(
                "write `{op} rect 300x300`, `round 300` or a member section like `tube 100x100x5`"
            ),
        };
        let mut upright = Upright {
            kind,
            size,
            base: 0.0,
            top: f64::NAN,
            material: "concrete".into(),
        };
        for (key, text) in &line.named {
            let value = || eval(text, &self.scope).with_context(|| format!("`{key}={text}`"));
            match key.as_str() {
                "base" => upright.base = value()?,
                "top" => upright.top = value()?,
                "material" => upright.material = text.clone(),
                other => bail!("`{op}` takes base=, top= and material=, not `{other}=`"),
            }
        }
        if upright.top.is_nan() {
            bail!("a column needs `top=`, a height or a level like `top=L1`");
        }
        if upright.top <= upright.base {
            bail!(
                "the column's top {} is not above its base {}",
                upright.top,
                upright.base
            );
        }
        Ok(upright)
    }

    fn put_upright(&mut self, owner: &str, name: &str, at: (f64, f64), u: &Upright) -> Result<()> {
        let solid = straight_member(
            &u.kind,
            &u.size,
            Point3::new(at.0, at.1, u.base),
            Point3::new(at.0, at.1, u.top),
            Vector3::unit_y(),
            false,
        )
        .with_context(|| format!("building `{name}`"))?;
        self.put_body(name.to_string(), solid, &u.material)?;
        self.note_built(owner, name);
        Ok(())
    }

    pub(crate) fn op_column(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a column needs a label, like `c1: column A1 rect 300x300 top=L1`")
        })?;
        self.check_free_name(&name)?;
        let [at, section @ ..] = line.positional.as_slice() else {
            bail!("write `c1: column A1 rect 300x300 base=L0 top=L1`");
        };
        let point = self.plan_point(at)?;
        let upright = self.upright(line, section, "column")?;
        self.ensure_library()?;
        self.put_upright(&name, &name, point, &upright)?;
        Ok(format!(
            "column {name} at {at}: {} {}, {} high",
            upright.kind,
            upright.size,
            crate::parse::label(upright.top - upright.base)
        ))
    }

    pub(crate) fn op_column_grid(&mut self, line: &Line) -> Result<String> {
        let name = line.label.clone().ok_or_else(|| {
            anyhow!("a column grid needs a label, like `cg: column_grid rect 200x200 top=L1`")
        })?;
        self.check_free_name(&name)?;
        let upright = self.upright(line, &line.positional, "column_grid")?;
        let crossings: Vec<(String, (f64, f64))> = self
            .arch
            .grids
            .iter()
            .filter(|g| g.along_x)
            .flat_map(|x| {
                self.arch
                    .grids
                    .iter()
                    .filter(|g| !g.along_x)
                    .map(move |y| (format!("{}{}", x.name, y.name), (x.offset, y.offset)))
            })
            .collect();
        if crossings.is_empty() {
            bail!("a column grid needs x and y `grid` lines to stand on");
        }
        self.ensure_library()?;
        for (crossing, at) in &crossings {
            self.put_upright(&name, &format!("{name}.{crossing}"), *at, &upright)?;
        }
        Ok(format!(
            "column grid {name}: {} columns, {} {}",
            crossings.len(),
            upright.kind,
            upright.size
        ))
    }
}
