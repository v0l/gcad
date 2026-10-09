use crate::sim::{Outcome, Series};
use anyhow::{Result, bail};
use image::{Rgb, RgbImage};

type Contour = Vec<(f64, f64)>;

pub struct Canvas {
    pub image: RgbImage,
    font: ttf_parser::Face<'static>,
}

pub const PALETTE: &[[u8; 3]] = &[
    [31, 119, 180],
    [255, 127, 14],
    [44, 160, 44],
    [214, 39, 40],
    [148, 103, 189],
    [140, 86, 75],
    [227, 119, 194],
    [127, 127, 127],
    [188, 189, 34],
    [23, 190, 207],
];

const INK: [u8; 3] = [40, 40, 40];
const FAINT: [u8; 3] = [225, 225, 225];

#[derive(Clone, Copy)]
pub enum Anchor {
    Left,
    Centre,
    Right,
}

struct Outline {
    contours: Vec<Contour>,
    at: (f64, f64),
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.contours.push(vec![(x as f64, y as f64)]);
        self.at = (x as f64, y as f64);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.at = (x as f64, y as f64);
        if let Some(contour) = self.contours.last_mut() {
            contour.push(self.at);
        }
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, p1, p2) = (self.at, (x1 as f64, y1 as f64), (x as f64, y as f64));
        for k in 1..=6 {
            let t = k as f64 / 6.0;
            let u = 1.0 - t;
            self.line_to(
                (u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0) as f32,
                (u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1) as f32,
            );
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p = [
            self.at,
            (x1 as f64, y1 as f64),
            (x2 as f64, y2 as f64),
            (x as f64, y as f64),
        ];
        for k in 1..=8 {
            let t = k as f64 / 8.0;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            let x = (0..4).map(|i| w[i] * p[i].0).sum::<f64>();
            let y = (0..4).map(|i| w[i] * p[i].1).sum::<f64>();
            self.line_to(x as f32, y as f32);
        }
    }

    fn close(&mut self) {}
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Canvas {
        Canvas {
            image: RgbImage::from_pixel(width, height, Rgb([255, 255, 255])),
            font: ttf_parser::Face::parse(epaint_default_fonts::UBUNTU_LIGHT, 0)
                .expect("the bundled font parses"),
        }
    }

    pub fn fill(&mut self, contours: &[Contour], colour: [u8; 3]) {
        let (width, height) = self.image.dimensions();
        let ys = contours.iter().flatten().map(|p| p.1);
        let low = ys.clone().fold(f64::INFINITY, f64::min).floor().max(0.0) as u32;
        let high = ys
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .min(height as f64) as u32;
        let edges: Vec<((f64, f64), (f64, f64))> = contours
            .iter()
            .filter(|c| c.len() > 1)
            .flat_map(|c| (0..c.len()).map(move |i| (c[i], c[(i + 1) % c.len()])))
            .filter(|(a, b)| a.1 != b.1)
            .collect();
        const SAMPLES: usize = 4;
        let mut cover = vec![0.0f64; width as usize + 1];
        for row in low..high {
            cover.iter_mut().for_each(|c| *c = 0.0);
            for s in 0..SAMPLES {
                let y = row as f64 + (s as f64 + 0.5) / SAMPLES as f64;
                let mut crossings: Vec<(f64, i32)> = edges
                    .iter()
                    .filter(|(a, b)| (a.1 <= y) != (b.1 <= y))
                    .map(|(a, b)| {
                        let t = (y - a.1) / (b.1 - a.1);
                        (a.0 + t * (b.0 - a.0), if b.1 > a.1 { 1 } else { -1 })
                    })
                    .collect();
                crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut winding = 0;
                for pair in crossings.windows(2) {
                    winding += pair[0].1;
                    if winding != 0 {
                        span(&mut cover, pair[0].0, pair[1].0, 1.0 / SAMPLES as f64);
                    }
                }
            }
            for (x, &c) in cover.iter().enumerate().take(width as usize) {
                if c <= 0.0 {
                    continue;
                }
                let c = c.min(1.0);
                let pixel = self.image.get_pixel_mut(x as u32, row);
                for k in 0..3 {
                    pixel[k] = (pixel[k] as f64 * (1.0 - c) + colour[k] as f64 * c).round() as u8;
                }
            }
        }
    }

    pub fn line(&mut self, points: &[(f64, f64)], width: f64, colour: [u8; 3]) {
        let half = width / 2.0;
        let quads: Vec<Contour> = points
            .windows(2)
            .filter_map(|pair| {
                let (a, b) = (pair[0], pair[1]);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let length = (dx * dx + dy * dy).sqrt();
                (length > 1.0e-9).then(|| {
                    let (nx, ny) = (-dy / length * half, dx / length * half);
                    let (ex, ey) = (dx / length * half, dy / length * half);
                    vec![
                        (a.0 + nx - ex, a.1 + ny - ey),
                        (b.0 + nx + ex, b.1 + ny + ey),
                        (b.0 - nx + ex, b.1 - ny + ey),
                        (a.0 - nx - ex, a.1 - ny - ey),
                    ]
                })
            })
            .collect();
        for quad in quads {
            self.fill(&[quad], colour);
        }
    }

    pub fn rect(&mut self, (x0, y0): (f64, f64), (x1, y1): (f64, f64), colour: [u8; 3]) {
        self.fill(&[vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]], colour);
    }

    pub fn text_width(&self, text: &str, size: f64) -> f64 {
        let scale = size / self.font.units_per_em() as f64;
        text.chars()
            .filter_map(|c| self.font.glyph_index(c))
            .map(|g| self.font.glyph_hor_advance(g).unwrap_or(0) as f64 * scale)
            .sum()
    }

    pub fn text(
        &mut self,
        (x, y): (f64, f64),
        size: f64,
        text: &str,
        anchor: Anchor,
        colour: [u8; 3],
    ) {
        let scale = size / self.font.units_per_em() as f64;
        let width = self.text_width(text, size);
        let mut pen = match anchor {
            Anchor::Left => x,
            Anchor::Centre => x - width / 2.0,
            Anchor::Right => x - width,
        };
        let mut contours = Vec::new();
        for c in text.chars() {
            let Some(glyph) = self.font.glyph_index(c) else {
                continue;
            };
            let mut outline = Outline {
                contours: Vec::new(),
                at: (0.0, 0.0),
            };
            self.font.outline_glyph(glyph, &mut outline);
            contours.extend(outline.contours.into_iter().map(|contour| {
                contour
                    .into_iter()
                    .map(|(gx, gy)| (pen + gx * scale, y - gy * scale))
                    .collect::<Contour>()
            }));
            pen += self.font.glyph_hor_advance(glyph).unwrap_or(0) as f64 * scale;
        }
        self.fill(&contours, colour);
    }

    pub fn paste(&mut self, picture: &RgbImage, (x0, y0): (u32, u32)) {
        for (x, y, pixel) in picture.enumerate_pixels() {
            if x + x0 < self.image.width() && y + y0 < self.image.height() {
                self.image.put_pixel(x + x0, y + y0, *pixel);
            }
        }
    }

    pub fn save(&self, path: &str) -> Result<()> {
        self.image.save(path)?;
        Ok(())
    }
}

