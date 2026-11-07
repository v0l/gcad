use super::rig::{IDENTITY_QUAT, Pose, Quat, Rig};
use super::{P3, dot, normalized};

#[derive(Clone, Debug, PartialEq)]
pub struct Twist {
    pub segments: Vec<usize>,
    pub axis: P3,
}

fn quat_mul(a: Quat, b: Quat) -> Quat {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn conjugate(q: Quat) -> Quat {
    [-q[0], -q[1], -q[2], q[3]]
}

pub fn split(q: Quat, axis: P3) -> (f64, Quat) {
    let along = dot([q[0], q[1], q[2]], axis);
    let t = [axis[0] * along, axis[1] * along, axis[2] * along, q[3]];
    let n = t.iter().map(|v| v * v).sum::<f64>().sqrt();
    if n < 1.0e-12 {
        return (0.0, q);
    }
    let twist = t.map(|v| v / n);
    let angle = 2.0 * twist[3].clamp(-1.0, 1.0).acos() * if along < 0.0 { -1.0 } else { 1.0 };
    (angle, quat_mul(conjugate(twist), q))
}

fn about(axis: P3, angle: f64) -> Quat {
    let s = (angle / 2.0).sin();
    [axis[0] * s, axis[1] * s, axis[2] * s, (angle / 2.0).cos()]
}

pub fn segment_name(name: &str, k: usize) -> String {
    for side in ["_l", "_r", "_left", "_right"] {
        if let Some(stem) = name.strip_suffix(side) {
            return format!("{stem}_twist{k}{side}");
        }
    }
    format!("{name}_twist{k}")
}

impl Twist {
    pub fn new(segments: Vec<usize>, head: P3, tail: P3) -> Twist {
        Twist {
            segments,
            axis: normalized(super::sub(tail, head)),
        }
    }
}

impl Rig {
    pub fn chain_end(&self, bone: usize) -> usize {
        self.twists
            .iter()
            .find(|t| t.segments.first() == Some(&bone))
            .and_then(|t| t.segments.last().copied())
            .unwrap_or(bone)
    }

    pub fn rotation_track(&self, clip: usize, bone: usize) -> Vec<(f64, Quat)> {
        if !self.driven().contains(&bone) {
            return self.clips[clip]
                .bone_keys(bone)
                .iter()
                .map(|k| (k.t, k.rotation))
                .collect();
        }
        let clip_data = &self.clips[clip];
        let mut times: Vec<f64> = clip_data.keys.iter().map(|k| k.t).collect();
        let end = clip_data.duration();
        let frames = (end * clip_data.fps).ceil().max(0.0) as usize;
        times.extend((0..=frames).map(|f| (f as f64 / clip_data.fps).min(end)));
        times.sort_by(f64::total_cmp);
        times.dedup_by(|a, b| (*a - *b).abs() < 1.0e-9);
        times
            .into_iter()
            .map(|t| {
                let pose = self.effective(&self.sample(clip, t));
                (t, pose.get(&bone).copied().unwrap_or(IDENTITY_QUAT))
            })
            .collect()
    }

    pub fn driven(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for t in &self.twists {
            out.extend(t.segments.iter().skip(1));
            if let Some(&last) = t.segments.last() {
                out.extend(self.children(last).first());
            }
        }
        out
    }

    pub fn effective(&self, pose: &Pose) -> Pose {
        let mut out = pose.clone();
        for t in &self.twists {
            let Some(&last) = t.segments.last() else {
                continue;
            };
            let Some(&end) = self.children(last).first() else {
                continue;
            };
            let q = out.get(&end).copied().unwrap_or(IDENTITY_QUAT);
            let (angle, swing) = split(q, t.axis);
            let part = about(t.axis, angle / t.segments.len() as f64);
            for &s in t.segments.iter().skip(1) {
                let own = out.get(&s).copied().unwrap_or(IDENTITY_QUAT);
                out.insert(s, quat_mul(part, own));
            }
            out.insert(end, quat_mul(part, swing));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::rig::{Bone, quat_axis_angle};

    fn arm() -> Rig {
        let bone = |name: &str, parent: Option<usize>, z0: f64, z1: f64| Bone {
            name: name.into(),
            parent,
            head: [0.0, 0.0, z0],
            tail: [0.0, 0.0, z1],
        };
        Rig {
            bones: vec![
                bone("lower", None, 0.0, 4.0),
                bone("lower_twist1", Some(0), 4.0, 8.0),
                bone("lower_twist2", Some(1), 8.0, 12.0),
                bone("hand", Some(2), 12.0, 14.0),
            ],
            twists: vec![Twist::new(vec![0, 1, 2], [0.0; 3], [0.0, 0.0, 12.0])],
            ..Default::default()
        }
    }

    #[test]
    fn a_wrist_twist_spreads_over_the_segments_and_the_hand_ends_the_same() {
        let rig = arm();
        let mut pose = Pose::new();
        let turn = quat_axis_angle([0.0, 0.0, 1.0], 90.0).unwrap();
        let bend = quat_axis_angle([1.0, 0.0, 0.0], 30.0).unwrap();
        pose.insert(3, quat_mul(turn, bend));
        let plain = Rig {
            twists: Vec::new(),
            ..rig.clone()
        };
        let a = plain.world(&pose);
        let b = rig.world(&pose);
        for (i, j) in (0..3).flat_map(|i| (0..3).map(move |j| (i, j))) {
            assert!((a[3].r[i][j] - b[3].r[i][j]).abs() < 1.0e-9);
        }
        assert!((0..3).all(|k| (a[3].t[k] - b[3].t[k]).abs() < 1.0e-9));
        let eff = rig.effective(&pose);
        let (angle, _) = split(eff[&1], [0.0, 0.0, 1.0]);
        assert!(
            (angle.to_degrees() - 30.0).abs() < 1.0e-6,
            "{}",
            angle.to_degrees()
        );
        assert!(!eff.contains_key(&0));
        assert_eq!(rig.driven(), vec![1, 2, 3]);
    }

    #[test]
    fn segment_names_keep_the_side_last() {
        assert_eq!(segment_name("fore_l", 2), "fore_twist2_l");
        assert_eq!(segment_name("lower_b", 1), "lower_b_twist1");
    }
}
