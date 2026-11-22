use crate::Model;
use crate::arch::{Fill, Opening};
use crate::geometry;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

const ALPHABET: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

pub fn global_id(kind: &str, name: &str) -> String {
    let digest = Sha256::digest(format!("gcad:{kind}:{name}"));
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    let mut value = u128::from_be_bytes(bytes);
    let mut out = [0u8; 22];
    for slot in out.iter_mut().rev() {
        *slot = ALPHABET[(value & 63) as usize];
        value >>= 6;
    }
    out[0] = ALPHABET[(out[0] & 3) as usize];
    String::from_utf8_lossy(&out).into_owned()
}

pub fn text(value: &str) -> String {
    let mut out = String::from("'");
    let mut wide = String::new();
    let flush = |out: &mut String, wide: &mut String| {
        if !wide.is_empty() {
            out.push_str("\\X2\\");
            for unit in wide.encode_utf16() {
                let _ = write!(out, "{unit:04X}");
            }
            out.push_str("\\X0\\");
            wide.clear();
        }
    };
    for c in value.chars() {
        if c.is_ascii() && !c.is_ascii_control() {
            flush(&mut out, &mut wide);
            match c {
                '\'' => out.push_str("''"),
                '\\' => out.push_str("\\\\"),
                other => out.push(other),
            }
        } else {
            wide.push(c);
        }
    }
    flush(&mut out, &mut wide);
    out.push('\'');
    out
}

pub fn real(value: f64) -> String {
    let rounded = (value * 1.0e6).round() / 1.0e6;
    let mut out = format!("{rounded}");
    if !out.contains('.') {
        out.push('.');
    }
    if out == "-0." {
        out = "0.".into();
    }
    out
}

struct Writer {
    lines: Vec<String>,
}

impl Writer {
    fn add(&mut self, body: String) -> usize {
        self.lines.push(body);
        self.lines.len()
    }

