use super::{BLACK, Mark};

pub struct Pattern {
    pub name: &'static str,
    pub lines: &'static [(f64, f64, bool)],
}

pub const PATTERNS: &[Pattern] = &[
    Pattern {
        name: "brick",
        lines: &[(45.0, 2.0, false)],
    },
    Pattern {
        name: "block",
        lines: &[(45.0, 2.4, false), (135.0, 2.4, false)],
    },
    Pattern {
        name: "concrete",
        lines: &[(45.0, 3.0, false), (45.0, 1.0, true)],
    },
    Pattern {
        name: "insulation",
        lines: &[(90.0, 1.4, true)],
    },
    Pattern {
        name: "timber",
        lines: &[(0.0, 1.6, false)],
    },
    Pattern {
        name: "earth",
        lines: &[(30.0, 3.0, false), (150.0, 3.0, true)],
    },
    Pattern {
        name: "glass",
        lines: &[(45.0, 5.0, false)],
    },
    Pattern {
        name: "metal",
        lines: &[(45.0, 1.2, false)],
    },
    Pattern {
        name: "none",
        lines: &[],
    },
];

pub fn pattern(name: &str) -> Option<&'static Pattern> {
    PATTERNS.iter().find(|p| p.name == name)
}

pub fn default_pattern(material: &str) -> &'static str {
    match material {
        "brick" => "brick",
        "block" => "block",
        "concrete" | "screed" | "stone" => "concrete",
        "pir" | "wool" | "eps" | "insulation" | "mineral_wool" => "insulation",
        "wood" | "timber" | "oak" | "pine" | "osb" | "plywood" => "timber",
        "earth" | "soil" => "earth",
        "glass" => "glass",
        "steel" | "stainless" | "aluminium" | "aluminum" | "copper" | "brass" => "metal",
        _ => "none",
    }
}

pub fn hatch_marks(rings: &[Vec<(f64, f64)>], pattern: &Pattern, stretch: f64) -> Vec<Mark> {
    let mut marks = Vec::new();
    for &(angle, base, dashed) in pattern.lines {
        let spacing = base * stretch;
        let a = angle.to_radians();
        let direction = (a.cos(), -a.sin());
        let normal = (-direction.1, direction.0);
        let mut low = f64::MAX;
        let mut high = f64::MIN;
        for p in rings.iter().flatten() {
            let c = p.0 * normal.0 + p.1 * normal.1;
            low = low.min(c);
            high = high.max(c);
        }
        if low > high {
            continue;
        }
        let first = (low / spacing).ceil() as i64;
        let last = (high / spacing).floor() as i64;
        let mut strokes: Vec<Vec<(f64, f64)>> = Vec::new();
        for k in first..=last {
            let c = k as f64 * spacing + spacing * 1.0e-6;
            let mut hits: Vec<f64> = Vec::new();
            for ring in rings {
                for i in 0..ring.len() {
                    let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
                    let dp = p.0 * normal.0 + p.1 * normal.1 - c;
                    let dq = q.0 * normal.0 + q.1 * normal.1 - c;
                    if (dp <= 0.0) != (dq <= 0.0) {
                        let t = dp / (dp - dq);
                        let x = p.0 + (q.0 - p.0) * t;
                        let y = p.1 + (q.1 - p.1) * t;
                        hits.push(x * direction.0 + y * direction.1);
                    }
                }
            }
            hits.sort_by(f64::total_cmp);
            for pair in hits.chunks_exact(2) {
                let at = |w: f64| {
                    (
                        direction.0 * w + normal.0 * c,
                        direction.1 * w + normal.1 * c,
                    )
                };
                strokes.push(vec![at(pair[0]), at(pair[1])]);
            }
        }
        if !strokes.is_empty() {
            marks.push(Mark::Shape {
                loops: strokes,
                closed: false,
                fill: None,
                stroke: Some(BLACK),
                width: 0.09,
                dashed,
            });
        }
    }
    marks
}
