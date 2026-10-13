use anyhow::{Result, bail};

pub const MU0: f64 = 4.0e-7 * std::f64::consts::PI;

#[derive(Clone, Debug)]
pub struct Curve {
    b: Vec<f64>,
    h: Vec<f64>,
    slope: Vec<f64>,
    work: Vec<f64>,
}

impl Curve {
    pub fn new(points: &[(f64, f64)]) -> Result<Curve> {
        let mut pairs: Vec<(f64, f64)> = points.to_vec();
        if pairs.first().is_none_or(|p| p.0 != 0.0 || p.1 != 0.0) {
            pairs.insert(0, (0.0, 0.0));
        }
        for w in pairs.windows(2) {
            if !(w[1].0 > w[0].0 && w[1].1 > w[0].1) {
                bail!(
                    "a B-H curve must rise in both H and B, but goes from {:?} to {:?}",
                    w[0],
                    w[1]
                );
            }
        }
        let h: Vec<f64> = pairs.iter().map(|p| p.0).collect();
        let b: Vec<f64> = pairs.iter().map(|p| p.1).collect();
        let n = b.len();
        let secant: Vec<f64> = (0..n - 1)
            .map(|i| (h[i + 1] - h[i]) / (b[i + 1] - b[i]))
            .collect();
        let mut slope = vec![0.0; n];
        slope[0] = secant[0];
        slope[n - 1] = secant[n - 2];
        for i in 1..n - 1 {
            let (a, c) = (secant[i - 1], secant[i]);
            let (wa, wc) = (b[i + 1] - b[i], b[i] - b[i - 1]);
            slope[i] = (wa + wc) / (wa / a + wc / c);
        }
        let mut work = vec![0.0; n];
        for i in 0..n - 1 {
            let d = b[i + 1] - b[i];
            work[i + 1] =
                work[i] + d * (h[i] + h[i + 1]) / 2.0 + d * d * (slope[i] - slope[i + 1]) / 12.0;
        }
        Ok(Curve { b, h, slope, work })
    }

    fn span(&self, b: f64) -> usize {
        self.b
            .partition_point(|&x| x <= b)
            .clamp(1, self.b.len() - 1)
            - 1
    }

    pub fn h(&self, b: f64) -> (f64, f64, f64) {
        let last = self.b.len() - 1;
        if b >= self.b[last] {
            let over = b - self.b[last];
            let nu = 1.0 / MU0;
            return (
                self.h[last] + over * nu,
                nu,
                self.work[last] + over * self.h[last] + 0.5 * over * over * nu,
            );
        }
        let i = self.span(b);
        let d = self.b[i + 1] - self.b[i];
        let t = (b - self.b[i]) / d;
        let (h0, h1, m0, m1) = (
            self.h[i],
            self.h[i + 1],
            self.slope[i] * d,
            self.slope[i + 1] * d,
        );
        let (t2, t3) = (t * t, t * t * t);
        let h = (2.0 * t3 - 3.0 * t2 + 1.0) * h0
            + (t3 - 2.0 * t2 + t) * m0
            + (-2.0 * t3 + 3.0 * t2) * h1
            + (t3 - t2) * m1;
        let dh = ((6.0 * t2 - 6.0 * t) * h0
            + (3.0 * t2 - 4.0 * t + 1.0) * m0
            + (-6.0 * t2 + 6.0 * t) * h1
            + (3.0 * t2 - 2.0 * t) * m1)
            / d;
        let t4 = t2 * t2;
        let work = self.work[i]
            + d * ((0.5 * t4 - t3 + t) * h0
                + (0.25 * t4 - 2.0 / 3.0 * t3 + 0.5 * t2) * m0
                + (-0.5 * t4 + t3) * h1
                + (0.25 * t4 - t3 / 3.0) * m1);
        (h, dh, work)
    }

    pub fn reluctivity(&self, b: f64) -> (f64, f64) {
        if b < 1.0e-9 {
            return (self.slope[0], self.slope[0]);
        }
        let (h, dh, _) = self.h(b);
        (h / b, dh)
    }

    pub fn energy(&self, b: f64) -> f64 {
        self.h(b).2
    }
}

