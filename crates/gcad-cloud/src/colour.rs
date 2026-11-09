use crate::cloud::Cloud;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColourBy {
    #[default]
    Rgb,
    Intensity,
    Height,
    Normal,
    Class,
}

impl ColourBy {
    pub const ALL: [ColourBy; 5] = [
        ColourBy::Rgb,
        ColourBy::Intensity,
        ColourBy::Height,
        ColourBy::Normal,
        ColourBy::Class,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ColourBy::Rgb => "rgb",
            ColourBy::Intensity => "intensity",
            ColourBy::Height => "height",
            ColourBy::Normal => "normal",
            ColourBy::Class => "class",
        }
    }

    pub fn parse(text: &str) -> Option<ColourBy> {
        ColourBy::ALL.into_iter().find(|m| m.name() == text)
    }

    pub fn available(self, cloud: &Cloud) -> bool {
        match self {
            ColourBy::Rgb => cloud.colours.is_some(),
            ColourBy::Intensity => cloud.intensity.is_some(),
            ColourBy::Height => true,
            ColourBy::Normal => cloud.normals.is_some(),
            ColourBy::Class => cloud.classes.is_some(),
        }
    }
}

const GREY: [u8; 3] = [170, 170, 170];

const RAMP: [[f32; 3]; 5] = [
    [0.05, 0.15, 0.65],
    [0.0, 0.7, 0.9],
    [0.2, 0.8, 0.3],
    [0.95, 0.85, 0.15],
    [0.85, 0.15, 0.1],
];

pub fn ramp(t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0) * (RAMP.len() - 1) as f32;
    let at = (t.floor() as usize).min(RAMP.len() - 2);
    let f = t - at as f32;
    std::array::from_fn(|k| ((RAMP[at][k] * (1.0 - f) + RAMP[at + 1][k] * f) * 255.0).round() as u8)
}

pub fn class_colour(class: u8) -> [u8; 3] {
    match class {
        0 => [120, 120, 120],
        1 => [190, 190, 190],
        2 => [150, 100, 50],
        3 => [140, 220, 90],
        4 => [60, 180, 60],
        5 => [20, 120, 30],
        6 => [220, 60, 50],
        7 => [255, 0, 255],
        9 => [40, 110, 230],
        10 => [90, 90, 100],
        11 => [60, 60, 60],
        17 => [240, 170, 40],
        18 => [255, 255, 0],
        other => {
            let h = (other as u32).wrapping_mul(2654435761);
            [(h >> 8) as u8 | 64, (h >> 16) as u8 | 64, (h >> 24) as u8 | 64]
        }
    }
}

pub fn colours(cloud: &Cloud, by: ColourBy, indices: &[usize]) -> Vec<[u8; 3]> {
    let by = if by.available(cloud) { by } else { ColourBy::Height };
    match by {
        ColourBy::Rgb => {
            let c = cloud.colours.as_deref().unwrap_or(&[]);
            indices.iter().map(|&i| c.get(i).copied().unwrap_or(GREY)).collect()
        }
        ColourBy::Intensity => {
            let c = cloud.intensity.as_deref().unwrap_or(&[]);
            let (lo, hi) = c
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
            let span = (hi - lo).max(f32::EPSILON);
            indices
                .iter()
                .map(|&i| {
                    let t = ((c[i] - lo) / span).clamp(0.0, 1.0);
                    let g = (40.0 + 215.0 * t) as u8;
                    [g, g, g]
                })
                .collect()
        }
        ColourBy::Height => {
            let (lo, hi) = cloud.bounds().map_or((0.0, 1.0), |(lo, hi)| (lo[2], hi[2]));
            let span = (hi - lo).max(f64::EPSILON);
            indices
                .iter()
                .map(|&i| ramp(((cloud.positions[i][2] - lo) / span) as f32))
                .collect()
        }
        ColourBy::Normal => {
            let n = cloud.normals.as_deref().unwrap_or(&[]);
            indices
                .iter()
                .map(|&i| n[i].map(|v| ((v * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8))
                .collect()
        }
        ColourBy::Class => {
            let c = cloud.classes.as_deref().unwrap_or(&[]);
            indices.iter().map(|&i| class_colour(c[i])).collect()
        }
    }
}
