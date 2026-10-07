use crate::geometry;
use anyhow::{Result, bail};
use monstertruck::mesh::PolygonMesh;
use monstertruck::mesh::stl::{StlType, write};
use monstertruck::modeling::*;
use monstertruck::step::save::{CompleteStepDisplay, StepHeaderDescriptor, StepModels};
use std::fmt::Write as _;

pub type Colour = [f64; 3];

pub fn export(solids: &[&Solid], path: &str) -> Result<()> {
    let parts: Vec<(&Solid, Option<Colour>)> = solids.iter().map(|s| (*s, None)).collect();
    export_coloured(&parts, path)
}

fn fine_mesh(solid: &Solid) -> PolygonMesh {
    geometry::mesh(
        solid,
        (geometry::bounds(solid).diameter() * 2.0e-4).max(0.01),
    )
}

fn merged_mesh(parts: &[(&Solid, Option<Colour>)]) -> PolygonMesh {
    let mut mesh = PolygonMesh::default();
    parts
        .iter()
        .for_each(|(solid, _)| mesh.merge(fine_mesh(solid)));
    mesh
}

pub fn export_coloured(parts: &[(&Solid, Option<Colour>)], path: &str) -> Result<()> {
    let extension = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "step" | "stp" => std::fs::write(path, step_text(parts))?,
        "stl" => write(
            &merged_mesh(parts),
            &mut std::fs::File::create(path)?,
            StlType::Binary,
        )?,
        "obj" => monstertruck::mesh::obj::write(&merged_mesh(parts), std::fs::File::create(path)?)?,
        "3mf" => std::fs::write(path, three_mf(parts))?,
        "svg" => std::fs::write(path, crate::drawing::drawing(parts))?,
        other => bail!("cannot export `.{other}`, use .step, .stl, .obj, .3mf or .svg"),
    }
    Ok(())
}

fn step_text(parts: &[(&Solid, Option<Colour>)]) -> String {
    let header = StepHeaderDescriptor {
        organization_system: "linecad".to_string(),
        ..Default::default()
    };
    let compressed: Vec<_> = parts.iter().map(|(solid, _)| solid.compress()).collect();
    let step = CompleteStepDisplay::new(
        compressed.iter().collect::<StepModels<'_, _, _, _>>(),
        header,
    )
    .to_string();
    if parts.iter().all(|(_, colour)| colour.is_none()) {
        return step;
    }
    with_colours(&step, parts)
}

