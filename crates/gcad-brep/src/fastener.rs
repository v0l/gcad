use anyhow::{Result, anyhow, bail};

#[derive(Clone, Copy, Debug)]
pub struct Metric {
    pub name: &'static str,
    pub major: f64,
    pub tap: f64,
    pub pitch: f64,
    pub clearance: [f64; 3],
    pub socket_head: (f64, f64),
    pub socket_key: (f64, f64),
    pub flat_key: (f64, f64),
    pub counterbore: f64,
    pub flat_head: f64,
    pub hex_head: (f64, f64),
    pub nut: (f64, f64),
    pub washer: (f64, f64),
    pub insert: Option<(f64, f64, f64)>,
}

const fn metric(
    name: &'static str,
    [major, tap, pitch]: [f64; 3],
    clearance: [f64; 3],
    [head_d, key_s, key_t, counterbore, flat_head]: [f64; 5],
    [bolt_k, nut_s, nut_m, washer_d, washer_h]: [f64; 5],
    insert: Option<(f64, f64, f64)>,
    flat_key: (f64, f64),
) -> Metric {
    Metric {
        name,
        major,
        tap,
        pitch,
        clearance,
        socket_head: (head_d, major),
        socket_key: (key_s, key_t),
        flat_key,
        counterbore,
        flat_head,
        hex_head: (nut_s, bolt_k),
        nut: (nut_s, nut_m),
        washer: (washer_d, washer_h),
        insert,
    }
}

pub const METRIC: &[Metric] = &[
    metric(
        "M2",
        [2.0, 1.6, 0.4],
        [2.2, 2.4, 2.6],
        [3.8, 1.5, 1.0, 4.3, 4.4],
        [1.4, 4.0, 1.6, 5.0, 0.3],
        Some((3.2, 4.0, 3.6)),
        (1.3, 0.8),
    ),
    metric(
        "M2.5",
        [2.5, 2.05, 0.45],
        [2.7, 2.9, 3.1],
        [4.5, 2.0, 1.1, 5.0, 5.5],
        [1.7, 5.0, 2.0, 6.0, 0.5],
        None,
        (1.5, 1.0),
    ),
    metric(
        "M3",
        [3.0, 2.5, 0.5],
        [3.2, 3.4, 3.6],
        [5.5, 2.5, 1.3, 6.0, 6.72],
        [2.0, 5.5, 2.4, 7.0, 0.5],
        Some((4.0, 5.7, 4.6)),
        (2.0, 1.1),
    ),
    metric(
        "M4",
        [4.0, 3.3, 0.7],
        [4.3, 4.5, 4.8],
        [7.0, 3.0, 2.0, 8.0, 8.96],
        [2.8, 7.0, 3.2, 9.0, 0.8],
        Some((5.6, 8.1, 6.3)),
        (2.5, 1.5),
    ),
    metric(
        "M5",
        [5.0, 4.2, 0.8],
        [5.3, 5.5, 5.8],
        [8.5, 4.0, 2.5, 10.0, 11.2],
        [3.5, 8.0, 4.7, 10.0, 1.0],
        Some((6.4, 9.5, 7.1)),
        (3.0, 1.9),
    ),
    metric(
        "M6",
        [6.0, 5.0, 1.0],
        [6.4, 6.6, 7.0],
        [10.0, 5.0, 3.0, 11.0, 13.44],
        [4.0, 10.0, 5.2, 12.0, 1.6],
        Some((8.0, 12.7, 8.7)),
        (4.0, 2.2),
    ),
    metric(
        "M8",
        [8.0, 6.8, 1.25],
        [8.4, 9.0, 10.0],
        [13.0, 6.0, 4.0, 15.0, 17.92],
        [5.3, 13.0, 6.8, 16.0, 1.6],
        None,
        (5.0, 3.0),
    ),
    metric(
        "M10",
        [10.0, 8.5, 1.5],
        [10.5, 11.0, 12.0],
        [16.0, 8.0, 5.0, 18.0, 22.4],
        [6.4, 16.0, 8.4, 20.0, 2.0],
        None,
        (6.0, 3.6),
    ),
    metric(
        "M12",
        [12.0, 10.2, 1.75],
        [13.0, 13.5, 14.5],
        [18.0, 10.0, 6.0, 20.0, 26.88],
        [7.5, 18.0, 10.8, 24.0, 2.5],
        None,
        (8.0, 4.3),
    ),
];

