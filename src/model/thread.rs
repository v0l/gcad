use super::Model;
use super::args::{Args, label_of, positive};
use super::mate;
use super::skin::{interpolate, knots, uniform};
use crate::geometry;
use crate::parse::Line;
use crate::select;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;
use std::collections::HashSet;
use std::f64::consts::TAU;

pub(crate) const THREADS: &[(&str, f64, f64, f64)] = &[
    ("M2", 2.0, 1.6, 0.4),
    ("M2.5", 2.5, 2.05, 0.45),
    ("M3", 3.0, 2.5, 0.5),
    ("M4", 4.0, 3.3, 0.7),
    ("M5", 5.0, 4.2, 0.8),
    ("M6", 6.0, 5.0, 1.0),
    ("M8", 8.0, 6.8, 1.25),
    ("M10", 10.0, 8.5, 1.5),
    ("M12", 12.0, 10.2, 1.75),
];

const SPANS_PER_TURN: usize = 32;
const END_SAMPLES: usize = 33;

pub(crate) fn thread_named(name: &str) -> Result<(f64, f64, f64)> {
    THREADS
        .iter()
        .find(|(known, ..)| known.eq_ignore_ascii_case(name))
        .map(|&(_, major, tap, pitch)| (major, tap, pitch))
        .ok_or_else(|| {
            anyhow!(
                "unknown thread `{name}`; known: {}",
                THREADS.iter().map(|t| t.0).collect::<Vec<_>>().join(", ")
            )
        })
}

pub(crate) fn basic_profile(major: f64, pitch: f64) -> Vec<(f64, f64)> {
    let crest = major / 2.0;
    let root = crest - 5.0 / 8.0 * 3.0_f64.sqrt() / 2.0 * pitch;
    vec![
        (0.0, crest),
        (pitch / 8.0, crest),
        (pitch * 7.0 / 16.0, root),
        (pitch * 11.0 / 16.0, root),
        (pitch, crest),
    ]
}

fn slope(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.1 - a.1) / (b.0 - a.0)
}

pub(crate) fn clipped(profile: &[(f64, f64)], radius: f64, external: bool) -> Vec<(f64, f64)> {
    let clamp = |r: f64| {
        if external {
            r.min(radius)
        } else {
            r.max(radius)
        }
    };
    let pitch = profile[profile.len() - 1].0 - profile[0].0;
    let mut points = vec![(profile[0].0, clamp(profile[0].1))];
    for pair in profile.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if (a.1 - radius) * (b.1 - radius) < 0.0 {
            let t = (radius - a.1) / (b.1 - a.1);
            points.push((a.0 + (b.0 - a.0) * t, radius));
        }
        points.push((b.0, clamp(b.1)));
    }
    points.dedup_by(|b, a| (b.0 - a.0).abs() < pitch * 1.0e-9);
    let same = |x: f64, y: f64| (x - y).abs() < 1.0e-9;
    let mut merged: Vec<(f64, f64)> = vec![points[0]];
    for &point in &points[1..] {
        if merged.len() >= 2 {
            let before = merged[merged.len() - 2];
            let last = merged[merged.len() - 1];
            if same(slope(before, last), slope(last, point)) {
                merged.pop();
            }
        }
        merged.push(point);
    }
    let n = merged.len() - 1;
    if n >= 2 && same(slope(merged[n - 1], merged[n]), slope(merged[0], merged[1])) {
        let next = merged[1];
        merged.remove(0);
        merged.pop();
        merged.push((next.0 + pitch, next.1));
    }
    merged
}

struct Helix {
    origin: Point3,
    along: Vector3,
    x: Vector3,
    y: Vector3,
    pitch: f64,
}

impl Helix {
    fn at(&self, theta: f64, s: f64, r: f64) -> Point3 {
        self.origin
            + (self.x * theta.cos() + self.y * theta.sin()) * r
            + self.along * (s + self.pitch * theta / TAU)
    }

    fn radial(&self, p: Point3) -> Vector3 {
        let d = p - self.origin;
        d - self.along * self.along.dot(d)
    }
}

struct Ends {
    axis: Vector3,
    centre: Point3,
    radius: f64,
    bottom: f64,
    top: f64,
}

fn edge_points(edge: &Edge) -> Vec<Point3> {
    let curve = edge.oriented_curve();
    let (t0, t1) = curve.range_tuple();
    (0..=8)
        .map(|i| curve.subs(t0 + (t1 - t0) * i as f64 / 8.0))
        .collect()
}

