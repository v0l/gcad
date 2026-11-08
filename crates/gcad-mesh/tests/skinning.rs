use gcad_mesh::mesh::skin_check::{SkinReport, check};

fn example(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/../../examples/mesh/{name}.gcad",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("example")
}

fn report(source: &str) -> SkinReport {
    let model = gcad_mesh::build(source).expect("builds");
    let body = &model.bodies[0];
    check(body.rest_mesh(), body.rig().expect("rig")).expect("skinned")
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

#[test]
fn distance_method_reproduces_the_old_auto_weights_byte_for_byte() {
    for (name, hash, len) in [
        ("rigged_creature", 0x9cd7b7c99b5c77d8, 477712),
        ("skin_arm", 0x9be69288ebd5feb5, 259912),
    ] {
        let source = example(name)
            .replace("\nrelax 6\n", "\n")
            .replace("\nskin auto\n", "\nskin auto method=distance power=4\n");
        let glb = gcad_mesh::to_glb(&source).expect("builds");
        assert_eq!((glb.len(), fnv(&glb)), (len, hash), "{name}");
    }
}

#[test]
fn skin_smooth_softens_a_hard_part_binding() {
    let base = "upper: limb 0,0,0 0,0,10 r=2\nlower: limb 0,0,10 0,0,20 r=2\nremesh 0.5\nbone a head=0,0,0 tail=0,0,10\nbone b parent=a tail=0,0,20\nskin part=upper bone=a\nskin part=lower bone=b\n";
    let hard = report(base);
    assert!(hard.max_jump > 0.9, "{}", hard.max_jump);
    let soft = report(&format!("{base}skin smooth sphere=0,0,10 r=4 passes=40\n"));
    assert!(soft.max_jump < 0.35, "{}", soft.max_jump);
    let one = report(&format!("{base}skin smooth bone=b passes=40\n"));
    assert!(one.max_jump < hard.max_jump);
}

#[test]
fn a_painted_stroke_and_its_mirror_match_and_smooth_softens_them() {
    let base = example("rigged_creature");
    let painted = |extra: &str| {
        let source = base.replace("\nskin auto\n", &format!("\nskin auto\n{extra}\n"));
        let model = gcad_mesh::build(&source).expect("builds");
        let body = &model.bodies[0];
        (body.rest_mesh().clone(), body.rig().unwrap().clone())
    };
    let (mesh, auto) = painted("");
    let (_, one) =
        painted("skin paint bone=hind_l sphere=3,8,8 3,7.5,8.5 r=1 strength=0.6 mirror=x");
    let near = |p: [f64; 3]| {
        (0..mesh.positions.len())
            .min_by(|a, b| {
                let d =
                    |v: usize| gcad_mesh::mesh::length(gcad_mesh::mesh::sub(mesh.positions[v], p));
                d(*a).total_cmp(&d(*b))
            })
            .unwrap()
    };
    let left = near([3.0, 8.0, 8.0]);
    let right = near([-3.0, 8.0, 8.0]);
    let weight = |rig: &gcad_mesh::mesh::rig::Rig, v: usize, bone: &str| {
        let b = rig.bone_index(bone).unwrap();
        rig.skin.as_ref().unwrap()[v]
            .iter()
            .find(|(x, _)| *x == b)
            .map_or(0.0, |(_, w)| *w)
    };
    assert!(weight(&one, left, "hind_l") >= weight(&auto, left, "hind_l"));
    let before = (weight(&auto, left, "hind_l") - weight(&auto, right, "hind_r")).abs();
    let gap = (weight(&one, left, "hind_l") - weight(&one, right, "hind_r")).abs();
    assert!(gap <= before + 0.05, "{gap} {before}");
    assert!(weight(&one, right, "hind_r") >= weight(&auto, right, "hind_r"));
    let (_, mirrored) = painted("skin paint bone=hind_l sphere=3,8,8 r=1 strength=1\nskin mirror");
    let swap = |w: &[(usize, f64)], rig: &gcad_mesh::mesh::rig::Rig| -> Vec<(String, f64)> {
        let mut out: Vec<(String, f64)> = w
            .iter()
            .map(|(b, x)| (rig.bones[*b].name.replace("_l", "_r"), *x))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    };
    let skin = mirrored.skin.as_ref().unwrap();
    assert_eq!(swap(&skin[left], &mirrored), swap(&skin[right], &mirrored));
    let (_, cleaned) = painted(
        "skin paint bone=hind_l sphere=3,8,8 r=1 strength=1\nskin smooth sphere=3,8,8 r=2 passes=20\nskin normalize",
    );
    let r = check(&mesh, &cleaned).unwrap();
    assert!(r.max_jump < 0.5, "{}", r.max_jump);
}

#[test]
fn dual_quaternion_blending_keeps_the_twisted_wrist_round() {
    let width = |extra: &str| {
        let source = example("skin_arm").replace("\nskin auto\n", &format!("\nskin auto\n{extra}"));
        let model = gcad_mesh::build(&source).expect("builds");
        let body = &model.bodies[0];
        let mut rig = body.rig().unwrap().clone();
        let clip = rig.clip("rom").unwrap();
        rig.pose = rig.sample(clip, 2.0);
        let posed = rig.posed(body.rest_mesh(), &rig.pose);
        posed
            .positions
            .iter()
            .filter(|p| (p[2] - 23.0).abs() < 0.3)
            .map(|p| (p[0] * p[0] + p[1] * p[1]).sqrt())
            .fold(f64::INFINITY, f64::min)
    };
    let linear = width("");
    let dq = width("skin blend dq=1\n");
    assert!(dq > linear * 1.15, "{linear} {dq}");
    let error = gcad_mesh::build(&format!("{}skin blend dq=2\n", example("skin_arm"))).unwrap_err();
    assert!(error.message.contains("from 0"), "{}", error.message);
}

#[test]
fn twist_segments_spread_the_wrist_roll_and_the_glb_plays_the_same_pose() {
    let source = example("skin_arm").replace(
        "bone lower_b parent=upper_b tail=0,0,23",
        "bone lower_b parent=upper_b tail=0,0,23 twist=3",
    );
    let model = gcad_mesh::build(&source).expect("builds");
    let body = &model.bodies[0];
    let rig = body.rig().unwrap();
    let names: Vec<&str> = rig.bones.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "upper_b",
            "lower_b",
            "lower_b_twist1",
            "lower_b_twist2",
            "hand_b"
        ]
    );
    assert_eq!(rig.bones[4].parent, Some(3));
    let wrist = |source: &str| {
        let model = gcad_mesh::build(source).expect("builds");
        let body = &model.bodies[0];
        let mut r = body.rig().unwrap().clone();
        let clip = r.clip("rom").unwrap();
        r.pose = r.sample(clip, 2.0);
        r.posed(body.rest_mesh(), &r.pose)
            .positions
            .iter()
            .filter(|p| (p[2] - 23.0).abs() < 0.3)
            .map(|p| (p[0] * p[0] + p[1] * p[1]).sqrt())
            .fold(f64::INFINITY, f64::min)
    };
    let (plain, twisted) = (wrist(&example("skin_arm")), wrist(&source));
    assert!(twisted > plain * 1.15, "{plain} {twisted}");
    let clip = body.clip_index("rom");
    let expected = {
        let mut r = rig.clone();
        r.pose = r.sample(clip.unwrap(), 1.7);
        r.posed(body.rest_mesh(), &r.pose)
    };
    let doc = {
        let glb = gcad_mesh::to_glb_with(
            &source,
            &gcad_mesh::Options {
                y_up: false,
                ..Default::default()
            },
        )
        .unwrap();
        let n = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        serde_json::from_slice::<serde_json::Value>(&glb[20..20 + n]).unwrap()
    };
    let channels = doc["animations"][0]["channels"].as_array().unwrap();
    let animated: Vec<u64> = channels
        .iter()
        .map(|c| c["target"]["node"].as_u64().unwrap())
        .collect();
    assert!(animated.len() >= 4, "{animated:?}");
    let options = gcad_mesh::Options {
        y_up: false,
        ..Default::default()
    };
    let built = gcad_mesh::build_with(&source, &options).unwrap();
    let b = &built.bodies[0];
    let library = b.skinned_positions(&b.bone_matrices(clip, 1.7));
    let gap = |p: &[[f32; 3]]| {
        p.iter()
            .zip(&expected.positions)
            .flat_map(|(a, e)| (0..3).map(move |k| (a[k] as f64 - e[k]).abs()))
            .fold(0.0, f64::max)
    };
    assert!(gap(&library) < 1.0e-3, "library {}", gap(&library));
    let played = b.skinned_positions(&played_matrices(b, "rom", 1.7));
    let worst = gap(&played);
    assert!(worst < 1.0e-3, "{worst}");
}

