#[cfg(feature = "game-assets")]
pub mod asset;
pub mod host;
pub mod progress;
pub mod view;

pub use gcad_drafting::{bom, drawing, export, nest};
pub use gcad_geom::{geometry, offset, packed, select};
pub use gcad_mesh::{gltf, mesh};
pub use gcad_parse as parse;
pub use gcad_plot as plot;
#[cfg(not(target_arch = "wasm32"))]
pub use gcad_remote as remote;
pub use gcad_render::{inspect, render};
pub use gcad_sim as sim;
pub use gcad_sim::{drafting, flow, model};
pub use gcad_calc as calc;
