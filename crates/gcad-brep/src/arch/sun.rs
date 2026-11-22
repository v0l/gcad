use super::Sun;
use crate::Model;
use crate::parse::{Line, eval};
use anyhow::{Context, Result, anyhow, bail};

const CUMULATIVE: [f64; 12] = [0.0, 31.0, 59.0, 90.0, 120.0, 151.0, 181.0, 212.0, 243.0, 273.0, 304.0, 334.0];

fn day_of_year(text: &str) -> Result<f64> {
    let (d, m) = text
        .split_once('-')
        .ok_or_else(|| anyhow!("write the date as day-month, like 21-06"))?;
    let (d, m): (f64, usize) = (d.parse()?, m.parse()?);
    if !(1..=12).contains(&m) || !(1.0..=31.0).contains(&d) {
        bail!("`{text}` is not a date like 21-06");
    }
    Ok(CUMULATIVE[m - 1] + d)
}

fn hours(text: &str) -> Result<f64> {
    let (h, m) = text
        .split_once(':')
        .ok_or_else(|| anyhow!("write the time as 15:00"))?;
    Ok(h.parse::<f64>()? + m.parse::<f64>()? / 60.0)
}

pub(crate) fn position(lat: f64, day: f64, time: f64) -> (f64, f64) {
    let decl = 23.44_f64.to_radians() * (std::f64::consts::TAU * (day - 81.0) / 365.0).sin();
    let hour_angle = (15.0 * (time - 12.0)).to_radians();
    let phi = lat.to_radians();
    let sin_alt = phi.sin() * decl.sin() + phi.cos() * decl.cos() * hour_angle.cos();
    let alt = sin_alt.asin();
    let y = -hour_angle.sin() * decl.cos();
    let x = decl.sin() * phi.cos() - hour_angle.cos() * decl.cos() * phi.sin();
    let mut az = y.atan2(x).to_degrees();
    az = az.rem_euclid(360.0);
    (alt.to_degrees(), az)
}

impl Model {
    pub(crate) fn op_sun(&mut self, line: &Line) -> Result<String> {
        let (mut lat, mut day, mut time) = (None, None, 12.0);
        for (key, text) in &line.named {
            match key.as_str() {
                "lat" => lat = Some(eval(text, &self.scope).with_context(|| format!("`lat={text}`"))?),
                "date" => day = Some(day_of_year(text)?),
                "time" => time = hours(text)?,
                other => bail!("`sun` takes lat=, date= and time=, not `{other}=`"),
            }
        }
        let lat = lat.ok_or_else(|| anyhow!("`sun` needs `lat=`"))?;
        let day = day.ok_or_else(|| anyhow!("`sun` needs `date=`, like 21-06"))?;
        if !(-90.0..=90.0).contains(&lat) {
            bail!("lat= is a latitude from -90 to 90");
        }
        let (altitude, azimuth) = position(lat, day, time);
        self.arch.sun = Some(Sun { altitude, azimuth });
        Ok(format!("sun altitude {altitude:.2} azimuth {azimuth:.2}"))
    }

    pub(crate) fn check_shadow(&mut self, line: &Line) -> Result<String> {
        let name = line
            .positional
            .get(1)
            .ok_or_else(|| anyhow!("write `check shadow w1`"))?;
        let wall = self
            .arch
            .wall(name)
            .cloned()
            .ok_or_else(|| anyhow!("no wall `{name}`"))?;
        let sun = self.arch.sun.ok_or_else(|| anyhow!("`check shadow` needs a `sun` line first"))?;
        let height = wall.top - wall.base;
        let text = if sun.altitude <= 0.0 {
            format!("{name} shadow unbounded, the sun is down")
        } else {
            let length = height / sun.altitude.to_radians().tan();
            format!("{name} shadow {length:.0} mm at azimuth {:.1}", (sun.azimuth + 180.0).rem_euclid(360.0))
        };
        self.finish_check(line, Vec::new(), text)
    }
}
