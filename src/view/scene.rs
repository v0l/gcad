use crate::geometry;
use crate::model::Model;
use egui_bench::viewer3d::{self, Material, Shading};
use monstertruck::meshing::prelude::*;
use monstertruck::modeling::*;
use std::sync::Arc;

pub use egui_bench::viewer3d::V3;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Ink {
    #[default]
    Body,
    Edge,
    Marked,
}

impl Material for Ink {
    fn shading(&self) -> Shading {
        match self {
            Ink::Body => Shading {
                capped: true,
                ..Shading::default()
            },
            Ink::Edge => Shading::default(),
            Ink::Marked => Shading {
                tinted: false,
                ..Shading::default()
            },
        }
    }
}

pub type Surface = viewer3d::Surface<Ink>;
pub type Lines = viewer3d::Lines<Ink>;

#[derive(Clone)]
pub struct Part {
    pub name: String,
    pub colour: V3,
    pub volume: f64,
    pub current: bool,
    pub source: Option<String>,
    pub material: Option<(String, f64)>,
    pub size: [f64; 3],
    pub hidden: bool,
    pub role: Role,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    Body,
    Field,
    Streams,
    Slice { index: usize, moment: bool },
}

#[derive(Clone)]
pub struct FaceInfo {
    pub body: usize,
    pub kind: String,
    pub area: f64,
}

pub struct Scene {
    pub mesh: Arc<viewer3d::Scene<Ink>>,
    pub parts: Vec<Part>,
    pub vertices: Vec<(usize, V3)>,
    pub faces: Vec<FaceInfo>,
}

pub struct Highlight<'a> {
    pub faces: &'a [usize],
    pub colour: V3,
}

pub const BODY: V3 = [0.62, 0.66, 0.72];
pub const EDGE: V3 = [0.05, 0.06, 0.07];
pub const EDGE_WIDTH: f32 = 1.5;
pub const MARKED_WIDTH: f32 = 3.5;
pub const WIND: V3 = [0.45, 0.75, 1.0];

