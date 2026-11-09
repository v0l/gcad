use crate::cloud::{Cloud, Layout, Point};
use crate::sample::Collector;
use crate::{Imported, Options, Source};
use anyhow::{Context, Result, anyhow, bail};
use e57::{
    CartesianCoordinate, E57Reader, E57Writer, Quaternion, Record, RecordDataType, RecordName,
    RecordValue, Transform, Translation,
};
use std::io::{Read, Seek};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub rotation: [f64; 4],
    pub translation: [f64; 3],
}

impl Default for Pose {
    fn default() -> Self {
        Pose {
            rotation: [1.0, 0.0, 0.0, 0.0],
            translation: [0.0; 3],
        }
    }
}

pub fn read(source: Box<dyn Source>, options: &Options) -> Result<Imported> {
    read_from(source, options)
}

fn read_from<T: Read + Seek>(source: T, options: &Options) -> Result<Imported> {
    let mut file = E57Reader::new(source).map_err(|e| anyhow!("reading E57: {e}"))?;
    let scans = file.pointclouds();
    if scans.is_empty() {
        bail!("the E57 file has no point clouds");
    }
    let picked: Vec<(usize, &e57::PointCloud)> = match options.scan {
        Some(n) => vec![(
            n,
            scans.get(n).ok_or_else(|| {
                anyhow!("`scan={n}` but the file has {} scan(s), 0 to {}", scans.len(), scans.len() - 1)
            })?,
        )],
        None => scans.iter().enumerate().collect(),
    };
    let layout = Layout {
        colours: picked.iter().any(|(_, s)| s.has_color()),
        intensity: picked.iter().any(|(_, s)| s.has_intensity()),
        ..Layout::default()
    };
    let total: u64 = picked.iter().map(|(_, s)| s.records).sum();
    let mut collector = Collector::new(layout, options.max_points, Some(total));
    for (index, scan) in &picked {
        let mut reader = file
            .pointcloud_simple(scan)
            .map_err(|e| anyhow!("scan {index}: {e}"))?;
        reader.intensity_to_color(false);
        for point in &mut reader {
            let point = point.map_err(|e| anyhow!("scan {index}: {e}"))?;
            let CartesianCoordinate::Valid { x, y, z } = point.cartesian else {
                continue;
            };
            let mut out = Point::at([x, y, z]);
            out.scan = *index as u16 + 1;
            out.colour = point
                .color
                .map(|c| [c.red, c.green, c.blue].map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8))
                .or(layout.colours.then_some([180; 3]));
            out.intensity = point.intensity;
            collector.offer(out);
        }
    }
    let (cloud, seen) = collector.finish();
    Ok(Imported {
        cloud,
        in_file: seen,
        scans: picked.len(),
    })
}

pub fn write(path: &Path, scans: &[(&Cloud, Pose)]) -> Result<()> {
    let mut writer = E57Writer::from_file(path, "gcad-cloud-0001")
        .map_err(|e| anyhow!("writing {}: {e}", path.display()))?;
    for (k, (cloud, pose)) in scans.iter().enumerate() {
        let layout = cloud.layout();
        let mut prototype = vec![
            Record {
                name: RecordName::CartesianX,
                data_type: RecordDataType::F64,
            },
            Record {
                name: RecordName::CartesianY,
                data_type: RecordDataType::F64,
            },
            Record {
                name: RecordName::CartesianZ,
                data_type: RecordDataType::F64,
            },
        ];
        if layout.colours {
            prototype.extend([
                Record::COLOR_RED_U8,
                Record::COLOR_GREEN_U8,
                Record::COLOR_BLUE_U8,
            ]);
        }
        if layout.intensity {
            prototype.push(Record::INTENSITY_UNIT_F32);
        }
        let mut out = writer
            .add_pointcloud(&format!("gcad-scan-{k:04}"), prototype)
            .map_err(|e| anyhow!("{e}"))?;
        out.set_transform(Some(Transform {
            rotation: Quaternion {
                w: pose.rotation[0],
                x: pose.rotation[1],
                y: pose.rotation[2],
                z: pose.rotation[3],
            },
            translation: Translation {
                x: pose.translation[0],
                y: pose.translation[1],
                z: pose.translation[2],
            },
        }));
        for i in 0..cloud.len() {
            let mut values = cloud.positions[i]
                .iter()
                .map(|&v| RecordValue::Double(v))
                .collect::<Vec<_>>();
            if let Some(colours) = &cloud.colours {
                values.extend(colours[i].iter().map(|&c| RecordValue::Integer(c as i64)));
            }
            if let Some(intensity) = &cloud.intensity {
                values.push(RecordValue::Single(intensity[i].clamp(0.0, 1.0)));
            }
            out.add_point(values).map_err(|e| anyhow!("{e}"))?;
        }
        out.finalize().map_err(|e| anyhow!("{e}"))?;
    }
    writer
        .finalize()
        .with_context(|| format!("writing {}", path.display()))
        .map_err(|e| anyhow!("{e:#}"))
}
