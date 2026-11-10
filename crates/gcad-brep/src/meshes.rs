use super::Model;
use super::args::label_of;
use crate::mesh::{self, Mesh};
use crate::parse::Line;
use anyhow::{Context, Result, anyhow, bail};
use std::sync::Arc;

pub use gcad_mesh::interp::{MESH_OPERATIONS, MeshBody};

impl Model {
    pub fn has_mesh(&self) -> bool {
        self.scene.has_mesh()
    }

    pub(crate) fn mesh_op(&self, op: &str) -> Result<()> {
        if let Some(wall) = &self.open {
            bail!(
                "`{op}` works on solids and body `{}` holds the open wall `{wall}`; an `open=1` wall goes in its own body",
                self.body_name()
            );
        }
        if self.has_mesh() {
            bail!(
                "`{op}` works on solids and body `{}` is a mesh; start a solid with `body name`",
                self.body_name()
            );
        }
        Ok(())
    }

    pub(super) fn start_mesh(&self, op: &str) -> Result<()> {
        if let Some(wall) = &self.open {
            bail!(
                "`{op}` makes a mesh and body `{}` holds the open wall `{wall}`; start one with `body name`",
                self.body_name()
            );
        }
        if self.solid.is_some() {
            bail!(
                "`{op}` makes a mesh and body `{}` is a solid; start a mesh with `body name`",
                self.body_name()
            );
        }
        self.require_empty_sketch(op)
    }

    pub fn rest_meshes(&self) -> Vec<(String, Arc<Mesh>)> {
        self.scene.rest_meshes()
    }

    pub fn meshes(&self) -> Vec<(String, Arc<Mesh>)> {
        self.scene.meshes()
    }

    pub fn part_colours_of(&self, body: &str, mesh: &Mesh) -> Vec<Option<[f64; 3]>> {
        self.scene.part_colours_of(body, mesh)
    }

    pub fn part_gloss_of(&self, body: &str, mesh: &Mesh) -> Vec<f64> {
        self.scene.part_gloss_of(body, mesh)
    }

    pub fn part_shine_of(&self, body: &str, mesh: &Mesh) -> Vec<gcad_mesh::interp::Shine> {
        self.scene.part_shine_of(body, mesh)
    }

    pub fn mesh_shading(&self) -> Vec<(String, Arc<Mesh>, Vec<[f64; 3]>, Vec<gcad_mesh::interp::Shine>)> {
        self.scene.mesh_shading(&self.colours)
    }

    pub fn fur_shading(&self) -> Vec<(String, Arc<Mesh>, Vec<[f64; 3]>, Vec<gcad_mesh::interp::Shine>)> {
        self.scene.fur_shading(&self.colours)
    }

    pub fn mesh_tints(&self) -> Vec<(String, Arc<Mesh>, Vec<[f64; 3]>)> {
        self.scene.mesh_tints(&self.colours)
    }

    pub fn coloured_meshes(&self) -> Vec<(String, Arc<Mesh>, Option<[f64; 3]>)> {
        self.scene.coloured_meshes(&self.colours)
    }

    pub fn rig_of(&self, body: &str) -> Option<&mesh::rig::Rig> {
        self.scene.rig_of(body)
    }

    pub fn rigs(&self) -> Vec<(String, &mesh::rig::Rig)> {
        self.scene.rigs()
    }

    pub(crate) fn finish_mesh_body(&mut self) -> Result<()> {
        self.scene.finish_mesh_body()
    }

    pub(crate) fn colour_mesh_part(
        &mut self,
        part: &str,
        colour: [f64; 3],
        gloss: Option<f64>,
        soft: Option<f64>,
        metal: Option<(f64, f64)>,
    ) -> Result<String> {
        self.scene.colour_mesh_part(part, colour, gloss, soft, metal)
    }

    pub(crate) fn op_mesh_pose(&mut self, line: &Line) -> Result<String> {
        self.scene.op_mesh_pose(&self.scope, line)
    }

    pub(crate) fn op_blob(&mut self, line: &Line) -> Result<String> {
        self.start_mesh("blob")?;
        self.scene.op_blob(&self.scope, line)
    }

