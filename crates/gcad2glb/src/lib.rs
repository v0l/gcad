use std::path::Path;

pub use gcad_mesh::Options;
pub use gcad_mesh::gltf::Fit;

pub struct Glb {
    pub bytes: Vec<u8>,
    pub notes: Vec<String>,
}

pub fn glb_from_path(path: &Path, options: &Options, fit: &Fit) -> Result<Glb, String> {
    let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut notes = Vec::new();
    let glb = gcad_mesh::build_with(&source, options)
        .map_err(|e| e.to_string())
        .and_then(|model| model.to_glb().map_err(|e| format!("{e:#}")));
    let bytes = match glb {
        Err(message) => with_solids(path, options, message, &mut notes),
        other => other,
    }?;
    let bytes = match fit.is_none() {
        true => bytes,
        false => gcad_mesh::gltf::fit(&bytes, fit).map_err(|e| format!("{e:#}"))?,
    };
    Ok(Glb { bytes, notes })
}

#[cfg(feature = "brep")]
fn with_solids(
    path: &Path,
    options: &Options,
    refused: String,
    notes: &mut Vec<String>,
) -> Result<Vec<u8>, String> {
    if gcad_brep::is_sim(path) {
        return Err(refused);
    }
    let mut run = gcad_brep::run_path(path, &[], None).map_err(|e| format!("{e:#}"))?;
    if let Some((_, Err(_))) = run.steps.last() {
        return Err(refused);
    }
    if options.detail != 1.0 || !options.y_up {
        return Err("--detail and --z-up only apply to files without solid operations".into());
    }
    if run.model.assembly {
        return Err("gcad2glb writes part files; export the parts of an assembly".into());
    }
    notes.extend(run.model.scene.budget(options.triangles, options.max_error));
    gcad_brep::glb::glb_of(&run.model).map_err(|e| format!("{e:#}"))
}

#[cfg(not(feature = "brep"))]
fn with_solids(
    _path: &Path,
    _options: &Options,
    refused: String,
    _notes: &mut Vec<String>,
) -> Result<Vec<u8>, String> {
    Err(format!(
        "{refused}; this gcad2glb was built without the brep feature, so it writes meshes only"
    ))
}
