use monstertruck::meshing::prelude::*;
use monstertruck::modeling::*;
use monstertruck::step::save::{CompleteStepDisplay, StepModel};

fn main() -> anyhow::Result<()> {
    let plate: Solid = primitive::cuboid(BoundingBox::from_iter([Point3::new(-20., -15., 0.), Point3::new(20., 15., 3.)]));
    let seed = builder::vertex(Point3::new(11.6, 5.0, -1.0));
    let rim = builder::revolve(&seed, Point3::new(10.0, 5.0, -1.0), Vector3::unit_z(), builder::SweepAngle::Closed, 4);
    let tool: Solid = builder::extrude(&builder::try_attach_plane(&[rim])?, Vector3::unit_z() * 5.0);
    let drilled = monstertruck::solid::difference_normalized(&plate, &tool)?;
    let mut shell = drilled.boundaries()[0].clone();
    let top: Vec<Edge> = shell.edge_iter().filter(|e| e.front().point().z > 2.999 && e.back().point().z > 2.999).fold(Vec::new(), |mut v, e| { if !v.iter().any(|x: &Edge| x.id() == e.id()) { v.push(e) } v });
    let outer: Vec<Edge> = top.iter().filter(|e| e.front().point().x.abs() > 19.9 || e.front().point().y.abs() > 14.9).cloned().collect();
    let rim: Vec<Edge> = top.iter().filter(|e| (e.front().point() - Point3::new(10.0, 5.0, 3.0)).magnitude() < 1.7).cloned().collect();
    fillet_edges(&mut shell, &outer, Some(&FilletOptions::constant(1.0)))?;
    fillet_edges(&mut shell, &rim, Some(&FilletOptions::constant(0.4).with_profile(FilletProfile::Chamfer)))?;
    let solid = Solid::try_new(vec![shell])?;
    let mesh = solid.triangulation(0.01).to_polygon();
    println!("faces {} volume {:.3}", solid.boundaries()[0].len(), mesh.volume());
    let step = CompleteStepDisplay::new(StepModel::from(&solid.compress()), Default::default()).to_string();
    std::fs::write("/tmp/linecad-fillet.step", &step)?;
    monstertruck::mesh::stl::write(&mesh, &mut std::fs::File::create("/tmp/linecad-fillet.stl")?, monstertruck::mesh::stl::StlType::Binary)?;
    println!("step {} bytes, brep={}", step.len(), step.contains("MANIFOLD_SOLID_BREP"));
    Ok(())
}
