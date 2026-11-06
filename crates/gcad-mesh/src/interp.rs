use crate::args::{Args, label_of, point3, positive};
use crate::mesh::sdf::{self, Combine, Primitive, Shape};
use crate::mesh::{self, Mesh, Region, Selection};
use crate::parse::Line;
use crate::parse::Scope;
use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use std::sync::Arc;

pub const MESH_OPERATIONS: &[&str] = &[
    "blob", "limb", "remesh", "select", "inflate", "smooth", "refine", "crease", "pinch",
    "flatten", "noise", "relax", "decimate", "bone", "skin", "pose", "clip", "key", "brush",
    "stroke", "dab", "mask", "hide", "isolate", "show", "group", "join", "dyntopo",
    "subdivide", "level", "project", "layer", "finish",
];

pub type Shaded = (String, Arc<Mesh>, Vec<[f64; 3]>, Vec<f64>);

type Named = (String, Vec<Region>, f64, Option<usize>);

#[derive(Clone, Default)]
pub struct MeshBody {
    pub blockout: Vec<Primitive>,
    pub cell: Option<f64>,
    pub mesh: Option<Arc<Mesh>>,
    pub selection: Option<Selection>,
    pub selections: Vec<Named>,
    pub rig: crate::mesh::rig::Rig,
    pub clip: Option<usize>,
    pub brushes: Vec<(String, crate::mesh::stroke::Brush)>,
    pub dyntopo: Option<f64>,
    pub levels: Vec<(String, Arc<Mesh>)>,
    pub(crate) preview: std::sync::OnceLock<std::result::Result<Arc<Mesh>, String>>,
}

impl MeshBody {
    pub fn is_empty(&self) -> bool {
        self.blockout.is_empty() && self.mesh.is_none() && self.rig.is_empty()
    }
}

#[derive(Clone, Default)]
pub struct Scene {
    pub body: String,
    pub cell_scale: f64,
    pub mesh_body: MeshBody,
    pub mesh_bodies: Vec<(String, Arc<Mesh>)>,
    pub mesh_rigs: Vec<(String, crate::mesh::rig::Rig)>,
    pub part_colours: HashMap<(String, String), [f64; 3]>,
    pub part_gloss: HashMap<(String, String), f64>,
    pub part_soft: HashMap<(String, String), f64>,
}

const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn mesh_combine(args: &Args<'_>) -> Result<Combine> {
    match args.values.get("mode").copied() {
        None | Some("add") => Ok(Combine::Add),
        Some("cut") => Ok(Combine::Cut),
        Some("intersect") => Ok(Combine::Intersect),
        Some("shell") => Ok(Combine::Shell),
        Some(other) => bail!("mode must be add, cut, intersect or shell, got `{other}`"),
    }
}

fn axis_index(text: &str) -> Result<usize> {
    match text.to_ascii_lowercase().as_str() {
        "x" => Ok(0),
        "y" => Ok(1),
        "z" => Ok(2),
        other => bail!("mirror takes x, y or z, got `{other}`"),
    }
}

fn numbers(text: &str, scope: &crate::parse::Scope) -> Result<Vec<f64>> {
    crate::parse::split_top_level(text, ',')
        .iter()
        .map(|part| crate::parse::eval(part, scope))
        .collect()
}

impl Scene {
    pub fn cell_factor(&self) -> f64 {
        if self.cell_scale > 0.0 {
            self.cell_scale
        } else {
            1.0
        }
    }

    pub fn body_name(&self) -> String {
        if self.body.is_empty() {
            "main".to_string()
        } else {
            self.body.clone()
        }
    }

    pub fn has_mesh(&self) -> bool {
        !self.mesh_body.is_empty()
    }

    pub fn start_mesh(&self, _op: &str) -> Result<()> {
        Ok(())
    }

    pub fn rest_meshes(&self) -> Vec<(String, Arc<Mesh>)> {
        let mut out = self.mesh_bodies.clone();
        if let Some(mesh) = self.current_mesh().ok().flatten() {
            out.push((self.body_name(), mesh));
        }
        out
    }

    pub fn meshes(&self) -> Vec<(String, Arc<Mesh>)> {
        self.rest_meshes()
            .into_iter()
            .map(|(name, mesh)| match self.rig_of(&name) {
                Some(rig) if rig.is_posed() && rig.skin_fits(&mesh) => {
                    let posed = rig.posed(&mesh, &rig.pose);
                    (name, Arc::new(posed))
                }
                _ => (name, mesh),
            })
            .collect()
    }

