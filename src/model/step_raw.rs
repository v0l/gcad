use monstertruck::modeling::*;
use std::collections::HashMap;

pub(crate) struct Raw {
    records: HashMap<u64, String>,
    containers: HashMap<u64, u64>,
}

fn records_of(text: &str) -> HashMap<u64, String> {
    let data = text.find("DATA;").map_or(text, |at| &text[at + 5..]);
    let mut out = HashMap::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in data.chars() {
        match c {
            '\'' => {
                quoted = !quoted;
                current.push(c);
            }
            ';' if !quoted => {
                let record = current.trim();
                if let Some(rest) = record.strip_prefix('#')
                    && let Some((id, body)) = rest.split_once('=')
                    && let Ok(id) = id.trim().parse::<u64>()
                {
                    out.insert(id, body.trim().to_string());
                }
                current.clear();
            }
            c if c.is_whitespace() && !quoted => {}
            _ => current.push(c),
        }
    }
    out
}

fn refs(text: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut k = 0;
    while k < bytes.len() {
        if bytes[k] == b'#' {
            let start = k + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if let Ok(id) = text[start..end].parse() {
                out.push(id);
            }
            k = end;
        } else {
            k += 1;
        }
    }
    out
}

fn args_of<'a>(record: &'a str, name: &str) -> Option<&'a str> {
    let at = record.find(&format!("{name}("))? + name.len() + 1;
    let mut depth = 1;
    let mut quoted = false;
    for (k, c) in record[at..].char_indices() {
        match c {
            '\'' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return Some(&record[at..at + k]);
                }
            }
            _ => {}
        }
    }
    None
}

fn numbers(text: &str) -> Vec<f64> {
    text.split(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'E' | 'e')))
        .filter_map(|word| word.parse().ok())
        .collect()
}

