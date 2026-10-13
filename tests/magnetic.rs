use gcad::sim::magnetic::field::{Field, linkage, solve, torque};
use gcad::sim::magnetic::plane::{Gap, Layout, Plane, Shape, Stuff, Toward, mesh};
use gcad::sim::magnetic::steel::{MU0, named};
use std::f64::consts::PI;

fn disc(centre: [f64; 2], radius: f64, sides: usize) -> Vec<[f64; 2]> {
    (0..sides)
        .map(|k| {
            let a = 2.0 * PI * k as f64 / sides as f64;
            [centre[0] + radius * a.cos(), centre[1] + radius * a.sin()]
        })
        .collect()
}

fn ring(inner: f64, outer: f64, sides: usize) -> Vec<Vec<[f64; 2]>> {
    vec![disc([0.0; 2], outer, sides), disc([0.0; 2], inner, sides)]
}

fn shape(name: &str, loops: Vec<Vec<[f64; 2]>>, stuff: Stuff, turning: bool) -> Shape {
    Shape {
        name: name.into(),
        loops,
        stuff,
        turning,
    }
}

fn triangle_area(field: &Field, plane: &Plane, e: usize) -> f64 {
    let [a, b, c] = plane.triangles[e].map(|v| field.points[v]);
    0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]))
}

fn centroid(field: &Field, plane: &Plane, e: usize) -> [f64; 2] {
    plane.triangles[e].iter().fold([0.0; 2], |m, &v| {
        [
            m[0] + field.points[v][0] / 3.0,
            m[1] + field.points[v][1] / 3.0,
        ]
    })
}

#[test]
fn field_of_a_round_wire() {
    let (radius, amps) = (5.0e-3, 100.0);
    let shapes = vec![shape(
        "wire",
        vec![disc([0.0; 2], radius, 180)],
        Stuff::Coil { amps },
        false,
    )];
    let layout = Layout {
        shapes: shapes.clone(),
        size: 0.5e-3,
        outer: 60.0e-3,
        gap: None,
    };
    let plane = mesh(&layout).expect("meshes");
    let field = solve(&plane, &shapes, 0).expect("solves");
    let (mut squares, mut area) = (0.0, 0.0);
    for (e, b) in field.flux.iter().enumerate() {
        let m = centroid(&field, &plane, e);
        let r = m[0].hypot(m[1]);
        if !(1.0e-3..=30.0e-3).contains(&r) {
            continue;
        }
        let exact = if r < radius {
            MU0 * amps * r / (2.0 * PI * radius * radius)
        } else {
            MU0 * amps / (2.0 * PI * r)
        };
        let a = triangle_area(&field, &plane, e);
        squares += a * (b[0].hypot(b[1]) - exact).powi(2);
        area += a;
    }
    let rms = (squares / area).sqrt() / (MU0 * amps / (2.0 * PI * radius));
    assert!(rms < 0.01, "rms error {rms} of the surface field");
}

#[test]
fn round_magnet_holds_half_its_remanence() {
    let (radius, br) = (10.0e-3, 1.2);
    let shapes = vec![shape(
        "magnet",
        vec![disc([0.0; 2], radius, 240)],
        Stuff::Magnet {
            br,
            mu: 1.0,
            toward: Toward::Along([1.0, 0.0]),
        },
        false,
    )];
    let layout = Layout {
        shapes: shapes.clone(),
        size: 0.3e-3,
        outer: 150.0e-3,
        gap: None,
    };
    let plane = mesh(&layout).expect("meshes");
    let field = solve(&plane, &shapes, 0).expect("solves");
    let (mut sum, mut area) = ([0.0; 2], 0.0);
    let (mut squares, mut around) = (0.0, 0.0);
    for (e, b) in field.flux.iter().enumerate() {
        let m = centroid(&field, &plane, e);
        let r = m[0].hypot(m[1]);
        if plane.region[e] == Some(0) && r < 0.8 * radius {
            sum = [sum[0] + b[0], sum[1] + b[1]];
            area += 1.0;
        }
        if (1.5 * radius..3.0 * radius).contains(&r) {
            let (c, s) = (m[0] / r, m[1] / r);
            let scale = 0.5 * br * radius * radius / (r * r);
            let exact = [scale * (2.0 * c * c - 1.0), scale * 2.0 * c * s];
            let a = triangle_area(&field, &plane, e);
            squares += a * ((b[0] - exact[0]).powi(2) + (b[1] - exact[1]).powi(2));
            around += a;
        }
    }
    let mean = [sum[0] / area, sum[1] / area];
    assert!(
        (mean[0] - 0.5 * br).abs() < 0.005 * br && mean[1].abs() < 0.005 * br,
        "inside {mean:?}"
    );
    let outside = (squares / around).sqrt() / (0.5 * br);
    assert!(outside < 0.01, "dipole field off by {outside} of Br/2");
}

