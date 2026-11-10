use crate::gltf;
use crate::interp::Scene;
use crate::mesh::rig::{Pose, Rig, Xf};
use crate::mesh::{Mesh, P3};
use crate::ops;
use crate::parse::{Line, Scope, eval, parse_program};
use crate::solid_ops::{self, Solids};
use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Options {
    pub detail: f64,
    pub y_up: bool,
    pub vars: Vec<(String, f64)>,
    pub triangles: Option<usize>,
    pub max_error: Option<f64>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            detail: 1.0,
            y_up: true,
            vars: Vec::new(),
            triangles: None,
            max_error: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Error {
    pub line: usize,
    pub text: String,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "line {} `{}`: {}", self.line, self.text, self.message)
        }
    }
}

impl std::error::Error for Error {}

pub type Matrix = [f32; 16];

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub colour: [f32; 3],
    pub gloss: f32,
    pub metal: f32,
    pub rough: f32,
    pub first_index: usize,
    pub index_count: usize,
}

#[derive(Clone, Debug)]
pub struct Joint {
    pub name: String,
    pub parent: Option<usize>,
    pub head: [f32; 3],
    pub tail: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct Track {
    pub joint: usize,
    pub times: Vec<f32>,
    pub rotations: Vec<[f32; 4]>,
}

#[derive(Clone, Debug)]
pub struct MoveTrack {
    pub joint: usize,
    pub times: Vec<f32>,
    pub offsets: Vec<[f32; 3]>,
}

#[derive(Clone, Debug)]
pub struct WeightTrack {
    pub times: Vec<f32>,
    pub weights: Vec<Vec<f32>>,
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    pub fps: f32,
    pub duration: f32,
    pub tracks: Vec<Track>,
    pub moves: Vec<MoveTrack>,
    pub weights: Option<WeightTrack>,
}

#[derive(Clone, Debug)]
pub struct MorphTarget {
    pub name: String,
    pub deltas: Vec<[f32; 3]>,
}

#[derive(Clone, Debug)]
pub struct NormalMap {
    pub size: usize,
    pub png: Vec<u8>,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub tangents: Vec<[f32; 4]>,
    pub source: Vec<u32>,
    pub indices: Vec<u32>,
    pub parts: Vec<Part>,
}

pub struct RawBody {
    pub name: String,
    pub mesh: Arc<Mesh>,
    pub rig: Option<Rig>,
    pub tints: Vec<[f64; 3]>,
    pub gloss: Vec<crate::interp::Shine>,
    pub colours: Vec<[f32; 3]>,
    pub detail: Option<crate::interp::Detail>,
}

#[derive(Clone, Debug)]
pub struct Body {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colours: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub parts: Vec<Part>,
    pub joints: Vec<Joint>,
    pub joint_indices: Vec<[u16; 4]>,
    pub joint_weights: Vec<[f32; 4]>,
    pub clips: Vec<Clip>,
    pub targets: Vec<MorphTarget>,
    pub target_weights: Vec<f32>,
    y_up: bool,
    mesh: Arc<Mesh>,
    rig: Option<Rig>,
    tints: Vec<[f64; 3]>,
    gloss: Vec<crate::interp::Shine>,
    detail: Option<crate::interp::Detail>,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub bodies: Vec<Body>,
}

fn to_up(p: P3, y_up: bool) -> P3 {
    if y_up { gltf::y_up(p) } else { p }
}

fn f3(p: P3) -> [f32; 3] {
    p.map(|v| v as f32)
}

fn matrix_of(xf: &Xf, y_up: bool) -> Matrix {
    let (r, t) = if y_up {
        let swap = |m: [[f64; 3]; 3]| -> [[f64; 3]; 3] {
            let rows = [m[0], m[2], [-m[1][0], -m[1][1], -m[1][2]]];
            std::array::from_fn(|i| [rows[i][0], rows[i][2], -rows[i][1]])
        };
        (swap(xf.r), gltf::y_up(xf.t))
    } else {
        (xf.r, xf.t)
    };
    let mut out = [0.0f32; 16];
    for c in 0..3 {
        for row in 0..3 {
            out[c * 4 + row] = r[row][c] as f32;
        }
        out[12 + c] = t[c] as f32;
    }
    out[15] = 1.0;
    out
}

impl Body {
    pub fn new(raw: RawBody, y_up: bool) -> Body {
        body_of(raw, y_up)
    }

    pub fn rest_mesh(&self) -> &Mesh {
        &self.mesh
    }

