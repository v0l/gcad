use gcad_mesh::gltf::{Fit, fit};
use serde_json::Value;

fn creature() -> Vec<u8> {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/mesh/rigged_creature.gcad"
    ))
    .expect("example");
    gcad_mesh::to_glb(&source).expect("builds")
}

fn json(glb: &[u8]) -> Value {
    let n = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&glb[20..20 + n]).expect("json")
}

fn bounds(doc: &Value) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for mesh in doc["meshes"].as_array().unwrap() {
        for p in mesh["primitives"].as_array().unwrap() {
            let a = &doc["accessors"][p["attributes"]["POSITION"].as_u64().unwrap() as usize];
            for k in 0..3 {
                lo[k] = lo[k].min(a["min"][k].as_f64().unwrap());
                hi[k] = hi[k].max(a["max"][k].as_f64().unwrap());
            }
        }
    }
    (lo, hi)
}

fn binary_chunk(glb: &[u8]) -> &[u8] {
    let n = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    &glb[20 + n..]
}

#[test]
fn height_scales_the_model_to_that_height_with_feet_on_the_ground() {
    let raw = creature();
    let fitted = fit(
        &raw,
        &Fit {
            height: Some(1.0),
            ..Fit::default()
        },
    )
    .expect("fits");
    let (before, after) = (json(&raw), json(&fitted));
    let (lo, hi) = bounds(&before);
    let roots = after["scenes"][0]["nodes"].as_array().unwrap();
    assert_eq!(roots.len(), 1);
    let model = &after["nodes"][roots[0].as_u64().unwrap() as usize];
    assert_eq!(model["name"], "model");
    assert_eq!(model["children"], before["scenes"][0]["nodes"]);
    let k = model["scale"][0].as_f64().unwrap();
    let t = &model["translation"];
    assert!(((hi[1] - lo[1]) * k - 1.0).abs() < 1e-9, "height {k}");
    assert!((lo[1] * k + t[1].as_f64().unwrap()).abs() < 1e-9);
    assert_eq!(t[0].as_f64(), Some(0.0));
    assert_eq!(binary_chunk(&raw), binary_chunk(&fitted));
    assert_eq!(after["animations"], before["animations"]);
    assert_eq!(after["skins"], before["skins"]);
    let total = u32::from_le_bytes(fitted[8..12].try_into().unwrap()) as usize;
    assert_eq!(total, fitted.len());
}

#[test]
fn centre_puts_the_middle_on_the_vertical_axis() {
    let raw = creature();
    let fitted = fit(
        &raw,
        &Fit {
            scale: Some(0.01),
            centre: true,
            ..Fit::default()
        },
    )
    .expect("fits");
    let (lo, hi) = bounds(&json(&raw));
    let doc = json(&fitted);
    let model = &doc["nodes"][doc["scenes"][0]["nodes"][0].as_u64().unwrap() as usize];
    let t: Vec<f64> = (0..3)
        .map(|i| model["translation"][i].as_f64().unwrap())
        .collect();
    assert_eq!(model["scale"][1].as_f64(), Some(0.01));
    for k in [0, 2] {
        assert!(((lo[k] + hi[k]) / 2.0 * 0.01 + t[k]).abs() < 1e-9);
    }
    assert!((lo[1] * 0.01 + t[1]).abs() < 1e-9);
}

#[test]
fn height_and_scale_together_are_refused() {
    let error = fit(
        &creature(),
        &Fit {
            height: Some(1.0),
            scale: Some(2.0),
            ..Fit::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("height"), "{error}");
}
