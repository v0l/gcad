use super::sculpt::fbm;
use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pattern {
    Noise,
    Pores,
    Scales,
    Wrinkles,
    Stitch,
    Hair,
    Dots,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Alpha {
    pub pattern: Pattern,
    pub size: f64,
    pub seed: u32,
}

const PATTERNS: &str = "noise, pores, scales, wrinkles, stitch, hair or dots";

impl Alpha {
    pub fn parse(text: &str, number: impl Fn(&str) -> Result<f64>) -> Result<Alpha> {
        let (name, inner) = match text.split_once('(') {
            Some((name, rest)) => match rest.strip_suffix(')') {
                Some(inner) => (name, Some(inner)),
                None => bail!("`{text}` misses its closing bracket"),
            },
            None => (text, None),
        };
        let pattern = match name {
            "noise" => Pattern::Noise,
            "pores" => Pattern::Pores,
            "scales" => Pattern::Scales,
            "wrinkles" => Pattern::Wrinkles,
            "stitch" => Pattern::Stitch,
            "hair" => Pattern::Hair,
            "dots" => Pattern::Dots,
            other => bail!("alpha is {PATTERNS} with an optional (size,seed), got `{other}`"),
        };
        let values: Vec<f64> = inner
            .map(|i| i.split(',').map(|v| number(v.trim())).collect())
            .transpose()?
            .unwrap_or_default();
        let size = values.first().copied().unwrap_or(0.0);
        if size < 0.0 {
            bail!("alpha size must be positive, got {size}");
        }
        let seed = values.get(1).copied().unwrap_or(1.0);
        if seed < 0.0 || seed.fract() != 0.0 {
            bail!("alpha seed is a whole number, got {seed}");
        }
        Ok(Alpha {
            pattern,
            size,
            seed: seed as u32,
        })
    }

    pub fn value(&self, u: f64, v: f64, world: [f64; 3], fallback: f64) -> f64 {
        let s = if self.size > 0.0 { self.size } else { fallback };
        let (x, y) = (u / s, v / s);
        match self.pattern {
            Pattern::Noise => fbm(world.map(|c| c / s), self.seed),
            Pattern::Pores => -cells(x, y, self.seed, 0.35).0.powi(2),
            Pattern::Dots => cells(x, y, self.seed, 0.45).0.powi(2),
            Pattern::Scales => {
                let (_, edge) = cells(x, y * 1.15 + 0.5 * x.floor(), self.seed, 0.0);
                (edge * 3.0).min(1.0)
            }
            Pattern::Wrinkles => {
                let wobble = 0.35 * fbm([x * 0.5, 0.0, self.seed as f64], self.seed);
                let t = (y + wobble).rem_euclid(1.0);
                -(1.0 - (2.0 * t - 1.0).abs()).powi(4)
            }
            Pattern::Stitch => {
                let along = x.rem_euclid(1.0);
                if along < 0.6 && y.abs() < 0.5 {
                    let a = (along / 0.6 * std::f64::consts::PI).sin();
                    let b = (1.0 - (2.0 * y).powi(2)).max(0.0);
                    a * b
                } else {
                    0.0
                }
            }
            Pattern::Hair => {
                let strand = (y + 0.25 * fbm([x * 0.3, y.floor(), self.seed as f64], self.seed))
                    .rem_euclid(1.0);
                let ridge = (1.0 - (2.0 * strand - 1.0).abs()).powi(2);
                ridge * 2.0 - 1.0
            }
        }
    }
}

fn jitter(i: i64, j: i64, seed: u32) -> (f64, f64) {
    let h = |k: u64| {
        let mut h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (j as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ (seed as u64 ^ k).wrapping_mul(0x1656_67B1_9E37_79F9);
        h ^= h >> 31;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 29;
        (h >> 11) as f64 / (1u64 << 53) as f64
    };
    (h(1), h(2))
}

fn cells(x: f64, y: f64, seed: u32, radius: f64) -> (f64, f64) {
    let (ci, cj) = (x.floor() as i64, y.floor() as i64);
    let (mut f1, mut f2) = (f64::INFINITY, f64::INFINITY);
    for di in -1..=1 {
        for dj in -1..=1 {
            let (i, j) = (ci + di, cj + dj);
            let (jx, jy) = jitter(i, j, seed);
            let (px, py) = (i as f64 + 0.15 + 0.7 * jx, j as f64 + 0.15 + 0.7 * jy);
            let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    let spot = if radius > 0.0 {
        (1.0 - f1 / radius).max(0.0)
    } else {
        0.0
    };
    (spot, (f2 - f1) * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(t: &str) -> Result<f64> {
        Ok(t.parse::<f64>()?)
    }

    #[test]
    fn patterns_stay_in_range_and_repeat() {
        for name in ["noise", "pores", "scales", "wrinkles", "stitch", "hair", "dots"] {
            let a = Alpha::parse(&format!("{name}(0.5,3)"), n).unwrap();
            for k in 0..200 {
                let (u, v) = (k as f64 * 0.037, k as f64 * 0.021 - 2.0);
                let x = a.value(u, v, [u, v, 0.3], 1.0);
                assert!((-1.0..=1.0).contains(&x), "{name} {x}");
                assert_eq!(x, a.value(u, v, [u, v, 0.3], 1.0));
            }
        }
    }

    #[test]
    fn stitch_is_a_dash_then_a_gap() {
        let a = Alpha::parse("stitch(1)", n).unwrap();
        assert!((a.value(0.3, 0.0, [0.0; 3], 1.0) - 1.0).abs() < 1.0e-12);
        assert_eq!(a.value(0.8, 0.0, [0.0; 3], 1.0), 0.0);
    }

    #[test]
    fn unknown_pattern_names_the_patterns() {
        let e = Alpha::parse("fur", n).unwrap_err().to_string();
        assert!(e.contains("wrinkles"), "{e}");
    }
}
