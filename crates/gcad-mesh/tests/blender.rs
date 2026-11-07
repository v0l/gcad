use gcad_mesh::mesh::skin_check::check;

fn example(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/../../examples/mesh/{name}.gcad",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("example")
}

type Matched = Vec<([f64; 3], Vec<(usize, f64)>)>;

#[test]
#[ignore = "needs BLENDER_WEIGHTS=dir with <fixture>.json from tools/blender/auto_weights.py"]
fn blender_automatic_weights_scored_by_skincheck() {
    let dir = std::env::var("BLENDER_WEIGHTS").expect("BLENDER_WEIGHTS");
    for name in ["rigged_creature", "skin_stress", "skin_arm"] {
        let Ok(text) = std::fs::read_to_string(format!("{dir}/{name}.json")) else {
            continue;
        };
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        let model = gcad_mesh::build(&example(name)).expect("builds");
        let body = &model.bodies[0];
        let mesh = body.rest_mesh();
        let mut rig = body.rig().unwrap().clone();
        let theirs: Matched = doc["vertices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                let p = v["p"].as_array().unwrap();
                let p = [
                    p[0].as_f64().unwrap(),
                    p[1].as_f64().unwrap(),
                    p[2].as_f64().unwrap(),
                ];
                let w = v["w"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|e| {
                        let bone = rig.bone_index(e[0].as_str()?)?;
                        Some((bone, e[1].as_f64()?))
                    })
                    .collect();
                (p, w)
            })
            .collect();
        let mut worst = 0.0f64;
        let skin = mesh
            .positions
            .iter()
            .map(|p| {
                let (q, w) = theirs
                    .iter()
                    .min_by(|a, b| {
                        let d = |x: &[f64; 3]| (0..3).map(|k| (x[k] - p[k]).powi(2)).sum::<f64>();
                        d(&a.0).total_cmp(&d(&b.0))
                    })
                    .unwrap();
                worst = worst.max((0..3).map(|k| (q[k] - p[k]).powi(2)).sum::<f64>().sqrt());
                let mut w = w.clone();
                w.sort_by(|a, b| b.1.total_cmp(&a.1));
                w.truncate(4);
                let sum: f64 = w.iter().map(|x| x.1).sum();
                if sum > 0.0 {
                    w.iter_mut().for_each(|x| x.1 /= sum);
                }
                w
            })
            .collect();
        rig.skin = Some(skin);
        let report = check(mesh, &rig).unwrap();
        println!("== {name} (blender automatic weights, matched within {worst:.4})\n{report}");
        let ours = check(mesh, body.rig().unwrap()).unwrap();
        println!("== {name} (gcad skin auto)\n{ours}");
    }
}
