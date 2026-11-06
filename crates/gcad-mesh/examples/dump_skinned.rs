fn main() {
    let args: Vec<String> = std::env::args().collect();
    let source = std::fs::read_to_string(&args[1]).expect("spec");
    let clip_name = &args[2];
    let t: f64 = args[3].parse().expect("time");
    let model = gcad_mesh::build(&source).expect("builds");
    std::fs::write(&args[4], model.to_glb().expect("glb")).expect("write glb");
    let body = &model.bodies[0];
    let clip = body.clip_index(clip_name).expect("clip");
    let matrices = body.bone_matrices(Some(clip), t);
    let posed = body.skinned_positions(&matrices);
    let step = posed.len() / 2000;
    let rows: Vec<String> = (0..posed.len())
        .step_by(step)
        .map(|i| format!("[{i},{},{},{}]", posed[i][0], posed[i][1], posed[i][2]))
        .collect();
    std::fs::write(&args[5], format!("[{}]", rows.join(","))).expect("write json");
}
