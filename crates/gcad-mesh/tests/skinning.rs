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
fn skin_auto_refuses_bad_settings() {
    let base = "arm: limb 0,0,0 0,0,10 r=2\nremesh 0.8\nbone a head=0,0,0 tail=0,0,10\n";
    for (line, words) in [
        ("skin auto method=heat2", "smooth or distance"),
        ("skin auto core=2", "core is a fraction"),
        ("skin auto passes=1.5", "whole number"),
        ("skin auto unrelated=maybe", "true or false"),
        ("skin smooth", "bind them first"),
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

#[test]
fn smoothed_auto_weights_keep_the_creature_clean() {
    let r = report(&example("rigged_creature"));
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
    let r = report(&example("skin_stress"));
    assert_clean("skin_stress", &r, 8);
    assert!(r.quality.squashed <= 12, "{:?}", r.quality);
}

#[test]
fn smoothed_auto_weights_keep_the_arm_clean() {
    let r = report(&example("skin_arm"));
    assert_clean("skin_arm", &r, 16);
}
