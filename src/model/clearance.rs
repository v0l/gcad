use super::Model;
use crate::geometry::{self, Meshed};
use crate::select;
use anyhow::{Result, bail};
use monstertruck::modeling::*;

type V = [f64; 3];

#[derive(Clone, Copy)]
struct Span {
    low: V,
    high: V,
}

impl Span {
    fn around(points: impl IntoIterator<Item = Point3>) -> Option<Span> {
        let mut points = points.into_iter().map(|p| [p.x, p.y, p.z]);
        let first = points.next()?;
        Some(points.fold(
            Span {
                low: first,
                high: first,
            },
            |s, p| Span {
                low: std::array::from_fn(|k| s.low[k].min(p[k])),
                high: std::array::from_fn(|k| s.high[k].max(p[k])),
            },
        ))
    }

    fn of_box(b: &BoundingBox<Point3>) -> Span {
        let (low, high) = (b.min(), b.max());
        Span {
            low: [low.x, low.y, low.z],
            high: [high.x, high.y, high.z],
        }
    }

    fn join(self, other: Span) -> Span {
        Span {
            low: std::array::from_fn(|k| self.low[k].min(other.low[k])),
            high: std::array::from_fn(|k| self.high[k].max(other.high[k])),
        }
    }

    fn meets(&self, other: &Span) -> bool {
        (0..3).all(|k| self.low[k] <= other.high[k] && other.low[k] <= self.high[k])
    }

    fn describe(&self) -> String {
        let size: V = std::array::from_fn(|k| self.high[k] - self.low[k]);
        format!(
            "{} x {} x {}, [{}]..[{}]",
            fixed(size[0]),
            fixed(size[1]),
            fixed(size[2]),
            triple(self.low, ", "),
            triple(self.high, ", ")
        )
    }
}

fn fixed(v: f64) -> String {
    let text = format!("{v:.3}");
    match text.strip_prefix('-') {
        Some(bare) if bare.chars().all(|c| c == '0' || c == '.') => bare.to_string(),
        _ => text,
    }
}

fn triple(v: V, between: &str) -> String {
    v.map(fixed).join(between)
}

const SIDES: [&str; 6] = ["-x", "+x", "-y", "+y", "-z", "+z"];

impl Model {
    pub(crate) fn measure_box(&self, names: &[&str]) -> Result<String> {
        let bodies = self.named_solids();
        if names.is_empty() {
            let spans: Vec<(String, Span)> = bodies
                .iter()
                .map(|(name, solid)| (name.clone(), Span::of_box(&geometry::bounds(solid))))
                .collect();
            let mut text: Vec<String> = spans
                .iter()
                .map(|(name, span)| format!("{name} {}", span.describe()))
                .collect();
            if let Some(all) = spans.iter().map(|(_, s)| *s).reduce(Span::join)
                && spans.len() > 1
            {
                text.push(format!("all {}", all.describe()));
            }
            if text.is_empty() {
                bail!("there are no bodies to measure yet");
            }
            return Ok(text.join("; "));
        }
        let mut text = Vec::new();
        for name in names {
            let span = match bodies.iter().find(|(n, _)| n == name) {
                Some((_, solid)) => Span::of_box(&geometry::bounds(solid)),
                None => self.faces_span(name)?,
            };
            text.push(format!("{name} {}", span.describe()));
        }
        Ok(text.join("; "))
    }

    fn faces_span(&self, selector: &str) -> Result<Span> {
        let Ok(solid) = self.active("measure") else {
            bail!(
                "no body called `{selector}`; bodies are {:?}",
                self.body_names()
            );
        };
        let faces = select::faces(solid);
        let chosen = select::select_faces(selector, solid, &self.groups, self.tolerance())?;
        let points = chosen.iter().flat_map(|&i| {
            faces[i]
                .edge_iter()
                .flat_map(|edge| geometry::curve_samples(&edge.curve()))
                .collect::<Vec<_>>()
        });
        match Span::around(points) {
            Some(span) => Ok(span),
            None => bail!("`{selector}` matched no faces"),
        }
    }

