#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Units {
    #[default]
    Mm,
    Cm,
    M,
    In,
}

impl Units {
    pub const ALL: [Units; 4] = [Units::Mm, Units::Cm, Units::M, Units::In];

    pub fn name(self) -> &'static str {
        match self {
            Units::Mm => "mm",
            Units::Cm => "cm",
            Units::M => "m",
            Units::In => "in",
        }
    }

    pub fn parse(text: &str) -> Option<Units> {
        Units::ALL.into_iter().find(|u| u.name() == text.trim())
    }

    pub fn per_mm(self) -> f64 {
        match self {
            Units::Mm => 1.0,
            Units::Cm => 0.1,
            Units::M => 0.001,
            Units::In => 1.0 / 25.4,
        }
    }

    fn shift(self) -> usize {
        match self {
            Units::Mm => 0,
            Units::Cm => 1,
            Units::M => 3,
            Units::In => 2,
        }
    }

    pub fn number(self, mm: f64, decimals: usize) -> String {
        fixed(mm * self.per_mm(), decimals + self.shift())
    }

    pub fn length(self, mm: f64, decimals: usize) -> String {
        format!("{} {}", self.number(mm, decimals), self.name())
    }

    pub fn lengths(self, mm: &[f64], decimals: usize, between: &str) -> String {
        let numbers: Vec<String> = mm.iter().map(|v| self.number(*v, decimals)).collect();
        format!("{} {}", numbers.join(between), self.name())
    }

    pub fn area(self, mm2: f64, decimals: usize) -> String {
        let value = fixed(mm2 * self.per_mm().powi(2), decimals + 2 * self.shift());
        format!("{value} {}²", self.name())
    }

    pub fn volume(self, mm3: f64, decimals: usize) -> String {
        let value = fixed(mm3 * self.per_mm().powi(3), decimals + 3 * self.shift());
        format!("{value} {}³", self.name())
    }

    pub fn mass(self, grams: f64) -> String {
        match self {
            Units::Mm | Units::Cm => format!("{} g", fixed(grams, 1)),
            Units::M => format!("{} kg", fixed(grams / 1000.0, 4)),
            Units::In => format!("{} lb", fixed(grams / 453.592_37, 4)),
        }
    }

    pub fn summary(self, text: &str) -> String {
        let text = rewrite(text, "volume ", float, |v| {
            format!("volume {}", self.volume(v, 3))
        });
        rewrite(&text, "bbox ", bbox, |[x0, y0, z0, x1, y1, z1]| {
            let join = |v: [f64; 3]| v.map(|x| self.number(x, 3)).join(", ");
            format!(
                "bbox [{}]..[{}] {}",
                join([x0, y0, z0]),
                join([x1, y1, z1]),
                self.name()
            )
        })
    }
}

fn fixed(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    match text.strip_prefix('-') {
        Some(bare) if bare.chars().all(|c| c == '0' || c == '.') => bare.to_string(),
        _ => text,
    }
}

fn rewrite<T>(
    text: &str,
    key: &str,
    read: fn(&str) -> Option<(T, usize)>,
    write: impl Fn(T) -> String,
) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(key) {
        out.push_str(&rest[..at]);
        let after = &rest[at + key.len()..];
        match read(after) {
            Some((value, used)) => {
                out.push_str(&write(value));
                rest = &after[used..];
            }
            None => {
                out.push_str(key);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn float(text: &str) -> Option<(f64, usize)> {
    let end = text
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || c == '-' && i == 0))
        .map_or(text.len(), |(i, _)| i);
    let end = if text[..end].ends_with('.') {
        end - 1
    } else {
        end
    };
    Some((text[..end].parse().ok()?, end))
}

fn bbox(text: &str) -> Option<([f64; 6], usize)> {
    let mut values = [0.0; 6];
    let mut at = 0;
    for (i, value) in values.iter_mut().enumerate() {
        let before = match i {
            0 => "[",
            3 => "]..[",
            _ => ", ",
        };
        text[at..].strip_prefix(before)?;
        at += before.len();
        let (read, used) = float(&text[at..])?;
        *value = read;
        at += used;
    }
    text[at..].strip_prefix(']')?;
    Some((values, at + 1))
}

#[cfg(test)]
mod tests {
    use super::Units;

    #[test]
    fn lengths_areas_and_volumes_carry_their_unit() {
        assert_eq!(Units::Mm.length(12.5, 3), "12.500 mm");
        assert_eq!(Units::Cm.length(12.5, 3), "1.2500 cm");
        assert_eq!(Units::M.length(12.5, 1), "0.0125 m");
        assert_eq!(Units::In.length(25.4, 1), "1.000 in");
        assert_eq!(Units::Cm.area(250.0, 1), "2.500 cm²");
        assert_eq!(Units::Cm.volume(13800.0, 1), "13.8000 cm³");
        assert_eq!(Units::Mm.length(-0.0001, 3), "0.000 mm");
        assert_eq!(Units::Mm.mass(12.34), "12.3 g");
        assert_eq!(Units::M.mass(1500.0), "1.5000 kg");
    }

    #[test]
    fn a_line_summary_is_converted() {
        let text = "8 edge(s); 45 faces, volume 13800.099, bbox [-30.000, -20.000, 0.000]..[30.000, 20.000, 16.000] (2 other bodies)";
        assert_eq!(
            Units::Cm.summary(text),
            "8 edge(s); 45 faces, volume 13.800099 cm³, bbox [-3.0000, -2.0000, 0.0000]..[3.0000, 2.0000, 1.6000] cm (2 other bodies)"
        );
        assert_eq!(Units::Mm.summary("no solid here"), "no solid here");
    }

    #[test]
    fn the_summary_format_is_the_one_the_model_writes() {
        let lines = crate::parse::parse_program("plane XY\nrect 20 10\nextrude 5\n").expect("part");
        let snapshots = crate::model::run_snapshots(&lines);
        let text = snapshots
            .last()
            .and_then(|s| s.result.clone().ok())
            .expect("summary");
        let converted = Units::Mm.summary(&text);
        assert!(converted.contains("volume 1000.000 mm³"), "{converted}");
        assert!(
            converted.contains("..[10.000, 5.000, 5.000] mm"),
            "{converted}"
        );
    }
}
