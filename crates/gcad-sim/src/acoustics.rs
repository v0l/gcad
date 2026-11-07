use super::skin::{self, Patch};
use super::statics::{FieldView, Mark, Stimulus};
use super::{Curve, Outcome, Reading, Series, Spec};
use anyhow::{Result, bail};
use monstertruck::modeling::*;
use sims_acoustics::{Air, BANDS, Rays, Room, Surface, trace};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Absorb {
    pub label: String,
    pub faces: Vec<usize>,
    pub coefficients: [f64; BANDS.len()],
    pub scattering: Option<f64>,
}

pub const SCATTERING: f64 = 0.2;
pub const RAYS: usize = 20_000;

pub const STOCK: &[(&str, [f64; BANDS.len()])] = &[
    ("concrete", [0.01, 0.01, 0.02, 0.02, 0.02, 0.03, 0.03]),
    ("brick", [0.03, 0.03, 0.03, 0.04, 0.05, 0.07, 0.07]),
    ("plaster", [0.013, 0.015, 0.02, 0.03, 0.04, 0.05, 0.05]),
    ("plasterboard", [0.29, 0.10, 0.05, 0.04, 0.07, 0.09, 0.09]),
    ("glass", [0.35, 0.25, 0.18, 0.12, 0.07, 0.04, 0.04]),
    ("wood", [0.15, 0.11, 0.10, 0.07, 0.06, 0.07, 0.07]),
    ("panelling", [0.42, 0.21, 0.10, 0.08, 0.06, 0.06, 0.06]),
    ("marble", [0.01, 0.01, 0.01, 0.01, 0.02, 0.02, 0.02]),
    ("carpet", [0.02, 0.06, 0.14, 0.37, 0.60, 0.65, 0.65]),
    ("curtain", [0.07, 0.31, 0.49, 0.75, 0.70, 0.60, 0.60]),
    ("ceiling-tile", [0.50, 0.70, 0.60, 0.70, 0.70, 0.50, 0.50]),
    ("audience", [0.39, 0.57, 0.80, 0.94, 0.92, 0.87, 0.87]),
];

pub fn stock(name: &str) -> Option<[f64; BANDS.len()]> {
    let name = name.to_ascii_lowercase();
    STOCK.iter().find(|(n, _)| *n == name).map(|(_, a)| *a)
}

pub fn stock_names() -> String {
    STOCK.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
}

struct Setup {
    room: Room,
    patches: Vec<Patch>,
    source: [f64; 3],
    receivers: Vec<(String, [f64; 3])>,
    rays: Rays,
    absorption: Vec<[f64; BANDS.len()]>,
    obstacles: Vec<String>,
    shell: String,
}

fn metres(p: Point3) -> [f64; 3] {
    [p.x * 1.0e-3, p.y * 1.0e-3, p.z * 1.0e-3]
}

fn air(spec: &Spec) -> Air {
    Air {
        celsius: spec.ambient.unwrap_or(20.0),
        humidity: spec.humidity.unwrap_or(50.0),
        ..Air::default()
    }
}

fn setup(spec: &Spec) -> Result<Setup> {
    let patches = skin::patches(spec, 0.0)?;
    let mut absorption: Vec<Option<(f64, [f64; BANDS.len()])>> = vec![None; patches.len()];
    for rule in &spec.absorb {
        for tag in &rule.faces {
            if let Some(at) = patches.iter().position(|p| p.tag == *tag) {
                absorption[at] = Some((rule.scattering.unwrap_or(SCATTERING), rule.coefficients));
            }
        }
    }
    let bare: Vec<String> = patches
        .iter()
        .zip(&absorption)
        .filter(|(_, a)| a.is_none())
        .map(|(p, _)| p.body.clone())
        .collect();
    if !bare.is_empty() {
        let mut names = bare.clone();
        names.dedup();
        bail!(
            "{} face(s) of {} have no absorption; start with `absorb all concrete` and override surfaces after it (materials: {})",
            bare.len(),
            names
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", "),
            stock_names()
        );
    }
    let mut bodies: Vec<String> = patches.iter().map(|p| p.body.clone()).collect();
    bodies.dedup();
    let volumes: Vec<(String, f64)> = bodies
        .iter()
        .map(|b| {
            let own: Vec<&Patch> = patches.iter().filter(|p| &p.body == b).collect();
            (b.clone(), skin::volume(&own))
        })
        .collect();
    let (shell, biggest) = volumes
        .iter()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .cloned()
        .expect("a body");
    if biggest.abs() < 1.0e-6 {
        bail!("the room has no volume; model the air inside the room as a closed solid");
    }
    let mut obstacles = Vec::new();
    let mut surfaces = Vec::new();
    let mut absorbed = Vec::new();
    for (patch, found) in patches.iter().zip(&absorption) {
        let (scattering, coefficients) = found.expect("checked above");
        let own = volumes
            .iter()
            .find(|(b, _)| *b == patch.body)
            .map_or(0.0, |v| v.1);
        let flip = patch.body != shell && own * biggest > 0.0;
        if flip && !obstacles.contains(&patch.body) {
            obstacles.push(patch.body.clone());
        }
        let triangles = patch
            .triangles
            .iter()
            .map(|t| {
                let mut p = t.map(|v| metres(patch.points[v]));
                if flip {
                    p.swap(1, 2);
                }
                p
            })
            .collect();
        surfaces.push(Surface {
            triangles,
            absorption: coefficients,
            scattering,
        });
        absorbed.push(coefficients);
    }
    let room = Room {
        surfaces,
        air: air(spec),
        air_absorption: true,
    };
    let Some(speaker) = spec.speaker else {
        bail!("place the sound source with `speaker x,y,z`");
    };
    if spec.probes.is_empty() {
        bail!("place a listener with `hall: probe x,y,z`");
    }
    let receivers: Vec<(String, [f64; 3])> = spec
        .probes
        .iter()
        .map(|(n, p)| (n.clone(), metres(*p)))
        .collect();
    let longest = (0..BANDS.len())
        .map(|b| room.eyring(b))
        .filter(|t| t.is_finite())
        .fold(0.0, f64::max);
    let bounds: BoundingBox<Point3> = patches
        .iter()
        .flat_map(|p| p.points.iter().copied())
        .collect();
    let smallest = {
        let d = bounds.diagonal();
        d.x.min(d.y).min(d.z) * 1.0e-3
    };
    let rays = Rays {
        count: spec.rays.unwrap_or(RAYS),
        seconds: (1.5 * longest).clamp(0.5, 8.0),
        radius: (smallest / 10.0).clamp(0.1, 0.5),
        ..Rays::default()
    };
    Ok(Setup {
        room,
        patches,
        source: metres(speaker),
        receivers,
        rays,
        absorption: absorbed,
        obstacles,
        shell,
    })
}

