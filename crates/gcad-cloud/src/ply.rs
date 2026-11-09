use crate::cloud::{Cloud, Layout, Point};
use crate::sample::Collector;
use crate::{Imported, Options, Source};
use anyhow::{Context, Result, anyhow, bail};
use std::io::{BufRead, BufReader, Write};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Scalar {
    fn parse(name: &str) -> Result<Scalar> {
        Ok(match name {
            "char" | "int8" => Scalar::I8,
            "uchar" | "uint8" => Scalar::U8,
            "short" | "int16" => Scalar::I16,
            "ushort" | "uint16" => Scalar::U16,
            "int" | "int32" => Scalar::I32,
            "uint" | "uint32" => Scalar::U32,
            "float" | "float32" => Scalar::F32,
            "double" | "float64" => Scalar::F64,
            other => bail!("unknown PLY type `{other}`"),
        })
    }

    fn size(self) -> usize {
        match self {
            Scalar::I8 | Scalar::U8 => 1,
            Scalar::I16 | Scalar::U16 => 2,
            Scalar::I32 | Scalar::U32 | Scalar::F32 => 4,
            Scalar::F64 => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    X,
    Y,
    Z,
    Nx,
    Ny,
    Nz,
    Red,
    Green,
    Blue,
    Intensity,
    Class,
    Scan,
    Other,
}

fn role_of(name: &str) -> Role {
    match name {
        "x" => Role::X,
        "y" => Role::Y,
        "z" => Role::Z,
        "nx" => Role::Nx,
        "ny" => Role::Ny,
        "nz" => Role::Nz,
        "red" | "r" | "diffuse_red" => Role::Red,
        "green" | "g" | "diffuse_green" => Role::Green,
        "blue" | "b" | "diffuse_blue" => Role::Blue,
        "intensity" | "scalar_intensity" => Role::Intensity,
        "classification" | "class" | "scalar_classification" => Role::Class,
        "scan_id" | "scan" => Role::Scan,
        _ => Role::Other,
    }
}

#[derive(Clone, Debug)]
enum Kind {
    Value(Scalar),
    List(Scalar, Scalar),
}

#[derive(Clone, Debug)]
struct Property {
    role: Role,
    kind: Kind,
}

#[derive(Clone, Debug)]
struct Element {
    name: String,
    count: u64,
    properties: Vec<Property>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Encoding {
    Ascii,
    Little,
    Big,
}

struct Header {
    encoding: Encoding,
    elements: Vec<Element>,
}

fn read_line(reader: &mut impl BufRead) -> Result<String> {
    let mut bytes = Vec::new();
    if reader.read_until(b'\n', &mut bytes)? == 0 {
        bail!("the PLY header ends before `end_header`");
    }
    Ok(String::from_utf8_lossy(&bytes).trim().to_string())
}

fn read_header(reader: &mut impl BufRead) -> Result<Header> {
    if read_line(reader)? != "ply" {
        bail!("not a PLY file, it does not start with `ply`");
    }
    let mut encoding = None;
    let mut elements: Vec<Element> = Vec::new();
    loop {
        let line = read_line(reader)?;
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["end_header"] => break,
            ["format", name, _] => {
                encoding = Some(match *name {
                    "ascii" => Encoding::Ascii,
                    "binary_little_endian" => Encoding::Little,
                    "binary_big_endian" => Encoding::Big,
                    other => bail!("unknown PLY format `{other}`"),
                })
            }
            ["element", name, count] => elements.push(Element {
                name: name.to_string(),
                count: count
                    .parse()
                    .with_context(|| format!("element count `{count}`"))?,
                properties: Vec::new(),
            }),
            ["property", "list", count, item, name] => {
                let element = elements
                    .last_mut()
                    .ok_or_else(|| anyhow!("a PLY property comes before any element"))?;
                element.properties.push(Property {
                    role: role_of(name),
                    kind: Kind::List(Scalar::parse(count)?, Scalar::parse(item)?),
                });
            }
            ["property", kind, name] => {
                let element = elements
                    .last_mut()
                    .ok_or_else(|| anyhow!("a PLY property comes before any element"))?;
                element.properties.push(Property {
                    role: role_of(name),
                    kind: Kind::Value(Scalar::parse(kind)?),
                });
            }
            _ => {}
        }
    }
    Ok(Header {
        encoding: encoding.ok_or_else(|| anyhow!("the PLY header has no `format` line"))?,
        elements,
    })
}

struct Values<'a, R: BufRead> {
    reader: &'a mut R,
    encoding: Encoding,
    line: Vec<f64>,
    text: String,
}

impl<R: BufRead> Values<'_, R> {
    fn binary(&mut self, kind: Scalar) -> Result<f64> {
        let mut buffer = [0u8; 8];
        let bytes = &mut buffer[..kind.size()];
        self.reader.read_exact(bytes)?;
        if self.encoding == Encoding::Big {
            bytes.reverse();
        }
        Ok(match kind {
            Scalar::I8 => bytes[0] as i8 as f64,
            Scalar::U8 => bytes[0] as f64,
            Scalar::I16 => i16::from_le_bytes([bytes[0], bytes[1]]) as f64,
            Scalar::U16 => u16::from_le_bytes([bytes[0], bytes[1]]) as f64,
            Scalar::I32 => i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64,
            Scalar::U32 => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64,
            Scalar::F32 => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64,
            Scalar::F64 => f64::from_le_bytes(buffer),
        })
    }

    fn next_line(&mut self) -> Result<()> {
        loop {
            self.text.clear();
            if self.reader.read_line(&mut self.text)? == 0 {
                bail!("the PLY data ends early");
            }
            if !self.text.trim().is_empty() {
                break;
            }
        }
        self.line.clear();
        for word in self.text.split_ascii_whitespace() {
            self.line.push(
                word.parse()
                    .with_context(|| format!("`{word}` is not a number in the PLY data"))?,
            );
        }
        self.line.reverse();
        Ok(())
    }

    fn ascii(&mut self) -> Result<f64> {
        self.line
            .pop()
            .ok_or_else(|| anyhow!("a PLY vertex line has too few values"))
    }

    fn value(&mut self, kind: Scalar) -> Result<f64> {
        match self.encoding {
            Encoding::Ascii => self.ascii(),
            _ => self.binary(kind),
        }
    }

    fn record(&mut self, element: &Element, mut keep: impl FnMut(Role, f64, Scalar)) -> Result<()> {
        if self.encoding == Encoding::Ascii {
            self.next_line()?;
        }
        for property in &element.properties {
            match property.kind {
                Kind::Value(kind) => {
                    let v = self.value(kind)?;
                    keep(property.role, v, kind);
                }
                Kind::List(count, item) => {
                    let n = self.value(count)? as usize;
                    for _ in 0..n {
                        self.value(item)?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn byte_of(value: f64, kind: Scalar) -> u8 {
    let scaled = match kind {
        Scalar::F32 | Scalar::F64 if value <= 1.0 => value * 255.0,
        Scalar::U16 => value / 257.0,
        _ => value,
    };
    scaled.round().clamp(0.0, 255.0) as u8
}

pub fn read(source: Box<dyn Source>, options: &Options) -> Result<Imported> {
    let mut reader = BufReader::with_capacity(1 << 16, source);
    let header = read_header(&mut reader)?;
    let at = header
        .elements
        .iter()
        .position(|e| e.name == "vertex")
        .ok_or_else(|| anyhow!("the PLY file has no `vertex` element"))?;
    let vertex = &header.elements[at];
    let has = |role: Role| vertex.properties.iter().any(|p| p.role == role);
    if !(has(Role::X) && has(Role::Y) && has(Role::Z)) {
        bail!("the PLY vertex element has no x, y and z");
    }
    let layout = Layout {
        normals: has(Role::Nx) && has(Role::Ny) && has(Role::Nz),
        colours: has(Role::Red) && has(Role::Green) && has(Role::Blue),
        intensity: has(Role::Intensity),
        classes: has(Role::Class),
    };
    let mut values = Values {
        reader: &mut reader,
        encoding: header.encoding,
        line: Vec::new(),
        text: String::new(),
    };
    for before in &header.elements[..at] {
        for _ in 0..before.count {
            values.record(before, |_, _, _| {})?;
        }
    }
    let mut collector = Collector::new(layout, options.max_points, Some(vertex.count));
    for _ in 0..vertex.count {
        let mut point = Point::default();
        let mut normal = [0f32; 3];
        let mut colour = [0u8; 3];
        values.record(vertex, |role, v, kind| match role {
            Role::X => point.position[0] = v,
            Role::Y => point.position[1] = v,
            Role::Z => point.position[2] = v,
            Role::Nx => normal[0] = v as f32,
            Role::Ny => normal[1] = v as f32,
            Role::Nz => normal[2] = v as f32,
            Role::Red => colour[0] = byte_of(v, kind),
            Role::Green => colour[1] = byte_of(v, kind),
            Role::Blue => colour[2] = byte_of(v, kind),
            Role::Intensity => point.intensity = Some(v as f32),
            Role::Class => point.class = Some(v as u8),
            Role::Scan => point.scan = v as u16,
            Role::Other => {}
        })?;
        point.normal = layout.normals.then_some(normal);
        point.colour = layout.colours.then_some(colour);
        collector.offer(point);
    }
    let (cloud, seen) = collector.finish();
    Ok(Imported {
        cloud,
        in_file: seen,
        scans: 1,
    })
}

fn header_of(cloud: &Cloud, format: &str) -> (String, bool) {
    let layout = cloud.layout();
    let with_scans = cloud.scans.iter().any(|&s| s != 0);
    let mut header = format!("ply\nformat {format} 1.0\n");
    header.push_str("comment written by gcad\n");
    header.push_str(&format!("element vertex {}\n", cloud.len()));
    header.push_str("property double x\nproperty double y\nproperty double z\n");
    if layout.normals {
        header.push_str("property float nx\nproperty float ny\nproperty float nz\n");
    }
    if layout.colours {
        header.push_str("property uchar red\nproperty uchar green\nproperty uchar blue\n");
    }
    if layout.intensity {
        header.push_str("property float intensity\n");
    }
    if layout.classes {
        header.push_str("property uchar classification\n");
    }
    if with_scans {
        header.push_str("property ushort scan_id\n");
    }
    header.push_str("end_header\n");
    (header, with_scans)
}

pub fn write_ascii(cloud: &Cloud, out: &mut impl Write) -> Result<()> {
    let (header, with_scans) = header_of(cloud, "ascii");
    let mut out = std::io::BufWriter::with_capacity(1 << 16, out);
    out.write_all(header.as_bytes())?;
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
        if with_scans {
            write!(out, " {}", cloud.scans[i])?;
        }
        writeln!(out)?;
    }
    out.flush()?;
    Ok(())
}

pub fn write(cloud: &Cloud, out: &mut impl Write) -> Result<()> {
    let (header, with_scans) = header_of(cloud, "binary_little_endian");
    let mut out = std::io::BufWriter::with_capacity(1 << 16, out);
    out.write_all(header.as_bytes())?;
    for i in 0..cloud.len() {
        for v in cloud.positions[i] {
            out.write_all(&v.to_le_bytes())?;
        }
        if let Some(normals) = &cloud.normals {
            for v in normals[i] {
                out.write_all(&v.to_le_bytes())?;
            }
        }
        if let Some(colours) = &cloud.colours {
            out.write_all(&colours[i])?;
        }
        if let Some(intensity) = &cloud.intensity {
            out.write_all(&intensity[i].to_le_bytes())?;
        }
        if let Some(classes) = &cloud.classes {
            out.write_all(&[classes[i]])?;
        }
        if with_scans {
            out.write_all(&cloud.scans[i].to_le_bytes())?;
        }
    }
    out.flush()?;
    Ok(())
}
