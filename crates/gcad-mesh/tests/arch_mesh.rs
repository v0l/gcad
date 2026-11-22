const HEAD: &str = "units mm\nlevel L0 0 \"Ground\"\nlevel L1 2700 \"First\"\ngrid A x=0\ngrid B x=8000\ngrid 1 y=0\ngrid 2 y=6000\nwall_type ext brick:100 pir:100 block:100\nwall_type int block:100\n";

fn volume(source: &str) -> (f64, usize) {
    let model = gcad_mesh::build_with(
        source,
        &gcad_mesh::Options {
            y_up: false,
            ..Default::default()
        },
    )
    .expect("builds");
    let mut total = 0.0;
    let mut triangles = 0;
    for body in &model.bodies {
        triangles += body.indices.len() / 3;
        total += body
            .indices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| body.positions[i as usize].map(f64::from));
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum::<f64>();
    }
    (total, triangles)
}

#[test]
fn the_house_example_builds_under_the_budget() {
    let source = include_str!("../../../examples/mesh/house.gcad");
    let (_, triangles) = volume(source);
    assert!(triangles < 4000, "{triangles}");
}

#[test]
fn wall_layers_are_named_and_coloured_parts() {
    let model = gcad_mesh::build_with(
        &format!("{HEAD}w1: wall A1 B1 type=ext base=L0 top=L1"),
        &gcad_mesh::Options {
            y_up: false,
            ..Default::default()
        },
    )
    .expect("builds");
    let wall = model.body("w1").expect("the wall");
    let names: Vec<&str> = wall.parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["brick", "pir", "block"]);
    assert_ne!(wall.parts[0].colour, wall.parts[1].colour);
    assert_ne!(wall.parts[1].colour, wall.parts[2].colour);
    assert_eq!(wall.parts[0].colour, [0.62, 0.3, 0.22]);
}

#[test]
fn a_street_of_twenty_houses_shares_one_mesh() {
    let source = format!(
        "{HEAD}building \"house\"\nw1: wall A1 B1 B2 A2 closed type=ext base=L0 top=L1 justify=outside\nd1: door on=w1 at=2000 w=900 h=2100\nr1: roof w1 type=gable pitch=35 overhang=300 thickness=200 base=L1\nrepeat of=house count=20 step=10000,0"
    );
    let options = gcad_mesh::Options {
        y_up: false,
        ..Default::default()
    };
    let model = gcad_mesh::build_with(&source, &options).expect("builds");
    assert_eq!(model.groups.len(), 1);
    assert_eq!(model.groups[0].name, "house");
    assert_eq!(model.instance_count(0), 20);
    let single = gcad_mesh::build_with(
        &source.replace("repeat of=house count=20 step=10000,0", ""),
        &options,
    )
    .expect("builds");
    assert_eq!(model.unique_triangles(), single.unique_triangles());
    assert!(model.unique_triangles() < 4000);
    assert_eq!(model.instances[19].transform[12], 190000.0);
    let glb = model.to_glb().expect("glb");
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json = std::str::from_utf8(&glb[20..20 + json_len]).unwrap();
    let nodes = json.matches("\"translation\"").count();
    assert!(nodes >= 19 * model.groups[0].bodies.len(), "{nodes}");
}

#[test]
fn a_straight_wall_is_a_prism() {
    let (v, _) = volume(&format!("{HEAD}w1: wall A1 B1 type=int base=L0 top=L1"));
    assert!((v - 8000.0 * 100.0 * 2700.0).abs() < 1.0e3, "{v}");
}

#[test]
fn a_closed_wall_has_no_gap_at_corners() {
    let (v, _) = volume(&format!(
        "{HEAD}w1: wall A1 B1 B2 A2 closed type=int base=L0 top=L1 justify=outside"
    ));
    let plan = 8000.0 * 6000.0 - 7800.0 * 5800.0;
    assert!((v - plan * 2700.0).abs() < 1.0e-3 * plan * 2700.0, "{v}");
}