fn played_matrices(body: &gcad_mesh::Body, clip: &str, t: f32) -> Vec<gcad_mesh::Matrix> {
    let clip = &body.clips[body.clip_index(clip).unwrap()];
    let joints = &body.joints;
    let rotation = |joint: usize| -> [f64; 4] {
        let Some(track) = clip.tracks.iter().find(|tr| tr.joint == joint) else {
            return [0.0, 0.0, 0.0, 1.0];
        };
        let at = track
            .times
            .iter()
            .position(|x| *x > t)
            .unwrap_or(track.times.len());
        let q = if at == 0 {
            track.rotations[0]
        } else if at == track.times.len() {
            track.rotations[at - 1]
        } else {
            let (a, b) = (track.rotations[at - 1], track.rotations[at]);
            let f = (t - track.times[at - 1]) / (track.times[at] - track.times[at - 1]);
            let dot: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
            let b = if dot < 0.0 { b.map(|v| -v) } else { b };
            let angle = dot.abs().clamp(-1.0, 1.0).acos();
            let (wa, wb) = if angle < 1.0e-4 {
                (1.0 - f, f)
            } else {
                (
                    ((1.0 - f) * angle).sin() / angle.sin(),
                    (f * angle).sin() / angle.sin(),
                )
            };
            let q: Vec<f32> = (0..4).map(|k| a[k] * wa + b[k] * wb).collect();
            let n = q.iter().map(|v| v * v).sum::<f32>().sqrt();
            [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
        };
        q.map(|v| v as f64)
    };
    let mut world: Vec<([[f64; 3]; 3], [f64; 3])> = Vec::new();
    for (i, j) in joints.iter().enumerate() {
        let r = gcad_mesh::mesh::rig::quat_matrix(rotation(i));
        let head = j.head.map(|v| v as f64);
        let offset = match j.parent {
            Some(p) => {
                let ph = joints[p].head.map(|v| v as f64);
                [head[0] - ph[0], head[1] - ph[1], head[2] - ph[2]]
            }
            None => head,
        };
        world.push(match j.parent {
            Some(p) => {
                let (pr, pt) = world[p];
                let rr = std::array::from_fn(|a| {
                    std::array::from_fn(|b| (0..3).map(|k| pr[a][k] * r[k][b]).sum())
                });
                let tt = std::array::from_fn(|a| {
                    pt[a] + (0..3).map(|k| pr[a][k] * offset[k]).sum::<f64>()
                });
                (rr, tt)
            }
            None => (r, offset),
        });
    }
    world
        .iter()
        .zip(joints)
        .map(|((r, t), j)| {
            let head = j.head.map(|v| v as f64);
            let tt: [f64; 3] =
                std::array::from_fn(|a| t[a] - (0..3).map(|k| r[a][k] * head[k]).sum::<f64>());
            let mut m = [0.0f32; 16];
            for c in 0..3 {
                for row in 0..3 {
                    m[c * 4 + row] = r[row][c] as f32;
                }
                m[12 + c] = tt[c] as f32;
            }
            m[15] = 1.0;
            m
        })
        .collect()
}

#[test]
fn skin_auto_refuses_bad_settings() {
    let base = "arm: limb 0,0,0 0,0,10 r=2\nremesh 0.8\nbone a head=0,0,0 tail=0,0,10\n";
    for (line, words) in [
        ("skin auto method=heat2", "smooth, heat or distance"),
        ("skin auto core=2", "core is a fraction"),
        ("skin auto passes=1.5", "whole number"),
        ("skin auto unrelated=maybe", "true or false"),
        ("skin smooth", "bind them first"),
        (
            "skin paint bone=a sphere=0,0,5 r=1 strength=2",
            "from -1 to 1",
        ),
        ("skin auto 1,2,3", "take more points"),
        ("skin mirror", "bind them first"),
    ] {
        let error = gcad_mesh::build(&format!("{base}{line}\n")).unwrap_err();
        assert!(error.message.contains(words), "{line}: {}", error.message);
    }
}

fn assert_clean(name: &str, r: &SkinReport, flips: usize) {
    assert!(r.max_jump < 0.35, "{name} max jump {}", r.max_jump);
    assert_eq!(r.steep_edges, 0, "{name}");
    assert_eq!(r.unrelated, 0, "{name}");
    assert_eq!(r.influences[0], 0, "{name}");
    assert!(r.min_core >= 0.85, "{name} core {}", r.min_core);
    assert!(r.quality.flipped <= flips, "{name} {:?}", r.quality);
}

fn with_method(name: &str, method: &str) -> String {
    example(name).replace("\nskin auto\n", &format!("\nskin auto method={method}\n"))
}

#[test]
fn heat_weights_keep_the_creature_clean() {
    let r = report(&example("rigged_creature"));
    assert_clean("rigged_creature", &r, 0);
    assert!(r.min_core >= 0.99, "{}", r.min_core);
    assert!(r.contested < 852 / 2, "{}", r.contested);
    assert!(
        r.quality.stretched == 0 && r.quality.squashed == 0,
        "{:?}",
        r.quality
    );
    assert!(r.joint_slivers.len() <= 2, "{}", r.joint_slivers.len());
}

#[test]
fn heat_weights_keep_the_stress_quadruped_clean() {
    let r = report(&example("skin_stress"));
    assert_clean("skin_stress", &r, 4);
    assert!(r.min_core >= 0.99, "{}", r.min_core);
    assert!(r.quality.squashed <= 8, "{:?}", r.quality);
    assert!(r.joint_slivers.len() <= 4, "{}", r.joint_slivers.len());
}

#[test]
fn heat_weights_halve_the_squashed_wrist_of_the_arm() {
    let r = report(&example("skin_arm"));
    assert_clean("skin_arm", &r, 30);
    assert!(r.quality.squashed <= 166 / 2, "{:?}", r.quality);
}

#[test]
fn heat_is_the_default_method() {
    assert_eq!(
        report(&example("skin_arm")),
        report(&with_method("skin_arm", "heat"))
    );
}

#[test]
fn smoothed_auto_weights_keep_the_creature_clean() {
    let r = report(&with_method("rigged_creature", "smooth"));
    assert_clean("rigged_creature", &r, 8);
    assert!(r.contested < 852 / 2, "{}", r.contested);
    assert!(
        r.quality.stretched <= 10 && r.quality.squashed <= 2,
        "{:?}",
        r.quality
    );
}

#[test]
fn smoothed_auto_weights_keep_the_stress_quadruped_clean() {
    let r = report(&with_method("skin_stress", "smooth"));
    assert_clean("skin_stress", &r, 8);
    assert!(r.quality.squashed <= 12, "{:?}", r.quality);
}

#[test]
fn smoothed_auto_weights_keep_the_arm_clean() {
    let r = report(&with_method("skin_arm", "smooth"));
    assert_clean("skin_arm", &r, 16);
}

#[test]
fn refine_joints_adds_rings_at_the_elbow_and_the_bend_stops_flipping() {
    let plain = example("skin_arm");
    let rigged = plain.replace(
        "\nskin auto\n",
        "\nrefine joints edge=0.3 reach=1.8\nskin auto\nskin smooth sphere=0,0,12 r=5 passes=300\n",
    );
    let before = report(&plain);
    let after = report(&rigged);
    let triangles = |source: &str| {
        let model = gcad_mesh::build(source).expect("builds");
        model.bodies[0].rest_mesh().triangles.len()
    };
    assert_eq!(before.quality.flipped, 28);
    assert!(after.quality.flipped < 5, "{}", after.quality.flipped);
    let (a, b) = (triangles(&plain), triangles(&rigged));
    assert!(b > a && (b as f64) < a as f64 * 1.2, "{a} -> {b}");
}

#[test]
fn refine_joints_needs_bones_and_an_unskinned_mesh() {
    let plain = example("skin_arm");
    let early = plain.replace("\nbone upper_b", "\nrefine joints edge=0.3\nbone upper_b");
    let message = gcad_mesh::build(&early).err().expect("fails").to_string();
    assert!(message.contains("bone"), "{message}");
    let late = plain.replace("\nclip rom", "\nrefine joints edge=0.3 reach=1.8\nclip rom");
    let message = gcad_mesh::build(&late).err().expect("fails").to_string();
    assert!(message.contains("before `skin auto`"), "{message}");
}

fn glb_json(glb: &[u8]) -> serde_json::Value {
    let n = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&glb[20..20 + n]).expect("json")
}

