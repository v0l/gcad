use super::rig::{MAX_INFLUENCES, Rig};
use super::{Mesh, P3, cross, dot, length, sub};
use std::fmt;

pub const JUMP_LIMIT: f64 = 0.35;
pub const CONTESTED_MARGIN: f64 = 0.1;
pub const CORE_FRACTION: f64 = 0.4;
pub const RIGID: f64 = 0.99;
pub const UNRELATED_MIN: f64 = 0.05;
pub const SQUASH: f64 = 0.25;
pub const STRETCH: f64 = 4.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoreFraction {
    pub bone: String,
    pub vertices: usize,
    pub rigid: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PoseQuality {
    pub poses: usize,
    pub min_area_ratio: f64,
    pub max_area_ratio: f64,
    pub worst_aspect_growth: f64,
    pub flipped: usize,
    pub squashed: usize,
    pub stretched: usize,
    pub worst_pose: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkinReport {
    pub vertices: usize,
    pub edges: usize,
    pub max_jump: f64,
    pub jump_p99: f64,
    pub steep_edges: usize,
    pub contested: usize,
    pub influences: [usize; MAX_INFLUENCES + 1],
    pub unrelated: usize,
    pub cores: Vec<CoreFraction>,
    pub min_core: f64,
    pub quality: PoseQuality,
}

fn weight_of(w: &[(usize, f64)], bone: usize) -> f64 {
    w.iter().find(|(b, _)| *b == bone).map_or(0.0, |(_, v)| *v)
}

fn edges(mesh: &Mesh) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = mesh
        .triangles
        .iter()
        .flat_map(|t| {
            (0..3).map(move |k| {
                let (a, b) = (t[k] as usize, t[(k + 1) % 3] as usize);
                (a.min(b), a.max(b))
            })
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

pub fn edge_jump(a: &[(usize, f64)], b: &[(usize, f64)]) -> f64 {
    a.iter()
        .chain(b)
        .map(|(bone, _)| (weight_of(a, *bone) - weight_of(b, *bone)).abs())
        .fold(0.0, f64::max)
}

fn segment_t(p: P3, a: P3, b: P3) -> (f64, f64) {
    let ab = sub(b, a);
    let len2 = dot(ab, ab).max(1.0e-18);
    let t = (dot(sub(p, a), ab) / len2).clamp(0.0, 1.0);
    let q = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
    (t, length(sub(p, q)))
}

pub fn related(rig: &Rig, a: usize, b: usize) -> bool {
    a == b || rig.bones[a].parent == Some(b) || rig.bones[b].parent == Some(a)
}

struct Shape {
    area: f64,
    normal: P3,
    aspect: f64,
}

fn shape(mesh: &Mesh, t: [u32; 3]) -> Shape {
    let [a, b, c] = mesh.corners(t);
    let n = cross(sub(b, a), sub(c, a));
    let area = length(n) / 2.0;
    let longest = [sub(b, a), sub(c, b), sub(a, c)]
        .iter()
        .map(|e| dot(*e, *e))
        .fold(0.0, f64::max);
    Shape {
        area,
        normal: n,
        aspect: longest / (area * 2.0).max(1.0e-18),
    }
}

pub fn poses(rig: &Rig) -> Vec<(String, Rig)> {
    let mut out = Vec::new();
    for (c, clip) in rig.clips.iter().enumerate() {
        let mut times: Vec<f64> = clip
            .keys
            .iter()
            .map(|k| k.t)
            .chain(clip.moves.iter().map(|k| k.t))
            .collect();
        times.sort_by(f64::total_cmp);
        times.dedup_by(|a, b| (*a - *b).abs() < 1.0e-9);
        let mids: Vec<f64> = times.windows(2).map(|w| (w[0] + w[1]) / 2.0).collect();
        times.extend(mids);
        times.sort_by(f64::total_cmp);
        for t in times {
            let mut posed = rig.clone();
            posed.pose = rig.sample(c, t);
            posed.moved = rig.sample_moves(c, t);
            out.push((format!("{} t={}", clip.name, (t * 1000.0).round() / 1000.0), posed));
        }
    }
    out
}

fn quality(mesh: &Mesh, rig: &Rig) -> PoseQuality {
    let rest: Vec<Shape> = mesh.triangles.iter().map(|&t| shape(mesh, t)).collect();
    let mut q = PoseQuality {
        min_area_ratio: 1.0,
        max_area_ratio: 1.0,
        worst_aspect_growth: 1.0,
        ..Default::default()
    };
    let mut worst_bad = 0;
    for (name, posed_rig) in poses(rig) {
        let posed = posed_rig.posed(mesh, &posed_rig.pose);
        let mut bad = 0;
        for (t, r) in mesh.triangles.iter().zip(&rest) {
            if r.area < 1.0e-12 {
                continue;
            }
            let s = shape(&posed, *t);
            let ratio = s.area / r.area;
            q.min_area_ratio = q.min_area_ratio.min(ratio);
            q.max_area_ratio = q.max_area_ratio.max(ratio);
            q.worst_aspect_growth = q.worst_aspect_growth.max(s.aspect / r.aspect);
            if dot(s.normal, r.normal) < 0.0 {
                q.flipped += 1;
                bad += 1;
            }
            if ratio < SQUASH {
                q.squashed += 1;
                bad += 1;
            }
            if ratio > STRETCH {
                q.stretched += 1;
                bad += 1;
            }
        }
        if q.poses == 0 || bad > worst_bad {
            worst_bad = bad;
            q.worst_pose = name;
        }
        q.poses += 1;
    }
    q
}

pub fn check(mesh: &Mesh, rig: &Rig) -> Option<SkinReport> {
    let skin = rig.skin.as_ref().filter(|s| s.len() == mesh.positions.len())?;
    let mut report = SkinReport {
        vertices: skin.len(),
        ..Default::default()
    };
    let mut jumps: Vec<f64> = edges(mesh)
        .iter()
        .map(|&(a, b)| edge_jump(&skin[a], &skin[b]))
        .collect();
    report.edges = jumps.len();
    report.steep_edges = jumps.iter().filter(|j| **j > JUMP_LIMIT).count();
    jumps.sort_by(f64::total_cmp);
    report.max_jump = jumps.last().copied().unwrap_or(0.0);
    report.jump_p99 = jumps
        .get((jumps.len() as f64 * 0.99) as usize)
        .copied()
        .unwrap_or(report.max_jump);
    for w in skin {
        let used: Vec<&(usize, f64)> = w.iter().filter(|(_, v)| *v > 1.0e-4).collect();
        report.influences[used.len().min(MAX_INFLUENCES)] += 1;
        let mut sorted: Vec<f64> = used.iter().map(|(_, v)| *v).collect();
        sorted.sort_by(|a, b| b.total_cmp(a));
        if sorted.len() > 1 && sorted[1] >= sorted[0] * (1.0 - CONTESTED_MARGIN) {
            report.contested += 1;
        }
        if let Some(&&(winner, _)) = used.iter().max_by(|a, b| a.1.total_cmp(&b.1))
            && used
                .iter()
                .any(|(b, v)| *v >= UNRELATED_MIN && !related(rig, winner, *b))
        {
            report.unrelated += 1;
        }
    }
    let lo = 0.5 - CORE_FRACTION / 2.0;
    let hi = 0.5 + CORE_FRACTION / 2.0;
    let mut cores: Vec<(usize, usize)> = vec![(0, 0); rig.bones.len()];
    for (p, w) in mesh.positions.iter().zip(skin) {
        let nearest = rig
            .bones
            .iter()
            .enumerate()
            .map(|(i, b)| (i, segment_t(*p, b.head, b.tail)))
            .min_by(|a, b| a.1.1.total_cmp(&b.1.1));
        let Some((bone, (t, _))) = nearest else {
            continue;
        };
        if t < lo || t > hi {
            continue;
        }
        cores[bone].0 += 1;
        if weight_of(w, bone) >= RIGID {
            cores[bone].1 += 1;
        }
    }
    report.cores = rig
        .bones
        .iter()
        .zip(&cores)
        .filter(|(_, (n, _))| *n > 0)
        .map(|(b, &(n, r))| CoreFraction {
            bone: b.name.clone(),
            vertices: n,
            rigid: r as f64 / n as f64,
        })
        .collect();
    report.min_core = report.cores.iter().map(|c| c.rigid).fold(1.0, f64::min);
    report.quality = quality(mesh, rig);
    Some(report)
}

impl fmt::Display for SkinReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let q = &self.quality;
        writeln!(f, "vertices {} edges {}", self.vertices, self.edges)?;
        writeln!(
            f,
            "weight jump across an edge: max {:.3}, p99 {:.3}, {} edge(s) over {JUMP_LIMIT}",
            self.max_jump, self.jump_p99, self.steep_edges
        )?;
        writeln!(
            f,
            "contested vertices (top two within {:.0}%): {}",
            CONTESTED_MARGIN * 100.0,
            self.contested
        )?;
        writeln!(
            f,
            "influences per vertex: {}",
            self.influences
                .iter()
                .enumerate()
                .map(|(k, n)| format!("{k}:{n}"))
                .collect::<Vec<_>>()
                .join(" ")
        )?;
        writeln!(
            f,
            "vertices weighted to a bone that is not the winner, its parent or child: {}",
            self.unrelated
        )?;
        writeln!(
            f,
            "rigid core (middle {:.0}% of each bone at {RIGID}+): min {:.3}; {}",
            CORE_FRACTION * 100.0,
            self.min_core,
            self.cores
                .iter()
                .map(|c| format!("{} {:.2}", c.bone, c.rigid))
                .collect::<Vec<_>>()
                .join(", ")
        )?;
        writeln!(
            f,
            "posed over {} clip pose(s): area ratio {:.3}..{:.3}, worst aspect growth {:.2}, flipped {}, squashed {}, stretched {}{}",
            q.poses,
            q.min_area_ratio,
            q.max_area_ratio,
            q.worst_aspect_growth,
            q.flipped,
            q.squashed,
            q.stretched,
            if q.worst_pose.is_empty() {
                String::new()
            } else {
                format!(", worst pose {}", q.worst_pose)
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::rig::{Bone, Clip, Key, quat_axis_angle};

    fn strip() -> Mesh {
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        for i in 0..=10 {
            positions.push([0.0, 0.0, i as f64]);
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

    fn arm(skin: Vec<Vec<(usize, f64)>>) -> Rig {
        Rig {
            bones: vec![
                Bone {
                    name: "up".into(),
                    parent: None,
                    head: [0.5, 0.0, 0.0],
                    tail: [0.5, 0.0, 5.0],
                },
                Bone {
                    name: "low".into(),
                    parent: Some(0),
                    head: [0.5, 0.0, 5.0],
                    tail: [0.5, 0.0, 10.0],
                },
            ],
            skin: Some(skin),
            clips: vec![Clip {
                name: "bend".into(),
                fps: 24.0,
                keys: vec![
                    Key {
                        t: 0.0,
                        bone: 1,
                        rotation: quat_axis_angle([0.0, 1.0, 0.0], 0.0).unwrap(),
                    },
                    Key {
                        t: 1.0,
                        bone: 1,
                        rotation: quat_axis_angle([0.0, 1.0, 0.0], 90.0).unwrap(),
                    },
                ],
                moves: Vec::new(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_hard_switch_between_bones_is_one_full_jump() {
        let mesh = strip();
        let skin = (0..mesh.positions.len())
            .map(|i| vec![(usize::from(i / 2 > 5), 1.0)])
            .collect();
        let r = check(&mesh, &arm(skin)).unwrap();
        assert_eq!(r.max_jump, 1.0);
        assert!(r.steep_edges > 0);
        assert_eq!(r.min_core, 1.0);
        assert_eq!(r.influences[1], mesh.positions.len());
        assert_eq!(r.quality.poses, 3);
    }

    #[test]
    fn a_ramp_over_the_joint_has_small_jumps_and_no_unrelated_weights() {
        let mesh = strip();
        let skin = (0..mesh.positions.len())
            .map(|i| {
                let z = (i / 2) as f64;
                let w = ((z - 3.0) / 4.0).clamp(0.0, 1.0);
                let mut v = vec![(0, 1.0 - w), (1, w)];
                v.retain(|(_, x)| *x > 0.0);
                v
            })
            .collect();
        let r = check(&mesh, &arm(skin)).unwrap();
        assert!(r.max_jump <= 0.26, "{}", r.max_jump);
        assert_eq!(r.steep_edges, 0);
        assert_eq!(r.unrelated, 0);
        assert_eq!(r.contested, 2);
    }

    #[test]
    fn a_vertex_bound_to_an_unrelated_bone_is_counted() {
        let mesh = strip();
        let mut rig = arm(vec![vec![(0, 1.0)]; mesh.positions.len()]);
        rig.bones.push(Bone {
            name: "other".into(),
            parent: None,
            head: [5.0, 0.0, 0.0],
            tail: [5.0, 0.0, 1.0],
        });
        rig.skin.as_mut().unwrap()[0] = vec![(1, 0.6), (2, 0.4)];
        let r = check(&mesh, &rig).unwrap();
        assert_eq!(r.unrelated, 1);
    }

    #[test]
    fn folding_the_strip_flat_flips_triangles() {
        let mesh = strip();
        let skin = (0..mesh.positions.len())
            .map(|i| vec![(usize::from(i / 2 > 5), 1.0)])
            .collect();
        let mut rig = arm(skin);
        rig.clips[0].keys[1].rotation = quat_axis_angle([0.0, 0.0, 1.0], 180.0).unwrap();
        let r = check(&mesh, &rig).unwrap();
        assert!(r.quality.flipped > 0);
        assert!(r.quality.worst_pose.starts_with("bend t="));
    }
}
