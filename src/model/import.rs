use crate::geometry::{self, Profile, Segment};
use anyhow::{Context, Result, anyhow, bail};
use monstertruck::modeling::*;
use std::collections::HashMap;

type P2 = (f64, f64);

pub(crate) fn stl_solid(path: &std::path::Path) -> Result<Solid> {
    let file = std::fs::File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let mesh = monstertruck::mesh::stl::read(file, monstertruck::mesh::stl::StlType::Automatic)
        .map_err(|e| anyhow!("{}: {e}", path.display()))?;
    mesh_solid(&mesh)
}

fn find(parent: &mut [usize], i: usize) -> usize {
    let mut root = i;
    while parent[root] != root {
        root = parent[root];
    }
    let mut at = i;
    while parent[at] != root {
        let next = parent[at];
        parent[at] = root;
        at = next;
    }
    root
}

fn mesh_solid(mesh: &monstertruck::mesh::PolygonMesh) -> Result<Solid> {
    let (points, triangles) = geometry::welded(mesh);
    if triangles.len() < 4 {
        bail!(
            "the mesh has {} triangles; a solid needs at least 4",
            triangles.len()
        );
    }
    let size = points
        .iter()
        .collect::<BoundingBox<Point3>>()
        .diameter()
        .max(1.0e-9);
    let normals: Vec<Vector3> = triangles
        .iter()
        .map(|[a, b, c]| {
            (points[*b] - points[*a])
                .cross(points[*c] - points[*a])
                .normalize()
        })
        .collect();
    let mut owner: HashMap<(usize, usize), usize> = HashMap::new();
    for (t, [a, b, c]) in triangles.iter().enumerate() {
        for edge in [(*a, *b), (*b, *c), (*c, *a)] {
            if owner.insert(edge, t).is_some() {
                bail!("the mesh is not a closed, consistently wound surface");
            }
        }
    }
    if owner.keys().any(|&(a, b)| !owner.contains_key(&(b, a))) {
        bail!("the mesh has holes; it must be watertight to become a solid");
    }
    let mut parent: Vec<usize> = (0..triangles.len()).collect();
    for (&(a, b), &t) in &owner {
        let u = owner[&(b, a)];
        let (n, m) = (normals[t], normals[u]);
        let offset = |k: usize| points[triangles[k][0]].to_vec().dot(normals[k]);
        if n.dot(m) > 1.0 - 1.0e-9 && (offset(t) - offset(u)).abs() < size * 1.0e-9 {
            let (rt, ru) = (find(&mut parent, t), find(&mut parent, u));
            parent[rt] = ru;
        }
    }
    let groups: Vec<usize> = (0..triangles.len()).map(|t| find(&mut parent, t)).collect();
    let mut boundary: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
    for (&(a, b), &t) in &owner {
        if groups[owner[&(b, a)]] != groups[t] {
            boundary.entry(groups[t]).or_default().push((a, b));
        }
    }
    let mut touching: HashMap<usize, Vec<usize>> = HashMap::new();
    for edges in boundary.values() {
        for &(a, b) in edges {
            if a < b {
                touching.entry(a).or_default().push(b);
                touching.entry(b).or_default().push(a);
            }
        }
    }
    let removable = |v: usize| {
        touching.get(&v).is_some_and(|near| {
            near.len() == 2 && {
                let (p, q) = (points[near[0]] - points[v], points[near[1]] - points[v]);
                p.normalize().dot(q.normalize()) < -1.0 + 1.0e-12
            }
        })
    };
    let vertices: Vec<Vertex> = points.iter().map(|p| Vertex::new(*p)).collect();
    let mut edges: HashMap<(usize, usize), Edge> = HashMap::new();
    let mut edge = |a: usize, b: usize| -> Edge {
        if let Some(e) = edges.get(&(a, b)) {
            return e.clone();
        }
        if let Some(e) = edges.get(&(b, a)) {
            return e.inverse();
        }
        let e = Edge::new(
            &vertices[a],
            &vertices[b],
            Curve::Line(Line(points[a], points[b])),
        );
        edges.insert((a, b), e.clone());
        e
    };
    let mut faces = Vec::new();
    let mut keys: Vec<usize> = boundary.keys().copied().collect();
    keys.sort_unstable();
    for group in keys {
        let mut next: HashMap<usize, Vec<usize>> = HashMap::new();
        for &(a, b) in &boundary[&group] {
            next.entry(a).or_default().push(b);
        }
        let mut wires = Vec::new();
        while let Some(&start) = next.iter().find(|(_, v)| !v.is_empty()).map(|(k, _)| k) {
            let mut chain = vec![start];
            let mut at = next.get_mut(&start).and_then(Vec::pop).expect("non-empty");
            while at != start {
                chain.push(at);
                at = next
                    .get_mut(&at)
                    .and_then(Vec::pop)
                    .ok_or_else(|| anyhow!("a face boundary in the mesh does not close"))?;
            }
            let kept: Vec<usize> = chain.iter().copied().filter(|&v| !removable(v)).collect();
            if kept.len() < 3 {
                bail!("a face of the mesh collapsed while merging");
            }
            let wire: Wire = (0..kept.len())
                .map(|i| edge(kept[i], kept[(i + 1) % kept.len()]))
                .collect();
            wires.push(wire);
        }
        let normal = normals[group];
        let origin = points[triangles[group][0]];
        let u = (points[triangles[group][1]] - origin).normalize();
        let v = normal.cross(u);
        let plane = Plane::new(origin, origin + u, origin + v);
        faces.push(
            Face::try_new(wires, Surface::Plane(plane)).map_err(|e| anyhow!("a mesh face: {e}"))?,
        );
    }
    Solid::try_new(vec![faces.into()])
        .map_err(|e| anyhow!("the mesh does not close into a solid: {e}"))
}

