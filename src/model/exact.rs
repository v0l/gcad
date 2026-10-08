use super::mate;
use monstertruck::modeling::*;
use std::collections::HashMap;
use std::f64::consts::FRAC_PI_2;

fn polyline(curve: &Curve) -> Option<Vec<Point3>> {
    match curve {
        Curve::BsplineCurve(b) if b.degree() == 1 => Some(b.control_points().clone()),
        Curve::IntersectionCurve(i) => polyline(i.leader()),
        _ => None,
    }
}

fn spread(points: &[Point3]) -> (Point3, Point3, Point3) {
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
    (first, second, third)
}

fn arc(
    centre: Point3,
    radius: f64,
    u: Vector3,
    w: Vector3,
    sweep: f64,
    ends: (Point3, Point3),
) -> Curve {
    let spans = (sweep.abs() / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = sweep / spans as f64;
    let half = (step / 2.0).cos();
    let at = |angle: f64, r: f64| centre + (u * angle.cos() + w * angle.sin()) * r;
    let mut points = vec![at(0.0, radius).to_homogeneous()];
    let mut knots = vec![0.0; 3];
    for i in 0..spans {
        let a = step * i as f64;
        let middle = at(a + step / 2.0, radius / half);
        points.push((middle.to_vec() * half).extend(half));
        points.push(at(a + step, radius).to_homogeneous());
        if i + 1 < spans {
            knots.extend([(i + 1) as f64; 2]);
        }
    }
    knots.extend([spans as f64; 3]);
    let last = points.len() - 1;
    points[0] = ends.0.to_homogeneous();
    points[last] = ends.1.to_homogeneous();
    Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
        KnotVector::from(knots),
        points,
    )))
}

fn exact_curve(edge: &Edge) -> Option<Curve> {
    let mut points = polyline(&edge.curve())?;
    let (front, back) = (edge.absolute_front().point(), edge.absolute_back().point());
    if points.len() < 3 {
        return None;
    }
    if points[0].distance(front) > points[points.len() - 1].distance(front) {
        points.reverse();
    }
    let size = points
        .iter()
        .map(|p| p.distance(points[0]))
        .fold(0.0, f64::max);
    let tolerance = size.max(1.0) * 1.0e-9;
    let chord = back - front;
    if chord.magnitude() > tolerance
        && points
            .iter()
            .all(|p| (*p - front).cross(chord.normalize()).magnitude() < tolerance)
    {
        return Some(Curve::Line(Line(front, back)));
    }
    let (a, b, c) = spread(&points);
    let normal = (b - a).cross(c - a);
    if normal.magnitude() < tolerance * size {
        return None;
    }
    let normal = normal.normalize();
    if points.iter().any(|p| normal.dot(*p - a).abs() > tolerance) {
        return None;
    }
    let u = (b - a).normalize();
    let v = normal.cross(u);
    let flat: Vec<(f64, f64)> = points
        .iter()
        .map(|p| ((*p - a).dot(u), (*p - a).dot(v)))
        .collect();
    let ((cu, cv), radius, worst) = mate::fit_circle(&flat)?;
    if worst > tolerance {
        return None;
    }
    let centre = a + u * cu + v * cv;
    let start = (front - centre).normalize();
    let side = normal.cross(start);
    let mut sweep = 0.0;
    let mut previous = 0.0;
    for p in &points[1..] {
        let d = *p - centre;
        let angle = d.dot(side).atan2(d.dot(start));
        let mut step = angle - previous;
        if step > std::f64::consts::PI {
            step -= std::f64::consts::TAU;
        } else if step < -std::f64::consts::PI {
            step += std::f64::consts::TAU;
        }
        sweep += step;
        previous = angle;
    }
    if sweep.abs() < 1.0e-6 {
        return None;
    }
    Some(arc(centre, radius, start, side, sweep, (front, back)))
}

pub(crate) fn exact_edges(solid: &Solid) -> Solid {
    let mut replaced: HashMap<EdgeId, Edge> = HashMap::new();
    for shell in solid.boundaries() {
        for edge in shell.edge_iter() {
            if replaced.contains_key(&edge.id()) {
                continue;
            }
            if let Some(curve) = exact_curve(&edge) {
                let fresh = Edge::new(edge.absolute_front(), edge.absolute_back(), curve);
                replaced.insert(edge.id(), fresh);
            }
        }
    }
    if replaced.is_empty() {
        return solid.clone();
    }
    let swap = |e: &Edge| match replaced.get(&e.id()) {
        Some(fresh) if e.orientation() => fresh.clone(),
        Some(fresh) => fresh.inverse(),
        None => e.clone(),
    };
    let shells: Option<Vec<Shell>> = solid
        .boundaries()
        .iter()
        .map(|shell| {
            shell
                .face_iter()
                .map(|face| {
                    let wires: Vec<Wire> = face
                        .absolute_boundaries()
                        .iter()
                        .map(|wire| wire.edge_iter().map(swap).collect())
                        .collect();
                    let mut rebuilt = Face::try_new(wires, face.surface()).ok()?;
                    if !face.orientation() {
                        rebuilt.invert();
                    }
                    Some(rebuilt)
                })
                .collect::<Option<Vec<Face>>>()
                .map(Shell::from)
        })
        .collect();
    shells
        .and_then(|shells| Solid::try_new(shells).ok())
        .unwrap_or_else(|| solid.clone())
}
