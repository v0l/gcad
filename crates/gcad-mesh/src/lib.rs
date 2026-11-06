pub mod args;
pub mod ffi;
pub mod gltf;
pub mod interp;
pub mod mesh;
pub mod ops;
pub mod parse;
pub mod rig_ops;
pub mod script;

pub use script::{
    Body, Clip, Error, Joint, Matrix, Model, MoveTrack, Options, Part, Track, build, build_with,
};

pub fn to_glb(source: &str) -> Result<Vec<u8>, Error> {
    build(source)?.to_glb().map_err(|e| Error {
        line: 0,
        text: String::new(),
        message: format!("{e:#}"),
    })
}
