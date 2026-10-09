use super::Model;
use super::args::{label_of, point3};
use crate::parse::Line;
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

enum Ring {
    Polygon(Vec<(f64, f64)>),
    Circle((f64, f64), f64),
}

struct Section {
    name: String,
    rings: Vec<Ring>,
    reach: Vec<(f64, f64)>,
}

fn sizes(text: &str, count: usize, kind: &str, shape: &str) -> Result<Vec<f64>> {
    let values = text
        .split(['x', 'X'])
        .map(|part| {
            part.parse::<f64>()
                .ok()
                .filter(|v| *v > 0.0)
                .ok_or_else(|| anyhow!("`{text}` is not a size for `{kind}`; write it {shape}"))
        })
        .collect::<Result<Vec<_>>>()?;
    if values.len() != count {
        bail!("`{kind}` takes {shape}, got `{text}`");
    }
    Ok(values)
}

fn section(kind: &str, size: &str) -> Result<Section> {
    let rect = |w: f64, h: f64| {
        vec![
            (-w / 2.0, -h / 2.0),
            (w / 2.0, -h / 2.0),
            (w / 2.0, h / 2.0),
            (-w / 2.0, h / 2.0),
        ]
    };
    let rings = match kind {
        "tube" => {
            let [w, h, t] = sizes(size, 3, kind, "WxHxT, like 40x40x3")?[..] else {
                unreachable!()
            };
            if 2.0 * t >= w.min(h) {
                bail!("a {w}x{h} tube cannot have {t} walls");
            }
            vec![
                Ring::Polygon(rect(w, h)),
                Ring::Polygon(rect(w - 2.0 * t, h - 2.0 * t)),
            ]
        }
        "bar" => {
            let [w, h] = sizes(size, 2, kind, "WxH, like 40x10")?[..] else {
                unreachable!()
            };
            vec![Ring::Polygon(rect(w, h))]
        }
        "pipe" => {
            let [d, t] = sizes(size, 2, kind, "DxT, like 33.7x2.6")?[..] else {
                unreachable!()
            };
            if 2.0 * t >= d {
                bail!("a {d} pipe cannot have {t} walls");
            }
            vec![
                Ring::Circle((0.0, 0.0), d / 2.0),
                Ring::Circle((0.0, 0.0), d / 2.0 - t),
            ]
        }
        "rod" => {
            let [d] = sizes(size, 1, kind, "D, like 12")?[..] else {
                unreachable!()
            };
            vec![Ring::Circle((0.0, 0.0), d / 2.0)]
        }
        "angle" => {
            let [w, h, t] = sizes(size, 3, kind, "WxHxT, like 40x40x4")?[..] else {
                unreachable!()
            };
            if t >= w.min(h) {
                bail!("a {w}x{h} angle cannot be {t} thick");
            }
            vec![Ring::Polygon(vec![
                (0.0, 0.0),
                (w, 0.0),
                (w, t),
                (t, t),
                (t, h),
                (0.0, h),
            ])]
        }
        "channel" => {
            let [w, h, t] = sizes(size, 3, kind, "WxHxT, like 50x25x3")?[..] else {
                unreachable!()
            };
            if 2.0 * t >= w || t >= h {
                bail!("a {w}x{h} channel cannot be {t} thick");
            }
            let s = w / 2.0;
            vec![Ring::Polygon(vec![
                (-s, 0.0),
                (s, 0.0),
                (s, h),
                (s - t, h),
                (s - t, t),
                (-s + t, t),
                (-s + t, h),
                (-s, h),
            ])]
        }
        other => bail!(
            "unknown member `{other}`; members are tube WxHxT, bar WxH, pipe DxT, rod D, angle WxHxT and channel WxHxT"
        ),
    };
    let reach = match &rings[0] {
        Ring::Polygon(points) => points.clone(),
        Ring::Circle(c, r) => (0..64)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / 64.0;
                (c.0 + r * a.cos(), c.1 + r * a.sin())
            })
            .collect(),
    };
    Ok(Section {
        name: format!("{kind} {size}"),
        rings,
        reach,
    })
}

fn shifted(section: Section, by: (f64, f64)) -> Section {
    let mv = |(u, v): (f64, f64)| (u + by.0, v + by.1);
    Section {
        rings: section
            .rings
            .into_iter()
            .map(|ring| match ring {
                Ring::Polygon(points) => Ring::Polygon(points.into_iter().map(mv).collect()),
                Ring::Circle(c, r) => Ring::Circle(mv(c), r),
            })
            .collect(),
        reach: section.reach.into_iter().map(mv).collect(),
        ..section
    }
}

