use crate::glb::{ARRAY_BUFFER, Buffer, ELEMENT_ARRAY_BUFFER, UNSIGNED_INT, UNSIGNED_SHORT};
use crate::interp::Detail;
use crate::mesh::bake::bake_normals;
use crate::mesh::rig::{IDENTITY_QUAT, Rig, Weights};
use crate::mesh::uv::Expanded;
use crate::mesh::{Mesh, P3, length};
use anyhow::Result;
use serde_json::{Value, json};

pub struct Body<'a> {
    pub name: String,
    pub mesh: &'a Mesh,
    pub rig: Option<&'a Rig>,
    pub colours: Vec<[f64; 3]>,
    pub gloss: Vec<crate::interp::Shine>,
    pub detail: Option<Detail>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fit {
    pub height: Option<f64>,
    pub scale: Option<f64>,
    pub centre: bool,
}

impl Fit {
    pub fn is_none(&self) -> bool {
        *self == Fit::default()
    }
}

fn chunks(glb: &[u8]) -> Result<(Value, &[u8])> {
    let word = |at: usize| -> Result<usize> {
        let bytes = glb
            .get(at..at + 4)
            .ok_or_else(|| anyhow::anyhow!("GLB is truncated"))?;
        Ok(u32::from_le_bytes(bytes.try_into()?) as usize)
    };
    anyhow::ensure!(glb.starts_with(b"glTF"), "not a GLB file");
    let n = word(12)?;
    anyhow::ensure!(&glb[16..20] == b"JSON", "GLB has no JSON chunk first");
    let json = glb
        .get(20..20 + n)
        .ok_or_else(|| anyhow::anyhow!("GLB is truncated"))?;
    Ok((serde_json::from_slice(json)?, &glb[20 + n..]))
}

fn position_bounds(doc: &Value) -> Option<(P3, P3)> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for mesh in doc["meshes"].as_array()? {
        for primitive in mesh["primitives"].as_array()? {
            let index = primitive["attributes"]["POSITION"].as_u64()? as usize;
            let accessor = &doc["accessors"][index];
            for k in 0..3 {
                lo[k] = lo[k].min(accessor["min"][k].as_f64()?);
                hi[k] = hi[k].max(accessor["max"][k].as_f64()?);
            }
        }
    }
    lo[0].is_finite().then_some((lo, hi))
}

pub fn fit(glb: &[u8], fit: &Fit) -> Result<Vec<u8>> {
    anyhow::ensure!(
        fit.height.is_none() || fit.scale.is_none(),
        "give a height or a scale, not both"
    );
    let (mut doc, rest) = chunks(glb)?;
    let (lo, hi) = position_bounds(&doc)
        .ok_or_else(|| anyhow::anyhow!("the GLB has no mesh positions with bounds to fit"))?;
    let k = match (fit.height, fit.scale) {
        (Some(h), _) => {
            anyhow::ensure!(h > 0.0 && h.is_finite(), "height must be above 0");
            anyhow::ensure!(hi[1] > lo[1], "the model has no height to scale");
            h / (hi[1] - lo[1])
        }
        (None, Some(s)) => {
            anyhow::ensure!(s > 0.0 && s.is_finite(), "scale must be above 0");
            s
        }
        (None, None) => 1.0,
    };
    let mid = |i: usize| match fit.centre {
        true => -(lo[i] + hi[i]) / 2.0 * k,
        false => 0.0,
    };
    let scene = doc["scene"].as_u64().unwrap_or(0) as usize;
    let roots = doc["scenes"][scene]["nodes"].take();
    let nodes = doc["nodes"]
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("the GLB has no nodes"))?;
    nodes.push(json!({
        "name": "model",
        "children": roots,
        "scale": [k, k, k],
        "translation": [mid(0), -lo[1] * k, mid(2)],
    }));
    doc["scenes"][scene]["nodes"] = json!([nodes.len() - 1]);
    let mut json_bytes = serde_json::to_vec(&doc)?;
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let total = 12 + 8 + json_bytes.len() + rest.len();
    let mut out = Vec::with_capacity(total);
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend((total as u32).to_le_bytes());
    out.extend((json_bytes.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(&json_bytes);
    out.extend(rest);
    Ok(out)
}

pub(crate) fn y_up(p: P3) -> P3 {
    [p[0], p[2], -p[1]]
}

fn linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub(crate) fn quat_y_up(q: [f64; 4]) -> [f64; 4] {
    [q[0], q[2], -q[1], q[3]]
}

fn flat(points: impl Iterator<Item = P3>) -> Vec<f32> {
    points.flat_map(|p| p.map(|v| v as f32)).collect()
}

pub(crate) fn float_weights(rig: &Rig, vertices: usize) -> (Vec<u16>, Vec<f32>) {
    weights_of(rig.skin.as_ref().expect("skinned"), vertices)
}

fn weights_of(skin: &Weights, vertices: usize) -> (Vec<u16>, Vec<f32>) {
    let mut joints = vec![0u16; vertices * 4];
    let mut weights = vec![0f32; vertices * 4];
    for (i, influences) in skin.iter().enumerate().take(vertices) {
        let mut stored = [0f32; 4];
        for (k, (bone, w)) in influences.iter().take(4).enumerate() {
            joints[i * 4 + k] = *bone as u16;
            stored[k] = *w as f32;
        }
        let sum: f32 = stored.iter().sum();
        if sum <= 0.0 {
            stored[0] = 1.0;
        } else {
            let first = stored.iter().position(|w| *w > 0.0).unwrap_or(0);
            let rest: f32 = stored
                .iter()
                .enumerate()
                .filter(|(k, _)| *k != first)
                .map(|(_, w)| w)
                .sum();
            stored[first] = 1.0 - rest;
        }
        weights[i * 4..i * 4 + 4].copy_from_slice(&stored);
    }
    (joints, weights)
}

type Flattened<'a> = (Vec<P3>, Vec<P3>, Vec<u16>, &'a [[u32; 3]], Vec<u32>);

