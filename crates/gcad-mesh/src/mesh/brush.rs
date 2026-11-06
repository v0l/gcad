use anyhow::{Result, bail};

#[derive(Clone, Debug, PartialEq)]
pub enum Falloff {
    Smooth,
    Linear,
    Sharp,
    Flat,
    Curve(Vec<f64>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub falloff: Falloff,
    pub focal: f64,
    pub edge: f64,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            falloff: Falloff::Smooth,
            focal: 0.0,
            edge: 1.0,
        }
    }
}

pub fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Falloff {
    pub fn parse(text: &str, number: impl Fn(&str) -> Result<f64>) -> Result<Falloff> {
        Ok(match text {
            "smooth" => Falloff::Smooth,
            "linear" => Falloff::Linear,
            "sharp" => Falloff::Sharp,
            "flat" => Falloff::Flat,
            other => {
                let Some(inner) = other
                    .strip_prefix("curve(")
                    .and_then(|r| r.strip_suffix(')'))
                else {
                    bail!(
                        "falloff is smooth, linear, sharp, flat or curve(edge,...,centre), got `{other}`"
                    );
                };
                let values: Vec<f64> = inner
                    .split(',')
                    .map(|v| number(v.trim()))
                    .collect::<Result<_>>()?;
                if values.len() < 2 {
                    bail!("a falloff curve needs at least two values, from the edge to the centre");
                }
                if values.iter().any(|v| !(0.0..=1.0).contains(v)) {
                    bail!("falloff curve values go from 0 to 1, got {inner}");
                }
                Falloff::Curve(values)
            }
        })
    }

    pub fn at(&self, u: f64) -> f64 {
        let u = u.clamp(0.0, 1.0);
        match self {
            Falloff::Smooth => 1.0 - smoothstep(u),
            Falloff::Linear => 1.0 - u,
            Falloff::Sharp => (1.0 - u) * (1.0 - u),
            Falloff::Flat => {
                if u < 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Falloff::Curve(values) => {
                let x = (1.0 - u) * (values.len() - 1) as f64;
                let k = (x.floor() as usize).min(values.len() - 2);
                let f = x - k as f64;
                values[k] + (values[k + 1] - values[k]) * f
            }
        }
    }
}

impl Profile {
    pub fn weight(&self, t: f64) -> f64 {
        if !(0.0..1.0).contains(&t) {
            return if t < 0.0 { 1.0 } else { 0.0 };
        }
        let core = 1.0 - self.edge.clamp(0.0, 1.0);
        if t <= core {
            return 1.0;
        }
        let u = (t - core) / (1.0 - core);
        let u = u.powf(2.0f64.powf(-self.focal.clamp(-1.0, 1.0)));
        self.falloff.at(u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1.0e-12, "{a} vs {b}");
    }

    #[test]
    fn named_falloffs_at_the_middle() {
        let at = |f: Falloff| {
            Profile {
                falloff: f,
                ..Profile::default()
            }
            .weight(0.5)
        };
        close(at(Falloff::Smooth), 0.5);
        close(at(Falloff::Linear), 0.5);
        close(at(Falloff::Sharp), 0.25);
        close(at(Falloff::Flat), 1.0);
    }

    #[test]
    fn curve_runs_from_the_edge_to_the_centre() {
        let p = Profile {
            falloff: Falloff::Curve(vec![0.0, 0.2, 1.0]),
            ..Profile::default()
        };
        close(p.weight(0.0), 1.0);
        close(p.weight(0.5), 0.2);
        close(p.weight(0.75), 0.1);
        close(p.weight(1.0), 0.0);
    }

    #[test]
    fn edge_keeps_a_hard_core() {
        let p = Profile {
            falloff: Falloff::Linear,
            edge: 0.25,
            ..Profile::default()
        };
        close(p.weight(0.7), 1.0);
        close(p.weight(0.875), 0.5);
    }

    #[test]
    fn focal_shift_hardens_or_points_the_falloff() {
        let p = |focal| Profile {
            falloff: Falloff::Linear,
            focal,
            ..Profile::default()
        };
        close(p(-1.0).weight(0.5), 0.75);
        close(p(1.0).weight(0.25), 0.5);
        close(p(0.0).weight(0.5), 0.5);
    }

    #[test]
    fn curve_parses_and_checks_its_values() {
        let n = |t: &str| t.parse::<f64>().map_err(anyhow::Error::from);
        assert_eq!(
            Falloff::parse("curve(0,0.2,1)", n).unwrap(),
            Falloff::Curve(vec![0.0, 0.2, 1.0])
        );
        assert!(Falloff::parse("curve(2,1)", n).is_err());
        assert!(Falloff::parse("soft", n).is_err());
    }
}