    pub(crate) fn measure_clearance(&self, a: &str, b: &str) -> Result<String> {
        let (first, second) = (self.named_body(a)?, self.named_body(b)?);
        let (sa, sb) = (
            Span::of_box(&geometry::bounds(&first)),
            Span::of_box(&geometry::bounds(&second)),
        );
        let gaps: V = std::array::from_fn(|k| (sb.low[k] - sa.high[k]).max(sa.low[k] - sb.high[k]));
        let boxes = format!(
            "box gaps x {} y {} z {} (negative overlaps)",
            fixed(gaps[0]),
            fixed(gaps[1]),
            fixed(gaps[2])
        );
        let meshes = geometry::meshed_all(&[&first, &second]);
        if let Some(shared) = meshes[0].clash(&meshes[1]) {
            return Ok(format!(
                "no clearance, {a} and {b} share about {shared:.3}; {boxes}"
            ));
        }
        let (gap, from, to) = nearest(meshes[0].triangles(), meshes[1].triangles());
        let touching = if gap < self.tolerance() * 10.0 {
            ", touching"
        } else {
            ""
        };
        Ok(format!(
            "clearance {}{touching} from {a} at {} to {b} at {}; {boxes}",
            fixed(gap),
            triple(from, ","),
            triple(to, ",")
        ))
    }

    pub(crate) fn measure_space(&self, at: Option<Point3>) -> Result<String> {
        let bodies = self.named_solids();
        if bodies.is_empty() {
            bail!("there are no bodies to measure the space between yet");
        }
        let solids: Vec<&Solid> = bodies.iter().map(|(_, s)| *s).collect();
        let meshes = geometry::meshed_all(&solids);
        let extent = meshes
            .iter()
            .map(|m| Span::of_box(m.bounds()))
            .reduce(Span::join)
            .expect("one body");
        let seed = at.map_or_else(
            || std::array::from_fn(|k| (extent.low[k] + extent.high[k]) / 2.0),
            |p| [p.x, p.y, p.z],
        );
        let place = triple(seed, ",");
        if let Some(inside) = meshes.iter().position(|m| encloses(m.triangles(), seed)) {
            bail!(
                "{place} is inside {}; give `at=x,y,z` a point in the empty space (`measure box` says where each body is)",
                bodies[inside].0
            );
        }
        let walls = Walls::new(&meshes);
        let (found, sides) = walls.grow(seed, extent).ok_or_else(|| {
            anyhow::anyhow!("{place} is on a surface; move `at=` into the empty space")
        })?;
        let name = |by: Option<usize>| by.map_or("open", |b| bodies[b].0.as_str());
        let sides: Vec<String> = sides
            .iter()
            .zip(SIDES)
            .map(|(by, side)| format!("{side} {}", name(*by)))
            .collect();
        let reach: Vec<String> = SIDES
            .iter()
            .enumerate()
            .map(|(side, label)| match walls.reach(seed, side) {
                Some((t, body)) => format!("{label} {} {}", fixed(t), name(Some(body))),
                None => format!("{label} open"),
            })
            .collect();
        Ok(format!(
            "space {}; {}; reach from {place}: {}",
            found.describe(),
            sides.join(", "),
            reach.join(", ")
        ))
    }
}

struct Walls {
    triangles: Vec<([V; 3], Span, usize)>,
}

impl Walls {
    fn new(meshes: &[std::sync::Arc<Meshed>]) -> Walls {
        let triangles = meshes
            .iter()
            .enumerate()
            .flat_map(|(body, mesh)| {
                mesh.triangles().iter().map(move |t| {
                    let corners = t.map(|p| [p.x, p.y, p.z]);
                    let span = Span::around(t.iter().copied()).expect("three corners");
                    (corners, span, body)
                })
            })
            .collect();
        Walls { triangles }
    }

