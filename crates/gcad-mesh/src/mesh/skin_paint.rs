use super::rig::{MAX_INFLUENCES, Rig, Weights, trim};
use super::skin_check::SkinReport;
use super::skin_smooth::PRUNE;
use super::{Mesh, P3, length, smoothstep, sub};

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub bone: usize,
    pub points: Vec<P3>,
    pub radius: f64,
    pub falloff: f64,
    pub strength: f64,
}

fn profile(d: f64, radius: f64, falloff: f64) -> f64 {
    if d <= radius {
        1.0
    } else if falloff > 0.0 && d < radius + falloff {
        1.0 - smoothstep((d - radius) / falloff)
    } else {
        0.0
    }
}

fn reach(p: P3, stroke: &Stroke) -> f64 {
    stroke
        .points
        .iter()
        .map(|c| profile(length(sub(p, *c)), stroke.radius, stroke.falloff))
        .fold(0.0, f64::max)
}

fn take_from(rig: &Rig, w: &[(usize, f64)], bone: usize, k: f64) -> Vec<(usize, f64)> {
    let had = w.iter().find(|(b, _)| *b == bone).map_or(0.0, |(_, v)| *v);
    if had <= 0.0 {
        return w.to_vec();
    }
    let moved = had * k;
    let others: f64 = w.iter().filter(|(b, _)| *b != bone).map(|(_, v)| v).sum();
    let mut next: Vec<(usize, f64)> = w
        .iter()
        .map(|&(b, v)| {
            if b == bone {
                (b, v - moved)
            } else if others > 0.0 {
                (b, v + moved * v / others)
            } else {
                (b, v)
            }
        })
        .collect();
    if others <= 0.0 {
        let heir = rig.bones[bone]
            .parent
            .or_else(|| rig.children(bone).first().copied());
        let Some(heir) = heir else {
            return w.to_vec();
        };
        next.push((heir, moved));
    }
    next
}

pub fn paint_stroke(rig: &mut Rig, mesh: &Mesh, stroke: &Stroke) -> usize {
    let snapshot = rig.clone();
    let skin = rig.ensure_skin(mesh.positions.len());
    let mut count = 0;
    for (p, w) in mesh.positions.iter().zip(skin.iter_mut()) {
        let k = (reach(*p, stroke) * stroke.strength.abs()).clamp(0.0, 1.0);
        if k <= 0.0 {
            continue;
        }
        let next = if stroke.strength >= 0.0 {
            let mut next: Vec<(usize, f64)> = w.iter().map(|&(b, v)| (b, v * (1.0 - k))).collect();
            match next.iter_mut().find(|(b, _)| *b == stroke.bone) {
                Some((_, v)) => *v += k,
                None => next.push((stroke.bone, k)),
            }
            next
        } else {
            take_from(&snapshot, w, stroke.bone, k)
        };
        let mut next = trim(next);
        next.retain(|(_, v)| *v >= PRUNE);
        *w = trim(next);
        count += 1;
    }
    count
}

pub fn stroke_region(mesh: &Mesh, points: &[P3], radius: f64) -> Vec<bool> {
    mesh.positions
        .iter()
        .map(|p| points.iter().any(|c| length(sub(*p, *c)) <= radius))
        .collect()
}

pub fn mirror_bone(rig: &Rig, bone: usize, axis: usize) -> usize {
    super::super::interp::mirrored_name(&rig.bones[bone].name, axis)
        .and_then(|n| rig.bone_index(&n))
        .unwrap_or(bone)
}