fn ring_wire(ring: &Ring, placement: Matrix4) -> Wire {
    let unit: Wire = match ring {
        Ring::Polygon(points) => {
            let vertices = builder::vertices(points.iter().map(|&(u, v)| Point3::new(u, v, 0.0)));
            (0..vertices.len())
                .map(|i| builder::line(&vertices[i], &vertices[(i + 1) % vertices.len()]))
                .collect()
        }
        Ring::Circle(c, r) => {
            let seam = Point3::new(
                c.0 + r * std::f64::consts::FRAC_1_SQRT_2,
                c.1 + r * std::f64::consts::FRAC_1_SQRT_2,
                0.0,
            );
            primitive::circle(seam, Point3::new(c.0, c.1, 0.0), Vector3::unit_z(), 4)
        }
    };
    builder::transformed(&unit, placement)
}

struct Cut {
    point: Point3,
    normal: Vector3,
}

fn oblique(cut: &Cut, along: Vector3, x: Vector3, y: Vector3) -> Matrix4 {
    let lean = along.dot(cut.normal);
    let skew = |v: Vector3| v - along * (v.dot(cut.normal) / lean);
    Matrix4::from_cols(
        skew(x).extend(0.0),
        skew(y).extend(0.0),
        along.extend(0.0),
        cut.point.to_homogeneous(),
    )
}

fn member_solid(
    section: &Section,
    start: &Cut,
    end: &Cut,
    along: Vector3,
    x: Vector3,
    y: Vector3,
) -> Result<Solid> {
    let (from, to) = (oblique(start, along, x, y), oblique(end, along, x, y));
    let mut faces: Vec<Face> = Vec::new();
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    for (k, ring) in section.rings.iter().enumerate() {
        let (a, b) = (ring_wire(ring, from), ring_wire(ring, to));
        let shell: Shell = builder::try_skin_wires(&[a.clone(), b.clone()])
            .map_err(|e| anyhow!("cannot build the member: {e}"))?;
        let mut sides: Vec<Face> = shell.face_iter().cloned().collect();
        if k > 0 {
            sides.iter_mut().for_each(|face| {
                face.invert();
            });
        }
        faces.extend(sides);
        starts.push(if k == 0 { a.inverse() } else { a });
        ends.push(if k == 0 { b } else { b.inverse() });
    }
    faces.push(
        builder::try_attach_plane(starts).map_err(|e| anyhow!("cannot cap the member: {e}"))?,
    );
    faces.push(builder::try_attach_plane(ends).map_err(|e| anyhow!("cannot cap the member: {e}"))?);
    let solid =
        Solid::try_new(vec![faces.into()]).map_err(|e| anyhow!("the member is not closed: {e}"))?;
    Ok(super::solids::oriented(solid))
}