fn closed(start: P2, segments: Vec<Segment>) -> Profile {
    if segments.iter().all(|s| matches!(s, Segment::Line(_))) {
        let mut points = vec![start];
        points.extend(segments.iter().map(Segment::end));
        points.pop();
        Profile::Polygon { points }
    } else {
        Profile::Path { start, segments }
    }
}

fn near(a: P2, b: P2, tolerance: f64) -> bool {
    (a.0 - b.0).hypot(a.1 - b.1) <= tolerance
}

fn chain(pieces: Vec<(P2, Segment)>, tolerance: f64) -> Result<Vec<Profile>> {
    let reverse = |(from, segment): &(P2, Segment)| -> (P2, Segment) {
        let to = segment.end();
        let back = match segment {
            Segment::Line(_) => Segment::Line(*from),
            Segment::Arc { via, .. } => Segment::Arc {
                to: *from,
                via: *via,
            },
            Segment::Cubic { c1, c2, .. } => Segment::Cubic {
                to: *from,
                c1: *c2,
                c2: *c1,
            },
        };
        (to, back)
    };
    let mut left = pieces;
    let mut profiles = Vec::new();
    while let Some((start, first)) = left.pop() {
        let mut segments = vec![first];
        loop {
            let cursor = segments.last().expect("non-empty").end();
            if near(cursor, start, tolerance) {
                break;
            }
            let found = left.iter().position(|(from, s)| {
                near(*from, cursor, tolerance) || near(s.end(), cursor, tolerance)
            });
            let Some(i) = found else {
                bail!(
                    "the drawing has an open outline near {:.3},{:.3}",
                    cursor.0,
                    cursor.1
                );
            };
            let piece = left.swap_remove(i);
            let piece = if near(piece.0, cursor, tolerance) {
                piece
            } else {
                reverse(&piece)
            };
            segments.push(piece.1);
        }
        profiles.push(closed(start, segments));
    }
    Ok(profiles)
}

fn bulge_segment(from: P2, to: P2, bulge: f64) -> Segment {
    if bulge.abs() < 1.0e-12 {
        return Segment::Line(to);
    }
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let chord = dx.hypot(dy);
    let sagitta = bulge * chord / 2.0;
    let left = (-dy / chord, dx / chord);
    let middle = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
    Segment::Arc {
        to,
        via: (middle.0 - left.0 * sagitta, middle.1 - left.1 * sagitta),
    }
}

