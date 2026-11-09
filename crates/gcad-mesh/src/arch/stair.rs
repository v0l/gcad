#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StairRule {
    pub sum: f64,
    pub max_rise: f64,
    pub min_tread: f64,
}

impl Default for StairRule {
    fn default() -> Self {
        StairRule {
            sum: 630.0,
            max_rise: 180.0,
            min_tread: 220.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Steps {
    pub risers: usize,
    pub rise: f64,
    pub tread: f64,
}

impl Steps {
    pub fn treads(&self) -> usize {
        self.risers - 1
    }

    pub fn going(&self) -> f64 {
        self.treads() as f64 * self.tread
    }

    pub fn height(&self) -> f64 {
        self.risers as f64 * self.rise
    }

    pub fn pitch(&self) -> f64 {
        self.rise.atan2(self.tread).to_degrees()
    }

    pub fn nosing_at(&self, s: f64) -> f64 {
        self.rise + s * self.rise / self.tread
    }

    pub fn low_headroom_from(&self, ceiling: f64, headroom: f64) -> Option<f64> {
        let s = (ceiling - headroom - self.rise) * self.tread / self.rise;
        (s < self.going()).then_some(s.max(0.0))
    }
}

pub fn parse_rule(text: &str) -> Result<f64, String> {
    let sum = text
        .strip_prefix("2R+T=")
        .and_then(|n| n.parse::<f64>().ok())
        .filter(|n| *n > 0.0)
        .ok_or_else(|| format!("a stair rule is written `2R+T=630`, not `{text}`"))?;
    Ok(sum)
}

pub fn steps(
    height: f64,
    rule: &StairRule,
    risers: Option<usize>,
    tread: Option<f64>,
) -> Result<Steps, String> {
    if height <= 0.0 {
        return Err("a stair must climb, its top is not above its foot".into());
    }
    if rule.max_rise <= 0.0 {
        return Err("max_rise must be above zero".into());
    }
    let risers = match risers {
        Some(n) if n < 2 => return Err("a stair needs at least 2 risers".into()),
        Some(n) => n,
        None => ((height / rule.max_rise - 1.0e-9).ceil() as usize).max(2),
    };
    let rise = height / risers as f64;
    let tread = tread.unwrap_or(rule.sum - 2.0 * rise);
    if tread <= 0.0 {
        return Err(format!(
            "too steep: {risers} risers of {rise:.1} leave no tread under 2R+T={}",
            rule.sum
        ));
    }
    Ok(Steps {
        risers,
        rise,
        tread,
    })
}

pub fn check(steps: &Steps, rule: &StairRule) -> Result<(), String> {
    if steps.rise > rule.max_rise + 1.0e-9 || steps.tread < rule.min_tread - 1.0e-9 {
        return Err(format!(
            "too steep: {} risers of {:.1} and treads of {:.1} under 2R+T={}, max_rise={} and min_tread={}",
            steps.risers, steps.rise, steps.tread, rule.sum, rule.max_rise, rule.min_tread
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_metres_at_180_is_17_risers() {
        let s = steps(3000.0, &StairRule::default(), None, None).unwrap();
        assert_eq!(s.risers, 17);
        assert_eq!(s.treads(), 16);
        assert!((s.rise - 3000.0 / 17.0).abs() < 1.0e-9);
        assert!((s.tread - (630.0 - 6000.0 / 17.0)).abs() < 1.0e-9);
        assert!((s.going() - 16.0 * s.tread).abs() < 1.0e-9);
        assert!((s.height() - 3000.0).abs() < 1.0e-9);
        assert!(check(&s, &StairRule::default()).is_ok());
    }

    #[test]
    fn an_exact_multiple_does_not_add_a_riser() {
        let s = steps(2700.0, &StairRule::default(), None, None).unwrap();
        assert_eq!(s.risers, 15);
        assert!((s.tread - 270.0).abs() < 1.0e-9);
    }

    #[test]
    fn ten_risers_in_three_metres_is_too_steep() {
        let rule = StairRule::default();
        let s = steps(3000.0, &rule, Some(10), None).unwrap();
        assert!(check(&s, &rule).unwrap_err().contains("2R+T"));
        assert!(steps(3000.0, &rule, Some(5), None).is_err());
    }

    #[test]
    fn the_rule_parses() {
        assert_eq!(parse_rule("2R+T=620"), Ok(620.0));
        assert!(parse_rule("R+T=460").is_err());
    }

    #[test]
    fn headroom_runs_out_past_the_nosing_line() {
        let s = steps(2700.0, &StairRule::default(), None, None).unwrap();
        let from = s.low_headroom_from(2500.0, 2000.0).unwrap();
        assert!((from - 480.0).abs() < 1.0e-9);
        assert!((s.nosing_at(from) + 2000.0 - 2500.0).abs() < 1.0e-9);
        assert_eq!(s.low_headroom_from(10000.0, 2000.0), None);
        assert_eq!(s.low_headroom_from(1000.0, 2000.0), Some(0.0));
    }
}
