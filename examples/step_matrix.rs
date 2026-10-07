use monstertruck::modeling::*;
use monstertruck::step::save::{CompleteStepDisplay, StepModel};

fn flatten(curve: &Curve) -> Curve {
    match curve {
        Curve::IntersectionCurve(ic) => flatten(ic.leader()),
        Curve::ParameterCurve(_) => Curve::NurbsCurve(NurbsCurve::new(curve.lift_up())),
        other => other.clone(),
    }
}

fn write(name: &str, solid: &Solid) {
    let compressed = solid.compress();
    let step = CompleteStepDisplay::new(StepModel::from(&compressed), Default::default()).to_string();
    std::fs::write(format!("/tmp/step-{name}.step"), step).unwrap();
    let flat = compressed.map_curves(flatten);
    let step = CompleteStepDisplay::new(StepModel::from_curve3d_only_solid(&flat), Default::default()).to_string();
    std::fs::write(format!("/tmp/step-{name}-3d.step"), step).unwrap();
}

fn edges(shell: &Shell, pick: impl Fn(Point3, Point3) -> bool) -> Vec<Edge> {
    shell.edge_iter().filter(|e| pick(e.front().point(), e.back().point())).fold(Vec::new(), |mut v, e| { if !v.iter().any(|x: &Edge| x.id() == e.id()) { v.push(e) } v })
}

fn blend(solid: &Solid, pick: impl Fn(Point3, Point3) -> bool, opts: FilletOptions) -> Solid {
    let mut shell = solid.boundaries()[0].clone();
    let e = edges(&shell, pick);
    fillet_edges(&mut shell, &e, Some(&opts)).unwrap();
    Solid::new(vec![shell])
}

fn main() {
    let plate: Solid = primitive::cuboid(BoundingBox::from_iter([Point3::new(-20., -15., 0.), Point3::new(20., 15., 3.)]));
    write("box", &plate);
    let seed = builder::vertex(Point3::new(11.6, 5.0, -1.0));
    let rim = builder::revolve(&seed, Point3::new(10.0, 5.0, -1.0), Vector3::unit_z(), builder::SweepAngle::Closed, 4);
    let tool: Solid = builder::extrude(&builder::try_attach_plane(&[rim]).unwrap(), Vector3::unit_z() * 5.0);
    write("cylinder", &tool);
    write("drilled", &monstertruck::solid::difference_normalized(&plate, &tool).unwrap());
    let top = |a: Point3, b: Point3| a.z > 2.999 && b.z > 2.999;
    write("one-edge", &blend(&plate, move |a, b| top(a, b) && a.y > 14.9 && b.y > 14.9, FilletOptions::constant(1.0)));
    write("perimeter", &blend(&plate, top, FilletOptions::constant(1.0)));
    write("chamfer-perimeter", &blend(&plate, top, FilletOptions::constant(1.0).with_profile(FilletProfile::Chamfer)));
}