pub(crate) fn dxf_profiles(text: &str) -> Result<Vec<Profile>> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(i32, &str)> = lines
        .chunks(2)
        .filter(|pair| pair.len() == 2)
        .map(|pair| {
            Ok((
                pair[0]
                    .parse::<i32>()
                    .map_err(|_| anyhow!("`{}` is not a DXF group code", pair[0]))?,
                pair[1],
            ))
        })
        .collect::<Result<_>>()?;
    let start = pairs
        .windows(2)
        .position(|w| w[0] == (0, "SECTION") && w[1] == (2, "ENTITIES"))
        .ok_or_else(|| anyhow!("the DXF has no ENTITIES section"))?
        + 2;
    let mut entities: Vec<(&str, Vec<(i32, &str)>)> = Vec::new();
    for &(code, value) in &pairs[start..] {
        if code == 0 {
            if value == "ENDSEC" {
                break;
            }
            entities.push((value, Vec::new()));
        } else if let Some((_, fields)) = entities.last_mut() {
            fields.push((code, value));
        }
    }
    let number = |fields: &[(i32, &str)], code: i32| -> Result<f64> {
        fields
            .iter()
            .find(|(c, _)| *c == code)
            .ok_or_else(|| anyhow!("a DXF entity is missing group {code}"))?
            .1
            .parse::<f64>()
            .map_err(|e| anyhow!("group {code}: {e}"))
    };
    let mut profiles = Vec::new();
    let mut pieces: Vec<(P2, Segment)> = Vec::new();
    let mut polyline: Option<(bool, Vec<(P2, f64)>)> = None;
    let finish = |closed_flag: bool,
                  vertices: Vec<(P2, f64)>,
                  profiles: &mut Vec<Profile>,
                  pieces: &mut Vec<(P2, Segment)>| {
        let n = vertices.len();
        let count = if closed_flag { n } else { n.saturating_sub(1) };
        let segments: Vec<(P2, Segment)> = (0..count)
            .map(|i| {
                let ((from, bulge), (to, _)) = (vertices[i], vertices[(i + 1) % n]);
                (from, bulge_segment(from, to, bulge))
            })
            .collect();
        if closed_flag && n >= 2 {
            let start = segments[0].0;
            profiles.push(closed(
                start,
                segments.into_iter().map(|(_, s)| s).collect(),
            ));
        } else {
            pieces.extend(segments);
        }
    };
    for (kind, fields) in &entities {
        match *kind {
            "LINE" => pieces.push((
                (number(fields, 10)?, number(fields, 20)?),
                Segment::Line((number(fields, 11)?, number(fields, 21)?)),
            )),
            "CIRCLE" => profiles.push(Profile::Circle {
                center: (number(fields, 10)?, number(fields, 20)?),
                diameter: 2.0 * number(fields, 40)?,
            }),
            "ARC" => {
                let (cx, cy, r) = (
                    number(fields, 10)?,
                    number(fields, 20)?,
                    number(fields, 40)?,
                );
                let a0 = number(fields, 50)?.to_radians();
                let mut a1 = number(fields, 51)?.to_radians();
                while a1 <= a0 {
                    a1 += std::f64::consts::TAU;
                }
                let at = |a: f64| (cx + r * a.cos(), cy + r * a.sin());
                pieces.push((
                    at(a0),
                    Segment::Arc {
                        to: at(a1),
                        via: at((a0 + a1) / 2.0),
                    },
                ));
            }
            "LWPOLYLINE" => {
                let flag = fields
                    .iter()
                    .find(|(c, _)| *c == 70)
                    .map_or(0, |(_, v)| v.parse::<i32>().unwrap_or(0));
                let mut vertices: Vec<(P2, f64)> = Vec::new();
                let mut x = None;
                for &(code, value) in fields {
                    let v: f64 = match code {
                        10 | 20 | 42 => value.parse().map_err(|e| anyhow!("group {code}: {e}"))?,
                        _ => continue,
                    };
                    match code {
                        10 => x = Some(v),
                        20 => vertices.push((
                            (x.take().ok_or_else(|| anyhow!("a vertex has no x"))?, v),
                            0.0,
                        )),
                        _ => {
                            if let Some(last) = vertices.last_mut() {
                                last.1 = v;
                            }
                        }
                    }
                }
                finish(flag & 1 == 1, vertices, &mut profiles, &mut pieces);
            }
            "POLYLINE" => {
                let flag = fields
                    .iter()
                    .find(|(c, _)| *c == 70)
                    .map_or(0, |(_, v)| v.parse::<i32>().unwrap_or(0));
                polyline = Some((flag & 1 == 1, Vec::new()));
            }
            "VERTEX" => {
                if let Some((_, vertices)) = polyline.as_mut() {
                    let bulge = number(fields, 42).unwrap_or(0.0);
                    vertices.push(((number(fields, 10)?, number(fields, 20)?), bulge));
                }
            }
            "SEQEND" => {
                if let Some((flag, vertices)) = polyline.take() {
                    finish(flag, vertices, &mut profiles, &mut pieces);
                }
            }
            "TEXT" | "MTEXT" | "DIMENSION" | "POINT" | "INSERT" | "HATCH" => {}
            other => bail!(
                "the DXF has a {other}; gcad reads LINE, ARC, CIRCLE, LWPOLYLINE and POLYLINE"
            ),
        }
    }
    let scale = profiles.len() + pieces.len();
    if scale == 0 {
        bail!("the DXF has no outlines");
    }
    profiles.extend(chain(pieces, 1.0e-6)?);
    Ok(profiles)
}

