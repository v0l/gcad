use super::{
    Mesh,
    animation::{Clip, Interpolation, Track, Values},
    scene::{Node, Scene, Transform},
    skin::Weights,
};
use crate::mesh::rig::{MAX_INFLUENCES, Rig};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

pub struct MeshBody {
    pub mesh: Mesh,
    pub rig: Rig,
}

pub fn convert(source: &crate::mesh::Mesh, rig: Option<&Rig>) -> Result<MeshBody> {
    ensure!(!source.positions.is_empty(), "mesh body has no vertices");
    let normals = source
        .vertex_normals()
        .into_iter()
        .map(|n| {
            let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if length.is_finite() && (length - 1.0).abs() < 1e-9 {
                n
            } else {
                [0.0, 0.0, 1.0]
            }
        })
        .collect();
    let mut mesh = Mesh {
        positions: source.positions.clone(),
        normals,
        texcoords: None,
        tangents: None,
        triangles: source.triangles.clone(),
    };
    mesh.triangles.retain(|t| {
        let [a, b, c] = t.map(|v| source.positions[v as usize].map(|x| f64::from(x as f32)));
        let n = crate::mesh::cross(crate::mesh::sub(b, a), crate::mesh::sub(c, a));
        let area_squared = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
        area_squared.is_finite() && area_squared > 0.0
    });
    mesh.validate()?;
    Ok(MeshBody {
        mesh,
        rig: rig.cloned().unwrap_or_default(),
    })
}

pub fn joint_weights(rig: &Rig, vertices: usize) -> Option<Vec<Weights>> {
    let skin = rig.skin.as_ref().filter(|s| s.len() == vertices)?;
    Some(
        skin.iter()
            .map(|influences| {
                let sum: f64 = influences
                    .iter()
                    .take(MAX_INFLUENCES)
                    .map(|(_, w)| w)
                    .sum();
                let mut result = Weights {
                    joints: [0; 4],
                    weights: [0.0; 4],
                };
                if sum <= 0.0 {
                    result.weights[0] = 1.0;
                    return result;
                }
                for (slot, (bone, weight)) in influences.iter().take(MAX_INFLUENCES).enumerate() {
                    result.joints[slot] = *bone as u16;
                    result.weights[slot] = weight / sum;
                }
                result
            })
            .collect(),
    )
}

pub fn place(scene: &mut Scene, owner: usize, rig: &Rig) -> Result<Vec<usize>> {
    ensure!(
        rig.bones.len() <= 65536,
        "a skin supports at most 65536 bones"
    );
    let base = scene.nodes.len();
    let owner_name = scene.nodes[owner].name.clone();
    for (i, bone) in rig.bones.iter().enumerate() {
        scene.nodes.push(Node {
            name: format!("{owner_name}/{}", bone.name),
            parent: Some(bone.parent.map_or(owner, |p| base + p)),
            mesh: None,
            transform: Transform {
                translation: rig.local_offset(i),
                ..Default::default()
            },
        });
    }
    Ok((base..base + rig.bones.len()).collect())
}

pub fn clips(rig: &Rig, joints: &[usize]) -> Result<Vec<Clip>> {
    rig.clips
        .iter()
        .map(|clip| {
            let mut tracks = Vec::new();
            for (bone, &node) in joints.iter().enumerate() {
                let keys = latest(clip.bone_keys(bone).into_iter().map(|k| (k.t, k.rotation)));
                if !keys.is_empty() {
                    tracks.push(Track {
                        node,
                        times: keys.iter().map(|k| k.0).collect(),
                        interpolation: Interpolation::Linear,
                        values: Values::Rotation(
                            keys.iter().map(|k| super::animation::normalized(k.1)).collect(),
                        ),
                    });
                }
                let rest = rig.local_offset(bone);
                let moves = latest(
                    clip.bone_moves(bone)
                        .into_iter()
                        .map(|k| (k.t, std::array::from_fn(|i| rest[i] + k.offset[i]))),
                );
                if !moves.is_empty() {
                    tracks.push(Track {
                        node,
                        times: moves.iter().map(|k| k.0).collect(),
                        interpolation: Interpolation::Linear,
                        values: Values::Translation(moves.iter().map(|k| k.1).collect()),
                    });
                }
            }
            Ok(Clip {
                name: clip.name.clone(),
                tracks,
            })
        })
        .collect()
}