    pub(crate) fn op_limb(&mut self, line: &Line) -> Result<String> {
        self.start_mesh("limb")?;
        self.scene.op_limb(&self.scope, line)
    }

    pub(crate) fn op_block(&mut self, line: &Line) -> Result<String> {
        self.start_mesh("block")?;
        self.scene.op_block(&self.scope, line)
    }

    pub(crate) fn op_remesh(&mut self, line: &Line) -> Result<String> {
        self.scene.op_remesh(&self.scope, line)
    }

    pub(crate) fn op_select(&mut self, line: &Line) -> Result<String> {
        self.scene.op_select(&self.scope, line)
    }

    pub(crate) fn op_inflate(&mut self, line: &Line) -> Result<String> {
        self.scene.op_inflate(&self.scope, line)
    }

    pub(crate) fn op_smooth(&mut self, line: &Line) -> Result<String> {
        self.scene.op_smooth(&self.scope, line)
    }

    pub(crate) fn op_refine(&mut self, line: &Line) -> Result<String> {
        self.scene.op_refine(&self.scope, line)
    }

    pub(crate) fn op_crease(&mut self, line: &Line) -> Result<String> {
        self.scene.op_crease(&self.scope, line)
    }

    pub(crate) fn op_pinch(&mut self, line: &Line) -> Result<String> {
        self.scene.op_pinch(&self.scope, line)
    }

    pub(crate) fn op_flatten(&mut self, line: &Line) -> Result<String> {
        self.scene.op_flatten(&self.scope, line)
    }

    pub(crate) fn op_noise(&mut self, line: &Line) -> Result<String> {
        self.scene.op_noise(&self.scope, line)
    }

    pub(crate) fn op_relax(&mut self, line: &Line) -> Result<String> {
        self.scene.op_relax(&self.scope, line)
    }

    pub(crate) fn op_decimate(&mut self, line: &Line) -> Result<String> {
        self.scene.op_decimate(&self.scope, line)
    }

    pub(crate) fn op_bone(&mut self, line: &Line) -> Result<String> {
        self.start_mesh("bone")?;
        self.scene.op_bone(&self.scope, line)
    }

    pub(crate) fn op_skin(&mut self, line: &Line) -> Result<String> {
        self.scene.op_skin(&self.scope, line)
    }

    pub(crate) fn op_clip(&mut self, line: &Line) -> Result<String> {
        self.start_mesh("clip")?;
        self.scene.op_clip(&self.scope, line)
    }

    pub(crate) fn op_key(&mut self, line: &Line) -> Result<String> {
        self.start_mesh("key")?;
        self.scene.op_key(&self.scope, line)
    }

    pub(crate) fn import_mesh(&mut self, line: &Line, path: &std::path::Path) -> Result<String> {
        self.start_mesh("import")?;
        if self.scene.has_mesh() {
            bail!(
                "body `{}` is already a mesh; start another with `body name`",
                self.body_name()
            );
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let mut mesh = match ext.as_str() {
            "obj" => {
                let text = crate::host::read_to_string(path)
                    .with_context(|| format!("reading {}", path.display()))?;
                mesh::read_obj(&text).with_context(|| format!("reading {}", path.display()))?
            }
            "stl" => {
                let bytes = crate::host::read(path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let stl = monstertruck::mesh::stl::read(
                    std::io::Cursor::new(bytes),
                    monstertruck::mesh::stl::StlType::Automatic,
                )
                .map_err(|e| anyhow!("{}: {e}", path.display()))?;
                let positions = stl.positions();
                mesh::from_triangles(
                    stl.faces()
                        .triangle_iter()
                        .map(|t| {
                            t.map(|v| {
                                let p = positions[v.pos];
                                [p.x, p.y, p.z]
                            })
                        })
                        .collect(),
                )
            }
            other => bail!("`import ... mesh` reads .obj and .stl files, not `.{other}`"),
        };
        if mesh.parts == ["mesh"] {
            mesh.parts = vec![label_of(line)];
        }
        self.scene.mesh_body.mesh = Some(Arc::new(mesh));
        self.scene.describe_mesh()
    }
}