#[derive(Clone, Copy)]
struct Affine([f64; 6]);

impl Affine {
    const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn then(self, inner: Affine) -> Affine {
        Affine(geometry::compose_2d(self.0, inner.0))
    }

    fn apply(&self, p: P2) -> P2 {
        geometry::place_2d(self.0, p)
    }
}

fn svg_numbers(text: &str) -> Vec<f64> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    let flush = |current: &mut String, numbers: &mut Vec<f64>| {
        if let Ok(v) = current.parse::<f64>() {
            numbers.push(v);
        }
        current.clear();
    };
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let exponent_sign = (c == '-' || c == '+') && i > 0 && matches!(chars[i - 1], 'e' | 'E');
        if c.is_ascii_digit() || c == 'e' || c == 'E' || exponent_sign {
            current.push(c);
        } else if c == '.' {
            if current.contains('.') && !current.contains(['e', 'E']) {
                flush(&mut current, &mut numbers);
            }
            current.push(c);
        } else if c == '-' || c == '+' {
            flush(&mut current, &mut numbers);
            current.push(c);
        } else {
            flush(&mut current, &mut numbers);
        }
    }
    flush(&mut current, &mut numbers);
    numbers
}

fn svg_transform(text: &str) -> Result<Affine> {
    let mut result = Affine::IDENTITY;
    for part in text.split(')').map(str::trim).filter(|p| !p.is_empty()) {
        let (name, args) = part
            .split_once('(')
            .ok_or_else(|| anyhow!("cannot read the SVG transform `{text}`"))?;
        let v = svg_numbers(args);
        let get = |i: usize| v.get(i).copied().unwrap_or(0.0);
        let step = match name.trim().trim_start_matches(',').trim() {
            "matrix" if v.len() == 6 => Affine([v[0], v[2], v[1], v[3], v[4], v[5]]),
            "translate" => Affine([1.0, 0.0, 0.0, 1.0, get(0), get(1)]),
            "scale" => {
                let sx = get(0);
                let sy = v.get(1).copied().unwrap_or(sx);
                Affine([sx, 0.0, 0.0, sy, 0.0, 0.0])
            }
            "rotate" => {
                let (s, c) = get(0).to_radians().sin_cos();
                let (cx, cy) = (get(1), get(2));
                Affine([c, -s, s, c, cx - c * cx + s * cy, cy - s * cx - c * cy])
            }
            other => bail!("gcad does not read the SVG transform `{other}`"),
        };
        result = result.then(step);
    }
    Ok(result)
}

