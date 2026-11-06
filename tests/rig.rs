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
    assert!(
        message.contains("b_up") && message.contains("b_low"),
        "{message}"
    );
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
    assert!(
        text.contains("2 bone(s)") && text.contains("b_low (parent b_up)"),
        "{text}"
    );
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
fn skin_auto_does_not_bind_the_body_to_an_arm_beside_it() {
    let model = build(
        "torso: blob 0,0,20 r=6,4,12\narm_l: limb 8.5,0,30 8.5,0,10 r=2 blend=0.5\nremesh 0.5\nbone spine head=0,0,8 tail=0,0,32\nbone arm_l head=8.5,0,30 tail=8.5,0,10\nskin auto",
    );
    let rig = rig_of(&model);
    let mesh = rest_of(&model);
    let skin = rig.skin.as_ref().unwrap();
    let torso = mesh.parts.iter().position(|p| p == "torso").unwrap() as u16;
    let worst = mesh
        .labels
        .iter()
        .zip(skin)
        .filter(|(l, _)| **l == torso)
        .zip(&mesh.positions)
        .filter(|(_, p)| p[0] < 4.0)
        .map(|((_, w), _)| {
            w.iter()
                .filter(|(b, _)| *b == 1)
                .map(|(_, w)| *w)
                .sum::<f64>()
        })
        .fold(0.0, f64::max);
    assert!(worst < 0.05, "torso vertices carry {worst:.2} of the arm");
}

#[test]
fn skin_auto_does_not_bind_a_leg_to_a_hand_hanging_beside_it() {
    let model = build(
        "leg_l: limb 6,0,40 6,0,0 r=4\narm_l: limb 14,0,70 14,0,44 r=2\nremesh 0.5\nbone thigh head=6,0,40 tail=6,0,0\nbone arm head=14,0,70 tail=14,0,44\nbone hand parent=arm tail=14,0,38\nskin auto",
    );
    let rig = rig_of(&model);
    let mesh = rest_of(&model);
    let skin = rig.skin.as_ref().unwrap();
    let leg = mesh.parts.iter().position(|p| p == "leg_l").unwrap() as u16;
    let worst = mesh
        .labels
        .iter()
        .zip(skin)
        .filter(|(l, _)| **l == leg)
        .map(|(_, w)| {
            w.iter()
                .filter(|(b, _)| *b != 0)
                .map(|(_, w)| *w)
                .sum::<f64>()
        })
        .fold(0.0, f64::max);
    assert!(
        worst < 0.02,
        "leg vertices carry {worst:.2} of the arm and hand"
    );
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
    assert!(
        message.contains("upper") && message.contains("lower"),
        "{message}"
    );
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
    assert!(*rest_of(&model) == *posed_of(&model));
    let reset = build(&format!(
        "{ARM}skin auto\npose bone=b_low rotate=40 axis=x\npose reset"
    ));
    assert!(*rest_of(&reset) == *posed_of(&reset));
}

#[test]
fn a_bent_limb_stays_closed_and_keeps_its_volume() {
    let bent = build(&format!("{ARM}skin auto\npose bone=b_low rotate=60 axis=x"));
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
    let message = failure(&format!(
        "{LIMB}bone a head=0,0,0 tail=0,0,20\npose bone=zz rotate=3 axis=x"
    ));
    assert!(
        message.contains("`zz`") && message.contains('a'),
        "{message}"
    );
}

#[test]
fn pose_without_skin_leaves_the_mesh_alone() {
    let model = build(&format!(
        "{LIMB}bone a head=0,0,0 tail=0,0,20\npose bone=a rotate=30 axis=x"
    ));
    assert!(rig_of(&model).skin.is_none());
    assert!(*rest_of(&model) == *posed_of(&model));
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
    assert!(
        (q[0] - expected).abs() < 1.0e-9 && (q[3] - 20.0_f64.to_radians().cos()).abs() < 1.0e-9
    );
    assert!(*rest_of(&model) != *posed_of(&model));
}

