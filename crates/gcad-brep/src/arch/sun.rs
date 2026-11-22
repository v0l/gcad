use super::Sun;
use super::check::named_value;
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

fn ray_hits(o: [f64; 3], d: [f64; 3], t: &[[f64; 3]; 3]) -> bool {
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let h = cross(d, e2);
    let a = dot(e1, h);
    if a.abs() < 1.0e-12 {
        return false;
    }
    let s = sub(o, t[0]);
    let u = dot(s, h) / a;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = cross(s, e1);
    let v = dot(d, q) / a;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    dot(e2, q) / a > 1.0e-3
}

impl Model {
    fn toward_sun(&self, altitude: f64, azimuth: f64) -> [f64; 3] {
        let north = self.arch.building.as_ref().map_or(0.0, |b| b.north).to_radians();
        let (n, e) = ((-north.sin(), north.cos()), (north.cos(), north.sin()));
        let (alt, az) = (altitude.to_radians(), azimuth.to_radians());
        let flat = alt.cos();
        [
            flat * (n.0 * az.cos() + e.0 * az.sin()),
            flat * (n.1 * az.cos() + e.1 * az.sin()),
            alt.sin(),
        ]
    }

    pub fn sun_direction(&self) -> Option<[f64; 3]> {
        let sun = self.arch.sun?;
        Some(self.toward_sun(sun.altitude, sun.azimuth))
    }

    pub(crate) fn check_sunhours(&mut self, line: &Line) -> Result<String> {
        let at = line
            .positional
            .get(1)
            .ok_or_else(|| anyhow!("write `check sunhours x,y,z`"))?;
        let p = crate::args::point3(at, &self.scope)?;
        let sun = self.arch.sun.ok_or_else(|| anyhow!("`check sunhours` needs a `sun` line first"))?;
        let mut triangles: Vec<[[f64; 3]; 3]> = Vec::new();
        for (_, solid) in self.named_solids() {
            let mesh = crate::geometry::mesh(solid, crate::geometry::mesh_tolerance(solid));
            let positions = mesh.positions();
            for t in mesh.faces().triangle_iter() {
                triangles.push(t.map(|v| {
                    let q = positions[v.pos];
                    [q.x, q.y, q.z]
                }));
            }
        }
        let step = 0.1;
        let mut hours = 0.0;
        let mut time = 0.0;
        while time < 24.0 {
            let (alt, az) = position(sun.lat, sun.day, time + step / 2.0);
            if alt > 0.0 {
                let d = self.toward_sun(alt, az);
                let origin = [p.x, p.y, p.z];
                if !triangles.iter().any(|t| ray_hits(origin, d, t)) {
                    hours += step;
                }
            }
            time += step;
        }
        let min = named_value(line, "min").map(|m| eval(&m, &self.scope)).transpose()?;
        let problems = min
            .filter(|m| hours < *m)
            .map(|m| vec![format!("sunhours {hours:.1} under {m}")])
            .unwrap_or_default();
        self.finish_check(line, problems, format!("sunhours {hours:.1} h at {at}"))
    }

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
        self.arch.sun = Some(Sun { altitude, azimuth, lat, day });
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
