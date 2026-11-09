use super::P2;

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

pub fn flight_profile(steps: &Steps, treads: usize, nosing: f64, waist: f64) -> Vec<P2> {
    let (r, t) = (steps.rise, steps.tread);
    let lip = (r / 4.0).min(40.0);
    let mut ring = vec![(0.0, 0.0)];
    for k in 0..treads {
        let (s, z) = (k as f64 * t, k as f64 * r);
        if nosing > 0.0 {
            ring.push((s, z + r - lip));
            ring.push((s - nosing, z + r - lip));
            ring.push((s - nosing, z + r));
        } else {
            ring.push((s, z + r));
        }
        ring.push((s + t, z + r));
    }
    let going = treads as f64 * t;
    let depth = waist / steps.pitch().to_radians().cos();
    ring.push((going, going * r / t - depth));
    ring.push(((depth * t / r).min(going), 0.0));
    ring
}

fn clip(ring: Vec<P2>, keep: impl Fn(P2) -> f64) -> Vec<P2> {
    let n = ring.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let (fa, fb) = (keep(a), keep(b));
        if fa >= 0.0 {
            out.push(a);
        }
        if (fa >= 0.0) != (fb >= 0.0) {
            let k = fa / (fa - fb);
            out.push((a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k));
        }
    }
    out
}

pub fn stringer_profile(steps: &Steps, treads: usize, above: f64, depth: f64, top: f64) -> Vec<P2> {
    let (r, t) = (steps.rise, steps.tread);
    let going = treads as f64 * t;
    let line = |s: f64| r + s * r / t;
    let ring = vec![
        (-t, line(-t) - depth),
        (going, line(going) - depth),
        (going, line(going) + above),
        (-t, line(-t) + above),
    ];
    let ring = clip(ring, |p| p.1);
    let ring = clip(ring, |p| p.0);
    clip(ring, |p| top - p.1)
}

