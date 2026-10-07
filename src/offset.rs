use crate::geometry::Segment;

type P2 = (f64, f64);

fn sub(a: P2, b: P2) -> P2 {
    (a.0 - b.0, a.1 - b.1)
}

fn add(a: P2, b: P2) -> P2 {
    (a.0 + b.0, a.1 + b.1)
}

fn mul(a: P2, k: f64) -> P2 {
    (a.0 * k, a.1 * k)
}

fn dot(a: P2, b: P2) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

fn cross(a: P2, b: P2) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

fn length(a: P2) -> f64 {
    a.0.hypot(a.1)
}

fn unit(a: P2) -> P2 {
    mul(a, 1.0 / length(a))
}

fn circumcentre(a: P2, b: P2, c: P2) -> Option<P2> {
    let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    if d.abs() < 1.0e-14 {
        return None;
    }
    let (a2, b2, c2) = (dot(a, a), dot(b, b), dot(c, c));
    Some((
        (a2 * (b.1 - c.1) + b2 * (c.1 - a.1) + c2 * (a.1 - b.1)) / d,
        (a2 * (c.0 - b.0) + b2 * (a.0 - c.0) + c2 * (b.0 - a.0)) / d,
    ))
}

#[derive(Clone, Copy)]
enum Element {
    Line { point: P2, direction: P2 },
    Circle { centre: P2, radius: f64 },
}

fn meet(a: Element, b: Element, near: P2) -> Option<P2> {
    let closest = |candidates: Vec<P2>| {
        candidates
            .into_iter()
            .min_by(|p, q| length(sub(*p, near)).total_cmp(&length(sub(*q, near))))
    };
    let line_circle = |point: P2, direction: P2, centre: P2, radius: f64| {
        let f = sub(point, centre);
        let (b, c) = (dot(f, direction), dot(f, f) - radius * radius);
        let disc = b * b - c;
        if disc < -1.0e-12 * radius * radius {
            return None;
        }
        let root = disc.max(0.0).sqrt();
        closest(vec![
            add(point, mul(direction, -b - root)),
            add(point, mul(direction, -b + root)),
        ])
    };
    match (a, b) {
        (
            Element::Line {
                point: p,
                direction: d,
            },
            Element::Line {
                point: q,
                direction: e,
            },
        ) => {
            let denominator = cross(d, e);
            if denominator.abs() < 1.0e-12 {
                return None;
            }
            let t = cross(sub(q, p), e) / denominator;
            Some(add(p, mul(d, t)))
        }
        (Element::Line { point, direction }, Element::Circle { centre, radius })
        | (Element::Circle { centre, radius }, Element::Line { point, direction }) => {
            line_circle(point, direction, centre, radius)
        }
        (
            Element::Circle {
                centre: c0,
                radius: r0,
            },
            Element::Circle {
                centre: c1,
                radius: r1,
            },
        ) => {
            let between = sub(c1, c0);
            let d = length(between);
            if d < 1.0e-12 || d > r0 + r1 + 1.0e-9 || d < (r0 - r1).abs() - 1.0e-9 {
                return None;
            }
            let a = (r0 * r0 - r1 * r1 + d * d) / (2.0 * d);
            let h = (r0 * r0 - a * a).max(0.0).sqrt();
            let base = add(c0, mul(between, a / d));
            let across = mul((-between.1 / d, between.0 / d), h);
            closest(vec![add(base, across), sub(base, across)])
        }
    }
}

