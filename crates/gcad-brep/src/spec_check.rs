const SCREW_KINDS: [&str; 8] = [
    "iso4762", "iso4017", "iso4014", "iso7380", "iso10642", "shcs", "fhcs", "hex",
];

const ISO_888_LENGTHS: [f64; 30] = [
    2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0,
    55.0, 60.0, 65.0, 70.0, 80.0, 90.0, 100.0, 110.0, 120.0, 130.0, 140.0, 150.0, 160.0, 180.0,
];

pub fn spec_warnings(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some((kind, size)) = spec.split_once(':') else {
        return out;
    };
    if SCREW_KINDS.contains(&kind)
        && let Some((thread, length)) = size.rsplit_once(['x', 'X'])
        && let Ok(length) = length.parse::<f64>()
        && !ISO_888_LENGTHS.iter().any(|l| (l - length).abs() < 1.0e-9)
    {
        let nearest = ISO_888_LENGTHS
            .iter()
            .copied()
            .min_by(|a, b| (a - length).abs().total_cmp(&(b - length).abs()))
            .unwrap_or(length);
        out.push(format!(
            "warning: {length} mm is not a standard length for {thread} ({kind}); the nearest ISO 888 length is {nearest} mm"
        ));
    }
    out
}

const SCREW_TITLES: [&str; 5] = ["ISO 4762", "ISO 4017", "ISO 4014", "ISO 7380", "ISO 10642"];

pub fn narrow_hole(title: &str, shank_radius: f64, hole_radius: f64) -> Option<String> {
    if !SCREW_TITLES.iter().any(|t| title.starts_with(t)) {
        return None;
    }
    let size = title.split_whitespace().nth(2)?.split(['x', 'X']).next()?;
    let metric = super::fastener::named(size).ok()?;
    if (shank_radius * 2.0 - metric.major).abs() > 1.0e-6 {
        return None;
    }
    let hole = hole_radius * 2.0;
    (hole + 1.0e-6 < metric.tap).then(|| {
        format!(
            "warning: the {hole:.2} mm hole is narrower than the {:.2} mm tap drill of {}, so the screw cannot enter it",
            metric.tap, metric.name
        )
    })
}
