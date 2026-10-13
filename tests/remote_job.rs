mod common;

use common::scratch;
use std::path::Path;

fn local_and_packed(spec: &Path) -> (serde_json::Value, serde_json::Value) {
    let run = gcad::model::run_path(spec, &[], None).expect("reads");
    let spec_model = run.model.sim.as_deref().expect("a simulation");
    let stamp = gcad::sim::stamp(spec, &[]);
    let local = gcad::sim::simulate(spec_model).expect("solves here");

    let packed = gcad::sim::job::pack(spec_model, stamp.clone()).expect("packs");
    let unpacked = gcad::sim::job::unpack(&packed).expect("unpacks");
    assert_eq!(unpacked.stamp, stamp);
    let remote = gcad::sim::simulate(&unpacked.spec).expect("solves from the job");
    (local.to_json(&stamp), remote.to_json(&unpacked.stamp))
}

fn same(local: &serde_json::Value, remote: &serde_json::Value) {
    assert_eq!(local["inputs"], remote["inputs"]);
    assert_eq!(local["notes"], remote["notes"]);
    let field = |v: &serde_json::Value| v["field"]["values"].as_array().map(Vec::len);
    assert_eq!(field(local), field(remote), "the mesh differs");
    let readings = |v: &serde_json::Value| v["readings"].as_array().cloned().unwrap_or_default();
    let (a, b) = (readings(local), readings(remote));
    assert_eq!(a.len(), b.len());
    let largest = |unit: &serde_json::Value| {
        a.iter()
            .filter(|r| &r["unit"] == unit)
            .map(|r| r["value"].as_f64().unwrap().abs())
            .fold(0.0, f64::max)
    };
    for (here, there) in a.iter().zip(&b) {
        assert_eq!(here["name"], there["name"]);
        let (x, y) = (
            here["value"].as_f64().unwrap(),
            there["value"].as_f64().unwrap(),
        );
        let scale = largest(&here["unit"]).max(x.abs()).max(1.0e-12);
        assert!(
            (x - y).abs() <= 1.0e-7 * scale,
            "{}: {x} against {y}",
            here["name"]
        );
    }
}

#[test]
fn packed_static_solves_the_same() {
    let (local, remote) = local_and_packed(Path::new("examples/parts/bracket.gsim"));
    same(&local, &remote);
}

#[test]
fn packed_thermal_solves_the_same() {
    let (local, remote) = local_and_packed(Path::new("examples/parts/heatsink.gsim"));
    same(&local, &remote);
}

#[cfg(feature = "gpu-tests")]
#[test]
fn packed_flow_solves_the_same() {
    let (local, remote) = local_and_packed(Path::new("examples/flow/cube.gsim"));
    same(&local, &remote);
}

#[test]
fn torque_stays_local() {
    let dir = std::path::PathBuf::from(scratch("job_torque"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("bar.gcad"),
        "rect 100 10 at=50,0\nextrude 10 both\nmaterial aluminium\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("base.gcad"),
        "rect 20 20 at=0,-30\nextrude 10 both\nmaterial steel\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("arm.gasm"),
        "part base base.gcad\npart bar bar.gcad\naxis pivot 0,0,0 0,1,0\njoint swing bar base turn about=pivot min=-90 max=90\n",
    )
    .unwrap();
    let spec = dir.join("t.gsim");
    std::fs::write(&spec, "sim torque\npart arm.gasm\nsweep swing steps=4\n").unwrap();
    let run = gcad::model::run_path(&spec, &[], None).expect("reads");
    let error = gcad::sim::job::pack(run.model.sim.as_deref().unwrap(), vec![])
        .expect_err("torque is refused");
    assert!(error.to_string().contains("run it locally"), "{error}");
}

#[test]
fn other_versions_are_refused() {
    assert!(gcad::sim::job::unpack(b"not gzip").is_err());
}

fn written(test: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(scratch(test));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    dir
}

#[test]
fn packed_warming_carries_its_heat_simulation() {
    let dir = written(
        "job_warm",
        &[
            (
                "bar.gcad",
                "rect 100 10 at=50,0\nbar: extrude 10 both\nmaterial steel\n",
            ),
            (
                "hot.gsim",
                "sim thermal\npart bar.gcad\ntemperature <X 20\ntemperature >X 120\nmesh 4\n",
            ),
            (
                "t.gsim",
                "sim static\npart bar.gcad\nfix <X\nwarm hot.gsim from=20\nmesh 4\n",
            ),
        ],
    );
    let (local, remote) = local_and_packed(&dir.join("t.gsim"));
    same(&local, &remote);
}

#[cfg(feature = "gpu-tests")]
#[test]
fn packed_wind_load_carries_the_pressure() {
    let dir = written(
        "job_wind",
        &[
            ("cube.gcad", "plane XY\nrect 100 100\nblock: extrude 100\n"),
            (
                "flow.gsim",
                "sim flow\npart cube.gcad\nwind 10 toward=1,0,0\nmesh 8\naverage 4\n",
            ),
            (
                "t.gsim",
                "sim static\npart cube.gcad\nmaterial aluminium\nbase: fix <Z\nwind flow.gsim\nmesh 15\n",
            ),
        ],
    );
    let flow = dir.join("flow.gsim");
    let run = gcad::model::run_path(&flow, &[], None).expect("reads");
    let outcome = gcad::sim::simulate(run.model.sim.as_deref().unwrap()).expect("flows");
    gcad::sim::save(&flow, &[], &outcome).expect("saves");
    let (local, remote) = local_and_packed(&dir.join("t.gsim"));
    same(&local, &remote);
}

#[test]
fn packed_bearings_solve_the_same() {
    let dir = written(
        "job_bearing",
        &[
            (
                "housing.gcad",
                "rect 40 40\nh: extrude 20\nplane h.end\nbore: hole 20.04 0,0\nmaterial steel\n",
            ),
            (
                "shaft.gcad",
                "circle 20\ns: extrude 300\nplane s.end\nrect 14 14\nsq: extrude 10\nmaterial steel\n",
            ),
            (
                "pair.gasm",
                "part a housing.gcad\nmove a 0,0,40\npart b housing.gcad\nmove b 0,0,200\npart s shaft.gcad\n",
            ),
            (
                "t.gsim",
                "sim static\npart pair.gasm\nfix a:h.start\nfix b:h.start\nkey: slide s:>X\nbearing a:bore.side s axial C0=2900\nbearing b:bore.side s\nload s:sq.end force=0,-1000,0\nmesh 4\n",
            ),
        ],
    );
    let (local, remote) = local_and_packed(&dir.join("t.gsim"));
    same(&local, &remote);
}
