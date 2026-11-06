use super::brush::{Falloff, smoothstep};
use super::channels::Layer;
use super::grid::{Grid, Surface};
use super::{Mesh, P3, Selection, add, dot, length, scale, sub, surface_distance};
use anyhow::{Result, bail};

impl Mesh {
    pub fn curvature(&self) -> Vec<f64> {
        let around = self.neighbours();
        let normals = self.vertex_normals();
        (0..self.positions.len())
            .map(|i| {
                let ring = &around[i];
                if ring.is_empty() {
                    return 0.0;
                }
                let p = self.positions[i];
                let (sum, sq) = ring.iter().fold(([0.0; 3], 0.0), |(s, q), &j| {
                    let d = sub(self.positions[j as usize], p);
                    (add(s, d), q + dot(d, d))
                });
                let n = ring.len() as f64;
                let mean = scale(sum, 1.0 / n);
                let h = dot(mean, normals[i]);
                let e2 = sq / n - h * h;
                if e2 <= 0.0 { 0.0 } else { -2.0 * h / e2 }
            })
            .collect()
    }

    pub fn mask_with(&mut self, values: &[f64], amount: f64) -> usize {
        let mask = self.mask_mut();
        let mut count = 0;
        for (m, &v) in mask.iter_mut().zip(values) {
            let w = (v * amount).clamp(0.0, 1.0);
            if w > *m {
                *m = w;
            }
            if *m > 0.0 {
                count += 1;
            }
        }
        count
    }

    pub fn mask_count(&self) -> usize {
        (0..self.positions.len())
            .filter(|&i| self.mask.get(i).copied().unwrap_or(0.0) > 0.0)
            .count()
    }

    pub fn mask_invert(&mut self) {
        for m in self.mask_mut() {
            *m = 1.0 - m.clamp(0.0, 1.0);
        }
    }

    pub fn mask_blur(&mut self, passes: usize) {
        let around = self.neighbours();
        for _ in 0..passes {
            let before = self.mask_mut().clone();
            for (i, m) in self.mask.iter_mut().enumerate() {
                let ring = &around[i];
                if ring.is_empty() {
                    continue;
                }
                let sum: f64 = ring.iter().map(|&j| before[j as usize]).sum();
                *m = 0.5 * before[i] + 0.5 * sum / ring.len() as f64;
            }
        }
    }

    pub fn mask_grow(&mut self, by: f64) {
        let core: Vec<bool> = self.mask_mut().iter().map(|&m| m >= 0.5).collect();
        let reach = surface_distance(self, &core, by);
        for (m, d) in self.mask.iter_mut().zip(reach) {
            if d < by {
                *m = 1.0;
            }
        }
    }

    pub fn mask_shrink(&mut self, by: f64) {
        self.mask_invert();
        self.mask_grow(by);
        self.mask_invert();
    }

    pub fn join_parts(&mut self, a: u16, b: u16, blend: f64, profile: &Falloff) -> usize {
        let side = |want: u16| -> Vec<bool> { self.labels.iter().map(|&l| l == want).collect() };
        let (in_a, in_b) = (side(a), side(b));
        let border: Vec<bool> = {
            let around = self.neighbours();
            (0..self.positions.len())
                .map(|i| {
                    (in_a[i] && around[i].iter().any(|&j| in_b[j as usize]))
                        || (in_b[i] && around[i].iter().any(|&j| in_a[j as usize]))
                })
                .collect()
        };
        let reach = if border.iter().any(|&b| b) {
            surface_distance(self, &border, blend)
        } else {
            let grid_a: Vec<P3> = (0..self.positions.len())
                .filter(|&i| in_a[i])
                .map(|i| self.positions[i])
                .collect();
            let grid_b: Vec<P3> = (0..self.positions.len())
                .filter(|&i| in_b[i])
                .map(|i| self.positions[i])
                .collect();
            let (ga, gb) = (Grid::new(&grid_a, blend), Grid::new(&grid_b, blend));
            (0..self.positions.len())
                .map(|i| {
                    let p = self.positions[i];
                    let other = if in_a[i] {
                        gb.nearest(&grid_b, p)
                    } else if in_b[i] {
                        ga.nearest(&grid_a, p)
                    } else {
                        None
                    };
                    other.map_or(f64::INFINITY, |(_, d)| d)
                })
                .collect()
        };
        let weights: Vec<f64> = reach
            .iter()
            .enumerate()
            .map(|(i, &d)| {
                if (in_a[i] || in_b[i]) && d < blend {
                    profile.at(d / blend) * (1.0 - self.mask_at(i).clamp(0.0, 1.0))
                } else {
                    0.0
                }
            })
            .collect();
        let passes = 12;
        let around = self.neighbours();
        for _ in 0..passes {
            let before = self.positions.clone();
            for (i, p) in self.positions.iter_mut().enumerate() {
                let w = weights[i];
                if w <= 0.0 || around[i].is_empty() {
                    continue;
                }
                let sum = around[i]
                    .iter()
                    .fold([0.0; 3], |s, &j| add(s, before[j as usize]));
                let mean = scale(sum, 1.0 / around[i].len() as f64);
                *p = add(*p, scale(sub(mean, *p), 0.5 * w));
            }
        }
        weights.iter().filter(|&&w| w > 0.0).count()
    }

