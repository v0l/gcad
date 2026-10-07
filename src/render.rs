use crate::geometry;
use anyhow::Result;
use image::{Rgb, RgbImage};
use monstertruck::modeling::*;

const CELL: u32 = 600;
const BACKGROUND: Rgb<u8> = Rgb([250, 250, 248]);
const EDGE: Rgb<u8> = Rgb([25, 25, 30]);
const OVERLAY: f64 = 1.0e300;

struct View {
    right: Vector3,
    up: Vector3,
    toward_eye: Vector3,
}

impl View {
    fn looking_from(eye: Vector3, up_hint: Vector3) -> View {
        let toward_eye = eye.normalize();
        let right = up_hint.cross(toward_eye).normalize();
        View {
            right,
            up: toward_eye.cross(right),
            toward_eye,
        }
    }
}

fn views() -> [View; 4] {
    [
        View::looking_from(Vector3::new(1.0, -1.0, 0.8), Vector3::unit_z()),
        View::looking_from(Vector3::unit_z(), Vector3::unit_y()),
        View::looking_from(-Vector3::unit_y(), Vector3::unit_z()),
        View::looking_from(Vector3::unit_x(), Vector3::unit_z()),
    ]
}

struct Canvas<'a> {
    image: &'a mut RgbImage,
    depth: Vec<f64>,
    offset: (u32, u32),
}

impl Canvas<'_> {
    fn plot(&mut self, x: i64, y: i64, z: f64, color: Rgb<u8>) {
        if x < 0 || y < 0 || x >= CELL as i64 || y >= CELL as i64 {
            return;
        }
        let index = y as usize * CELL as usize + x as usize;
        if z >= self.depth[index] {
            self.depth[index] = z;
            self.image
                .put_pixel(self.offset.0 + x as u32, self.offset.1 + y as u32, color);
        }
    }

    fn triangle(&mut self, p: [(f64, f64, f64); 3], color: Rgb<u8>) {
        let min_x = p
            .iter()
            .map(|q| q.0)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as i64;
        let max_x = p
            .iter()
            .map(|q| q.0)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .min(CELL as f64 - 1.0) as i64;
        let min_y = p
            .iter()
            .map(|q| q.1)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as i64;
        let max_y = p
            .iter()
            .map(|q| q.1)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .min(CELL as f64 - 1.0) as i64;
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        if area.abs() < 1.0e-12 {
            return;
        }
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let w0 = ((p[1].0 - px) * (p[2].1 - py) - (p[2].0 - px) * (p[1].1 - py)) / area;
                let w1 = ((p[2].0 - px) * (p[0].1 - py) - (p[0].0 - px) * (p[2].1 - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    self.plot(x, y, w0 * p[0].2 + w1 * p[1].2 + w2 * p[2].2, color);
                }
            }
        }
    }

    fn line(&mut self, a: (f64, f64, f64), b: (f64, f64, f64), color: Rgb<u8>, bias: f64) {
        let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil() as usize).max(1);
        (0..=steps).for_each(|i| {
            let t = i as f64 / steps as f64;
            let (x, y, z) = (
                a.0 + (b.0 - a.0) * t,
                a.1 + (b.1 - a.1) * t,
                a.2 + (b.2 - a.2) * t,
            );
            self.plot(x.round() as i64, y.round() as i64, z + bias, color);
        });
    }
}

fn edge_polylines(solid: &Solid) -> Vec<Vec<Point3>> {
    let mut seen = Vec::new();
    solid
        .boundaries()
        .iter()
        .flat_map(|shell| shell.edge_iter())
        .filter(|edge| {
            let fresh = !seen.contains(&edge.id());
            seen.push(edge.id());
            fresh
        })
        .map(|edge| {
            let curve = edge.curve();
            let (t0, t1) = curve.range_tuple();
            (0..=48)
                .map(|i| curve.subs(t0 + (t1 - t0) * i as f64 / 48.0))
                .collect()
        })
        .collect()
}

