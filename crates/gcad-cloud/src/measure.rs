use crate::cloud::P3;
use crate::fit::Primitive;
use anyhow::{Result, bail};

const PARALLEL_DEGREES: f64 = 1.0;

#[derive(Clone, Copy, Debug)]
pub enum Feature {
    Point(P3),
    Shape(Primitive),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Distance {
    pub distance: f64,
    pub signed: Option<f64>,
    pub what: &'static str,
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: P3) -> P3 {
    let n = norm(a).max(1.0e-300);
    a.map(|v| v / n)
}

fn to_axis(p: P3, origin: P3, axis: P3) -> f64 {
    let a = unit(axis);
    let d = sub(p, origin);
    let along = dot(d, a);
    norm(std::array::from_fn(|k| d[k] - along * a[k]))
}

fn plain(distance: f64, what: &'static str) -> Distance {
    Distance {
        distance,
        signed: None,
        what,
    }
}

fn signed(value: f64, what: &'static str) -> Distance {
    Distance {
        distance: value.abs(),
        signed: Some(value),
        what,
    }
}

fn plane_offset(point: P3, origin: P3, normal: P3) -> f64 {
    dot(unit(normal), sub(point, origin))
}

fn angle_between_lines(a: P3, b: P3) -> f64 {
    dot(unit(a), unit(b)).abs().clamp(0.0, 1.0).acos().to_degrees()
}

fn centre(radius_shape: Primitive) -> P3 {
    match radius_shape {
        Primitive::Sphere { centre, .. } => centre,
        _ => [0.0; 3],
    }
}

pub fn distance(a: &Feature, b: &Feature) -> Result<Distance> {
    use Feature::{Point, Shape};
    use Primitive::{Cone, Cylinder, Plane, Sphere};
    Ok(match (*a, *b) {
        (Point(p), Point(q)) => plain(norm(sub(p, q)), "point to point"),
        (Point(p), Shape(Plane { origin, normal })) | (Shape(Plane { origin, normal }), Point(p)) => {
            signed(plane_offset(p, origin, normal), "point to plane")
        }
        (Point(p), Shape(Cylinder { origin, axis, .. }))
        | (Shape(Cylinder { origin, axis, .. }), Point(p)) => {
            plain(to_axis(p, origin, axis), "point to cylinder axis")
        }
        (Point(p), Shape(s @ Sphere { .. })) | (Shape(s @ Sphere { .. }), Point(p)) => {
            plain(norm(sub(p, centre(s))), "point to sphere centre")
        }
        (Shape(Plane { origin: o1, normal: n1 }), Shape(Plane { origin: o2, normal: n2 })) => {
            let angle = angle_between_lines(n1, n2);
            if angle > PARALLEL_DEGREES {
                bail!("the planes are not parallel, they differ by {angle:.2} degrees");
            }
            signed(plane_offset(o2, o1, n1), "plane to plane")
        }
        (
            Shape(Cylinder { origin: o1, axis: a1, .. }),
            Shape(Cylinder { origin: o2, axis: a2, .. }),
        ) => {
            let n = cross(a1, a2);
            let value = if norm(n) / (norm(a1) * norm(a2)) < PARALLEL_DEGREES.to_radians().sin() {
                to_axis(o2, o1, a1)
            } else {
                dot(sub(o2, o1), unit(n)).abs()
            };
            plain(value, "cylinder axis to cylinder axis")
        }
        (Shape(Cylinder { origin, axis, .. }), Shape(Plane { origin: po, normal }))
        | (Shape(Plane { origin: po, normal }), Shape(Cylinder { origin, axis, .. })) => {
            let angle = 90.0 - angle_between_lines(axis, normal);
            if angle > PARALLEL_DEGREES {
                bail!("the cylinder axis is not parallel to the plane, it is {angle:.2} degrees off");
            }
            plain(plane_offset(origin, po, normal).abs(), "cylinder axis to plane")
        }
        (Shape(Cylinder { origin, axis, .. }), Shape(s @ Sphere { .. }))
        | (Shape(s @ Sphere { .. }), Shape(Cylinder { origin, axis, .. })) => {
            plain(to_axis(centre(s), origin, axis), "sphere centre to cylinder axis")
        }
        (Shape(s @ Sphere { .. }), Shape(Plane { origin, normal }))
        | (Shape(Plane { origin, normal }), Shape(s @ Sphere { .. })) => {
            signed(plane_offset(centre(s), origin, normal), "sphere centre to plane")
        }
        (Shape(s @ Sphere { .. }), Shape(t @ Sphere { .. })) => {
            plain(norm(sub(centre(s), centre(t))), "sphere centre to sphere centre")
        }
        (Shape(Cone { .. }), _) | (_, Shape(Cone { .. })) => {
            bail!("`measure` does not take a fitted cone; use a plane, cylinder or sphere")
        }
    })
}

pub fn diameter(feature: &Feature) -> Result<f64> {
    match feature {
        Feature::Shape(Primitive::Cylinder { radius, .. })
        | Feature::Shape(Primitive::Sphere { radius, .. }) => Ok(2.0 * radius),
        _ => bail!("only a fitted cylinder or sphere has a diameter"),
    }
}