fn latest<T: Copy>(keys: impl Iterator<Item = (f64, T)>) -> Vec<(f64, T)> {
    let mut by_time = BTreeMap::new();
    for (t, v) in keys {
        by_time.insert(t.to_bits(), (t, v));
    }
    let mut out: Vec<_> = by_time.into_values().collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{quantize, scene::SceneMesh, skin::Skin};

    const ARM: &str = "upper: limb 0,0,0 0,0,20 r=3\nlower: limb 0,0,20 0,0,40 r=3\nremesh 0.8\nbone b_up head=0,0,0 tail=0,0,20\nbone b_low parent=b_up tail=0,0,40\nskin auto\nclip bend fps=30\nkey t=0 bone=b_low rotate=0 axis=x\nkey t=1 bone=b_low rotate=80 axis=x\nkey t=0 bone=b_up move=0,0,0\nkey t=1 bone=b_up move=0,3,0\n";

    fn arm_scene() -> (Scene, crate::model::Model, Vec<usize>) {
        let lines = crate::parse::parse_program(ARM).unwrap();
        let run = crate::model::run(&lines);
        assert!(run.steps.iter().all(|(_, r)| r.is_ok()));
        let model = run.model;
        let (name, rest) = model.rest_meshes().remove(0);
        let body = convert(&rest, model.rig_of(&name)).unwrap();
        let mut scene = Scene {
            nodes: vec![Node {
                name: "arm".into(),
                mesh: Some(0),
                ..Default::default()
            }],
            meshes: vec![SceneMesh {
                name: "arm".into(),
                primitives: vec![crate::asset::scene::Primitive {
                    geometry: body.mesh.clone(),
                    material: 0,
                }],
            }],
            materials: vec![Default::default()],
            ..Default::default()
        };
        let joints = place(&mut scene, 0, &body.rig).unwrap();
        scene.clips = clips(&body.rig, &joints).unwrap();
        (scene, model, joints)
    }

    #[test]
    fn asset_skin_deformation_matches_mesh_body_linear_blend_skinning() {
        let (scene, model, joints) = arm_scene();
        let (name, rest) = model.rest_meshes().remove(0);
        let rig = model.rig_of(&name).unwrap();
        let body = convert(&rest, Some(rig)).unwrap();
        let skin = Skin::bind(
            &scene,
            0,
            joints,
            vec![joint_weights(rig, rest.positions.len()).unwrap()],
        )
        .unwrap();
        for t in [0.0, 0.3, 0.5, 1.0, 2.0] {
            let mut posed_rig = rig.clone();
            posed_rig.moved = rig.sample_moves(0, t);
            let expected = posed_rig.posed(&rest, &rig.sample(0, t));
            let pose = scene.clips[0].sample(&scene, t, false).unwrap();
            let actual = skin.deform_positions(&scene, &pose).unwrap();
            for (a, e) in actual[0].iter().zip(&expected.positions) {
                assert!(
                    (0..3).all(|k| (a[k] - e[k]).abs() < 1e-9),
                    "t={t}: {a:?} vs {e:?}"
                );
            }
            assert_eq!(actual[0].len(), body.mesh.positions.len());
        }
    }

    #[test]
    fn mesh_body_skin_prunes_and_quantises_through_the_asset_passes() {
        let (scene, model, joints) = arm_scene();
        let (name, rest) = model.rest_meshes().remove(0);
        let rig = model.rig_of(&name).unwrap();
        let skin = Skin::bind(
            &scene,
            0,
            joints,
            vec![joint_weights(rig, rest.positions.len()).unwrap()],
        )
        .unwrap();
        let (pruned, report) = skin.prune_palette(&scene, &[]).unwrap();
        assert_eq!(report.original_joints, 2);
        assert_eq!(pruned.weights[0].len(), rest.positions.len());
        let (bytes, quant) = quantize::encode(
            &scene,
            &[skin],
            &quantize::Options {
                samples_per_clip: 8,
                max_error_mm: 0.05,
                max_angle_degrees: 1.0,
            },
        )
        .unwrap();
        assert!(!bytes.is_empty() && quant.max_position_error_mm <= 0.05);
    }

    #[test]
    fn unskinned_vertices_bind_to_the_first_bone() {
        let mut rig = Rig::default();
        rig.skin = Some(vec![vec![], vec![(1, 2.0), (0, 2.0)]]);
        let weights = joint_weights(&rig, 2).unwrap();
        assert_eq!(weights[0].weights, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(weights[1].joints[..2], [1, 0]);
        assert_eq!(weights[1].weights[..2], [0.5, 0.5]);
        assert!(joint_weights(&rig, 3).is_none());
    }
}
