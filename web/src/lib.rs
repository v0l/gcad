pub mod alloc;
pub mod link;

#[cfg(target_arch = "wasm32")]
mod fetch;
#[cfg(target_arch = "wasm32")]
mod start;
#[cfg(target_arch = "wasm32")]
pub use wasm_bindgen_rayon::init_thread_pool;

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOC: alloc::Cached = alloc::Cached;