    pub fn colour_mesh_part(
        &mut self,
        part: &str,
        colour: [f64; 3],
        gloss: Option<f64>,
        soft: Option<f64>,
    ) -> Result<String> {
        let mesh = self
            .current_mesh()?
            .ok_or_else(|| anyhow!("`color part=` colours a part of a mesh body"))?;
        if !mesh.parts.iter().any(|p| p == part) {
            bail!(
                "the mesh has no part `{part}`; its parts are {}",
                mesh.parts.join(", ")
            );
        }
        self.part_colours
            .insert((self.body_name(), part.to_string()), colour);
        if let Some(g) = gloss {
            self.part_gloss
                .insert((self.body_name(), part.to_string()), g);
        }
        if let Some(w) = soft {
            self.part_soft
                .insert((self.body_name(), part.to_string()), w);
        }
        Ok(format!(
            "part `{part}` is {:.2},{:.2},{:.2}",
            colour[0], colour[1], colour[2]
        ))
    }

    pub fn part_colours_of(&self, body: &str, mesh: &Mesh) -> Vec<Option<[f64; 3]>> {
        mesh.parts
            .iter()
            .map(|p| {
                self.part_colours
                    .get(&(body.to_string(), p.clone()))
                    .copied()
            })
            .collect()
    }

    pub fn part_gloss_of(&self, body: &str, mesh: &Mesh) -> Vec<f64> {
        mesh.parts
            .iter()
            .map(|p| {
                self.part_gloss
                    .get(&(body.to_string(), p.clone()))
                    .copied()
                    .unwrap_or(0.0)
            })
            .collect()
    }

    pub fn mesh_shading(&self, colours: &HashMap<String, [f64; 3]>) -> Vec<Shaded> {
        self.mesh_tints(colours)
            .into_iter()
            .map(|(name, mesh, tints)| {
                let gloss = self.part_gloss_of(&name, &mesh);
                let soft: Vec<f64> = mesh
                    .parts
                    .iter()
                    .map(|p| {
                        self.part_soft
                            .get(&(name.clone(), p.clone()))
                            .copied()
                            .unwrap_or(0.0)
                    })
                    .collect();
                let colours = mesh.blend_colours(&tints, &soft);
                let shine = mesh
                    .labels
                    .iter()
                    .map(|&l| gloss.get(l as usize).copied().unwrap_or(0.0))
                    .collect();
                (name, mesh, colours, shine)
            })
            .collect()
    }

    pub fn mesh_tints(
        &self,
        colours: &HashMap<String, [f64; 3]>,
    ) -> Vec<(String, Arc<Mesh>, Vec<[f64; 3]>)> {
        self.coloured_meshes(colours)
            .into_iter()
            .map(|(name, mesh, colour)| {
                let base = colour.unwrap_or([150.0 / 255.0, 175.0 / 255.0, 205.0 / 255.0]);
                let tints = self
                    .part_colours_of(&name, &mesh)
                    .into_iter()
                    .map(|c| c.unwrap_or(base))
                    .collect();
                (name, mesh, tints)
            })
            .collect()
    }

    pub fn coloured_meshes(
        &self,
        colours: &HashMap<String, [f64; 3]>,
    ) -> Vec<(String, Arc<Mesh>, Option<[f64; 3]>)> {
        self.meshes()
            .into_iter()
            .map(|(name, mesh)| {
                let colour = colours.get(&name).copied();
                (name, mesh, colour)
            })
            .collect()
    }

    pub fn current_mesh(&self) -> Result<Option<Arc<Mesh>>> {
        if let Some(mesh) = &self.mesh_body.mesh {
            return Ok(Some(mesh.clone()));
        }
        if self.mesh_body.blockout.is_empty() {
            return Ok(None);
        }
        self.mesh_body
            .preview
            .get_or_init(|| {
                self.blockout_mesh()
                    .map(Arc::new)
                    .map_err(|e| format!("{e:#}"))
            })
            .clone()
            .map(Some)
            .map_err(|e| anyhow!(e))
    }

    pub fn blockout_mesh(&self) -> Result<Mesh> {
        let cell = self.mesh_body.cell.unwrap_or_else(|| self.default_cell());
        sdf::remesh(&self.mesh_body.blockout, cell)
    }