pub fn hull(mut points: Vec<P2>) -> Vec<P2> {
    points.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    points.dedup_by(|a, b| (a.0 - b.0).abs() < 1.0e-9 && (a.1 - b.1).abs() < 1.0e-9);
    if points.len() < 3 {
        return points;
    }
    let turn = |o: P2, a: P2, b: P2| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut lower: Vec<P2> = Vec::new();
    for &p in &points {
        while lower.len() >= 2 && turn(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 1.0e-9 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<P2> = Vec::new();
    for &p in points.iter().rev() {
        while upper.len() >= 2 && turn(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 1.0e-9 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

pub fn split_treads(total: usize, turns: usize, steps_in_turns: usize) -> Result<Vec<usize>, String> {
    let left = total
        .checked_sub(turns * steps_in_turns)
        .filter(|n| *n >= turns + 1)
        .ok_or_else(|| format!("{total} treads are too few for {turns} turns"))?;
    let flights = turns + 1;
    Ok((0..flights)
        .map(|k| left / flights + usize::from(k < left % flights))
        .collect())
}

pub fn parse_ratio(text: &str) -> Result<f64, String> {
    let bad = || format!("a slope is written 1:12, not `{text}`");
    let (a, b) = text.split_once(':').ok_or_else(bad)?;
    let (a, b): (f64, f64) = (a.parse().map_err(|_| bad())?, b.parse().map_err(|_| bad())?);
    if a <= 0.0 || b <= 0.0 {
        return Err(bad());
    }
    Ok(a / b)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RampPart {
    pub landing: bool,
    pub s0: f64,
    pub s1: f64,
    pub z0: f64,
    pub z1: f64,
}

pub fn ramp_parts(
    length: f64,
    rise: f64,
    max_slope: f64,
    landing: f64,
    every: f64,
) -> Result<Vec<RampPart>, String> {
    if length <= 0.0 {
        return Err("a ramp needs two different ends".into());
    }
    let mut runs = 1usize;
    let sloped = loop {
        let sloped = length - (runs - 1) as f64 * landing;
        if sloped <= 0.0 {
            return Err(format!(
                "{runs} runs with landings of {landing:.0} do not fit in {length:.0}"
            ));
        }
        if every <= 0.0 || sloped <= every * runs as f64 + 1.0e-9 {
            break sloped;
        }
        runs += 1;
    };
    let slope = rise.abs() / sloped;
    if slope > max_slope + 1.0e-12 {
        return Err(format!(
            "too steep: {:.0} up over {:.0} of slope is 1:{:.1}, steeper than 1:{:.1}",
            rise.abs(),
            sloped,
            1.0 / slope,
            1.0 / max_slope
        ));
    }
    let run = sloped / runs as f64;
    let mut parts = Vec::new();
    let (mut s, mut z) = (0.0, 0.0);
    for k in 0..runs {
        if k > 0 {
            parts.push(RampPart {
                landing: true,
                s0: s,
                s1: s + landing,
                z0: z,
                z1: z,
            });
            s += landing;
        }
        let z1 = z + rise / runs as f64;
        parts.push(RampPart {
            landing: false,
            s0: s,
            s1: s + run,
            z0: z,
            z1,
        });
        (s, z) = (s + run, z1);
    }
    Ok(parts)
}

pub fn rung_count(length: f64, spacing: f64) -> usize {
    if spacing <= 0.0 {
        return 0;
    }
    ((length / spacing - 1.0e-9).ceil() as usize).saturating_sub(1)
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

    fn area(ring: &[P2]) -> f64 {
        super::super::layout::signed_area(ring)
    }

    #[test]
    fn a_flight_is_its_steps_over_a_waist() {
        let s = steps(2700.0, &StairRule::default(), None, None).unwrap();
        let (n, r, t) = (14.0, 180.0, 270.0);
        let ring = flight_profile(&s, 14, 0.0, 150.0);
        let depth = 150.0 / s.pitch().to_radians().cos();
        let foot = depth * t / r;
        let expected = t * r * n * (n + 1.0) / 2.0 - r / (2.0 * t) * (n * t - foot).powi(2);
        assert!((area(&ring) + expected).abs() < 1.0e-6, "{}", area(&ring));
        let top = ring.iter().map(|p| p.1).fold(0.0, f64::max);
        assert!((top - n * r).abs() < 1.0e-9);
        let nosed = flight_profile(&s, 14, 20.0, 150.0);
        assert!((area(&nosed) - area(&ring) + n * 20.0 * 40.0).abs() < 1.0e-6);
    }

    #[test]
    fn a_stringer_stops_at_the_floor_above() {
        let s = steps(2700.0, &StairRule::default(), None, None).unwrap();
        let ring = stringer_profile(&s, 14, 50.0, 300.0, 2700.0);
        let top = ring.iter().map(|p| p.1).fold(0.0, f64::max);
        assert!((top - 2700.0).abs() < 1.0e-9);
        assert!(ring.iter().all(|p| p.0 >= 0.0 && p.0 <= 14.0 * 270.0 + 1.0e-9));
    }

    #[test]
    fn the_hull_drops_inner_and_collinear_points() {
        let h = hull(vec![
            (0.0, 0.0),
            (2.0, 0.0),
            (1.0, 0.0),
            (2.0, 2.0),
            (1.0, 1.0),
            (0.0, 2.0),
        ]);
        assert_eq!(h, vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
    }

    #[test]
    fn treads_split_between_flights() {
        assert_eq!(split_treads(16, 1, 1), Ok(vec![8, 7]));
        assert_eq!(split_treads(16, 1, 3), Ok(vec![7, 6]));
        assert!(split_treads(2, 1, 1).is_err());
    }

    #[test]
    fn a_long_ramp_gets_a_landing() {
        let parts = ramp_parts(13500.0, 600.0, 1.0 / 20.0, 1500.0, 10000.0).unwrap();
        assert_eq!(parts.iter().filter(|p| p.landing).count(), 1);
        assert_eq!(parts.len(), 3);
        assert!((parts[0].s1 - 6000.0).abs() < 1.0e-9);
        assert!((parts[2].z1 - 600.0).abs() < 1.0e-9);
        assert!((parts[2].s1 - 13500.0).abs() < 1.0e-9);
        let short = ramp_parts(6000.0, 600.0, 1.0 / 20.0, 1500.0, 10000.0).unwrap_err();
        assert!(short.contains("1:10"), "{short}");
        assert!(ramp_parts(12000.0, 600.0, 1.0 / 20.0, 1500.0, 10000.0).is_err());
        assert_eq!(parse_ratio("1:12"), Ok(1.0 / 12.0));
    }

    #[test]
    fn rungs_fill_between_the_ends() {
        assert_eq!(rung_count(3000.0, 300.0), 9);
        assert_eq!(rung_count(3100.0, 300.0), 10);
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