pub fn named(name: &str) -> Option<Curve> {
    let table: &[(f64, f64)] = match name {
        "m19" => &M19,
        "1010" | "steel" | "low_carbon" => &STEEL_1010,
        _ => return None,
    };
    Curve::new(table).ok()
}

pub const NAMES: &[&str] = &["m19", "1010"];

const M19: [(f64, f64); 46] = [
    (15.120714, 0.05),
    (22.718292, 0.1),
    (27.842733, 0.15),
    (31.871434, 0.2),
    (35.365044, 0.25),
    (38.600588, 0.3),
    (41.736202, 0.35),
    (44.873979, 0.4),
    (48.087807, 0.45),
    (51.437236, 0.5),
    (54.975221, 0.55),
    (58.752993, 0.6),
    (62.823644, 0.65),
    (67.245285, 0.7),
    (72.084406, 0.75),
    (77.4201, 0.8),
    (83.350021, 0.85),
    (89.999612, 0.9),
    (97.537353, 0.95),
    (106.201406, 1.0),
    (116.348464, 1.05),
    (128.547329, 1.1),
    (143.765431, 1.15),
    (163.754169, 1.2),
    (191.868158, 1.25),
    (234.833507, 1.3),
    (306.509769, 1.35),
    (435.255202, 1.4),
    (674.911968, 1.45),
    (1108.325569, 1.5),
    (1813.085468, 1.55),
    (2801.217421, 1.6),
    (4053.653117, 1.65),
    (5591.10689, 1.7),
    (7448.318413, 1.75),
    (9708.81567, 1.8),
    (12486.931615, 1.85),
    (16041.483644, 1.9),
    (21249.420624, 1.95),
    (31313.495878, 2.0),
    (53589.446877, 2.05),
    (88477.484601, 2.1),
    (124329.41054, 2.15),
    (159968.5693, 2.2),
    (197751.604272, 2.25),
    (234024.751347, 2.3),
];

const STEEL_1010: [(f64, f64); 37] = [
    (79.577472, 0.211862),
    (100.182101, 0.265665),
    (126.121793, 0.332377),
    (158.77793, 0.414377),
    (199.889571, 0.513811),
    (251.646061, 0.631899),
    (316.80362, 0.767784),
    (398.832128, 0.917018),
    (502.099901, 1.070353),
    (632.106325, 1.214255),
    (795.774715, 1.334637),
    (1001.821011, 1.422981),
    (1261.217929, 1.480634),
    (1587.779301, 1.517214),
    (1998.89571, 1.544515),
    (2516.460605, 1.571296),
    (3168.036204, 1.602049),
    (3988.321282, 1.638404),
    (5020.999013, 1.68049),
    (6321.06325, 1.727311),
    (7957.747155, 1.776659),
    (10018.210114, 1.825401),
    (12612.179293, 1.870557),
    (15877.79301, 1.910809),
    (19988.957103, 1.947222),
    (25164.606052, 1.982328),
    (31680.362037, 2.018252),
    (39883.212823, 2.055398),
    (50209.990127, 2.092545),
    (63210.632497, 2.128095),
    (79577.471546, 2.161612),
    (100182.101136, 2.194644),
    (126121.792926, 2.230339),
    (158777.930096, 2.272386),
    (199889.57103, 2.324282),
    (251646.060522, 2.389356),
    (316803.62037, 2.471238),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_passes_its_points_and_integrates() {
        let curve = named("m19").unwrap();
        for &(h, b) in &M19 {
            assert!((curve.h(b).0 - h).abs() < 1.0e-6 * h.max(1.0));
        }
        let b = 1.37;
        let steps = 20_000;
        let mut work = 0.0;
        for k in 0..steps {
            let x = (k as f64 + 0.5) / steps as f64 * b;
            work += curve.h(x).0 * b / steps as f64;
        }
        assert!((curve.energy(b) - work).abs() < 1.0e-6 * work);
        let (h, dh, _) = curve.h(b);
        let e = 1.0e-6;
        let numeric = (curve.h(b + e).0 - curve.h(b - e).0) / (2.0 * e);
        assert!(
            (dh - numeric).abs() < 1.0e-4 * dh,
            "{dh} {numeric} at h {h}"
        );
        let above = curve.h(2.5);
        assert!((above.1 - 1.0 / MU0).abs() < 1.0e-9 / MU0);
    }
}
