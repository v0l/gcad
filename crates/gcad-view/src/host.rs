pub use gcad_host::*;

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn wake(ctx: &egui::Context) {
    if cfg!(not(target_arch = "wasm32")) {
        ctx.request_repaint();
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Revision(Arc<egui_bench::revision::Snapshot>);

#[cfg(not(target_arch = "wasm32"))]
impl Snapshot for Revision {
    fn covers(&self, path: &Path) -> bool {
        self.0.covers(path)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.0.read(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.0.is_file(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.0.is_dir(path)
    }

    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        self.0.canonicalize(path)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn show_revision(snapshot: Option<Arc<egui_bench::revision::Snapshot>>) {
    set_snapshot(
        snapshot
            .clone()
            .map(|s| Arc::new(Revision(s)) as Arc<dyn Snapshot>),
    );
    egui_bench::revision::set_active(snapshot);
}
