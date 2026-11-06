mod common;

use common::*;
use gcad::mesh::Mesh;
use gcad::model::Model;

const LIMB: &str = "arm: limb 0,0,0 0,0,20 r=3\nremesh 0.4\n";
const ARM: &str = "upper: limb 0,0,0 0,0,20 r=3\nlower: limb 0,0,20 0,0,40 r=3\nremesh 0.4\nbone b_up head=0,0,0 tail=0,0,20\nbone b_low parent=b_up tail=0,0,40\n";

fn rest_of(model: &Model) -> std::sync::Arc<Mesh> {
    model.rest_meshes().last().expect("mesh").1.clone()
}

fn posed_of(model: &Model) -> std::sync::Arc<Mesh> {
    model.meshes().last().expect("mesh").1.clone()
}

fn rig_of(model: &Model) -> &gcad::mesh::rig::Rig {
    model.rigs().last().expect("a rig").1
}

#[test]
fn bones_build_a_hierarchy() {
    let model = build(ARM);
    let rig = rig_of(&model);
    assert_eq!(rig.bones.len(), 2);
    assert_eq!(rig.bones[1].parent, Some(0));
    assert_eq!(rig.bones[1].head, [0.0, 0.0, 20.0]);
}

#[test]
fn bone_errors_name_the_bones() {
    let message = failure(&format!("{ARM}bone hand parent=elbow tail=0,0,50"));
    assert!(message.contains("b_up") && message.contains("b_low"), "{message}");
    let twice = failure(&format!("{ARM}bone b_up head=0,0,0 tail=0,0,1"));
    assert!(twice.contains("already"), "{twice}");
}

#[test]
fn mirror_bone_makes_the_other_side() {
    let model = build(
        "bone arm_l head=2,0,10 tail=6,0,10\nbone hand_l parent=arm_l tail=9,0,10\nmirror x of=arm_l\n",
    );
    let rig = rig_of(&model);
    let names: Vec<&str> = rig.bones.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["arm_l", "hand_l", "arm_r", "hand_r"]);
    assert_eq!(rig.bones[2].head, [-2.0, 0.0, 10.0]);
    assert_eq!(rig.bones[3].parent, Some(2));
    assert_eq!(rig.bones[3].tail, [-9.0, 0.0, 10.0]);
}

#[test]
fn inspect_lists_the_skeleton() {
    let model = build(ARM);
    let text = rig_of(&model).describe();
    assert!(text.contains("2 bone(s)") && text.contains("b_low (parent b_up)"), "{text}");
}

#[test]
fn skin_auto_weights_sum_to_one_with_four_influences_at_most() {
    let model = build(&format!(
        "{ARM}bone b_x parent=b_low tail=0,10,40\nbone b_y parent=b_low tail=10,0,40\nbone b_z parent=b_low tail=-10,0,40\nbone b_w parent=b_low tail=0,-10,40\nskin auto"
    ));
    let rig = rig_of(&model);
    let skin = rig.skin.as_ref().expect("skinned");
    assert_eq!(skin.len(), rest_of(&model).positions.len());
    let mut mixed = 0;
    for weights in skin {
        let sum: f64 = weights.iter().map(|(_, w)| w).sum();
        assert!((sum - 1.0).abs() < 1.0e-9, "sum {sum}");
        assert!(weights.len() <= 4);
        mixed += (weights.len() > 1) as usize;
    }
    assert!(mixed > 0);
}

#[test]
fn skin_auto_binds_each_end_to_its_own_bone() {
    let model = build(&format!("{ARM}skin auto"));
    let rest = rest_of(&model);
    let skin = rig_of(&model).skin.as_ref().expect("skinned");
    let lowest = (0..rest.positions.len())
        .min_by(|&a, &b| rest.positions[a][2].total_cmp(&rest.positions[b][2]))
        .unwrap();
    let highest = (0..rest.positions.len())
        .max_by(|&a, &b| rest.positions[a][2].total_cmp(&rest.positions[b][2]))
        .unwrap();
    assert_eq!(skin[lowest][0].0, 0);
    assert_eq!(skin[highest][0].0, 1);
}

#[test]
fn skin_part_moves_only_with_its_bone() {
    let model = build(&format!(
        "{ARM}skin part=upper bone=b_up\nskin part=lower bone=b_low\npose bone=b_low rotate=90 axis=x"
    ));
    let rest = rest_of(&model);
    let posed = posed_of(&model);
    let upper = rest.parts.iter().position(|p| p == "upper").unwrap() as u16;
    for i in 0..rest.positions.len() {
        let moved = (0..3).any(|k| (rest.positions[i][k] - posed.positions[i][k]).abs() > 1.0e-9);
        if rest.labels[i] == upper {
            assert!(!moved, "upper vertex {i} moved");
        }
    }
    let lower_moved = (0..rest.positions.len())
        .filter(|&i| rest.labels[i] != upper)
        .any(|i| rest.positions[i][2] > 30.0 && posed.positions[i][2] < 25.0);
    assert!(lower_moved);
}

