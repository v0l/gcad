pub mod arch;
pub mod args;
pub mod brush_ops;
pub mod ffi;
pub mod fur_ops;
pub mod glb;
pub mod gltf;
pub mod interp;
pub mod mesh;
pub mod ops;
pub use gcad_parse as parse;
pub mod rig_ops;
pub mod script;
pub mod solid;
pub mod solid_ops;

pub use script::{
    Body, Clip, Error, Joint, Matrix, Model, MorphTarget, MoveTrack, NormalMap, Options, Part,
    RawBody, Track, WeightTrack, build, build_with,
};

pub fn to_glb(source: &str) -> Result<Vec<u8>, Error> {
    to_glb_with(source, &Options::default())
}

pub fn to_glb_with(source: &str, options: &Options) -> Result<Vec<u8>, Error> {
    build_with(source, options)?.to_glb().map_err(|e| Error {
        line: 0,
        text: String::new(),
        message: format!("{e:#}"),
    })
}