    fn list(ids: &[usize]) -> String {
        format!(
            "({})",
            ids.iter()
                .map(|id| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    }

    fn point(&mut self, p: [f64; 3]) -> usize {
        self.add(format!(
            "IFCCARTESIANPOINT(({},{},{}))",
            real(p[0]),
            real(p[1]),
            real(p[2])
        ))
    }

    fn place(&mut self, relative: Option<usize>, at: [f64; 3]) -> usize {
        let point = self.point(at);
        let axes = self.add(format!("IFCAXIS2PLACEMENT3D(#{point},$,$)"));
        let rel = relative.map_or("$".to_string(), |r| format!("#{r}"));
        self.add(format!("IFCLOCALPLACEMENT({rel},#{axes})"))
    }

    fn rel(&mut self, kind: &str, name: &str, tail: String) -> usize {
        self.add(format!(
            "{kind}('{}',$,$,$,{tail})",
            global_id(kind, name)
        ))
    }

    fn material(&mut self, known: &mut Vec<(String, usize)>, name: &str) -> usize {
        if let Some((_, id)) = known.iter().find(|(n, _)| n == name) {
            return *id;
        }
        let id = self.add(format!("IFCMATERIAL({},$,$)", text(name)));
        known.push((name.to_string(), id));
        id
    }

    fn property(&mut self, name: &str, value: &str) -> usize {
        self.add(format!("IFCPROPERTYSINGLEVALUE({},$,{value},$)", text(name)))
    }

    fn pset(&mut self, kind: &str, owner: &str, target: usize, props: Vec<usize>) {
        let set = self.add(format!(
            "IFCPROPERTYSET('{}',$,{},$,{})",
            global_id(kind, owner),
            text(kind),
            Writer::list(&props)
        ));
        self.rel(
            "IFCRELDEFINESBYPROPERTIES",
            &format!("{owner}.{kind}"),
            format!("(#{target}),#{set}"),
        );
    }

    fn style(&mut self, item: usize, colour: [f64; 3], opacity: Option<f64>) {
        let rgb = self.add(format!(
            "IFCCOLOURRGB($,{},{},{})",
            real(colour[0]),
            real(colour[1]),
            real(colour[2])
        ));
        let transparency = real(1.0 - opacity.unwrap_or(1.0));
        let render = self.add(format!(
            "IFCSURFACESTYLERENDERING(#{rgb},{transparency},$,$,$,$,$,$,.NOTDEFINED.)"
        ));
        let style = self.add(format!("IFCSURFACESTYLE($,.BOTH.,(#{render}))"));
        let assignment = self.add(format!("IFCPRESENTATIONSTYLEASSIGNMENT((#{style}))"));
        self.add(format!("IFCSTYLEDITEM(#{item},(#{assignment}),$)"));
    }

    fn faceted(&mut self, solid: &crate::Solid) -> Option<usize> {
        let mesh = geometry::mesh(solid, geometry::mesh_tolerance(solid));
        let (points, triangles) = geometry::welded(&mesh);
        if triangles.is_empty() {
            return None;
        }
        let coordinates = points
            .iter()
            .map(|p| format!("({},{},{})", real(p.x), real(p.y), real(p.z)))
            .collect::<Vec<_>>()
            .join(",");
        let list = self.add(format!("IFCCARTESIANPOINTLIST3D(({coordinates}))"));
        let indices = triangles
            .iter()
            .map(|[a, b, c]| format!("({},{},{})", a + 1, b + 1, c + 1))
            .collect::<Vec<_>>()
            .join(",");
        Some(self.add(format!(
            "IFCTRIANGULATEDFACESET(#{list},$,.T.,({indices}),$)"
        )))
    }
}

struct Element {
    id: usize,
    level: usize,
    spatial: bool,
}

fn level_of(levels: &[(String, f64)], z: f64) -> usize {
    levels
        .iter()
        .rposition(|(_, level)| *level <= z + 1.0)
        .unwrap_or(0)
}

fn class_of(op: &str) -> (&'static str, &'static str) {
    match op {
        "wall" => ("IFCWALL", ".NOTDEFINED."),
        "slab" | "ground_slab" => ("IFCSLAB", ".FLOOR."),
        "foundation" => ("IFCFOOTING", ".NOTDEFINED."),
        "ceiling" | "floor_finish" => ("IFCCOVERING", ".NOTDEFINED."),
        "roof" => ("IFCROOF", ".NOTDEFINED."),
        "stair" => ("IFCSTAIR", ".NOTDEFINED."),
        "ramp" => ("IFCRAMP", ".NOTDEFINED."),
        "railing" | "handrail" => ("IFCRAILING", ".NOTDEFINED."),
        "column" | "column_grid" => ("IFCCOLUMN", ".NOTDEFINED."),
        "beam" | "joist" | "truss" => ("IFCBEAM", ".NOTDEFINED."),
        "curtainwall" => ("IFCCURTAINWALL", ".NOTDEFINED."),
        _ => ("IFCBUILDINGELEMENTPROXY", ".NOTDEFINED."),
    }
}

pub fn write(model: &Model, project: &str) -> String {
    let arch = &model.arch;
    let mut w = Writer { lines: Vec::new() };
    let building = arch.building.clone().unwrap_or_default();
    let project_name = if building.name.is_empty() {
        project.to_string()
    } else {
        building.name.clone()
    };
    let origin = w.point([0.0, 0.0, 0.0]);
    let axes = w.add(format!("IFCAXIS2PLACEMENT3D(#{origin},$,$)"));
    let north = building.north.to_radians();
    let direction = w.add(format!(
        "IFCDIRECTION(({},{}))",
        real(-north.sin()),
        real(north.cos())
    ));
    let context = w.add(format!(
        "IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-5,#{axes},#{direction})"
    ));
    let length = w.add("IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)".into());
    let area = w.add("IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)".into());
    let volume = w.add("IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)".into());
    let angle = w.add("IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)".into());
    let units = w.add(format!(
        "IFCUNITASSIGNMENT({})",
        Writer::list(&[length, area, volume, angle])
    ));
    let project_id = w.add(format!(
        "IFCPROJECT('{}',$,{},$,$,$,$,(#{context}),#{units})",
        global_id("project", &project_name),
        text(&project_name)
    ));
    let site_place = w.place(None, [0.0, 0.0, 0.0]);
    let site = w.add(format!(
        "IFCSITE('{}',$,'Site',$,$,#{site_place},$,$,.ELEMENT.,$,$,$,$,$)",
        global_id("site", "site")
    ));
    let building_place = w.place(Some(site_place), [0.0, 0.0, building.datum]);
    let building_id = w.add(format!(
        "IFCBUILDING('{}',$,{},$,$,#{building_place},$,$,.ELEMENT.,$,$,$)",
        global_id("building", &project_name),
        text(&project_name)
    ));
    let mut levels: Vec<(String, f64)> = arch
        .levels
        .iter()
        .map(|l| (l.name.clone(), l.z))
        .collect();
    if levels.is_empty() {
        levels.push(("L0".into(), 0.0));
    }
    let mut storeys = Vec::new();
    for (k, level) in arch.levels.iter().enumerate() {
        let place = w.place(Some(building_place), [0.0, 0.0, level.z]);
        let title = if level.title.is_empty() {
            level.name.clone()
        } else {
            level.title.clone()
        };
        let id = w.add(format!(
            "IFCBUILDINGSTOREY('{}',$,{},$,$,#{place},$,{},.ELEMENT.,{})",
            global_id("storey", &level.name),
            text(&level.name),
            text(&title),
            real(level.z)
        ));
        storeys.push(id);
        let _ = k;
    }
    if storeys.is_empty() {
        let place = w.place(Some(building_place), [0.0, 0.0, 0.0]);
        storeys.push(w.add(format!(
            "IFCBUILDINGSTOREY('{}',$,'L0',$,$,#{place},$,$,.ELEMENT.,0.)",
            global_id("storey", "L0")
        )));
    }

    let mut materials: Vec<(String, usize)> = Vec::new();
    let body_of = |name: &str| model.named_solids().into_iter().find(|(n, _)| n == name);
    let mut elements: Vec<Element> = Vec::new();
    let mut by_name: Vec<(String, usize)> = Vec::new();

    let mut represent = |w: &mut Writer, bodies: &[String]| -> (String, f64) {
        let mut items = Vec::new();
        let mut low = f64::INFINITY;
        for name in bodies {
            if let Some((_, solid)) = body_of(name) {
                low = low.min(geometry::bounds(solid).min().z);
                if let Some(item) = w.faceted(solid) {
                    if let Some(colour) = model.colours.get(name) {
                        w.style(item, *colour, model.opacities.get(name).copied());
                    }
                    items.push(item);
                }
            }
        }
        if items.is_empty() {
            return ("$".into(), if low.is_finite() { low } else { 0.0 });
        }
        let shape = w.add(format!(
            "IFCSHAPEREPRESENTATION(#{context},'Body','Tessellation',{})",
            Writer::list(&items)
        ));
        let definition = w.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,({}))", format!("#{shape}")));
        (format!("#{definition}"), low)
    };

    let bodies_of = |owner: &str| -> Vec<String> {
        arch.built
            .iter()
            .find(|b| b.owner == owner)
            .map(|b| b.bodies.clone())
            .unwrap_or_default()
    };

    for (label, op) in &arch.classes {
        if matches!(op.as_str(), "door" | "window" | "garage_door" | "opening" | "room") {
            continue;
        }
        let bodies = bodies_of(label);
        let (class, predefined) = class_of(op);
        let units: Vec<(String, Vec<String>)> = if op == "column_grid" {
            bodies.iter().map(|b| (b.clone(), vec![b.clone()])).collect()
        } else if op == "stair" {
            vec![(label.clone(), Vec::new())]
        } else {
            vec![(label.clone(), bodies.clone())]
        };
        for (name, parts) in units {
            let (flights, rest): (Vec<String>, Vec<String>) = if op == "stair" {
                bodies.iter().cloned().partition(|b| b.contains(".flight."))
            } else {
                (Vec::new(), parts)
            };
            let (shape, low) = represent(&mut w, &rest);
            let low = if low.is_finite() && !rest.is_empty() {
                low
            } else {
                flights
                    .iter()
                    .filter_map(|f| body_of(f).map(|(_, s)| geometry::bounds(s).min().z))
                    .fold(f64::INFINITY, f64::min)
            };
            let low = if low.is_finite() { low } else { 0.0 };
            let place = w.place(Some(building_place), [0.0, 0.0, 0.0]);
            let id = w.add(format!(
                "{class}('{}',$,{},$,$,#{place},{shape},$,{predefined})",
                global_id(class, &name),
                text(&name)
            ));
            elements.push(Element {
                id,
                level: level_of(&levels, low),
                spatial: false,
            });
            by_name.push((name.clone(), id));
            if op == "wall"
                && let Some(wall) = arch.wall(&name)
            {
                let layers = &wall.tiers[0].layers;
                let ids: Vec<usize> = layers
                    .iter()
                    .map(|layer| {
                        let material = w.material(&mut materials, &layer.material);
                        w.add(format!(
                            "IFCMATERIALLAYER(#{material},{},$,{},$,$,$)",
                            real(layer.thickness),
                            text(&layer.material)
                        ))
                    })
                    .collect();
                let set = w.add(format!(
                    "IFCMATERIALLAYERSET({},{},$)",
                    Writer::list(&ids),
                    text(&name)
                ));
                let (lo, _) = wall.outer_span();
                let usage = w.add(format!(
                    "IFCMATERIALLAYERSETUSAGE(#{set},.AXIS2.,.POSITIVE.,{})",
                    real(lo)
                ));
                w.rel(
                    "IFCRELASSOCIATESMATERIAL",
                    &format!("{name}.material"),
                    format!("(#{id}),#{usage}"),
                );
                let conductivity: Option<Vec<f64>> = layers
                    .iter()
                    .map(|l| {
                        model
                            .library
                            .get(&crate::materials::slug(&l.material))
                            .and_then(|e| e.mechanics.conductivity)
                            .filter(|k| *k > 0.0)
                    })
                    .collect();
                let mut props = vec![w.property("Reference", &format!("IFCIDENTIFIER({})", text(&name)))];
                if let Some(k) = conductivity {
                    let resistance: f64 = 0.17
                        + layers
                            .iter()
                            .zip(&k)
                            .map(|(l, k)| l.thickness / 1000.0 / k)
                            .sum::<f64>();
                    props.push(w.property(
                        "ThermalTransmittance",
                        &format!("IFCTHERMALTRANSMITTANCEMEASURE({})", real(1.0 / resistance)),
                    ));
                }
                w.pset("Pset_WallCommon", &name, id, props);
            } else if matches!(class, "IFCSLAB" | "IFCROOF" | "IFCCOLUMN" | "IFCBEAM") {
                let props = vec![w.property("Reference", &format!("IFCIDENTIFIER({})", text(&name)))];
                let kind = match class {
                    "IFCSLAB" => "Pset_SlabCommon",
                    "IFCROOF" => "Pset_RoofCommon",
                    "IFCCOLUMN" => "Pset_ColumnCommon",
                    _ => "Pset_BeamCommon",
                };
                w.pset(kind, &name, id, props);
            }
            if !flights.is_empty() {
                let mut ids = Vec::new();
                for flight in &flights {
                    let (shape, _) = represent(&mut w, std::slice::from_ref(flight));
                    let place = w.place(Some(building_place), [0.0, 0.0, 0.0]);
                    ids.push(w.add(format!(
                        "IFCSTAIRFLIGHT('{}',$,{},$,$,#{place},{shape},$,$,$,$,$,.STRAIGHT.)",
                        global_id("IFCSTAIRFLIGHT", flight),
                        text(flight)
                    )));
                }
                w.rel(
                    "IFCRELAGGREGATES",
                    &format!("{name}.flights"),
                    format!("#{id},{}", Writer::list(&ids)),
                );
            }
        }
    }

    let hosts: Vec<(&Opening, usize)> = arch
        .openings
        .iter()
        .filter_map(|o| {
            by_name
                .iter()
                .find(|(n, _)| *n == o.host)
                .map(|(_, id)| (o, *id))
        })
        .collect();
    for (opening, host) in hosts {
        let level = elements
            .iter()
            .find(|e| e.id == host)
            .map_or(0, |e| e.level);
        let place = w.place(Some(building_place), [0.0, 0.0, 0.0]);
        let hole = w.add(format!(
            "IFCOPENINGELEMENT('{}',$,{},$,$,#{place},$,$,.OPENING.)",
            global_id("IFCOPENINGELEMENT", &opening.name),
            text(&format!("{} opening", opening.name))
        ));
        w.rel(
            "IFCRELVOIDSELEMENT",
            &opening.name,
            format!("#{host},#{hole}"),
        );
        let class = match opening.fill {
            Fill::Window { .. } => Some("IFCWINDOW"),
            Fill::Door { .. } | Fill::Garage { .. } | Fill::Part { .. } => Some("IFCDOOR"),
            Fill::Hole | Fill::Cut(_) => None,
        };
        let Some(class) = class else { continue };
        let (shape, _) = represent(&mut w, &bodies_of(&opening.name));
        let place = w.place(Some(building_place), [0.0, 0.0, 0.0]);
        let filler = w.add(format!(
            "{class}('{}',$,{},$,$,#{place},{shape},$,{},{},$,$,$)",
            global_id(class, &opening.name),
            text(&opening.name),
            real(opening.h),
            real(opening.w)
        ));
        w.rel(
            "IFCRELFILLSELEMENT",
            &opening.name,
            format!("#{hole},#{filler}"),
        );
        elements.push(Element {
            id: filler,
            level,
            spatial: false,
        });
        let mark = opening.mark.clone().unwrap_or_else(|| opening.name.clone());
        let props = vec![w.property("Reference", &format!("IFCIDENTIFIER({})", text(&mark)))];
        let kind = if class == "IFCWINDOW" {
            "Pset_WindowCommon"
        } else {
            "Pset_DoorCommon"
        };
        w.pset(kind, &opening.name, filler, props);
    }

    for room in &arch.rooms {
        let place = w.place(Some(building_place), [room.rect[0], room.rect[1], room.level]);
        let id = w.add(format!(
            "IFCSPACE('{}',$,{},$,$,#{place},$,{},.ELEMENT.,.INTERNAL.,$)",
            global_id("IFCSPACE", &room.name),
            text(&room.name),
            text(&room.title)
        ));
        elements.push(Element {
            id,
            level: level_of(&levels, room.level),
            spatial: true,
        });
        let props = vec![
            w.property("Reference", &format!("IFCIDENTIFIER({})", text(&room.number))),
            w.property("Category", &format!("IFCLABEL({})", text(&room.floor))),
        ];
        w.pset("Pset_SpaceCommon", &room.name, id, props);
        let mut quantity = |kind: &str, name: &str, value: f64| {
            w.add(format!("{kind}('{name}',$,$,{},$)", real(value)))
        };
        let quantities = [
            quantity("IFCQUANTITYAREA", "NetFloorArea", room.area()),
            quantity("IFCQUANTITYAREA", "GrossFloorArea", room.area()),
            quantity("IFCQUANTITYLENGTH", "Height", room.height),
            quantity("IFCQUANTITYVOLUME", "NetVolume", room.volume()),
        ];
        let set = w.add(format!(
            "IFCELEMENTQUANTITY('{}',$,'Qto_SpaceBaseQuantities',$,$,{})",
            global_id("quantities", &room.name),
            Writer::list(&quantities)
        ));
        w.rel(
            "IFCRELDEFINESBYPROPERTIES",
            &format!("{}.quantities", room.name),
            format!("(#{id}),#{set}"),
        );
    }

    for (k, storey) in storeys.iter().enumerate() {
        let contained: Vec<usize> = elements
            .iter()
            .filter(|e| e.level == k && !e.spatial)
            .map(|e| e.id)
            .collect();
        if !contained.is_empty() {
            w.rel(
                "IFCRELCONTAINEDINSPATIALSTRUCTURE",
                &format!("storey {k}"),
                format!("{},#{storey}", Writer::list(&contained)),
            );
        }
        let spaces: Vec<usize> = elements
            .iter()
            .filter(|e| e.level == k && e.spatial)
            .map(|e| e.id)
            .collect();
        if !spaces.is_empty() {
            w.rel(
                "IFCRELAGGREGATES",
                &format!("spaces {k}"),
                format!("#{storey},{}", Writer::list(&spaces)),
            );
        }
    }
    w.rel(
        "IFCRELAGGREGATES",
        "project",
        format!("#{project_id},(#{site})"),
    );
    w.rel("IFCRELAGGREGATES", "site", format!("#{site},(#{building_id})"));
    w.rel(
        "IFCRELAGGREGATES",
        "building",
        format!("#{building_id},{}", Writer::list(&storeys)),
    );

    let mut out = String::from("ISO-10303-21;\nHEADER;\n");
    out.push_str("FILE_DESCRIPTION(('ViewDefinition [CoordinationView]'),'2;1');\n");
    let _ = writeln!(
        out,
        "FILE_NAME({},'2000-01-01T00:00:00',(''),(''),'gcad','gcad','');",
        text(&format!("{project}.ifc"))
    );
    out.push_str("FILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n");
    for (k, line) in w.lines.iter().enumerate() {
        let _ = writeln!(out, "#{}={line};", k + 1);
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    out
}