#[test]
fn move_keys_shift_a_bone_and_reach_the_glb() {
    let model = build(&format!(
        "{ARM}skin auto\nclip hop fps=30\nkey t=0 bone=b_up move=0,0,0\nkey t=1 bone=b_up move=0,0,10\nkey t=1 bone=b_low rotate=30 axis=x\npose clip=hop t=0.5"
    ));
    let rig = rig_of(&model);
    assert_eq!(rig.clips[0].moves.len(), 2);
    assert_eq!(rig.clips[0].keys.len(), 1);
    assert_eq!(rig.moved[&0], [0.0, 0.0, 5.0]);
    let lift = |m: &Mesh| {
        m.positions
            .iter()
            .map(|p| p[2])
            .fold(f64::INFINITY, f64::min)
    };
    let rise = lift(&posed_of(&model)) - lift(&rest_of(&model));
    assert!((rise - 5.0).abs() < 1.0e-2, "rose {rise}");
    let (json, bin) = glb_json(&gcad::export::glb_of(&model).expect("glb"));
    let channels = json["animations"][0]["channels"].as_array().unwrap();
    let moved = channels
        .iter()
        .find(|c| c["target"]["path"] == "translation")
        .expect("a translation channel");
    let sampler = &json["animations"][0]["samplers"][moved["sampler"].as_u64().unwrap() as usize];
    let out = floats(&json, &bin, sampler["output"].as_u64().unwrap() as usize);
    assert_eq!(out, [0.0, 0.0, 0.0, 0.0, 10.0, 0.0]);
}

#[test]
fn key_without_a_clip_is_refused() {
    let message = failure(&format!("{ARM}key t=0 bone=b_low rotate=3 axis=x"));
    assert!(message.contains("clip"), "{message}");
}

#[test]
fn inspect_report_lists_the_skeleton() {
    let model = build(ARM);
    let picture = gcad::inspect::inspect(
        &model,
        &gcad::inspect::Options {
            view: gcad::inspect::view("front").unwrap(),
            centre: None,
            span: None,
            part: None,
            grid: None,
            marks: vec![],
            picks: vec![],
        },
    )
    .unwrap();
    assert!(
        picture.report.contains("skeleton of 2 bone(s)"),
        "{}",
        picture.report
    );
    assert!(picture.report.contains("b_low (parent b_up)"));
}

fn glb_json(bytes: &[u8]) -> (serde_json::Value, Vec<u8>) {
    assert_eq!(&bytes[0..4], b"glTF");
    assert_eq!(
        u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
        bytes.len()
    );
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json = serde_json::from_slice(&bytes[20..20 + json_len]).expect("json chunk");
    let bin = bytes[28 + json_len..].to_vec();
    (json, bin)
}

fn floats(json: &serde_json::Value, bin: &[u8], accessor: usize) -> Vec<f32> {
    let a = &json["accessors"][accessor];
    let v = &json["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
    let width = match a["type"].as_str().unwrap() {
        "SCALAR" => 1,
        "VEC3" => 3,
        "VEC4" => 4,
        "MAT4" => 16,
        other => panic!("{other}"),
    };
    let start = v["byteOffset"].as_u64().unwrap() as usize;
    let n = a["count"].as_u64().unwrap() as usize * width;
    (0..n)
        .map(|k| f32::from_le_bytes(bin[start + 4 * k..start + 4 * k + 4].try_into().unwrap()))
        .collect()
}

#[test]
fn glb_carries_skin_weights_and_animations() {
    let model = build(&format!(
        "{ARM}skin auto\nclip bend fps=30\nkey t=0 bone=b_low rotate=0 axis=x\nkey t=1 bone=b_low rotate=80 axis=x"
    ));
    let bytes = gcad::export::glb_of(&model).expect("glb");
    let (json, bin) = glb_json(&bytes);
    assert_eq!(json["asset"]["version"], "2.0");
    let skin = &json["skins"][0];
    assert_eq!(skin["joints"].as_array().unwrap().len(), 2);
    let ibm = floats(
        &json,
        &bin,
        skin["inverseBindMatrices"].as_u64().unwrap() as usize,
    );
    assert_eq!(ibm.len(), 32);
    assert_eq!(&ibm[12..15], &[0.0, 0.0, 0.0]);
    assert_eq!(&ibm[28..31], &[0.0, -20.0, 0.0]);
    let prim = &json["meshes"][0]["primitives"][0]["attributes"];
    let weights = floats(&json, &bin, prim["WEIGHTS_0"].as_u64().unwrap() as usize);
    for w in weights.chunks(4) {
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1.0e-6);
    }
    let animation = &json["animations"][0];
    assert_eq!(animation["name"], "bend");
    let sampler = &animation["samplers"][0];
    let times = floats(&json, &bin, sampler["input"].as_u64().unwrap() as usize);
    assert_eq!(times, [0.0, 1.0]);
}

