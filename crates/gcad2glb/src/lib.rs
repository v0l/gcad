use std::ops::Deref;
use std::path::Path;

#[cfg(feature = "brep")]
pub use gcad_brep;
pub use gcad_mesh;
pub use gcad_mesh::gltf::Fit;
pub use gcad_mesh::interp::LodSize;
pub use gcad_mesh::{
    Body, Clip, Error, Joint, Matrix, Model, MorphTarget, MoveTrack, NormalMap, Part, Track,
    WeightTrack,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub detail: f64,
    pub y_up: bool,
    pub vars: Vec<(String, f64)>,
    pub triangles: Option<usize>,
    pub max_error: Option<f64>,
    pub fit: Fit,
    pub lods: Option<Vec<LodSize>>,
    pub texture_size: Option<usize>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            detail: 1.0,
            y_up: true,
            vars: Vec::new(),
            triangles: None,
            max_error: None,
            fit: Fit::default(),
            lods: None,
            texture_size: None,
        }
    }
}

impl Options {
    fn mesh(&self) -> gcad_mesh::Options {
        gcad_mesh::Options {
            detail: self.detail,
            y_up: self.y_up,
            vars: self.vars.clone(),
            triangles: self.triangles,
            max_error: self.max_error,
        }
    }

    pub fn set(mut self, name: &str, value: f64) -> Self {
        self.vars.retain(|(n, _)| n != name);
        self.vars.push((name.to_string(), value));
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub scale: f32,
    pub translation: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct Assets {
    pub model: Model,
    pub options: Options,
    pub notes: Vec<String>,
}

impl Deref for Assets {
    type Target = Model;

    fn deref(&self) -> &Model {
        &self.model
    }
}

pub const VERSION: &str = gcad_version::BUILD;

pub fn cache_key(source: &str, options: &Options) -> u64 {
    cache_key_with(VERSION, source, options)
}

pub fn cache_key_with(version: &str, source: &str, options: &Options) -> u64 {
    let mut hash = Fnv::default();
    hash.text(version);
    hash.text(source);
    hash.float(options.detail);
    hash.bytes(&[options.y_up as u8]);
    hash.count(options.vars.len());
    for (name, value) in &options.vars {
        hash.text(name);
        hash.float(*value);
    }
    hash.maybe(options.triangles.map(|n| n as f64));
    hash.maybe(options.max_error);
    hash.maybe(options.fit.height);
    hash.maybe(options.fit.scale);
    hash.bytes(&[options.fit.centre as u8]);
    match &options.lods {
        None => hash.bytes(&[0]),
        Some(lods) => {
            hash.count(lods.len() + 1);
            for lod in lods {
                match lod {
                    LodSize::Triangles(n) => hash.float(*n as f64),
                    LodSize::Share(s) => hash.float(-s),
                }
            }
        }
    }
    hash.maybe(options.texture_size.map(|n| n as f64));
    hash.0
}

struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= *b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn count(&mut self, n: usize) {
        self.bytes(&(n as u64).to_le_bytes());
    }

    fn text(&mut self, text: &str) {
        self.count(text.len());
        self.bytes(text.as_bytes());
    }

    fn float(&mut self, v: f64) {
        self.bytes(&v.to_bits().to_le_bytes());
    }

    fn maybe(&mut self, v: Option<f64>) {
        match v {
            None => self.bytes(&[0]),
            Some(v) => {
                self.bytes(&[1]);
                self.float(v);
            }
        }
    }
}

fn plain(message: impl Into<String>) -> Error {
    Error {
        line: 0,
        text: String::new(),
        message: message.into(),
    }
}

fn kind_of(path: &Path) -> Option<String> {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

fn kernel_first(source: &str, options: &Options) -> bool {
    cfg!(feature = "brep") && options.detail == 1.0 && has_flat_solids(source)
}

fn has_flat_solids(source: &str) -> bool {
    let solid = |word: &str| gcad_mesh::solid_ops::OPS.contains(&word);
    gcad_mesh::parse::parse_program(source).is_ok_and(|lines| {
        lines.iter().any(|line| {
            solid(&line.op) || (line.op == "if" && line.text.split_whitespace().any(solid))
        })
    })
}

impl Assets {
    pub fn from_source(source: &str, options: &Options) -> Result<Assets, Error> {
        let (model, notes) = match kernel_first(source, options) {
            true => solids::from_source(source, options, None)?,
            false => match gcad_mesh::build_with(source, &options.mesh()) {
                Ok(model) => (model, Vec::new()),
                Err(refused) => solids::from_source(source, options, Some(refused))?,
            },
        };
        Assets::finish(model, notes, options)
    }

    pub fn from_path(path: impl AsRef<Path>, options: &Options) -> Result<Assets, Error> {
        let path = path.as_ref();
        let kind = kind_of(path);
        if kind.as_deref() == Some("gsim") {
            return Err(plain(
                "a .gsim is a simulation; gcad2glb writes parts and assemblies",
            ));
        }
        let source = std::fs::read_to_string(path)
            .map_err(|e| plain(format!("reading {}: {e}", path.display())))?;
        let (model, notes) = match kind.as_deref() {
            Some("gasm") => solids::from_path(path, options, None)?,
            _ if kernel_first(&source, options) => solids::from_path(path, options, None)?,
            _ => match gcad_mesh::build_with(&source, &options.mesh()) {
                Ok(model) => (model, Vec::new()),
                Err(refused) => solids::from_path(path, options, Some(refused))?,
            },
        };
        Assets::finish(model, notes, options)
    }

    fn finish(mut model: Model, notes: Vec<String>, options: &Options) -> Result<Assets, Error> {
        if options.fit.height.is_some() && options.fit.scale.is_some() {
            return Err(plain("give a height or a scale, not both"));
        }
        for body in &mut model.bodies {
            if let Some(lods) = &options.lods {
                body.set_lods(lods.clone());
            }
            if let Some(size) = options.texture_size {
                body.set_bake_size(size);
            }
        }
        Ok(Assets {
            model,
            options: options.clone(),
            notes,
        })
    }

    pub fn triangles(&self) -> usize {
        self.bodies.iter().map(|b| b.indices.len() / 3).sum()
    }

    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in self.bodies.iter().flat_map(|b| &b.positions) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        lo[0].is_finite().then_some((lo, hi))
    }

    pub fn fit(&self) -> Option<Transform> {
        let fit = &self.options.fit;
        if fit.is_none() {
            return None;
        }
        let (lo, hi) = self.bounds()?;
        let up = if self.options.y_up { 1 } else { 2 };
        let k = match (fit.height, fit.scale) {
            (Some(h), _) => (h / (hi[up] - lo[up]).max(f32::MIN_POSITIVE) as f64) as f32,
            (None, Some(s)) => s as f32,
            (None, None) => 1.0,
        };
        let translation = std::array::from_fn(|i| match (i == up, fit.centre) {
            (true, _) => -lo[i] * k,
            (false, true) => -(lo[i] + hi[i]) / 2.0 * k,
            (false, false) => 0.0,
        });
        Some(Transform {
            scale: k,
            translation,
        })
    }

    pub fn key(&self, source: &str) -> u64 {
        cache_key(source, &self.options)
    }

    pub fn to_glb(&self) -> Result<Vec<u8>, Error> {
        let bytes = self.model.to_glb().map_err(|e| plain(format!("{e:#}")))?;
        match self.options.fit.is_none() {
            true => Ok(bytes),
            false => {
                gcad_mesh::gltf::fit(&bytes, &self.options.fit).map_err(|e| plain(format!("{e:#}")))
            }
        }
    }
}

#[cfg(feature = "brep")]
mod solids {
    use super::{Error, Model, Options, plain};
    use std::path::Path;

