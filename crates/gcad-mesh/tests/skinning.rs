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
        let source =
            example(name).replace("\nskin auto\n", "\nskin auto method=distance power=4\n");
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
    assert_clean("rigged_creature", &r, 4);
    assert!(r.min_core >= 0.99, "{}", r.min_core);
    assert!(r.contested < 852 / 2, "{}", r.contested);
    assert!(
        r.quality.stretched <= 6 && r.quality.squashed == 0,
        "{:?}",
        r.quality
    );
}

#[test]
fn heat_weights_keep_the_stress_quadruped_clean() {
    let r = report(&example("skin_stress"));
    assert_clean("skin_stress", &r, 6);
    assert!(r.min_core >= 0.99, "{}", r.min_core);
    assert!(r.quality.squashed <= 8, "{:?}", r.quality);
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