fn glb_floats(glb: &[u8], doc: &serde_json::Value, accessor: u64) -> Vec<f32> {
    let n = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let bin = &glb[20 + n + 8..];
    let a = &doc["accessors"][accessor as usize];
    let view = &doc["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
    let start = view["byteOffset"].as_u64().unwrap() as usize;
    let width = match a["type"].as_str().unwrap() {
        "SCALAR" => 1,
        "VEC3" => 3,
        other => panic!("{other}"),
    };
    let count = a["count"].as_u64().unwrap() as usize * width;
    bin[start..start + count * 4]
        .chunks(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

#[test]
fn a_corrective_removes_the_elbow_flips() {
    let plain = report(&example("skin_arm"));
    assert!(plain.quality.flipped > 0);
    let fixed = report(&example("skin_arm_corrective"));
    assert_eq!(fixed.quality.flipped, 0);
}

#[test]
fn the_morph_target_plays_the_same_pose_as_rig_posed() {
    let source = example("skin_arm_corrective");
    let model = gcad_mesh::build(&source).expect("builds");
    let body = &model.bodies[0];
    let rig = body.rig().expect("rig");
    assert_eq!(rig.correctives.len(), 1);
    let glb = gcad_mesh::to_glb(&source).expect("builds");
    let doc = glb_json(&glb);
    let mesh = &doc["meshes"][0];
    assert_eq!(mesh["extras"]["targetNames"][0], "elbow");
    let target = mesh["primitives"][0]["targets"][0]["POSITION"]
        .as_u64()
        .unwrap();
    let delta = glb_floats(&glb, &doc, target);
    let channel = doc["animations"][0]["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["target"]["path"] == "weights")
        .expect("weights channel");
    let sampler = &doc["animations"][0]["samplers"][channel["sampler"].as_u64().unwrap() as usize];
    let times = glb_floats(&glb, &doc, sampler["input"].as_u64().unwrap());
    let weights = glb_floats(&glb, &doc, sampler["output"].as_u64().unwrap());
    let clip = rig.clip("rom").unwrap();
    let mut elbow_full = false;
    for (t, w) in times.iter().zip(&weights) {
        let pose = rig.sample(clip, f64::from(*t));
        let expected = rig.posed(body.rest_mesh(), &pose);
        let mut bare = rig.clone();
        bare.correctives.clear();
        let mut shifted = body.rest_mesh().clone();
        for (p, d) in shifted.positions.iter_mut().zip(delta.chunks(3)) {
            let d = [f64::from(d[0]), -f64::from(d[2]), f64::from(d[1])];
            for k in 0..3 {
                p[k] += d[k] * f64::from(*w);
            }
        }
        let played = bare.posed(&shifted, &pose);
        for (a, b) in played.positions.iter().zip(&expected.positions) {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() < 1.0e-3, "t={t}");
            }
        }
        elbow_full |= *w == 1.0;
    }
    assert!(elbow_full);
}
