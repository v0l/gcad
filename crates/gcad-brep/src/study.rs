use super::Model;
use crate::parse::Line;
use anyhow::Result;
use std::any::Any;
use std::path::Path;
use std::sync::OnceLock;

pub trait Study: Any + Send + Sync {
    fn boxed(&self) -> Box<dyn Study>;
    fn lines(&self) -> fn(&mut Model, &Line) -> Result<String>;
    fn solids(&self) -> Result<Vec<(String, &monstertruck::modeling::Solid)>>;
}

impl Clone for Box<dyn Study> {
    fn clone(&self) -> Self {
        self.boxed()
    }
}

static START: OnceLock<fn(&Path) -> Box<dyn Study>> = OnceLock::new();

pub fn set_study(start: fn(&Path) -> Box<dyn Study>) {
    let _ = START.set(start);
}

pub fn is_sim(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gsim"))
}

pub(crate) fn start_study(path: &Path) -> Box<dyn Study> {
    let start = START
        .get()
        .expect("a .gsim needs the simulation language, installed by gcad::sim::install");
    start(path)
}
