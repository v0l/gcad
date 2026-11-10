use super::{Mesh, P3, add, cross, dot, length, normalized, scale, sub};
use std::collections::{HashMap, HashSet};

const ARC_STEPS: usize = 3;

pub struct Plate {
    pub mesh: Mesh,
    pub rim_edges: usize,
    pub patch_triangles: usize,
    pub origin: Vec<u32>,
}

fn smoothed_border(
    positions: &[P3],
    border: &[(u32, u32, P3)],
    passes: usize,
) -> HashMap<u32, P3> {
    let mut neighbours: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(u, v, _) in border {
        neighbours.entry(u).or_default().push(v);
        neighbours.entry(v).or_default().push(u);
    }
    let mut current: HashMap<u32, P3> = neighbours
        .keys()
        .map(|&i| (i, positions[i as usize]))
        .collect();
    for _ in 0..passes {
        for factor in [0.5, -0.53] {
            let next: HashMap<u32, P3> = current
                .iter()
                .map(|(&i, &p)| {
                    let ring = &neighbours[&i];
                    let mean = scale(
                        ring.iter().fold([0.0; 3], |acc, j| add(acc, current[j])),
                        1.0 / ring.len() as f64,
                    );
                    (i, add(p, scale(sub(mean, p), factor)))
                })
                .collect();
            current = next;
        }
    }
    current
}

fn profile(t: f64, rim: f64) -> Vec<(f64, f64)> {
    let r = rim.min(t / 2.0);
    if r <= 1.0e-9 {
        return vec![(0.0, t), (0.0, 0.0)];
    }
    let arc = |k: usize| (k as f64 / ARC_STEPS as f64) * std::f64::consts::FRAC_PI_2;
    let mut out = vec![(-r, t)];
    out.extend((1..=ARC_STEPS).map(|k| (-r + r * arc(k).sin(), t - r + r * arc(k).cos())));
    out.extend((0..ARC_STEPS).rev().map(|k| (-r + r * arc(k + 1).sin(), r - r * arc(k + 1).cos())));
    out.push((-r, 0.0));
    out
}

