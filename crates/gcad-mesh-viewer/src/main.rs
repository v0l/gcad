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

fn meshes_of(body: &gcad_mesh::Body, positions: &[[f32; 3]]) -> Vec<Mesh> {
    let light = vec3(0.4, 0.8, 0.5).normalize();
    body.indices
        .chunks(3 * 1000)
        .map(|chunk| {
            let mut remap = std::collections::HashMap::new();
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            for &i in chunk {
                let slot = *remap.entry(i).or_insert_with(|| {
                    let i = i as usize;
                    let (p, n, c) = (positions[i], body.normals[i], body.colours[i]);
                    let shade = 0.35 + 0.65 * vec3(n[0], n[1], n[2]).dot(light).max(0.0);
                    vertices.push(Vertex {
                        position: vec3(p[0], p[1], p[2]),
                        uv: vec2(0.0, 0.0),
                        color: [
                            (c[0] * shade * 255.0) as u8,
                            (c[1] * shade * 255.0) as u8,
                            (c[2] * shade * 255.0) as u8,
                            255,
                        ],
                        normal: vec4(n[0], n[1], n[2], 0.0),
                    });
                    (vertices.len() - 1) as u16
                });
                indices.push(slot);
            }
            Mesh {
                vertices,
                indices,
                texture: None,
            }
        })
        .collect()
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
            position: vec3(angle.sin() * 42.0, 14.0, angle.cos() * 42.0),
            target: vec3(0.0, 5.0, 0.0),
            up: vec3(0.0, 1.0, 0.0),
            ..Default::default()
        });
        for mesh in meshes_of(body, &posed) {
            draw_mesh(&mesh);
        }
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
