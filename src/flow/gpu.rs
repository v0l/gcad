use anyhow::Result;

pub use sims_field::Gpu;

pub fn open() -> Result<Gpu> {
    sims_flow::open(std::env::var("GCAD_FLOW_CPU").is_ok())
}