struct PathBuilder {
    transform: Affine,
    profiles: Vec<Profile>,
    start: Option<P2>,
    segments: Vec<Segment>,
}

impl PathBuilder {
    fn at(&self, p: P2) -> P2 {
        let q = self.transform.apply(p);
        (q.0, -q.1)
    }

    fn close(&mut self) {
        if let Some(start) = self.start.take() {
            let segments = std::mem::take(&mut self.segments);
            let last = segments.last().map(Segment::end);
            let mut segments = segments;
            if last.is_some_and(|l| !near(l, start, 1.0e-9)) {
                segments.push(Segment::Line(start));
            }
            if segments.len() >= 2 {
                self.profiles.push(closed(start, segments));
            }
        }
    }

    fn line(&mut self, to: P2) {
        let to = self.at(to);
        self.segments.push(Segment::Line(to));
    }

    fn similar(&self) -> bool {
        let [a, b, c, d, _, _] = self.transform.0;
        (a - d).abs() < 1.0e-9 && (b + c).abs() < 1.0e-9
            || (a + d).abs() < 1.0e-9 && (b - c).abs() < 1.0e-9
    }

    fn arc(&mut self, via: P2, to: P2) {
        let (via, to) = (self.at(via), self.at(to));
        self.segments.push(Segment::Arc { to, via });
    }

    fn cubic(&mut self, c1: P2, c2: P2, to: P2) {
        let (c1, c2, to) = (self.at(c1), self.at(c2), self.at(to));
        self.segments.push(Segment::Cubic { to, c1, c2 });
    }
}

type ArcPiece = (P2, P2, P2, P2);

fn arc_cubics(
    from: P2,
    rx: f64,
    ry: f64,
    phi: f64,
    large: bool,
    sweep: bool,
    to: P2,
) -> Vec<ArcPiece> {
    if rx.abs() < 1.0e-12 || ry.abs() < 1.0e-12 || near(from, to, 1.0e-12) {
        return vec![(to, to, to, to)];
    }
    let (s, c) = phi.to_radians().sin_cos();
    let (dx, dy) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
    let (x1, y1) = (c * dx + s * dy, -s * dx + c * dy);
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let numerator = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = (numerator / denominator).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cx1, cy1) = (k * rx * y1 / ry, -k * ry * x1 / rx);
    let (cx, cy) = (
        c * cx1 - s * cy1 + (from.0 + to.0) / 2.0,
        s * cx1 + c * cy1 + (from.1 + to.1) / 2.0,
    );
    let angle = |ux: f64, uy: f64| uy.atan2(ux);
    let theta = angle((x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut delta = angle((-x1 - cx1) / rx, (-y1 - cy1) / ry) - theta;
    if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    } else if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    }
    let pieces = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = delta / pieces as f64;
    let point = |t: f64| {
        let (x, y) = (rx * t.cos(), ry * t.sin());
        (c * x - s * y + cx, s * x + c * y + cy)
    };
    let tangent = |t: f64| {
        let (x, y) = (-rx * t.sin(), ry * t.cos());
        (c * x - s * y, s * x + c * y)
    };
    let alpha = 4.0 / 3.0 * (step / 4.0).tan();
    (0..pieces)
        .map(|i| {
            let (t0, t1) = (theta + step * i as f64, theta + step * (i + 1) as f64);
            let (p0, p1) = (point(t0), point(t1));
            let (d0, d1) = (tangent(t0), tangent(t1));
            (
                (p0.0 + alpha * d0.0, p0.1 + alpha * d0.1),
                (p1.0 - alpha * d1.0, p1.1 - alpha * d1.1),
                p1,
                point((t0 + t1) / 2.0),
            )
        })
        .collect()
}