fn plane_normal(points: &[Point3]) -> Vector3 {
    let first = points[0];
    let far = |from: &dyn Fn(Point3) -> f64| {
        points
            .iter()
            .copied()
            .max_by(|a, b| from(*a).total_cmp(&from(*b)))
            .expect("points")
    };
    let second = far(&|p| p.distance(first));
    let chord = (second - first).normalize();
    let third = far(&|p| (p - first).cross(chord).magnitude());
    (second - first).cross(third - first)
}

fn ends_of(edges: &[Edge], rough: &mate::Cylinder, tolerance: f64) -> Result<Ends> {
    let points: Vec<Point3> = edges.iter().flat_map(edge_points).collect();
    let height = |p: &Point3| rough.axis.dot(p.to_vec());
    let (low, high) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
            (a.min(height(p)), b.max(height(p)))
        });
    let middle = (low + high) / 2.0;
    let bottom: Vec<Point3> = points
        .iter()
        .copied()
        .filter(|p| height(p) < middle)
        .collect();
    let axis = plane_normal(&bottom);
    let axis = if axis.dot(rough.axis) < 0.0 {
        -axis
    } else {
        axis
    }
    .normalize();
    let u = axis
        .cross(if axis.x.abs() < 0.9 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        })
        .normalize();
    let v = axis.cross(u);
    let flat: Vec<(f64, f64)> = bottom
        .iter()
        .map(|p| (p.to_vec().dot(u), p.to_vec().dot(v)))
        .collect();
    let ((cu, cv), radius, _) =
        mate::fit_circle(&flat).ok_or_else(|| anyhow!("the round face's ends are not circles"))?;
    let centre = Point3::from_vec(u * cu + v * cv);
    let slack = tolerance * 100.0 + radius * 1.0e-6;
    let along = |p: &Point3| axis.dot(*p - centre);
    let bottom_at = bottom.iter().map(along).sum::<f64>() / bottom.len() as f64;
    let top: Vec<f64> = points
        .iter()
        .filter(|p| height(p) >= middle)
        .map(along)
        .collect();
    let top_at = top.iter().sum::<f64>() / top.len() as f64;
    let flat_ends = points.iter().all(|p| {
        let a = along(p);
        let off = ((*p - centre) - axis * a).magnitude() - radius;
        ((a - bottom_at).abs() < slack || (a - top_at).abs() < slack) && off.abs() < slack
    });
    if !flat_ends {
        bail!("a thread goes on a plain round face that runs between two flat ends");
    }
    Ok(Ends {
        axis,
        centre,
        radius,
        bottom: bottom_at,
        top: top_at,
    })
}

fn turning(points: &[Point3], centre: Point3, axis: Vector3) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            (points[i] - centre)
                .cross(points[(i + 1) % n] - centre)
                .dot(axis)
        })
        .sum()
}

fn wire_points(wire: &Wire) -> Vec<Point3> {
    wire.edge_iter()
        .flat_map(|e| {
            let mut points = edge_points(e);
            points.pop();
            points
        })
        .collect()
}

fn capped(cap: &Face, end: &HashSet<EdgeId>, new_loop: &Wire, helix: &Helix) -> Result<Face> {
    let mut replaced = false;
    let wires = cap
        .absolute_boundaries()
        .iter()
        .map(|wire| {
            if !wire.edge_iter().all(|e| end.contains(&e.id())) {
                return wire.clone();
            }
            replaced = true;
            let old = turning(&wire_points(wire), helix.origin, helix.along);
            let new = turning(&wire_points(new_loop), helix.origin, helix.along);
            if old * new > 0.0 {
                new_loop.clone()
            } else {
                new_loop.inverse()
            }
        })
        .collect::<Vec<Wire>>();
    if !replaced {
        bail!("a thread's flat end must meet the round face along a whole loop");
    }
    let mut face = Face::try_new(wires, cap.surface())
        .map_err(|error| anyhow!("threaded end face: {error}"))?;
    if !cap.orientation() {
        face.invert();
    }
    Ok(face)
}

struct Rail {
    params: Vec<f64>,
    vertices: Vec<Vertex>,
    edges: Vec<Edge>,
}

impl Rail {
    fn new(curve: &BsplineCurve<Point3>, params: Vec<f64>) -> Rail {
        let vertices: Vec<Vertex> = params
            .iter()
            .map(|&t| builder::vertex(curve.subs(t)))
            .collect();
        let edges = params
            .windows(2)
            .enumerate()
            .map(|(i, pair)| {
                let mut whole = curve.clone();
                let mut piece = whole.cut(pair[0]);
                piece.cut(pair[1]);
                Edge::new(&vertices[i], &vertices[i + 1], Curve::BsplineCurve(piece))
            })
            .collect();
        Rail {
            params,
            vertices,
            edges,
        }
    }