pub fn render(solids: &[&Solid], path: &str) -> Result<()> {
    let mut mesh = monstertruck::mesh::PolygonMesh::default();
    solids
        .iter()
        .for_each(|solid| mesh.merge(geometry::mesh(solid, geometry::mesh_tolerance(solid))));
    let positions = mesh.positions();
    let triangles: Vec<[usize; 3]> = mesh
        .faces()
        .triangle_iter()
        .map(|t| [t[0].pos, t[1].pos, t[2].pos])
        .collect();
    let edges: Vec<Vec<Point3>> = solids
        .iter()
        .flat_map(|solid| edge_polylines(solid))
        .collect();
    let bounds: BoundingBox<Point3> = solids
        .iter()
        .flat_map(|solid| [geometry::bounds(solid).min(), geometry::bounds(solid).max()])
        .collect();
    let center = bounds.center().to_vec();
    let mut image = RgbImage::from_pixel(CELL * 2, CELL * 2, BACKGROUND);
    for (index, view) in views().iter().enumerate() {
        let project = |p: Point3| {
            let v = p.to_vec() - center;
            (v.dot(view.right), v.dot(view.up), v.dot(view.toward_eye))
        };
        let projected: Vec<(f64, f64, f64)> = positions.iter().map(|&p| project(p)).collect();
        let extent = projected
            .iter()
            .map(|p| p.0.abs().max(p.1.abs()))
            .fold(1.0e-9, f64::max);
        let scale = (CELL as f64 * 0.42) / extent;
        let depth_range = projected.iter().map(|p| p.2.abs()).fold(1.0e-9, f64::max);
        let to_pixel = |(x, y, z): (f64, f64, f64)| {
            (
                CELL as f64 / 2.0 + x * scale,
                CELL as f64 / 2.0 - y * scale,
                z,
            )
        };
        let mut canvas = Canvas {
            image: &mut image,
            depth: vec![f64::NEG_INFINITY; (CELL * CELL) as usize],
            offset: ((index as u32 % 2) * CELL, (index as u32 / 2) * CELL),
        };
        for triangle in &triangles {
            let [a, b, c] = triangle.map(|i| positions[i]);
            let normal = (b - a).cross(c - a);
            if normal.magnitude2() < 1.0e-24 {
                continue;
            }
            let light = normal
                .normalize()
                .dot(Vector3::new(0.3, -0.5, 0.8).normalize());
            let facing = normal.normalize().dot(view.toward_eye).abs();
            let shade = 0.35 + 0.4 * facing + 0.25 * light.max(0.0);
            let color = Rgb([
                (150.0 * shade) as u8,
                (175.0 * shade) as u8,
                (205.0 * shade) as u8,
            ]);
            canvas.triangle(triangle.map(|i| to_pixel(projected[i])), color);
        }
        let bias = depth_range * 0.01;
        for polyline in &edges {
            polyline.windows(2).for_each(|pair| {
                canvas.line(
                    to_pixel(project(pair[0])),
                    to_pixel(project(pair[1])),
                    EDGE,
                    bias,
                );
            });
        }
        let triad = extent * 0.15;
        let origin = (40.0, CELL as f64 - 40.0);
        [
            (Vector3::unit_x(), Rgb([220, 40, 40])),
            (Vector3::unit_y(), Rgb([30, 160, 60])),
            (Vector3::unit_z(), Rgb([40, 80, 220])),
        ]
        .into_iter()
        .for_each(|(axis, color)| {
            let tip = (
                origin.0 + axis.dot(view.right) * triad * scale,
                origin.1 - axis.dot(view.up) * triad * scale,
                OVERLAY,
            );
            canvas.line((origin.0, origin.1, OVERLAY), tip, color, 0.0);
        });
    }
    (0..CELL * 2).for_each(|i| {
        image.put_pixel(CELL, i, Rgb([200, 200, 200]));
        image.put_pixel(i, CELL, Rgb([200, 200, 200]));
    });
    image.save(path)?;
    Ok(())
}
