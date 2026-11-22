use super::Report;
use super::fit;
use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use std::sync::OnceLock;

const TABLE: &str = include_str!("data/bearings.toml");

pub const SOURCES: [&str; 5] = [
    "ISO 15 boundary dimensions; C and C0 from the SKF catalogue values at https://mechcodex.com/reference/ball-bearing-dimensions",
    "speeds, corner radii, thrust and linear bearings: https://iskbearing.com/product-details/6205 (same address for each number)",
    "miniature bearings: https://tflbearing.com/products/ball-bearings/miniature-deep-groove-ball-bearings-6 and https://product.minebeamitsumi.com/en/product/category/bearing/miniature_small/parts/608.html",
    "shoulders: SKF General Catalogue 17000/1 EN abutment and fillet dimensions, https://www.skf.com/binaries/pub12/Images/0901d196802809de-Rolling-bearings---17000_1-EN_tcm_12-121486.pdf; estimates from DIN 5418:1993 table 1, https://www.normsplash.com/Samples/DIN/148879982/DIN-5418-1993-en.pdf",
    "life: ISO 281 basic rating life, L10 = (C / P)^3 million revolutions for ball bearings",
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Radial,
    Flanged,
    Thrust,
    Linear,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Radial => "deep groove ball bearing",
            Kind::Flanged => "flanged deep groove ball bearing",
            Kind::Thrust => "single direction thrust ball bearing",
            Kind::Linear => "linear ball bushing",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Bearing {
    pub name: String,
    pub kind: Kind,
    pub bore: f64,
    pub outer: f64,
    pub width: f64,
    pub dynamic_kn: f64,
    pub static_kn: f64,
    pub speed: f64,
    pub corner: f64,
    pub flange_dia: f64,
    pub flange_width: f64,
    light_series: bool,
}

#[derive(Deserialize)]
struct Row(String, f64, f64, f64, f64, f64, f64, f64, u32, f64, f64);

#[derive(Deserialize)]
struct File {
    radial: Vec<Row>,
    flanged: Vec<Row>,
    thrust: Vec<Row>,
    linear: Vec<Row>,
    shoulders: Vec<[f64; 4]>,
    abutment: Vec<(String, f64, f64, f64)>,
    f0: f64,
    radial_x: f64,
    load_factors: Vec<[f64; 3]>,
}

fn file() -> &'static File {
    static FILE: OnceLock<File> = OnceLock::new();
    FILE.get_or_init(|| {
        toml::from_str(TABLE).unwrap_or_else(|e| panic!("calc table bearings: {e}"))
    })
}

pub fn all() -> &'static [Bearing] {
    static ALL: OnceLock<Vec<Bearing>> = OnceLock::new();
    ALL.get_or_init(|| {
        let f = file();
        let make = |rows: &[Row], kind: Kind| -> Vec<Bearing> {
            rows.iter()
                .map(|r| Bearing {
                    name: r.0.clone(),
                    kind,
                    bore: r.1,
                    outer: r.2,
                    width: r.3,
                    dynamic_kn: r.4,
                    static_kn: r.5,
                    speed: r.6,
                    corner: r.7,
                    light_series: r.8 == 890,
                    flange_dia: r.9,
                    flange_width: r.10,
                })
                .collect()
        };
        let mut out = make(&f.radial, Kind::Radial);
        out.extend(make(&f.flanged, Kind::Flanged));
        out.extend(make(&f.thrust, Kind::Thrust));
        out.extend(make(&f.linear, Kind::Linear));
        out
    })
}

