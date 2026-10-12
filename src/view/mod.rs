mod app;
mod scene;

pub use app::App;
#[cfg(not(target_arch = "wasm32"))]
pub use app::run;
