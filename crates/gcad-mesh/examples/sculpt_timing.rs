use gcad_mesh::interp::Scene;
use gcad_mesh::parse::{Scope, parse_program};
use std::time::Instant;

fn main() {
    let levels: usize = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let source = format!(
        "ball: blob 0,0,0 r=10\nremesh 0.1\nsubdivide {levels}\nbrush d type=draw size=1 strength=0.2\nbrush c type=clay size=2 strength=0.1\nbrush w type=dam_standard size=0.3 strength=0.1\nbrush s type=smooth size=1.5 strength=2\nbrush m type=move size=3\ndab d at=0,0,10\nstroke c from=-5,0,8.66 to=5,0,8.66\nstroke w from=-5,-3,8 to=5,-3,8\ndab s at=0,0,10\nstroke m from=0,0,10 to=0,0,11\nmask sphere=0,0,10 r=2\nselect all\ninflate 0.01\nfinish decimate=20000"
    );
    let mut scene = Scene::default();
    let scope = Scope::new();
    for line in parse_program(&source).expect("parses") {
        let start = Instant::now();
        let out = match line.op.as_str() {
            "blob" => scene.op_blob(&scope, &line),
            "remesh" => scene.op_remesh(&scope, &line),
            "subdivide" => scene.op_subdivide(&scope, &line),
            "brush" => scene.op_brush(&scope, &line),
            "dab" => scene.op_dab(&scope, &line),
            "stroke" => scene.op_stroke(&scope, &line),
            "mask" => scene.op_mask(&scope, &line),
            "select" => scene.op_select(&scope, &line),
            "inflate" => scene.op_inflate(&scope, &line),
            "finish" => scene.op_finish(&scope, &line),
            other => panic!("{other}"),
        }
        .expect("runs");
        println!(
            "{:>8.0} ms  {}  {}",
            start.elapsed().as_secs_f64() * 1000.0,
            line.text,
            out.chars().take(110).collect::<String>()
        );
    }
}
