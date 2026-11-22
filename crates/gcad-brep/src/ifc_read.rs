use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::Point3;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Null,
    Derived,
    Ref(usize),
    Num(f64),
    Text(String),
    Enum(String),
    List(Vec<Arg>),
    Typed(String, Vec<Arg>),
}

impl Arg {
    pub fn text(&self) -> Option<&str> {
        match self {
            Arg::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Arg::Num(n) => Some(*n),
            Arg::Typed(_, inner) => inner.first().and_then(Arg::num),
            _ => None,
        }
    }

    pub fn id(&self) -> Option<usize> {
        match self {
            Arg::Ref(id) => Some(*id),
            _ => None,
        }
    }

    pub fn items(&self) -> &[Arg] {
        match self {
            Arg::List(items) => items,
            _ => &[],
        }
    }

    pub fn refs(&self, out: &mut Vec<usize>) {
        match self {
            Arg::Ref(id) => out.push(*id),
            Arg::List(items) | Arg::Typed(_, items) => items.iter().for_each(|a| a.refs(out)),
            _ => {}
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entity {
    pub class: String,
    pub args: Vec<Arg>,
}

#[derive(Debug, Default)]
pub struct Ifc {
    pub schema: String,
    pub entities: BTreeMap<usize, Entity>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn skip(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn word(&mut self) -> String {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            self.at += 1;
        }
        String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned()
    }

    fn arg(&mut self) -> Result<Arg> {
        self.skip();
        match self.peek().ok_or_else(|| anyhow!("the file ends inside an entity"))? {
            b'$' => {
                self.at += 1;
                Ok(Arg::Null)
            }
            b'*' => {
                self.at += 1;
                Ok(Arg::Derived)
            }
            b'#' => {
                self.at += 1;
                let start = self.at;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.at += 1;
                }
                let digits = std::str::from_utf8(&self.bytes[start..self.at])?;
                Ok(Arg::Ref(digits.parse()?))
            }
            b'\'' => {
                self.at += 1;
                let mut out = Vec::new();
                loop {
                    match self.peek().ok_or_else(|| anyhow!("an unterminated string"))? {
                        b'\'' if self.bytes.get(self.at + 1) == Some(&b'\'') => {
                            out.push(b'\'');
                            self.at += 2;
                        }
                        b'\'' => {
                            self.at += 1;
                            break;
                        }
                        c => {
                            out.push(c);
                            self.at += 1;
                        }
                    }
                }
                Ok(Arg::Text(decode(&String::from_utf8_lossy(&out))))
            }
            b'.' => {
                self.at += 1;
                let name = self.word();
                if self.peek() != Some(b'.') {
                    bail!("an enumeration `.{name}` is not closed");
                }
                self.at += 1;
                Ok(Arg::Enum(name))
            }
            b'(' => {
                self.at += 1;
                Ok(Arg::List(self.list()?))
            }
            c if c.is_ascii_digit() || c == b'-' || c == b'+' => {
                let start = self.at;
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'E' | b'e'))
                {
                    self.at += 1;
                }
                let text = std::str::from_utf8(&self.bytes[start..self.at])?;
                Ok(Arg::Num(
                    text.parse().map_err(|_| anyhow!("`{text}` is not a number"))?,
                ))
            }
            c if c.is_ascii_alphabetic() => {
                let name = self.word();
                self.skip();
                if self.peek() != Some(b'(') {
                    bail!("`{name}` is not a value");
                }
                self.at += 1;
                Ok(Arg::Typed(name.to_ascii_uppercase(), self.list()?))
            }
            other => bail!("unexpected `{}` in an entity", other as char),
        }
    }

    fn list(&mut self) -> Result<Vec<Arg>> {
        let mut items = Vec::new();
        loop {
            self.skip();
            match self.peek() {
                Some(b')') => {
                    self.at += 1;
                    return Ok(items);
                }
                Some(b',') => self.at += 1,
                Some(_) => items.push(self.arg()?),
                None => bail!("a list is not closed"),
            }
        }
    }
}

fn decode(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("\\X2\\") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 4..];
        let end = tail.find("\\X0\\").unwrap_or(tail.len());
        let units: Vec<u16> = tail[..end]
            .as_bytes()
            .chunks(4)
            .filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
            .collect();
        out.push_str(&String::from_utf16_lossy(&units));
        rest = tail.get(end + 4..).unwrap_or("");
    }
    out.push_str(rest);
    out.replace("\\\\", "\\")
}

