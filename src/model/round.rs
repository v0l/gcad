use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

type Weighted = (Point3, f64);

fn homogeneous((point, weight): Weighted) -> Vector4 {
    (point.to_vec() * weight).extend(weight)
}

fn arc_points(
    from: Point3,
    to: Point3,
    centre: Point3,
    middle_dir: Vector3,
    cos: f64,
) -> [Weighted; 3] {
    [
        (from, 1.0),
        (centre + middle_dir, ((1.0 + cos) / 2.0).sqrt()),
        (to, 1.0),
    ]
}

fn arc_curve(from: Point3, to: Point3, centre: Point3, middle_dir: Vector3, cos: f64) -> Curve {
    Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
        KnotVector::bezier_knot(2),
        arc_points(from, to, centre, middle_dir, cos)
            .map(homogeneous)
            .to_vec(),
    )))
}

fn circular_points(from: Point3, to: Point3, centre: Point3) -> [Weighted; 3] {
    let (a, b) = (from - centre, to - centre);
    let cos = a.normalize().dot(b.normalize()).clamp(-1.0, 1.0);
    arc_points(from, to, centre, (a + b) / (1.0 + cos), cos)
}

fn circular_arc(from: Point3, to: Point3, centre: Point3) -> Curve {
    let (a, b) = (from - centre, to - centre);
    let cos = a.normalize().dot(b.normalize()).clamp(-1.0, 1.0);
    arc_curve(from, to, centre, (a + b) / (1.0 + cos), cos)
}

fn revolved(
    profile: [Weighted; 3],
    origin: Point3,
    axis: Vector3,
    from: Vector3,
    to: Vector3,
) -> NurbsSurface<Vector4> {
    let cos = from.dot(to).clamp(-1.0, 1.0);
    let sin = from.cross(to).magnitude();
    let turn = from.cross(to).normalize();
    let half = ((1.0 + cos) / 2.0).sqrt();
    let rows = profile
        .into_iter()
        .map(|(point, weight)| {
            let centre = origin + axis * axis.dot(point - origin);
            let start = point - centre;
            let end = start * cos + turn.cross(start) * sin;
            vec![
                homogeneous((point, weight)),
                homogeneous((centre + (start + end) / (1.0 + cos), weight * half)),
                homogeneous((centre + end, weight)),
            ]
        })
        .collect();
    NurbsSurface::new(BsplineSurface::new(
        (KnotVector::bezier_knot(2), KnotVector::bezier_knot(2)),
        rows,
    ))
}

fn elliptic_quarter(centre: Point3, w: Vector3, u: Vector3) -> Curve {
    arc_curve(centre + w, centre + u, centre, w + u, 0.0)
}

fn cylinder(
    start: Point3,
    normals: [Vector3; 2],
    radius: f64,
    along: Vector3,
) -> NurbsSurface<Vector4> {
    let cos = normals[0].dot(normals[1]).clamp(-1.0, 1.0);
    let weight = ((1.0 + cos) / 2.0).sqrt();
    let middle = start + (normals[0] + normals[1]) * (radius / (1.0 + cos));
    let row = |p: Point3, w: f64| {
        vec![
            (p.to_vec() * w).extend(w),
            ((p + along).to_vec() * w).extend(w),
        ]
    };
    NurbsSurface::new(BsplineSurface::new(
        (KnotVector::bezier_knot(2), KnotVector::bezier_knot(1)),
        vec![
            row(start + normals[0] * radius, 1.0),
            row(middle, weight),
            row(start + normals[1] * radius, 1.0),
        ],
    ))
}

fn sphere(centre: Point3, radius: f64, normals: [Vector3; 3]) -> Surface {
    let middle = (normals[0] + normals[1] + normals[2]).normalize();
    let pole = middle.cross(normals[0]).normalize();
    let seam = -middle;
    let placement = Matrix4::from_cols(
        seam.extend(0.0),
        pole.cross(seam).extend(0.0),
        pole.extend(0.0),
        centre.to_homogeneous(),
    );
    Surface::SphericalSurface(Processor::with_transform(
        Sphere::new(Point3::origin(), radius),
        placement,
    ))
}

