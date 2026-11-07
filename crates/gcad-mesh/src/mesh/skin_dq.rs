use super::rig::{Quat, Weights, Xf};
use super::{P3, add, cross, scale, sub};

#[derive(Clone, Copy, Debug)]
pub struct Dual {
    pub real: Quat,
    pub dual: Quat,
}

fn quat_mul(a: Quat, b: Quat) -> Quat {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

pub fn quat_of(m: &[[f64; 3]; 3]) -> Quat {
    let trace = m[0][0] + m[1][1] + m[2][2];
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (m[2][1] - m[1][2]) / s,
            (m[0][2] - m[2][0]) / s,
            (m[1][0] - m[0][1]) / s,
            0.25 * s,
        ]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [
            0.25 * s,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
            (m[2][1] - m[1][2]) / s,
        ]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [
            (m[0][1] + m[1][0]) / s,
            0.25 * s,
            (m[1][2] + m[2][1]) / s,
            (m[0][2] - m[2][0]) / s,
        ]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [
            (m[0][2] + m[2][0]) / s,
            (m[1][2] + m[2][1]) / s,
            0.25 * s,
            (m[1][0] - m[0][1]) / s,
        ]
    };
    let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    q.map(|v| v / n)
}

impl Dual {
    pub fn of(xf: &Xf) -> Dual {
        let real = quat_of(&xf.r);
        let t = [xf.t[0], xf.t[1], xf.t[2], 0.0];
        let dual = quat_mul(t, real).map(|v| v * 0.5);
        Dual { real, dual }
    }

    fn apply(&self, p: P3) -> P3 {
        let n = self.real.iter().map(|v| v * v).sum::<f64>().sqrt();
        let r = self.real.map(|v| v / n);
        let d = self.dual.map(|v| v / n);
        let v = [r[0], r[1], r[2]];
        let w = r[3];
        let rotated = add(p, scale(cross(v, add(cross(v, p), scale(p, w))), 2.0));
        let dv = [d[0], d[1], d[2]];
        let t = scale(sub(add(scale(dv, w), cross(v, dv)), scale(v, d[3])), 2.0);
        add(rotated, t)
    }
}

pub fn blend_point(p: P3, weights: &[(usize, f64)], xf: &[Xf], duals: &[Dual], dq: f64) -> P3 {
    let lbs = || {
        weights
            .iter()
            .fold([0.0; 3], |sum, (b, w)| add(sum, scale(xf[*b].apply(p), *w)))
    };
    if dq <= 0.0 {
        return lbs();
    }
    let Some(&(first, _)) = weights.iter().max_by(|a, b| a.1.total_cmp(&b.1)) else {
        return p;
    };
    let pivot = duals[first].real;
    let mut real = [0.0; 4];
    let mut dual = [0.0; 4];
    for &(b, w) in weights {
        let q = duals[b];
        let sign = if dot4(q.real, pivot) < 0.0 { -w } else { w };
        for k in 0..4 {
            real[k] += q.real[k] * sign;
            dual[k] += q.dual[k] * sign;
        }
    }
    let skinned = Dual { real, dual }.apply(p);
    if dq >= 1.0 {
        return skinned;
    }
    let linear = lbs();
    add(scale(linear, 1.0 - dq), scale(skinned, dq))
}

fn dot4(a: Quat, b: Quat) -> f64 {
    a.iter().zip(&b).map(|(x, y)| x * y).sum()
}

pub fn pose_positions(positions: &[P3], skin: &Weights, xf: &[Xf], dq: f64) -> Vec<P3> {
    let duals: Vec<Dual> = xf.iter().map(Dual::of).collect();
    positions
        .iter()
        .zip(skin)
        .map(|(p, w)| blend_point(*p, w, xf, &duals, dq))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::rig::{quat_axis_angle, quat_matrix};

    fn xf(axis: P3, degrees: f64, t: P3) -> Xf {
        Xf {
            r: quat_matrix(quat_axis_angle(axis, degrees).unwrap()),
            t,
        }
    }

    #[test]
    fn one_bone_moves_a_point_like_its_matrix() {
        let x = xf([0.3, 1.0, 0.2], 70.0, [1.0, -2.0, 0.5]);
        let d = [Dual::of(&x)];
        let p = [0.4, 1.5, -2.0];
        let a = blend_point(p, &[(0, 1.0)], &[x], &d, 1.0);
        let b = x.apply(p);
        assert!((0..3).all(|k| (a[k] - b[k]).abs() < 1.0e-9), "{a:?} {b:?}");
    }

    #[test]
    fn half_a_twist_keeps_the_radius_where_linear_blending_collapses_it() {
        let still = xf([0.0, 0.0, 1.0], 0.0, [0.0; 3]);
        let turned = xf([0.0, 0.0, 1.0], 180.0, [0.0; 3]);
        let xfs = [still, turned];
        let d: Vec<Dual> = xfs.iter().map(Dual::of).collect();
        let p = [1.0, 0.0, 0.0];
        let w = [(0, 0.5), (1, 0.5)];
        let lbs = blend_point(p, &w, &xfs, &d, 0.0);
        let dq = blend_point(p, &w, &xfs, &d, 1.0);
        let r = |v: P3| (v[0] * v[0] + v[1] * v[1]).sqrt();
        assert!(r(lbs) < 1.0e-9, "{lbs:?}");
        assert!((r(dq) - 1.0).abs() < 1.0e-9, "{dq:?}");
        let half = blend_point(p, &w, &xfs, &d, 0.5);
        assert!((r(half) - 0.5).abs() < 1.0e-9);
    }
}
