mod common;

use common::scratch;

const BEAM: &str = "rect 200 20 at=100,0\nbeam: extrude 10\nmaterial steel\n";

#[test]
fn static_run_reports_its_stages() {
    let dir = std::path::PathBuf::from(scratch("static_run_reports_its_stages"));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    std::fs::write(dir.join("beam.gcad"), BEAM).expect("part");
    let path = dir.join("test.gsim");
    std::fs::write(
        &path,
        "sim static\npart beam.gcad\nfix <X\nload >X force=0,0,-100\nmesh 4",
    )
    .expect("spec");
    let seen: std::sync::Arc<std::sync::Mutex<Vec<gcad::progress::Report>>> = Default::default();
    let into = seen.clone();
    let listening = gcad::progress::listen(move |r| {
        into.lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(r.clone())
    });
    let run = gcad::model::run_path(&path, &[], None).expect("reads");
    gcad::sim::simulate(run.model.sim.as_deref().expect("a simulation")).expect("simulates");
    drop(listening);
    let seen = seen.lock().unwrap_or_else(|e| e.into_inner());
    let finished = |name: &str| seen.iter().any(|r| r.finished && r.stage == name);
    for stage in ["meshing", "assembling the stiffness", "taking readings"] {
        assert!(finished(stage), "{stage} in {seen:?}");
    }
    assert!(finished("solving") || finished("factoring the stiffness"));
    let meshing = seen
        .iter()
        .rfind(|r| r.stage == "meshing")
        .expect("meshing");
    assert_eq!((meshing.done, meshing.total), (1.0, Some(1.0)));
}
