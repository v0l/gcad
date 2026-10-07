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
    let tidied: Vec<Solid> = parts.iter().map(|(solid, _)| even_leaders(solid)).collect();
    let compressed: Vec<_> = tidied.iter().map(|solid| solid.compress()).collect();
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

pub struct Instance<'a> {
    pub name: String,
    pub product: usize,
    pub placement: Matrix4,
    pub solid: &'a Solid,
}

pub struct Product {
    pub name: String,
    pub solid: Solid,
    pub colour: Option<Colour>,
}

pub fn assembly(model: &crate::model::Model) -> (Vec<Product>, Vec<Instance<'_>>) {
    let mut products: Vec<(Option<crate::model::Source>, Product)> = Vec::new();
    let mut instances = Vec::new();
    for (name, solid) in model.named_solids() {
        let source = model.sources.get(&name).cloned();
        let placement = model
            .placements
            .get(&name)
            .copied()
            .unwrap_or_else(Matrix4::identity);
        let known = products
            .iter()
            .position(|(s, _)| source.is_some() && *s == source);
        let product = match known {
            Some(index) => index,
            None => {
                let local = placement
                    .invert()
                    .map(|undo| builder::transformed(solid, undo))
                    .unwrap_or_else(|| solid.clone());
                let label = source
                    .as_ref()
                    .map(|s| {
                        let stem = s
                            .file
                            .file_stem()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if s.body == "main" {
                            stem
                        } else {
                            format!("{stem}.{}", s.body)
                        }
                    })
                    .unwrap_or_else(|| name.clone());
                products.push((
                    source,
                    Product {
                        name: label,
                        solid: local,
                        colour: model.colours.get(&name).copied(),
                    },
                ));
                products.len() - 1
            }
        };
        instances.push(Instance {
            name,
            product,
            placement,
            solid,
        });
    }
    (products.into_iter().map(|(_, p)| p).collect(), instances)
}

pub fn export_model(model: &crate::model::Model, name: &str, path: &str) -> Result<()> {
    let step = path.to_ascii_lowercase();
    if model.assembly && (step.ends_with(".step") || step.ends_with(".stp")) {
        let (products, instances) = assembly(model);
        std::fs::write(path, assembly_step(name, &products, &instances))?;
        return Ok(());
    }
    export_coloured(&model.parts(), path)
}

fn quoted(text: &str) -> String {
    text.replace('\'', "''")
}

