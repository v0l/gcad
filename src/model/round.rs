use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;
use std::collections::HashMap;

struct Corner {
    faces: [usize; 3],
    centre: Point3,
}

fn arc(from: Point3, to: Point3, centre: Point3) -> Curve {
    let (a, b) = ((from - centre).normalize(), (to - centre).normalize());
    let cos = a.dot(b).clamp(-1.0, 1.0);
    let weight = ((1.0 + cos) / 2.0).sqrt();
    let radius = (from - centre).magnitude();
    let middle = centre + (a + b) * (radius / (1.0 + cos));
    let points = vec![
        from.to_homogeneous(),
        (middle.to_vec() * weight).extend(weight),
        to.to_homogeneous(),
    ];
    Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
        KnotVector::bezier_knot(2),
        points,
    )))
}

fn cylinder(first: [Point3; 2], centre: Point3, along: Vector3) -> NurbsSurface<Vector4> {
    let (a, b) = (
        (first[0] - centre).normalize(),
        (first[1] - centre).normalize(),
    );
    let cos = a.dot(b).clamp(-1.0, 1.0);
    let weight = ((1.0 + cos) / 2.0).sqrt();
    let radius = (first[0] - centre).magnitude();
    let middle = centre + (a + b) * (radius / (1.0 + cos));
    let row = |p: Point3, w: f64| {
        vec![
            (p.to_vec() * w).extend(w),
            ((p + along).to_vec() * w).extend(w),
        ]
    };
    NurbsSurface::new(BsplineSurface::new(
        (KnotVector::bezier_knot(2), KnotVector::bezier_knot(1)),
        vec![row(first[0], 1.0), row(middle, weight), row(first[1], 1.0)],
    ))
}

fn sphere(centre: Point3, radius: f64, normals: [Vector3; 3]) -> Surface {
    let middle = (normals[0] + normals[1] + normals[2]).normalize();
    let pole = middle.cross(normals[0]).normalize();
    let seam = -middle;
    let third = pole.cross(seam);
    let placement = Matrix4::from_cols(
        seam.extend(0.0),
        third.extend(0.0),
        pole.extend(0.0),
        centre.to_homogeneous(),
    );
    Surface::SphericalSurface(Processor::with_transform(
        Sphere::new(Point3::origin(), radius),
        placement,
    ))
}

fn plane_with_normal(point: Point3, normal: Vector3) -> Plane {
    let helper = if normal.x.abs() < 0.9 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    };
    let x = helper.cross(normal).normalize();
    Plane::new(point, point + x, point + normal.cross(x))
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
    if surface_faces_out {
        Face::try_new(vec![wire], surface).map_err(|error| anyhow!("rounded face: {error}"))
    } else {
        let mut face = Face::try_new(vec![wire.inverse()], surface)
            .map_err(|error| anyhow!("rounded face: {error}"))?;
        face.invert();
        Ok(face)
    }
}