fn svg_path(data: &str, transform: Affine) -> Result<Vec<Profile>> {
    let mut builder = PathBuilder {
        transform,
        profiles: Vec::new(),
        start: None,
        segments: Vec::new(),
    };
    let mut commands: Vec<(char, Vec<f64>)> = Vec::new();
    let mut text = String::new();
    for c in data.chars() {
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            if let Some((_, args)) = commands.last_mut() {
                *args = svg_numbers(&text);
            }
            text.clear();
            commands.push((c, Vec::new()));
        } else {
            text.push(c);
        }
    }
    if let Some((_, args)) = commands.last_mut() {
        *args = svg_numbers(&text);
    }
    let (mut cursor, mut subpath_start) = ((0.0, 0.0), (0.0, 0.0));
    let mut last_control: Option<(char, P2)> = None;
    for (command, args) in commands {
        let relative = command.is_ascii_lowercase();
        let offset = |p: P2, cursor: P2| {
            if relative {
                (p.0 + cursor.0, p.1 + cursor.1)
            } else {
                p
            }
        };
        let arity = match command.to_ascii_uppercase() {
            'M' | 'L' | 'T' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'S' | 'Q' => 4,
            'A' => 7,
            'Z' => 0,
            other => bail!("unknown SVG path command `{other}`"),
        };
        if arity == 0 {
            builder.close();
            cursor = subpath_start;
            last_control = None;
            continue;
        }
        if args.len() % arity != 0 || args.is_empty() {
            bail!("SVG path command `{command}` has {} numbers", args.len());
        }
        for (k, v) in args.chunks(arity).enumerate() {
            let upper = command.to_ascii_uppercase();
            match upper {
                'M' if k == 0 => {
                    builder.close();
                    cursor = offset((v[0], v[1]), cursor);
                    subpath_start = cursor;
                    builder.start = Some(builder.at(cursor));
                }
                'M' | 'L' => {
                    cursor = offset((v[0], v[1]), cursor);
                    builder.line(cursor);
                }
                'H' => {
                    cursor = (if relative { cursor.0 + v[0] } else { v[0] }, cursor.1);
                    builder.line(cursor);
                }
                'V' => {
                    cursor = (cursor.0, if relative { cursor.1 + v[0] } else { v[0] });
                    builder.line(cursor);
                }
                'C' | 'S' => {
                    let (c1, c2, to) = if upper == 'C' {
                        (
                            offset((v[0], v[1]), cursor),
                            offset((v[2], v[3]), cursor),
                            offset((v[4], v[5]), cursor),
                        )
                    } else {
                        let reflected = match last_control {
                            Some(('C', c)) => (2.0 * cursor.0 - c.0, 2.0 * cursor.1 - c.1),
                            _ => cursor,
                        };
                        (
                            reflected,
                            offset((v[0], v[1]), cursor),
                            offset((v[2], v[3]), cursor),
                        )
                    };
                    builder.cubic(c1, c2, to);
                    last_control = Some(('C', c2));
                    cursor = to;
                    continue;
                }
                'Q' | 'T' => {
                    let (q, to) = if upper == 'Q' {
                        (offset((v[0], v[1]), cursor), offset((v[2], v[3]), cursor))
                    } else {
                        let reflected = match last_control {
                            Some(('Q', c)) => (2.0 * cursor.0 - c.0, 2.0 * cursor.1 - c.1),
                            _ => cursor,
                        };
                        (reflected, offset((v[0], v[1]), cursor))
                    };
                    let c1 = (
                        cursor.0 + 2.0 / 3.0 * (q.0 - cursor.0),
                        cursor.1 + 2.0 / 3.0 * (q.1 - cursor.1),
                    );
                    let c2 = (
                        to.0 + 2.0 / 3.0 * (q.0 - to.0),
                        to.1 + 2.0 / 3.0 * (q.1 - to.1),
                    );
                    builder.cubic(c1, c2, to);
                    last_control = Some(('Q', q));
                    cursor = to;
                    continue;
                }
                _ => {
                    let to = offset((v[5], v[6]), cursor);
                    let circular = (v[0].abs() - v[1].abs()).abs() < 1.0e-9 * v[0].abs().max(1.0)
                        && builder.similar();
                    for (c1, c2, end, middle) in
                        arc_cubics(cursor, v[0], v[1], v[2], v[3] != 0.0, v[4] != 0.0, to)
                    {
                        if near(c1, end, 0.0) && near(c2, end, 0.0) {
                            builder.line(end);
                        } else if circular {
                            builder.arc(middle, end);
                        } else {
                            builder.cubic(c1, c2, end);
                        }
                    }
                    cursor = to;
                }
            }
            last_control = None;
        }
    }
    builder.close();
    Ok(builder.profiles)
}

