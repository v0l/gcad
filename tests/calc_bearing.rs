use gcad::calc::bearing;

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} vs {expected}"
    );
}

#[test]
fn bearing_608_matches_the_catalogue() {
    let b = bearing::find("608").expect("608");
    assert_eq!((b.bore, b.outer, b.width), (8.0, 22.0, 7.0));
    near(b.dynamic_kn, 3.293, 1e-9);
    near(b.static_kn, 1.379, 1e-9);
}

#[test]
fn l10_life_is_the_cube_of_the_load_ratio() {
    let b = bearing::find("6205").expect("6205");
    near(bearing::life_million_revs(b, 14.0 / 2.0), 8.0, 1e-9);
    near(bearing::life_hours(8.0, 1000.0), 8.0e6 / 60000.0, 1e-9);
}

#[test]
fn unknown_bearings_list_the_known_ones() {
    let error = bearing::find("9999").expect_err("unknown").to_string();
    assert!(error.contains("6205") && error.contains("LM8UU"), "{error}");
}