impl Model {
    pub(crate) fn op_member(&mut self, line: &Line) -> Result<String> {
        let [kind, size, rest @ ..] = line.positional.as_slice() else {
            bail!(
                "write `member kind size x,y,z x,y,z ...`, like `member tube 40x40x3 0,0,0 500,0,0`"
            );
        };
        let mut section = section(kind, size)?;
        let mut closed = false;
        let mut points = Vec::new();
        for word in rest {
            match word.as_str() {
                "closed" => closed = true,
                text => points.push(point3(text, &self.scope)?),
            }
        }
        if points.len() < 2 {
            bail!("a member needs at least two points");
        }
        if closed && points.len() < 3 {
            bail!("a closed frame needs at least three points");
        }
        let mut up = None;
        let mut spin = 0.0;
        for (name, text) in &line.named {
            match name.as_str() {
                "up" => up = Some(point3(text, &self.scope)?.to_vec()),
                "rotate" => spin = crate::parse::eval(text, &self.scope)?,
                "offset" => {
                    section = shifted(section, crate::parse::eval_point(text, &self.scope)?)
                }
                other => bail!("`member` takes up=, rotate= and offset=, not `{other}=`"),
            }
        }
        let count = if closed {
            points.len()
        } else {
            points.len() - 1
        };
        let corner = |i: usize| points[i % points.len()];
        let directions: Vec<Vector3> = (0..count)
            .map(|i| {
                let d = corner(i + 1) - corner(i);
                if d.magnitude() < 1.0e-9 {
                    bail!("a member has two identical points in a row");
                }
                Ok(d.normalize())
            })
            .collect::<Result<_>>()?;
        let plane = (0..count)
            .flat_map(|i| (i + 1..count).map(move |j| (i, j)))
            .map(|(i, j)| directions[i].cross(directions[j]))
            .find(|n| n.magnitude() > 1.0e-6)
            .map(|n| n.normalize());
        let flat = plane.is_some_and(|n| {
            points.iter().all(|p| {
                (p - points[0]).dot(n).abs() < 1.0e-6 * (1.0 + (p - points[0]).magnitude())
            })
        });
        let label = label_of(line);
        let mut said = Vec::new();
        for i in 0..count {
            let along = directions[i];
            let mut lift = match (up, flat.then_some(plane).flatten()) {
                (Some(u), _) => u,
                (None, Some(n)) => {
                    if n.z < -1.0e-9
                        || (n.z.abs() < 1.0e-9 && (n.y < 0.0 || (n.y == 0.0 && n.x < 0.0)))
                    {
                        -n
                    } else {
                        n
                    }
                }
                (None, None) => Vector3::unit_z(),
            };
            lift -= along * lift.dot(along);
            if lift.magnitude() < 1.0e-6 {
                lift = Vector3::unit_y() - along * along.y;
                if lift.magnitude() < 1.0e-6 {
                    lift = Vector3::unit_x() - along * along.x;
                }
            }
            let y0 = lift.normalize();
            let x0 = y0.cross(along);
            let (s, c) = spin.to_radians().sin_cos();
            let (x, y) = (x0 * c + y0 * s, y0 * c - x0 * s);
            let mitre = |at: usize, other: Option<Vector3>| Cut {
                point: corner(at),
                normal: match other {
                    Some(o) => (along + o).normalize(),
                    None => along,
                },
            };
            let before = if i > 0 || closed {
                Some(directions[(i + count - 1) % count])
            } else {
                None
            };
            let after = if i + 1 < count || closed {
                Some(directions[(i + 1) % count])
            } else {
                None
            };
            let (start, end) = (mitre(i, before), mitre(i + 1, after));
            if start.normal.dot(along) < 0.2 || end.normal.dot(along) < 0.2 {
                bail!("the corner at point {} turns too sharply to mitre", i + 1);
            }
            let solid = member_solid(&section, &start, &end, along, x, y)?;
            let length = section
                .reach
                .iter()
                .map(|&(u, v)| {
                    let q = |cut: &Cut| {
                        let p = cut.point + x * u + y * v;
                        let t = -(p - cut.point).dot(cut.normal) / along.dot(cut.normal);
                        (p + along * t - corner(0)).dot(along)
                    };
                    q(&end) - q(&start)
                })
                .fold(0.0, f64::max);
            let angles = [&start, &end]
                .map(|cut| cut.normal.dot(along).clamp(-1.0, 1.0).acos().to_degrees());
            let name = if count == 1 && !self.body_names().contains(&label) {
                label.clone()
            } else {
                let mut k = i + 1;
                let mut name = format!("{label}_{k}");
                while self.body_names().contains(&name) {
                    k += 1;
                    name = format!("{label}_{k}");
                }
                name
            };
            let ends = if angles.iter().all(|a| *a < 1.0e-6) {
                "square ends".to_string()
            } else {
                format!("cut {:.1}/{:.1}", angles[0], angles[1])
            };
            self.sources.insert(
                name.clone(),
                super::Source {
                    file: std::path::PathBuf::from(&section.name),
                    body: format!("{length:.1} long, {ends}"),
                    vars: Vec::new(),
                },
            );
            let groups: Vec<(&str, Surface)> = select::faces(&solid)
                .iter()
                .map(|face| {
                    let surface = face.oriented_surface();
                    let group = match &surface {
                        Surface::Plane(p) if p.normal().dot(along).abs() > 1.0e-6 => "end",
                        _ => "side",
                    };
                    (group, surface)
                })
                .collect();
            self.record(&label, groups);
            self.members.push(name.clone());
            said.push(format!("{name} {length:.1}"));
            self.bodies.push((name, solid));
        }
        Ok(format!("{}: {}", section.name, said.join(", ")))
    }
}
