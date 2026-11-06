use crate::mesh::rig::{IDENTITY_QUAT, Rig};
use crate::mesh::{Mesh, P3};
use anyhow::Result;
use serde_json::{Value, json};

pub struct Body<'a> {
    pub name: String,
    pub mesh: &'a Mesh,
    pub rig: Option<&'a Rig>,
    pub colours: Vec<[f64; 3]>,
    pub gloss: Vec<f64>,
}

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;

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

#[derive(Default)]
struct Builder {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Builder {
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let mut view =
            json!({"buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len()});
        if let Some(t) = target {
            view["target"] = json!(t);
        }
        self.bin.extend_from_slice(bytes);
        self.views.push(view);
        self.views.len() - 1
    }

    fn accessor(
        &mut self,
        bytes: &[u8],
        target: Option<u32>,
        component: u32,
        count: usize,
        kind: &str,
        bounds: Option<(Vec<f64>, Vec<f64>)>,
    ) -> usize {
        let view = self.view(bytes, target);
        let mut a =
            json!({"bufferView": view, "componentType": component, "count": count, "type": kind});
        if let Some((lo, hi)) = bounds {
            a["min"] = json!(lo);
            a["max"] = json!(hi);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    fn floats(
        &mut self,
        values: &[f32],
        width: usize,
        kind: &str,
        target: Option<u32>,
        bounds: bool,
    ) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let b = bounds.then(|| {
            let mut lo = vec![f64::INFINITY; width];
            let mut hi = vec![f64::NEG_INFINITY; width];
            for chunk in values.chunks(width) {
                for (k, v) in chunk.iter().enumerate() {
                    lo[k] = lo[k].min(*v as f64);
                    hi[k] = hi[k].max(*v as f64);
                }
            }
            (lo, hi)
        });
        self.accessor(&bytes, target, FLOAT, values.len() / width, kind, b)
    }
}

fn flat(points: impl Iterator<Item = P3>) -> Vec<f32> {
    points.flat_map(|p| p.map(|v| v as f32)).collect()
}

pub(crate) fn float_weights(rig: &Rig, vertices: usize) -> (Vec<u16>, Vec<f32>) {
    let skin = rig.skin.as_ref().expect("skinned");
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

pub fn glb(bodies: &[Body<'_>]) -> Result<Vec<u8>> {
    let mut b = Builder::default();
    let mut nodes: Vec<Value> = Vec::new();
    let mut scene: Vec<usize> = Vec::new();
    let mut meshes: Vec<Value> = Vec::new();
    let mut skins: Vec<Value> = Vec::new();
    let mut materials: Vec<Value> = Vec::new();
    let mut animations: Vec<Value> = Vec::new();
    for body in bodies {
        let mesh = body.mesh;
        let positions = flat(mesh.positions.iter().map(|p| y_up(*p)));
        let normals = flat(mesh.vertex_normals().into_iter().map(y_up));
        let position = b.floats(&positions, 3, "VEC3", Some(ARRAY_BUFFER), true);
        let normal = b.floats(&normals, 3, "VEC3", Some(ARRAY_BUFFER), false);
        let mut attributes = json!({"POSITION": position, "NORMAL": normal});
        let rig = body.rig.filter(|r| r.skin_fits(mesh));
        if let Some(rig) = rig {
            let (joints, weights) = float_weights(rig, mesh.positions.len());
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
        let mut primitives = Vec::new();
        for (part, name) in mesh.parts.iter().enumerate() {
            let indices: Vec<u32> = mesh
                .triangles
                .iter()
                .filter(|t| mesh.labels[t[0] as usize] as usize == part)
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
            let gloss = body.gloss.get(part).copied().unwrap_or(0.0);
            materials.push(json!({
                "name": format!("{}.{name}", body.name),
                "pbrMetallicRoughness": {
                    "baseColorFactor": [linear(colour[0]), linear(colour[1]), linear(colour[2]), 1.0],
                    "metallicFactor": 0.0,
                    "roughnessFactor": 1.0 - gloss * 0.8,
                },
            }));
            primitives.push(json!({
                "attributes": attributes,
                "indices": accessor,
                "material": materials.len() - 1,
                "mode": 4,
            }));
        }
        meshes.push(json!({"name": body.name, "primitives": primitives}));
        let mesh_index = meshes.len() - 1;
        let mut node = json!({"name": body.name, "mesh": mesh_index});
        if let Some(rig) = rig {
            let base = nodes.len();
            for (i, bone) in rig.bones.iter().enumerate() {
                let q = quat_y_up(rig.pose.get(&i).copied().unwrap_or(IDENTITY_QUAT));
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
            for clip in &rig.clips {
                let mut samplers = Vec::new();
                let mut channels = Vec::new();
                for bone in 0..rig.bones.len() {
                    let keys = clip.bone_keys(bone);
                    if keys.is_empty() {
                        continue;
                    }
                    let times: Vec<f32> = keys.iter().map(|k| k.t as f32).collect();
                    let rotations: Vec<f32> = keys
                        .iter()
                        .flat_map(|k| {
                            let q = quat_y_up(k.rotation);
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
                if !channels.is_empty() {
                    animations.push(json!({
                        "name": clip.name,
                        "samplers": samplers,
                        "channels": channels,
                    }));
                }
            }
        }
        nodes.push(node);
        scene.push(nodes.len() - 1);
    }
    let mut root = json!({
        "asset": {"version": "2.0", "generator": "gcad"},
        "scene": 0,
        "scenes": [{"nodes": scene}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "buffers": [{"byteLength": b.bin.len().div_ceil(4) * 4}],
        "bufferViews": b.views,
        "accessors": b.accessors,
    });
    if !skins.is_empty() {
        root["skins"] = json!(skins);
    }
    if !animations.is_empty() {
        root["animations"] = json!(animations);
    }
    let mut json_bytes = serde_json::to_vec(&root)?;
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    while !b.bin.len().is_multiple_of(4) {
        b.bin.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + b.bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend((total as u32).to_le_bytes());
    out.extend((json_bytes.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(&json_bytes);
    out.extend((b.bin.len() as u32).to_le_bytes());
    out.extend(b"BIN\0");
    out.extend(&b.bin);
    Ok(out)
}
