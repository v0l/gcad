use super::solids::oriented;
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

struct Piece {
    curve: Curve,
    from: f64,
    to: f64,
}

impl Piece {
    fn at(&self, t: f64) -> Point3 {
        self.curve.subs(self.from + (self.to - self.from) * t)
    }

    fn straight(&self) -> bool {
        matches!(self.curve, Curve::Line(_))
    }

    fn by_length(&self, count: usize) -> Vec<Point3> {
        let dense: Vec<Point3> = (0..=128).map(|i| self.at(i as f64 / 128.0)).collect();
        let lengths: Vec<f64> = std::iter::once(0.0)
            .chain(dense.windows(2).scan(0.0, |sum, pair| {
                *sum += pair[0].distance(pair[1]);
                Some(*sum)
            }))
            .collect();
        let total = *lengths.last().expect("non-empty");
        (0..=count)
            .map(|i| {
                let target = total * i as f64 / count as f64;
                let k = lengths
                    .windows(2)
                    .position(|w| w[1] >= target)
                    .unwrap_or(127);
                let span = (lengths[k + 1] - lengths[k]).max(1.0e-300);
                let t = (k as f64 + ((target - lengths[k]) / span).clamp(0.0, 1.0)) / 128.0;
                self.at(t)
            })
            .collect()
    }

    fn split(self, parts: usize) -> Vec<Piece> {
        if parts == 1 {
            return vec![self];
        }
        let marks = self.by_length(parts);
        let dense: Vec<f64> = (0..=512).map(|i| i as f64 / 512.0).collect();
        let param = |p: Point3| {
            dense
                .iter()
                .copied()
                .min_by(|a, b| self.at(*a).distance(p).total_cmp(&self.at(*b).distance(p)))
                .expect("non-empty")
        };
        let cuts: Vec<f64> = marks.iter().map(|p| param(*p)).collect();
        (0..parts)
            .map(|i| {
                let (a, b) = (
                    if i == 0 { 0.0 } else { cuts[i] },
                    if i + 1 == parts { 1.0 } else { cuts[i + 1] },
                );
                Piece {
                    curve: self.curve.clone(),
                    from: self.from + (self.to - self.from) * a,
                    to: self.from + (self.to - self.from) * b,
                }
            })
            .collect()
    }
}

fn pieces(wire: &Wire) -> Vec<Piece> {
    wire.edge_iter()
        .map(|edge| {
            let curve = edge.oriented_curve();
            let (from, to) = curve.range_tuple();
            Piece { curve, from, to }
        })
        .collect()
}

fn newell(points: &[Point3]) -> Vector3 {
    (0..points.len()).fold(Vector3::new(0.0, 0.0, 0.0), |n, i| {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        n + Vector3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        )
    })
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

pub(crate) fn knots(params: &[f64], degree: usize) -> KnotVector {
    let n = params.len();
    let mut values = vec![0.0; degree + 1];
    values
        .extend((1..n - degree).map(|j| params[j..j + degree].iter().sum::<f64>() / degree as f64));
    values.extend(vec![1.0; degree + 1]);
    KnotVector::from(values)
}

pub(crate) fn interpolate(
    points: &[Point3],
    params: &[f64],
    knots: &KnotVector,
) -> Result<Vec<Point3>> {
    if points.len() == 2 && knots.len() == 4 {
        return Ok(points.to_vec());
    }
    let pairs: Vec<(f64, Point3)> = params.iter().copied().zip(points.iter().copied()).collect();
    BsplineCurve::try_interpolate(knots.clone(), pairs)
        .map(|curve| curve.control_points().clone())
        .map_err(|e| anyhow!("cannot fit the loft surface: {e}"))
}

pub(crate) fn uniform(count: usize) -> Vec<f64> {
    (0..count).map(|i| i as f64 / (count - 1) as f64).collect()
}

