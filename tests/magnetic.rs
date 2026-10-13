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

fn arc(radius: f64, from: f64, to: f64) -> Vec<[f64; 2]> {
    let grid = 0.5f64.to_radians();
    let opening = 1.0e-3 / 24.0e-3;
    let mut angles = vec![from, to];
    let (low, high) = (from.min(to), from.max(to));
    let specials: Vec<f64> = (-1..13)
        .flat_map(|slot| {
            let s = (30.0 * slot as f64 + 15.0).to_radians();
            [s - opening, s + opening]
        })
        .collect();
    let mut k = (low / grid).floor() as i64;
    while (k as f64) * grid < high {
        let a = k as f64 * grid;
        let crowded = specials
            .iter()
            .chain([&low, &high])
            .any(|b| (a - b).abs() < 0.4 * grid);
        if a > low + 1.0e-9 && a < high - 1.0e-9 && !crowded {
            angles.push(a);
        }
        k += 1;
    }
    for &a in &specials {
        if a > low + 1.0e-9 && a < high - 1.0e-9 {
            angles.push(a);
        }
    }
    angles.sort_by(|a, b| a.total_cmp(b));
    if to < from {
        angles.reverse();
    }
    angles
        .into_iter()
        .map(|a| [radius * a.cos(), radius * a.sin()])
        .collect()
}

fn joined(pieces: &[Vec<[f64; 2]>]) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::new();
    for piece in pieces {
        for &p in piece {
            if out
                .last()
                .is_none_or(|q: &[f64; 2]| (q[0] - p[0]).hypot(q[1] - p[1]) > 1.0e-12)
            {
                out.push(p);
            }
        }
    }
    if out.len() > 1 {
        let (a, b) = (out[0], out[out.len() - 1]);
        if (a[0] - b[0]).hypot(a[1] - b[1]) < 1.0e-12 {
            out.pop();
        }
    }
    out
}

const PHASES: [(usize, f64); 12] = [
    (0, 1.0),
    (0, -1.0),
    (1, -1.0),
    (1, 1.0),
    (2, 1.0),
    (2, -1.0),
    (0, -1.0),
    (0, 1.0),
    (1, 1.0),
    (1, -1.0),
    (2, -1.0),
    (2, 1.0),
];

fn motor(amps: [f64; 3], turns: f64) -> Vec<Shape> {
    let mm = 1.0e-3;
    let deg = |d: f64| d.to_radians();
    let (shaft, rotor, magnet, bore, opening_top, slot_top, outer) = (
        6.0 * mm,
        20.0 * mm,
        23.0 * mm,
        24.0 * mm,
        26.0 * mm,
        34.0 * mm,
        40.0 * mm,
    );
    let half_magnet = deg(15.3);
    let mut rim = Vec::new();
    let mut shapes = Vec::new();
    for k in 0..10 {
        let c = deg(36.0 * k as f64);
        rim.push(arc(rotor, c - deg(18.0), c - half_magnet));
        rim.push(arc(rotor, c - half_magnet, c + half_magnet));
        rim.push(arc(rotor, c + half_magnet, c + deg(18.0)));
        shapes.push(shape(
            &format!("magnet{k}"),
            vec![joined(&[
                arc(rotor, c - half_magnet, c + half_magnet),
                arc(magnet, c + half_magnet, c - half_magnet),
            ])],
            Stuff::Magnet {
                br: 1.2,
                mu: 1.05,
                toward: Toward::Radial(if k % 2 == 0 { 1.0 } else { -1.0 }),
            },
            true,
        ));
    }
    shapes.insert(
        0,
        shape(
            "rotor",
            vec![joined(&rim), disc([0.0; 2], shaft, 72)],
            Stuff::Steel(named("1010").unwrap()),
            true,
        ),
    );
    let opening = (2.0 * mm / 2.0) / bore;
    let half_slot = deg(7.5);
    let mut contour = Vec::new();
    for k in 0..12 {
        let s = deg(30.0 * k as f64 + 15.0);
        let previous = deg(30.0 * k as f64 - 15.0);
        contour.push(arc(bore, previous + opening, s - opening));
        contour.push(arc(opening_top, s - opening, s - half_slot));
        contour.push(arc(slot_top, s - half_slot, s + half_slot));
        contour.push(arc(opening_top, s + half_slot, s + opening));
    }
    shapes.push(shape(
        "stator",
        vec![disc([0.0; 2], outer, 720), joined(&contour)],
        Stuff::Steel(named("m19").unwrap()),
        false,
    ));
    for k in 0..12 {
        let s = deg(30.0 * k as f64 + 15.0);
        let (lower_phase, lower_sign) = PHASES[(k + 1) % 12];
        let (upper_phase, upper_sign) = PHASES[k];
        shapes.push(shape(
            &format!("slot{k}.lower"),
            vec![joined(&[
                arc(opening_top, s - half_slot, s),
                arc(slot_top, s, s - half_slot),
            ])],
            Stuff::Coil {
                amps: -lower_sign * turns * amps[lower_phase],
            },
            false,
        ));
        shapes.push(shape(
            &format!("slot{k}.upper"),
            vec![joined(&[
                arc(opening_top, s, s + half_slot),
                arc(slot_top, s + half_slot, s),
            ])],
            Stuff::Coil {
                amps: upper_sign * turns * amps[upper_phase],
            },
            false,
        ));
    }
    shapes
}