fn counter_clockwise(points: &[Point3], outward: Vector3) -> bool {
    let n = points.len();
    let area: Vector3 = (0..n)
        .map(|i| points[i].to_vec().cross(points[(i + 1) % n].to_vec()))
        .sum();
    area.dot(outward) > 0.0
}

fn sample(edge: &Edge) -> Vec<Point3> {
    let curve = edge.oriented_curve();
    let (t0, t1) = curve.range_tuple();
    (0..4)
        .map(|i| curve.subs(t0 + (t1 - t0) * i as f64 / 4.0))
        .collect()
}

fn face_from(
    edges: Vec<Edge>,
    surface: Surface,
    outward: Vector3,
    surface_faces_out: bool,
) -> Result<Face> {
    let wire: Wire = edges.into();
    let points: Vec<Point3> = wire.edge_iter().flat_map(sample).collect();
    let wire = if counter_clockwise(&points, outward) {
        wire
    } else {
        wire.inverse()
    };
    let wire = if surface_faces_out {
        wire
    } else {
        wire.inverse()
    };
    let mut face =
        Face::try_new(vec![wire], surface).map_err(|error| anyhow!("rounded face: {error}"))?;
    if !surface_faces_out {
        face.invert();
    }
    Ok(face)
}

fn solve(planes: [(Vector3, f64); 3]) -> Option<Point3> {
    let rows = Matrix3::from_cols(planes[0].0, planes[1].0, planes[2].0).transpose();
    rows.invert().map(|inverse| {
        Point3::from_vec(inverse * Vector3::new(planes[0].1, planes[1].1, planes[2].1))
    })
}

#[derive(Clone)]
enum Corner {
    Untouched,
    Ball {
        centre: Point3,
        faces: [usize; 3],
        sign: f64,
    },
    Miter {
        common: usize,
        centre: Point3,
        meet: Point3,
        sign: f64,
    },
    End {
        selected: [usize; 2],
        centre: Point3,
        sign: f64,
    },
    Torus {
        top: usize,
        sides: [usize; 2],
        centre: Point3,
        sign: f64,
    },
}

struct Polyhedron {
    faces: Vec<Face>,
    planes: Vec<Option<(Vector3, f64)>>,
    vertices: Vec<Vertex>,
    edges: Vec<(Edge, usize, usize, bool)>,
}

impl Polyhedron {
    fn vertex(&self, v: &Vertex) -> usize {
        self.vertices
            .iter()
            .position(|known| known == v)
            .expect("vertex of the shell")
    }

    fn edge(&self, e: &Edge) -> usize {
        self.edges
            .iter()
            .position(|(known, ..)| known.is_same(e))
            .expect("edge of the shell")
    }

    fn normal(&self, face: usize) -> Result<Vector3> {
        self.planes[face]
            .map(|p| p.0)
            .ok_or_else(|| anyhow!("rounding meets a curved face"))
    }

    fn offset(&self, face: usize, by: f64) -> Result<(Vector3, f64)> {
        self.planes[face]
            .map(|(n, d)| (n, d - by))
            .ok_or_else(|| anyhow!("rounding meets a curved face"))
    }

    fn sign(&self, index: usize) -> Result<f64> {
        let (edge, a, b, _) = &self.edges[index];
        let (na, nb) = (self.normal(*a)?, self.normal(*b)?);
        let oriented = self.faces[*b]
            .boundaries()
            .iter()
            .flat_map(|wire| wire.edge_iter().cloned().collect::<Vec<_>>())
            .find(|e| e.is_same(edge))
            .expect("edge of its face");
        let along = oriented.back().point() - oriented.front().point();
        let into_b = nb.cross(along);
        Ok(if na.dot(into_b) < 0.0 { 1.0 } else { -1.0 })
    }
}

fn straight(edge: &Edge) -> bool {
    let curve = edge.curve();
    if matches!(curve, Curve::Line(_)) {
        return true;
    }
    let (t0, t1) = curve.range_tuple();
    let (p, q) = (curve.subs(t0), curve.subs(t1));
    let chord = q - p;
    let length = chord.magnitude();
    length > 0.0
        && (1..16).all(|i| {
            let x = curve.subs(t0 + (t1 - t0) * i as f64 / 16.0) - p;
            x.cross(chord).magnitude() / length < 1.0e-7 * length.max(1.0)
        })
}