    fn index(&self, theta: f64) -> usize {
        self.params
            .iter()
            .position(|t| (t - theta).abs() < 1.0e-9)
            .expect("a vertex on the rail")
    }

    fn vertex(&self, theta: f64) -> &Vertex {
        &self.vertices[self.index(theta)]
    }

    fn run(&self, from: f64, to: f64) -> Vec<Edge> {
        self.edges[self.index(from)..self.index(to)].to_vec()
    }
}

pub(crate) struct Threaded {
    pub(crate) solid: Solid,
    pub(crate) strips: Vec<Surface>,
    pub(crate) external: bool,
    pub(crate) length: f64,
}

pub(crate) fn thread_faces(
    solid: &Solid,
    picked: &[usize],
    major: f64,
    pitch: f64,
    left: bool,
    tolerance: f64,
) -> Result<Threaded> {
    let faces = select::faces(solid);
    let owners = select::edge_owners(&faces);
    let mesh_tolerance = geometry::mesh_tolerance(solid) * 0.5;
    let (rough, points, normals) = mate::fit_face(&faces[picked[0]], mesh_tolerance)
        .ok_or_else(|| anyhow!("`on=` must pick a round face"))?;
    for &i in &picked[1..] {
        let (other, ..) = mate::fit_face(&faces[i], mesh_tolerance)
            .ok_or_else(|| anyhow!("`on=` must pick a round face"))?;
        let d = other.point - rough.point;
        let offset = (d - rough.axis * d.dot(rough.axis)).magnitude();
        if rough.axis.cross(other.axis).magnitude() > 1.0e-3
            || offset > rough.radius * 1.0e-3
            || (other.radius - rough.radius).abs() > rough.radius * 1.0e-3
        {
            bail!("`on=` must pick one round face, or the pieces of one");
        }
    }
    let outward = points
        .iter()
        .zip(&normals)
        .map(|(p, n)| {
            let d = *p - rough.point;
            n.dot(d - rough.axis * d.dot(rough.axis))
        })
        .sum::<f64>();
    let external = outward > 0.0;
    let mine: HashSet<usize> = picked.iter().copied().collect();
    let mut end_edges: Vec<Edge> = Vec::new();
    let mut caps: Vec<usize> = Vec::new();
    for &i in picked {
        for edge in faces[i].edge_iter() {
            let others: Vec<usize> = owners[&edge.id()]
                .iter()
                .copied()
                .filter(|f| !mine.contains(f))
                .collect();
            if others.is_empty() {
                continue;
            }
            if !end_edges.iter().any(|e| e.is_same(&edge)) {
                end_edges.push(edge.clone());
            }
            for other in others {
                if !caps.contains(&other) {
                    caps.push(other);
                }
            }
        }
    }
    let ends = ends_of(&end_edges, &rough, tolerance)?;
    let crest = major / 2.0;
    let profile = basic_profile(major, pitch);
    let root = profile[2].1;
    let slack = ends.radius * 1.0e-4;
    let radius = if (ends.radius - if external { crest } else { root }).abs() < slack {
        if external { crest } else { root }
    } else {
        ends.radius
    };
    if external && (radius > crest || radius <= root) {
        bail!(
            "an M{major} thread needs a rod between {:.3} and {major} across, this one is {:.3}",
            root * 2.0,
            radius * 2.0
        );
    }
    if !external && (radius < root || radius >= crest) {
        bail!(
            "an M{major} thread needs a hole between {:.3} and {major} across, this one is {:.3}",
            root * 2.0,
            radius * 2.0
        );
    }
    let profile = clipped(&profile, radius, external);
    let n = profile.len() - 1;
    let x = {
        let seed = if ends.axis.x.abs() < 0.9 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        };
        (seed - ends.axis * ends.axis.dot(seed)).normalize()
    };
    let y = if left {
        x.cross(ends.axis)
    } else {
        ends.axis.cross(x)
    };
    let helix = Helix {
        origin: ends.centre + ends.axis * ends.bottom,
        along: ends.axis,
        x,
        y,
        pitch,
    };
    let length = ends.top - ends.bottom;
    for &cap in &caps {
        let flat = match faces[cap].oriented_surface() {
            Surface::Plane(plane) => plane.normal().cross(ends.axis).magnitude() < 1.0e-6,
            _ => false,
        };
        if !flat {
            bail!("a thread's ends must be flat faces square to its axis");
        }
    }
    if caps.len() != 2 {
        bail!("a thread goes on a round face with one flat face at each end");
    }

    let step = TAU / SPANS_PER_TURN as f64;
    let lowest = -TAU * profile[n].0 / pitch - TAU;
    let highest = TAU * (length - profile[0].0) / pitch + TAU;
    let start = (lowest / step).floor() * step;
    let spans = ((highest - start) / step).ceil() as usize;
    let end = start + spans as f64 * step;
    let mut knot_values = vec![start; 4];
    knot_values.extend((1..spans).map(|k| start + k as f64 * step));
    knot_values.extend(vec![end; 4]);
    let theta_knots = KnotVector::from(knot_values.clone());
    let scale = 6.0 / (4.0 + 2.0 * step.cos());
    let rails: Vec<Vec<Point3>> = profile
        .iter()
        .map(|&(s, r)| {
            (0..spans + 3)
                .map(|k| {
                    let greville = knot_values[k + 1..k + 4].iter().sum::<f64>() / 3.0;
                    helix.at(greville, s, r * scale)
                })
                .collect()
        })
        .collect();
    let surfaces: Vec<BsplineSurface<Point3>> = (0..n)
        .map(|k| {
            BsplineSurface::new(
                (theta_knots.clone(), KnotVector::bezier_knot(1)),
                (0..spans + 3)
                    .map(|m| vec![rails[k][m], rails[k + 1][m]])
                    .collect(),
            )
        })
        .collect();
    let rail_curve = |j: usize| BsplineCurve::new(theta_knots.clone(), rails[j].clone());
    let snap = |theta: f64| {
        let k = ((theta - start) / step).round();
        if (theta - (start + k * step)).abs() < step * 1.0e-9 {
            knot_values[k as usize + 3]
        } else {
            theta
        }
    };
    let low_at = |j: usize| snap(-TAU * profile[j].0 / pitch);
    let high_at = |j: usize| snap(TAU * (length - profile[j].0) / pitch);
    let cuts: Vec<f64> = (1..)
        .map(|m| snap(low_at(0) + TAU * m as f64))
        .take_while(|&theta| theta < high_at(n) - step)
        .collect();
    let shift = |k: usize| if k + 1 == n { TAU } else { 0.0 };
    let rail_runs: Vec<Rail> = (0..n)
        .map(|j| {
            let wrapped = cuts
                .iter()
                .map(|c| snap(c + if j == 0 { TAU } else { 0.0 }));
            let mut params: Vec<f64> = cuts
                .iter()
                .copied()
                .chain(wrapped)
                .filter(|&t| t > low_at(j) + step && t < high_at(j) - step)
                .collect();
            params.sort_by(f64::total_cmp);
            params.dedup_by(|b, a| (*b - *a).abs() < 1.0e-9);
            params.insert(0, low_at(j));
            params.push(high_at(j));
            Rail::new(&rail_curve(j), params)
        })
        .collect();
    let pcurve = |surface: &BsplineSurface<Point3>, from: Point2, to: Point2| -> Result<Curve> {
        let params = uniform(END_SAMPLES);
        let points: Vec<Point3> = params
            .iter()
            .map(|&t| {
                let uv = from + (to - from) * t;
                surface.subs(uv.x, uv.y)
            })
            .collect();
        let knots = knots(&params, 3);
        let control = interpolate(&points, &params, &knots)?;
        Ok(Curve::BsplineCurve(BsplineCurve::new(knots, control)))
    };
    let mut bottom_edges = Vec::new();
    let mut top_edges = Vec::new();
    let mut strips = Vec::new();
    for k in 0..n {
        let upper = &rail_runs[(k + 1) % n];
        let lower = &rail_runs[k];
        let lifted = shift(k);
        let mut bounds = vec![low_at(k + 1)];
        bounds.extend(cuts.iter().copied());
        bounds.push(high_at(k));
        let columns: Vec<Edge> = cuts
            .iter()
            .map(|&theta| {
                let (from, to) = (lower.vertex(theta), upper.vertex(theta + lifted));
                let curve =
                    BsplineCurve::new(KnotVector::bezier_knot(1), vec![from.point(), to.point()]);
                Edge::new(from, to, Curve::BsplineCurve(curve))
            })
            .collect();
        let last = bounds.len() - 2;
        for f in 0..=last {
            let (a, b) = (bounds[f], bounds[f + 1]);
            let mut surface = surfaces[k].clone();
            let mut piece = surface.cut_u(a);
            piece.cut_u(b);
            let mut edges = lower.run(a.max(low_at(k)), b.min(high_at(k)));
            if f == last {
                let top = Edge::new(
                    lower.vertex(high_at(k)),
                    upper.vertex(high_at(k + 1) + lifted),
                    pcurve(
                        &piece,
                        Point2::new(high_at(k), 0.0),
                        Point2::new(high_at(k + 1), 1.0),
                    )?,
                );
                edges.push(top.clone());
                top_edges.push(top);
            } else {
                edges.push(columns[f].clone());
            }
            edges.extend(
                upper
                    .run(
                        a.max(low_at(k + 1)) + lifted,
                        b.min(high_at(k + 1)) + lifted,
                    )
                    .into_iter()
                    .rev()
                    .map(|e| e.inverse()),
            );
            if f == 0 {
                let bottom = Edge::new(
                    lower.vertex(low_at(k)),
                    upper.vertex(low_at(k + 1) + lifted),
                    pcurve(
                        &piece,
                        Point2::new(low_at(k), 0.0),
                        Point2::new(low_at(k + 1), 1.0),
                    )?,
                );
                edges.push(bottom.inverse());
                bottom_edges.push(bottom);
            } else {
                edges.push(columns[f - 1].inverse());
            }
            let middle = (a + b) / 2.0;
            let surface = Surface::BsplineSurface(piece);
            let normal = surface.normal(middle, 0.5);
            let mut face = Face::try_new(vec![edges.into()], surface.clone())
                .map_err(|error| anyhow!("thread face: {error}"))?;
            if (normal.dot(helix.radial(surface.subs(middle, 0.5))) > 0.0) != external {
                face.invert();
            }
            strips.push(face);
        }
    }
    let bottom_loop: Wire = bottom_edges.into();
    let top_loop: Wire = top_edges.into();
    let end_ids: HashSet<EdgeId> = end_edges.iter().map(|e| e.id()).collect();
    let height = |face: &Face| {
        let p = face
            .vertex_iter()
            .next()
            .expect("a face has vertices")
            .point();
        helix.along.dot(p - helix.origin)
    };
    let picked_ids: HashSet<FaceId> = picked.iter().map(|&i| faces[i].id()).collect();
    let cap_ids: Vec<FaceId> = caps.iter().map(|&i| faces[i].id()).collect();
    let mut shells = Vec::new();
    for shell in solid.boundaries() {
        let mut kept: Vec<Face> = Vec::new();
        let mut touched = false;
        for face in shell.face_iter() {
            if picked_ids.contains(&face.id()) {
                touched = true;
            } else if cap_ids.contains(&face.id()) {
                let new_loop = if height(face).abs() < (length / 2.0).abs() {
                    &bottom_loop
                } else {
                    &top_loop
                };
                kept.push(capped(face, &end_ids, new_loop, &helix)?);
            } else {
                kept.push(face.clone());
            }
        }
        if touched {
            kept.extend(strips.iter().cloned());
        }
        shells.push(Shell::from(kept));
    }
    let solid = Solid::try_new(shells)
        .map_err(|error| anyhow!("the threaded solid is not closed: {error}"))?;
    Ok(Threaded {
        solid,
        strips: strips.iter().map(|f| f.oriented_surface()).collect(),
        external,
        length,
    })
}