struct Level {
    targets: Vec<Vec<P3>>,
    mesh: Mesh,
    skin: Option<Weights>,
    map: Option<Vec<u8>>,
    expanded: Option<Expanded>,
}

fn size_of(spec: crate::interp::LodSize, full: usize) -> usize {
    match spec {
        crate::interp::LodSize::Triangles(n) => n,
        crate::interp::LodSize::Share(share) => (full as f64 * share).round() as usize,
    }
    .max(4)
}

pub fn lod_meshes(base: &Mesh, detail: &Detail) -> Vec<Mesh> {
    let source = detail.full.as_deref().unwrap_or(base);
    detail
        .lods
        .iter()
        .map(|spec| {
            let mut level = source.clone();
            let target = size_of(*spec, source.triangles.len());
            level.finish(source, Some(target), None);
            level
        })
        .collect()
}

pub(crate) fn nearest_weights(skin: &Weights, from: &Mesh, to: &Mesh) -> Weights {
    let grid = crate::mesh::grid::Grid::new(&from.positions, 1.0);
    let cell = from
        .bounds()
        .map(|(lo, hi)| (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max) / 64.0)
        .unwrap_or(1.0);
    let grid = if cell > 0.0 {
        crate::mesh::grid::Grid::new(&from.positions, cell)
    } else {
        grid
    };
    to.positions
        .iter()
        .map(|p| {
            grid.nearest(&from.positions, *p)
                .map(|(i, _)| skin[i as usize].clone())
                .unwrap_or_default()
        })
        .collect()
}

fn levels_of(body: &Body<'_>, rig: Option<&Rig>) -> Vec<Level> {
    let skin = rig.and_then(|r| r.skin.clone());
    let targets = rig
        .map(|r| r.correctives.iter().map(|c| c.delta.clone()).collect())
        .unwrap_or_default();
    let mut first = Level {
        targets,
        mesh: body.mesh.clone(),
        skin: skin.clone(),
        map: None,
        expanded: None,
    };
    let detail = body.detail.as_ref();
    if let Some(size) = detail.and_then(|d| d.bake) {
        let high = detail.and_then(|d| d.full.as_deref()).unwrap_or(body.mesh);
        let baked = bake_normals(body.mesh, high, size);
        first.map = Some(baked.png());
        first.expanded = Some(baked.expanded);
    }
    let mut out = vec![first];
    if let Some(detail) = detail {
        for mesh in lod_meshes(body.mesh, detail) {
            let weights = skin.as_ref().map(|s| nearest_weights(s, body.mesh, &mesh));
            out.push(Level {
                targets: Vec::new(),
                mesh,
                skin: weights,
                map: None,
                expanded: None,
            });
        }
    }
    out
}