pub fn plan(spec: &Spec) -> Result<String> {
    let s = setup(spec)?;
    Ok(format!(
        "{} m3 of air inside `{}`, {} m2 of surface, {} obstacle(s)\n{} rays for {:.1} s, listener spheres {} m across, {} listener(s)\nair {} °C, {}% humidity, sound {:.0} m/s",
        crate::plot::label(s.room.volume()),
        s.shell,
        crate::plot::label(s.room.area()),
        s.obstacles.len(),
        s.rays.count,
        s.rays.seconds,
        crate::plot::label(s.rays.radius * 2.0),
        s.receivers.len(),
        crate::plot::label(s.room.air.celsius),
        crate::plot::label(s.room.air.humidity),
        s.room.air.speed()
    ))
}

fn reading(name: String, value: f64, unit: &str, at: &str) -> Reading {
    Reading {
        name,
        value,
        unit: unit.into(),
        at: at.into(),
    }
}

fn middle(values: [Option<f64>; 2]) -> Option<f64> {
    match values {
        [Some(a), Some(b)] => Some((a + b) / 2.0),
        _ => None,
    }
}

pub fn run(spec: &Spec) -> Result<Outcome> {
    let s = setup(spec)?;
    let points: Vec<[f64; 3]> = s.receivers.iter().map(|r| r.1).collect();
    let responses = trace(&s.room, s.source, &points, &s.rays).map_err(|e| anyhow::anyhow!(e))?;
    let mut readings = vec![
        reading(
            "volume".into(),
            s.room.volume(),
            "m³",
            "the air inside the room",
        ),
        reading(
            "surface".into(),
            s.room.area(),
            "m²",
            "every face, obstacles included",
        ),
    ];
    let sabine: Vec<f64> = (0..BANDS.len()).map(|b| s.room.sabine(b)).collect();
    let eyring: Vec<f64> = (0..BANDS.len()).map(|b| s.room.eyring(b)).collect();
    readings.push(reading(
        "sabine".into(),
        (sabine[2] + sabine[3]) / 2.0,
        "s",
        "reverberation time from the absorption area, 500 and 1000 Hz",
    ));
    readings.push(reading(
        "eyring".into(),
        (eyring[2] + eyring[3]) / 2.0,
        "s",
        "reverberation time for a diffuse room, 500 and 1000 Hz",
    ));
    let mut curves = vec![
        Curve {
            name: "sabine".into(),
            unit: "s".into(),
            values: sabine,
        },
        Curve {
            name: "eyring".into(),
            unit: "s".into(),
            values: eyring,
        },
    ];
    let mut notes = vec![format!(
        "{} rays, {} s long, listener spheres {} m across; air at {} °C and {}% humidity absorbs the treble",
        s.rays.count,
        crate::plot::label(s.rays.seconds),
        crate::plot::label(s.rays.radius * 2.0),
        crate::plot::label(s.room.air.celsius),
        crate::plot::label(s.room.air.humidity)
    )];
    let mut lost = Vec::new();
    for ((name, point), response) in s.receivers.iter().zip(&responses) {
        let at = format!(
            "{:.0},{:.0},{:.0}",
            point[0] * 1.0e3,
            point[1] * 1.0e3,
            point[2] * 1.0e3
        );
        let mid = |f: &dyn Fn(usize) -> Option<f64>| middle([f(2), f(3)]);
        let figures = [
            (
                "T30",
                "s",
                mid(&|b| response.t30(b)),
                "reverberation time over a 30 dB fit, 500 and 1000 Hz",
            ),
            (
                "T20",
                "s",
                mid(&|b| response.t20(b)),
                "reverberation time over a 20 dB fit, 500 and 1000 Hz",
            ),
            (
                "EDT",
                "s",
                mid(&|b| response.edt(b)),
                "early decay time, 500 and 1000 Hz",
            ),
            (
                "C80",
                "dB",
                mid(&|b| Some(response.clarity(b, 80.0))),
                "clarity for music, 500 and 1000 Hz",
            ),
            (
                "D50",
                "",
                mid(&|b| Some(response.definition(b))),
                "definition, the early share of the sound, 500 and 1000 Hz",
            ),
            (
                "STI",
                "",
                Some(response.sti()),
                "speech transmission index with no background noise",
            ),
        ];
        for (figure, unit, value, what) in figures {
            match value.filter(|v| v.is_finite()) {
                Some(value) => readings.push(reading(
                    format!("{name}.{figure}"),
                    value,
                    unit,
                    &format!("{what}, at {at}"),
                )),
                None => lost.push(format!("{name}.{figure}")),
            }
        }
        for (figure, unit, f) in [
            (
                "T30",
                "s",
                &(|b: usize| response.t30(b)) as &dyn Fn(usize) -> Option<f64>,
            ),
            ("EDT", "s", &|b: usize| response.edt(b)),
        ] {
            curves.push(Curve {
                name: format!("{name}.{figure}"),
                unit: unit.into(),
                values: (0..BANDS.len())
                    .map(|b| f(b).filter(|v| v.is_finite()).unwrap_or(0.0))
                    .collect(),
            });
        }
    }
    if !lost.is_empty() {
        notes.push(format!(
            "no reading for {}: the decay did not fall far enough in {} s, or too few rays reached the listener; raise `rays`",
            lost.join(", "),
            crate::plot::label(s.rays.seconds)
        ));
    }
    if !s.obstacles.is_empty() {
        notes.push(format!(
            "{} sit inside `{}` and count as obstacles",
            s.obstacles
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", "),
            s.shell
        ));
    }
    notes.push(
        "geometric acoustics: rays carry energy, so it says nothing about modes or diffraction below a few hundred hertz"
            .into(),
    );
    let _ = &s.absorption;
    Ok(Outcome {
        kind: "acoustics".into(),
        readings,
        notes,
        series: vec![Series {
            title: "reverberation time by octave band".into(),
            axis: "Hz".into(),
            x: BANDS.to_vec(),
            curves,
        }],
        ..Default::default()
    })
}

