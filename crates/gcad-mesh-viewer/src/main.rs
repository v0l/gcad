use macroquad::prelude::*;

const SPEC: &str = include_str!("../../../examples/mesh/rigged_creature.gcad");

fn window() -> Conf {
    Conf {
        window_title: "gcad-mesh viewer".into(),
        window_width: 960,
        window_height: 640,
        ..Default::default()
    }
}

fn mesh_of(body: &gcad_mesh::Body, positions: &[[f32; 3]]) -> Mesh {
    let vertices = positions
        .iter()
        .zip(&body.normals)
        .zip(&body.colours)
        .map(|((p, n), c)| Vertex {
            position: vec3(p[0], p[1], p[2]),
            uv: vec2(0.0, 0.0),
            color: [
                (c[0] * 255.0) as u8,
                (c[1] * 255.0) as u8,
                (c[2] * 255.0) as u8,
                255,
            ],
            normal: vec4(n[0], n[1], n[2], 0.0),
        })
        .collect();
    Mesh {
        vertices,
        indices: body.indices.iter().map(|i| *i as u16).collect(),
        texture: None,
    }
}

#[macroquad::main(window)]
async fn main() {
    let started = std::time::Instant::now();
    let model = gcad_mesh::build(SPEC).expect("the spec builds");
    println!("built in {:?}", started.elapsed());
    let body = &model.bodies[0];
    let clip = body.clip_index("walk");
    let duration = clip.map_or(1.0, |c| body.clips[c].duration as f64).max(0.1);
    loop {
        clear_background(Color::new(0.12, 0.13, 0.16, 1.0));
        let t = get_time() % duration;
        let matrices = body.bone_matrices(clip, t);
        let posed = body.skinned_positions(&matrices);
        let angle = get_time() as f32 * 0.5;
        set_camera(&Camera3D {
            position: vec3(angle.sin() * 70.0, 25.0, angle.cos() * 70.0),
            target: vec3(0.0, 8.0, 0.0),
            up: vec3(0.0, 1.0, 0.0),
            ..Default::default()
        });
        draw_mesh(&mesh_of(body, &posed));
        set_default_camera();
        draw_text(
            "walk clip, CPU skinned from gcad_mesh bone matrices",
            20.0,
            30.0,
            24.0,
            WHITE,
        );
        next_frame().await;
    }
}