fn motor_plane(shapes: &[Shape], nodes: usize, size: f64) -> Plane {
    mesh(&Layout {
        shapes: shapes.to_vec(),
        size,
        outer: 60.0e-3,
        gap: Some(Gap {
            inner: 23.0e-3,
            outer: 24.0e-3,
            nodes,
        }),
    })
    .expect("meshes")
}

fn shoelace(points: &[[f64; 2]]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|k| {
            let (a, b) = (points[k], points[(k + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

fn femm_lua(shapes: &[Shape], turn: f64, fine: f64) -> String {
    let (sin, cos) = turn.sin_cos();
    let place = |p: [f64; 2], turning: bool| {
        let p = if turning {
            [p[0] * cos - p[1] * sin, p[0] * sin + p[1] * cos]
        } else {
            p
        };
        [p[0] * 1.0e3, p[1] * 1.0e3]
    };
    let key = |p: [f64; 2]| ((p[0] * 1.0e6).round() as i64, (p[1] * 1.0e6).round() as i64);
    let mut nodes = std::collections::BTreeMap::new();
    let mut edges = std::collections::BTreeSet::new();
    for shape in shapes {
        for ring in &shape.loops {
            let pts: Vec<[f64; 2]> = ring.iter().map(|&p| place(p, shape.turning)).collect();
            for k in 0..pts.len() {
                let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
                let (ka, kb) = (key(a), key(b));
                nodes.insert(ka, a);
                nodes.insert(kb, b);
                if ka != kb {
                    edges.insert(if ka < kb { (ka, kb) } else { (kb, ka) });
                }
            }
        }
    }
    let mut s = String::new();
    s += "newdocument(0)\nmi_probdef(0, 'millimeters', 'planar', 1e-8, 1000, 30)\n";
    s += "mi_addmaterial('air', 1, 1, 0, 0)\n";
    s += "mi_getmaterial('M-19 Steel')\nmi_getmaterial('1010 Steel')\n";
    for (ka, a) in &nodes {
        let _ = ka;
        s += &format!("mi_addnode({:.9}, {:.9})\n", a[0], a[1]);
    }
    for (ka, kb) in &edges {
        let (a, b) = (nodes[ka], nodes[kb]);
        s += &format!(
            "mi_addsegment({:.9}, {:.9}, {:.9}, {:.9})\n",
            a[0], a[1], b[0], b[1]
        );
    }
    let label = |s: &mut String,
                 at: [f64; 2],
                 material: &str,
                 size: f64,
                 magdir: &str,
                 group: i32| {
        let size = size * fine;
        *s += &format!(
            "mi_addblocklabel({:.9}, {:.9})\nmi_selectlabel({:.9}, {:.9})\nmi_setblockprop('{material}', 0, {size}, '<None>', {magdir}, {group}, 0)\nmi_clearselected()\n",
            at[0], at[1], at[0], at[1]
        );
    };
    for (k, shape) in shapes.iter().enumerate() {
        let plane = mesh(&Layout {
            shapes: vec![shape.clone()],
            size: 1.0e-3,
            outer: 0.1,
            gap: None,
        })
        .expect("label mesh");
        let e = plane.region.iter().position(|r| *r == Some(0)).unwrap();
        let m = plane.triangles[e].iter().fold([0.0; 2], |m, &v| {
            [
                m[0] + plane.points[v][0] / 3.0,
                m[1] + plane.points[v][1] / 3.0,
            ]
        });
        let at = place(m, shape.turning);
        let group = if shape.turning { 1 } else { 0 };
        match &shape.stuff {
            Stuff::Steel(_) => {
                let name = if shape.name == "rotor" {
                    "1010 Steel"
                } else {
                    "M-19 Steel"
                };
                label(&mut s, at, name, 0.5, "0", group);
            }
            Stuff::Magnet { br, mu, toward } => {
                let hc = br / (MU0 * mu);
                s += &format!("mi_addmaterial('magnet{k}', {mu}, {mu}, {hc}, 0)\n");
                let dir = match toward {
                    Toward::Radial(sign) if *sign > 0.0 => "'theta'".to_string(),
                    Toward::Radial(_) => "'theta+180'".to_string(),
                    Toward::Along(d) => format!("{}", (d[1].atan2(d[0]) + turn).to_degrees()),
                };
                label(&mut s, at, &format!("magnet{k}"), 0.4, &dir, group);
            }
            Stuff::Coil { amps } => {
                let area: f64 = shape.loops.iter().map(|l| shoelace(l)).sum::<f64>().abs();
                let j = amps / area / 1.0e6;
                s += &format!("mi_addmaterial('coil{k}', 1, 1, 0, {j})\n");
                label(&mut s, at, &format!("coil{k}"), 0.5, "0", group);
            }
            _ => {}
        }
    }
    label(&mut s, [0.0, 0.0], "air", 1.0, "0", 1);
    label(&mut s, [23.5 * cos, 23.5 * sin], "air", 0.2, "0", 0);
    label(&mut s, [50.0, 0.0], "air", 2.0, "0", 0);
    s += "mi_addboundprop('zero', 0, 0, 0, 0, 0, 0, 0, 0, 0)\n";
    s += "for k = 0, 359 do\n  local a, b = 2 * PI * k / 360, 2 * PI * (k + 1) / 360\n  mi_addnode(60 * cos(a), 60 * sin(a))\nend\n";
    s += "for k = 0, 359 do\n  local a, b = 2 * PI * k / 360, 2 * PI * (k + 1) / 360\n  mi_addsegment(60 * cos(a), 60 * sin(a), 60 * cos(b), 60 * sin(b))\n  local m = (a + b) / 2\n  mi_selectsegment(59.99 * cos(m), 59.99 * sin(m))\nend\n";
    s += "mi_setsegmentprop('zero', 0, 1, 0, 0)\nmi_clearselected()\n";
    s += "mi_saveas('/tmp/femm_motor.fem')\n";
    s
}

fn femm(script: &str) -> String {
    let femmcli = std::env::var("FEMMCLI").expect("FEMMCLI, xfemm's femmcli");
    let base = std::env::var("FEMMBASE").expect("FEMMBASE, xfemm's femmcli/release");
    std::fs::write("/tmp/femm_script.lua", script).unwrap();
    let out = std::process::Command::new(&femmcli)
        .arg(format!("--lua-base-dir={base}"))
        .arg(format!("--lua-init={base}/init.lua"))
        .arg("--lua-script=/tmp/femm_script.lua")
        .output()
        .expect("femmcli runs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
#[ignore = "needs xfemm's femmcli in FEMMCLI and its femmcli/release in FEMMBASE"]
fn femm_motor_reference() {
    for (label, amps) in [("cogging", [0.0; 3]), ("loaded", [10.0, -5.0, -5.0])] {
        let shapes = motor(amps, 40.0);
        let plane = motor_plane(&shapes, 720, 0.5e-3);
        for step in [0, 1, 2, 3, 4, 5, 6] {
            let turn = (0.5 * step as f64).to_radians();
            femm(&femm_lua(
                &shapes,
                turn,
                std::env::var("FEMMFINE").map_or(1.0, |v| v.parse().unwrap()),
            ));
            let text = std::fs::read_to_string("/tmp/femm_motor.fem").unwrap();
            std::fs::write(
                "/tmp/femm_motor.fem",
                text.replacen("[MinAngle]", "[DoSmartMesh] = 0\n[MinAngle]", 1),
            )
            .unwrap();
            let out = femm(
                "open('/tmp/femm_motor.fem')\nmi_analyze(1)\nmi_loadsolution()\nmo_groupselectblock(1)\nprint('torque', mo_blockintegral(22))\n",
            );
            let reference = out
                .lines()
                .find_map(|l| l.strip_prefix("torque")?.trim().parse::<f64>().ok())
                .unwrap_or(f64::NAN);
            let ours = torque(&plane, &solve(&plane, &shapes, step).expect("solves"));
            println!(
                "{label} {:.1} deg: femm {reference:.5}, ours {ours:.5}",
                0.5 * step as f64
            );
        }
    }
}

const FEMM_LOADED: [f64; 7] = [
    -25.78477, -30.34298, -34.68755, -38.87110, -42.89371, -46.47095, -49.96885,
];

#[test]
fn surface_magnet_motor_matches_femm() {
    let loaded = motor([10.0, -5.0, -5.0], 40.0);
    let plane = motor_plane(&loaded, 720, 0.5e-3);
    for (step, reference) in FEMM_LOADED.iter().enumerate() {
        let got = torque(
            &plane,
            &solve(&plane, &loaded, step as i64).expect("solves"),
        );
        assert!(
            (got - reference).abs() < 0.01 * reference.abs(),
            "{:.1} deg: {got} against FEMM's {reference} N·m/m",
            0.5 * step as f64
        );
    }
    let still = motor([0.0; 3], 0.0);
    let cogging: Vec<f64> = (0..=12)
        .map(|step| torque(&plane, &solve(&plane, &still, step).expect("solves")))
        .collect();
    let peak = cogging.iter().fold(0.0f64, |m, t| m.max(t.abs()));
    assert!((peak - 0.53).abs() < 0.06, "cogging peak {peak} N·m/m");
    for k in 0..=6 {
        assert!(
            (cogging[k] + cogging[12 - k]).abs() < 0.12 * peak,
            "cogging at {} and {} deg: {} and {}",
            0.5 * k as f64,
            6.0 - 0.5 * k as f64,
            cogging[k],
            cogging[12 - k]
        );
    }
}
