use crate::cloud::{Cloud, Layout, Point};
use crate::sample::Collector;
use crate::{Imported, Options, Source};
use anyhow::{Result, bail};
use std::io::{BufRead, BufReader, Write};

fn words(line: &str) -> Vec<&str> {
    line.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .collect()
}

fn numbers(line: &str) -> Option<Vec<f64>> {
    words(line).iter().map(|w| w.parse().ok()).collect()
}

fn column_of(names: &[&str], wanted: &[&str]) -> Option<usize> {
    names
        .iter()
        .position(|n| wanted.contains(&n.to_ascii_lowercase().as_str()))
}

#[derive(Clone, Debug, Default)]
struct Columns {
    x: usize,
    y: usize,
    z: usize,
    normal: Option<usize>,
    colour: Option<usize>,
    intensity: Option<usize>,
    class: Option<usize>,
    width: usize,
}

fn named_columns(names: &[&str]) -> Option<Columns> {
    let (x, y, z) = (
        column_of(names, &["x"])?,
        column_of(names, &["y"])?,
        column_of(names, &["z"])?,
    );
    Some(Columns {
        x,
        y,
        z,
        normal: column_of(names, &["nx"]),
        colour: column_of(names, &["r", "red"]),
        intensity: column_of(names, &["intensity", "i"]),
        class: column_of(names, &["class", "classification"]),
        width: names.len(),
    })
}

fn positional_columns(width: usize, pts: bool) -> Result<Columns> {
    match (width, pts) {
        (0..=2, _) => bail!("a point line needs at least x, y and z, this one has {width} values"),
        (3, _) => Ok(Columns {
            x: 0,
            y: 1,
            z: 2,
            width,
            ..Columns::default()
        }),
        (4, true) => Ok(Columns {
            x: 0,
            y: 1,
            z: 2,
            intensity: Some(3),
            width,
            ..Columns::default()
        }),
        (6, false) => Ok(Columns {
            x: 0,
            y: 1,
            z: 2,
            colour: Some(3),
            width,
            ..Columns::default()
        }),
        (6, true) => Ok(Columns {
            x: 0,
            y: 1,
            z: 2,
            intensity: Some(3),
            colour: Some(4),
            width,
            ..Columns::default()
        }),
        (7, true) => Ok(Columns {
            x: 0,
            y: 1,
            z: 2,
            intensity: Some(3),
            colour: Some(4),
            width,
            ..Columns::default()
        }),
        _ => Ok(Columns {
            x: 0,
            y: 1,
            z: 2,
            normal: (width >= 6).then_some(3),
            width,
            ..Columns::default()
        }),
    }
}

fn point_of(values: &[f64], columns: &Columns, layout: Layout) -> Point {
    let mut point = Point::at([values[columns.x], values[columns.y], values[columns.z]]);
    if layout.normals
        && let Some(n) = columns.normal
    {
        point.normal = Some([values[n] as f32, values[n + 1] as f32, values[n + 2] as f32]);
    }
    if layout.colours
        && let Some(c) = columns.colour
    {
        point.colour = Some([values[c], values[c + 1], values[c + 2]].map(|v| v.round().clamp(0.0, 255.0) as u8));
    }
    if layout.intensity
        && let Some(i) = columns.intensity
    {
        point.intensity = Some(values[i] as f32);
    }
    if layout.classes
        && let Some(c) = columns.class
    {
        point.class = Some(values[c] as u8);
    }
    point
}

pub fn read(source: Box<dyn Source>, options: &Options, pts: bool) -> Result<Imported> {
    let reader = BufReader::with_capacity(1 << 16, source);
    let mut collector: Option<(Collector, Columns, Layout)> = None;
    let mut pending: Option<Columns> = None;
    let mut counted = pts;
    let mut declared: Option<u64> = None;
    for (number, line) in reader.lines().enumerate() {
        let line = line?;
        let text = line.trim().trim_start_matches('\u{feff}');
        if text.is_empty() || text.starts_with('#') || text.starts_with("//") {
            continue;
        }
        if collector.is_none() && counted && declared.is_none() {
            if let Ok(n) = text.parse::<u64>() {
                declared = Some(n);
                counted = false;
                continue;
            }
            counted = false;
        }
        let Some(values) = numbers(text) else {
            if collector.is_none() && pending.is_none() {
                let names = words(text);
                if let Some(columns) = named_columns(&names) {
                    pending = Some(columns);
                    continue;
                }
            }
            bail!(
                "line {}: `{text}` is not a point; a point line is numbers: x y z, then optional r g b or intensity",
                number + 1
            );
        };
        if collector.is_none() {
            let columns = match pending.take() {
                Some(columns) => columns,
                None => positional_columns(values.len(), pts)?,
            };
            let layout = Layout {
                normals: columns.normal.is_some_and(|n| n + 2 < columns.width),
                colours: columns.colour.is_some_and(|c| c + 2 < columns.width),
                intensity: columns.intensity.is_some(),
                classes: columns.class.is_some(),
            };
            collector = Some((
                Collector::new(layout, options.max_points, declared),
                columns,
                layout,
            ));
        }
        let Some((collector, columns, layout)) = collector.as_mut() else {
            continue;
        };
        if values.len() < columns.width {
            bail!(
                "line {}: {} values, but the first point had {}",
                number + 1,
                values.len(),
                columns.width
            );
        }
        collector.offer(point_of(&values, columns, *layout));
    }
    let (cloud, seen) = match collector {
        Some((collector, ..)) => collector.finish(),
        None => (Cloud::default(), 0),
    };
    Ok(Imported {
        cloud,
        in_file: seen,
        scans: 1,
    })
}

pub fn write(cloud: &Cloud, out: &mut impl Write) -> Result<()> {
    let layout = cloud.layout();
    let mut out = std::io::BufWriter::with_capacity(1 << 16, out);
    let mut names = vec!["x", "y", "z"];
    if layout.normals {
        names.extend(["nx", "ny", "nz"]);
    }
    if layout.colours {
        names.extend(["r", "g", "b"]);
    }
    if layout.intensity {
        names.push("intensity");
    }
    if layout.classes {
        names.push("class");
    }
    if names.len() > 3 {
        writeln!(out, "{}", names.join(" "))?;
    }
    for i in 0..cloud.len() {
        let [x, y, z] = cloud.positions[i];
        write!(out, "{x} {y} {z}")?;
        if let Some(normals) = &cloud.normals {
            let n = normals[i];
            write!(out, " {} {} {}", n[0], n[1], n[2])?;
        }
        if let Some(colours) = &cloud.colours {
            let c = colours[i];
            write!(out, " {} {} {}", c[0], c[1], c[2])?;
        }
        if let Some(intensity) = &cloud.intensity {
            write!(out, " {}", intensity[i])?;
        }
        if let Some(classes) = &cloud.classes {
            write!(out, " {}", classes[i])?;
        }
        writeln!(out)?;
    }
    out.flush()?;
    Ok(())
}