    pub fn rig(&self) -> Option<&Rig> {
        self.rig.as_ref()
    }

    pub fn detail(&self) -> Option<&crate::interp::Detail> {
        self.detail.as_ref()
    }

    pub fn set_lods(&mut self, lods: Vec<crate::interp::LodSize>) {
        let detail = self.detail.get_or_insert_with(Default::default);
        detail.lods = lods;
        if detail.is_empty() && detail.full.is_none() {
            self.detail = None;
        }
    }

    pub fn set_bake_size(&mut self, size: usize) -> bool {
        match self.detail.as_mut().and_then(|d| d.bake.as_mut()) {
            Some(bake) => {
                *bake = size;
                true
            }
            None => false,
        }
    }

    pub fn lods(&self) -> Vec<Body> {
        let Some(detail) = &self.detail else {
            return Vec::new();
        };
        let skin = self.rig.as_ref().and_then(|r| r.skin.as_ref());
        gltf::lod_meshes(&self.mesh, detail)
            .into_iter()
            .enumerate()
            .map(|(k, mesh)| {
                let rig = self.rig.as_ref().map(|r| Rig {
                    skin: skin.map(|s| gltf::nearest_weights(s, &self.mesh, &mesh)),
                    correctives: Vec::new(),
                    ..r.clone()
                });
                body_of(
                    RawBody {
                        name: format!("{}_LOD{}", self.name, k + 1),
                        mesh: Arc::new(mesh),
                        rig,
                        tints: self.tints.clone(),
                        gloss: self.gloss.clone(),
                        colours: Vec::new(),
                        detail: None,
                    },
                    self.y_up,
                )
            })
            .collect()
    }

    pub fn normal_map(&self) -> Option<NormalMap> {
        let detail = self.detail.as_ref()?;
        let size = detail.bake?;
        let high = detail.full.as_deref().unwrap_or(&self.mesh);
        let baked = crate::mesh::bake::bake_normals(&self.mesh, high, size);
        let e = &baked.expanded;
        let labels: Vec<u16> = e
            .source
            .iter()
            .map(|&i| self.mesh.labels[i as usize])
            .collect();
        let (indices, parts) = parts_of(
            &self.mesh.parts,
            &e.triangles,
            &labels,
            &self.tints,
            &self.gloss,
        );
        Some(NormalMap {
            size,
            png: baked.png(),
            positions: e
                .positions
                .iter()
                .map(|p| f3(to_up(*p, self.y_up)))
                .collect(),
            normals: e.normals.iter().map(|n| f3(to_up(*n, self.y_up))).collect(),
            uvs: e.uvs.iter().map(|q| [q[0] as f32, q[1] as f32]).collect(),
            tangents: e
                .tangents
                .iter()
                .map(|t| {
                    let v = f3(to_up([t[0], t[1], t[2]], self.y_up));
                    [v[0], v[1], v[2], t[3] as f32]
                })
                .collect(),
            source: e.source.clone(),
            indices,
            parts,
        })
    }

    pub fn joint_count(&self) -> usize {
        self.joints.len()
    }

    pub fn clip_index(&self, name: &str) -> Option<usize> {
        self.clips.iter().position(|c| c.name == name)
    }

    pub fn bone_matrices(&self, clip: Option<usize>, t: f64) -> Vec<Matrix> {
        let Some(rig) = &self.rig else {
            return Vec::new();
        };
        let mut posed = rig.clone();
        match clip {
            Some(c) if c < rig.clips.len() => {
                posed.pose = rig.sample(c, t);
                posed.moved = rig.sample_moves(c, t);
            }
            _ => {
                posed.pose = Pose::new();
                posed.moved.clear();
            }
        }
        let pose = posed.pose.clone();
        posed
            .skin_transforms(&pose)
            .iter()
            .map(|xf| matrix_of(xf, self.y_up))
            .collect()
    }

    pub fn skinned_positions(&self, matrices: &[Matrix]) -> Vec<[f32; 3]> {
        self.positions
            .iter()
            .zip(self.joint_indices.iter().zip(&self.joint_weights))
            .map(|(p, (joints, weights))| {
                let mut out = [0.0f32; 3];
                for (j, w) in joints.iter().zip(weights) {
                    if *w == 0.0 {
                        continue;
                    }
                    let Some(m) = matrices.get(*j as usize) else {
                        continue;
                    };
                    for (row, slot) in out.iter_mut().enumerate() {
                        *slot += w
                            * (m[row] * p[0] + m[4 + row] * p[1] + m[8 + row] * p[2] + m[12 + row]);
                    }
                }
                out
            })
            .collect()
    }
}

impl Model {
    pub fn body(&self, name: &str) -> Option<&Body> {
        self.bodies.iter().find(|b| b.name == name)
    }