pub fn mirror_skin(rig: &mut Rig, mesh: &Mesh, axis: usize, from_positive: bool) -> usize {
    let Some(skin) = rig.skin.clone() else {
        return 0;
    };
    let (lo, hi) = mesh.bounds().unwrap_or_default();
    let size = length(sub(hi, lo)).max(1.0e-9);
    let tolerance = size * 1.0e-3;
    let grid = super::grid::Grid::new(&mesh.positions, size / 64.0);
    let mut out = skin.clone();
    let mut count = 0;
    for (v, p) in mesh.positions.iter().enumerate() {
        let side = p[axis];
        let target = if from_positive {
            side < -tolerance
        } else {
            side > tolerance
        };
        if !target {
            continue;
        }
        let mut q = *p;
        q[axis] = -q[axis];
        let Some((source, _)) = grid.nearest(&mesh.positions, q) else {
            continue;
        };
        out[v] = skin[source as usize]
            .iter()
            .map(|&(b, w)| (mirror_bone(rig, b, axis), w))
            .collect();
        count += 1;
    }
    rig.skin = Some(out);
    count
}

pub fn normalise(rig: &mut Rig) -> usize {
    let Some(skin) = rig.skin.as_mut() else {
        return 0;
    };
    let mut changed = 0;
    for w in skin.iter_mut() {
        let mut next = trim(w.clone());
        next.retain(|(_, v)| *v >= PRUNE);
        let next = trim(next);
        if next != *w {
            changed += 1;
        }
        *w = next;
    }
    changed
}

pub fn heat_colour(w: f64) -> [f64; 3] {
    let w = w.clamp(0.0, 1.0);
    let stops = [
        (0.0, [0.05, 0.08, 0.35]),
        (0.25, [0.0, 0.45, 0.85]),
        (0.5, [0.1, 0.8, 0.3]),
        (0.75, [0.95, 0.85, 0.1]),
        (1.0, [0.9, 0.12, 0.08]),
    ];
    let i = stops
        .iter()
        .rposition(|(at, _)| *at <= w)
        .unwrap_or(0)
        .min(stops.len() - 2);
    let (a, ca) = stops[i];
    let (b, cb) = stops[i + 1];
    let t = (w - a) / (b - a);
    std::array::from_fn(|k| ca[k] + (cb[k] - ca[k]) * t)
}

pub fn bone_colours(skin: &Weights, bone: usize) -> Vec<[f64; 3]> {
    skin.iter()
        .map(|w| heat_colour(w.iter().find(|(b, _)| *b == bone).map_or(0.0, |(_, v)| *v)))
        .collect()
}

pub fn influence_colours(skin: &Weights) -> Vec<[f64; 3]> {
    let palette = [
        [0.2, 0.2, 0.2],
        [0.25, 0.45, 0.85],
        [0.2, 0.75, 0.35],
        [0.95, 0.75, 0.15],
        [0.9, 0.2, 0.15],
    ];
    skin.iter()
        .map(|w| {
            palette[w
                .iter()
                .filter(|(_, v)| *v > 1.0e-4)
                .count()
                .min(MAX_INFLUENCES)]
        })
        .collect()
}

pub fn problem_vertices(mesh: &Mesh, rig: &Rig, report: &SkinReport) -> Vec<bool> {
    let mut bad = vec![false; mesh.positions.len()];
    for &v in report.steep.iter().chain(&report.loose) {
        bad[v] = true;
    }
    for &t in &report.quality.flips {
        if let Some(tri) = mesh.triangles.iter().find(|tri| tri[0] as usize == t) {
            for i in tri {
                bad[*i as usize] = true;
            }
        }
    }
    if let Some(skin) = rig.skin.as_ref() {
        for (v, w) in skin.iter().enumerate() {
            if w.is_empty() {
                bad[v] = true;
            }
        }
    }
    bad
}