    fn blocker(&self, region: &Span) -> Option<usize> {
        self.triangles
            .iter()
            .find(|(t, span, _)| span.meets(region) && meets(t, region))
            .map(|(_, _, body)| *body)
    }

    fn reach(&self, from: V, side: usize) -> Option<(f64, usize)> {
        let (k, sign) = (side / 2, if side % 2 == 1 { 1.0 } else { -1.0 });
        let (a, b) = ((k + 1) % 3, (k + 2) % 3);
        self.triangles
            .iter()
            .filter(|(_, span, _)| {
                span.low[a] <= from[a]
                    && from[a] <= span.high[a]
                    && span.low[b] <= from[b]
                    && from[b] <= span.high[b]
            })
            .filter_map(|(t, _, body)| {
                let [p, q, r] = t.map(|v| [v[a] - from[a], v[b] - from[b]]);
                let area = (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
                if area.abs() < 1.0e-18 {
                    return None;
                }
                let u = (p[1] * r[0] - p[0] * r[1]) / area;
                let v = (q[1] * p[0] - q[0] * p[1]) / area;
                let depth = t[0][k] + u * (t[1][k] - t[0][k]) + v * (t[2][k] - t[0][k]);
                let ahead = (depth - from[k]) * sign;
                (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && ahead > 0.0).then_some((ahead, *body))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))
    }

    fn grow(&self, seed: V, extent: Span) -> Option<(Span, [Option<usize>; 6])> {
        let size = (0..3)
            .map(|k| extent.high[k] - extent.low[k])
            .fold(0.0, f64::max)
            .max(1.0e-6);
        let least = size * 1.0e-6;
        let mut found = Span {
            low: seed.map(|v| v - least),
            high: seed.map(|v| v + least),
        };
        if self.blocker(&found).is_some() {
            return None;
        }
        let slab = |found: &Span, side: usize, reach: f64| {
            let (k, up) = (side / 2, side % 2 == 1);
            let mut region = *found;
            if up {
                region.low[k] = found.high[k] - least;
                region.high[k] = found.high[k] + reach;
            } else {
                region.high[k] = found.low[k] + least;
                region.low[k] = found.low[k] - reach;
            }
            region
        };
        let limit = |side: usize| {
            let k = side / 2;
            if side % 2 == 1 {
                extent.high[k]
            } else {
                extent.low[k]
            }
        };
        let room = |found: &Span, side: usize| {
            let k = side / 2;
            if side % 2 == 1 {
                limit(side) - found.high[k]
            } else {
                found.low[k] - limit(side)
            }
        };
        let mut step = size / 64.0;
        while step >= least {
            let mut moved = true;
            while moved {
                moved = false;
                for side in 0..6 {
                    let reach = step.min(room(&found, side));
                    if reach <= 0.0 {
                        continue;
                    }
                    let region = slab(&found, side, reach);
                    if self.blocker(&region).is_none() {
                        let k = side / 2;
                        if side % 2 == 1 {
                            found.high[k] += reach;
                        } else {
                            found.low[k] -= reach;
                        }
                        moved = true;
                    }
                }
            }
            step /= 2.0;
        }
        let sides = std::array::from_fn(|side| self.blocker(&slab(&found, side, least * 8.0)));
        Some((found, sides))
    }
}

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn meets(triangle: &[V; 3], region: &Span) -> bool {
    let centre: V = std::array::from_fn(|k| (region.low[k] + region.high[k]) / 2.0);
    let half: V = std::array::from_fn(|k| (region.high[k] - region.low[k]) / 2.0);
    let v = triangle.map(|p| sub(p, centre));
    let apart = |axis: V| {
        let r = half[0] * axis[0].abs() + half[1] * axis[1].abs() + half[2] * axis[2].abs();
        let p = v.map(|q| dot(q, axis));
        let (low, high) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
        low > r || high < -r
    };
    let units = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    if units.iter().any(|u| apart(*u)) {
        return false;
    }
    let edges = [sub(v[1], v[0]), sub(v[2], v[1]), sub(v[0], v[2])];
    let normal = cross(edges[0], edges[1]);
    if dot(normal, normal) > 0.0 && apart(normal) {
        return false;
    }
    !edges.iter().any(|e| {
        units.iter().any(|u| {
            let axis = cross(*u, *e);
            dot(axis, axis) > 1.0e-24 && apart(axis)
        })
    })
}

fn encloses(triangles: &[[Point3; 3]], at: V) -> bool {
    let direction = Vector3::new(0.5773, 0.5779, 0.5767).normalize();
    let origin = Point3::new(at[0], at[1], at[2]);
    let crossings = triangles
        .iter()
        .filter(|[a, b, c]| {
            let (e1, e2) = (b - a, c - a);
            let p = direction.cross(e2);
            let det = e1.dot(p);
            if det.abs() < 1.0e-14 {
                return false;
            }
            let s = origin - a;
            let u = s.dot(p) / det;
            let q = s.cross(e1);
            let v = direction.dot(q) / det;
            u >= 0.0 && v >= 0.0 && u + v <= 1.0 && e2.dot(q) / det > 0.0
        })
        .count();
    crossings % 2 == 1
}

fn closest_on_triangle(p: V, [a, b, c]: [V; 3]) -> V {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let along = |from: V, to: V, t: f64| std::array::from_fn(|k| from[k] + (to[k] - from[k]) * t);
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return along(a, b, d1 / (d1 - d3));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return along(a, c, d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return along(b, c, (d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let total = 1.0 / (va + vb + vc);
    let (v, w) = (vb * total, vc * total);
    std::array::from_fn(|k| a[k] + ab[k] * v + ac[k] * w)
}

fn nearest(first: &[[Point3; 3]], second: &[[Point3; 3]]) -> (f64, V, V) {
    use rayon::prelude::*;
    let wide = |t: &[Point3; 3]| t.map(|p| [p.x, p.y, p.z]);
    let one_way = |from: &[[Point3; 3]], onto: &[[Point3; 3]]| {
        let onto: Vec<([V; 3], Span)> = onto
            .iter()
            .map(|t| (wide(t), Span::around(t.iter().copied()).expect("corners")))
            .collect();
        let mut corners: Vec<V> = from.iter().flat_map(wide).collect();
        corners.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        corners.dedup();
        corners
            .par_iter()
            .map(|&p| {
                let mut best = (f64::INFINITY, p, p);
                for (t, span) in &onto {
                    let outside: f64 = (0..3)
                        .map(|k| {
                            (span.low[k] - p[k])
                                .max(p[k] - span.high[k])
                                .max(0.0)
                                .powi(2)
                        })
                        .sum();
                    if outside >= best.0 * best.0 {
                        continue;
                    }
                    let q = closest_on_triangle(p, *t);
                    let d = dot(sub(q, p), sub(q, p)).sqrt();
                    if d < best.0 {
                        best = (d, p, q);
                    }
                }
                best
            })
            .reduce(
                || (f64::INFINITY, [0.0; 3], [0.0; 3]),
                |a, b| if b.0 < a.0 { b } else { a },
            )
    };
    let forward = one_way(first, second);
    let (gap, to, from) = one_way(second, first);
    if forward.0 <= gap {
        forward
    } else {
        (gap, from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::{Span, Walls, closest_on_triangle, meets};

    fn cube(low: [f64; 3], high: [f64; 3], body: usize) -> Vec<([[f64; 3]; 3], Span, usize)> {
        let corner =
            |i: usize| std::array::from_fn(|k| if i >> k & 1 == 1 { high[k] } else { low[k] });
        let quads = [
            [0, 2, 6, 4],
            [1, 5, 7, 3],
            [0, 4, 5, 1],
            [2, 3, 7, 6],
            [0, 1, 3, 2],
            [4, 6, 7, 5],
        ];
        quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .map(|t| {
                let corners = t.map(corner);
                let span = Span::around(
                    corners
                        .iter()
                        .map(|c| monstertruck::modeling::Point3::new(c[0], c[1], c[2])),
                )
                .expect("corners");
                (corners, span, body)
            })
            .collect()
    }

    #[test]
    fn a_closed_box_reaches_its_walls_and_fills_up() {
        let mut triangles = cube([-10.0, -5.0, 0.0], [10.0, 5.0, 4.0], 0);
        triangles.extend(cube([2.0, -1.0, 0.0], [4.0, 1.0, 1.0], 1));
        let walls = Walls { triangles };
        let from = [-2.0, 0.0, 2.0];
        let reach: Vec<_> = (0..6)
            .map(|side| {
                walls
                    .reach(from, side)
                    .map(|(t, b)| ((t * 1000.0).round() / 1000.0, b))
            })
            .collect();
        assert_eq!(
            reach,
            [
                Some((8.0, 0)),
                Some((12.0, 0)),
                Some((5.0, 0)),
                Some((5.0, 0)),
                Some((2.0, 0)),
                Some((2.0, 0))
            ]
        );
        let extent = Span {
            low: [-10.0, -5.0, 0.0],
            high: [10.0, 5.0, 4.0],
        };
        let (found, sides) = walls.grow(from, extent).expect("room");
        let size: Vec<f64> = (0..3)
            .map(|k| ((found.high[k] - found.low[k]) * 100.0).round() / 100.0)
            .collect();
        assert_eq!(size, [12.0, 10.0, 4.0], "{:?}..{:?}", found.low, found.high);
        assert_eq!(
            sides,
            [Some(0), Some(1), Some(0), Some(0), Some(0), Some(0)]
        );
    }

    fn region(low: [f64; 3], high: [f64; 3]) -> Span {
        Span { low, high }
    }

    #[test]
    fn a_triangle_through_a_box_meets_it_and_one_beside_does_not() {
        let through = [[-5.0, -5.0, 0.5], [5.0, -5.0, 0.5], [0.0, 5.0, 0.5]];
        assert!(meets(&through, &region([0.0; 3], [1.0; 3])));
        let beside = [[2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [2.0, 0.0, 3.0]];
        assert!(!meets(&beside, &region([0.0; 3], [1.0; 3])));
        let tilted_past_a_corner = [[1.6, 0.0, 0.0], [0.0, 1.6, 0.0], [0.0, 0.0, 1.6]];
        assert!(meets(&tilted_past_a_corner, &region([0.0; 3], [1.0; 3])));
        let beyond_the_corner = [[3.5, 0.0, 0.0], [0.0, 3.5, 0.0], [0.0, 0.0, 3.5]];
        assert!(!meets(&beyond_the_corner, &region([0.0; 3], [1.0; 3])));
    }

    #[test]
    fn a_wall_on_the_box_face_counts_as_touching() {
        let wall = [[1.0, -5.0, -5.0], [1.0, 5.0, -5.0], [1.0, 0.0, 5.0]];
        assert!(meets(&wall, &region([0.0; 3], [1.0; 3])));
    }

    #[test]
    fn the_closest_point_lands_inside_on_an_edge_or_on_a_corner() {
        let t = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]];
        assert_eq!(closest_on_triangle([1.0, 1.0, 3.0], t), [1.0, 1.0, 0.0]);
        assert_eq!(closest_on_triangle([2.0, -3.0, 0.0], t), [2.0, 0.0, 0.0]);
        assert_eq!(closest_on_triangle([-1.0, -1.0, 1.0], t), [0.0, 0.0, 0.0]);
    }
}