fn span(cover: &mut [f64], from: f64, to: f64, weight: f64) {
    let last = cover.len() as f64 - 1.0;
    let (from, to) = (from.clamp(0.0, last), to.clamp(0.0, last));
    if to <= from {
        return;
    }
    let mut x = from.floor();
    while x < to {
        let overlap = (x + 1.0).min(to) - x.max(from);
        cover[x as usize] += overlap * weight;
        x += 1.0;
    }
}

pub fn ticks(low: f64, high: f64, about: usize) -> Vec<f64> {
    let range = (high - low).abs().max(1.0e-12);
    let raw = range / about.max(1) as f64;
    let power = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * power)
        .find(|s| *s >= raw)
        .unwrap_or(10.0 * power);
    let first = (low / step).ceil() as i64;
    let last = (high / step).floor() as i64;
    (first..=last).map(|k| k as f64 * step).collect()
}

pub fn label(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

fn panel(canvas: &mut Canvas, series: &Series, top: f64, height: f64, width: f64) {
    let legend = 300.0;
    let (left, right) = (90.0, width - legend - 30.0);
    let (upper, lower) = (top + 50.0, top + height - 60.0);
    canvas.text(
        ((left + right) / 2.0, top + 30.0),
        22.0,
        &series.title,
        Anchor::Centre,
        INK,
    );
    let values: Vec<f64> = series
        .curves
        .iter()
        .flat_map(|c| c.values.iter().copied())
        .filter(|v| v.is_finite())
        .collect();
    let (mut low, mut high) = values
        .iter()
        .fold((0.0f64, 0.0f64), |(l, h), v| (l.min(*v), h.max(*v)));
    if high - low < 1.0e-9 {
        high += 1.0;
        low -= 1.0;
    }
    let pad = (high - low) * 0.05;
    let (low, high) = (low - pad, high + pad);
    let (x0, x1) = series
        .x
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
            (l.min(*v), h.max(*v))
        });
    let (x0, x1) = if x1 > x0 {
        (x0, x1)
    } else {
        (x0 - 1.0, x0 + 1.0)
    };
    let px = |x: f64| left + (x - x0) / (x1 - x0) * (right - left);
    let py = |y: f64| lower - (y - low) / (high - low) * (lower - upper);
    for t in ticks(low, high, 6) {
        canvas.line(&[(left, py(t)), (right, py(t))], 1.0, FAINT);
        canvas.text(
            (left - 8.0, py(t) + 5.0),
            15.0,
            &label(t),
            Anchor::Right,
            INK,
        );
    }
    for t in ticks(x0, x1, 8) {
        canvas.line(&[(px(t), upper), (px(t), lower)], 1.0, FAINT);
        canvas.text((px(t), lower + 20.0), 15.0, &label(t), Anchor::Centre, INK);
    }
    if low < 0.0 && high > 0.0 {
        canvas.line(&[(left, py(0.0)), (right, py(0.0))], 1.5, [150, 150, 150]);
    }
    canvas.line(&[(left, upper), (left, lower), (right, lower)], 1.5, INK);
    canvas.text(
        ((left + right) / 2.0, lower + 45.0),
        17.0,
        &series.axis,
        Anchor::Centre,
        INK,
    );
    let units: Vec<&str> = series.curves.iter().map(|c| c.unit.as_str()).collect();
    let mut units = units.clone();
    units.dedup();
    canvas.text(
        (left - 8.0, upper - 14.0),
        15.0,
        &units.join(", "),
        Anchor::Right,
        INK,
    );
    for (k, curve) in series.curves.iter().enumerate() {
        let colour = PALETTE[k % PALETTE.len()];
        let mut run: Vec<(f64, f64)> = Vec::new();
        for (x, y) in series.x.iter().zip(&curve.values) {
            if y.is_finite() {
                run.push((px(*x), py(*y)));
            } else if !run.is_empty() {
                canvas.line(&run, 2.5, colour);
                run.clear();
            }
        }
        canvas.line(&run, 2.5, colour);
        let ly = upper + 10.0 + k as f64 * 22.0;
        canvas.line(
            &[(right + 30.0, ly - 5.0), (right + 60.0, ly - 5.0)],
            3.0,
            colour,
        );
        canvas.text((right + 68.0, ly), 15.0, &curve.name, Anchor::Left, INK);
    }
}

