use gcad_cloud::{Cloud, Layout, Point, Pose, e57, las, ply, xyz};
use std::path::Path;

struct Noise(u64);

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    }
}

fn cube_surface(side: f64, step: f64, noise: f64, rng: &mut Noise) -> Cloud {
    let n = (side / step).round() as usize;
    let layout = Layout {
        normals: true,
        colours: true,
        intensity: true,
        classes: true,
    };
    let mut cloud = Cloud::with_layout(layout, 6 * (n + 1) * (n + 1));
    let faces: [(usize, f64, [f32; 3], [u8; 3], u8); 6] = [
        (0, 0.0, [-1.0, 0.0, 0.0], [220, 60, 60], 1),
        (0, side, [1.0, 0.0, 0.0], [60, 220, 60], 1),
        (1, 0.0, [0.0, -1.0, 0.0], [60, 60, 220], 1),
        (1, side, [0.0, 1.0, 0.0], [220, 220, 60], 1),
        (2, 0.0, [0.0, 0.0, -1.0], [60, 220, 220], 2),
        (2, side, [0.0, 0.0, 1.0], [220, 60, 220], 6),
    ];
    for (axis, at, normal, colour, class) in faces {
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        for i in 0..=n {
            for j in 0..=n {
                let mut p = [0.0; 3];
                p[axis] = at;
                p[a] = i as f64 * step;
                p[b] = j as f64 * step;
                let edge = i == 0 || j == 0 || i == n || j == n;
                if noise > 0.0 && !edge {
                    p[axis] += noise * rng.next();
                }
                let mut point = Point::at(p);
                point.normal = Some(normal);
                point.colour = Some(colour);
                point.intensity = Some(((p[0] + p[1] + p[2]) / (3.0 * side)).clamp(0.0, 1.0) as f32);
                point.class = Some(class);
                cloud.push(point);
            }
        }
    }
    cloud
}

fn moved(cloud: &Cloud, by: [f64; 3]) -> Cloud {
    let mut out = cloud.clone();
    out.positions.iter_mut().for_each(|p| {
        (0..3).for_each(|k| p[k] += by[k]);
    });
    out
}

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "tests/fixtures/clouds".into());
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir)?;
    let mut rng = Noise(7);
    let cube = cube_surface(20.0, 1.0, 0.05, &mut rng);
    let mut ascii = Vec::new();
    ply::write_ascii(&cube, &mut ascii)?;
    std::fs::write(dir.join("cube_ascii.ply"), ascii)?;
    ply::write(&cube, &mut std::fs::File::create(dir.join("cube_binary.ply"))?)?;
    xyz::write(&cube, &mut std::fs::File::create(dir.join("cube.xyz"))?)?;

    let sparse = cube_surface(20.0, 4.0, 0.0, &mut rng);
    let mut second = sparse.clone();
    second.scans.iter_mut().for_each(|s| *s = 2);
    e57::write(
        &dir.join("two_scans.e57"),
        &[
            (&sparse, Pose::default()),
            (
                &second,
                Pose {
                    rotation: [
                        std::f64::consts::FRAC_1_SQRT_2,
                        0.0,
                        0.0,
                        std::f64::consts::FRAC_1_SQRT_2,
                    ],
                    translation: [100.0, 50.0, 0.0],
                },
            ),
        ],
    )?;

    let survey = moved(&cube_surface(20.0, 2.0, 0.0, &mut rng), [500000.0, 4000000.0, 0.0]);
    las::write(&dir.join("survey.las"), &survey, 0.001)?;
    las::write(&dir.join("survey.laz"), &survey, 0.001)?;
    Ok(())
}
