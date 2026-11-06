use crate::gltf;
use crate::interp::Scene;
use crate::mesh::rig::{Pose, Rig, Xf};
use crate::mesh::{Mesh, P3};
use crate::ops;
use crate::parse::{Line, Scope, eval, parse_program};
use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Options {
    pub detail: f64,
    pub y_up: bool,
    pub vars: Vec<(String, f64)>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            detail: 1.0,
            y_up: true,
            vars: Vec::new(),
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
pub struct Clip {
    pub name: String,
    pub fps: f32,
    pub duration: f32,
    pub tracks: Vec<Track>,
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
    y_up: bool,
    mesh: Arc<Mesh>,
    rig: Option<Rig>,
    tints: Vec<[f64; 3]>,
    gloss: Vec<f64>,
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
        let pose: Pose = match clip {
            Some(c) if c < rig.clips.len() => rig.sample(c, t),
            _ => Pose::new(),
        };
        rig.skin_transforms(&pose)
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
            })
            .collect();
        gltf::glb(&bodies)
    }
}

struct Run {
    scene: Scene,
    scope: Scope,
    fixed: Scope,
    colours: HashMap<String, [f64; 3]>,
}

const SUBSET: &[&str] = &[
    "let", "if", "blob", "limb", "remesh", "select", "inflate", "smooth", "refine", "crease",
    "pinch", "flatten", "noise", "relax", "decimate", "bone", "skin", "pose", "clip", "key",
    "move", "rotate", "scale", "mirror", "body", "color",
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
        Ok(())
    }

    fn apply(&mut self, line: &Line) -> Result<()> {
        if !SUBSET.contains(&line.op.as_str()) {
            bail!(
                "`{}` is not a mesh operation; gcad-mesh runs {}",
                line.op,
                SUBSET.join(", ")
            );
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
            "skin" => scene.op_skin(scope, line),
            "pose" => scene.op_mesh_pose(scope, line),
            "clip" => scene.op_clip(scope, line),
            "key" => scene.op_key(scope, line),
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
            let gloss = scene.part_gloss_of(&name, &mesh);
            let vertex_colours = shading
                .iter()
                .find(|(n, ..)| *n == name)
                .map(|(_, _, c, _)| c.iter().map(|c| f3(*c)).collect())
                .unwrap_or_default();
            body_of(
                name.clone(),
                mesh,
                scene.rig_of(&name).cloned(),
                part_tints,
                gloss,
                vertex_colours,
                y_up,
            )
        })
        .collect();
    Model { bodies }
}

fn body_of(
    name: String,
    mesh: Arc<Mesh>,
    rig: Option<Rig>,
    tints: Vec<[f64; 3]>,
    gloss: Vec<f64>,
    colours: Vec<[f32; 3]>,
    y_up: bool,
) -> Body {
    let positions = mesh.positions.iter().map(|p| f3(to_up(*p, y_up))).collect();
    let normals = mesh
        .vertex_normals()
        .into_iter()
        .map(|n| f3(to_up(n, y_up)))
        .collect();
    let mut indices = Vec::new();
    let mut parts = Vec::new();
    for (part, part_name) in mesh.parts.iter().enumerate() {
        let first = indices.len();
        indices.extend(
            mesh.triangles
                .iter()
                .filter(|t| mesh.labels[t[0] as usize] as usize == part)
                .flatten(),
        );
        if indices.len() == first {
            continue;
        }
        parts.push(Part {
            name: part_name.clone(),
            colour: f3(tints.get(part).copied().unwrap_or([0.7; 3])),
            gloss: gloss.get(part).copied().unwrap_or(0.0) as f32,
            first_index: first,
            index_count: indices.len() - first,
        });
    }
    let active = rig.filter(|r| r.skin_fits(&mesh));
    let mut joints = Vec::new();
    let mut joint_indices = Vec::new();
    let mut joint_weights = Vec::new();
    let mut clips = Vec::new();
    if let Some(rig) = &active {
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
            .map(|clip| Clip {
                name: clip.name.clone(),
                fps: clip.fps as f32,
                duration: clip.duration() as f32,
                tracks: (0..rig.bones.len())
                    .filter_map(|bone| {
                        let keys = clip.bone_keys(bone);
                        if keys.is_empty() {
                            return None;
                        }
                        Some(Track {
                            joint: bone,
                            times: keys.iter().map(|k| k.t as f32).collect(),
                            rotations: keys
                                .iter()
                                .map(|k| {
                                    let q = if y_up {
                                        gltf::quat_y_up(k.rotation)
                                    } else {
                                        k.rotation
                                    };
                                    let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
                                    let n = if n > 0.0 { n } else { 1.0 };
                                    q.map(|v| (v / n) as f32)
                                })
                                .collect(),
                        })
                    })
                    .collect(),
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
        y_up,
        mesh,
        rig: active,
        tints,
        gloss,
    }
}