pub fn chart(outcome: &Outcome, path: &str) -> Result<()> {
    if !path.to_ascii_lowercase().ends_with(".png") {
        bail!("a simulation renders to a .png");
    }
    if outcome.series.is_empty() {
        bail!("this result has nothing to plot; add a `sweep` line and rerun `gcad sim`");
    }
    let width = 1400.0;
    let panels: Vec<Series> = outcome.series.iter().flat_map(by_unit).collect();
    if panels.is_empty() {
        bail!("every curve in this result is zero");
    }
    let heights: Vec<f64> = panels
        .iter()
        .map(|s| (s.curves.len() as f64 * 22.0 + 140.0).max(480.0))
        .collect();
    let total: f64 = heights.iter().sum();
    let mut canvas = Canvas::new(width as u32, total as u32);
    let mut top = 0.0;
    for (series, height) in panels.iter().zip(heights) {
        panel(&mut canvas, series, top, height, width);
        top += height;
    }
    canvas.save(path)
}

fn by_unit(series: &Series) -> Vec<Series> {
    let shown: Vec<&crate::sim::Curve> = series
        .curves
        .iter()
        .filter(|c| c.values.iter().any(|v| v.is_finite() && v.abs() > 1.0e-9))
        .collect();
    let mut units: Vec<&str> = Vec::new();
    for curve in &shown {
        if !units.contains(&curve.unit.as_str()) {
            units.push(&curve.unit);
        }
    }
    units
        .iter()
        .map(|unit| Series {
            title: if units.len() > 1 {
                format!("{}, {unit}", series.title)
            } else {
                series.title.clone()
            },
            axis: series.axis.clone(),
            x: series.x.clone(),
            curves: shown
                .iter()
                .filter(|c| c.unit == *unit)
                .map(|c| (*c).clone())
                .collect(),
        })
        .collect()
}

pub fn heat(t: f64) -> [f64; 3] {
    let stops = [
        [0.17, 0.30, 0.75],
        [0.15, 0.65, 0.85],
        [0.35, 0.75, 0.35],
        [0.95, 0.85, 0.25],
        [0.85, 0.20, 0.15],
    ];
    let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f64;
    let i = (t.floor() as usize).min(stops.len() - 2);
    let f = t - i as f64;
    [0, 1, 2].map(|k| stops[i][k] * (1.0 - f) + stops[i + 1][k] * f)
}

pub fn colour_bar(
    canvas: &mut Canvas,
    (x, top, bottom): (f64, f64, f64),
    low: f64,
    high: f64,
    unit: &str,
) {
    let steps = 200;
    for k in 0..steps {
        let t = k as f64 / steps as f64;
        let y0 = bottom - (bottom - top) * t;
        let y1 = bottom - (bottom - top) * (t + 1.0 / steps as f64);
        let c = heat(t).map(|v| (v * 255.0) as u8);
        canvas.rect((x, y1), (x + 30.0, y0 + 0.5), c);
    }
    for t in ticks(low, high, 8) {
        let y = bottom - (bottom - top) * (t - low) / (high - low).max(1.0e-300);
        canvas.line(&[(x + 30.0, y), (x + 38.0, y)], 1.5, INK);
        canvas.text((x + 44.0, y + 5.0), 15.0, &label(t), Anchor::Left, INK);
    }
    canvas.text((x + 15.0, top - 16.0), 16.0, unit, Anchor::Centre, INK);
}

pub fn title(canvas: &mut Canvas, (x, y): (f64, f64), text: &str) {
    canvas.text((x, y), 20.0, text, Anchor::Centre, INK);
}
