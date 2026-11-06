use super::{P3, add, length, scale, sub};

pub type Quadric = [f64; 10];

pub fn plane_quadric(n: P3, d: f64, w: f64) -> Quadric {
    let [a, b, c] = n;
    [
        a * a * w,
        a * b * w,
        a * c * w,
        a * d * w,
        b * b * w,
        b * c * w,
        b * d * w,
        c * c * w,
        c * d * w,
        d * d * w,
    ]
}

pub fn plus(a: &Quadric, b: &Quadric) -> Quadric {
    std::array::from_fn(|k| a[k] + b[k])
}

pub fn error(q: &Quadric, p: P3) -> f64 {
    let [x, y, z] = p;
    q[0] * x * x
        + 2.0 * q[1] * x * y
        + 2.0 * q[2] * x * z
        + 2.0 * q[3] * x
        + q[4] * y * y
        + 2.0 * q[5] * y * z
        + 2.0 * q[6] * y
        + q[7] * z * z
        + 2.0 * q[8] * z
        + q[9]
}

pub fn optimum(q: &Quadric, a: P3, b: P3) -> P3 {
    let m = [[q[0], q[1], q[2]], [q[1], q[4], q[5]], [q[2], q[5], q[7]]];
    let rhs = [-q[3], -q[6], -q[8]];
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let scale_of = m.iter().flatten().fold(0.0f64, |s, v| s.max(v.abs()));
    if det.abs() > 1.0e-9 * scale_of.powi(3).max(1.0e-30) {
        let solve = |col: usize| {
            let mut r = m;
            for row in 0..3 {
                r[row][col] = rhs[row];
            }
            (r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
                - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
                + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]))
                / det
        };
        let p = [solve(0), solve(1), solve(2)];
        let span = length(sub(a, b));
        let mid = scale(add(a, b), 0.5);
        if length(sub(p, mid)) <= 2.0 * span {
            return p;
        }
    }
    [a, scale(add(a, b), 0.5), b]
        .into_iter()
        .min_by(|x, y| error(q, *x).total_cmp(&error(q, *y)))
        .expect("three candidates")
}

pub fn vertex_quadrics(positions: &[P3], triangles: &[[u32; 3]]) -> Vec<Quadric> {
    let mut out = vec![[0.0; 10]; positions.len()];
    for &t in triangles {
        let [a, b, c] = t.map(|i| positions[i as usize]);
        let raw = super::cross(sub(b, a), sub(c, a));
        let area = length(raw) / 2.0;
        if area <= 0.0 {
            continue;
        }
        let normal = super::normalized(raw);
        let q = plane_quadric(normal, -super::dot(normal, a), area);
        for i in t {
            out[i as usize] = plus(&out[i as usize], &q);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_patch_costs_nothing_and_a_lifted_point_costs_its_distance_squared() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let q = vertex_quadrics(&positions, &[[0, 1, 2]]);
        let sum = plus(&q[0], &q[1]);
        assert!(error(&sum, [0.5, 0.2, 0.0]).abs() < 1.0e-12);
        let lifted = error(&q[2], [0.0, 1.0, 2.0]);
        assert!((lifted - 4.0 * 0.5).abs() < 1.0e-12);
    }
}