pub fn offset_path(
    start: P2,
    segments: &[Segment],
    distances: &[f64],
) -> Result<(P2, Vec<Segment>), String> {
    let n = segments.len();
    let ends: Vec<P2> = std::iter::once(start)
        .chain(segments.iter().map(Segment::end))
        .collect();
    let mut outline = Vec::new();
    for (i, segment) in segments.iter().enumerate() {
        outline.push(ends[i]);
        if let Segment::Arc { via, .. } = segment {
            outline.push(*via);
        }
    }
    let area: f64 = (0..outline.len())
        .map(|i| cross(outline[i], outline[(i + 1) % outline.len()]))
        .sum();
    let side = if area > 0.0 { 1.0 } else { -1.0 };
    let inward = |direction: P2| mul((-direction.1, direction.0), side);
    let mut elements = Vec::new();
    let mut tangents = Vec::new();
    for (i, segment) in segments.iter().enumerate() {
        let (from, to, delta) = (ends[i], ends[i + 1], distances[i]);
        match segment {
            Segment::Line(_) => {
                let direction = unit(sub(to, from));
                elements.push(Element::Line {
                    point: add(from, mul(inward(direction), delta)),
                    direction,
                });
                tangents.push((direction, direction));
            }
            Segment::Arc { via, .. } => {
                let centre = circumcentre(from, *via, to).ok_or("an arc is straight")?;
                let radius = length(sub(from, centre));
                let turning_left = cross(sub(*via, from), sub(to, *via)) > 0.0;
                let tangent_at = |p: P2| {
                    let r = sub(p, centre);
                    if turning_left {
                        unit((-r.1, r.0))
                    } else {
                        unit((r.1, -r.0))
                    }
                };
                let moved = length(sub(add(from, mul(inward(tangent_at(from)), delta)), centre));
                if moved < 1.0e-9 * radius.max(1.0) {
                    return Err("the offset closes up an arc".to_string());
                }
                elements.push(Element::Circle {
                    centre,
                    radius: moved,
                });
                tangents.push((tangent_at(from), tangent_at(to)));
            }
            Segment::Cubic { .. } => return Err("offset works on lines and arcs".to_string()),
        }
    }
    let corners: Vec<P2> = (0..n)
        .map(|i| {
            let previous = (i + n - 1) % n;
            let vertex = ends[i];
            let (incoming, outgoing) = (tangents[previous].1, tangents[i].0);
            let (d0, d1) = (distances[previous], distances[i]);
            let guess = add(
                vertex,
                mul(unit(add(inward(incoming), inward(outgoing))), d0.max(d1)),
            );
            if cross(incoming, outgoing).abs() < 1.0e-9 && dot(incoming, outgoing) > 0.0 {
                if (d0 - d1).abs() > 1.0e-12 {
                    return Err("edges that meet smoothly need the same offset".to_string());
                }
                return Ok(add(vertex, mul(inward(outgoing), d1)));
            }
            meet(elements[previous], elements[i], guess)
                .ok_or_else(|| "the offset edges no longer meet".to_string())
        })
        .collect::<Result<_, _>>()?;
    let mut result = Vec::new();
    for (i, segment) in segments.iter().enumerate() {
        let (from, to) = (corners[i], corners[(i + 1) % n]);
        match (segment, elements[i]) {
            (Segment::Line(_), _) => {
                if dot(sub(to, from), sub(ends[i + 1], ends[i])) <= 0.0 {
                    return Err("the offset is too large for the shape".to_string());
                }
                result.push(Segment::Line(to));
            }
            (Segment::Arc { via, .. }, Element::Circle { centre, radius }) => {
                let via = add(centre, mul(unit(sub(*via, centre)), radius));
                if cross(sub(via, from), sub(to, via)).abs() < 1.0e-12 {
                    return Err("the offset is too large for an arc".to_string());
                }
                result.push(Segment::Arc { to, via });
            }
            _ => unreachable!("segments and elements line up"),
        }
    }
    Ok((corners[0], result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_a_d_shape() {
        let segments = vec![
            Segment::Line((30.0, 0.0)),
            Segment::Arc {
                to: (30.0, 20.0),
                via: (40.0, 10.0),
            },
            Segment::Line((0.0, 20.0)),
            Segment::Line((0.0, 0.0)),
        ];
        let (start, grown) = offset_path((0.0, 0.0), &segments, &[-2.0; 4]).unwrap();
        println!("{start:?} {grown:?}");
        assert!(length(sub(start, (-2.0, -2.0))) < 1.0e-9);
    }
}
