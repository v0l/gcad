use gcad2glb::{Assets, Fit, Options};

const MUSHROOM: &str = "\
let cap=6
stem: blob 0,0,4 r=2,2,5
cap: blob 0,0,10 r=cap,cap,3 blend=1
remesh 0.4
color #e8dcc0
color #c0392b part=cap
";

fn main() -> Result<(), gcad2glb::Error> {
    let fit = Fit {
        height: Some(1.0),
        ..Default::default()
    };
    let options = Options {
        fit,
        ..Options::default().set("cap", 8.0)
    };
    let prop = Assets::from_source(MUSHROOM, &options)?;
    for body in &prop.bodies {
        for part in &body.parts {
            let range = part.first_index..part.first_index + part.index_count;
            let corners = |t: &[u32]| {
                t.iter()
                    .map(|i| body.positions[*i as usize])
                    .collect::<Vec<_>>()
            };
            let triangles: Vec<Vec<[f32; 3]>> =
                body.indices[range].chunks(3).map(corners).collect();
            println!(
                "{} {}: {} triangles, {:?}",
                body.name,
                part.name,
                triangles.len(),
                part.colour
            );
        }
    }
    let placed = prop.fit().expect("asked for a height");
    println!("scale {} offset {:?}", placed.scale, placed.translation);
    println!("cache key {:016x}", gcad2glb::cache_key(MUSHROOM, &options));
    Ok(())
}