pub fn problem_colours(mesh: &Mesh, rig: &Rig, report: &SkinReport) -> Vec<[f64; 3]> {
    problem_vertices(mesh, rig, report)
        .into_iter()
        .map(|b| {
            if b {
                [0.95, 0.15, 0.1]
            } else {
                [0.55, 0.55, 0.58]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::rig::Bone;

    fn strip() -> Mesh {
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        for i in 0..=10 {
            positions.push([-1.0, 0.0, i as f64]);
            positions.push([1.0, 0.0, i as f64]);
        }
        for i in 0..10u32 {
            let a = i * 2;
            triangles.push([a, a + 1, a + 3]);
            triangles.push([a, a + 3, a + 2]);
        }
        Mesh {
            labels: vec![0; positions.len()],
            positions,
            triangles,
            parts: vec!["strip".into()],
            ..Default::default()
        }
    }

    fn rig() -> Rig {
        let bone = |name: &str, parent: Option<usize>, x: f64| Bone {
            name: name.into(),
            parent,
            head: [x, 0.0, 0.0],
            tail: [x, 0.0, 10.0],
        };
        Rig {
            bones: vec![
                bone("root", None, 0.0),
                bone("leg_l", Some(0), 1.0),
                bone("leg_r", Some(0), -1.0),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn a_stroke_paints_along_its_points_without_stacking() {
        let mesh = strip();
        let mut r = rig();
        r.skin = Some(vec![vec![(0, 1.0)]; mesh.positions.len()]);
        let stroke = Stroke {
            bone: 1,
            points: vec![[1.0, 0.0, 2.0], [1.0, 0.0, 2.5], [1.0, 0.0, 3.0]],
            radius: 0.6,
            falloff: 0.0,
            strength: 0.5,
        };
        let touched = paint_stroke(&mut r, &mesh, &stroke);
        assert_eq!(touched, 2);
        let skin = r.skin.as_ref().unwrap();
        assert_eq!(skin[5], vec![(0, 0.5), (1, 0.5)]);
        assert_eq!(skin[4], vec![(0, 1.0)]);
    }

    #[test]
    fn negative_strength_hands_weight_to_the_other_bones() {
        let mesh = strip();
        let mut r = rig();
        r.skin = Some(vec![vec![(1, 0.6), (0, 0.4)]; mesh.positions.len()]);
        let stroke = Stroke {
            bone: 1,
            points: vec![[1.0, 0.0, 0.0]],
            radius: 0.1,
            falloff: 0.0,
            strength: -0.5,
        };
        paint_stroke(&mut r, &mesh, &stroke);
        let w = &r.skin.as_ref().unwrap()[1];
        assert!(
            (w.iter().find(|(b, _)| *b == 0).unwrap().1 - 0.7).abs() < 1.0e-12,
            "{w:?}"
        );
        let mut alone = rig();
        alone.skin = Some(vec![vec![(1, 1.0)]; mesh.positions.len()]);
        paint_stroke(
            &mut alone,
            &mesh,
            &Stroke {
                strength: -1.0,
                ..stroke
            },
        );
        assert_eq!(alone.skin.as_ref().unwrap()[1], vec![(0, 1.0)]);
    }

    #[test]
    fn mirroring_copies_the_left_side_to_the_right_with_bones_swapped() {
        let mesh = strip();
        let mut r = rig();
        r.skin = Some(
            (0..mesh.positions.len())
                .map(|v| {
                    if v % 2 == 1 {
                        vec![(1, 0.8), (0, 0.2)]
                    } else {
                        vec![(0, 1.0)]
                    }
                })
                .collect(),
        );
        let n = mirror_skin(&mut r, &mesh, 0, true);
        assert_eq!(n, 11);
        assert_eq!(r.skin.as_ref().unwrap()[4], vec![(2, 0.8), (0, 0.2)]);
    }

    #[test]
    fn normalise_sums_to_one_and_drops_tiny_weights() {
        let mut r = rig();
        r.skin = Some(vec![vec![(0, 2.0), (1, 1.0), (2, 0.005)]]);
        assert_eq!(normalise(&mut r), 1);
        let w = &r.skin.as_ref().unwrap()[0];
        assert_eq!(w.len(), 2);
        assert!((w[0].1 - 2.0 / 3.0).abs() < 1.0e-12);
    }

    #[test]
    fn the_heat_map_runs_blue_to_red() {
        assert_eq!(heat_colour(0.0), [0.05, 0.08, 0.35]);
        assert_eq!(heat_colour(1.0), [0.9, 0.12, 0.08]);
        let mid = heat_colour(0.5);
        assert!(mid[1] > mid[0] && mid[1] > mid[2]);
    }
}
