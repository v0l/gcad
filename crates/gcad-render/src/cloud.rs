use crate::model::Model;
use anyhow::{Result, bail};
use gcad_cloud::colour::{self, ColourBy};
use gcad_cloud::Cloud;
use gcad_plot::{Anchor, Canvas};
use image::{Rgb, RgbImage};

const CELL: u32 = 600;
const LEGEND: u32 = 150;
const DRAW_CAP: usize = 400_000;
const BACKGROUND: [u8; 3] = [250, 250, 248];

struct View {
    right: [f64; 3],
    up: [f64; 3],
    toward: [f64; 3],
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    v.map(|c| c / n)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn view(eye: [f64; 3], hint: [f64; 3]) -> View {
    let toward = unit(eye);
    let right = unit(cross(hint, toward));
    View {
        right,
        up: cross(toward, right),
        toward,
    }
}

fn views() -> [View; 4] {
    [
        view([1.0, -1.0, 0.8], [0.0, 0.0, 1.0]),
        view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        view([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
        view([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ]
}

pub fn point_colours(cloud: &Cloud) -> (ColourBy, Vec<[u8; 3]>) {
    let by = if cloud.deviation.is_some() {
        ColourBy::Deviation
    } else if cloud.colours.is_some() {
        ColourBy::Rgb
    } else {
        ColourBy::Height
    };
    let all: Vec<usize> = (0..cloud.len()).collect();
    (by, colour::colours(cloud, by, &all))
}

pub fn draw_views(cloud: &Cloud, size: u32) -> RgbImage {
    let (_, colours) = point_colours(cloud);
    let (lo, hi) = cloud.bounds().unwrap_or(([0.0; 3], [1.0; 3]));
    let centre: [f64; 3] = std::array::from_fn(|k| (lo[k] + hi[k]) / 2.0);
    let radius = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt() / 2.0;
    let radius = radius.max(1.0e-9);
    let step = (cloud.len() / DRAW_CAP).max(1);
    let mut image = RgbImage::from_pixel(size * 2, size * 2, Rgb(BACKGROUND));
    for (index, view) in views().iter().enumerate() {
        let mut depth = vec![f64::NEG_INFINITY; (size * size) as usize];
        let (ox, oy) = ((index as u32 % 2) * size, (index as u32 / 2) * size);
        let scale = size as f64 * 0.42 / radius;
        for i in (0..cloud.len()).step_by(step) {
            let d: [f64; 3] = std::array::from_fn(|k| cloud.positions[i][k] - centre[k]);
            let x = size as f64 / 2.0 + dot(d, view.right) * scale;
            let y = size as f64 / 2.0 - dot(d, view.up) * scale;
            let z = dot(d, view.toward);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (px, py) = (x.round() as i64 + dx, y.round() as i64 + dy);
                    if px < 0 || py < 0 || px >= size as i64 || py >= size as i64 {
                        continue;
                    }
                    let slot = py as usize * size as usize + px as usize;
                    if z >= depth[slot] {
                        depth[slot] = z;
                        image.put_pixel(ox + px as u32, oy + py as u32, Rgb(colours[i]));
                    }
                }
            }
        }
    }
    (0..size * 2).for_each(|i| {
        image.put_pixel(size, i, Rgb([200, 200, 200]));
        image.put_pixel(i, size, Rgb([200, 200, 200]));
    });
    image
}

pub fn legend(canvas: &mut Canvas, x: f64, top: f64, bottom: f64, limit: f64, tolerance: f64) {
    let steps = 200;
    for k in 0..steps {
        let t = k as f64 / steps as f64;
        let y0 = bottom - (bottom - top) * t;
        let y1 = bottom - (bottom - top) * (t + 1.0 / steps as f64);
        canvas.rect((x, y1), (x + 30.0, y0 + 0.5), colour::diverging(t as f32));
    }
    let ink = [40, 40, 40];
    for t in gcad_plot::ticks(-limit, limit, 6) {
        let y = bottom - (bottom - top) * (t + limit) / (2.0 * limit);
        canvas.line(&[(x + 30.0, y), (x + 38.0, y)], 1.5, ink);
        canvas.text((x + 44.0, y + 5.0), 15.0, &gcad_parse::label(t), Anchor::Left, ink);
    }
    canvas.text((x + 15.0, top - 22.0), 16.0, "deviation", Anchor::Centre, ink);
    canvas.text((x + 15.0, top - 6.0), 14.0, &format!("tol {}", gcad_parse::label(tolerance)), Anchor::Centre, ink);
}

pub fn render_clouds(model: &Model, path: &str) -> Result<()> {
    let clouds = model.named_clouds();
    let Some((_, cloud)) = clouds
        .iter()
        .find(|(_, c)| c.deviation.is_some())
        .or_else(|| clouds.first())
    else {
        bail!("the model has no point cloud");
    };
    let views = draw_views(cloud, CELL);
    let width = CELL * 2 + LEGEND;
    let mut canvas = Canvas::new(width, CELL * 2);
    canvas.paste(&views, (0, 0));
    if cloud.deviation.is_some() {
        let tolerance = model.inspections.first().map_or(0.0, |i| i.tolerance);
        legend(
            &mut canvas,
            (CELL * 2 + 10) as f64,
            90.0,
            (CELL * 2) as f64 - 90.0,
            colour::deviation_limit(cloud),
            tolerance,
        );
    }
    canvas.save(path)
}
