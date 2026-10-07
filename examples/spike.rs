use monstertruck::meshing::prelude::*;
use monstertruck::modeling::*;
use monstertruck::step::save::{CompleteStepDisplay, StepHeaderDescriptor, StepModel};
use std::time::Instant;

fn rect_wire(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Wire {
    let v = builder::vertices([
        Point3::new(x0, y0, z),
        Point3::new(x1, y0, z),
        Point3::new(x1, y1, z),
        Point3::new(x0, y1, z),
    ]);
    vec![
        builder::line(&v[0], &v[1]),
        builder::line(&v[1], &v[2]),
        builder::line(&v[2], &v[3]),
        builder::line(&v[3], &v[0]),
    ]
    .into()
}

fn cylinder(cx: f64, cy: f64, r: f64, z0: f64, h: f64) -> Solid {
    let seed = builder::vertex(Point3::new(cx + r, cy, z0));
    let rim = builder::revolve(
        &seed,
        Point3::new(cx, cy, z0),
        Vector3::unit_z(),
        builder::SweepAngle::Closed,
        4,
    );
    let base = builder::try_attach_plane(&[rim]).unwrap();
    builder::extrude(&base, Vector3::unit_z() * h)
}

fn report(name: &str, s: &Solid, t: Instant) {
    let mesh = s.triangulation(0.05).to_polygon();
    let shell = &s.boundaries()[0];
    println!(
        "{name}: faces={} edges={} cond={:?} vol={:.3} tris={} {:?}",
        shell.len(),
        shell.edge_iter().count() / 2,
        shell.shell_condition(),
        mesh.volume(),
        mesh.faces().len(),
        t.elapsed()
    );
}

fn main() -> anyhow::Result<()> {
    let t = Instant::now();
    let plate: Solid =
        profile::solid_from_planar_profile(vec![rect_wire(-20., -15., 20., 15., 0.)], Vector3::unit_z() * 3.0)?;
    report("plate", &plate, t);

    let t = Instant::now();
    let mut hole = cylinder(10., 5., 1.6, -1., 5.);
    hole.not();
    let cut = monstertruck::solid::and(&plate, &hole, 0.001)?;
    report("plate-hole", &cut, t);

    let t = Instant::now();
    let mut holes = cut.clone();
    for (x, y) in [(-10., 5.), (10., -5.), (-10., -5.)] {
        let mut h = cylinder(x, y, 1.6, -1., 5.);
        h.not();
        holes = monstertruck::solid::and(&holes, &h, 0.001)?;
    }
    report("plate-4holes", &holes, t);

    let t = Instant::now();
    let mut shell = plate.boundaries()[0].clone();
    let vertical: Vec<Edge> = shell
        .edge_iter()
        .filter(|e| {
            let (a, b) = (e.front().point(), e.back().point());
            (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
        })
        .fold(Vec::new(), |mut acc, e| {
            if !acc.iter().any(|x: &Edge| x.id() == e.id()) {
                acc.push(e);
            }
            acc
        });
    println!("vertical edges {}", vertical.len());
    let r = fillet_edges(&mut shell, &vertical, Some(&FilletOptions::constant(4.0)));
    let filleted = Solid::new(vec![shell]);
    println!("fillet result {r:?}");
    report("plate-fillet", &filleted, t);

    let t = Instant::now();
    let mut shell = plate.boundaries()[0].clone();
    let top: Vec<Edge> = shell
        .edge_iter()
        .filter(|e| (e.front().point().z - 3.0).abs() < 1e-9 && (e.back().point().z - 3.0).abs() < 1e-9)
        .fold(Vec::new(), |mut acc, e| {
            if !acc.iter().any(|x: &Edge| x.id() == e.id()) {
                acc.push(e);
            }
            acc
        });
    let r = fillet_edges(
        &mut shell,
        &top,
        Some(&FilletOptions::constant(0.5).with_profile(FilletProfile::Chamfer)),
    );
    println!("chamfer result {r:?} edges {}", top.len());
    let chamfered = Solid::new(vec![shell]);
    report("plate-chamfer", &chamfered, t);

    let t = Instant::now();
    let mut shell = holes.boundaries()[0].clone();
    let top: Vec<Edge> = shell
        .edge_iter()
        .filter(|e| (e.front().point().z - 3.0).abs() < 1e-9 && (e.back().point().z - 3.0).abs() < 1e-9 && (e.front().point().x.abs() > 19.9 || e.front().point().y.abs() > 14.9))
        .fold(Vec::new(), |mut acc, e| {
            if !acc.iter().any(|x: &Edge| x.id() == e.id()) {
                acc.push(e);
            }
            acc
        });
    let r = fillet_edges(&mut shell, &top, Some(&FilletOptions::constant(1.0)));
    println!("fillet after boolean {r:?} edges {}", top.len());
    let fab = Solid::new(vec![shell]);
    report("holes-fillet", &fab, t);

    let t = Instant::now();
    let compressed = holes.compress();
    let step = CompleteStepDisplay::new(StepModel::from(&compressed), StepHeaderDescriptor::default()).to_string();
    std::fs::write("/tmp/spike.step", &step)?;
    let mesh = holes.triangulation(0.02).to_polygon();
    let mut f = std::fs::File::create("/tmp/spike.stl")?;
    monstertruck::mesh::stl::write(&mesh, &mut f, monstertruck::mesh::stl::StlType::Binary)?;
    println!("export {:?} step={}B", t.elapsed(), step.len());
    Ok(())
}