    type Built = (Model, Vec<String>);

    fn finished(
        mut run: gcad_brep::Run,
        options: &Options,
        refused: Option<Error>,
    ) -> Result<Built, Error> {
        if let Some((line, Err(e))) = run.steps.last() {
            return Err(match refused {
                Some(refused) if refused.line >= line.number => refused,
                _ => Error {
                    line: line.number,
                    text: line.text.chars().take(80).collect(),
                    message: format!("{e:#}"),
                },
            });
        }
        if options.detail != 1.0 {
            return Err(plain(
                "--detail only applies to files without solid operations",
            ));
        }
        let notes = run.model.scene.budget(options.triangles, options.max_error);
        Ok((gcad_brep::glb::mesh_model(&run.model, options.y_up), notes))
    }

    pub fn from_source(
        source: &str,
        options: &Options,
        refused: Option<Error>,
    ) -> Result<Built, Error> {
        let lines = gcad_mesh::parse::parse_program(source).map_err(|e| plain(format!("{e:#}")))?;
        finished(
            gcad_brep::run_full(None, &options.vars, &lines),
            options,
            refused,
        )
    }

    pub fn from_path(
        path: &Path,
        options: &Options,
        refused: Option<Error>,
    ) -> Result<Built, Error> {
        let run =
            gcad_brep::run_path(path, &options.vars, None).map_err(|e| plain(format!("{e:#}")))?;
        finished(run, options, refused)
    }
}

#[cfg(not(feature = "brep"))]
mod solids {
    use super::{Error, Model, Options};
    use std::path::Path;

    type Built = (Model, Vec<String>);

    fn without(refused: Error) -> Error {
        Error {
            message: format!(
                "{}; this gcad2glb was built without the brep feature, so it writes meshes only",
                refused.message
            ),
            ..refused
        }
    }

    pub fn from_source(
        _source: &str,
        _options: &Options,
        refused: Option<Error>,
    ) -> Result<Built, Error> {
        Err(without(
            refused.unwrap_or_else(|| super::plain("a solid file")),
        ))
    }

    pub fn from_path(
        path: &Path,
        _options: &Options,
        refused: Option<Error>,
    ) -> Result<Built, Error> {
        Err(without(refused.unwrap_or_else(|| {
            super::plain(format!("{} is an assembly", path.display()))
        })))
    }
}