fn emit_level(
    b: &mut Buffer,
    materials: &mut Vec<Value>,
    images: &mut Vec<Value>,
    textures: &mut Vec<Value>,
    body: &Body<'_>,
    level: &Level,
    name: &str,
) -> Value {
    let mesh = &level.mesh;
    let smooth;
    let (positions, normals, labels, triangles, source): Flattened = match &level.expanded {
        Some(e) => (
            e.positions.clone(),
            e.normals.clone(),
            e.source.iter().map(|&i| mesh.labels[i as usize]).collect(),
            &e.triangles,
            e.source.clone(),
        ),
        None => {
            smooth = mesh.vertex_normals();
            (
                mesh.positions.clone(),
                smooth,
                mesh.labels.clone(),
                &mesh.triangles,
                (0..mesh.positions.len() as u32).collect(),
            )
        }
    };
    let normals: Vec<P3> = normals
        .into_iter()
        .map(|n| if length(n) < 0.5 { [0.0, 0.0, 1.0] } else { n })
        .collect();
    let position = b.floats(
        &flat(positions.iter().map(|p| y_up(*p))),
        3,
        "VEC3",
        Some(ARRAY_BUFFER),
        true,
    );
    let normal = b.floats(
        &flat(normals.iter().map(|p| y_up(*p))),
        3,
        "VEC3",
        Some(ARRAY_BUFFER),
        false,
    );
    let mut attributes = json!({"POSITION": position, "NORMAL": normal});
    let mut normal_texture = None;
    if let (Some(e), Some(png)) = (&level.expanded, &level.map) {
        let uvs: Vec<f32> = e
            .uvs
            .iter()
            .flat_map(|q| [q[0] as f32, q[1] as f32])
            .collect();
        attributes["TEXCOORD_0"] = json!(b.floats(&uvs, 2, "VEC2", Some(ARRAY_BUFFER), false));
        let tangents: Vec<f32> = e
            .tangents
            .iter()
            .flat_map(|t| {
                let v = y_up([t[0], t[1], t[2]]);
                [v[0] as f32, v[1] as f32, v[2] as f32, t[3] as f32]
            })
            .collect();
        attributes["TANGENT"] = json!(b.floats(&tangents, 4, "VEC4", Some(ARRAY_BUFFER), false));
        let view = b.view(png, None);
        images.push(json!({"name": format!("{}.normal", body.name), "bufferView": view, "mimeType": "image/png"}));
        textures.push(json!({"sampler": 0, "source": images.len() - 1}));
        normal_texture = Some(textures.len() - 1);
    }
    if let Some(skin) = &level.skin {
        let per_vertex: Weights = source.iter().map(|&i| skin[i as usize].clone()).collect();
        let (joints, weights) = weights_of(&per_vertex, per_vertex.len());
        let jbytes: Vec<u8> = joints.iter().flat_map(|v| v.to_le_bytes()).collect();
        attributes["JOINTS_0"] = json!(b.accessor(
            &jbytes,
            Some(ARRAY_BUFFER),
            UNSIGNED_SHORT,
            joints.len() / 4,
            "VEC4",
            None
        ));
        attributes["WEIGHTS_0"] = json!(b.floats(&weights, 4, "VEC4", Some(ARRAY_BUFFER), false));
    }
    let targets: Vec<Value> = level
        .targets
        .iter()
        .map(|delta| {
            let moved: Vec<P3> = source.iter().map(|&i| delta[i as usize]).collect();
            json!({"POSITION": b.floats(&flat(moved.iter().map(|p| y_up(*p))), 3, "VEC3", Some(ARRAY_BUFFER), true)})
        })
        .collect();
    let mut primitives = Vec::new();
    for (part, part_name) in mesh.parts.iter().enumerate() {
        let indices: Vec<u32> = triangles
            .iter()
            .filter(|t| labels[t[0] as usize] as usize == part)
            .flat_map(|t| *t)
            .collect();
        if indices.is_empty() {
            continue;
        }
        let bytes: Vec<u8> = indices.iter().flat_map(|v| v.to_le_bytes()).collect();
        let accessor = b.accessor(
            &bytes,
            Some(ELEMENT_ARRAY_BUFFER),
            UNSIGNED_INT,
            indices.len(),
            "SCALAR",
            None,
        );
        let colour = body.colours.get(part).copied().unwrap_or([0.7, 0.7, 0.7]);
        let shine = body.gloss.get(part).copied().unwrap_or_default();
        let mut material = json!({
            "name": format!("{name}.{part_name}"),
            "pbrMetallicRoughness": {
                "baseColorFactor": [linear(colour[0]), linear(colour[1]), linear(colour[2]), 1.0],
                "metallicFactor": shine.metal,
                "roughnessFactor": shine.rough,
            },
        });
        if let Some(t) = normal_texture {
            material["normalTexture"] = json!({"index": t});
        }
        materials.push(material);
        let mut primitive = json!({
            "attributes": attributes,
            "indices": accessor,
            "material": materials.len() - 1,
            "mode": 4,
        });
        if !targets.is_empty() {
            primitive["targets"] = json!(targets);
        }
        primitives.push(primitive);
    }
    json!({"name": name, "primitives": primitives})
}

