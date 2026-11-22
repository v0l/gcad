use super::Model;
use crate::calc::bearing::{self, Kind};
use crate::geometry;
use monstertruck::modeling::*;

const CONTACT: f64 = 0.15;
const END_REACH: f64 = 0.5;

struct Seat {
    name: String,
    origin: Point3,
    axis: Vector3,
    found: &'static bearing::Bearing,
}

fn polar(seat: &Seat, p: Point3) -> (f64, f64) {
    let d = p - seat.origin;
    let t = d.dot(seat.axis);
    (t, (d - seat.axis * t).magnitude())
}

impl Model {
    fn bearing_seats(&self) -> Vec<Seat> {
        self.body_names()
            .into_iter()
            .filter_map(|name| {
                let title = self.sources.get(&name)?.file.to_str()?.to_string();
                let mut words = title.split_whitespace();
                let spec = words.next()?;
                if words.next()? != "bearing" {
                    return None;
                }
                let found = bearing::find(spec).ok()?;
                if !matches!(found.kind, Kind::Radial | Kind::Flanged) {
                    return None;
                }
                let placed = self
                    .placements
                    .get(&name)
                    .copied()
                    .unwrap_or_else(Matrix4::identity);
                Some(Seat {
                    name,
                    origin: placed.transform_point(Point3::new(0.0, 0.0, 0.0)),
                    axis: placed.transform_vector(Vector3::unit_z()).normalize(),
                    found,
                })
            })
            .collect()
    }

    pub fn seat_warnings(&self) -> Vec<String> {
        let mut out = self.engagement_warnings();
        out.extend(self.shoulder_warnings());
        out
    }

    fn engagement_warnings(&self) -> Vec<String> {
        self.thread_notes
            .iter()
            .filter_map(|(body, thread, length)| {
                let material = self.materials.get(body)?;
                crate::calc::thread::engagement_note(thread, *length, &material.name)
            })
            .collect()
    }

    fn shoulder_warnings(&self) -> Vec<String> {
        let seats = self.bearing_seats();
        if seats.is_empty() {
            return Vec::new();
        }
        let others: Vec<(String, &Solid)> = self
            .named_solids()
            .into_iter()
            .filter(|(name, _)| !seats.iter().any(|s| s.name == *name))
            .collect();
        let solids: Vec<&Solid> = others.iter().map(|(_, s)| *s).collect();
        let meshes = geometry::meshed_all(&solids);
        let mut out = Vec::new();
        for seat in &seats {
            let b = seat.found;
            let ends = [0.0, b.width];
            let shoulders = bearing::shoulders(b);
            for ((name, _), mesh) in others.iter().zip(&meshes) {
                let polars: Vec<[(f64, f64); 3]> = mesh
                    .triangles()
                    .iter()
                    .map(|t| t.map(|p| polar(seat, p)))
                    .collect();
                let touches = |radius: f64| {
                    polars.iter().any(|t| {
                        t.iter().all(|(_, r)| (r - radius).abs() < CONTACT)
                            && t.iter().any(|(a, _)| *a > 0.0 && *a < b.width)
                    })
                };
                let on_shaft = touches(b.bore / 2.0);
                let in_housing = !on_shaft && touches(b.outer / 2.0);
                if !on_shaft && !in_housing {
                    continue;
                }
                let seat_radius = if on_shaft { b.bore } else { b.outer } / 2.0;
                let reach = polars
                    .iter()
                    .filter(|t| {
                        let flat = t.iter().all(|(a, _)| (a - t[0].0).abs() < 0.01);
                        flat && ends.iter().any(|e| (t[0].0 - e).abs() <= END_REACH)
                    })
                    .flat_map(|t| t.iter().map(|(_, r)| *r))
                    .filter(|r| {
                        if on_shaft {
                            *r > seat_radius + 0.1
                        } else {
                            *r < seat_radius - 0.1
                        }
                    })
                    .fold(None, |best: Option<f64>, r| {
                        Some(match best {
                            Some(x) if on_shaft => x.max(r),
                            Some(x) => x.min(r),
                            None => r,
                        })
                    });
                let (needs, side) = if on_shaft {
                    (shoulders.shaft_min, "shaft shoulder at least")
                } else {
                    (shoulders.housing_max, "housing shoulder at most")
                };
                let tall_enough = reach.is_some_and(|r| {
                    if on_shaft {
                        r * 2.0 + 0.1 >= needs
                    } else {
                        r * 2.0 - 0.1 <= needs
                    }
                });
                if reach.is_none() {
                    out.push(format!(
                        "warning: bearing {} sits {} `{name}` with no shoulder within {END_REACH} mm of either face; the catalogue needs a {side} {needs:.1} mm across (gcad calc bearing {})",
                        seat.name,
                        if on_shaft { "on" } else { "in" },
                        b.name
                    ));
                } else if !tall_enough {
                    out.push(format!(
                        "warning: the shoulder for bearing {} on `{name}` is {:.1} mm across; the catalogue needs a {side} {needs:.1} mm across (gcad calc bearing {})",
                        seat.name,
                        reach.unwrap_or_default() * 2.0,
                        b.name
                    ));
                }
            }
        }
        out
    }
}