pub(crate) fn skin(sections: &[Wire], smooth: bool) -> Result<Solid> {
    if sections.len() < 2 {
        bail!("a loft needs at least two sections");
    }
    let counts: Vec<usize> = sections.iter().map(|w| w.len()).collect();
    let common = counts.iter().fold(1, |l, &c| l / gcd(l, c) * c);
    if common > 240 {
        bail!("sections have {counts:?} edges, too far apart to match up");
    }
    let mixed = counts.iter().any(|&c| c != counts[0]);
    let mut rings: Vec<Vec<Piece>> = sections
        .iter()
        .map(|wire| {
            let parts = common / wire.len();
            pieces(wire)
                .into_iter()
                .flat_map(|p| p.split(parts))
                .collect()
        })
        .collect();
    let corners = |ring: &[Piece]| -> Vec<Point3> { ring.iter().map(|p| p.at(0.0)).collect() };
    let outline = |ring: &[Piece]| -> Vec<Point3> {
        ring.iter()
            .flat_map(|p| (0..8).map(move |i| p.at(i as f64 / 8.0)))
            .collect()
    };
    for s in 1..rings.len() {
        let reference = newell(&outline(&rings[s - 1]));
        let ring = &mut rings[s];
        if newell(&outline(ring)).dot(reference) < 0.0 {
            ring.reverse();
            ring.iter_mut()
                .for_each(|p| std::mem::swap(&mut p.from, &mut p.to));
        }
    }
    if mixed {
        for s in 1..rings.len() {
            let centre = |points: &[Point3]| {
                Point3::from_vec(
                    points.iter().map(|p| p.to_vec()).sum::<Vector3>() / points.len() as f64,
                )
            };
            let (before, here) = (corners(&rings[s - 1]), corners(&rings[s]));
            let (cb, ch) = (centre(&before), centre(&here));
            let shift = (0..common)
                .min_by(|&a, &b| {
                    let cost = |k: usize| -> f64 {
                        (0..common)
                            .map(|i| ((before[i] - cb) - (here[(i + k) % common] - ch)).magnitude())
                            .sum()
                    };
                    cost(a).total_cmp(&cost(b))
                })
                .unwrap_or(0);
            rings[s].rotate_left(shift);
        }
    }
    let count = rings.len();
    let centres: Vec<Point3> = rings
        .iter()
        .map(|ring| {
            let o = outline(ring);
            Point3::from_vec(o.iter().map(|p| p.to_vec()).sum::<Vector3>() / o.len() as f64)
        })
        .collect();
    let mut v_params = vec![0.0];
    for pair in centres.windows(2) {
        v_params.push(v_params.last().expect("non-empty") + pair[0].distance(pair[1]).max(1.0e-9));
    }
    let total = *v_params.last().expect("non-empty");
    v_params.iter_mut().for_each(|v| *v /= total);
    let v_degree = if smooth { 3.min(count - 1) } else { 1 };
    let v_knots = knots(&v_params, v_degree);
    let mut nets = Vec::new();
    for j in 0..common {
        let straight = rings.iter().all(|ring| ring[j].straight());
        let samples = if straight { 1 } else { 16 };
        let u_params = uniform(samples + 1);
        let u_degree = if straight { 1 } else { 3 };
        let u_knots = knots(&u_params, u_degree);
        let rows = rings
            .iter()
            .map(|ring| {
                let mut row = ring[j].by_length(samples);
                row[0] = ring[j].at(0.0);
                row[samples] = ring[(j + 1) % common].at(0.0);
                interpolate(&row, &u_params, &u_knots)
            })
            .collect::<Result<Vec<_>>>()?;
        let net = (0..rows[0].len())
            .map(|i| {
                let column: Vec<Point3> = rows.iter().map(|row| row[i]).collect();
                interpolate(&column, &v_params, &v_knots)
            })
            .collect::<Result<Vec<_>>>()?;
        nets.push((u_knots, net));
    }
    let vertices: Vec<[Vertex; 2]> = (0..common)
        .map(|j| {
            let column = &nets[j].1[0];
            [
                Vertex::new(column[0]),
                Vertex::new(*column.last().expect("non-empty")),
            ]
        })
        .collect();
    let sides: Vec<Edge> = (0..common)
        .map(|j| {
            let column = nets[j].1[0].clone();
            let curve = BsplineCurve::new(v_knots.clone(), column);
            Edge::new(&vertices[j][0], &vertices[j][1], Curve::BsplineCurve(curve))
        })
        .collect();
    let ring_edges = |end: usize| -> Vec<Edge> {
        (0..common)
            .map(|j| {
                let (u_knots, net) = &nets[j];
                let row: Vec<Point3> = net
                    .iter()
                    .map(|column| {
                        if end == 0 {
                            column[0]
                        } else {
                            *column.last().expect("non-empty")
                        }
                    })
                    .collect();
                let curve = BsplineCurve::new(u_knots.clone(), row);
                Edge::new(
                    &vertices[j][end],
                    &vertices[(j + 1) % common][end],
                    Curve::BsplineCurve(curve),
                )
            })
            .collect()
    };
    let (bottom, top) = (ring_edges(0), ring_edges(1));
    let mut faces: Vec<Face> = (0..common)
        .map(|j| {
            let (u_knots, net) = &nets[j];
            let surface = BsplineSurface::new((u_knots.clone(), v_knots.clone()), net.clone());
            let wire: Wire = vec![
                bottom[j].clone(),
                sides[(j + 1) % common].clone(),
                top[j].inverse(),
                sides[j].inverse(),
            ]
            .into();
            Face::new(vec![wire], Surface::BsplineSurface(surface))
        })
        .collect();
    let cap = |edges: Vec<Edge>| -> Result<Face> {
        let points: Vec<Point3> = edges
            .iter()
            .flat_map(|e| {
                let c = e.oriented_curve();
                let (t0, t1) = c.range_tuple();
                (0..8)
                    .map(move |i| c.subs(t0 + (t1 - t0) * i as f64 / 8.0))
                    .collect::<Vec<_>>()
            })
            .collect();
        let normal = newell(&points).normalize();
        let origin = points[0];
        let x = (points[points.len() / 3] - origin).normalize();
        let x = (x - normal * x.dot(normal)).normalize();
        let plane = Plane::new(origin, origin + x, origin + normal.cross(x));
        Face::try_new(vec![edges.into()], Surface::Plane(plane))
            .map_err(|e| anyhow!("cannot cap the loft: {e}"))
    };
    let start: Vec<Edge> = bottom.iter().rev().map(Edge::inverse).collect();
    faces.push(cap(start)?);
    faces.push(cap(top)?);
    let solid = Solid::try_new(vec![faces.into()])
        .map_err(|e| anyhow!("lofted solid is not closed: {e}"))?;
    Ok(oriented(solid))
}