impl Model {
    pub(crate) fn op_thread(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["size"], &["on", "pitch"], true)?;
        let left = match args.rest.as_slice() {
            [] => false,
            ["left"] => true,
            _ => bail!("`thread` takes `left` for a left-hand thread"),
        };
        let name = args.text("size")?;
        let (major, _, standard) = thread_named(name)?;
        let pitch = match args.optional_number("pitch", &self.scope)? {
            Some(p) => positive(p, "pitch")?,
            None => standard,
        };
        let selector = args.text("on")?;
        let solid = self.active("thread")?.clone();
        let tolerance = self.tolerance();
        let picked = select::select_faces(selector, &solid, &self.groups, tolerance)?;
        if picked.is_empty() {
            bail!("`{selector}` matched no faces");
        }
        let threaded = thread_faces(&solid, &picked, major, pitch, left, tolerance)?;
        let label = label_of(line);
        threaded
            .strips
            .into_iter()
            .for_each(|surface| self.groups.record(&label, "faces", surface));
        self.solid = Some(threaded.solid);
        Ok(format!(
            "{} {} x {pitch} thread, {:.3} long; {}",
            if threaded.external {
                "outside"
            } else {
                "inside"
            },
            name.to_uppercase(),
            threaded.length,
            self.describe_solid()?
        ))
    }
}
