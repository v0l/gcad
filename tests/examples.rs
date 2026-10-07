mod common;

use common::*;
use std::f64::consts::PI;

#[test]
fn plate() {
    assert_volume(&build(&part("plate")), 3441.98, 0.001);
}

#[test]
fn bracket() {
    assert_volume(&build(&part("bracket")), 13800.03, 0.001);
}

#[test]
fn ring() {
    assert_volume(&build(&part("ring")), PI * (100.0 - 36.0) * 10.0, 0.0005);
}

#[test]
fn pipe() {
    let centerline = 12.0 + 14.0 + 12.0 + 2.0 * (PI / 2.0 * 8.0);
    assert_volume(&build(&part("pipe")), PI * 9.0 * centerline, 0.0005);
}

#[test]
fn enclosure() {
    let model = build(&part("enclosure"));
    assert_eq!(model.solids().len(), 2);
    assert_volume(&model, 22501.75 + 8895.46, 0.002);
}