fn turning_magnet_and_wire(nodes: usize) -> (Plane, Vec<Shape>, f64, f64, [f64; 2]) {
    let (radius, br, amps, wire) = (10.0e-3, 1.2, 200.0, [30.0e-3, 0.0]);
    let shapes = vec![
        shape(
            "magnet",
            vec![disc([0.0; 2], radius, 360)],
            Stuff::Magnet {
                br,
                mu: 1.0,
                toward: Toward::Along([0.0, 1.0]),
            },
            true,
        ),
        shape(
            "wire",
            vec![disc(wire, 2.0e-3, 90)],
            Stuff::Coil { amps },
            false,
        ),
    ];
    let layout = Layout {
        shapes: shapes.clone(),
        size: 0.4e-3,
        outer: 150.0e-3,
        gap: Some(Gap {
            inner: radius,
            outer: 14.0e-3,
            nodes,
        }),
    };
    (mesh(&layout).expect("meshes"), shapes, br, amps, wire)
}

fn magnet_torque_from_the_wire(br: f64, radius: f64, amps: f64, wire: [f64; 2], angle: f64) -> f64 {
    let moment = br / MU0 * PI * radius * radius;
    let m = [moment * angle.cos(), moment * angle.sin()];
    let r = wire[0].hypot(wire[1]);
    let u = [wire[0] / r, wire[1] / r];
    let along = (m[0] * u[0] + m[1] * u[1]) / moment;
    let scale = MU0 * moment / (2.0 * PI * r * r);
    let b = [
        scale * (2.0 * along * u[0] - m[0] / moment),
        scale * (2.0 * along * u[1] - m[1] / moment),
    ];
    let force = [-amps * b[1], amps * b[0]];
    -(wire[0] * force[1] - wire[1] * force[0])
}

#[test]
fn a_wire_turns_a_magnet() {
    let nodes = 360;
    let (plane, shapes, br, amps, wire) = turning_magnet_and_wire(nodes);
    for step in [0i64, 30, 75, 120] {
        let field = solve(&plane, &shapes, step).expect("solves");
        let angle = PI / 2.0 + 2.0 * PI * step as f64 / nodes as f64;
        let exact = magnet_torque_from_the_wire(br, 10.0e-3, amps, wire, angle);
        let got = torque(&plane, &field);
        let peak = magnet_torque_from_the_wire(br, 10.0e-3, amps, wire, 0.0).abs();
        assert!(
            (got - exact).abs() < 0.01 * peak,
            "step {step}: {got} against {exact} N·m/m"
        );
        let ahead = solve(&plane, &shapes, step + 1).expect("solves");
        let behind = solve(&plane, &shapes, step - 1).expect("solves");
        let work = (ahead.coenergy - behind.coenergy) / (2.0 * 2.0 * PI / nodes as f64);
        assert!(
            (work - exact).abs() < 0.03 * peak,
            "step {step}: coenergy gives {work} against {exact}"
        );
    }
}

fn integral(a: f64, b: f64, f: impl Fn(f64) -> f64) -> f64 {
    let steps = 4000;
    let dr = (b - a) / steps as f64;
    (0..steps).map(|k| f(a + (k as f64 + 0.5) * dr) * dr).sum()
}

#[test]
fn saturating_toroid_carries_its_flux() {
    let curve = named("m19").expect("m19");
    let (r1, r2) = (20.0e-3, 30.0e-3);
    let (c1, c2, c3, c4) = (15.0e-3, 18.0e-3, 32.0e-3, 35.0e-3);
    for amps in [100.0, 600.0, 3000.0] {
        let shapes = vec![
            shape(
                "core",
                ring(r1, r2, 360),
                Stuff::Steel(curve.clone()),
                false,
            ),
            shape("inside", ring(c1, c2, 360), Stuff::Coil { amps }, false),
            shape(
                "outside",
                ring(c3, c4, 360),
                Stuff::Coil { amps: -amps },
                false,
            ),
        ];
        let layout = Layout {
            shapes: shapes.clone(),
            size: 0.4e-3,
            outer: 60.0e-3,
            gap: None,
        };
        let plane = mesh(&layout).expect("meshes");
        let field = solve(&plane, &shapes, 0).expect("solves");
        let areas = gcad::sim::magnetic::field::areas(&plane, shapes.len());
        let got = linkage(&plane, &field, 1, areas[1]) - linkage(&plane, &field, 2, areas[2]);
        let air = |r: f64| MU0 * amps / (2.0 * PI * r);
        let iron = |r: f64| {
            let h = amps / (2.0 * PI * r);
            let (mut lo, mut hi) = (0.0, 3.0);
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                if curve.h(mid).0 < h {
                    lo = mid
                } else {
                    hi = mid
                }
            }
            0.5 * (lo + hi)
        };
        let inside = |r: f64| (r * r - c1 * c1) / (c2 * c2 - c1 * c1);
        let beyond = |r: f64| (c4 * c4 - r * r) / (c4 * c4 - c3 * c3);
        let exact = integral(c1, c2, |r| air(r) * inside(r) * inside(r))
            + integral(c2, r1, air)
            + integral(r1, r2, iron)
            + integral(r2, c3, air)
            + integral(c3, c4, |r| air(r) * beyond(r) * beyond(r));
        assert!(
            (got - exact).abs() < 0.01 * exact,
            "{amps} A: {got} against {exact} Wb/m, {} Newton steps",
            field.iterations
        );
    }
}
