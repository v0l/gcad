pub mod cloud;
pub mod e57;
pub mod kdtree;
pub mod las;
pub mod ply;
pub mod sample;
pub mod xyz;

pub use cloud::{Cloud, Layout, P3, Point, memory};
pub use e57::Pose;
pub use kdtree::KdTree;

use anyhow::{Result, bail};
use std::io::{Cursor, Read, Seek};
use std::path::Path;

pub trait Source: Read + Seek + Send + Sync + 'static {}

impl<T: Read + Seek + Send + Sync + 'static> Source for T {}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub max_points: Option<usize>,
    pub scan: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Imported {
    pub cloud: Cloud,
    pub in_file: u64,
    pub scans: usize,
}

impl Imported {
    pub fn report(&self) -> String {
        let sampled = if (self.cloud.len() as u64) < self.in_file {
            format!(", sampled from {}", self.in_file)
        } else {
            String::new()
        };
        format!("{}{sampled}", self.cloud.summary())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Ply,
    Xyz,
    Pts,
    Csv,
    E57,
    Las,
    Laz,
}

pub const EXTENSIONS: &str = ".ply, .xyz, .pts, .csv, .e57, .las, .laz";

pub fn format_of(path: &Path) -> Result<Format> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    Ok(match ext.as_str() {
        "ply" => Format::Ply,
        "xyz" | "txt" | "asc" => Format::Xyz,
        "pts" => Format::Pts,
        "csv" => Format::Csv,
        "e57" => Format::E57,
        "las" => Format::Las,
        "laz" => Format::Laz,
        other => bail!("a cloud is read from {EXTENSIONS}, not `.{other}`"),
    })
}

pub fn import(format: Format, source: Box<dyn Source>, options: &Options) -> Result<Imported> {
    if options.scan.is_some() && format != Format::E57 {
        bail!("`scan=` picks one scan of an .e57 file; other formats hold a single scan");
    }
    match format {
        Format::Ply => ply::read(source, options),
        Format::Xyz | Format::Csv => xyz::read(source, options, false),
        Format::Pts => xyz::read(source, options, true),
        Format::E57 => e57::read(source, options),
        Format::Las | Format::Laz => las::read(source, options),
    }
}

pub fn import_bytes(format: Format, bytes: Vec<u8>, options: &Options) -> Result<Imported> {
    import(format, Box::new(Cursor::new(bytes)), options)
}

pub fn write(cloud: &Cloud, path: &Path) -> Result<()> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "ply" => ply::write(cloud, &mut std::fs::File::create(path)?),
        "xyz" | "txt" | "csv" => xyz::write(cloud, &mut std::fs::File::create(path)?),
        "e57" => e57::write(path, &[(cloud, Pose::default())]),
        "las" | "laz" => las::write(path, cloud, 0.001),
        other => bail!("a cloud exports as .ply, .xyz, .csv, .e57, .las or .laz, not `.{other}`"),
    }
}
