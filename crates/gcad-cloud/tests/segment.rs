use gcad_cloud::segment::{area, outline};

fn l_points(step: f64) -> Vec<[f64; 2]> {
    let n = (40.0 / step) as usize;
    let mut points = Vec::new();
    for i in 0..=n {
        for j in 0..=n {
            let (x, y) = (i as f64 * step, j as f64 * step);
            if x <= 15.0 || y <= 15.0 {
                points.push([x, y]);
            }
        }
    }
    points
}

#[test]
fn outline_of_an_l_shape_is_concave_with_the_right_area() {
    let corners = outline(&l_points(0.25), 0.2).unwrap();
    assert!((area(&corners) - 975.0).abs() < 10.0, "{}", area(&corners));
}