    pub fn project_onto(&mut self, target: &Mesh, selection: &Selection) -> (usize, f64) {
        let surface = Surface::new(target);
        let (mut moved, mut most) = (0, 0.0f64);
        for i in 0..self.positions.len() {
            let w = selection.weights[i];
            if w <= 0.0 {
                continue;
            }
            let p = self.positions[i];
            if let Some(hit) = surface.closest(p) {
                let q = add(p, scale(sub(hit.point, p), w.min(1.0)));
                most = most.max(length(sub(q, p)));
                self.moved_to(i, q);
                moved += 1;
            }
        }
        (moved, most)
    }

    pub fn subdivide(&mut self, levels: usize) -> usize {
        let all = Selection {
            regions: vec![super::Region::All],
            falloff: 0.0,
            mirror: None,
            weights: vec![1.0; self.positions.len()],
            centre: [0.0; 3],
            weighting: Default::default(),
        };
        let mut added = 0;
        for _ in 0..levels {
            let mut every = all.clone();
            every.weights = vec![1.0; self.positions.len()];
            added += self.refine(&every, 0.0);
        }
        added
    }

    pub fn start_layer(&mut self, name: &str) -> Result<()> {
        if self.layers.iter().any(|l| l.name == name) {
            bail!("there is already a layer `{name}`; dial it with `layer {name} amount=0.5`");
        }
        self.freeze_layers();
        self.layers.push(Layer {
            name: name.to_string(),
            amount: 1.0,
            delta: Vec::new(),
            origin: Some(self.positions.clone()),
        });
        Ok(())
    }

    pub fn freeze_layers(&mut self) {
        for layer in &mut self.layers {
            if let Some(origin) = layer.origin.take() {
                layer.delta = self
                    .positions
                    .iter()
                    .zip(&origin)
                    .map(|(p, o)| sub(*p, *o))
                    .collect();
                layer.amount = 1.0;
            }
        }
    }

    pub fn set_layer(&mut self, name: &str, amount: f64) -> Result<f64> {
        self.freeze_layers();
        let Some(k) = self.layers.iter().position(|l| l.name == name) else {
            bail!(
                "there is no layer `{name}`; the layers are {}",
                if self.layers.is_empty() {
                    "none".to_string()
                } else {
                    self.layers
                        .iter()
                        .map(|l| l.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
        };
        let change = amount - self.layers[k].amount;
        let mut most = 0.0f64;
        for i in 0..self.positions.len() {
            let d = scale(self.layers[k].delta[i], change);
            most = most.max(length(d));
            self.positions[i] = add(self.positions[i], d);
        }
        self.layers[k].amount = amount;
        Ok(most)
    }

    pub fn height_values(&self, axis: P3, from: f64, to: f64) -> Vec<f64> {
        self.positions
            .iter()
            .map(|&p| {
                let h = dot(p, axis);
                if (to - from).abs() < 1.0e-12 {
                    if h >= to { 1.0 } else { 0.0 }
                } else {
                    smoothstep((h - from) / (to - from))
                }
            })
            .collect()
    }
}