    pub fn to_glb(&self) -> Result<Vec<u8>> {
        let bodies: Vec<gltf::Body<'_>> = self
            .bodies
            .iter()
            .map(|b| gltf::Body {
                name: b.name.clone(),
                mesh: &b.mesh,
                rig: b.rig.as_ref(),
                colours: b.tints.clone(),
                gloss: b.gloss.clone(),
                detail: b.detail.clone(),
            })
            .collect();
        gltf::glb(&bodies)
    }
}

struct Run {
    solids: Solids,
    scene: Scene,
    scope: Scope,
    fixed: Scope,
    colours: HashMap<String, [f64; 3]>,
}

const SUBSET: &[&str] = &[
    "let",
    "if",
    "blob",
    "limb",
    "block",
    "remesh",
    "select",
    "inflate",
    "smooth",
    "refine",
    "crease",
    "pinch",
    "flatten",
    "noise",
    "relax",
    "decimate",
    "bone",
    "corrective",
    "skin",
    "pose",
    "clip",
    "key",
    "move",
    "rotate",
    "scale",
    "mirror",
    "body",
    "color",
    "brush",
    "stroke",
    "dab",
    "mask",
    "hide",
    "isolate",
    "show",
    "group",
    "join",
    "plate",
    "dyntopo",
    "subdivide",
    "level",
    "project",
    "layer",
    "finish",
    "bake",
    "lod",
];

impl Run {
    fn op_let(&mut self, line: &Line) -> Result<()> {
        if !line.positional.is_empty() {
            bail!("`let` takes name=value pairs only");
        }
        for (name, text) in &line.named {
            let value = match self.fixed.get(name) {
                Some(fixed) => *fixed,
                None => eval(text, &self.scope).map_err(|e| anyhow!("`{name}`: {e:#}"))?,
            };
            self.scope.insert(name.clone(), value);
        }
        Ok(())
    }

    fn op_body(&mut self, line: &Line) -> Result<()> {
        let [name] = line.positional.as_slice() else {
            bail!("`body` takes one name");
        };
        if !line.named.is_empty() {
            bail!("`body` takes one name");
        }
        let scene = &mut self.scene;
        if scene.body == *name || scene.mesh_bodies.iter().any(|(n, _)| n == name) {
            bail!("there is already a body called `{name}`");
        }
        scene.finish_mesh_body()?;
        scene.body = name.clone();
        self.solids.new_body();
        Ok(())
    }