pub fn is_metric(text: &str) -> bool {
    text.len() > 1
        && text.as_bytes()[0].eq_ignore_ascii_case(&b'm')
        && text[1..].parse::<f64>().is_ok()
}

pub fn named(name: &str) -> Result<&'static Metric> {
    METRIC
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            anyhow!(
                "unknown size `{name}`; known: {}",
                METRIC.iter().map(|m| m.name).collect::<Vec<_>>().join(", ")
            )
        })
}

impl Metric {
    pub fn counterbore_depth(&self) -> f64 {
        self.socket_head.1 + 0.4
    }

    pub fn flat_head_height(&self) -> f64 {
        (self.flat_head - self.major) / 2.0
    }
}

const SIZE_STEPS: [f64; 13] = [
    3.0, 6.0, 10.0, 18.0, 30.0, 50.0, 80.0, 120.0, 180.0, 250.0, 315.0, 400.0, 500.0,
];

const GRADES: [(u32, [f64; 13]); 7] = [
    (
        5,
        [4., 5., 6., 8., 9., 11., 13., 15., 18., 20., 23., 25., 27.],
    ),
    (
        6,
        [6., 8., 9., 11., 13., 16., 19., 22., 25., 29., 32., 36., 40.],
    ),
    (
        7,
        [
            10., 12., 15., 18., 21., 25., 30., 35., 40., 46., 52., 57., 63.,
        ],
    ),
    (
        8,
        [
            14., 18., 22., 27., 33., 39., 46., 54., 63., 72., 81., 89., 97.,
        ],
    ),
    (
        9,
        [
            25., 30., 36., 43., 52., 62., 74., 87., 100., 115., 130., 140., 155.,
        ],
    ),
    (
        10,
        [
            40., 48., 58., 70., 84., 100., 120., 140., 160., 185., 210., 230., 250.,
        ],
    ),
    (
        11,
        [
            60., 75., 90., 110., 130., 160., 190., 220., 250., 290., 320., 360., 400.,
        ],
    ),
];

const LOWER: [(&str, [f64; 13]); 5] = [
    ("H", [0.; 13]),
    (
        "G",
        [2., 4., 5., 6., 7., 9., 10., 12., 14., 15., 17., 18., 20.],
    ),
    (
        "F",
        [
            6., 10., 13., 16., 20., 25., 30., 36., 43., 50., 56., 62., 68.,
        ],
    ),
    (
        "E",
        [
            14., 20., 25., 32., 40., 50., 60., 72., 85., 100., 110., 125., 135.,
        ],
    ),
    (
        "D",
        [
            20., 30., 40., 50., 65., 80., 100., 120., 145., 170., 190., 210., 230.,
        ],
    ),
];

pub fn hole_limits(diameter: f64, fit: &str) -> Result<(f64, f64)> {
    let split = fit
        .find(|c: char| c.is_ascii_digit())
        .ok_or_else(|| anyhow!("a fit is a letter and a grade, like H7; got `{fit}`"))?;
    let (letter, grade) = fit.split_at(split);
    let grade: u32 = grade
        .parse()
        .map_err(|_| anyhow!("`{fit}` has no grade number"))?;
    if diameter > 500.0 {
        bail!("fits go up to 500 mm, the hole is {diameter}");
    }
    let range = SIZE_STEPS
        .iter()
        .position(|&top| diameter <= top)
        .unwrap_or(SIZE_STEPS.len() - 1);
    let tolerance = GRADES
        .iter()
        .find(|(g, _)| *g == grade)
        .map(|(_, values)| values[range] / 1000.0)
        .ok_or_else(|| anyhow!("grade {grade} is not known; use 5 to 11"))?;
    let lower = match letter {
        "JS" | "Js" | "js" => -tolerance / 2.0,
        _ => LOWER
            .iter()
            .find(|(l, _)| *l == letter)
            .map(|(_, values)| values[range] / 1000.0)
            .ok_or_else(|| anyhow!("hole fit `{letter}` is not known; use H, G, F, E, D or JS"))?,
    };
    Ok((diameter + lower, diameter + lower + tolerance))
}