fn v3(p: Point3) -> V3 {
    [p.x as f32, p.y as f32, p.z as f32]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: V3) -> V3 {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2])
        .sqrt()
        .max(1.0e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn face_colour(index: usize, base: V3, highlights: &[Highlight<'_>]) -> V3 {
    highlights
        .iter()
        .rev()
        .find(|h| h.faces.contains(&index))
        .map_or(base, |h| h.colour)
}

fn describe(surface: &monstertruck::modeling::Surface) -> String {
    match surface {
        monstertruck::modeling::Surface::Plane(plane) => {
            let n = plane.normal();
            let tidy = |v: f64| if v.abs() < 5.0e-4 { 0.0 } else { v };
            format!(
                "flat, normal {:.3}, {:.3}, {:.3}",
                tidy(n.x),
                tidy(n.y),
                tidy(n.z)
            )
        }
        _ => "curved".to_string(),
    }
}

type Meshed = monstertruck::topology::Solid<Point3, PolylineCurve<Point3>, Option<PolygonMesh>>;

fn faces(
    solid: &Solid,
    meshed: &Meshed,
    body: usize,
    base: V3,
    highlights: &[Highlight<'_>],
    out: &mut Vec<Surface>,
    info: &mut Vec<FaceInfo>,
) -> f64 {
    let originals: Vec<Face> = crate::select::faces(solid);
    let faces = meshed
        .boundaries()
        .iter()
        .flat_map(|shell| shell.face_iter().cloned())
        .collect::<Vec<_>>();
    let mut volume = 0.0;
    for (index, face) in faces.iter().enumerate() {
        let Some(mesh) = face.surface() else { continue };
        let mut surface = Surface {
            colour: face_colour(index, base, highlights),
            group: body,
            ..Default::default()
        };
        let positions = mesh.positions();
        let normals = mesh.normals();
        let mut area = 0.0;
        let outward = face.orientation();
        for triangle in mesh.faces().triangle_iter() {
            let triangle = if outward {
                triangle
            } else {
                [triangle[0], triangle[2], triangle[1]]
            };
            let corners = triangle.map(|v| v3(positions[v.pos]));
            let [a, b, c] = triangle.map(|v| positions[v.pos]);
            area += (b - a).cross(c - a).magnitude() / 2.0;
            volume += a.to_vec().dot(b.to_vec().cross(c.to_vec())) / 6.0;
            let flat = norm(cross(
                sub(corners[1], corners[0]),
                sub(corners[2], corners[0]),
            ));
            for (vertex, corner) in triangle.iter().zip(corners) {
                surface.positions.push(corner);
                let n = vertex.nor.and_then(|i| normals.get(i)).map_or(flat, |n| {
                    let n = norm([n.x as f32, n.y as f32, n.z as f32]);
                    if outward { n } else { [-n[0], -n[1], -n[2]] }
                });
                surface.normals.push(n);
            }
        }
        info.push(FaceInfo {
            body,
            kind: originals
                .get(index)
                .map_or_else(|| "face".to_string(), |f| describe(&f.oriented_surface())),
            area,
        });
        out.push(surface);
    }
    volume
}

fn polyline(edge: &Edge) -> Vec<V3> {
    geometry::curve_samples(&edge.curve())
        .into_iter()
        .map(v3)
        .collect()
}

fn segments(points: &[V3], out: &mut Vec<[V3; 2]>) {
    out.extend(
        points
            .windows(2)
            .filter(|pair| sub(pair[1], pair[0]).iter().any(|c| c.abs() > 1.0e-9))
            .map(|pair| [pair[0], pair[1]]),
    );
}

fn bend(
    surface: &monstertruck::modeling::Surface,
    at: Point3,
    across: Vector3,
) -> Option<(Vector3, f64)> {
    let (u, v) = surface.search_nearest_parameter(at, None, 100)?;
    let (su, sv) = (surface.uder(u, v), surface.vder(u, v));
    let n = surface.normal(u, v);
    let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let det = e * g - f * f;
    if det.abs() < 1.0e-18 {
        return None;
    }
    let (p, q) = (across.dot(su), across.dot(sv));
    let (a, b) = ((g * p - f * q) / det, (e * q - f * p) / det);
    let first = su * a + sv * b;
    let second = surface.uuder(u, v) * (a * a)
        + surface.uvder(u, v) * (2.0 * a * b)
        + surface.vvder(u, v) * (b * b);
    Some((n, second.dot(n) / first.magnitude2().max(1.0e-18)))
}

fn smooth(edge: &Edge, a: &Face, b: &Face) -> bool {
    let (sa, sb) = (a.oriented_surface(), b.oriented_surface());
    let curve = edge.curve();
    let (t0, t1) = curve.range_tuple();
    let h = (t1 - t0) * 1.0e-4;
    [0.25, 0.5, 0.75].iter().all(|&s| {
        let t = t0 + (t1 - t0) * s;
        let at = curve.subs(t);
        let tangent = curve.subs(t + h) - curve.subs(t - h);
        if tangent.magnitude2() < 1.0e-30 {
            return false;
        }
        let Some((na, _)) = bend(&sa, at, tangent) else {
            return false;
        };
        let across = na.cross(tangent).normalize();
        match (bend(&sa, at, across), bend(&sb, at, across)) {
            (Some((na, ka)), Some((nb, kb))) => {
                na.dot(nb) > 1.0 - 1.0e-6
                    && (ka - kb).abs() <= 1.0e-3 * (ka.abs() + kb.abs()) + 1.0e-9
            }
            _ => false,
        }
    })
}

fn edge_lines(
    solid: &Solid,
    meshed: &Meshed,
    skip: &std::collections::HashSet<EdgeId>,
    out: &mut Vec<[V3; 2]>,
) {
    for (shell, drawn) in solid.boundaries().iter().zip(meshed.boundaries()) {
        let mut owners: std::collections::HashMap<EdgeId, Vec<Face>> = Default::default();
        for face in shell.face_iter() {
            for edge in face.edge_iter() {
                owners.entry(edge.id()).or_default().push(face.clone());
            }
        }
        let mut seen = std::collections::HashSet::new();
        for (edge, polyline) in shell.edge_iter().zip(drawn.edge_iter()) {
            if !seen.insert(edge.id()) || skip.contains(&edge.id()) {
                continue;
            }
            if let Some([a, b]) = owners.get(&edge.id()).map(Vec::as_slice)
                && a.id() != b.id()
                && smooth(&edge, a, b)
            {
                continue;
            }
            let points: Vec<V3> = polyline.curve().iter().map(|p| v3(*p)).collect();
            segments(&points, out);
        }
    }
}

fn describe_source(source: &crate::model::Source) -> String {
    let file = source.file.file_name().map_or_else(
        || source.file.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let mut text = file;
    if !source.body.is_empty() {
        text.push_str(&format!(" body={}", source.body));
    }
    for (name, value) in &source.vars {
        text.push_str(&format!(" {name}={value}"));
    }
    text
}

pub fn build(
    model: &Model,
    highlights: &[Highlight<'_>],
    marked: &[Edge],
    marked_colour: V3,
) -> Scene {
    let current = model.current_body();
    let bodies: Vec<(String, &Solid)> = model
        .bodies
        .iter()
        .map(|(name, solid)| (name.clone(), solid))
        .chain(model.solid.as_ref().map(|solid| (current.clone(), solid)))
        .collect();
    let bounds: BoundingBox<Point3> = bodies
        .iter()
        .flat_map(|(_, s)| {
            let b = geometry::bounds(s);
            [b.min(), b.max()]
        })
        .collect();
    let radius = (bounds.diameter() / 2.0).max(1.0e-3) as f32;
    let mut surfaces = Vec::new();
    let mut lines = Vec::new();
    let mut info = Vec::new();
    let mut parts = Vec::new();
    let mut vertices = Vec::new();
    let skip: std::collections::HashSet<_> = marked.iter().map(|m| m.id()).collect();
    let built: Vec<_> = {
        use rayon::prelude::*;
        bodies
            .par_iter()
            .enumerate()
            .map(|(body, (name, solid))| {
                let meshed = solid.robust_triangulation(geometry::mesh_tolerance(solid) * 0.5);
                let is_current = *name == current && model.solid.is_some();
                let colour = model
                    .colours
                    .get(name)
                    .map_or(BODY, |c| c.map(|v| v as f32));
                let own: &[Highlight<'_>] = if is_current { highlights } else { &[] };
                let mut surfaces = Vec::new();
                let mut info = Vec::new();
                let volume = faces(solid, &meshed, body, colour, own, &mut surfaces, &mut info);
                let mut edges = Lines {
                    colour: EDGE,
                    material: Ink::Edge,
                    group: body,
                    width: EDGE_WIDTH,
                    ..Default::default()
                };
                edge_lines(solid, &meshed, &skip, &mut edges.segments);
                let mut seen = std::collections::HashSet::new();
                let vertices: Vec<(usize, V3)> = solid
                    .boundaries()
                    .iter()
                    .flat_map(|shell| shell.vertex_iter())
                    .filter(|v| seen.insert(v.id()))
                    .map(|v| (body, v3(v.point())))
                    .collect();
                let extent = geometry::bounds(solid);
                let (low, high) = (extent.min(), extent.max());
                let part = Part {
                    name: name.clone(),
                    colour,
                    volume,
                    current: is_current,
                    source: model.sources.get(name).map(describe_source),
                    material: model
                        .materials
                        .get(name)
                        .map(|m| (m.name.clone(), m.density)),
                    size: [high.x - low.x, high.y - low.y, high.z - low.z],
                    hidden: false,
                    role: Role::Body,
                };
                (surfaces, info, edges, vertices, part)
            })
            .collect()
    };
    for (body_surfaces, body_info, edges, body_vertices, part) in built {
        surfaces.extend(body_surfaces);
        info.extend(body_info);
        lines.push(edges);
        vertices.extend(body_vertices);
        parts.push(part);
    }
    let current_index = parts.iter().position(|p| p.current).unwrap_or(0);
    let mut highlighted = Lines {
        colour: marked_colour,
        material: Ink::Marked,
        group: current_index,
        width: MARKED_WIDTH,
        ..Default::default()
    };
    marked
        .iter()
        .for_each(|edge| segments(&polyline(edge), &mut highlighted.segments));
    lines.push(highlighted);
    Scene {
        mesh: Arc::new(viewer3d::Scene {
            surfaces,
            lines,
            centre: v3(bounds.center()),
            radius,
            ..Default::default()
        }),
        parts,
        vertices,
        faces: info,
    }
}

pub fn build_field(view: &crate::sim::statics::FieldView) -> Scene {
    let plain = view.values.is_empty();
    let mut surface = Surface {
        colour: if plain { BODY } else { [1.0, 1.0, 1.0] },
        texture: (!plain).then_some(0),
        group: 0,
        ..Default::default()
    };
    for t in &view.triangles {
        let corners = t.map(|v| v3(view.positions[v as usize]));
        let flat = norm(cross(
            sub(corners[1], corners[0]),
            sub(corners[2], corners[0]),
        ));
        for (k, corner) in corners.iter().enumerate() {
            surface.positions.push(*corner);
            surface.normals.push(flat);
            surface
                .uvs
                .push([view.shade[t[k] as usize].clamp(0.002, 0.998) as f32, 0.5]);
        }
    }
    let mut rgba = Vec::with_capacity(256 * 4);
    for k in 0..256 {
        let c = crate::plot::heat(k as f64 / 255.0);
        rgba.extend(c.map(|v| (v * 255.0) as u8));
        rgba.push(255);
    }
    let bounds: BoundingBox<Point3> = view.positions.iter().copied().collect();
    let low = bounds.min();
    let high = bounds.max();
    let short = |v: f64| {
        if v.abs() >= 100.0 {
            crate::plot::label(v.round())
        } else {
            crate::plot::label((v * 100.0).round() / 100.0)
        }
    };
    let name = if plain {
        view.title.clone()
    } else {
        format!(
            "{} ({} to {} {})",
            view.title.trim_start_matches("mean "),
            short(view.low),
            short(view.high),
            view.unit
        )
    };
    let point = |v: u32| v3(view.positions[v as usize]);
    let curve = |edges: &[[u32; 3]], colour: V3, width: f32| {
        let mut out = Lines {
            colour,
            material: Ink::Edge,
            group: 0,
            width,
            ..Default::default()
        };
        for [a, m, b] in edges {
            out.segments.push([point(*a), point(*m)]);
            out.segments.push([point(*m), point(*b)]);
        }
        out
    };
    let mut lines = vec![curve(&view.edges, EDGE, EDGE_WIDTH)];
    for mark in &view.marks {
        let colour = match mark.kind {
            crate::sim::statics::Stimulus::Push => [1.0, 0.31, 0.78],
            crate::sim::statics::Stimulus::Hold => [0.95, 0.95, 0.95],
            crate::sim::statics::Stimulus::Heat => [1.0, 0.67, 0.24],
            crate::sim::statics::Stimulus::Wind => WIND,
        };
        lines.push(curve(&mark.outline, colour, MARKED_WIDTH));
    }
    let mut surfaces = vec![surface];
    let mut images = vec![viewer3d::Image {
        width: 256,
        height: 1,
        rgba,
    }];
    let mut parts = vec![Part {
        name,
        colour: if plain {
            BODY
        } else {
            crate::plot::heat(1.0).map(|v| v as f32)
        },
        volume: 0.0,
        current: true,
        source: None,
        material: None,
        size: [high.x - low.x, high.y - low.y, high.z - low.z],
        hidden: false,
        role: Role::Field,
    }];
    let fastest = crate::plot::flow_scale(&view.slices);
    if !view.streams.is_empty() {
        let group = parts.len();
        let bins = 24;
        let mut bands: Vec<Lines> = (0..bins)
            .map(|b| Lines {
                colour: crate::plot::heat((b as f64 + 0.5) / bins as f64).map(|v| v as f32),
                material: Ink::Edge,
                group,
                width: 1.6,
                ..Default::default()
            })
            .collect();
        for stream in &view.streams {
            for (pair, speeds) in stream.points.windows(2).zip(stream.speed.windows(2)) {
                let t = 0.5 * (speeds[0] + speeds[1]) / fastest;
                let band = ((t * bins as f64) as usize).min(bins - 1);
                bands[band]
                    .segments
                    .push([pair[0].map(|v| v as f32), pair[1].map(|v| v as f32)]);
            }
        }
        lines.extend(bands);
        parts.push(Part {
            name: format!(
                "streamlines (0 to {} m/s)",
                crate::plot::label(fastest.round())
            ),
            colour: crate::plot::heat(0.8).map(|v| v as f32),
            volume: 0.0,
            current: false,
            source: None,
            material: None,
            size: [0.0; 3],
            hidden: false,
            role: Role::Streams,
        });
    }
    for (index, slice) in view.slices.iter().enumerate() {
        for (moment, values) in [(false, &slice.speed), (true, &slice.snapshot)] {
            let mut rgba = Vec::with_capacity(values.len() * 4);
            for v in values.iter() {
                let c = if *v < 0.0 {
                    [0.59, 0.59, 0.59]
                } else {
                    crate::plot::heat(v / fastest)
                };
                rgba.extend(c.map(|x| (x * 255.0) as u8));
                rgba.push(255);
            }
            images.push(viewer3d::Image {
                width: slice.width,
                height: slice.height(),
                rgba,
            });
            let group = parts.len();
            surfaces.push(slice_quad(slice, images.len() - 1, group));
            parts.push(Part {
                name: format!(
                    "{}, {}",
                    slice
                        .title
                        .trim_end_matches(" along the wind")
                        .trim_end_matches(" along the axis"),
                    if moment { "moment" } else { "mean" }
                ),
                colour: crate::plot::heat(0.5).map(|v| v as f32),
                volume: 0.0,
                current: false,
                source: None,
                material: None,
                size: [0.0; 3],
                hidden: true,
                role: Role::Slice { index, moment },
            });
        }
    }
    Scene {
        mesh: Arc::new(viewer3d::Scene {
            surfaces,
            lines,
            images,
            centre: v3(bounds.center()),
            radius: (bounds.diameter() / 2.0).max(1.0e-3) as f32,
            ..Default::default()
        }),
        parts,
        vertices: Vec::new(),
        faces: Vec::new(),
    }
}

fn slice_quad(slice: &crate::sim::Slice, texture: usize, group: usize) -> Surface {
    let o = slice.origin;
    let (right, down) = (slice.right, slice.down);
    let (w, h) = (slice.width as f64, slice.height() as f64);
    let at =
        |c: f64, r: f64| -> V3 { [0, 1, 2].map(|k| (o[k] + c * right[k] + r * down[k]) as f32) };
    let corners = [
        (at(-0.5, -0.5), [0.0, 0.0]),
        (at(w - 0.5, -0.5), [1.0, 0.0]),
        (at(w - 0.5, h - 0.5), [1.0, 1.0]),
        (at(-0.5, h - 0.5), [0.0, 1.0]),
    ];
    let normal = norm(cross(
        sub(corners[1].0, corners[0].0),
        sub(corners[3].0, corners[0].0),
    ));
    let back = normal.map(|v| -v);
    let mut surface = Surface {
        colour: [1.0, 1.0, 1.0],
        texture: Some(texture),
        group,
        ..Default::default()
    };
    for (triangle, n) in [
        ([0, 1, 2], normal),
        ([0, 2, 3], normal),
        ([0, 2, 1], back),
        ([0, 3, 2], back),
    ] {
        for k in triangle {
            surface.positions.push(corners[k].0);
            surface.normals.push(n);
            surface.uvs.push(corners[k].1);
        }
    }
    surface
}