impl Mesh {
    pub fn plate(&self, selected: &[bool], name: &str, t: f64, offset: f64, rim: f64, smooth: usize) -> Plate {
        let patch: Vec<[u32; 3]> = self
            .triangles
            .iter()
            .copied()
            .filter(|tri| tri.iter().all(|&i| selected[i as usize]))
            .collect();
        let normals = self.vertex_normals();
        let mut index: HashMap<u32, u32> = HashMap::new();
        let mut source: Vec<u32> = Vec::new();
        for tri in &patch {
            for &i in tri {
                index.entry(i).or_insert_with(|| {
                    source.push(i);
                    (source.len() - 1) as u32
                });
            }
        }
        let directed: HashSet<(u32, u32)> = patch
            .iter()
            .flat_map(|t| (0..3).map(move |k| (t[k], t[(k + 1) % 3])))
            .collect();
        let mut border: Vec<(u32, u32, P3)> = Vec::new();
        let mut edges: Vec<(u32, u32, P3)> = Vec::new();
        for tri in &patch {
            let [a, b, c] = self.corners(*tri);
            let face = normalized(cross(sub(b, a), sub(c, a)));
            for k in 0..3 {
                let (u, v) = (tri[k], tri[(k + 1) % 3]);
                if !directed.contains(&(v, u)) {
                    edges.push((u, v, face));
                }
            }
        }
        let rim_points = smoothed_border(&self.positions, &edges, smooth);
        for &(u, v, face) in &edges {
            let e = sub(rim_points[&v], rim_points[&u]);
            border.push((u, v, normalized(cross(e, face))));
        }
        let mut outward: HashMap<u32, P3> = HashMap::new();
        for &(u, v, d) in &border {
            for i in [u, v] {
                let sum = outward.entry(i).or_insert([0.0; 3]);
                *sum = add(*sum, d);
            }
        }
        let outward: HashMap<u32, P3> = outward
            .into_iter()
            .map(|(i, d)| {
                let n = normals[i as usize];
                (i, normalized(sub(d, scale(n, dot(d, n)))))
            })
            .collect();

        let steps = profile(t, rim);
        let place = |i: u32, (d, h): (f64, f64), edge: bool| {
            let p = rim_points
                .get(&i)
                .copied()
                .unwrap_or(self.positions[i as usize]);
            let n = normals[i as usize];
            let lifted = add(p, scale(n, offset + h));
            match outward.get(&i) {
                Some(dir) if edge => add(lifted, scale(*dir, d)),
                _ => lifted,
            }
        };
        let top = steps[0];
        let bottom = *steps.last().expect("profile");
        let count = source.len() as u32;
        let mut positions: Vec<P3> = source.iter().map(|&i| place(i, top, true)).collect();
        positions.extend(source.iter().map(|&i| place(i, bottom, true)));
        let mut triangles: Vec<[u32; 3]> = Vec::new();
        for tri in &patch {
            let [a, b, c] = tri.map(|i| index[&i]);
            triangles.push([a, b, c]);
            triangles.push([a + count, c + count, b + count]);
        }
        let mut rings: HashMap<u32, Vec<u32>> = HashMap::new();
        for &(u, v, _) in &border {
            for i in [u, v] {
                rings.entry(i).or_insert_with(|| {
                    let mut ids = vec![index[&i]];
                    for &step in &steps[1..steps.len() - 1] {
                        positions.push(place(i, step, true));
                        ids.push((positions.len() - 1) as u32);
                    }
                    ids.push(index[&i] + count);
                    ids
                });
            }
        }
        for &(u, v, _) in &border {
            let (ru, rv) = (&rings[&u], &rings[&v]);
            for k in 0..steps.len() - 1 {
                triangles.push([ru[k], rv[k + 1], rv[k]]);
                triangles.push([ru[k], ru[k + 1], rv[k + 1]]);
            }
        }
        let mut mesh = Mesh {
            labels: vec![0; positions.len()],
            positions,
            triangles,
            parts: vec![name.to_string()],
            ..Default::default()
        };
        mesh.mask.clear();
        let mut origin: Vec<u32> = source.clone();
        origin.extend(source.iter().copied());
        let mut ring_ids: Vec<(u32, u32)> = rings
            .iter()
            .flat_map(|(&i, ids)| ids[1..ids.len() - 1].iter().map(move |&id| (id, i)))
            .collect();
        ring_ids.sort_unstable();
        origin.extend(ring_ids.into_iter().map(|(_, i)| i));
        let _ = length;
        Plate {
            rim_edges: border.len(),
            patch_triangles: patch.len(),
            origin,
            mesh,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::sdf::ellipsoid_mesh;

    const UNIT: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    #[test]
    fn smoothing_the_border_loop_shortens_a_stepped_rim() {
        let ball = ellipsoid_mesh([0.0; 3], [10.0; 3], &UNIT, 24, "ball");
        let selected: Vec<bool> = ball.positions.iter().map(|p| p[2] > 3.0 + p[0] * 0.2).collect();
        let loop_length = |passes: usize| {
            let patch: Vec<[u32; 3]> = ball
                .triangles
                .iter()
                .copied()
                .filter(|t| t.iter().all(|&i| selected[i as usize]))
                .collect();
            let directed: HashSet<(u32, u32)> = patch
                .iter()
                .flat_map(|t| (0..3).map(move |k| (t[k], t[(k + 1) % 3])))
                .collect();
            let edges: Vec<(u32, u32, P3)> = directed
                .iter()
                .filter(|(u, v)| !directed.contains(&(*v, *u)))
                .map(|&(u, v)| (u, v, [0.0; 3]))
                .collect();
            let points = smoothed_border(&ball.positions, &edges, passes);
            edges
                .iter()
                .map(|&(u, v, _)| length(sub(points[&v], points[&u])))
                .sum::<f64>()
        };
        assert!(loop_length(6) < loop_length(0) * 0.99);
    }

    #[test]
    fn plate_of_a_cap_has_the_shell_sector_volume_and_a_closed_rim() {
        let ball = ellipsoid_mesh([0.0; 3], [10.0; 3], &UNIT, 96, "ball");
        let selected: Vec<bool> = ball.positions.iter().map(|p| p[2] > 6.0).collect();
        let plate = ball.plate(&selected, "cap", 0.4, 0.2, 0.15, 4);
        assert_eq!(plate.mesh.open_edges(), 0);
        let patch = Mesh {
            positions: ball.positions.clone(),
            triangles: ball
                .triangles
                .iter()
                .copied()
                .filter(|t| t.iter().all(|&i| selected[i as usize]))
                .collect(),
            ..Default::default()
        };
        assert_eq!(plate.rim_edges, patch.open_edges());
        let cone: f64 = ball
            .triangles
            .iter()
            .filter(|t| t.iter().all(|&i| selected[i as usize]))
            .map(|&t| {
                let [a, b, c] = ball.corners(t);
                dot(a, cross(b, c)) / 6.0
            })
            .sum();
        let expected = cone * ((10.6f64.powi(3) - 10.2f64.powi(3)) / 1000.0);
        let got = plate.mesh.volume();
        assert!((got - expected).abs() < expected * 0.02, "{got} vs {expected}");
    }
}