#[test]
fn skin_part_unknown_lists_the_parts() {
    let message = failure(&format!("{ARM}skin part=head bone=b_up"));
    assert!(message.contains("upper") && message.contains("lower"), "{message}");
}

#[test]
fn skin_paint_moves_weight_to_the_bone() {
    let model = build(&format!(
        "{ARM}skin part=upper bone=b_up\nskin part=lower bone=b_low\nskin paint bone=b_low sphere=0,0,10 r=4 falloff=3"
    ));
    let rest = rest_of(&model);
    let skin = rig_of(&model).skin.as_ref().unwrap();
    let inside = (0..rest.positions.len())
        .find(|&i| {
            let p = rest.positions[i];
            (p[0] * p[0] + p[1] * p[1] + (p[2] - 10.0).powi(2)).sqrt() < 3.5 && rest.labels[i] == 0
        })
        .expect("a vertex in the sphere");
    assert_eq!(skin[inside], vec![(1, 1.0)]);
    let far = (0..rest.positions.len())
        .find(|&i| rest.positions[i][2] < -2.0)
        .unwrap();
    assert_eq!(skin[far], vec![(0, 1.0)]);
    for w in skin {
        assert!((w.iter().map(|(_, v)| v).sum::<f64>() - 1.0).abs() < 1.0e-9);
    }
}

#[test]
fn an_unposed_rig_equals_the_rest_mesh() {
    let model = build(&format!("{ARM}skin auto"));
    assert_eq!(*rest_of(&model), *posed_of(&model));
    let reset = build(&format!(
        "{ARM}skin auto\npose bone=b_low rotate=40 axis=x\npose reset"
    ));
    assert_eq!(*rest_of(&reset), *posed_of(&reset));
}

#[test]
fn a_bent_limb_stays_closed_and_keeps_its_volume() {
    let bent = build(&format!(
        "{ARM}skin auto\npose bone=b_low rotate=60 axis=x"
    ));
    let before = rest_of(&bent);
    let after = posed_of(&bent);
    assert_eq!(after.open_edges(), 0);
    assert_eq!(after.triangles, before.triangles);
    let ratio = after.volume() / before.volume();
    assert!((0.85..1.05).contains(&ratio), "volume ratio {ratio:.3}");
    let (lo, hi) = after.bounds().unwrap();
    assert!(lo[1] < -10.0, "the forearm swings toward -y: {lo:?}");
    assert!(hi[2] < 36.0);
}

#[test]
fn pose_rotates_about_the_bone_head() {
    let model = build(
        "arm: limb 0,0,0 0,0,20 r=2\nremesh 0.3\nbone a head=0,0,0 tail=0,0,20\nskin auto\npose bone=a rotate=90 axis=y",
    );
    let (lo, hi) = posed_of(&model).bounds().unwrap();
    assert!((hi[0] - 22.0).abs() < 0.3, "{hi:?}");
    assert!(lo[2] > -2.3 && hi[2] < 2.3);
}

#[test]
fn pose_names_the_bones_when_it_misses() {
    let message = failure(&format!("{LIMB}bone a head=0,0,0 tail=0,0,20\npose bone=zz rotate=3 axis=x"));
    assert!(message.contains("`zz`") && message.contains('a'), "{message}");
}

#[test]
fn pose_without_skin_leaves_the_mesh_alone() {
    let model = build(&format!("{LIMB}bone a head=0,0,0 tail=0,0,20\npose bone=a rotate=30 axis=x"));
    assert!(rig_of(&model).skin.is_none());
    assert_eq!(*rest_of(&model), *posed_of(&model));
}

#[test]
fn clip_keys_are_sampled_between_keyframes() {
    let model = build(&format!(
        "{ARM}skin auto\nclip bend fps=30\nkey t=0 bone=b_low rotate=0 axis=x\nkey t=1 bone=b_low rotate=80 axis=x\npose clip=bend t=0.5"
    ));
    let rig = rig_of(&model);
    assert_eq!(rig.clips.len(), 1);
    assert_eq!(rig.clips[0].fps, 30.0);
    assert_eq!(rig.clips[0].keys.len(), 2);
    let q = rig.pose[&1];
    let expected = (20.0_f64.to_radians()).sin();
    assert!((q[0] - expected).abs() < 1.0e-9 && (q[3] - 20.0_f64.to_radians().cos()).abs() < 1.0e-9);
    assert_ne!(*rest_of(&model), *posed_of(&model));
}

#[test]
fn key_without_a_clip_is_refused() {
    let message = failure(&format!("{ARM}key t=0 bone=b_low rotate=3 axis=x"));
    assert!(message.contains("clip"), "{message}");
}