fn strings(text: &str) -> Vec<String> {
    text.split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

impl Raw {
    pub(crate) fn parse(text: &str) -> Raw {
        let records = records_of(text);
        let mut containers = HashMap::new();
        for (&id, record) in &records {
            if record.contains("REPRESENTATION(")
                && !record.contains("RELATIONSHIP")
                && !record.contains("DEFINITION_REPRESENTATION")
            {
                let kind = record.split('(').next().unwrap_or("");
                if let Some(args) = args_of(record, kind.trim()) {
                    let all = refs(args);
                    if let Some((_, items)) = all.split_last() {
                        for item in items {
                            containers.entry(*item).or_insert(id);
                        }
                    }
                }
            }
        }
        Raw {
            records,
            containers,
        }
    }

    fn record(&self, id: u64) -> Option<&str> {
        self.records.get(&id).map(String::as_str)
    }

    fn length_unit(&self, id: u64) -> Option<f64> {
        let record = self.record(id)?;
        if !record.contains("LENGTH_UNIT") {
            return None;
        }
        if let Some(args) = args_of(record, "SI_UNIT") {
            let prefix = args.split(',').next().unwrap_or("").trim();
            return Some(match prefix {
                ".MILLI." => 1.0,
                ".CENTI." => 10.0,
                ".DECI." => 100.0,
                ".KILO." => 1.0e6,
                ".MICRO." => 1.0e-3,
                ".NANO." => 1.0e-6,
                _ => 1000.0,
            });
        }
        let args = args_of(record, "CONVERSION_BASED_UNIT")?;
        let measure = *refs(args).first()?;
        let with_unit = self.record(measure)?;
        let factor = *numbers(args_of(with_unit, "LENGTH_MEASURE")?).first()?;
        let unit = *refs(with_unit).last()?;
        Some(factor * self.length_unit(unit)?)
    }

    fn context_scale(&self, context: u64) -> Option<f64> {
        let record = self.record(context)?;
        let units = args_of(record, "GLOBAL_UNIT_ASSIGNED_CONTEXT")?;
        refs(units).into_iter().find_map(|u| self.length_unit(u))
    }

    fn representation_scale(&self, representation: u64) -> f64 {
        self.record(representation)
            .and_then(|record| refs(record).last().copied())
            .and_then(|context| self.context_scale(context))
            .unwrap_or(1.0)
    }

    pub(crate) fn scale_of(&self, item: u64) -> f64 {
        self.containers
            .get(&item)
            .and_then(|rep| refs(self.record(*rep)?).last().copied())
            .and_then(|context| self.context_scale(context))
            .unwrap_or(1.0)
    }

    fn placement(&self, id: u64, scale: f64) -> Option<Matrix4> {
        let record = self.record(id)?;
        let parts = refs(args_of(record, "AXIS2_PLACEMENT_3D")?);
        let point = |id: u64| -> Option<Vector3> {
            let n = numbers(args_of(self.record(id)?, "CARTESIAN_POINT")?);
            (n.len() >= 3).then(|| Vector3::new(n[0], n[1], n[2]))
        };
        let direction = |id: Option<&u64>| -> Option<Vector3> {
            let n = numbers(args_of(self.record(*id?)?, "DIRECTION")?);
            (n.len() >= 3).then(|| Vector3::new(n[0], n[1], n[2]).normalize())
        };
        let origin = point(*parts.first()?)? * scale;
        let z = direction(parts.get(1)).unwrap_or_else(Vector3::unit_z);
        let seed = direction(parts.get(2)).unwrap_or_else(|| {
            if z.x.abs() < 0.9 {
                Vector3::unit_x()
            } else {
                Vector3::unit_y()
            }
        });
        let x = (seed - z * seed.dot(z)).normalize();
        let y = z.cross(x);
        Some(Matrix4::from_cols(
            x.extend(0.0),
            y.extend(0.0),
            z.extend(0.0),
            origin.extend(1.0),
        ))
    }

    fn transformations(&self) -> HashMap<u64, Vec<(u64, f64, u64, f64)>> {
        let mut shape_of: HashMap<u64, u64> = HashMap::new();
        let mut relations: Vec<(u64, u64)> = Vec::new();
        for (&id, record) in &self.records {
            if record.starts_with("PRODUCT_DEFINITION_SHAPE(")
                && let Some(&definition) = args_of(record, "PRODUCT_DEFINITION_SHAPE")
                    .map(refs)
                    .as_ref()
                    .and_then(|r| r.last())
            {
                shape_of.insert(id, definition);
            } else if record.starts_with("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(")
                && let Some(args) = args_of(record, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION")
                && let [relation, shape] = refs(args)[..]
            {
                relations.push((relation, shape));
            }
        }
        let mut out: HashMap<u64, Vec<(u64, f64, u64, f64)>> = HashMap::new();
        for (relation, shape) in relations {
            let Some(&nauo) = shape_of.get(&shape) else {
                continue;
            };
            let Some(pair) = self.record(relation).and_then(|record| {
                let transform = *refs(args_of(
                    record,
                    "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION",
                )?)
                .first()?;
                let reps = refs(args_of(record, "REPRESENTATION_RELATIONSHIP")?);
                let items = refs(args_of(
                    self.record(transform)?,
                    "ITEM_DEFINED_TRANSFORMATION",
                )?);
                Some((
                    *items.first()?,
                    self.representation_scale(*reps.first()?),
                    *items.get(1)?,
                    self.representation_scale(*reps.get(1)?),
                ))
            }) else {
                continue;
            };
            out.entry(nauo).or_default().push(pair);
        }
        out
    }

    pub(crate) fn edge_matrices(
        &self,
    ) -> HashMap<(String, String, String), Vec<(Matrix4, Matrix4)>> {
        let mut out: HashMap<(String, String, String), Vec<(Matrix4, Matrix4)>> = HashMap::new();
        let transformations = self.transformations();
        for (&id, record) in &self.records {
            if !record.starts_with("NEXT_ASSEMBLY_USAGE_OCCURRENCE(") {
                continue;
            }
            let Some(args) = args_of(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE") else {
                continue;
            };
            let words = strings(args);
            let key = (
                words.first().cloned().unwrap_or_default(),
                words.get(1).cloned().unwrap_or_default(),
                words.get(2).cloned().unwrap_or_default(),
            );
            for &(first, first_scale, second, second_scale) in
                transformations.get(&id).into_iter().flatten()
            {
                let (Some(a), Some(b), Some(a_mm), Some(b_mm)) = (
                    self.placement(first, 1.0),
                    self.placement(second, 1.0),
                    self.placement(first, first_scale),
                    self.placement(second, second_scale),
                ) else {
                    continue;
                };
                let (Some(a_inverse), Some(b_inverse), Some(a_mm_inverse), Some(b_mm_inverse)) =
                    (a.invert(), b.invert(), a_mm.invert(), b_mm.invert())
                else {
                    continue;
                };
                let entry = out.entry(key.clone()).or_default();
                entry.push((b * a_inverse, b_mm * a_mm_inverse));
                entry.push((a * b_inverse, a_mm * b_mm_inverse));
            }
        }
        out
    }
}

impl Raw {
    pub(crate) fn bounds(&self, item: u64) -> Option<(Point3, Point3)> {
        let mut seen = std::collections::HashSet::new();
        let mut queue = vec![item];
        let mut low = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut high = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut grow = |p: Point3, reach: f64| {
            for k in 0..3 {
                low[k] = low[k].min(p[k] - reach);
                high[k] = high[k].max(p[k] + reach);
            }
        };
        let point = |id: u64| -> Option<Point3> {
            let n = numbers(args_of(self.record(id)?, "CARTESIAN_POINT")?);
            (n.len() >= 3).then(|| Point3::new(n[0], n[1], n[2]))
        };
        while let Some(id) = queue.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(record) = self.record(id) else {
                continue;
            };
            if let Some(p) = point(id) {
                grow(p, 0.0);
                continue;
            }
            if record.starts_with("AXIS2_PLACEMENT_3D(") {
                continue;
            }
            for kind in ["CIRCLE", "CYLINDRICAL_SURFACE", "SPHERICAL_SURFACE"] {
                if record.starts_with(&format!("{kind}("))
                    && let Some(args) = args_of(record, kind)
                    && let Some(&axis) = refs(args).first()
                    && let Some(centre) = self
                        .record(axis)
                        .and_then(|a| refs(args_of(a, "AXIS2_PLACEMENT_3D")?).first().copied())
                        .and_then(point)
                    && let Some(&radius) = numbers(args.rsplit(',').next().unwrap_or("")).last()
                {
                    grow(centre, radius);
                }
            }
            queue.extend(refs(record));
        }
        let scale = self.scale_of(item);
        (low.x <= high.x).then(|| (low * scale, high * scale))
    }
}

pub(crate) struct Placed {
    pub id: u64,
    pub name: String,
    pub product: String,
    pub matrix: Matrix4,
}

const SOLIDS: [&str; 4] = [
    "MANIFOLD_SOLID_BREP(",
    "BREP_WITH_VOIDS(",
    "FACETED_BREP(",
    "SHELL_BASED_SURFACE_MODEL(",
];

impl Raw {
    pub(crate) fn kind(&self, id: u64) -> &str {
        self.record(id)
            .and_then(|r| r.split('(').next())
            .unwrap_or("")
            .trim()
    }

    fn product_name(&self, definition: u64) -> String {
        let name = || -> Option<String> {
            let formation =
                *refs(args_of(self.record(definition)?, "PRODUCT_DEFINITION")?).first()?;
            let record = self.record(formation)?;
            let kind = record.split('(').next()?.trim();
            let product = *refs(args_of(record, kind)?).first()?;
            strings(args_of(self.record(product)?, "PRODUCT")?)
                .get(1)
                .cloned()
        };
        name().unwrap_or_default()
    }

    pub(crate) fn placed(&self) -> Vec<Placed> {
        let mut shapes: HashMap<u64, u64> = HashMap::new();
        let mut used: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut links: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut children: HashMap<u64, Vec<(u64, u64, String)>> = HashMap::new();
        let mut is_child = std::collections::HashSet::new();
        let mut definitions = Vec::new();
        for (&id, record) in &self.records {
            if record.starts_with("PRODUCT_DEFINITION(") {
                definitions.push(id);
            } else if record.starts_with("PRODUCT_DEFINITION_SHAPE(") {
                if let Some(&definition) = args_of(record, "PRODUCT_DEFINITION_SHAPE")
                    .map(refs)
                    .as_ref()
                    .and_then(|r| r.last())
                {
                    shapes.insert(id, definition);
                }
            } else if record.starts_with("SHAPE_DEFINITION_REPRESENTATION(") {
                if let Some(args) = args_of(record, "SHAPE_DEFINITION_REPRESENTATION")
                    && let [shape, representation] = refs(args)[..]
                {
                    used.entry(shape).or_default().push(representation);
                }
            } else if record.contains("REPRESENTATION_RELATIONSHIP(")
                && !record.contains("WITH_TRANSFORMATION")
            {
                let kind = if record.contains("SHAPE_REPRESENTATION_RELATIONSHIP(") {
                    "SHAPE_REPRESENTATION_RELATIONSHIP"
                } else {
                    "REPRESENTATION_RELATIONSHIP"
                };
                if let Some(args) = args_of(record, kind)
                    && let [a, b] = refs(args)[..]
                {
                    links.entry(a).or_default().push(b);
                    links.entry(b).or_default().push(a);
                }
            } else if record.starts_with("NEXT_ASSEMBLY_USAGE_OCCURRENCE(")
                && let Some(args) = args_of(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE")
                && let [parent, child, ..] = refs(args)[..]
            {
                let name = strings(args).get(1).cloned().unwrap_or_default();
                children.entry(parent).or_default().push((id, child, name));
                is_child.insert(child);
            }
        }
        let mut representations: HashMap<u64, Vec<u64>> = HashMap::new();
        for (shape, definition) in &shapes {
            if let Some(list) = used.get(shape) {
                representations.entry(*definition).or_default().extend(list);
            }
        }
        let transformations = self.transformations();
        let leaves = |definition: u64| -> Vec<u64> {
            let mut seen = std::collections::HashSet::new();
            let mut queue: Vec<u64> = representations
                .get(&definition)
                .cloned()
                .unwrap_or_default();
            let mut out = Vec::new();
            while let Some(rep) = queue.pop() {
                if !seen.insert(rep) {
                    continue;
                }
                if let Some(record) = self.record(rep)
                    && let Some(kind) = record.split('(').next()
                    && let Some(args) = args_of(record, kind.trim())
                {
                    for item in refs(args) {
                        if self
                            .record(item)
                            .is_some_and(|r| SOLIDS.iter().any(|s| r.starts_with(s)))
                        {
                            out.push(item);
                        }
                    }
                }
                queue.extend(links.get(&rep).into_iter().flatten());
            }
            out.sort_unstable();
            out.dedup();
            out
        };
        let mut roots: Vec<u64> = definitions
            .into_iter()
            .filter(|d| !is_child.contains(d))
            .collect();
        roots.sort_unstable();
        let mut out = Vec::new();
        let mut stack: Vec<(u64, Matrix4, String, usize)> = roots
            .into_iter()
            .map(|root| (root, Matrix4::identity(), self.product_name(root), 0))
            .collect();
        while let Some((definition, matrix, name, depth)) = stack.pop() {
            let product = self.product_name(definition);
            for id in leaves(definition) {
                out.push(Placed {
                    id,
                    name: name.clone(),
                    product: product.clone(),
                    matrix,
                });
            }
            if depth > 32 {
                continue;
            }
            for (nauo, child, edge) in children.get(&definition).into_iter().flatten() {
                let step = transformations
                    .get(nauo)
                    .and_then(|pairs| pairs.first())
                    .and_then(|&(first, first_scale, second, second_scale)| {
                        let a = self.placement(first, first_scale)?;
                        let b = self.placement(second, second_scale)?;
                        Some(b * a.invert()?)
                    })
                    .unwrap_or_else(Matrix4::identity);
                let name = if edge.is_empty() {
                    self.product_name(*child)
                } else {
                    edge.clone()
                };
                stack.push((*child, matrix * step, name, depth + 1));
            }
        }
        out
    }
}

impl Raw {
    pub(crate) fn polygons(&self, model: u64) -> Option<Vec<Vec<Vec<Point3>>>> {
        let scale = self.scale_of(model);
        let point = |id: u64| -> Option<Point3> {
            let n = numbers(args_of(self.record(id)?, "CARTESIAN_POINT")?);
            (n.len() >= 3).then(|| Point3::new(n[0], n[1], n[2]) * scale)
        };
        let direction = |id: u64| -> Option<Vector3> {
            let n = numbers(args_of(self.record(id)?, "DIRECTION")?);
            (n.len() >= 3).then(|| Vector3::new(n[0], n[1], n[2]))
        };
        let record = self.record(model)?;
        let shells = if record.starts_with("SHELL_BASED_SURFACE_MODEL(") {
            refs(args_of(record, "SHELL_BASED_SURFACE_MODEL")?)
        } else if record.starts_with("FACETED_BREP(") {
            refs(args_of(record, "FACETED_BREP")?)
        } else {
            return None;
        };
        let mut out = Vec::new();
        for shell in shells {
            let record = self.record(shell)?;
            let kind = record.split('(').next()?.trim();
            for face in refs(args_of(record, kind)?) {
                let record = self.record(face)?;
                let kind = record.split('(').next()?.trim();
                if kind != "FACE_SURFACE" && kind != "ADVANCED_FACE" {
                    return None;
                }
                let args = args_of(record, kind)?;
                let all = refs(args);
                let (&surface, bounds) = all.split_last()?;
                let mut loops = Vec::new();
                for &bound in bounds {
                    let record = self.record(bound)?;
                    let kind = record.split('(').next()?.trim();
                    let outer = kind == "FACE_OUTER_BOUND";
                    let polygon_loop = *refs(args_of(record, kind)?).first()?;
                    let polygon: Vec<Point3> =
                        refs(args_of(self.record(polygon_loop)?, "POLY_LOOP")?)
                            .into_iter()
                            .map(point)
                            .collect::<Option<_>>()?;
                    if polygon.len() < 3 {
                        return None;
                    }
                    if outer {
                        loops.insert(0, polygon);
                    } else {
                        loops.push(polygon);
                    }
                }
                let along = args.trim_end().ends_with(".T.");
                let normal = self
                    .record(surface)
                    .and_then(|r| refs(args_of(r, "PLANE")?).first().copied())
                    .and_then(|axis| {
                        refs(args_of(self.record(axis)?, "AXIS2_PLACEMENT_3D")?)
                            .get(1)
                            .copied()
                    })
                    .and_then(direction)
                    .map(|n| if along { n } else { -n });
                let first = loops.first()?;
                let winding = (0..first.len()).fold(Vector3::zero(), |w, k| {
                    w + first[k]
                        .to_vec()
                        .cross(first[(k + 1) % first.len()].to_vec())
                });
                if normal.is_some_and(|n| winding.dot(n) < 0.0) {
                    loops[0].reverse();
                }
                out.push(loops);
            }
        }
        Some(out)
    }
}

pub(crate) fn close(a: &Matrix4, b: &Matrix4) -> bool {
    let scale = (0..4)
        .flat_map(|c| (0..4).map(move |r| (c, r)))
        .fold(1.0f64, |m, (c, r)| m.max(a[c][r].abs()).max(b[c][r].abs()));
    (0..4).all(|c| (0..4).all(|r| (a[c][r] - b[c][r]).abs() <= scale * 1.0e-9))
}
