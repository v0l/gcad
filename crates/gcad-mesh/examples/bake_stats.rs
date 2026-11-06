use gcad_mesh::mesh::bake::bake_normals;
use gcad_mesh::mesh::uv::unwrap;
use gcad_mesh::mesh::{Mesh, cross, length, sub};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let source = std::fs::read_to_string(&args[1]).expect("source");
    let size: usize = args[2].parse().expect("size");
    let model = gcad_mesh::build(&source).expect("builds");
    let body = &model.bodies[0];
    let mesh = Mesh {
        positions: body
            .positions
            .iter()
            .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
            .collect(),
        triangles: body.indices.chunks(3).map(|t| [t[0], t[1], t[2]]).collect(),
        labels: vec![0; body.positions.len()],
        parts: vec!["all".into()],
        ..Default::default()
    };
    let layout = unwrap(&mesh, size);
    let mut uv_area = 0.0;
    let mut ratios = Vec::new();
    for (f, t) in mesh.triangles.iter().enumerate() {
        let c = layout.corners[f];
        let ua = ((c[1][0] - c[0][0]) * (c[2][1] - c[0][1])
            - (c[2][0] - c[0][0]) * (c[1][1] - c[0][1]))
            .abs()
            / 2.0;
        let [a, b, d] = mesh.corners(*t);
        let area = length(cross(sub(b, a), sub(d, a))) / 2.0;
        uv_area += ua;
        if area > 1.0e-12 {
            ratios.push(ua / area);
        }
    }
    ratios.sort_by(|a, b| a.total_cmp(b));
    let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
    let median = ratios[ratios.len() / 2];
    let stretch = |r: f64| (median / r).max(r / median);
    let worst = ratios.iter().map(|r| stretch(*r)).fold(0.0, f64::max);
    let p99 = stretch(ratios[(ratios.len() as f64 * 0.01) as usize])
        .max(stretch(ratios[(ratios.len() as f64 * 0.99) as usize]));
    println!("triangles {}", mesh.triangles.len());
    println!("charts {}", layout.chart_count);
    println!("uv area used {:.1}% of the square", uv_area * 100.0);
    println!("texel area per surface area: mean {mean:.3e} median {median:.3e}");
    println!("area stretch vs median: p99 {p99:.2}x worst {worst:.2}x");
    let high = mesh.clone();
    let baked = bake_normals(&mesh, &high, size);
    println!(
        "pixels covered by triangles {:.1}% ({} of {})",
        baked.covered as f64 * 100.0 / (size * size) as f64,
        baked.covered,
        size * size
    );
    if let Some(out) = args.get(3) {
        std::fs::write(out, baked.png()).expect("png");
    }
}
