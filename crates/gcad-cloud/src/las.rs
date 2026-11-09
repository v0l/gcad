use crate::cloud::{Cloud, Layout, Point};
use crate::sample::Collector;
use crate::{Imported, Options, Source};
use anyhow::{Result, anyhow};
use las::point::Classification;
use las::{Builder, Color, PointDataBuilder, Reader, Transform, Vector, Writer};
use std::path::Path;

const CHUNK: u64 = 65_536;

pub fn read(source: Box<dyn Source>, options: &Options) -> Result<Imported> {
    let mut reader = Reader::new(source).map_err(|e| anyhow!("reading LAS: {e}"))?;
    let header = reader.header();
    let format = header.point_format();
    let layout = Layout {
        colours: format.has_color,
        intensity: true,
        classes: true,
        ..Layout::default()
    };
    let total = header.number_of_points();
    let mut collector = Collector::new(layout, options.max_points, Some(total));
    let mut chunk = PointDataBuilder::new().for_header(reader.header()).build();
    let mut left = total;
    while left > 0 {
        let got = reader
            .fill_points(CHUNK.min(left), &mut chunk)
            .map_err(|e| anyhow!("reading LAS points: {e}"))?;
        if got == 0 {
            break;
        }
        left -= got;
        for point in chunk.points() {
            let point = point.map_err(|e| anyhow!("reading LAS points: {e}"))?;
            let mut out = Point::at([point.x, point.y, point.z]);
            out.intensity = Some(point.intensity as f32 / 65535.0);
            out.class = Some(u8::from(point.classification));
            out.scan = point.point_source_id;
            out.colour = point.color.map(|c| {
                let wide = c.red.max(c.green).max(c.blue) > 255;
                [c.red, c.green, c.blue].map(|v| if wide { (v >> 8) as u8 } else { v as u8 })
            });
            collector.offer(out);
        }
    }
    let (cloud, seen) = collector.finish();
    Ok(Imported {
        cloud,
        in_file: seen,
        scans: 1,
    })
}

pub fn write(path: &Path, cloud: &Cloud, scale: f64) -> Result<()> {
    let (lo, _) = cloud.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
    let offset = lo.map(|v| (v / 100.0).floor() * 100.0);
    let mut builder = Builder::from((1, 2));
    builder.point_format = las::point::Format::new(if cloud.colours.is_some() { 2 } else { 0 })
        .map_err(|e| anyhow!("{e}"))?;
    builder.transforms = Vector {
        x: Transform {
            scale,
            offset: offset[0],
        },
        y: Transform {
            scale,
            offset: offset[1],
        },
        z: Transform {
            scale,
            offset: offset[2],
        },
    };
    let header = builder.into_header().map_err(|e| anyhow!("{e}"))?;
    let mut writer = Writer::from_path(path, header).map_err(|e| anyhow!("{e}"))?;
    for i in 0..cloud.len() {
        let [x, y, z] = cloud.positions[i];
        let mut point = las::Point {
            x,
            y,
            z,
            ..Default::default()
        };
        if let Some(intensity) = &cloud.intensity {
            point.intensity = (intensity[i].clamp(0.0, 1.0) * 65535.0).round() as u16;
        }
        if let Some(classes) = &cloud.classes {
            point.classification =
                Classification::new(classes[i]).unwrap_or(Classification::Unclassified);
        }
        if let Some(colours) = &cloud.colours {
            let [red, green, blue] = colours[i].map(|v| v as u16 * 257);
            point.color = Some(Color { red, green, blue });
        }
        writer.write_point(point).map_err(|e| anyhow!("{e}"))?;
    }
    writer.close().map_err(|e| anyhow!("{e}"))
}
