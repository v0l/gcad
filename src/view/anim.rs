use crate::mesh::Mesh;
use crate::mesh::rig::{Moves, Pose, Rig, quat_axis_angle};

#[derive(Clone, Debug, PartialEq)]
pub struct Playback {
    pub clip: Option<String>,
    pub time: f64,
    pub playing: bool,
    pub looping: bool,
    pub speed: f64,
    pub turns: Vec<(String, [f64; 3])>,
    pub skeleton: bool,
}

impl Default for Playback {
    fn default() -> Self {
        Playback {
            clip: None,
            time: 0.0,
            playing: false,
            looping: true,
            speed: 1.0,
            turns: Vec::new(),
            skeleton: false,
        }
    }
}

impl Playback {
    pub fn active(&self) -> bool {
        self.clip.is_some() || self.turns.iter().any(|(_, t)| *t != [0.0; 3])
    }

    pub fn clip_index(&self, rig: &Rig) -> Option<usize> {
        let name = self.clip.as_deref()?;
        rig.clips.iter().position(|c| c.name == name)
    }

    pub fn duration(&self, rig: &Rig) -> f64 {
        self.clip_index(rig)
            .map_or(0.0, |c| rig.clips[c].duration())
    }

    pub fn advance(&mut self, rig: &Rig, seconds: f64) {
        if !self.playing {
            return;
        }
        let duration = self.duration(rig);
        if duration <= 0.0 {
            self.playing = false;
            return;
        }
        self.time += seconds * self.speed;
        if self.looping {
            self.time = self.time.rem_euclid(duration);
        } else if self.time >= duration {
            self.time = duration;
            self.playing = false;
        } else if self.time < 0.0 {
            self.time = 0.0;
            self.playing = false;
        }
    }

    pub fn turn_of(&self, bone: &str) -> [f64; 3] {
        self.turns
            .iter()
            .find(|(b, _)| b == bone)
            .map_or([0.0; 3], |(_, t)| *t)
    }

    pub fn set_turn(&mut self, bone: &str, turn: [f64; 3]) {
        match self.turns.iter_mut().find(|(b, _)| b == bone) {
            Some(slot) => slot.1 = turn,
            None => self.turns.push((bone.to_string(), turn)),
        }
    }

    pub fn refit(&mut self, rig: &Rig) {
        if self.clip.is_some() && self.clip_index(rig).is_none() {
            self.clip = None;
            self.playing = false;
            self.time = 0.0;
        }
        self.time = self.time.clamp(0.0, self.duration(rig).max(0.0));
        self.turns.retain(|(b, _)| rig.bone_index(b).is_some());
    }

    pub fn reset(&mut self) {
        self.clip = None;
        self.time = 0.0;
        self.playing = false;
        self.turns.clear();
    }

    pub fn pose(&self, rig: &Rig) -> (Pose, Moves) {
        let (mut pose, moves) = match self.clip_index(rig) {
            Some(c) => (rig.sample(c, self.time), rig.sample_moves(c, self.time)),
            None => (rig.pose.clone(), rig.moved.clone()),
        };
        for (name, turn) in &self.turns {
            let Some(bone) = rig.bone_index(name) else {
                continue;
            };
            if *turn == [0.0; 3] {
                continue;
            }
            let mut q = pose.get(&bone).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]);
            for (axis, degrees) in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
                .into_iter()
                .zip(turn)
            {
                if let Ok(r) = quat_axis_angle(axis, *degrees) {
                    q = multiply(q, r);
                }
            }
            pose.insert(bone, q);
        }
        (pose, moves)
    }

    pub fn posed(&self, rig: &Rig, rest: &Mesh) -> Mesh {
        let (pose, moves) = self.pose(rig);
        let mut rig = rig.clone();
        rig.moved = moves;
        rig.posed(rest, &pose)
    }

    pub fn bones(&self, rig: &Rig) -> Vec<[[f64; 3]; 2]> {
        let (pose, moves) = self.pose(rig);
        let mut rig = rig.clone();
        rig.moved = moves;
        let world = rig.world(&pose);
        rig.bones
            .iter()
            .zip(&world)
            .map(|(b, w)| {
                let tail = crate::mesh::sub(b.tail, b.head);
                [w.t, crate::mesh::add(w.t, rotate(&w.r, tail))]
            })
            .collect()
    }
}

fn rotate(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|k| m[i][k] * v[k]).sum())
}