pub fn field_view(spec: &Spec) -> Result<FieldView> {
    let s = setup(spec)?;
    let mut positions: Vec<Point3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut edges: Vec<[u32; 3]> = Vec::new();
    let mut values: Vec<f64> = Vec::new();
    for (patch, coefficients) in s.patches.iter().zip(&s.absorption) {
        let base = positions.len();
        positions.extend(patch.points.iter().copied());
        values.extend(std::iter::repeat_n(coefficients[3], patch.points.len()));
        triangles.extend(patch.triangles.iter().map(|t| t.map(|v| (base + v) as u32)));
        edges.extend(
            super::wind::creases(&patch.points, &patch.triangles)
                .into_iter()
                .map(|[a, b]| [a, a, b].map(|v| (base + v) as u32)),
        );
    }
    let mut marks = Vec::new();
    let mut cross = |at: [f64; 3], text: String, kind: Stimulus| {
        let at = Point3::new(at[0] * 1.0e3, at[1] * 1.0e3, at[2] * 1.0e3);
        let reach = 150.0;
        let base = positions.len() as u32;
        for axis in 0..3 {
            let mut d = Vector3::zero();
            d[axis] = reach;
            positions.push(at - d);
            positions.push(at + d);
            values.extend([0.0, 0.0]);
        }
        let outline: Vec<[u32; 3]> = (0..3)
            .map(|k| [base + 2 * k, base + 2 * k, base + 2 * k + 1])
            .collect();
        marks.push(Mark {
            kind,
            text,
            nodes: Vec::new(),
            outline: outline.clone(),
            spot: Some(base + 1),
            at,
            toward: None,
        });
        edges.extend(outline);
    };
    cross(s.source, "speaker".into(), Stimulus::Push);
    for (name, at) in &s.receivers {
        cross(*at, name.clone(), Stimulus::Hold);
    }
    let shade: Vec<f64> = values.iter().map(|v| v.clamp(0.0, 1.0)).collect();
    Ok(FieldView {
        shade,
        triangles,
        edges,
        low: 0.0,
        high: 1.0,
        scale: 0.0,
        title: "absorption at 1 kHz".into(),
        unit: "α".into(),
        moved: vec![Vector3::zero(); positions.len()],
        rest: positions.clone(),
        values,
        positions,
        marks,
        index: Default::default(),
        slices: Vec::new(),
        streams: Vec::new(),
        travel: Default::default(),
        movie: None,
    })
}