fn square(a: Vector3, b: Vector3) -> bool {
    a.dot(b).abs() < 1.0e-9
}

pub(crate) fn round_edges(
    solid: &Solid,
    selected: &[Edge],
    radius: f64,
    flat: bool,
) -> Result<Solid> {
    let bend =
        |p: Point3, q: Point3, curve: Curve| if flat { Curve::Line(Line(p, q)) } else { curve };
    let [shell] = solid.boundaries().as_slice() else {
        bail!("rounding corners works on a single closed solid")
    };
    let faces: Vec<Face> = shell.face_iter().cloned().collect();
    let planes = faces
        .iter()
        .map(|face| match face.oriented_surface() {
            Surface::Plane(plane) => {
                Some((plane.normal(), plane.normal().dot(plane.origin().to_vec())))
            }
            _ => None,
        })
        .collect();
    let vertices: Vec<Vertex> = shell.vertex_iter().fold(Vec::new(), |mut all, v| {
        if !all.contains(&v) {
            all.push(v);
        }
        all
    });
    let edges = shell
        .edge_iter()
        .fold(Vec::<Edge>::new(), |mut all, e| {
            if !all.iter().any(|known| known.is_same(&e)) {
                all.push(e);
            }
            all
        })
        .into_iter()
        .map(|edge| {
            let owners: Vec<usize> = (0..faces.len())
                .filter(|&i| faces[i].edge_iter().any(|e| e.is_same(&edge)))
                .collect();
            let [a, b] = owners[..] else {
                bail!("an edge does not join two faces")
            };
            let chosen = selected.iter().any(|s| s.is_same(&edge));
            Ok((edge, a, b, chosen))
        })
        .collect::<Result<Vec<_>>>()?;
    let poly = Polyhedron {
        faces,
        planes,
        vertices,
        edges,
    };
    if poly
        .edges
        .iter()
        .any(|(e, _, _, chosen)| *chosen && !straight(e))
    {
        bail!("rounding corners where three edges meet needs straight edges");
    }
    let signs = (0..poly.edges.len())
        .map(|i| {
            if poly.edges[i].3 {
                poly.sign(i)
            } else {
                Ok(1.0)
            }
        })
        .collect::<Result<Vec<f64>>>()?;

    let corners = (0..poly.vertices.len())
        .map(|v| {
            let at: Vec<usize> = (0..poly.edges.len())
                .filter(|&i| {
                    let (e, ..) = &poly.edges[i];
                    poly.vertex(e.front()) == v || poly.vertex(e.back()) == v
                })
                .collect();
            let chosen: Vec<usize> = at.iter().copied().filter(|&i| poly.edges[i].3).collect();
            if chosen.is_empty() {
                return Ok(Corner::Untouched);
            }
            let around: Vec<usize> = (0..poly.faces.len())
                .filter(|&f| poly.faces[f].vertex_iter().any(|x| x == poly.vertices[v]))
                .collect();
            let [a, b, c] = around[..] else {
                bail!("a rounded corner must join exactly three faces")
            };
            if at
                .iter()
                .any(|&i| !straight(&poly.edges[i].0))
            {
                bail!("a rounded corner must have straight edges");
            }
            let normals = [poly.normal(a)?, poly.normal(b)?, poly.normal(c)?];
            let sign = signs[chosen[0]];
            let mixed = chosen.iter().any(|&i| signs[i] != sign);
            let ball = solve([
                poly.offset(a, sign * radius)?,
                poly.offset(b, sign * radius)?,
                poly.offset(c, sign * radius)?,
            ])
            .ok_or_else(|| anyhow!("a corner is degenerate"))?;
            let orthogonal = square(normals[0], normals[1])
                && square(normals[1], normals[2])
                && square(normals[0], normals[2]);
            if mixed && chosen.len() < 3 {
                bail!("an inside and an outside rounded edge meet at a corner with a sharp edge");
            }
            match chosen.len() {
                3 if mixed => {
                    let lone = chosen
                        .iter()
                        .copied()
                        .find(|&i| chosen.iter().filter(|&&j| signs[j] == signs[i]).count() == 1)
                        .expect("one edge differs");
                    let (_, la, lb, _) = poly.edges[lone];
                    let top = [a, b, c]
                        .into_iter()
                        .find(|f| *f != la && *f != lb)
                        .expect("three faces");
                    let (nt, na, nb) = (poly.normal(top)?, poly.normal(la)?, poly.normal(lb)?);
                    if !square(nt, na) || !square(nt, nb) {
                        bail!(
                            "an inside rounded edge meeting two outside ones needs the face across it square to its sides"
                        );
                    }
                    let pair = -signs[lone];
                    let centre = solve([
                        poly.offset(la, -pair * radius)?,
                        poly.offset(lb, -pair * radius)?,
                        poly.offset(top, pair * radius)?,
                    ])
                    .ok_or_else(|| anyhow!("a corner is degenerate"))?;
                    Ok(Corner::Torus {
                        top,
                        sides: [la, lb],
                        centre,
                        sign: pair,
                    })
                }
                3 => Ok(Corner::Ball {
                    centre: ball,
                    faces: [a, b, c],
                    sign,
                }),
                2 => {
                    if !orthogonal {
                        bail!("two rounded edges meeting a sharp one need square faces");
                    }
                    let (_, a0, b0, _) = poly.edges[chosen[0]];
                    let (_, a1, b1, _) = poly.edges[chosen[1]];
                    let common = [a0, b0]
                        .into_iter()
                        .find(|f| *f == a1 || *f == b1)
                        .ok_or_else(|| anyhow!("rounded edges at a corner share no face"))?;
                    let others: Vec<usize> =
                        [a, b, c].into_iter().filter(|f| *f != common).collect();
                    let meet = ball
                        + (poly.normal(others[0])? + poly.normal(others[1])?) * (sign * radius);
                    Ok(Corner::Miter {
                        common,
                        centre: ball,
                        meet,
                        sign,
                    })
                }
                _ => {
                    if !orthogonal {
                        bail!("a rounded edge ending at a sharp corner needs square faces");
                    }
                    let (_, ea, eb, _) = poly.edges[chosen[0]];
                    let end_face = [a, b, c]
                        .into_iter()
                        .find(|f| *f != ea && *f != eb)
                        .expect("three faces");
                    let centre = solve([
                        poly.offset(ea, sign * radius)?,
                        poly.offset(eb, sign * radius)?,
                        poly.offset(end_face, 0.0)?,
                    ])
                    .ok_or_else(|| anyhow!("a corner is degenerate"))?;
                    Ok(Corner::End {
                        selected: [ea, eb],
                        centre,
                        sign,
                    })
                }
            }
        })
        .collect::<Result<Vec<Corner>>>()?;

    let normal = |f: usize| poly.planes[f].expect("checked flat").0;
    let endpoint = |edge: usize, v: usize, face: usize| -> Point3 {
        let (_, ea, eb, chosen) = poly.edges[edge];
        match &corners[v] {
            Corner::Untouched => poly.vertices[v].point(),
            Corner::Ball { centre, sign, .. } => *centre + normal(face) * (sign * radius),
            Corner::Miter {
                common,
                centre,
                meet,
                sign,
            } => {
                if chosen && face == *common {
                    *centre + normal(face) * (sign * radius)
                } else {
                    *meet
                }
            }
            Corner::End {
                selected,
                centre,
                sign,
            } => {
                if chosen {
                    *centre + normal(face) * (sign * radius)
                } else {
                    let side = if selected.contains(&ea) { ea } else { eb };
                    *centre + normal(side) * (sign * radius)
                }
            }
            Corner::Torus {
                top, centre, sign, ..
            } => {
                if face == *top {
                    let side = if ea == *top { eb } else { ea };
                    *centre - normal(side) * (2.0 * sign * radius) + normal(face) * (sign * radius)
                } else {
                    *centre - normal(face) * (sign * radius)
                }
            }
        }
    };
    let torus_axis = |v: usize, edge: usize| -> Point3 {
        let Corner::Torus {
            top, centre, sign, ..
        } = &corners[v]
        else {
            unreachable!("torus corners only")
        };
        let (_, ea, eb, _) = poly.edges[edge];
        match [ea, eb].into_iter().find(|f| f != top) {
            Some(side) if ea == *top || eb == *top => {
                *centre - normal(side) * (2.0 * sign * radius)
            }
            _ => *centre,
        }
    };

    let tolerance = 1.0e-9
        * poly
            .vertices
            .iter()
            .map(|v| v.point().to_vec().magnitude())
            .fold(1.0, f64::max);
    let mut points: Vec<Vertex> = (0..poly.vertices.len())
        .filter(|&v| matches!(corners[v], Corner::Untouched))
        .map(|v| poly.vertices[v].clone())
        .collect();
    let mut vertex_at = |p: Point3| -> Vertex {
        match points.iter().find(|v| v.point().distance(p) < tolerance) {
            Some(v) => v.clone(),
            None => {
                let v = builder::vertex(p);
                points.push(v.clone());
                v
            }
        }
    };

    let mut pieces: Vec<Vec<Option<Edge>>> = vec![vec![None; 2]; poly.edges.len()];
    for (i, (edge, a, b, chosen)) in poly.edges.iter().enumerate() {
        let (v0, v1) = (
            poly.vertex(edge.absolute_front()),
            poly.vertex(edge.absolute_back()),
        );
        if *chosen {
            for (slot, face) in [*a, *b].into_iter().enumerate() {
                let (p, q) = (endpoint(i, v0, face), endpoint(i, v1, face));
                pieces[i][slot] = Some(builder::line(&vertex_at(p), &vertex_at(q)));
            }
        } else if matches!(corners[v0], Corner::Untouched)
            && matches!(corners[v1], Corner::Untouched)
        {
            pieces[i][0] = Some(edge.absolute_clone());
        } else {
            let (p, q) = (endpoint(i, v0, *a), endpoint(i, v1, *a));
            if p.distance(q) < tolerance {
                bail!("radius {radius} is too large for an edge of this solid");
            }
            pieces[i][0] = Some(builder::line(&vertex_at(p), &vertex_at(q)));
        }
    }

    let mut connectors: Vec<(usize, Edge)> = Vec::new();
    for (v, corner) in corners.iter().enumerate() {
        let chosen_here = |i: usize| {
            let (e, _, _, chosen) = &poly.edges[i];
            *chosen && (poly.vertex(e.front()) == v || poly.vertex(e.back()) == v)
        };
        match corner {
            Corner::Untouched => {}
            Corner::Ball { centre, .. } => {
                for i in (0..poly.edges.len()).filter(|&i| chosen_here(i)) {
                    let (_, a, b, _) = poly.edges[i];
                    let (p, q) = (endpoint(i, v, a), endpoint(i, v, b));
                    connectors.push((
                        v,
                        Edge::new(
                            &vertex_at(p),
                            &vertex_at(q),
                            bend(p, q, circular_arc(p, q, *centre)),
                        ),
                    ));
                }
            }
            Corner::Torus {
                top, centre, sign, ..
            } => {
                let mut tops = Vec::new();
                for i in (0..poly.edges.len()).filter(|&i| chosen_here(i)) {
                    let (_, a, b, _) = poly.edges[i];
                    let (p, q) = (endpoint(i, v, a), endpoint(i, v, b));
                    if a == *top {
                        tops.push(p);
                    } else if b == *top {
                        tops.push(q);
                    }
                    connectors.push((
                        v,
                        Edge::new(
                            &vertex_at(p),
                            &vertex_at(q),
                            bend(p, q, circular_arc(p, q, torus_axis(v, i))),
                        ),
                    ));
                }
                let [p, q] = tops[..] else {
                    bail!("a rounded inside corner must have two outside edges on one face")
                };
                let around = *centre + normal(*top) * (sign * radius);
                connectors.push((
                    v,
                    Edge::new(
                        &vertex_at(p),
                        &vertex_at(q),
                        bend(p, q, circular_arc(p, q, around)),
                    ),
                ));
            }
            Corner::Miter {
                common,
                centre,
                meet,
                sign,
            } => {
                let start = *centre + normal(*common) * (sign * radius);
                let curve = bend(
                    start,
                    *meet,
                    elliptic_quarter(*centre, start - *centre, *meet - *centre),
                );
                connectors.push((v, Edge::new(&vertex_at(start), &vertex_at(*meet), curve)));
            }
            Corner::End {
                selected,
                centre,
                sign,
            } => {
                let (p, q) = (
                    *centre + normal(selected[0]) * (sign * radius),
                    *centre + normal(selected[1]) * (sign * radius),
                );
                connectors.push((
                    v,
                    Edge::new(
                        &vertex_at(p),
                        &vertex_at(q),
                        bend(p, q, circular_arc(p, q, *centre)),
                    ),
                ));
            }
        }
    }
    let connector = |v: usize, from: Point3, to: Point3| -> Result<Edge> {
        connectors
            .iter()
            .filter(|(at, _)| *at == v)
            .find_map(|(_, edge)| {
                let (p, q) = (edge.front().point(), edge.back().point());
                if p.distance(from) < tolerance && q.distance(to) < tolerance {
                    Some(edge.clone())
                } else if p.distance(to) < tolerance && q.distance(from) < tolerance {
                    Some(edge.inverse())
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow!("rounded corner pieces do not meet"))
    };
    let piece = |edge: usize, face: usize, forward: bool| -> Edge {
        let (_, a, _, chosen) = poly.edges[edge];
        let slot = if chosen && face != a { 1 } else { 0 };
        let e = pieces[edge][slot].clone().expect("built above");
        if forward { e } else { e.inverse() }
    };

    let mut new_faces: Vec<Face> = Vec::new();
    for (f, face) in poly.faces.iter().enumerate() {
        let touched = face
            .vertex_iter()
            .any(|v| !matches!(corners[poly.vertex(&v)], Corner::Untouched));
        if !touched {
            new_faces.push(face.clone());
            continue;
        }
        let loops = face
            .boundaries()
            .iter()
            .map(|boundary| {
                let oriented: Vec<Edge> = boundary.edge_iter().cloned().collect();
                let mut out = Vec::new();
                for (k, e) in oriented.iter().enumerate() {
                    let index = poly.edge(e);
                    let current = piece(index, f, e.orientation());
                    out.push(current.clone());
                    let next = &oriented[(k + 1) % oriented.len()];
                    let next_piece = piece(poly.edge(next), f, next.orientation());
                    let v = poly.vertex(e.back());
                    let (end, start) = (current.back().point(), next_piece.front().point());
                    if end.distance(start) > tolerance {
                        out.push(connector(v, end, start)?);
                    }
                }
                Ok(out.into())
            })
            .collect::<Result<Vec<Wire>>>()?;
        new_faces.push(
            Face::try_new(loops, face.oriented_surface())
                .map_err(|error| anyhow!("rounded face: {error}"))?,
        );
    }

    for (i, (edge, a, b, chosen)) in poly.edges.iter().enumerate() {
        if !chosen {
            continue;
        }
        let (v0, v1) = (
            poly.vertex(edge.absolute_front()),
            poly.vertex(edge.absolute_back()),
        );
        let side_a = pieces[i][0].clone().expect("built");
        let side_b = pieces[i][1].clone().expect("built");
        let loop_edges = vec![
            side_a.clone(),
            connector(v1, side_a.back().point(), side_b.back().point())?,
            side_b.inverse(),
            connector(v0, side_b.front().point(), side_a.front().point())?,
        ];
        let direction = (edge.absolute_back().point() - edge.absolute_front().point()).normalize();
        let level = direction.dot(edge.absolute_front().point().to_vec()) - 2.0 * radius;
        let sign = signs[i];
        let start = solve([
            poly.offset(*a, sign * radius)?,
            poly.offset(*b, sign * radius)?,
            (direction, level),
        ])
        .ok_or_else(|| anyhow!("an edge is degenerate"))?;
        let length = (edge.absolute_back().point() - edge.absolute_front().point()).magnitude()
            + 4.0 * radius;
        let outward = (normal(*a) + normal(*b)).normalize();
        if flat {
            let plane = flat_through(
                [
                    side_a.front().point(),
                    side_a.back().point(),
                    side_b.front().point(),
                ],
                outward,
            );
            new_faces.push(face_from(loop_edges, Surface::Plane(plane), outward, true)?);
            continue;
        }
        let surface = cylinder(
            start,
            [normal(*a) * sign, normal(*b) * sign],
            radius,
            direction * length,
        );
        let faces_out = (surface
            .normal(0.5, 0.5)
            .dot(surface.subs(0.5, 0.5) - start - direction * (length / 2.0))
            > 0.0)
            == (sign > 0.0);
        new_faces.push(face_from(
            loop_edges,
            Surface::NurbsSurface(surface),
            outward,
            faces_out,
        )?);
    }

    for (v, corner) in corners.iter().enumerate() {
        let arcs = || -> Result<Vec<Edge>> {
            order_loop(
                connectors
                    .iter()
                    .filter(|(at, _)| *at == v)
                    .map(|(_, e)| e.clone())
                    .collect(),
            )
        };
        match corner {
            Corner::Ball {
                centre,
                faces,
                sign,
            } => {
                let normals = faces.map(normal);
                let outward = (normals[0] + normals[1] + normals[2]).normalize();
                let ordered = arcs()?;
                if flat {
                    let surface = flat_patch(&ordered, outward, tolerance)?;
                    new_faces.push(face_from(ordered, surface, outward, true)?);
                } else {
                    let surface = sphere(*centre, radius, normals.map(|n| n * *sign));
                    new_faces.push(face_from(ordered, surface, outward, *sign > 0.0)?);
                }
            }
            Corner::Torus {
                top,
                sides,
                centre,
                sign,
            } => {
                let axis = normal(*top);
                let outward = (axis + normal(sides[0]) + normal(sides[1])).normalize();
                let ordered = arcs()?;
                if flat {
                    let surface = flat_patch(&ordered, outward, tolerance)?;
                    new_faces.push(face_from(ordered, surface, outward, true)?);
                    continue;
                }
                let [from, to] = sides.map(|side| -normal(side) * *sign);
                let tube = *centre + from * (2.0 * radius);
                let profile =
                    circular_points(tube + axis * (sign * radius), tube - from * radius, tube);
                let surface = revolved(profile, *centre, axis, from, to);
                let middle = surface.subs(0.5, 0.5);
                let level = middle - axis * axis.dot(middle - *centre);
                let ring = *centre + (level - *centre).normalize() * (2.0 * radius);
                let faces_out =
                    (surface.normal(0.5, 0.5).dot(middle - ring) > 0.0) == (*sign > 0.0);
                new_faces.push(face_from(
                    ordered,
                    Surface::NurbsSurface(surface),
                    outward,
                    faces_out,
                )?);
            }
            _ => {}
        }
    }

    let shell: Shell = new_faces.into();
    Solid::try_new(vec![shell]).map_err(|error| anyhow!("rounded solid is not closed: {error}"))
}

fn flat_through(points: [Point3; 3], outward: Vector3) -> Plane {
    let normal = (points[1] - points[0]).cross(points[2] - points[0]);
    if normal.dot(outward) >= 0.0 {
        Plane::new(points[0], points[1], points[2])
    } else {
        Plane::new(points[0], points[2], points[1])
    }
}

fn flat_patch(ordered: &[Edge], outward: Vector3, tolerance: f64) -> Result<Surface> {
    let corners: Vec<Point3> = ordered.iter().map(|e| e.front().point()).collect();
    let plane = flat_through([corners[0], corners[1], corners[2]], outward);
    let normal = plane.normal();
    if corners
        .iter()
        .any(|p| normal.dot(*p - corners[0]).abs() > tolerance * 1.0e3)
    {
        bail!("the flat corner of this chamfer would not be flat");
    }
    Ok(Surface::Plane(plane))
}

fn order_loop(mut edges: Vec<Edge>) -> Result<Vec<Edge>> {
    let mut ordered = vec![edges.remove(0)];
    while !edges.is_empty() {
        let tail = ordered.last().expect("non-empty").back().clone();
        let next = edges
            .iter()
            .position(|e| e.front() == &tail || e.back() == &tail)
            .ok_or_else(|| anyhow!("corner arcs do not close"))?;
        let edge = edges.remove(next);
        ordered.push(if edge.front() == &tail {
            edge
        } else {
            edge.inverse()
        });
    }
    Ok(ordered)
}