fn multiply(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

#[cfg(test)]
mod tests {
    use super::Playback;

    const CREATURE: &str = include_str!("../../examples/mesh/rigged_creature.gcad");

    fn model() -> crate::model::Model {
        let lines = crate::parse::parse_program(CREATURE).expect("parses");
        crate::model::run(&lines).model
    }

    #[test]
    fn a_clip_at_t_poses_the_vertices_skinned_positions_does() {
        let model = model();
        let (name, rest) = model.rest_meshes().pop().expect("a mesh");
        let rig = model.rig_of(&name).expect("a rig");
        let options = gcad_mesh::Options {
            y_up: false,
            ..Default::default()
        };
        let built = gcad_mesh::build_with(CREATURE, &options).expect("builds");
        let body = &built.bodies[0];
        for (clip, t) in [("walk", 0.3), ("idle", 1.4), ("walk", 0.9)] {
            let playback = Playback {
                clip: Some(clip.to_string()),
                time: t,
                ..Default::default()
            };
            let posed = playback.posed(rig, &rest);
            let expected = body.skinned_positions(&body.bone_matrices(body.clip_index(clip), t));
            assert_eq!(posed.positions.len(), expected.len());
            let worst = posed
                .positions
                .iter()
                .zip(&expected)
                .flat_map(|(a, b)| (0..3).map(move |k| (a[k] as f32 - b[k]).abs()))
                .fold(0.0f32, f32::max);
            assert!(worst < 1.0e-4, "{clip} at {t}: {worst}");
            let moved = posed
                .positions
                .iter()
                .zip(&rest.positions)
                .flat_map(|(a, b)| (0..3).map(move |k| (a[k] - b[k]).abs()))
                .fold(0.0, f64::max);
            assert!(moved > 0.1, "{clip} at {t} moves the mesh");
        }
    }

    #[test]
    fn stopped_with_no_clip_shows_the_files_own_pose() {
        let model = model();
        let (name, rest) = model.rest_meshes().pop().expect("a mesh");
        let rig = model.rig_of(&name).expect("a rig");
        let shown = Playback::default().posed(rig, &rest);
        let file = model.meshes().pop().expect("posed").1;
        assert_eq!(shown.positions, file.positions);
    }

    #[test]
    fn a_bone_slider_turns_only_what_hangs_from_it() {
        let model = model();
        let (name, rest) = model.rest_meshes().pop().expect("a mesh");
        let rig = model.rig_of(&name).expect("a rig");
        let mut playback = Playback::default();
        let before = playback.bones(rig);
        playback.set_turn("tail1", [0.0, 0.0, 40.0]);
        let after = playback.bones(rig);
        let tail1 = rig.bone_index("tail1").unwrap();
        let tail2 = rig.bone_index("tail2").unwrap();
        let head = rig.bone_index("head_b").unwrap();
        assert_eq!(before[tail1][0], after[tail1][0]);
        assert_ne!(before[tail1][1], after[tail1][1]);
        assert_ne!(before[tail2][1], after[tail2][1]);
        assert_eq!(before[head], after[head]);
        assert!(playback.active());
        assert_ne!(
            playback.posed(rig, &rest),
            Playback::default().posed(rig, &rest)
        );
        playback.reset();
        assert!(!playback.active());
    }

    #[test]
    fn a_reload_keeps_the_clip_and_time_while_the_clip_is_there() {
        let model = model();
        let (name, _) = model.rest_meshes().pop().expect("a mesh");
        let rig = model.rig_of(&name).expect("a rig");
        let mut playback = Playback {
            clip: Some("idle".to_string()),
            time: 1.25,
            playing: true,
            ..Default::default()
        };
        playback.set_turn("tail1", [0.0, 0.0, 10.0]);
        playback.set_turn("gone", [5.0, 0.0, 0.0]);
        let kept = {
            let mut p = playback.clone();
            p.refit(rig);
            p
        };
        assert_eq!(kept.clip.as_deref(), Some("idle"));
        assert_eq!((kept.time, kept.playing), (1.25, true));
        assert_eq!(kept.turns, vec![("tail1".to_string(), [0.0, 0.0, 10.0])]);
        let edited = CREATURE.replace("clip idle", "clip rest");
        let lines = crate::parse::parse_program(&edited).expect("parses");
        let model = crate::model::run(&lines).model;
        let rig = model.rig_of(&name).expect("a rig");
        playback.refit(rig);
        assert_eq!(
            (playback.clip, playback.playing, playback.time),
            (None, false, 0.0)
        );
    }

    #[test]
    fn playing_loops_or_stops_at_the_end() {
        let model = model();
        let (name, _) = model.rest_meshes().pop().expect("a mesh");
        let rig = model.rig_of(&name).expect("a rig");
        let mut playback = Playback {
            clip: Some("walk".to_string()),
            playing: true,
            speed: 2.0,
            ..Default::default()
        };
        playback.advance(rig, 0.3);
        assert!((playback.time - 0.6).abs() < 1.0e-9);
        playback.advance(rig, 0.3);
        assert!((playback.time - 0.2).abs() < 1.0e-9, "{}", playback.time);
        playback.looping = false;
        playback.advance(rig, 1.0);
        assert_eq!((playback.time, playback.playing), (1.0, false));
    }
}
