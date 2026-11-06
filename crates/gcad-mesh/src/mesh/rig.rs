use super::{Mesh, P3, add, length, scale, smoothstep, sub};
use anyhow::{Result, bail};
use std::collections::HashMap;

pub type Quat = [f64; 4];

pub const IDENTITY_QUAT: Quat = [0.0, 0.0, 0.0, 1.0];
pub const MAX_INFLUENCES: usize = 4;

pub type Weights = Vec<Vec<(usize, f64)>>;
pub type Pose = HashMap<usize, Quat>;
pub type Moves = HashMap<usize, P3>;

#[derive(Clone, Debug, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub head: P3,
    pub tail: P3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub t: f64,
    pub bone: usize,
    pub rotation: Quat,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MoveKey {
    pub t: f64,
    pub bone: usize,
    pub offset: P3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub name: String,
    pub fps: f64,
    pub keys: Vec<Key>,
    pub moves: Vec<MoveKey>,
}

impl Clip {
    pub fn duration(&self) -> f64 {
        self.keys
            .iter()
            .map(|k| k.t)
            .chain(self.moves.iter().map(|k| k.t))
            .fold(0.0, f64::max)
    }

    pub fn bone_moves(&self, bone: usize) -> Vec<&MoveKey> {
        let mut keys: Vec<&MoveKey> = self.moves.iter().filter(|k| k.bone == bone).collect();
        keys.sort_by(|a, b| a.t.total_cmp(&b.t));
        keys
    }

