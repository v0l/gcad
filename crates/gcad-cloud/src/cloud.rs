use gcad_parse::label;

pub type P3 = [f64; 3];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cloud {
    pub positions: Vec<P3>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub colours: Option<Vec<[u8; 3]>>,
    pub intensity: Option<Vec<f32>>,
    pub classes: Option<Vec<u8>>,
    pub scans: Vec<u16>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub position: P3,
    pub normal: Option<[f32; 3]>,
    pub colour: Option<[u8; 3]>,
    pub intensity: Option<f32>,
    pub class: Option<u8>,
    pub scan: u16,
}

impl Point {
    pub fn at(position: P3) -> Point {
        Point {
            position,
            ..Point::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub normals: bool,
    pub colours: bool,
    pub intensity: bool,
    pub classes: bool,
}

const FILL_COLOUR: [u8; 3] = [180, 180, 180];

impl Cloud {
    pub fn with_layout(layout: Layout, capacity: usize) -> Cloud {
        fn column<T>(on: bool, capacity: usize) -> Option<Vec<T>> {
            on.then(|| Vec::with_capacity(capacity))
        }
        Cloud {
            positions: Vec::with_capacity(capacity),
            normals: column(layout.normals, capacity),
            colours: column(layout.colours, capacity),
            intensity: column(layout.intensity, capacity),
            classes: column(layout.classes, capacity),
            scans: Vec::with_capacity(capacity),
        }
    }

    pub fn from_positions(positions: Vec<P3>) -> Cloud {
        let scans = vec![0; positions.len()];
        Cloud {
            positions,
            scans,
            ..Cloud::default()
        }
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn layout(&self) -> Layout {
        Layout {
            normals: self.normals.is_some(),
            colours: self.colours.is_some(),
            intensity: self.intensity.is_some(),
            classes: self.classes.is_some(),
        }
    }

    pub fn push(&mut self, point: Point) {
        self.positions.push(point.position);
        self.scans.push(point.scan);
        if let Some(column) = &mut self.normals {
            column.push(point.normal.unwrap_or([0.0; 3]));
        }
        if let Some(column) = &mut self.colours {
            column.push(point.colour.unwrap_or(FILL_COLOUR));
        }
        if let Some(column) = &mut self.intensity {
            column.push(point.intensity.unwrap_or(0.0));
        }
        if let Some(column) = &mut self.classes {
            column.push(point.class.unwrap_or(0));
        }
    }

    pub fn set(&mut self, index: usize, point: Point) {
        self.positions[index] = point.position;
        self.scans[index] = point.scan;
        if let Some(column) = &mut self.normals {
            column[index] = point.normal.unwrap_or([0.0; 3]);
        }
        if let Some(column) = &mut self.colours {
            column[index] = point.colour.unwrap_or(FILL_COLOUR);
        }
        if let Some(column) = &mut self.intensity {
            column[index] = point.intensity.unwrap_or(0.0);
        }
        if let Some(column) = &mut self.classes {
            column[index] = point.class.unwrap_or(0);
        }
    }

    pub fn point(&self, index: usize) -> Point {
        Point {
            position: self.positions[index],
            normal: self.normals.as_ref().map(|c| c[index]),
            colour: self.colours.as_ref().map(|c| c[index]),
            intensity: self.intensity.as_ref().map(|c| c[index]),
            class: self.classes.as_ref().map(|c| c[index]),
            scan: self.scans[index],
        }
    }

    pub fn bounds(&self) -> Option<(P3, P3)> {
        let first = *self.positions.first()?;
        Some(
            self.positions
                .iter()
                .fold((first, first), |(lo, hi), p| {
                    (
                        std::array::from_fn(|k| lo[k].min(p[k])),
                        std::array::from_fn(|k| hi[k].max(p[k])),
                    )
                }),
        )
    }

    pub fn memory_bytes(&self) -> usize {
        let n = self.len();
        n * (24 + 2)
            + self.normals.as_ref().map_or(0, |_| n * 12)
            + self.colours.as_ref().map_or(0, |_| n * 3)
            + self.intensity.as_ref().map_or(0, |_| n * 4)
            + self.classes.as_ref().map_or(0, |_| n)
    }

    pub fn merge(&mut self, other: &Cloud) {
        let (a, b) = (self.layout(), other.layout());
        let union = Layout {
            normals: a.normals || b.normals,
            colours: a.colours || b.colours,
            intensity: a.intensity || b.intensity,
            classes: a.classes || b.classes,
        };
        let mut merged = Cloud::with_layout(union, self.len() + other.len());
        (0..self.len()).for_each(|i| merged.push(self.point(i)));
        (0..other.len()).for_each(|i| merged.push(other.point(i)));
        *self = merged;
    }

    pub fn select(&self, indices: &[usize]) -> Cloud {
        let mut out = Cloud::with_layout(self.layout(), indices.len());
        indices.iter().for_each(|&i| out.push(self.point(i)));
        out
    }

    pub fn scan_ids(&self) -> Vec<u16> {
        let mut ids = self.scans.clone();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    pub fn summary(&self) -> String {
        let count = format!("{} points", self.len());
        let Some((lo, hi)) = self.bounds() else {
            return count;
        };
        let at = |p: P3| format!("{},{},{}", label(p[0]), label(p[1]), label(p[2]));
        let mut fields = Vec::new();
        if self.normals.is_some() {
            fields.push("normals");
        }
        if self.colours.is_some() {
            fields.push("colour");
        }
        if self.intensity.is_some() {
            fields.push("intensity");
        }
        if self.classes.is_some() {
            fields.push("classes");
        }
        let has = if fields.is_empty() {
            String::new()
        } else {
            format!(", {}", fields.join(" "))
        };
        format!(
            "{count}, bounds {} to {}, {} scan(s), {}{has}",
            at(lo),
            at(hi),
            self.scan_ids().len(),
            memory(self.memory_bytes())
        )
    }
}

pub fn memory(bytes: usize) -> String {
    let b = bytes as f64;
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", b / 1024.0),
        1_048_576..1_073_741_824 => format!("{:.1} MB", b / 1_048_576.0),
        _ => format!("{:.2} GB", b / 1_073_741_824.0),
    }
}