pub fn assembly_step(name: &str, products: &[Product], instances: &[Instance<'_>]) -> String {
    let parts: Vec<(&Solid, Option<Colour>)> =
        products.iter().map(|p| (&p.solid, p.colour)).collect();
    let step = step_text(&parts);
    let breps: Vec<usize> = step
        .lines()
        .filter(|line| line.contains("= MANIFOLD_SOLID_BREP("))
        .filter_map(entity_id)
        .collect();
    let find = |kind: &str| {
        step.lines()
            .find(|line| line.contains(kind))
            .and_then(entity_id)
    };
    let (Some(root_rep), Some(root_pd), Some(root_product), Some(root_context)) = (
        find("= ADVANCED_BREP_SHAPE_REPRESENTATION("),
        find("= PRODUCT_DEFINITION('design'"),
        find("= PRODUCT('"),
        find("= PRODUCT_CONTEXT("),
    ) else {
        return step;
    };
    let definition_context = find("= PRODUCT_DEFINITION_CONTEXT(").unwrap_or(0);
    let geometry_context = step
        .lines()
        .find(|line| line.contains("= ADVANCED_BREP_SHAPE_REPRESENTATION("))
        .and_then(|line| line.rsplit('#').next())
        .and_then(|tail| tail.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|digits| digits.parse::<usize>().ok())
        .unwrap_or(0);
    if breps.len() != products.len() {
        return step;
    }
    let mut ids = Ids(step.lines().filter_map(entity_id).max().unwrap_or(0) + 1);
    let mut added = String::new();
    let axis = |ids: &mut Ids, added: &mut String, m: Matrix4| {
        let (o, z, x) = (
            m.w.truncate(),
            m.z.truncate().normalize(),
            m.x.truncate().normalize(),
        );
        let (p, dz, dx, a) = (ids.take(), ids.take(), ids.take(), ids.take());
        let _ = writeln!(
            added,
            "#{p} = CARTESIAN_POINT('', ({:.9}, {:.9}, {:.9}));\n\
             #{dz} = DIRECTION('', ({:.12}, {:.12}, {:.12}));\n\
             #{dx} = DIRECTION('', ({:.12}, {:.12}, {:.12}));\n\
             #{a} = AXIS2_PLACEMENT_3D('', #{p}, #{dz}, #{dx});",
            o.x, o.y, o.z, z.x, z.y, z.z, x.x, x.y, x.z
        );
        a
    };
    let root_axis = axis(&mut ids, &mut added, Matrix4::identity());
    let mut product_reps = Vec::new();
    for (product, brep) in products.iter().zip(&breps) {
        let label = quoted(&product.name);
        let own_axis = axis(&mut ids, &mut added, Matrix4::identity());
        let [prod, pdf, pd, pds, rep, sdr] = ids.many();
        let _ = writeln!(
            added,
            "#{prod} = PRODUCT('{label}', '{label}', '', (#{root_context}));\n\
             #{pdf} = PRODUCT_DEFINITION_FORMATION('', '', #{prod});\n\
             #{pd} = PRODUCT_DEFINITION('design', '', #{pdf}, #{definition_context});\n\
             #{pds} = PRODUCT_DEFINITION_SHAPE('', '', #{pd});\n\
             #{rep} = ADVANCED_BREP_SHAPE_REPRESENTATION('{label}', (#{brep}, #{own_axis}), #{geometry_context});\n\
             #{sdr} = SHAPE_DEFINITION_REPRESENTATION(#{pds}, #{rep});"
        );
        product_reps.push((pd, rep, own_axis));
    }
    let mut placed_axes = vec![root_axis];
    for (k, instance) in instances.iter().enumerate() {
        let (pd, rep, own_axis) = product_reps[instance.product];
        let there = axis(&mut ids, &mut added, instance.placement);
        placed_axes.push(there);
        let label = quoted(&instance.name);
        let [transform, relation, nauo, pds, cdsr] = ids.many();
        let _ = writeln!(
            added,
            "#{transform} = ITEM_DEFINED_TRANSFORMATION('', '', #{own_axis}, #{there});\n\
             #{relation} = ( REPRESENTATION_RELATIONSHIP('', '', #{rep}, #{root_rep}) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{transform}) SHAPE_REPRESENTATION_RELATIONSHIP() );\n\
             #{nauo} = NEXT_ASSEMBLY_USAGE_OCCURRENCE('{}', '{label}', '', #{root_pd}, #{pd}, $);\n\
             #{pds} = PRODUCT_DEFINITION_SHAPE('{label}', '', #{nauo});\n\
             #{cdsr} = CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{relation}, #{pds});",
            k + 1
        );
    }
    let assembly_name = quoted(name);
    let root_items = placed_axes
        .iter()
        .map(|a| format!("#{a}"))
        .collect::<Vec<_>>()
        .join(", ");
    let rewritten: Vec<String> = step
        .lines()
        .map(|line| match entity_id(line) {
            Some(id) if id == root_rep => format!(
                "#{id} = SHAPE_REPRESENTATION('{assembly_name}', ({root_items}), #{geometry_context});"
            ),
            Some(id) if id == root_product => format!(
                "#{id} = PRODUCT('{assembly_name}', '{assembly_name}', '', (#{root_context}));"
            ),
            _ => line.to_string(),
        })
        .collect();
    let step = rewritten.join("\n") + "\n";
    match step.rfind("ENDSEC;") {
        Some(at) => format!("{}{added}{}", &step[..at], &step[at..]),
        None => step,
    }
}

struct Ids(usize);

impl Ids {
    fn take(&mut self) -> usize {
        self.0 += 1;
        self.0 - 1
    }

    fn many<const N: usize>(&mut self) -> [usize; N] {
        std::array::from_fn(|_| self.take())
    }
}

fn even_leader(curve: &Curve) -> Curve {
    let Curve::IntersectionCurve(intersection) = curve else {
        return curve.clone();
    };
    let Curve::BsplineCurve(leader) = intersection.leader().as_ref() else {
        return curve.clone();
    };
    let poles = leader.control_points();
    if leader.degree() != 1 || poles.len() < 4 {
        return curve.clone();
    }
    let lengths: Vec<f64> = std::iter::once(0.0)
        .chain(poles.windows(2).scan(0.0, |total, pair| {
            *total += pair[0].distance(pair[1]);
            Some(*total)
        }))
        .collect();
    let total = *lengths.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return curve.clone();
    }
    let (t0, t1) = leader.range_tuple();
    let knots = leader.knot_vector();
    let pole_parameter = |i: usize| knots[i + 1];
    let count = poles.len() - 1;
    let points: Vec<Point3> = (0..=count)
        .map(|k| {
            if k == 0 || k == count {
                return poles[if k == 0 { 0 } else { count }];
            }
            let want = total * k as f64 / count as f64;
            let i = lengths.partition_point(|&l| l <= want).clamp(1, count) - 1;
            let span = lengths[i + 1] - lengths[i];
            let f = if span > 0.0 {
                (want - lengths[i]) / span
            } else {
                0.0
            };
            let t = pole_parameter(i) + (pole_parameter(i + 1) - pole_parameter(i)) * f;
            curve.subs(t)
        })
        .collect();
    let mut knots = KnotVector::uniform_knot(1, count);
    knots.transform(t1 - t0, t0);
    let mut even = intersection.clone();
    **even.leader_mut() = Curve::BsplineCurve(BsplineCurve::new(knots, points));
    Curve::IntersectionCurve(even)
}

fn even_leaders(solid: &Solid) -> Solid {
    solid.mapped(|p| *p, even_leader, |s| s.clone())
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
