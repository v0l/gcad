use std::path::{Path, PathBuf};

fn directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jobs")
}

fn describe(unpacked: &gcad::sim::job::Unpacked) -> String {
    let mut out = serde_json::to_string_pretty(&unpacked.spec).expect("the spec serializes");
    let model = unpacked.spec.target.as_ref().expect("a rebuilt model");
    let mut bodies: Vec<String> = model
        .bodies
        .iter()
        .map(|(name, solid)| {
            let volume =
                gcad::geometry::volume_at(solid, gcad::geometry::bounds(solid).diameter() * 2.0e-5);
            format!("body {name}: volume {volume:.5e}")
        })
        .collect();
    bodies.sort();
    let mut materials: Vec<String> = model
        .materials
        .iter()
        .map(|(name, material)| {
            format!(
                "material {name}: {}",
                serde_json::to_string(material).expect("the material serializes")
            )
        })
        .collect();
    materials.sort();
    out.push('\n');
    out.push_str(&bodies.join("\n"));
    out.push('\n');
    out.push_str(&materials.join("\n"));
    out.push('\n');
    out
}

fn check(name: &str, example: &str) {
    let job = directory().join(format!("{name}.job"));
    let golden = directory().join(format!("{name}.txt"));
    if std::env::var_os("GCAD_UPDATE_FIXTURES").is_some() {
        let spec_path = Path::new(example);
        let run = gcad::model::run_path(spec_path, &[], None).expect("reads the example");
        let spec = run.model.sim.as_deref().expect("a simulation");
        let packed =
            gcad::sim::job::pack(spec, gcad::sim::stamp(spec_path, &[])).expect("packs the job");
        std::fs::create_dir_all(directory()).expect("fixture directory");
        std::fs::write(&job, &packed).expect("writes the job");
        let unpacked = gcad::sim::job::unpack(&packed).expect("unpacks what it just packed");
        std::fs::write(&golden, describe(&unpacked)).expect("writes the description");
        std::fs::write(
            directory().join(format!("{name}.version")),
            env!("GCAD_BUILD_VERSION"),
        )
        .expect("writes the version");
        return;
    }
    let bytes = std::fs::read(&job).unwrap_or_else(|e| panic!("{}: {e}", job.display()));
    let unpacked = gcad::sim::job::unpack(&bytes).unwrap_or_else(|e| {
        panic!(
            "a stored {name} job no longer decodes: {e:#}\nthe job layout changed, so bump the minor version and regenerate with GCAD_UPDATE_FIXTURES=1"
        )
    });
    let expected = std::fs::read_to_string(&golden).expect("the stored description");
    assert_eq!(
        describe(&unpacked),
        expected,
        "a stored {name} job decodes to something else: the job layout changed (an enum variant inserted or a field moved), so bump the minor version and regenerate with GCAD_UPDATE_FIXTURES=1"
    );
}

#[test]
fn a_static_job_packed_earlier_decodes_to_the_same_problem() {
    check("static", "examples/parts/bracket.gsim");
}

#[test]
fn a_thermal_job_packed_earlier_decodes_to_the_same_problem() {
    check("thermal", "examples/parts/heatsink.gsim");
}

#[test]
fn a_job_from_another_build_of_the_same_line_is_accepted() {
    let bytes = std::fs::read(directory().join("static.job")).expect("the stored job");
    let unpacked = gcad::sim::job::unpack(&bytes).expect("accepted");
    assert!(unpacked.spec.target.is_some());
    let built = env!("GCAD_BUILD_VERSION");
    let packer = std::fs::read_to_string(directory().join("static.version")).expect("its version");
    assert!(gcad_sims_proto::same_line(packer.trim(), built));
}
