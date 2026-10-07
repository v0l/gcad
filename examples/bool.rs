use monstertruck::meshing::prelude::*;
use monstertruck::modeling::*;

fn cylinder(cx: f64, cy: f64, r: f64, z0: f64, h: f64) -> Solid {
    let seed = builder::vertex(Point3::new(cx + r, cy, z0));
    let rim = builder::revolve(&seed, Point3::new(cx, cy, z0), Vector3::unit_z(), builder::SweepAngle::Closed, 4);
    let base = builder::try_attach_plane(&[rim]).unwrap();
    builder::extrude(&base, Vector3::unit_z() * h)
}

fn main() {
    for scale in [1.0, 10.0, 0.1] {
        for tol in [0.001, 0.01, 0.05, 0.2] {
            let plate: Solid = primitive::cuboid(BoundingBox::from_iter([Point3::new(-20., -15., 0.) * scale, Point3::new(20., 15., 3.) * scale]));
            let mut h = cylinder(10. * scale, 5. * scale, 1.6 * scale, -1. * scale, 5. * scale);
            h.not();
            let t = std::time::Instant::now();
            let r = monstertruck::solid::and(&plate, &h, tol * scale);
            match r {
                Ok(s) => println!("scale {scale} tol {tol}: ok faces={} vol={:.4} {:?}", s.boundaries()[0].len(), s.triangulation(0.01*scale).to_polygon().volume() / scale.powi(3), t.elapsed()),
                Err(e) => println!("scale {scale} tol {tol}: ERR {e} {:?}", t.elapsed()),
            }
        }
    }
}
