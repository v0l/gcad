use monstertruck::meshing::prelude::*;
use monstertruck::modeling::*;

fn uniq(it: impl Iterator<Item = Edge>) -> Vec<Edge> {
    it.fold(Vec::new(), |mut acc, e| { if !acc.iter().any(|x: &Edge| x.id() == e.id()) { acc.push(e) } acc })
}
fn cylinder(cx: f64, cy: f64, r: f64, z0: f64, h: f64) -> Solid {
    let seed = builder::vertex(Point3::new(cx + r, cy, z0));
    let rim = builder::revolve(&seed, Point3::new(cx, cy, z0), Vector3::unit_z(), builder::SweepAngle::Closed, 4);
    let base = builder::try_attach_plane(&[rim]).unwrap();
    builder::extrude(&base, Vector3::unit_z() * h)
}

fn run(name: &str, solid: &Solid, pick: impl Fn(&Edge) -> bool, opts: FilletOptions) {
    let mut shell = solid.boundaries()[0].clone();
    let edges = uniq(shell.edge_iter().filter(|e| pick(e)));
    let t = std::time::Instant::now();
    let r = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fillet_edges(&mut shell, &edges, Some(&opts)))) { Ok(r) => r, Err(_) => { println!("{name}: PANIC"); return; } };
    let n = edges.len();
    match r {
        Err(e) => println!("{name}: n={n} ERR {e:?}"),
        Ok(()) => {
            let cond = shell.shell_condition();
            let s = Solid::try_new(vec![shell]);
            match s {
                Ok(s) => println!("{name}: n={n} ok faces={} cond={cond:?} vol={:.3} {:?}", s.boundaries()[0].len(), s.triangulation(0.01).to_polygon().volume(), t.elapsed()),
                Err(e) => println!("{name}: n={n} solid ERR {e} cond={cond:?}"),
            }
        }
    }
}

fn seq(name: &str, solid: &Solid, pick: impl Fn(&Edge) -> bool, r: f64, profile: FilletProfile) {
    let mut shell = solid.boundaries()[0].clone();
    let n = uniq(shell.edge_iter().filter(|e| pick(e))).len();
    for i in 0..n {
        let edges = uniq(shell.edge_iter().filter(|e| pick(e)));
        let Some(e) = edges.first() else { println!("{name}: ran out at {i}"); return };
        let opts = FilletOptions::constant(r).with_profile(profile.clone());
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fillet_edges(&mut shell, &[e.clone()], Some(&opts)))) {
            Ok(Ok(())) => {}
            Ok(Err(err)) => { println!("{name}: step {i} ERR {err:?}"); return }
            Err(_) => { println!("{name}: step {i} PANIC"); return }
        }
    }
    match Solid::try_new(vec![shell]) {
        Ok(s) => println!("{name}: seq ok faces={} vol={:.3}", s.boundaries()[0].len(), s.triangulation(0.01).to_polygon().volume()),
        Err(e) => println!("{name}: seq solid ERR {e}"),
    }
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let b: Solid = primitive::cuboid(BoundingBox::from_iter([Point3::new(-20., -15., 0.), Point3::new(20., 15., 3.)]));
    let top = |e: &Edge| (e.front().point().z - 3.0).abs() < 1e-9 && (e.back().point().z - 3.0).abs() < 1e-9;
    let one = |e: &Edge| top(e) && (e.front().point().y - e.back().point().y).abs() < 1e-9 && e.front().point().y > 0.0;
    let vert = |e: &Edge| (e.front().point().x - e.back().point().x).abs() < 1e-9 && (e.front().point().y - e.back().point().y).abs() < 1e-9;
    seq("box top4 round seq", &b, top, 1.0, FilletProfile::Round);
    seq("box top4 chamfer seq", &b, top, 1.0, FilletProfile::Chamfer);
    for r in [0.5] {
        run(&format!("box top4 round r={r}"), &b, top, FilletOptions::constant(r));
        run(&format!("box top4 chamfer r={r}"), &b, top, FilletOptions::constant(r).with_profile(FilletProfile::Chamfer));
        run(&format!("box top1 round r={r}"), &b, one, FilletOptions::constant(r));
        run(&format!("box top1 chamfer r={r}"), &b, one, FilletOptions::constant(r).with_profile(FilletProfile::Chamfer));
        run(&format!("box vert round r={r}"), &b, vert, FilletOptions::constant(r));
    }
    let tall: Solid = primitive::cuboid(BoundingBox::from_iter([Point3::new(0., 0., 0.), Point3::new(20., 20., 20.)]));
    run("cube vert4 r=3", &tall, vert, FilletOptions::constant(3.0));
    let all = |_: &Edge| true;
    run("cube all12 r=2", &tall, all, FilletOptions::constant(2.0));
    let cyl = cylinder(0., 0., 5., 0., 10.);
    run("cyl top round r=1", &cyl, |e: &Edge| e.front().point().z > 9.9 && e.back().point().z > 9.9, FilletOptions::constant(1.0));
    run("cyl top chamfer r=1", &cyl, |e: &Edge| e.front().point().z > 9.9 && e.back().point().z > 9.9, FilletOptions::constant(1.0).with_profile(FilletProfile::Chamfer));
    let mut h = cylinder(0., 0., 2., -1., 25.);
    h.not();
    let holed = monstertruck::solid::and(&tall.clone(), &{ let mut h = cylinder(10., 10., 2., -1., 25.); h.not(); h }, 0.001);
    let _ = h;
    match holed {
        Ok(s) => {
            run("holed cube top outer round r=1", &s, |e: &Edge| e.front().point().z > 19.99 && e.back().point().z > 19.99 && (e.front().point().x < 0.01 || e.front().point().x > 19.99 || e.front().point().y < 0.01 || e.front().point().y > 19.99), FilletOptions::constant(1.0));
            run("holed cube hole rim chamfer r=0.5", &s, |e: &Edge| e.front().point().z > 19.99 && e.back().point().z > 19.99 && (e.front().point() - Point3::new(10., 10., 20.)).magnitude() < 2.1, FilletOptions::constant(0.5).with_profile(FilletProfile::Chamfer));
        }
        Err(e) => println!("holed: {e}"),
    }
}
