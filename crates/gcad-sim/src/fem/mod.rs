pub use sims_fem::*;

#[cfg(not(target_arch = "wasm32"))]
pub fn gpu() -> Option<&'static sims_field::Gpu> {
    static GPU: std::sync::OnceLock<Option<sims_field::Gpu>> = std::sync::OnceLock::new();
    GPU.get_or_init(|| {
        if std::env::var("GCAD_FEM_CPU").is_ok() {
            return None;
        }
        sims_fem::gpu::open(sims_field::gpu::Software::Refuse).ok()
    })
    .as_ref()
}

#[cfg(target_arch = "wasm32")]
pub fn gpu() -> Option<&'static sims_field::Gpu> {
    None
}
