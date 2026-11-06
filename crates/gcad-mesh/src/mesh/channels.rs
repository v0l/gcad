use super::{Mesh, P3, add, scale, sub};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layer {
    pub name: String,
    pub amount: f64,
    pub delta: Vec<P3>,
    pub origin: Option<Vec<P3>>,
}

fn middle(a: P3, b: P3) -> P3 {
    scale(add(a, b), 0.5)
}

impl Layer {
    pub fn displacement(&self, positions: &[P3], i: usize) -> P3 {
        match &self.origin {
            Some(origin) => sub(positions[i], origin[i]),
            None => self.delta[i],
        }
    }
}

impl Mesh {
    pub fn mask_at(&self, i: usize) -> f64 {
        self.mask.get(i).copied().unwrap_or(0.0)
    }

    pub fn has_mask(&self) -> bool {
        self.mask.iter().any(|&m| m > 0.0)
    }

    pub fn mask_mut(&mut self) -> &mut Vec<f64> {
        self.mask.resize(self.positions.len(), 0.0);
        &mut self.mask
    }

    pub fn push_between(&mut self, p: P3, a: u32, b: u32) -> u32 {
        let (a, b) = (a as usize, b as usize);
        let label = self.labels[a];
        if !self.mask.is_empty() {
            let m = (self.mask_at(a) + self.mask_at(b)) * 0.5;
            self.mask_mut().push(m);
        }
        for layer in &mut self.layers {
            let d = middle(
                layer.displacement(&self.positions, a),
                layer.displacement(&self.positions, b),
            );
            match &mut layer.origin {
                Some(origin) => origin.push(sub(p, d)),
                None => layer.delta.push(d),
            }
        }
        self.positions.push(p);
        self.labels.push(label);
        (self.positions.len() - 1) as u32
    }

    pub fn moved_to(&mut self, i: usize, p: P3) {
        for layer in &mut self.layers {
            if let Some(origin) = &mut layer.origin {
                let d = sub(self.positions[i], origin[i]);
                origin[i] = sub(p, d);
            }
        }
        self.positions[i] = p;
    }

    pub fn keep_vertices(&mut self, keep: &[u32]) {
        let pick = |v: &[P3]| keep.iter().map(|&i| v[i as usize]).collect::<Vec<P3>>();
        if !self.mask.is_empty() {
            self.mask = keep.iter().map(|&i| self.mask_at(i as usize)).collect();
        }
        for layer in &mut self.layers {
            match &mut layer.origin {
                Some(origin) => *origin = pick(origin),
                None => layer.delta = pick(&layer.delta),
            }
        }
        self.positions = pick(&self.positions);
        self.labels = keep.iter().map(|&i| self.labels[i as usize]).collect();
    }

    pub(crate) fn merge_channels(&mut self, other: &Mesh, before: usize) {
        if !self.mask.is_empty() || !other.mask.is_empty() {
            self.mask.resize(before, 0.0);
            self.mask
                .extend((0..other.positions.len()).map(|i| other.mask_at(i)));
        }
        for layer in &mut self.layers {
            let theirs = other.layers.iter().find(|l| l.name == layer.name);
            let d = |i: usize| theirs.map_or([0.0; 3], |l| l.displacement(&other.positions, i));
            match &mut layer.origin {
                Some(origin) => origin.extend(
                    (0..other.positions.len()).map(|i| sub(other.positions[i], d(i))),
                ),
                None => layer.delta.extend((0..other.positions.len()).map(d)),
            }
        }
    }

    pub(crate) fn transform_channels(&self, out: &mut Mesh, f: &impl Fn(P3) -> P3) {
        out.mask = self.mask.clone();
        out.layers = self
            .layers
            .iter()
            .map(|layer| Layer {
                origin: layer
                    .origin
                    .as_ref()
                    .map(|o| o.iter().map(|&p| f(p)).collect()),
                delta: if layer.origin.is_some() {
                    Vec::new()
                } else {
                    self.positions
                        .iter()
                        .zip(&layer.delta)
                        .map(|(&p, &d)| sub(f(add(p, d)), f(p)))
                        .collect()
                },
                ..layer.clone()
            })
            .collect();
    }
}