    fn apply(&mut self, line: &Line) -> Result<()> {
        if !SUBSET.contains(&line.op.as_str()) && !solid_ops::OPS.contains(&line.op.as_str()) {
            bail!(
                "`{}` is not a mesh operation; gcad-mesh runs {}",
                line.op,
                SUBSET
                    .iter()
                    .chain(solid_ops::OPS)
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if solid_ops::OPS.contains(&line.op.as_str()) {
            return self.solids.apply(&mut self.scene, &self.scope, line);
        }
        if !crate::rig_ops::SCULPT_OPS.contains(&line.op.as_str())
            && line.op != "let"
            && line.op != "if"
        {
            self.scene.close_corrective()?;
        }
        let scope = &self.scope;
        let scene = &mut self.scene;
        match line.op.as_str() {
            "let" => return self.op_let(line),
            "body" => return self.op_body(line),
            "if" => {
                return match ops::guard(line, scope)? {
                    Ok(inner) => self.apply(&inner),
                    Err(_) => Ok(()),
                };
            }
            "blob" => scene.op_blob(scope, line),
            "limb" => scene.op_limb(scope, line),
            "block" => scene.op_block(scope, line),
            "remesh" => scene.op_remesh(scope, line),
            "select" => scene.op_select(scope, line),
            "inflate" => scene.op_inflate(scope, line),
            "smooth" => scene.op_smooth(scope, line),
            "refine" => scene.op_refine(scope, line),
            "crease" => scene.op_crease(scope, line),
            "pinch" => scene.op_pinch(scope, line),
            "flatten" => scene.op_flatten(scope, line),
            "noise" => scene.op_noise(scope, line),
            "relax" => scene.op_relax(scope, line),
            "decimate" => scene.op_decimate(scope, line),
            "bone" => scene.op_bone(scope, line),
            "corrective" => scene.op_corrective(scope, line),
            "skin" => scene.op_skin(scope, line),
            "pose" => scene.op_mesh_pose(scope, line),
            "clip" => scene.op_clip(scope, line),
            "key" => scene.op_key(scope, line),
            "brush" => scene.op_brush(scope, line),
            "stroke" => scene.op_stroke(scope, line),
            "dab" => scene.op_dab(scope, line),
            "mask" => scene.op_mask(scope, line),
            "hide" | "isolate" | "show" => scene.op_hide(scope, line),
            "group" => scene.op_group(scope, line),
            "join" => scene.op_join(scope, line),
            "plate" => scene.op_plate(scope, line),
            "dyntopo" => scene.op_dyntopo(scope, line),
            "subdivide" => scene.op_subdivide(scope, line),
            "level" => scene.op_level(scope, line),
            "project" => scene.op_project(scope, line),
            "layer" => scene.op_layer(scope, line),
            "finish" => scene.op_finish(scope, line),
            "bake" => scene.op_bake(scope, line),
            "lod" => scene.op_lod(scope, line),
            "move" => scene.op_move(scope, line),
            "rotate" => scene.op_rotate(scope, line, |_| None),
            "scale" => scene.op_scale(scope, line),
            "mirror" => scene.op_mirror(scope, line),
            "color" => scene.op_color(scope, line, &mut self.colours),
            other => bail!("`{other}` is not a mesh operation"),
        }
        .map(|_| ())
    }
}

fn failure(line: &Line, error: anyhow::Error) -> Error {
    Error {
        line: line.number,
        text: line.text.chars().take(80).collect(),
        message: format!("{error:#}"),
    }
}

pub fn build(source: &str) -> std::result::Result<Model, Error> {
    build_with(source, &Options::default())
}

pub fn build_with(source: &str, options: &Options) -> std::result::Result<Model, Error> {
    if !(options.detail > 0.0 && options.detail.is_finite()) {
        return Err(Error {
            line: 0,
            text: String::new(),
            message: format!("detail must be above 0, got {}", options.detail),
        });
    }
    let lines = parse_program(source).map_err(|e| {
        let message = format!("{e:#}");
        let line = message
            .strip_prefix("line ")
            .and_then(|rest| rest.split(':').next())
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(0);
        Error {
            line,
            text: source
                .lines()
                .nth(line.saturating_sub(1))
                .unwrap_or_default()
                .trim()
                .chars()
                .take(80)
                .collect(),
            message,
        }
    })?;
    let fixed: Scope = options.vars.iter().cloned().collect();
    let mut run = Run {
        solids: Solids::default(),
        scene: Scene {
            cell_scale: options.detail,
            ..Default::default()
        },
        scope: fixed.clone(),
        fixed,
        colours: HashMap::new(),
    };
    for line in &lines {
        run.apply(line).map_err(|e| failure(line, e))?;
    }
    run.scene.finish_mesh_body().map_err(|e| Error {
        line: 0,
        text: String::new(),
        message: format!("{e:#}"),
    })?;
    run.scene.budget(options.triangles, options.max_error);
    Ok(collect(&run, options.y_up))
}

fn collect(run: &Run, y_up: bool) -> Model {
    let scene = &run.scene;
    let rest = scene.rest_meshes();
    let tints = scene.mesh_tints(&run.colours);
    let shading = scene.mesh_shading(&run.colours);
    let bodies = rest
        .into_iter()
        .map(|(name, mesh)| {
            let part_tints = tints
                .iter()
                .find(|(n, ..)| *n == name)
                .map(|(_, _, t)| t.clone())
                .unwrap_or_default();
            let gloss = scene.part_shine_of(&name, &mesh);
            let vertex_colours = shading
                .iter()
                .find(|(n, ..)| *n == name)
                .map(|(_, _, c, _)| c.iter().map(|c| f3(*c)).collect())
                .unwrap_or_default();
            body_of(
                RawBody {
                    name: name.clone(),
                    mesh,
                    rig: scene.rig_of(&name).cloned(),
                    tints: part_tints,
                    gloss,
                    colours: vertex_colours,
                    detail: scene.detail_of(&name),
                },
                y_up,
            )
        })
        .collect();
    Model { bodies }
}

fn parts_of(
    names: &[String],
    triangles: &[[u32; 3]],
    labels: &[u16],
    tints: &[[f64; 3]],
    gloss: &[crate::interp::Shine],
) -> (Vec<u32>, Vec<Part>) {
    let mut indices = Vec::new();
    let mut parts = Vec::new();
    for (part, part_name) in names.iter().enumerate() {
        let first = indices.len();
        indices.extend(
            triangles
                .iter()
                .filter(|t| labels[t[0] as usize] as usize == part)
                .flatten(),
        );
        if indices.len() == first {
            continue;
        }
        let shine = gloss.get(part).copied().unwrap_or_default();
        parts.push(Part {
            name: part_name.clone(),
            colour: f3(tints.get(part).copied().unwrap_or([0.7; 3])),
            gloss: shine.gloss as f32,
            metal: shine.metal as f32,
            rough: shine.rough as f32,
            first_index: first,
            index_count: indices.len() - first,
        });
    }
    (indices, parts)
}

fn body_of(raw: RawBody, y_up: bool) -> Body {
    let RawBody {
        name,
        mesh,
        rig,
        tints,
        gloss,
        colours,
        detail,
    } = raw;
    let positions = mesh.positions.iter().map(|p| f3(to_up(*p, y_up))).collect();
    let normals = mesh
        .vertex_normals()
        .into_iter()
        .map(|n| f3(to_up(n, y_up)))
        .collect();
    let (indices, parts) = parts_of(&mesh.parts, &mesh.triangles, &mesh.labels, &tints, &gloss);
    let active = rig.filter(|r| r.skin_fits(&mesh));
    let mut joints = Vec::new();
    let mut joint_indices = Vec::new();
    let mut joint_weights = Vec::new();
    let mut clips = Vec::new();
    let mut targets = Vec::new();
    let mut target_weights = Vec::new();
    if let Some(rig) = &active {
        targets = rig
            .correctives
            .iter()
            .map(|c| MorphTarget {
                name: c.name.clone(),
                deltas: c.delta.iter().map(|d| f3(to_up(*d, y_up))).collect(),
            })
            .collect();
        if !targets.is_empty() {
            target_weights = rig
                .corrective_weights(&rig.pose)
                .into_iter()
                .map(|w| w as f32)
                .collect();
        }
        joints = rig
            .bones
            .iter()
            .map(|b| Joint {
                name: b.name.clone(),
                parent: b.parent,
                head: f3(to_up(b.head, y_up)),
                tail: f3(to_up(b.tail, y_up)),
            })
            .collect();
        let (j, w) = gltf::float_weights(rig, mesh.positions.len());
        joint_indices = j.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
        joint_weights = w.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
        clips = rig
            .clips
            .iter()
            .enumerate()
            .map(|(clip_index, clip)| Clip {
                name: clip.name.clone(),
                fps: clip.fps as f32,
                duration: clip.duration() as f32,
                tracks: (0..rig.bones.len())
                    .filter_map(|bone| {
                        let keys = rig.rotation_track(clip_index, bone);
                        if keys.is_empty() {
                            return None;
                        }
                        Some(Track {
                            joint: bone,
                            times: keys.iter().map(|k| k.0 as f32).collect(),
                            rotations: keys
                                .iter()
                                .map(|k| {
                                    let q = if y_up { gltf::quat_y_up(k.1) } else { k.1 };
                                    let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
                                    let n = if n > 0.0 { n } else { 1.0 };
                                    q.map(|v| (v / n) as f32)
                                })
                                .collect(),
                        })
                    })
                    .collect(),
                moves: (0..rig.bones.len())
                    .filter_map(|bone| {
                        let keys = clip.bone_moves(bone);
                        if keys.is_empty() {
                            return None;
                        }
                        let rest = rig.local_offset(bone);
                        Some(MoveTrack {
                            joint: bone,
                            times: keys.iter().map(|k| k.t as f32).collect(),
                            offsets: keys
                                .iter()
                                .map(|k| f3(to_up(crate::mesh::add(rest, k.offset), y_up)))
                                .collect(),
                        })
                    })
                    .collect(),
                weights: (!rig.correctives.is_empty()).then(|| {
                    let times = gltf::corrective_times(clip);
                    WeightTrack {
                        weights: times
                            .iter()
                            .map(|t| {
                                rig.corrective_weights(&rig.sample(clip_index, *t))
                                    .into_iter()
                                    .map(|w| w as f32)
                                    .collect()
                            })
                            .collect(),
                        times: times.iter().map(|t| *t as f32).collect(),
                    }
                }),
            })
            .collect();
    }
    Body {
        name,
        positions,
        normals,
        colours,
        indices,
        parts,
        joints,
        joint_indices,
        joint_weights,
        clips,
        targets,
        target_weights,
        y_up,
        mesh,
        rig: active,
        tints,
        gloss,
        detail,
    }
}