fn emit_fur(
    b: &mut Buffer,
    materials: &mut Vec<Value>,
    images: &mut Vec<Value>,
    textures: &mut Vec<Value>,
    body: &Body<'_>,
    rig: Option<&Rig>,
    mesh_json: &mut Value,
) {
    let Some(detail) = body.detail.as_ref().filter(|d| !d.fur.is_empty()) else {
        return;
    };
    if rig.is_some_and(|r| !r.correctives.is_empty()) {
        return;
    }
    let skin = rig.and_then(|r| r.skin.as_ref());
    let mut texture = None;
    for (k, spec) in detail.fur.iter().enumerate() {
        let Ok(cards) = spec.grow(body.mesh) else {
            continue;
        };
        if cards.is_empty() {
            continue;
        }
        let tint = spec.tint(body.mesh, &body.colours);
        let quads = cards.quads();
        let positions = flat(quads.positions.iter().map(|p| y_up(*p)));
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        for c in 0..cards.len() {
            for _ in 0..4 {
                normals.push(cards.surface[c]);
            }
            uvs.extend([0.0f32, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0]);
        }
        let mut attributes = json!({
            "POSITION": b.floats(&positions, 3, "VEC3", Some(ARRAY_BUFFER), true),
            "NORMAL": b.floats(&flat(normals.iter().map(|p| y_up(*p))), 3, "VEC3", Some(ARRAY_BUFFER), false),
            "TEXCOORD_0": b.floats(&uvs, 2, "VEC2", Some(ARRAY_BUFFER), false),
        });
        if let Some(skin) = skin.filter(|s| s.len() == body.mesh.positions.len()) {
            let per_vertex: Weights = cards
                .anchor
                .iter()
                .flat_map(|&a| std::iter::repeat_n(skin[a as usize].clone(), 4))
                .collect();
            let (joints, weights) = weights_of(&per_vertex, per_vertex.len());
            let jbytes: Vec<u8> = joints.iter().flat_map(|v| v.to_le_bytes()).collect();
            attributes["JOINTS_0"] = json!(b.accessor(
                &jbytes,
                Some(ARRAY_BUFFER),
                UNSIGNED_SHORT,
                joints.len() / 4,
                "VEC4",
                None
            ));
            attributes["WEIGHTS_0"] =
                json!(b.floats(&weights, 4, "VEC4", Some(ARRAY_BUFFER), false));
        }
        let indices: Vec<u32> = quads.triangles.iter().flatten().copied().collect();
        let bytes: Vec<u8> = indices.iter().flat_map(|v| v.to_le_bytes()).collect();
        let accessor = b.accessor(
            &bytes,
            Some(ELEMENT_ARRAY_BUFFER),
            UNSIGNED_INT,
            indices.len(),
            "SCALAR",
            None,
        );
        let texture = *texture.get_or_insert_with(|| {
            let view = b.view(&crate::mesh::fur::strand_png(), None);
            images.push(json!({"name": format!("{}.strand", body.name), "bufferView": view, "mimeType": "image/png"}));
            textures.push(json!({"sampler": 0, "source": images.len() - 1}));
            textures.len() - 1
        });
        materials.push(json!({
            "name": format!("{}.fur{}", body.name, k),
            "alphaMode": "MASK",
            "alphaCutoff": 0.5,
            "doubleSided": true,
            "pbrMetallicRoughness": {
                "baseColorFactor": [linear(tint[0]), linear(tint[1]), linear(tint[2]), 1.0],
                "baseColorTexture": {"index": texture},
                "metallicFactor": 0.0,
                "roughnessFactor": 1.0,
            },
        }));
        if let Some(list) = mesh_json["primitives"].as_array_mut() {
            list.push(json!({
                "attributes": attributes,
                "indices": accessor,
                "material": materials.len() - 1,
                "mode": 4,
            }));
        }
    }
}