    pub fn bone_keys(&self, bone: usize) -> Vec<&Key> {
        let mut keys: Vec<&Key> = self.keys.iter().filter(|k| k.bone == bone).collect();
        keys.sort_by(|a, b| a.t.total_cmp(&b.t));
        keys
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rig {
    pub bones: Vec<Bone>,
    pub skin: Option<Weights>,
    pub pose: Pose,
    pub moved: Moves,
    pub clips: Vec<Clip>,
}

#[derive(Clone, Copy, Debug)]
pub struct Xf {
    pub r: [[f64; 3]; 3],
    pub t: P3,
}

impl Xf {
    pub fn apply(&self, p: P3) -> P3 {
        add(super::apply(&self.r, p), self.t)
    }

    pub fn then(&self, inner: &Xf) -> Xf {
        let r = std::array::from_fn(|i| {
            std::array::from_fn(|j| (0..3).map(|k| self.r[i][k] * inner.r[k][j]).sum())
        });
        Xf {
            r,
            t: self.apply(inner.t),
        }
    }
}

pub fn quat_axis_angle(axis: P3, degrees: f64) -> Result<Quat> {
    let n = length(axis);
    if n < 1.0e-12 {
        bail!("the rotation axis must not be zero");
    }
    let half = degrees.to_radians() / 2.0;
    let s = half.sin() / n;
    Ok([axis[0] * s, axis[1] * s, axis[2] * s, half.cos()])
}

pub fn quat_matrix(q: Quat) -> [[f64; 3]; 3] {
    let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    let [x, y, z, w] = q.map(|v| v / n);
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

pub fn slerp(a: Quat, b: Quat, t: f64) -> Quat {
    let mut cos: f64 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
    let mut b = b;
    if cos < 0.0 {
        cos = -cos;
        b = b.map(|v| -v);
    }
    let (wa, wb) = if cos > 0.9995 {
        (1.0 - t, t)
    } else {
        let angle = cos.acos();
        let s = angle.sin();
        (((1.0 - t) * angle).sin() / s, (t * angle).sin() / s)
    };
    let q: Quat = std::array::from_fn(|k| a[k] * wa + b[k] * wb);
    let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    q.map(|v| v / n)
}

fn segment_distance(p: P3, a: P3, b: P3) -> f64 {
    let ab = sub(b, a);
    let len2 = super::dot(ab, ab);
    let t = if len2 > 1.0e-18 {
        (super::dot(sub(p, a), ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    length(sub(p, add(a, scale(ab, t))))
}

fn trim(mut weights: Vec<(usize, f64)>) -> Vec<(usize, f64)> {
    weights.retain(|(_, w)| *w > 0.0);
    weights.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    weights.truncate(MAX_INFLUENCES);
    let sum: f64 = weights.iter().map(|(_, w)| w).sum();
    weights.iter_mut().for_each(|(_, w)| *w /= sum);
    weights
}

impl Rig {
    pub fn is_empty(&self) -> bool {
        self.bones.is_empty()
    }

    pub fn bone_index(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }

    pub fn bone(&self, name: &str) -> Result<usize> {
        self.bone_index(name).ok_or_else(|| {
            let names: Vec<&str> = self.bones.iter().map(|b| b.name.as_str()).collect();
            anyhow::anyhow!(
                "no bone called `{name}`; the bones are {}",
                if names.is_empty() {
                    "none yet".to_string()
                } else {
                    names.join(", ")
                }
            )
        })
    }

    pub fn children(&self, bone: usize) -> Vec<usize> {
        (0..self.bones.len())
            .filter(|&i| self.bones[i].parent == Some(bone))
            .collect()
    }

    pub fn clip(&self, name: &str) -> Result<usize> {
        self.clips
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| {
                let names: Vec<&str> = self.clips.iter().map(|c| c.name.as_str()).collect();
                anyhow::anyhow!(
                    "no clip called `{name}`; the clips are {}",
                    if names.is_empty() {
                        "none yet".to_string()
                    } else {
                        names.join(", ")
                    }
                )
            })
    }

    pub fn skin_fits(&self, mesh: &Mesh) -> bool {
        self.skin
            .as_ref()
            .is_some_and(|s| s.len() == mesh.positions.len())
    }

    pub fn is_posed(&self) -> bool {
        self.pose.values().any(|q| *q != IDENTITY_QUAT)
            || self.moved.values().any(|m| *m != [0.0; 3])
    }

    pub fn local_offset(&self, bone: usize) -> P3 {
        let b = &self.bones[bone];
        match b.parent {
            Some(p) => sub(b.head, self.bones[p].head),
            None => b.head,
        }
    }

    pub fn posed_offset(&self, bone: usize) -> P3 {
        add(
            self.local_offset(bone),
            self.moved.get(&bone).copied().unwrap_or([0.0; 3]),
        )
    }

    pub fn world(&self, pose: &Pose) -> Vec<Xf> {
        let mut out: Vec<Xf> = Vec::with_capacity(self.bones.len());
        for (i, b) in self.bones.iter().enumerate() {
            let local = Xf {
                r: quat_matrix(pose.get(&i).copied().unwrap_or(IDENTITY_QUAT)),
                t: self.posed_offset(i),
            };
            out.push(match b.parent {
                Some(p) => out[p].then(&local),
                None => local,
            });
        }
        out
    }

    pub fn skin_transforms(&self, pose: &Pose) -> Vec<Xf> {
        self.world(pose)
            .into_iter()
            .zip(&self.bones)
            .map(|(w, b)| Xf {
                r: w.r,
                t: sub(w.t, super::apply(&w.r, b.head)),
            })
            .collect()
    }

    pub fn posed(&self, mesh: &Mesh, pose: &Pose) -> Mesh {
        let Some(skin) = self
            .skin
            .as_ref()
            .filter(|s| s.len() == mesh.positions.len())
        else {
            return mesh.clone();
        };
        let xf = self.skin_transforms(pose);
        let mut out = mesh.clone();
        for (p, weights) in out.positions.iter_mut().zip(skin) {
            let v = *p;
            *p = weights
                .iter()
                .fold([0.0; 3], |sum, (b, w)| add(sum, scale(xf[*b].apply(v), *w)));
        }
        out
    }

    pub fn sample(&self, clip: usize, t: f64) -> Pose {
        let clip = &self.clips[clip];
        let mut pose = Pose::new();
        for bone in 0..self.bones.len() {
            let keys = clip.bone_keys(bone);
            let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
                continue;
            };
            let q = if t <= first.t {
                first.rotation
            } else if t >= last.t {
                last.rotation
            } else {
                let next = keys.iter().position(|k| k.t > t).unwrap_or(keys.len() - 1);
                let (a, b) = (keys[next - 1], keys[next]);
                slerp(a.rotation, b.rotation, (t - a.t) / (b.t - a.t))
            };
            pose.insert(bone, q);
        }
        pose
    }

    pub fn sample_moves(&self, clip: usize, t: f64) -> Moves {
        let clip = &self.clips[clip];
        let mut moved = Moves::new();
        for bone in 0..self.bones.len() {
            let keys = clip.bone_moves(bone);
            let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
                continue;
            };
            let m = if t <= first.t {
                first.offset
            } else if t >= last.t {
                last.offset
            } else {
                let next = keys.iter().position(|k| k.t > t).unwrap_or(keys.len() - 1);
                let (a, b) = (keys[next - 1], keys[next]);
                let f = (t - a.t) / (b.t - a.t);
                add(a.offset, scale(sub(b.offset, a.offset), f))
            };
            moved.insert(bone, m);
        }
        moved
    }

    pub fn ensure_skin(&mut self, vertices: usize) -> &mut Weights {
        self.skin
            .get_or_insert_with(|| vec![vec![(0, 1.0)]; vertices])
    }

    pub fn auto_skin(&mut self, mesh: &Mesh, power: f64) {
        let (lo, hi) = mesh.bounds().unwrap_or_default();
        let eps = (length(sub(hi, lo)) * 1.0e-3).max(1.0e-9);
        let bones = self.bones.clone();
        let weights = mesh
            .positions
            .iter()
            .map(|&p| {
                let d: Vec<f64> = bones
                    .iter()
                    .map(|b| segment_distance(p, b.head, b.tail) + eps)
                    .collect();
                let nearest = d.iter().copied().fold(f64::INFINITY, f64::min);
                trim(
                    d.iter()
                        .enumerate()
                        .map(|(i, &d)| (i, (nearest / d).powf(power)))
                        .filter(|(_, w)| *w > 1.0e-4)
                        .collect(),
                )
            })
            .collect();
        self.skin = Some(weights);
    }

    pub fn bind_part(&mut self, mesh: &Mesh, part: u16, bone: usize) -> usize {
        let skin = self.ensure_skin(mesh.positions.len());
        let mut count = 0;
        for (i, w) in skin.iter_mut().enumerate() {
            if mesh.labels.get(i) == Some(&part) {
                *w = vec![(bone, 1.0)];
                count += 1;
            }
        }
        count
    }

    pub fn paint(
        &mut self,
        mesh: &Mesh,
        bone: usize,
        centre: P3,
        radius: f64,
        falloff: f64,
        strength: f64,
    ) -> usize {
        let skin = self.ensure_skin(mesh.positions.len());
        let mut count = 0;
        for (p, w) in mesh.positions.iter().zip(skin.iter_mut()) {
            let d = length(sub(*p, centre));
            let profile = if d <= radius {
                1.0
            } else if falloff > 0.0 && d < radius + falloff {
                1.0 - smoothstep((d - radius) / falloff)
            } else {
                0.0
            };
            let k = (profile * strength).clamp(0.0, 1.0);
            if k <= 0.0 {
                continue;
            }
            let mut next: Vec<(usize, f64)> = w.iter().map(|&(b, v)| (b, v * (1.0 - k))).collect();
            match next.iter_mut().find(|(b, _)| *b == bone) {
                Some((_, v)) => *v += k,
                None => next.push((bone, k)),
            }
            *w = trim(next);
            count += 1;
        }
        count
    }

    pub fn describe(&self) -> String {
        let n = |v: f64| {
            let s = format!("{v:.3}");
            let s = s.trim_end_matches('0').trim_end_matches('.');
            if s.is_empty() || s == "-0" {
                "0".to_string()
            } else {
                s.to_string()
            }
        };
        let p = |q: P3| format!("{},{},{}", n(q[0]), n(q[1]), n(q[2]));
        let mut out = format!(
            "skeleton of {} bone(s){}\n",
            self.bones.len(),
            if self.skin.is_some() { ", skinned" } else { "" }
        );
        for b in &self.bones {
            let depth = std::iter::successors(b.parent, |&q| self.bones[q].parent).count();
            out.push_str(&format!(
                "{}{} (parent {}) head {} tail {} length {}\n",
                "  ".repeat(depth + 1),
                b.name,
                b.parent.map_or("none", |q| self.bones[q].name.as_str()),
                p(b.head),
                p(b.tail),
                n(length(sub(b.tail, b.head))),
            ));
        }
        for c in &self.clips {
            out.push_str(&format!(
                "clip {} at {} fps, {} key(s), {} s\n",
                c.name,
                n(c.fps),
                c.keys.len(),
                n(c.duration())
            ));
        }
        out
    }
}