fn statements(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (at, c) in text.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            ';' if !quoted => {
                out.push(text[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    out
}

impl Ifc {
    pub fn parse(text: &str) -> Result<Ifc> {
        let text = strip_comments(text);
        let statements = statements(&text);
        if statements.first().map(|s| s.trim()) != Some("ISO-10303-21") {
            bail!("not a STEP physical file: it should start with `ISO-10303-21;`");
        }
        if !statements.iter().any(|s| s.trim() == "END-ISO-10303-21") {
            bail!("the file does not end with `END-ISO-10303-21;`");
        }
        let mut ifc = Ifc::default();
        let mut data = false;
        for statement in statements {
            let statement = statement.trim();
            match statement {
                "DATA" => data = true,
                "ENDSEC" => data = false,
                _ if statement.starts_with("FILE_SCHEMA") => {
                    ifc.schema = statement
                        .split('\'')
                        .nth(1)
                        .unwrap_or_default()
                        .to_string();
                }
                _ if data && statement.starts_with('#') => {
                    let (id, body) = statement
                        .split_once('=')
                        .ok_or_else(|| anyhow!("`{statement}` is not an entity"))?;
                    let id: usize = id.trim()[1..]
                        .parse()
                        .map_err(|_| anyhow!("`{id}` is not an entity id"))?;
                    let body = body.trim();
                    let open = body
                        .find('(')
                        .ok_or_else(|| anyhow!("#{id} has no arguments"))?;
                    let mut cursor = Cursor {
                        bytes: body.as_bytes(),
                        at: open + 1,
                    };
                    let args = cursor.list().map_err(|e| anyhow!("#{id}: {e}"))?;
                    if ifc
                        .entities
                        .insert(
                            id,
                            Entity {
                                class: body[..open].trim().to_ascii_uppercase(),
                                args,
                            },
                        )
                        .is_some()
                    {
                        bail!("#{id} is defined twice");
                    }
                }
                _ => {}
            }
        }
        Ok(ifc)
    }

    pub fn of(&self, class: &str) -> impl Iterator<Item = (usize, &Entity)> {
        let class = class.to_ascii_uppercase();
        self.entities
            .iter()
            .filter(move |(_, e)| e.class == class)
            .map(|(id, e)| (*id, e))
    }

    pub fn get(&self, id: usize) -> Option<&Entity> {
        self.entities.get(&id)
    }

    fn point(&self, id: usize) -> Option<[f64; 3]> {
        let e = self.get(id)?;
        let c = e.args.first()?.items();
        Some([
            c.first()?.num()?,
            c.get(1)?.num()?,
            c.get(2).and_then(Arg::num).unwrap_or(0.0),
        ])
    }

    fn triangulated(&self, e: &Entity) -> Option<Vec<[[f64; 3]; 3]>> {
        let list = self.get(e.args.first()?.id()?)?;
        let points: Vec<[f64; 3]> = list
            .args
            .first()?
            .items()
            .iter()
            .map(|p| {
                let c = p.items();
                Some([c.first()?.num()?, c.get(1)?.num()?, c.get(2)?.num()?])
            })
            .collect::<Option<_>>()?;
        e.args
            .get(3)?
            .items()
            .iter()
            .map(|t| {
                let i = t.items();
                let at = |k: usize| points.get(i.get(k)?.num()? as usize - 1).copied();
                Some([at(0)?, at(1)?, at(2)?])
            })
            .collect()
    }

    fn polygonal(&self, e: &Entity) -> Option<Vec<[[f64; 3]; 3]>> {
        let list = self.get(e.args.first()?.id()?)?;
        let points: Vec<[f64; 3]> = list
            .args
            .first()?
            .items()
            .iter()
            .map(|p| {
                let c = p.items();
                Some([c.first()?.num()?, c.get(1)?.num()?, c.get(2)?.num()?])
            })
            .collect::<Option<_>>()?;
        let mut out = Vec::new();
        for face in e.args.get(2)?.items() {
            let face = self.get(face.id()?)?;
            let ring: Vec<[f64; 3]> = face
                .args
                .first()?
                .items()
                .iter()
                .map(|i| points.get(i.num()? as usize - 1).copied())
                .collect::<Option<_>>()?;
            for k in 1..ring.len().saturating_sub(1) {
                out.push([ring[0], ring[k], ring[k + 1]]);
            }
        }
        Some(out)
    }

    fn faceted(&self, e: &Entity) -> Option<Vec<[[f64; 3]; 3]>> {
        let shell = self.get(e.args.first()?.id()?)?;
        let mut out = Vec::new();
        for face in shell.args.first()?.items() {
            for bound in self.get(face.id()?)?.args.first()?.items() {
                let bound = self.get(bound.id()?)?;
                if bound.class != "IFCFACEOUTERBOUND" {
                    continue;
                }
                let flip = matches!(bound.args.get(1), Some(Arg::Enum(f)) if f == "F");
                let lp = self.get(bound.args.first()?.id()?)?;
                let mut ring: Vec<[f64; 3]> = lp
                    .args
                    .first()?
                    .items()
                    .iter()
                    .map(|p| self.point(p.id()?))
                    .collect::<Option<_>>()?;
                if flip {
                    ring.reverse();
                }
                for k in 1..ring.len().saturating_sub(1) {
                    out.push([ring[0], ring[k], ring[k + 1]]);
                }
            }
        }
        Some(out)
    }

    pub fn items_of(&self, element: &Entity) -> Vec<Vec<[[f64; 3]; 3]>> {
        let Some(definition) = element
            .args
            .get(6)
            .and_then(Arg::id)
            .and_then(|id| self.get(id))
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for shape in definition.args.get(2).map_or(&[][..], Arg::items) {
            let Some(shape) = shape.id().and_then(|id| self.get(id)) else {
                continue;
            };
            for item in shape.args.get(3).map_or(&[][..], Arg::items) {
                let Some(item) = item.id().and_then(|id| self.get(id)) else {
                    continue;
                };
                let triangles = match item.class.as_str() {
                    "IFCTRIANGULATEDFACESET" => self.triangulated(item),
                    "IFCFACETEDBREP" => self.faceted(item),
                    "IFCPOLYGONALFACESET" => self.polygonal(item),
                    _ => None,
                };
                out.extend(triangles);
            }
        }
        out
    }

    pub fn elements(&self) -> Vec<(usize, &Entity)> {
        self.entities
            .iter()
            .filter(|(_, e)| {
                matches!(
                    e.class.as_str(),
                    "IFCWALL"
                        | "IFCSLAB"
                        | "IFCSPACE"
                        | "IFCROOF"
                        | "IFCSTAIR"
                        | "IFCSTAIRFLIGHT"
                        | "IFCRAILING"
                        | "IFCCOLUMN"
                        | "IFCBEAM"
                        | "IFCCURTAINWALL"
                        | "IFCDOOR"
                        | "IFCWINDOW"
                        | "IFCFOOTING"
                        | "IFCCOVERING"
                        | "IFCRAMP"
                        | "IFCBUILDINGELEMENTPROXY"
                        | "IFCFURNITURE"
                        | "IFCFURNISHINGELEMENT"
                        | "IFCSANITARYTERMINAL"
                        | "IFCFLOWTERMINAL"
                        | "IFCDISTRIBUTIONELEMENT"
                        | "IFCOPENINGELEMENT"
                )
            })
            .map(|(id, e)| (*id, e))
            .collect()
    }

    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.schema != "IFC4" {
            problems.push(format!("the schema is `{}`, not IFC4", self.schema));
        }
        for (id, e) in &self.entities {
            let mut refs = Vec::new();
            e.args.iter().for_each(|a| a.refs(&mut refs));
            for r in refs {
                if !self.entities.contains_key(&r) {
                    problems.push(format!("#{id} {} refers to #{r}, which is not defined", e.class));
                }
            }
        }
        let mut contained = std::collections::BTreeSet::new();
        for (_, rel) in self.of("IFCRELCONTAINEDINSPATIALSTRUCTURE") {
            let mut refs = Vec::new();
            if let Some(list) = rel.args.get(4) {
                list.refs(&mut refs);
            }
            contained.extend(refs);
        }
        for (_, rel) in self.of("IFCRELAGGREGATES") {
            let mut refs = Vec::new();
            if let Some(list) = rel.args.get(5) {
                list.refs(&mut refs);
            }
            contained.extend(refs);
        }
        let mut voids = std::collections::BTreeMap::new();
        for (_, rel) in self.of("IFCRELVOIDSELEMENT") {
            if let (Some(host), Some(hole)) = (
                rel.args.get(4).and_then(Arg::id),
                rel.args.get(5).and_then(Arg::id),
            ) {
                voids.insert(hole, host);
            }
        }
        let mut fills = std::collections::BTreeSet::new();
        for (_, rel) in self.of("IFCRELFILLSELEMENT") {
            if let Some(filler) = rel.args.get(5).and_then(Arg::id) {
                fills.insert(filler);
            }
        }
        for (id, e) in self.elements() {
            let name = e.args.get(2).and_then(Arg::text).unwrap_or("");
            match e.class.as_str() {
                "IFCOPENINGELEMENT" => {
                    if !voids.contains_key(&id) {
                        problems.push(format!("opening #{id} `{name}` voids no host"));
                    }
                }
                "IFCDOOR" | "IFCWINDOW" => {
                    if !fills.contains(&id) {
                        problems.push(format!("{} #{id} `{name}` fills no opening", e.class));
                    }
                }
                _ => {
                    if !contained.contains(&id) {
                        problems.push(format!("{} #{id} `{name}` is in no storey", e.class));
                    }
                }
            }
        }
        problems
    }
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("/*") {
        out.push_str(&rest[..at]);
        rest = rest[at..].split_once("*/").map_or("", |(_, tail)| tail);
    }
    out.push_str(rest);
    out
}

pub fn point3(p: [f64; 3]) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}

impl Ifc {
    pub fn millimetres(&self) -> f64 {
        self.of("IFCSIUNIT")
            .find(|(_, e)| matches!(e.args.get(1), Some(Arg::Enum(u)) if u == "LENGTHUNIT"))
            .map_or(1.0, |(_, e)| match e.args.get(2) {
                Some(Arg::Enum(p)) if p == "MILLI" => 1.0,
                Some(Arg::Enum(p)) if p == "CENTI" => 10.0,
                _ => 1000.0,
            })
    }
}

fn footprint(
    name: &str,
    class: &str,
    triangles: &[[[f64; 3]; 3]],
    scale: f64,
) -> Option<crate::arch::Reference> {
    let key = |p: [f64; 3]| ((p[0] * scale).round() as i64, (p[1] * scale).round() as i64);
    let mut edges: std::collections::BTreeMap<((i64, i64), (i64, i64)), usize> = Default::default();
    let mut z = f64::MAX;
    for t in triangles {
        for p in t {
            z = z.min(p[2] * scale);
        }
        let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
        let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
        let nz = u[0] * v[1] - u[1] * v[0];
        let normal =
            (u[1] * v[2] - u[2] * v[1]).powi(2) + (u[2] * v[0] - u[0] * v[2]).powi(2) + nz * nz;
        if normal <= 0.0 || nz <= 0.0 || nz * nz < 0.9 * normal {
            continue;
        }
        for k in 0..3 {
            let (a, b) = (key(t[k]), key(t[(k + 1) % 3]));
            if a == b {
                continue;
            }
            let ordered = if a < b { (a, b) } else { (b, a) };
            *edges.entry(ordered).or_default() += 1;
        }
    }
    let rings: Vec<Vec<(f64, f64)>> = edges
        .into_iter()
        .filter(|(_, n)| *n == 1)
        .map(|((a, b), _)| vec![(a.0 as f64, a.1 as f64), (b.0 as f64, b.1 as f64)])
        .collect();
    (!rings.is_empty()).then(|| crate::arch::Reference {
        name: name.to_string(),
        class: class.to_ascii_lowercase(),
        z,
        rings,
    })
}

impl crate::Model {
    pub(crate) fn import_ifc(
        &mut self,
        line: &crate::parse::Line,
        path: &std::path::Path,
    ) -> Result<String> {
        let text = crate::host::read_to_string(path)
            .map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
        let ifc = Ifc::parse(&text).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        let scale = ifc.millimetres();
        let prefix = crate::label_of(line);
        let mut made = 0;
        let mut skipped = 0;
        for (id, element) in ifc.elements() {
            if element.class == "IFCOPENINGELEMENT" {
                continue;
            }
            if element.class == "IFCSPACE" {
                for (_, triangles) in ifc.items_of(element).into_iter().enumerate() {
                    if let Some(reference) = footprint(
                        &element.args.get(2).and_then(Arg::text).unwrap_or("space").to_string(),
                        &element.class,
                        &triangles,
                        scale,
                    ) {
                        self.arch.references.push(reference);
                    }
                }
                continue;
            }
            let name = element
                .args
                .get(2)
                .and_then(Arg::text)
                .filter(|n| !n.is_empty())
                .map_or_else(|| format!("{}{id}", element.class.to_lowercase()), str::to_string);
            for (k, triangles) in ifc.items_of(element).into_iter().enumerate() {
                if matches!(element.class.as_str(), "IFCWALL" | "IFCSLAB" | "IFCSPACE") {
                    if let Some(reference) = footprint(&name, &element.class, &triangles, scale) {
                        self.arch.references.push(reference);
                    }
                }
                let faces: Vec<Vec<Vec<Point3>>> = triangles
                    .iter()
                    .map(|t| vec![t.iter().map(|p| point3(p.map(|c| c * scale))).collect()])
                    .collect();
                let label = if k == 0 {
                    format!("{prefix}.{name}")
                } else {
                    format!("{prefix}.{name}.{}", k + 1)
                };
                match crate::import::polygon_solid(&faces) {
                    Ok(solid) => {
                        self.bodies.push((label, solid));
                        made += 1;
                    }
                    Err(_) => skipped += 1,
                }
            }
        }
        if made == 0 {
            bail!("{} holds no geometry gcad can read as a solid", path.display());
        }
        let skipped = if skipped > 0 {
            format!(", {skipped} bodies are not closed and were left out")
        } else {
            String::new()
        };
        Ok(format!("imported {made} bodies from {}{skipped}", path.display()))
    }
}