pub(crate) fn corrective_times(clip: &crate::mesh::rig::Clip) -> Vec<f64> {
    let duration = clip.duration();
    let frames = (duration * clip.fps).ceil() as usize;
    let mut times: Vec<f64> = (0..=frames).map(|k| k as f64 / clip.fps).collect();
    times.retain(|t| *t < duration - 1.0e-9);
    times.push(duration);
    times.extend(clip.keys.iter().map(|k| k.t));
    times.sort_by(f64::total_cmp);
    times.dedup_by(|a, b| (*a - *b).abs() < 1.0e-9);
    times
}

pub fn glb(bodies: &[Body<'_>]) -> Result<Vec<u8>> {
    let mut b = Buffer::default();
    let mut nodes: Vec<Value> = Vec::new();
    let mut scene: Vec<usize> = Vec::new();
    let mut meshes: Vec<Value> = Vec::new();
    let mut skins: Vec<Value> = Vec::new();
    let mut materials: Vec<Value> = Vec::new();
    let mut animations: Vec<Value> = Vec::new();
    let mut images: Vec<Value> = Vec::new();
    let mut textures: Vec<Value> = Vec::new();
    for body in bodies {
        let rig = body.rig.filter(|r| r.skin_fits(body.mesh));
        let levels = levels_of(body, rig);
        let mut mesh_indices = Vec::new();
        for (k, level) in levels.iter().enumerate() {
            let name = if k == 0 {
                body.name.clone()
            } else {
                format!("{}_LOD{k}", body.name)
            };
            let json = emit_level(
                &mut b,
                &mut materials,
                &mut images,
                &mut textures,
                body,
                level,
                &name,
            );
            meshes.push(json);
            mesh_indices.push(meshes.len() - 1);
            if k == 0 {
                emit_fur(
                    &mut b,
                    &mut materials,
                    &mut images,
                    &mut textures,
                    body,
                    rig,
                    &mut meshes[mesh_indices[0]],
                );
            }
        }
        let mesh_index = mesh_indices[0];
        let mut node = json!({"name": body.name, "mesh": mesh_index});
        if let Some(rig) = rig.filter(|r| !r.correctives.is_empty()) {
            let weights = rig.corrective_weights(&rig.pose);
            meshes[mesh_index]["weights"] = json!(weights);
            meshes[mesh_index]["extras"] = json!({"targetNames": rig.correctives.iter().map(|c| c.name.clone()).collect::<Vec<_>>()});
        }
        let mut lod_nodes = Vec::new();
        for (k, mesh_index) in mesh_indices.iter().enumerate().skip(1) {
            lod_nodes.push((format!("{}_LOD{k}", body.name), *mesh_index));
        }
        if let Some(rig) = rig {
            let base = nodes.len();
            let rest_pose = rig.effective(&rig.pose);
            for (i, bone) in rig.bones.iter().enumerate() {
                let q = quat_y_up(rest_pose.get(&i).copied().unwrap_or(IDENTITY_QUAT));
                let n = (q.iter().map(|v| v * v).sum::<f64>()).sqrt();
                let mut joint = json!({
                    "name": bone.name,
                    "translation": y_up(rig.posed_offset(i)),
                    "rotation": q.map(|v| v / n),
                });
                let children: Vec<usize> = rig.children(i).into_iter().map(|c| base + c).collect();
                if !children.is_empty() {
                    joint["children"] = json!(children);
                }
                nodes.push(joint);
                if bone.parent.is_none() {
                    scene.push(base + i);
                }
            }
            let mut inverse = Vec::new();
            for bone in &rig.bones {
                let h = y_up(bone.head);
                inverse.extend([
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.0,
                    -h[0] as f32,
                    -h[1] as f32,
                    -h[2] as f32,
                    1.0,
                ]);
            }
            let ibm = b.floats(&inverse, 16, "MAT4", None, false);
            skins.push(json!({
                "name": body.name,
                "inverseBindMatrices": ibm,
                "joints": (0..rig.bones.len()).map(|i| base + i).collect::<Vec<_>>(),
                "skeleton": rig.bones.iter().position(|b| b.parent.is_none()).map(|r| base + r),
            }));
            node["skin"] = json!(skins.len() - 1);
            for (clip_index, clip) in rig.clips.iter().enumerate() {
                let mut samplers = Vec::new();
                let mut channels = Vec::new();
                for bone in 0..rig.bones.len() {
                    let rotations_of = rig.rotation_track(clip_index, bone);
                    if rotations_of.is_empty() {
                        continue;
                    }
                    let times: Vec<f32> = rotations_of.iter().map(|k| k.0 as f32).collect();
                    let rotations: Vec<f32> = rotations_of
                        .iter()
                        .flat_map(|k| {
                            let q = quat_y_up(k.1);
                            let n = (q.iter().map(|v| v * v).sum::<f64>()).sqrt();
                            q.map(|v| (v / n) as f32)
                        })
                        .collect();
                    let input = b.floats(&times, 1, "SCALAR", None, true);
                    let output = b.floats(&rotations, 4, "VEC4", None, false);
                    samplers
                        .push(json!({"input": input, "output": output, "interpolation": "LINEAR"}));
                    channels.push(json!({
                        "sampler": samplers.len() - 1,
                        "target": {"node": base + bone, "path": "rotation"},
                    }));
                }
                for bone in 0..rig.bones.len() {
                    let keys = clip.bone_moves(bone);
                    if keys.is_empty() {
                        continue;
                    }
                    let times: Vec<f32> = keys.iter().map(|k| k.t as f32).collect();
                    let rest = rig.local_offset(bone);
                    let offsets: Vec<f32> = keys
                        .iter()
                        .flat_map(|k| y_up(crate::mesh::add(rest, k.offset)).map(|v| v as f32))
                        .collect();
                    let input = b.floats(&times, 1, "SCALAR", None, true);
                    let output = b.floats(&offsets, 3, "VEC3", None, false);
                    samplers
                        .push(json!({"input": input, "output": output, "interpolation": "LINEAR"}));
                    channels.push(json!({
                        "sampler": samplers.len() - 1,
                        "target": {"node": base + bone, "path": "translation"},
                    }));
                }
                if !rig.correctives.is_empty() {
                    let times = corrective_times(clip);
                    let mut values: Vec<f32> = Vec::new();
                    for t in &times {
                        let pose = rig.sample(clip_index, *t);
                        values.extend(rig.corrective_weights(&pose).iter().map(|w| *w as f32));
                    }
                    let float_times: Vec<f32> = times.iter().map(|t| *t as f32).collect();
                    let input = b.floats(&float_times, 1, "SCALAR", None, true);
                    let output = b.floats(&values, 1, "SCALAR", None, false);
                    samplers
                        .push(json!({"input": input, "output": output, "interpolation": "LINEAR"}));
                    channels.push(json!({
                        "sampler": samplers.len() - 1,
                        "target": {"node": nodes.len(), "path": "weights"},
                    }));
                }
                if !channels.is_empty() {
                    animations.push(json!({
                        "name": clip.name,
                        "samplers": samplers,
                        "channels": channels,
                    }));
                }
            }
        }
        if !lod_nodes.is_empty() {
            let first = nodes.len() + 1;
            let ids: Vec<usize> = (0..lod_nodes.len()).map(|k| first + k).collect();
            node["extensions"] = json!({"MSFT_lod": {"ids": ids}});
            node["extras"] = json!({"lod": 0});
        }
        nodes.push(node);
        scene.push(nodes.len() - 1);
        for (k, (name, mesh_index)) in lod_nodes.iter().enumerate() {
            let mut lod = json!({"name": name, "mesh": mesh_index, "extras": {"lod": k + 1}});
            if let Some(skin) = nodes[nodes.len() - 1 - k]["skin"].as_u64() {
                lod["skin"] = json!(skin);
            }
            nodes.push(lod);
        }
    }
    let mut root = json!({
        "asset": {"version": "2.0", "generator": "gcad"},
        "scene": 0,
        "scenes": [{"nodes": scene}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
    });
    if !images.is_empty() {
        root["images"] = json!(images);
        root["textures"] = json!(textures);
        root["samplers"] =
            json!([{"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071}]);
    }
    if nodes.iter().any(|n| n.get("extensions").is_some()) {
        root["extensionsUsed"] = json!(["MSFT_lod"]);
    }
    if !skins.is_empty() {
        root["skins"] = json!(skins);
    }
    if !animations.is_empty() {
        root["animations"] = json!(animations);
    }
    b.finish(root)
}
