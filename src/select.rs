use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

#[derive(Clone, Debug)]
pub struct GroupEntry {
    pub label: String,
    pub group: String,
    pub surface: Surface,
}

#[derive(Clone, Debug, Default)]
pub struct Groups(pub Vec<GroupEntry>);

impl Groups {
    pub fn record(&mut self, label: &str, group: &str, surface: Surface) {
        self.0.push(GroupEntry {
            label: label.to_string(),
            group: group.to_string(),
            surface,
        });
    }

    fn surfaces(&self, label: &str, group: Option<&str>) -> Vec<&Surface> {
        self.0
            .iter()
            .filter(|entry| entry.label == label && group.is_none_or(|group| entry.group == group))
            .map(|entry| &entry.surface)
            .collect()
    }

    fn known(&self) -> String {
        let mut names: Vec<String> = self
            .0
            .iter()
            .map(|e| format!("{}.{}", e.label, e.group))
            .collect();
        names.dedup();
        names.sort();
        names.dedup();
        if names.is_empty() {
            "none yet".to_string()
        } else {
            names.join(", ")
        }
    }
}

pub fn edge_owners(faces: &[Face]) -> std::collections::HashMap<EdgeId, Vec<usize>> {
    let mut owners: std::collections::HashMap<EdgeId, Vec<usize>> = Default::default();
    faces.iter().enumerate().for_each(|(i, face)| {
        face.edge_iter().for_each(|edge| {
            let list = owners.entry(edge.id()).or_default();
            if !list.contains(&i) {
                list.push(i);
            }
        })
    });
    owners
}

pub fn faces(solid: &Solid) -> Vec<Face> {
    solid
        .boundaries()
        .iter()
        .flat_map(|shell| shell.face_iter().cloned())
        .collect()
}

fn plane_of(face: &Face) -> Option<Plane> {
    match face.oriented_surface() {
        Surface::Plane(plane) => Some(plane),
        _ => None,
    }
}

fn sample_points(face: &Face) -> Vec<Point3> {
    face.edge_iter()
        .flat_map(|edge| {
            let curve = edge.curve();
            let (t0, t1) = curve.range_tuple();
            [edge.front().point(), curve.subs((t0 + t1) / 2.0)]
        })
        .collect()
}

fn control_hull(surface: &Surface) -> Option<BoundingBox<Point3>> {
    match surface {
        Surface::BsplineSurface(s) => Some(s.control_points().iter().flatten().copied().collect()),
        Surface::NurbsSurface(s) => Some(
            s.control_points()
                .iter()
                .flatten()
                .map(|v| Point3::from_homogeneous(*v))
                .collect(),
        ),
        _ => None,
    }
}

pub fn face_on(face: &Face, surface: &Surface, tolerance: f64) -> bool {
    match (face.oriented_surface(), surface) {
        (Surface::Plane(a), Surface::Plane(b)) => {
            a.normal().dot(b.normal()) > 1.0 - 1.0e-9
                && (a.origin() - b.origin()).dot(b.normal()).abs() < tolerance
        }
        (Surface::Plane(_), _) | (_, Surface::Plane(_)) => false,
        (Surface::BsplineSurface(a), Surface::BsplineSurface(b)) if a == *b => true,
        (Surface::NurbsSurface(a), Surface::NurbsSurface(b)) if a == *b => true,
        (own, _) => {
            if let Some(hull) = control_hull(surface) {
                let (low, high) = (hull.min(), hull.max());
                let slack = tolerance * 10.0;
                let outside =
                    |p: Point3| (0..3).any(|k| p[k] < low[k] - slack || p[k] > high[k] + slack);
                if face.vertex_iter().any(|v| outside(v.point())) {
                    return false;
                }
            }
            let points = sample_points(face);
            let on_surface = |point: &Point3| {
                surface
                    .search_nearest_parameter(*point, None, 100)
                    .map(|(u, v)| (u, v, surface.subs(u, v).distance(*point)))
            };
            let all_on = points
                .iter()
                .all(|point| on_surface(point).is_some_and(|(_, _, d)| d < tolerance * 10.0));
            all_on
                && points.first().is_some_and(|point| {
                    let (u, v, _) = on_surface(point).expect("checked above");
                    own.search_nearest_parameter(*point, None, 100)
                        .is_some_and(|(s, t)| own.normal(s, t).dot(surface.normal(u, v)) > 0.5)
                })
        }
    }
}