fn attributes(tag: &str) -> HashMap<String, String> {
    let mut found = HashMap::new();
    let mut rest = tag;
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq]
            .split_whitespace()
            .last()
            .unwrap_or_default()
            .to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let body = &after[1..];
        let Some(end) = body.find(quote) else { break };
        found.insert(name, body[..end].to_string());
        rest = &body[end + 1..];
    }
    found
}

pub(crate) fn svg_profiles(text: &str) -> Result<Vec<Profile>> {
    let mut stack = vec![Affine::IDENTITY];
    let mut profiles = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let close = rest
            .find('>')
            .ok_or_else(|| anyhow!("the SVG has an unclosed tag"))?;
        let tag = &rest[..close];
        rest = &rest[close + 1..];
        if tag.starts_with(['!', '?']) {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            if name.trim() == "g" || name.trim() == "svg" {
                stack.pop();
            }
            continue;
        }
        let name: String = tag
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '/')
            .collect();
        let self_closing = tag.trim_end().ends_with('/');
        let attrs = attributes(tag);
        let parent = *stack.last().unwrap_or(&Affine::IDENTITY);
        let transform = match attrs.get("transform") {
            Some(t) => parent.then(svg_transform(t)?),
            None => parent,
        };
        let number = |key: &str| {
            attrs
                .get(key)
                .map(|v| svg_numbers(v).first().copied().unwrap_or(0.0))
                .unwrap_or(0.0)
        };
        let shape: Option<String> = match name.as_str() {
            "g" | "svg" => {
                if !self_closing {
                    stack.push(transform);
                }
                None
            }
            "rect" => {
                let (x, y, w, h) = (number("x"), number("y"), number("width"), number("height"));
                let r = number("rx").max(number("ry")).min(w / 2.0).min(h / 2.0);
                Some(if r > 0.0 {
                    format!(
                        "M{},{} H{} A{r},{r} 0 0 1 {},{} V{} A{r},{r} 0 0 1 {},{} H{} A{r},{r} 0 0 1 {},{} V{} A{r},{r} 0 0 1 {},{} Z",
                        x + r,
                        y,
                        x + w - r,
                        x + w,
                        y + r,
                        y + h - r,
                        x + w - r,
                        y + h,
                        x + r,
                        x,
                        y + h - r,
                        y + r,
                        x + r,
                        y
                    )
                } else {
                    format!("M{x},{y} H{} V{} H{x} Z", x + w, y + h)
                })
            }
            "circle" | "ellipse" => {
                let (cx, cy) = (number("cx"), number("cy"));
                let (rx, ry) = if name == "circle" {
                    (number("r"), number("r"))
                } else {
                    (number("rx"), number("ry"))
                };
                Some(format!(
                    "M{},{cy} A{rx},{ry} 0 0 1 {cx},{} A{rx},{ry} 0 0 1 {},{cy} A{rx},{ry} 0 0 1 {cx},{} A{rx},{ry} 0 0 1 {},{cy} Z",
                    cx + rx,
                    cy + ry,
                    cx - rx,
                    cy - ry,
                    cx + rx
                ))
            }
            "polygon" | "polyline" => attrs.get("points").map(|p| {
                let v = svg_numbers(p);
                let mut d = String::new();
                for (i, xy) in v.chunks(2).enumerate() {
                    if xy.len() == 2 {
                        d.push_str(&format!(
                            "{}{},{} ",
                            if i == 0 { 'M' } else { 'L' },
                            xy[0],
                            xy[1]
                        ));
                    }
                }
                d + "Z"
            }),
            "path" => attrs.get("d").cloned(),
            _ => None,
        };
        if let Some(d) = shape {
            profiles.extend(svg_path(&d, transform)?);
        }
    }
    if profiles.is_empty() {
        bail!("the SVG has no closed shapes");
    }
    Ok(profiles)
}
