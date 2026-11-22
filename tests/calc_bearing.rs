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

fn row(name: &str) -> (f64, f64, f64, f64, f64) {
    let b = bearing::find(name).expect(name);
    (b.bore, b.outer, b.width, b.dynamic_kn, b.static_kn)
}

#[test]
fn light_series_60xx_match_iso_15() {
    assert_eq!(row("6000").0, 10.0);
    assert_eq!(row("6004").0, 20.0);
    assert_eq!(row("6004").1, 42.0);
    assert_eq!(row("6004").2, 12.0);
    near(row("6005").3, 11.2, 1e-9);
    near(row("6008").4, 11.6, 1e-9);
}

#[test]
fn medium_series_62xx_match_iso_15() {
    assert_eq!((row("6204").0, row("6204").1, row("6204").2), (20.0, 47.0, 14.0));
    near(row("6206").3, 19.5, 1e-9);
    near(row("6210").4, 23.2, 1e-9);
}

#[test]
fn heavy_series_63xx_match_iso_15() {
    assert_eq!((row("6305").0, row("6305").1, row("6305").2), (25.0, 62.0, 17.0));
    near(row("6310").3, 61.8, 1e-9);
    near(row("6308").4, 24.0, 1e-9);
}

#[test]
fn shoulders_follow_din_5418() {
    let s = bearing::shoulders(bearing::find("6205").expect("6205"));
    near(s.height, 4.5, 1e-9);
    near(s.shaft_min, 34.0, 1e-9);
    near(s.housing_max, 43.0, 1e-9);
    near(s.fillet_max, 1.5, 1e-9);
    let light = bearing::shoulders(bearing::find("6005").expect("6005"));
    near(light.height, 2.3, 1e-9);
}

#[test]
fn linear_thrust_and_flanged_rows() {
    let lm = bearing::find("lm8uu").expect("LM8UU");
    assert_eq!((lm.bore, lm.outer, lm.width), (8.0, 15.0, 24.0));
    near(lm.dynamic_kn, 0.274, 1e-9);
    let t = bearing::find("51104").expect("51104");
    assert_eq!((t.bore, t.outer, t.width), (20.0, 35.0, 10.0));
    let f = bearing::find("F608").expect("F608");
    near(f.flange_dia, 25.0, 1e-9);
    near(f.flange_width, 1.5, 1e-9);
}

#[test]
fn axial_load_raises_the_equivalent_load() {
    let b = bearing::find("6205").expect("6205");
    near(bearing::equivalent_load(b, 2.0, 0.0).load_kn, 2.0, 1e-9);
    let mixed = bearing::equivalent_load(b, 2.0, 1.0);
    assert!(mixed.axial_matters && mixed.load_kn > 2.0, "{mixed:?}");
    near(bearing::equivalent_load(b, 2.0, 0.1).load_kn, 2.0, 1e-9);
}

#[test]
fn every_bearing_has_a_seat_that_fits_its_rings() {
    for b in bearing::all() {
        assert!(b.bore < b.outer && b.dynamic_kn > 0.0 && b.static_kn > 0.0, "{}", b.name);
        if matches!(b.kind, bearing::Kind::Radial | bearing::Kind::Flanged) {
            let s = bearing::shoulders(b);
            assert!(s.shaft_min < s.housing_max, "{}", b.name);
        }
    }
}

#[test]
fn unknown_bearings_list_the_known_ones() {
    let error = bearing::find("9999").expect_err("unknown").to_string();
    assert!(error.contains("6205") && error.contains("LM8UU"), "{error}");
}