fn entity_id(line: &str) -> Option<usize> {
    line.trim_start()
        .strip_prefix('#')?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn with_colours(step: &str, parts: &[(&Solid, Option<Colour>)]) -> String {
    let breps: Vec<usize> = step
        .lines()
        .filter(|line| line.contains("= MANIFOLD_SOLID_BREP("))
        .filter_map(entity_id)
        .collect();
    let context = step
        .lines()
        .find(|line| line.contains("= ADVANCED_BREP_SHAPE_REPRESENTATION("))
        .and_then(|line| line.rsplit('#').next())
        .and_then(|tail| tail.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|digits| digits.parse::<usize>().ok());
    let (Some(context), true) = (context, breps.len() == parts.len()) else {
        return step.to_string();
    };
    let mut next = step.lines().filter_map(entity_id).max().unwrap_or(0) + 1;
    let mut added = String::new();
    let mut styled = Vec::new();
    for (brep, (_, colour)) in breps.iter().zip(parts) {
        let Some([r, g, b]) = colour else { continue };
        let id = next;
        let _ = writeln!(
            added,
            "#{} = COLOUR_RGB('', {r:.4}, {g:.4}, {b:.4});\n\
             #{} = FILL_AREA_STYLE_COLOUR('', #{});\n\
             #{} = FILL_AREA_STYLE('', (#{}));\n\
             #{} = SURFACE_STYLE_FILL_AREA(#{});\n\
             #{} = SURFACE_SIDE_STYLE('', (#{}));\n\
             #{} = SURFACE_STYLE_USAGE(.BOTH., #{});\n\
             #{} = PRESENTATION_STYLE_ASSIGNMENT((#{}));\n\
             #{} = STYLED_ITEM('color', (#{}), #{brep});",
            id,
            id + 1,
            id,
            id + 2,
            id + 1,
            id + 3,
            id + 2,
            id + 4,
            id + 3,
            id + 5,
            id + 4,
            id + 6,
            id + 5,
            id + 7,
            id + 6,
        );
        styled.push(format!("#{}", id + 7));
        next += 8;
    }
    let _ = writeln!(
        added,
        "#{next} = MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('', ({}), #{context});",
        styled.join(", ")
    );
    match step.rfind("ENDSEC;") {
        Some(at) => format!("{}{added}{}", &step[..at], &step[at..]),
        None => step.to_string(),
    }
}

fn three_mf(parts: &[(&Solid, Option<Colour>)]) -> Vec<u8> {
    let mut model = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n<resources>\n",
    );
    let coloured = parts.iter().any(|(_, c)| c.is_some());
    if coloured {
        model.push_str("<basematerials id=\"1\">\n");
        for (i, (_, colour)) in parts.iter().enumerate() {
            let [r, g, b] = colour
                .unwrap_or([0.7, 0.7, 0.7])
                .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
            let _ = writeln!(
                model,
                "<base name=\"body{i}\" displaycolor=\"#{r:02X}{g:02X}{b:02X}\"/>"
            );
        }
        model.push_str("</basematerials>\n");
    }
    for (i, (solid, _)) in parts.iter().enumerate() {
        let (points, triangles) = geometry::welded(&fine_mesh(solid));
        let material = if coloured {
            format!(" pid=\"1\" pindex=\"{i}\"")
        } else {
            String::new()
        };
        let _ = writeln!(
            model,
            "<object id=\"{}\" type=\"model\"{material}>\n<mesh>\n<vertices>",
            i + 2
        );
        for p in &points {
            let _ = writeln!(model, "<vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", p.x, p.y, p.z);
        }
        model.push_str("</vertices>\n<triangles>\n");
        for [a, b, c] in &triangles {
            let _ = writeln!(model, "<triangle v1=\"{a}\" v2=\"{b}\" v3=\"{c}\"/>");
        }
        model.push_str("</triangles>\n</mesh>\n</object>\n");
    }
    model.push_str("</resources>\n<build>\n");
    for i in 0..parts.len() {
        let _ = writeln!(model, "<item objectid=\"{}\"/>", i + 2);
    }
    model.push_str("</build>\n</model>\n");
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>\n";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>\n";
    stored_zip(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("3D/3dmodel.model", model.as_bytes()),
    ])
}

fn crc32(data: &[u8]) -> u32 {
    !data.iter().fold(!0u32, |crc, &byte| {
        (0..8).fold(crc ^ byte as u32, |c, _| {
            if c & 1 == 1 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            }
        })
    })
}

fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = out.len() as u32;
        let (crc, size, name_len) = (crc32(data), data.len() as u32, name.len() as u16);
        let common = |buffer: &mut Vec<u8>| {
            buffer.extend_from_slice(&20u16.to_le_bytes());
            buffer.extend_from_slice(&0u16.to_le_bytes());
            buffer.extend_from_slice(&0u16.to_le_bytes());
            buffer.extend_from_slice(&0u16.to_le_bytes());
            buffer.extend_from_slice(&0x21u16.to_le_bytes());
            buffer.extend_from_slice(&crc.to_le_bytes());
            buffer.extend_from_slice(&size.to_le_bytes());
            buffer.extend_from_slice(&size.to_le_bytes());
            buffer.extend_from_slice(&name_len.to_le_bytes());
            buffer.extend_from_slice(&0u16.to_le_bytes());
        };
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        common(&mut out);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        common(&mut central);
        central.extend_from_slice(&[0u8; 10]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let start = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}
