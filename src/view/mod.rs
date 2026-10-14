mod app;
#[cfg(not(target_arch = "wasm32"))]
mod remote_panel;
mod scene;
mod textures;
mod units;

pub use app::App;
#[cfg(not(target_arch = "wasm32"))]
pub use app::run;
