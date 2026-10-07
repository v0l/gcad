mod app;
mod gl;
mod scene;

pub use app::run;
pub fn scene_for_timing(
    solid: &monstertruck::modeling::Solid,
    others: &[&monstertruck::modeling::Solid],
) -> usize {
    scene::build(solid, others, &[], &[], [1.0, 1.0, 1.0])
        .surfaces
        .len()
}