pub fn find(name: &str) -> Result<&'static Bearing> {
    let wanted = name.trim().to_ascii_uppercase();
    all().iter().find(|b| b.name == wanted).ok_or_else(|| {
        anyhow!(
            "unknown bearing `{name}`; known: {}",
            all()
                .iter()
                .map(|b| b.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shoulders {
    pub shaft_min: f64,
    pub housing_max: f64,
    pub fillet_max: f64,
    pub estimated: bool,
}

pub fn shoulders(b: &Bearing) -> Shoulders {
    let catalogue = if matches!(b.kind, Kind::Radial | Kind::Flanged) {
        let plain = b.name.trim_start_matches('F');
        file().abutment.iter().find(|r| r.0 == plain)
    } else {
        None
    };
    if let Some(r) = catalogue {
        return Shoulders {
            shaft_min: r.1,
            housing_max: r.2,
            fillet_max: r.3,
            estimated: false,
        };
    }
    let row = file()
        .shoulders
        .iter()
        .find(|r| r[0] >= b.corner - 1.0e-9)
        .unwrap_or_else(|| file().shoulders.last().expect("shoulder rows"));
    let height = if b.light_series { row[2] } else { row[3] };
    Shoulders {
        shaft_min: b.bore + 2.0 * height,
        housing_max: b.outer - 2.0 * height,
        fillet_max: row[1],
        estimated: true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Equivalent {
    pub load_kn: f64,
    pub factor_y: f64,
    pub limit_e: f64,
    pub axial_matters: bool,
}

pub fn equivalent_load(b: &Bearing, radial_kn: f64, axial_kn: f64) -> Equivalent {
    if b.kind == Kind::Thrust {
        return Equivalent {
            load_kn: axial_kn,
            factor_y: 1.0,
            limit_e: 0.0,
            axial_matters: true,
        };
    }
    if b.kind == Kind::Linear || axial_kn <= 0.0 {
        return Equivalent {
            load_kn: radial_kn,
            factor_y: 0.0,
            limit_e: 0.0,
            axial_matters: false,
        };
    }
    let f = file();
    let x = f.f0 * axial_kn / b.static_kn;
    let t = &f.load_factors;
    let at = t.iter().position(|r| x <= r[0]);
    let (e, y) = match at {
        None => (t[t.len() - 1][1], t[t.len() - 1][2]),
        Some(0) => (t[0][1], t[0][2]),
        Some(i) => {
            let (a, c) = (t[i - 1], t[i]);
            let s = (x - a[0]) / (c[0] - a[0]);
            (a[1] + s * (c[1] - a[1]), a[2] + s * (c[2] - a[2]))
        }
    };
    let axial_matters = radial_kn <= 0.0 || axial_kn / radial_kn > e;
    Equivalent {
        load_kn: if axial_matters {
            f.radial_x * radial_kn + y * axial_kn
        } else {
            radial_kn
        },
        factor_y: y,
        limit_e: e,
        axial_matters,
    }
}

pub fn life_million_revs(b: &Bearing, load_kn: f64) -> f64 {
    (b.dynamic_kn / load_kn).powi(3)
}

pub fn life_hours(million_revs: f64, rpm: f64) -> f64 {
    million_revs * 1.0e6 / (60.0 * rpm)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Load {
    Light,
    Normal,
    Heavy,
}

impl std::str::FromStr for Load {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "light" => Ok(Load::Light),
            "normal" => Ok(Load::Normal),
            "heavy" => Ok(Load::Heavy),
            _ => Err("load is light, normal or heavy".into()),
        }
    }
}

pub fn load_class(load_kn: f64, b: &Bearing) -> Load {
    let ratio = load_kn / b.dynamic_kn;
    if ratio <= 0.05 {
        Load::Light
    } else if ratio <= 0.10 {
        Load::Normal
    } else {
        Load::Heavy
    }
}

pub fn fit_classes(b: &Bearing, load: Load) -> (&'static str, &'static str) {
    match b.kind {
        Kind::Linear => ("h6", "H7"),
        Kind::Thrust => ("j6", "H8"),
        _ if b.bore <= 10.0 => ("js5", "H7"),
        _ => (
            match load {
                Load::Light => "js5",
                Load::Normal => "k5",
                Load::Heavy => "m5",
            },
            "H7",
        ),
    }
}

pub struct Input {
    pub bearing: &'static Bearing,
    pub radial_kn: f64,
    pub axial_kn: f64,
    pub rpm: Option<f64>,
    pub load: Option<Load>,
}

fn limits_text(class: &str, nominal: f64) -> Result<String> {
    let l = fit::limits(class, nominal)?;
    Ok(format!("{class}: {:.3} to {:.3} mm", l.min, l.max))
}

pub fn report(input: &Input) -> Result<Report> {
    let b = input.bearing;
    if input.radial_kn < 0.0 || input.axial_kn < 0.0 {
        bail!("loads are positive numbers in kN");
    }
    if let Some(rpm) = input.rpm
        && rpm <= 0.0
    {
        bail!("speed is above zero rpm");
    }
    let size = match b.kind {
        Kind::Thrust => format!("{} x {} x {} mm (bore, outer, height)", b.bore, b.outer, b.width),
        Kind::Linear => format!("{} x {} x {} mm (shaft, outer, length)", b.bore, b.outer, b.width),
        _ => format!("{} x {} x {} mm (bore, outer, width)", b.bore, b.outer, b.width),
    };
    let mut rows = vec![
        ("bearing".into(), format!("{} {}", b.name, b.kind.name())),
        ("size".into(), size),
        ("dynamic C".into(), format!("{} kN", b.dynamic_kn)),
        ("static C0".into(), format!("{} kN", b.static_kn)),
    ];
    if b.kind == Kind::Flanged {
        rows.push((
            "flange".into(),
            format!("{} mm across, {} thick", b.flange_dia, b.flange_width),
        ));
    }
    if b.speed > 0.0 {
        rows.push(("speed limit".into(), format!("{} rpm (grease)", b.speed)));
    } else {
        rows.push(("speed limit".into(), "not published for this size".into()));
    }
    let load_kn = if b.kind == Kind::Thrust && input.axial_kn <= 0.0 {
        input.radial_kn
    } else {
        equivalent_load(b, input.radial_kn, input.axial_kn).load_kn
    };
    let class = input.load.unwrap_or_else(|| {
        if load_kn > 0.0 {
            load_class(load_kn, b)
        } else {
            Load::Normal
        }
    });
    let (shaft, housing) = fit_classes(b, class);
    match b.kind {
        Kind::Thrust => {
            rows.push((
                "shaft washer".into(),
                limits_text(shaft, b.bore)?,
            ));
            rows.push(("housing washer".into(), limits_text(housing, b.outer)?));
        }
        Kind::Linear => {
            rows.push(("shaft".into(), limits_text(shaft, b.bore)?));
            rows.push(("housing".into(), limits_text(housing, b.outer)?));
        }
        _ => {
            rows.push(("shaft".into(), limits_text(shaft, b.bore)?));
            rows.push(("housing".into(), limits_text(housing, b.outer)?));
            let s = shoulders(b);
            let tag = if s.estimated { " (estimated)" } else { "" };
            rows.push((
                format!("shaft shoulder{tag}"),
                format!("at least {:.1} mm across, fillet up to {} mm", s.shaft_min, s.fillet_max),
            ));
            rows.push((
                format!("housing shoulder{tag}"),
                format!("at most {:.1} mm across, fillet up to {} mm", s.housing_max, s.fillet_max),
            ));
        }
    }
    let mut why = format!(
        "A {} carries {} kN dynamic and {} kN static.",
        b.name, b.dynamic_kn, b.static_kn
    );
    if load_kn > 0.0 {
        let eq = equivalent_load(b, input.radial_kn, input.axial_kn);
        rows.push(("equivalent load P".into(), format!("{:.3} kN", load_kn)));
        let revs = life_million_revs(b, load_kn);
        rows.push(("L10 life".into(), format!("{:.1} million revolutions", revs)));
        rows.push((
            "static safety C0/P".into(),
            format!("{:.1}", b.static_kn / load_kn),
        ));
        if let Some(rpm) = input.rpm {
            let hours = life_hours(revs, rpm);
            rows.push(("L10h life".into(), format!("{:.0} hours at {} rpm", hours, rpm)));
            if b.speed > 0.0 && rpm > b.speed {
                rows.push((
                    "warning".into(),
                    format!("{rpm} rpm is over the {} rpm limit", b.speed),
                ));
            }
        }
        why = format!(
            "At P = {:.3} kN (C/P = {:.1}), 9 in 10 bearings last {:.1} million revolutions{}.",
            load_kn,
            b.dynamic_kn / load_kn,
            revs,
            if eq.axial_matters && b.kind != Kind::Thrust && input.axial_kn > 0.0 {
                ", with the axial load counted"
            } else {
                ""
            }
        );
    }
    Ok(Report {
        title: format!("bearing {}", b.name),
        rows,
        why,
        formulas: vec![
            "P = Fr when Fa/Fr <= e, else P = 0.56 Fr + Y Fa; e and Y from f0 Fa / C0 (f0 = 14)".into(),
            "L10 = (C / P)^3 million revolutions; L10h = L10 x 1e6 / (60 n)".into(),
            "shoulders: da min, Da max and ra max are SKF catalogue values where the row says so; rows marked (estimated) use d + 2 h and D - 2 h with h from DIN 5418 for the corner radius".into(),
            "shaft js5 up to 10 mm bore, else js5 light, k5 normal, m5 heavy (P/C up to 0.05, 0.10, above); housing H7".into(),
        ],
        sources: SOURCES.iter().map(|s| s.to_string()).collect(),
    })
}