pub(crate) fn round_every_edge(solid: &Solid, radius: f64) -> Result<Solid> {
    let [shell] = solid.boundaries().as_slice() else {
        bail!("rounding every edge works on a single closed solid")
    };
    let faces: Vec<Face> = shell.face_iter().cloned().collect();
    let planes: Vec<(Vector3, f64)> = faces
        .iter()
        .map(|face| match face.oriented_surface() {
            Surface::Plane(plane) if face.boundaries().len() == 1 => {
                Ok((plane.normal(), plane.normal().dot(plane.origin().to_vec())))
            }
            _ => bail!(
                "rounding corners where three edges meet needs a flat-faced solid without holes"
            ),
        })
        .collect::<Result<_>>()?;
    let vertices: Vec<Vertex> = shell.vertex_iter().fold(Vec::new(), |mut all, v| {
        if !all.contains(&v) {
            all.push(v);
        }
        all
    });
    let scale = vertices
        .iter()
        .map(|v| v.point().to_vec().magnitude())
        .fold(1.0, f64::max);
    if vertices.iter().any(|v| {
        planes
            .iter()
            .any(|(n, d)| n.dot(v.point().to_vec()) > d + 1.0e-9 * scale)
    }) {
        bail!("rounding corners where three edges meet works on convex solids");
    }
    let corners: Vec<Corner> = vertices
        .iter()
        .map(|vertex| {
            let around: Vec<usize> = (0..faces.len())
                .filter(|&i| faces[i].vertex_iter().any(|v| v == *vertex))
                .collect();
            let [a, b, c] = around[..] else {
                bail!("rounding needs every corner to join exactly three faces")
            };
            let rows = Matrix3::from_cols(planes[a].0, planes[b].0, planes[c].0).transpose();
            let inverse = rows
                .invert()
                .ok_or_else(|| anyhow!("a corner is degenerate"))?;
            let centre = Point3::from_vec(
                inverse
                    * Vector3::new(
                        planes[a].1 - radius,
                        planes[b].1 - radius,
                        planes[c].1 - radius,
                    ),
            );
            Ok(Corner {
                faces: [a, b, c],
                centre,
            })
        })
        .collect::<Result<_>>()?;
    let corner_of = |vertex: &Vertex| {
        vertices
            .iter()
            .position(|v| v == vertex)
            .expect("collected above")
    };
    let touch = |corner: usize, face: usize| corners[corner].centre + planes[face].0 * radius;

    let mut points: HashMap<(usize, usize), Vertex> = HashMap::new();
    let mut point = |corner: usize, face: usize| {
        points
            .entry((corner, face))
            .or_insert_with(|| builder::vertex(touch(corner, face)))
            .clone()
    };
    let original_edges: Vec<(Edge, usize, usize)> = shell
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
            Ok((edge, a, b))
        })
        .collect::<Result<_>>()?;

    let mut contact: HashMap<(usize, usize), Edge> = HashMap::new();
    let mut ends: HashMap<(usize, usize, usize), Edge> = HashMap::new();
    let mut new_faces = Vec::new();
    for (index, (edge, a, b)) in original_edges.iter().enumerate() {
        let (v0, v1) = (corner_of(edge.front()), corner_of(edge.back()));
        if (corners[v1].centre - corners[v0].centre).dot(edge.back().point() - edge.front().point())
            <= 0.0
        {
            bail!("radius {radius} is too large for this solid");
        }
        for face in [*a, *b] {
            contact.insert(
                (index, face),
                builder::line(&point(v0, face), &point(v1, face)),
            );
        }
        for v in [v0, v1] {
            let curve = arc(touch(v, *a), touch(v, *b), corners[v].centre);
            ends.insert(
                (index, v, 0),
                Edge::new(&point(v, *a), &point(v, *b), curve),
            );
        }
        let along = corners[v1].centre - corners[v0].centre;
        let surface = cylinder([touch(v0, *a), touch(v0, *b)], corners[v0].centre, along);
        let loop_edges = vec![
            contact[&(index, *a)].clone(),
            ends[&(index, v1, 0)].clone(),
            contact[&(index, *b)].inverse(),
            ends[&(index, v0, 0)].inverse(),
        ];
        let outward = (planes[*a].0 + planes[*b].0).normalize();
        let faces_out = surface
            .normal(0.5, 0.5)
            .dot(surface.subs(0.5, 0.5) - corners[v0].centre)
            > 0.0;
        new_faces.push(face_from(
            loop_edges,
            Surface::NurbsSurface(surface),
            outward,
            faces_out,
        )?);
    }
    for (f, face) in faces.iter().enumerate() {
        let edges: Vec<Edge> = face.boundaries()[0]
            .edge_iter()
            .map(|e| {
                let index = original_edges
                    .iter()
                    .position(|(known, _, _)| known.is_same(e))
                    .expect("edge of the shell");
                let same = corner_of(e.front()) == corner_of(original_edges[index].0.front());
                if same {
                    contact[&(index, f)].clone()
                } else {
                    contact[&(index, f)].inverse()
                }
            })
            .collect();
        let corner = corner_of(
            face.boundaries()[0]
                .front_vertex()
                .expect("non-empty boundary"),
        );
        new_faces.push(face_from(
            edges,
            Surface::Plane(plane_with_normal(touch(corner, f), planes[f].0)),
            planes[f].0,
            true,
        )?);
    }
    for (v, corner) in corners.iter().enumerate() {
        let edges: Vec<Edge> = original_edges
            .iter()
            .enumerate()
            .filter(|(_, (edge, _, _))| corner_of(edge.front()) == v || corner_of(edge.back()) == v)
            .map(|(index, _)| ends[&(index, v, 0)].clone())
            .collect();
        let ordered = order_loop(edges)?;
        let normals = corner.faces.map(|f| planes[f].0);
        let outward = (normals[0] + normals[1] + normals[2]).normalize();
        new_faces.push(face_from(
            ordered,
            sphere(corner.centre, radius, normals),
            outward,
            true,
        )?);
    }
    let shell: Shell = new_faces.into();
    Solid::try_new(vec![shell]).map_err(|error| anyhow!("rounded solid is not closed: {error}"))
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