    fn default_cell(&self) -> f64 {
        let (lo, hi) = self
            .mesh_body
            .blockout
            .iter()
            .map(|p| p.shape.bounds())
            .fold(
                ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
                |(a, b), (lo, hi)| {
                    (
                        std::array::from_fn(|k| a[k].min(lo[k])),
                        std::array::from_fn(|k| b[k].max(hi[k])),
                    )
                },
            );
        let size = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max);
        (size / 120.0 * self.cell_factor()).max(1.0e-6)
    }

    pub fn finish_mesh_body(&mut self) -> Result<()> {
        if let Some(mesh) = self.current_mesh()? {
            self.mesh_bodies.push((self.body_name(), mesh));
            if !self.mesh_body.rig.is_empty() {
                self.mesh_rigs
                    .push((self.body_name(), self.mesh_body.rig.clone()));
            }
        }
        self.mesh_body = MeshBody::default();
        Ok(())
    }

    pub fn describe_mesh(&self) -> Result<String> {
        let mesh = self
            .current_mesh()?
            .ok_or_else(|| anyhow!("there is no mesh"))?;
        Ok(mesh.summary())
    }

    fn add_primitive(
        &mut self,
        scope: &Scope,
        line: &Line,
        shape: Shape,
        args: &Args<'_>,
    ) -> Result<String> {
        if self.mesh_body.mesh.is_some() {
            bail!(
                "`{}` adds to the blockout, which `remesh` has already turned into a mesh; sculpt it with `select` and the mesh ops",
                line.op
            );
        }
        let blend = args.optional_number("blend", scope)?.unwrap_or(0.0);
        if blend < 0.0 {
            bail!("blend must be 0 or more, got {blend}");
        }
        let combine = mesh_combine(args)?;
        let part = label_of(line);
        self.mesh_body.preview = Default::default();
        self.mesh_body.blockout.push(Primitive {
            shape,
            blend,
            combine,
            part,
        });
        Ok(format!(
            "{} primitive(s) in the blockout",
            self.mesh_body.blockout.len()
        ))
    }

    fn rotation_of(&self, scope: &Scope, args: &Args<'_>) -> Result<[[f64; 3]; 3]> {
        let mut m = IDENTITY;
        for (name, axis) in [
            ("rx", [1.0, 0.0, 0.0]),
            ("ry", [0.0, 1.0, 0.0]),
            ("rz", [0.0, 0.0, 1.0]),
        ] {
            if let Some(angle) = args.optional_number(name, scope)? {
                let r = mesh::rotation(axis, angle);
                m = std::array::from_fn(|i| {
                    std::array::from_fn(|j| (0..3).map(|k| r[i][k] * m[k][j]).sum())
                });
            }
        }
        Ok(m)
    }

    pub fn op_blob(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        self.start_mesh("blob")?;
        let args = Args::new(
            line,
            &["at", "r"],
            &["blend", "mode", "rx", "ry", "rz"],
            false,
        )?;
        let centre = point3(args.text("at")?, scope)?;
        let radii = numbers(args.text("r")?, scope)?;
        let radii = match radii[..] {
            [r] => [r; 3],
            [x, y, z] => [x, y, z],
            _ => bail!("`r` is one radius or x,y,z radii"),
        };
        radii
            .iter()
            .try_for_each(|r| positive(*r, "radius").map(|_| ()))?;
        let shape = Shape::Ellipsoid {
            centre,
            radii,
            rotation: self.rotation_of(scope, &args)?,
        };
        self.add_primitive(scope, line, shape, &args)
    }

    pub fn op_limb(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        self.start_mesh("limb")?;
        let args = Args::new(line, &[], &["r", "blend", "mode"], true)?;
        let joints: Vec<mesh::P3> = args
            .rest
            .iter()
            .map(|text| point3(text, scope))
            .collect::<Result<_>>()?;
        if joints.len() < 2 {
            bail!("`limb` needs at least two joints, like `limb 0,0,0 0,0,10 r=2`");
        }
        let radii = numbers(args.text("r")?, scope)?;
        let radii = match radii.len() {
            1 => vec![radii[0]; joints.len()],
            n if n == joints.len() => radii,
            n => bail!(
                "`r` is one radius or one per joint; there are {} joints and {n} radii",
                joints.len()
            ),
        };
        radii
            .iter()
            .try_for_each(|r| positive(*r, "radius").map(|_| ()))?;
        self.add_primitive(scope, line, Shape::Limb { joints, radii }, &args)
    }

    pub fn op_remesh(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        if line.positional.first().map(String::as_str) == Some("dynamesh") {
            let args = Args::new(line, &["mode", "cell"], &[], false)?;
            let cell = positive(args.number("cell", scope)?, "cell")? * self.cell_factor();
            let before = self.sculpted("remesh dynamesh")?.clone();
            let mut even = before.dynamesh(cell)?;
            even.hidden = before.hidden.clone();
            self.mesh_body.mesh = Some(Arc::new(even));
            self.mesh_body.levels.clear();
            return Ok(format!(
                "{} (was {} triangles)",
                self.describe_mesh()?,
                before.triangles.len()
            ));
        }
        let args = Args::new(line, &["cell"], &[], false)?;
        if self.mesh_body.blockout.is_empty() {
            bail!(
                "`remesh` turns a blockout of `blob` and `limb` lines into a mesh; there is none"
            );
        }
        if self.mesh_body.mesh.is_some() {
            bail!("the blockout is already a mesh");
        }
        let cell = match args.optional_number("cell", scope)? {
            Some(cell) => positive(cell, "cell")? * self.cell_factor(),
            None => self.default_cell(),
        };
        self.mesh_body.cell = Some(cell);
        let mesh = self.blockout_mesh()?;
        self.mesh_body.mesh = Some(Arc::new(mesh));
        self.describe_mesh()
    }

    pub fn sculpted(&mut self, op: &str) -> Result<&mut Mesh> {
        if self.mesh_body.mesh.is_none() {
            if self.mesh_body.blockout.is_empty() {
                bail!(
                    "`{op}` works on a mesh; make one with `blob`/`limb` and `remesh`, or `import file.obj mesh`"
                );
            }
            let mesh = self.blockout_mesh()?;
            self.mesh_body.mesh = Some(Arc::new(mesh));
        }
        let mesh = self.mesh_body.mesh.as_mut().expect("set above");
        Ok(Arc::make_mut(mesh))
    }

    fn region_of(&self, word: &str) -> Result<Vec<Region>> {
        if word == "all" {
            return Ok(vec![Region::All]);
        }
        if let Some((_, regions, _, _)) = self
            .mesh_body
            .selections
            .iter()
            .rev()
            .find(|(name, ..)| name == word)
        {
            return Ok(regions.clone());
        }
        Ok(vec![Region::Part(word.to_string())])
    }

    pub fn op_select(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &[],
            &["sphere", "r", "box", "falloff", "mirror", "profile", "strength"],
            true,
        )?;
        let mut regions = Vec::new();
        for word in &args.rest {
            for name in crate::parse::split_top_level(word, ',') {
                regions.extend(self.region_of(name.trim())?);
            }
        }
        if let Some(text) = args.values.get("sphere") {
            let c = point3(text, scope)?;
            let r = positive(args.number("r", scope)?, "r")?;
            regions.push(Region::Sphere(c, r));
        } else if args.has("r") {
            bail!("`r=` goes with `sphere=x,y,z`");
        }
        if let Some(text) = args.values.get("box") {
            let v = numbers(text, scope)?;
            let [a, b, c, d, e, f] = v[..] else {
                bail!("`box` is x0,y0,z0,x1,y1,z1");
            };
            regions.push(Region::Box(
                [a.min(d), b.min(e), c.min(f)],
                [a.max(d), b.max(e), c.max(f)],
            ));
        }
        if regions.is_empty() {
            bail!(
                "say what to select: a part name, `all`, `sphere=x,y,z r=`, or `box=x0,y0,z0,x1,y1,z1`"
            );
        }
        let falloff = args.optional_number("falloff", scope)?.unwrap_or(0.0);
        if falloff < 0.0 {
            bail!("falloff must be 0 or more, got {falloff}");
        }
        let mirror = args
            .values
            .get("mirror")
            .map(|t| axis_index(t))
            .transpose()?;
        let mesh = self.sculpted("select")?.clone();
        let mut weighting = mesh::Weighting::default();
        if let Some(text) = args.values.get("profile") {
            weighting.profile = crate::brush_ops::falloff_of(text, scope)?;
        }
        if let Some(s) = args.optional_number("strength", scope)? {
            if !(-4.0..=4.0).contains(&s) {
                bail!("`strength` goes from -4 to 4, got {s}");
            }
            weighting.strength = s;
        }
        let selection = Selection::weighted(&mesh, regions.clone(), falloff, mirror, weighting)?;
        let count = selection.count();
        let full = selection.weights.iter().filter(|&&w| w >= 1.0).count();
        let centre = selection.centre;
        self.mesh_body
            .selections
            .push((label_of(line), regions, falloff, mirror));
        self.mesh_body.selection = Some(selection);
        Ok(format!(
            "{count} of {} vertices, {full} at full strength, centre {:.3},{:.3},{:.3}",
            mesh.positions.len(),
            centre[0],
            centre[1],
            centre[2]
        ))
    }

    fn selection_for(&mut self, op: &str) -> Result<Selection> {
        let mesh = self.sculpted(op)?.clone();
        let selection = match self.mesh_body.selection.take() {
            Some(s) => s.again(&mesh)?,
            None => Selection::new(&mesh, vec![Region::All], 0.0, None)?,
        };
        self.mesh_body.selection = Some(selection.clone());
        Ok(selection)
    }

    fn sculpt_summary(&self, moved: usize, before: &Mesh) -> Result<String> {
        let after = self.mesh_body.mesh.as_ref().expect("sculpted");
        let most = before
            .positions
            .iter()
            .zip(&after.positions)
            .map(|(a, b)| mesh::length(mesh::sub(*a, *b)))
            .fold(0.0, f64::max);
        Ok(format!(
            "moved {moved} vertices, the most by {most:.3}; volume {:.3}",
            after.volume()
        ))
    }

    pub fn mesh_move(&mut self, by: mesh::P3) -> Result<String> {
        let selection = self.selection_for("move")?;
        let before = self.sculpted("move")?.clone();
        let moved = self
            .sculpted("move")?
            .deform(&selection, |p| mesh::add(p, by));
        self.sculpt_summary(moved, &before)
    }

    pub fn mesh_rotate(
        &mut self,
        axis: mesh::P3,
        angle: f64,
        about: Option<mesh::P3>,
    ) -> Result<String> {
        let selection = self.selection_for("rotate")?;
        let centre = about.unwrap_or(selection.centre);
        let m = mesh::rotation(axis, angle);
        let before = self.sculpted("rotate")?.clone();
        let moved = self.sculpted("rotate")?.deform(&selection, |p| {
            mesh::add(centre, mesh::apply(&m, mesh::sub(p, centre)))
        });
        self.sculpt_summary(moved, &before)
    }

    pub fn mesh_scale(&mut self, factors: mesh::P3, about: Option<mesh::P3>) -> Result<String> {
        let selection = self.selection_for("scale")?;
        let centre = about.unwrap_or(selection.centre);
        let before = self.sculpted("scale")?.clone();
        let flips = factors.iter().filter(|f| **f < 0.0).count() % 2 == 1;
        if flips {
            bail!("scale factors must be positive");
        }
        let moved = self.sculpted("scale")?.deform(&selection, |p| {
            let d = mesh::sub(p, centre);
            mesh::add(
                centre,
                [d[0] * factors[0], d[1] * factors[1], d[2] * factors[2]],
            )
        });
        self.sculpt_summary(moved, &before)
    }

    pub fn op_inflate(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["by"], &[], false)?;
        let by = args.number("by", scope)?;
        let selection = self.selection_for("inflate")?;
        let before = self.sculpted("inflate")?.clone();
        let moved = self.sculpted("inflate")?.inflate(&selection, by);
        self.sculpt_summary(moved, &before)
    }

    pub fn op_smooth(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["passes"], &[], false)?;
        let passes = args.optional_number("passes", scope)?.unwrap_or(1.0);
        if passes < 1.0 || passes.fract() != 0.0 || passes > 200.0 {
            bail!("`smooth` takes a whole number of passes from 1 to 200, got {passes}");
        }
        let selection = self.selection_for("smooth")?;
        let before = self.sculpted("smooth")?.clone();
        let moved = self.sculpted("smooth")?.smooth(&selection, passes as usize);
        self.sculpt_summary(moved, &before)
    }

    fn topology_summary(&mut self, op: &str, before: usize) -> Result<String> {
        let selection = self.selection_for(op)?;
        let mesh = self.mesh_body.mesh.as_ref().expect("sculpted");
        let (mean, longest) = mesh.edge_lengths(&selection);
        let open = match mesh.open_edges() {
            0 => String::new(),
            n => format!(", {n} open edges"),
        };
        Ok(format!(
            "{} triangles (was {before}); edges in the selection {mean:.3} on average, {longest:.3} at most; volume {:.3}{open}",
            mesh.triangles.len(),
            mesh.volume()
        ))
    }

    fn coarse_note(&self, selection: &Selection, size: f64) -> String {
        let mesh = self.mesh_body.mesh.as_ref().expect("sculpted");
        let (mean, _) = mesh.edge_lengths(selection);
        if mean > size {
            format!(
                "; edges here are {mean:.2} long, more than {size:.2}, so `refine {:.2}` first for a sharper result",
                size / 2.0
            )
        } else {
            String::new()
        }
    }

    pub fn op_refine(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["edge"], &[], false)?;
        let edge = positive(args.number("edge", scope)?, "edge length")?;
        let selection = self.selection_for("refine")?;
        let before = self.sculpted("refine")?.triangles.len();
        let (mean, _) = self.sculpted("refine")?.edge_lengths(&selection);
        let rounds = ((mean / edge).log2().ceil().max(0.0) as usize + 1).min(8);
        let mesh = self.sculpted("refine")?;
        mesh.refine_to(&selection, edge, rounds);
        if mesh.triangles.len() > 4_000_000 {
            bail!(
                "`refine {edge}` makes {} triangles; refine a smaller selection or a longer edge",
                mesh.triangles.len()
            );
        }
        self.topology_summary("refine", before)
    }

    pub fn op_crease(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["from", "to"], &["depth", "width"], false)?;
        let a = point3(args.text("from")?, scope)?;
        let b = point3(args.text("to")?, scope)?;
        let depth = args.optional_number("depth", scope)?.unwrap_or(0.3);
        let width = positive(
            args.optional_number("width", scope)?.unwrap_or(0.5),
            "width",
        )?;
        let selection = self.selection_for("crease")?;
        let note = self.coarse_note(&selection, width);
        let before = self.sculpted("crease")?.clone();
        let moved = self
            .sculpted("crease")?
            .crease(&selection, (a, b), depth, width);
        Ok(self.sculpt_summary(moved, &before)? + &note)
    }

    pub fn op_pinch(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["strength"], &["toward"], false)?;
        let strength = args.number("strength", scope)?;
        if !(-1.0..=1.0).contains(&strength) {
            bail!("pinch strength goes from -1 (spread) to 1 (all the way), got {strength}");
        }
        let selection = self.selection_for("pinch")?;
        let toward = match args.values.get("toward") {
            Some(text) => point3(text, scope)?,
            None => selection.centre,
        };
        let before = self.sculpted("pinch")?.clone();
        let moved = self.sculpted("pinch")?.pinch(&selection, toward, strength);
        self.sculpt_summary(moved, &before)
    }

    pub fn op_flatten(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cut"], &["at", "normal"], false)?;
        let cut = match args.values.get("cut").copied() {
            None => false,
            Some("cut") => true,
            Some(other) => bail!("`flatten` takes `cut`, got `{other}`"),
        };
        let plane = match (args.values.get("at"), args.values.get("normal")) {
            (Some(at), Some(normal)) => {
                let o = point3(at, scope)?;
                let n = crate::args::direction(normal, scope)?;
                if n == [0.0; 3] {
                    bail!("`normal` must not be zero");
                }
                Some((o, n))
            }
            (None, None) => None,
            _ => bail!(
                "give both `at=x,y,z` and `normal=`, or neither to fit a plane to the selection"
            ),
        };
        let selection = self.selection_for("flatten")?;
        let before = self.sculpted("flatten")?.clone();
        if cut && plane.is_none() {
            bail!(
                "`flatten cut` needs the plane: `at=x,y,z normal=`, the normal pointing out of what stays"
            );
        }
        let moved = self.sculpted("flatten")?.flatten(&selection, plane, cut);
        self.sculpt_summary(moved, &before)
    }

    pub fn op_noise(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["amplitude"], &["size", "seed"], false)?;
        let amplitude = args.number("amplitude", scope)?;
        let size = positive(args.optional_number("size", scope)?.unwrap_or(1.0), "size")?;
        let seed = args.optional_number("seed", scope)?.unwrap_or(1.0);
        if seed < 0.0 || seed.fract() != 0.0 {
            bail!("`seed` is a whole number, got {seed}");
        }
        let selection = self.selection_for("noise")?;
        let note = self.coarse_note(&selection, size / 2.0);
        let before = self.sculpted("noise")?.clone();
        let moved = self
            .sculpted("noise")?
            .noise(&selection, amplitude, size, seed as u32);
        Ok(self.sculpt_summary(moved, &before)? + &note)
    }

    pub fn op_relax(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["passes"], &[], false)?;
        let passes = args.optional_number("passes", scope)?.unwrap_or(1.0);
        if passes < 1.0 || passes.fract() != 0.0 || passes > 200.0 {
            bail!("`relax` takes a whole number of passes from 1 to 200, got {passes}");
        }
        let selection = self.selection_for("relax")?;
        let before = self.sculpted("relax")?.clone();
        let moved = self.sculpted("relax")?.relax(&selection, passes as usize);
        self.sculpt_summary(moved, &before)
    }

    pub fn op_decimate(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(line, &["triangles"], &[], false)?;
        let text = args.text("triangles")?;
        let selection = self.selection_for("decimate")?;
        let mesh = self.sculpted("decimate")?;
        let before = mesh.triangles.len();
        let target = match text.strip_suffix('%') {
            Some(share) => {
                let share: f64 = crate::parse::eval(share, scope)?;
                if !(0.0..100.0).contains(&share) || share == 0.0 {
                    bail!("keep between 0% and 100% of the triangles, got {share}%");
                }
                (before as f64 * share / 100.0).round() as usize
            }
            None => {
                let n = args.number("triangles", scope)?;
                if n < 4.0 || n.fract() != 0.0 {
                    bail!(
                        "`decimate` takes a whole number of triangles (at least 4) or a share like 25%"
                    );
                }
                n as usize
            }
        };
        let mesh = self.sculpted("decimate")?;
        mesh.decimate(&selection, target);
        let after = mesh.triangles.len();
        let note = if after > target {
            format!(
                "; stopped at {after}, the rest cannot go without tearing the surface or leaving the selection"
            )
        } else {
            String::new()
        };
        Ok(self.topology_summary("decimate", before)? + &note)
    }

    pub fn mesh_mirror(&mut self, axis: usize, at: f64, of: Option<&str>) -> Result<String> {
        if let Some(name) = of
            && self.mesh_body.rig.bone_index(name).is_some()
        {
            return self.mirror_bone(axis, at, name);
        }
        if self.mesh_body.mesh.is_none() {
            let picked: Vec<Primitive> = self
                .mesh_body
                .blockout
                .iter()
                .filter(|p| of.is_none_or(|name| p.part == name))
                .cloned()
                .collect();
            if picked.is_empty() {
                bail!("no blockout line is labelled `{}`", of.unwrap_or_default());
            }
            self.mesh_body.preview = Default::default();
            for p in picked {
                self.mesh_body.blockout.push(Primitive {
                    shape: p.shape.mirrored(axis, at),
                    part: mirrored_name(&p.part, axis).unwrap_or_else(|| p.part.clone()),
                    ..p
                });
            }
            return Ok(format!(
                "{} primitive(s) in the blockout",
                self.mesh_body.blockout.len()
            ));
        }
        if of.is_some() {
            bail!("`mirror of=` copies blockout lines; after `remesh` mirror the whole mesh");
        }
        let mesh = self.sculpted("mirror")?;
        let mut copy = mesh.transformed(
            |p| {
                let mut q = p;
                q[axis] = 2.0 * at - q[axis];
                q
            },
            true,
        );
        copy.parts = copy
            .parts
            .iter()
            .map(|p| mirrored_name(p, axis).unwrap_or_else(|| p.clone()))
            .collect();
        mesh.merge(&copy);
        self.describe_mesh()
    }
}

pub fn mirrored_name(name: &str, axis: usize) -> Option<String> {
    const SIDES: [(&str, &str); 4] = [
        ("_l", "_r"),
        ("_r", "_l"),
        ("_left", "_right"),
        ("_right", "_left"),
    ];
    (axis == 0)
        .then(|| {
            SIDES
                .iter()
                .find_map(|(from, to)| name.strip_suffix(from).map(|stem| format!("{stem}{to}")))
        })
        .flatten()
}