#[test]
fn glb_colours_are_linear() {
    let model = build(&format!("{LIMB}color #808080\n"));
    let (json, _) = glb_json(&gcad::export::glb_of(&model).expect("glb"));
    let base = json["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"][0]
        .as_f64()
        .unwrap();
    assert!((base - 0.2158).abs() < 1.0e-3, "{base}");
}

#[test]
fn glb_carries_solid_bodies_in_their_colours() {
    let model = build(
        "rect 20 10\nextrude 4\ncolor #ff0000\nbody post\nplane XY offset=4\ncircle 2\nextrude 10\ncolor #0000ff\n",
    );
    let (json, bin) = glb_json(&gcad::export::glb_of(&model).expect("glb"));
    let names: Vec<&str> = json["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["main", "post"]);
    let red = &json["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"];
    assert_eq!(red[0].as_f64().unwrap(), 1.0);
    assert_eq!(red[2].as_f64().unwrap(), 0.0);
    let attributes = &json["meshes"][1]["primitives"][0]["attributes"];
    let accessor = &json["accessors"][attributes["POSITION"].as_u64().unwrap() as usize];
    let top = accessor["max"][1].as_f64().unwrap();
    assert!((top - 14.0).abs() < 1.0e-6, "{top}");
    assert!(floats(&json, &bin, attributes["NORMAL"].as_u64().unwrap() as usize).len() > 0);
}

#[test]
fn glb_without_a_rig_has_no_skin() {
    let model = build(LIMB);
    let (json, _) = glb_json(&gcad::export::glb_of(&model).expect("glb"));
    assert!(json.get("skins").is_none() && json.get("animations").is_none());
}

#[test]
fn glb_exports_from_a_file_and_obj_is_posed() {
    let dir = std::path::PathBuf::from(scratch("rig_export"));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("rig.gcad");
    std::fs::write(
        &source,
        format!("{ARM}skin auto\npose bone=b_low rotate=60 axis=x\n"),
    )
    .unwrap();
    let run = gcad::model::run_path(&source, &[], None).expect("runs");
    let out = dir.join("rig.glb");
    gcad::export::export_model(&run.model, &source, out.to_str().unwrap()).expect("glb");
    assert!(std::fs::read(&out).unwrap().starts_with(b"glTF"));
    let obj = dir.join("rig.obj");
    gcad::export::export_model(&run.model, &source, obj.to_str().unwrap()).expect("obj");
    let text = std::fs::read_to_string(&obj).unwrap();
    let posed = gcad::mesh::read_obj(&text).expect("obj reads");
    assert_eq!(posed.positions.len(), rest_of(&run.model).positions.len());
    assert!(posed.bounds().unwrap().0[1] < -10.0);
}

#[test]
fn rigged_creature_example_poses_and_exports() {
    let path = std::path::PathBuf::from(format!(
        "{}/examples/mesh/rigged_creature.gcad",
        env!("CARGO_MANIFEST_DIR")
    ));
    let run = gcad::model::run_path(&path, &[], None).expect("runs");
    assert!(run.steps.iter().all(|(_, r)| r.is_ok()));
    let rig = run.model.rigs().last().unwrap().1.clone();
    assert!(rig.bones.len() >= 12 && rig.clips.len() == 2);
    let posed = run.model.meshes().last().unwrap().1.clone();
    assert_eq!(posed.open_edges(), 0);
    let rest = run.model.rest_meshes().last().unwrap().1.clone();
    assert!(*rest != *posed, "the walk pose moves the mesh");
    let ratio = posed.volume() / rest.volume();
    assert!((0.9..1.1).contains(&ratio), "{ratio}");
    assert!(
        gcad::export::glb_of(&run.model)
            .unwrap()
            .starts_with(b"glTF")
    );
}
