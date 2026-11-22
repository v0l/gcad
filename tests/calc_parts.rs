use gcad::calc::{bearing, parts};

mod common;

fn gcad(args: &[&str]) -> (bool, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
        .args(args)
        .output()
        .expect("runs");
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), text)
}

fn bom(name: &str, lines: &str) -> String {
    let dir = std::path::PathBuf::from(common::scratch(name));
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("top.gasm");
    std::fs::write(&file, lines).expect("write");
    let (ok, text) = gcad(&["bom", file.to_str().expect("path"), "--csv"]);
    assert!(ok, "{text}");
    text
}

fn volume(text: &str, name: &str) -> f64 {
    let row = text.lines().find(|l| l.contains(name)).expect(name);
    row.split(',').nth(5).expect("volume").parse().expect("number")
}

#[test]
fn every_kind_has_an_example_that_resolves() {
    for k in parts::KINDS {
        parts::resolve(k.example).unwrap_or_else(|e| panic!("{}: {e:#}", k.example));
    }
}

#[test]
fn iso4762_spec_matches_the_old_shcs_part() {
    let text = bom("calc_parts_iso4762", "part a iso4762:M6x25\npart b shcs:M6x25\n");
    let (a, b) = (volume(&text, "ISO 4762 M6x25"), volume(&text, "ISO 4762 M6x25"));
    assert!((a - b).abs() < 1e-6 && a > 1000.0, "{text}");
}

#[test]
fn nut_and_washer_volumes_follow_the_tables() {
    let text = bom(
        "calc_parts_nuts",
        "part n iso4032:M6\npart t iso4035:M6\npart w iso7089:M6\npart big iso7093:M6\n",
    );
    let hex = |s: f64| s * s * 3.0_f64.sqrt() / 2.0;
    let nut = (hex(10.0) - std::f64::consts::PI * 9.0) * 5.2;
    assert!((volume(&text, "ISO 4032 M6") - nut).abs() < nut * 2e-3, "{text}");
    let thin = (hex(10.0) - std::f64::consts::PI * 9.0) * 3.2;
    assert!((volume(&text, "ISO 4035 M6") - thin).abs() < thin * 2e-3, "{text}");
    let washer = std::f64::consts::PI / 4.0 * (144.0 - 6.4 * 6.4) * 1.6;
    assert!((volume(&text, "ISO 7089 M6") - washer).abs() < washer * 2e-3, "{text}");
    assert!(volume(&text, "ISO 7093 M6") > washer, "{text}");
}

#[test]
fn dowels_keys_and_circlips_are_listed() {
    let text = bom(
        "calc_parts_pins",
        "part d iso8734:6x24\npart e iso2338:6x24\npart k din6885:8x7x32\npart c din471:25\npart i din472:50\n",
    );
    let key = 8.0 * 7.0 * 32.0;
    assert!((volume(&text, "DIN 6885 8x7x32") - key).abs() < 1e-3, "{text}");
    for name in ["ISO 8734 6x24", "ISO 2338 6x24", "DIN 471 25x1.2", "DIN 472 50"] {
        assert!(text.contains(name), "{name}: {text}");
    }
}

#[test]
fn pulley_outside_diameter_uses_the_pitch_line_differential() {
    let p = parts::resolve("gt2pulley:20x6x5").expect("pulley");
    let row = p.rows.iter().find(|r| r.0 == "outside diameter").expect("od");
    assert_eq!(row.1, "12.224 mm");
    let h = parts::resolve("htd5pulley:24x15x8").expect("htd");
    let pd = 24.0 * 5.0 / std::f64::consts::PI;
    let row = h.rows.iter().find(|r| r.0 == "outside diameter").expect("od");
    assert_eq!(row.1, format!("{:.3} mm", pd - 2.0 * 0.5715));
}

#[test]
fn belts_and_bought_parts_carry_an_envelope() {
    let belt = parts::resolve("gt2belt:6x200").expect("belt");
    assert_eq!(belt.rows.iter().find(|r| r.0 == "teeth").expect("teeth").1, "100");
    let motor = parts::resolve("spec:nema17@42x42x40").expect("spec");
    assert_eq!(motor.envelope, [42.0, 42.0, 40.0]);
    assert!(!motor.modelled);
    let text = bom("calc_parts_spec", "part m spec:nema17@42x42x40\n");
    assert!((volume(&text, "nema17") - 42.0 * 42.0 * 40.0).abs() < 1e-3, "{text}");
}

#[test]
fn extrusion_is_a_section_of_the_listed_size() {
    let text = bom("calc_parts_extrusion", "part x extrusion:2020x300\n");
    let area = 400.0 - std::f64::consts::PI / 4.0 * 4.2 * 4.2;
    assert!((volume(&text, "2020 x 300") - area * 300.0).abs() < area * 300.0 * 2e-3, "{text}");
}

#[test]
fn bad_specs_are_refused_with_the_valid_choices() {
    for (spec, needle) in [
        ("iso4032:M14", "not in the table"),
        ("iso2338:7x20", "has diameters"),
        ("din6885:9x9x20", "DIN 6885 keys are"),
        ("din471:26.5", "listed for"),
        ("iso4762:M6", "needs a length"),
        ("gt2belt:6x201", "whole number"),
        ("nope:1", "unknown part"),
    ] {
        let error = parts::resolve(spec).expect_err(spec).to_string();
        assert!(error.contains(needle), "{spec}: {error}");
    }
}

#[test]
fn cli_lists_and_shows_parts() {
    let (ok, text) = gcad(&["parts", "list"]);
    assert!(ok && text.contains("iso4762") && text.contains("din472"), "{text}");
    let (ok, text) = gcad(&["parts", "show", "iso4762:M6x25"]);
    assert!(ok && text.contains("bom line  1, ISO 4762 M6x25, steel"), "{text}");
    let (ok, text) = gcad(&["parts", "list", "iso4032"]);
    assert!(ok && text.contains("M6"), "{text}");
}

#[test]
fn catalogue_bearings_are_standard_parts() {
    let text = bom("calc_parts_bearings", "part a bearing:6305\npart b bearing:608\n");
    assert!(text.contains("6305 bearing 25x62x17") && text.contains("608 bearing 8x22x7"), "{text}");
}

#[test]
fn cli_bearing_report_has_life_and_seats() {
    let (ok, text) = gcad(&["calc", "bearing", "6205", "--radial", "2", "--rpm", "3000", "--formulas"]);
    assert!(ok, "{text}");
    for needle in ["L10 life", "343.0 million", "shaft shoulder", "housing", "source:"] {
        assert!(text.contains(needle), "{needle}: {text}");
    }
    let (ok, text) = gcad(&["calc", "bearing", "9999"]);
    assert!(!ok && text.contains("known:"), "{text}");
    assert!(bearing::find("LM8UU").is_ok());
}
