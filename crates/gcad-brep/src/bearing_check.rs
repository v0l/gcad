use crate::calc::bearing::{self, Load};

pub fn bearing_seat(title: &str, own_radius: f64, other_radius: f64) -> Option<String> {
    let mut words = title.split_whitespace();
    let name = words.next()?;
    if words.next()? != "bearing" {
        return None;
    }
    let found = bearing::find(name).ok()?;
    let own = own_radius * 2.0;
    let other = other_radius * 2.0;
    let (what, classes, nominal) = if (own - found.bore).abs() < 0.3 {
        let classes: Vec<&str> = [Load::Light, Load::Normal, Load::Heavy]
            .into_iter()
            .map(|load| bearing::fit_classes(found, load).0)
            .collect();
        ("shaft", classes, found.bore)
    } else if (own - found.outer).abs() < 0.3 {
        let housing = bearing::fit_classes(found, Load::Normal).1;
        ("housing bore", vec![housing], found.outer)
    } else {
        return None;
    };
    let limits: Vec<_> = classes
        .iter()
        .filter_map(|class| crate::calc::fit::limits(class, nominal).ok())
        .collect();
    let low = limits.iter().map(|l| l.min).fold(f64::INFINITY, f64::min);
    let high = limits
        .iter()
        .map(|l| l.max)
        .fold(f64::NEG_INFINITY, f64::max);
    let slack = (high - low).max(0.03);
    (other < low - slack || other > high + slack).then(|| {
        format!(
            "warning: the {other:.3} mm {what} for {} should be {} ({low:.3} to {high:.3} mm); run gcad calc bearing {}",
            found.name,
            classes.join(" to "),
            found.name
        )
    })
}