fn axis(name: char) -> Option<Vector3> {
    match name.to_ascii_uppercase() {
        'X' => Some(Vector3::unit_x()),
        'Y' => Some(Vector3::unit_y()),
        'Z' => Some(Vector3::unit_z()),
        _ => None,
    }
}

fn facing(face: &Face, direction: Vector3) -> Option<f64> {
    plane_of(face)
        .filter(|plane| plane.normal().dot(direction) > 1.0 - 1.0e-9)
        .map(|plane| plane.origin().to_vec().dot(direction))
}

fn atom_faces(atom: &str, faces: &[Face], groups: &Groups, tolerance: f64) -> Result<Vec<usize>> {
    let directional = atom.len() == 2 && "<>+-".contains(&atom[..1]);
    if directional {
        let direction = axis(atom.chars().nth(1).expect("length checked"))
            .ok_or_else(|| anyhow!("`{atom}`: axis must be X, Y or Z"))?;
        let sign = if atom.starts_with('<') || atom.starts_with('-') {
            -1.0
        } else {
            1.0
        };
        let direction = direction * sign;
        let offsets: Vec<(usize, f64)> = faces
            .iter()
            .enumerate()
            .filter_map(|(i, face)| facing(face, direction).map(|offset| (i, offset)))
            .collect();
        if atom.starts_with('+') || atom.starts_with('-') {
            return Ok(offsets.into_iter().map(|(i, _)| i).collect());
        }
        let extreme = offsets
            .iter()
            .map(|&(_, o)| o)
            .fold(f64::NEG_INFINITY, f64::max);
        return Ok(offsets
            .into_iter()
            .filter(|&(_, offset)| (offset - extreme).abs() < tolerance)
            .map(|(i, _)| i)
            .collect());
    }
    if atom == "all" {
        return Ok((0..faces.len()).collect());
    }
    let (label, group) = match atom.split_once('.') {
        Some((label, group)) => (label, Some(group)),
        None => (atom, None),
    };
    let surfaces = groups.surfaces(label, group);
    if surfaces.is_empty() {
        bail!("`{atom}` names no faces; known groups: {}", groups.known());
    }
    Ok(faces
        .iter()
        .enumerate()
        .filter(|(_, face)| {
            surfaces
                .iter()
                .any(|surface| face_on(face, surface, tolerance))
        })
        .map(|(i, _)| i)
        .collect())
}

pub fn select_faces(
    expression: &str,
    solid: &Solid,
    groups: &Groups,
    tolerance: f64,
) -> Result<Vec<usize>> {
    let faces = faces(solid);
    let mut selected: Vec<usize> = expression
        .split(',')
        .map(|atom| {
            if atom.is_empty() {
                bail!("empty face selector in `{expression}`");
            }
            atom_faces(atom, &faces, groups, tolerance)
        })
        .collect::<Result<Vec<_>>>()?
        .concat();
    selected.sort_unstable();
    selected.dedup();
    Ok(selected)
}

pub fn select_edges(
    expression: &str,
    solid: &Solid,
    groups: &Groups,
    tolerance: f64,
) -> Result<Vec<Edge>> {
    let faces = faces(solid);
    let owners_of = edge_owners(&faces);
    let edges_of = |indices: &[usize]| -> Vec<Edge> {
        indices.iter().flat_map(|&i| faces[i].edge_iter()).collect()
    };
    let mut edges: Vec<Edge> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for term in expression.split('|') {
        let sets = term
            .split('&')
            .map(|set| select_faces(set, solid, groups, tolerance))
            .collect::<Result<Vec<_>>>()?;
        let found: Vec<Edge> = match sets.as_slice() {
            [only] => edges_of(only),
            [a, b] => edges_of(a)
                .into_iter()
                .filter(|edge| {
                    let owners = owners_of.get(&edge.id()).cloned().unwrap_or_default();
                    owners.iter().any(|i| a.contains(i))
                        && owners.iter().any(|j| {
                            b.contains(j) && owners.iter().any(|i| i != j && a.contains(i))
                        })
                })
                .collect(),
            _ => bail!("`{term}`: use at most one `&` per term"),
        };
        found.into_iter().for_each(|edge| {
            if seen.insert(edge.id()) {
                edges.push(edge);
            }
        });
    }
    Ok(edges)
}
