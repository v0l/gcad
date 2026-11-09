use crate::mesh::{Mesh, P3};

pub type P2 = (f64, f64);

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub origin: P3,
    pub x: P3,
    pub y: P3,
    pub normal: P3,
}

impl Frame {
    pub fn named(name: &str) -> Option<Frame> {
        let (x, y, normal) = match name.to_ascii_uppercase().as_str() {
            "XY" => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            "XZ" => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
            "YZ" => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            _ => return None,
        };
        Some(Frame {
            origin: [0.0; 3],
            x,
            y,
            normal,
        })
    }

    pub fn offset(&self, distance: f64) -> Frame {
        Frame {
            origin: along(self.origin, self.normal, distance),
            ..*self
        }
    }

    pub fn moved(&self, by: P3) -> Frame {
        Frame {
            origin: crate::mesh::add(self.origin, by),
            ..*self
        }
    }

    pub fn plane_vector(&self, u: f64, v: f64) -> P3 {
        crate::mesh::add(crate::mesh::scale(self.x, u), crate::mesh::scale(self.y, v))
    }

    fn at(&self, p: P2, z: f64) -> P3 {
        let flat = self.plane_vector(p.0, p.1);
        along(crate::mesh::add(self.origin, flat), self.normal, z)
    }
}

fn along(p: P3, direction: P3, distance: f64) -> P3 {
    crate::mesh::add(p, crate::mesh::scale(direction, distance))
}

#[derive(Clone, Debug)]
pub struct Region {
    pub outer: Vec<P2>,
    pub holes: Vec<Vec<P2>>,
}

#[derive(Clone, Debug)]
pub struct Extrude {
    pub label: String,
    pub frame: Frame,
    pub regions: Vec<Region>,
    pub depth: f64,
    pub start_round: f64,
    pub end_round: f64,
    pub copies: Vec<P3>,
    pub scale: f64,
}

const SHARP_DEGREES: f64 = 40.0;
const ROUND_RINGS: usize = 4;
const EPS: f64 = 1.0e-9;

fn circle_segments(radius: f64) -> usize {
    if radius >= 20.0 { 32 } else { 24 }
}

pub fn circle(centre: P2, diameter: f64) -> Vec<P2> {
    let r = diameter / 2.0;
    let n = circle_segments(r);
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            (centre.0 + r * a.cos(), centre.1 + r * a.sin())
        })
        .collect()
}

pub fn ngon(centre: P2, diameter: f64, sides: usize, start_degrees: f64) -> Vec<P2> {
    let r = diameter / 2.0;
    let start = start_degrees.to_radians();
    (0..sides)
        .map(|i| {
            let a = start + std::f64::consts::TAU * i as f64 / sides as f64;
            (centre.0 + r * a.cos(), centre.1 + r * a.sin())
        })
        .collect()
}

pub fn ellipse(centre: P2, width: f64, height: f64) -> Vec<P2> {
    let n = circle_segments(width.max(height) / 2.0);
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            (
                centre.0 + width / 2.0 * a.cos(),
                centre.1 + height / 2.0 * a.sin(),
            )
        })
        .collect()
}

pub fn rect(centre: P2, width: f64, height: f64, radius: f64) -> Vec<P2> {
    let (hw, hh) = (width / 2.0, height / 2.0);
    if radius <= 0.0 {
        return vec![
            (centre.0 - hw, centre.1 - hh),
            (centre.0 + hw, centre.1 - hh),
            (centre.0 + hw, centre.1 + hh),
            (centre.0 - hw, centre.1 + hh),
        ];
    }
    let steps = (circle_segments(radius) / 4).max(3);
    let corners = [
        (hw - radius, -(hh - radius), -90.0),
        (hw - radius, hh - radius, 0.0),
        (-(hw - radius), hh - radius, 90.0),
        (-(hw - radius), -(hh - radius), 180.0),
    ];
    corners
        .iter()
        .flat_map(|&(cx, cy, start)| {
            (0..=steps).map(move |k| {
                let a = (start + 90.0 * k as f64 / steps as f64).to_radians();
                (
                    centre.0 + cx + radius * a.cos(),
                    centre.1 + cy + radius * a.sin(),
                )
            })
        })
        .collect()
}

pub fn placed(loop_: &[P2], matrix: [f64; 6]) -> Vec<P2> {
    let [a, b, c, d, tx, ty] = matrix;
    loop_
        .iter()
        .map(|&(u, v)| (a * u + b * v + tx, c * u + d * v + ty))
        .collect()
}

fn signed_area(points: &[P2]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f64>()
        / 2.0
}

fn counter_clockwise(mut points: Vec<P2>) -> Vec<P2> {
    if signed_area(&points) < 0.0 {
        points.reverse();
    }
    points
}

fn clockwise(mut points: Vec<P2>) -> Vec<P2> {
    if signed_area(&points) > 0.0 {
        points.reverse();
    }
    points
}

fn inside(point: P2, polygon: &[P2]) -> bool {
    let n = polygon.len();
    let mut result = false;
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        if (a.1 > point.1) != (b.1 > point.1)
            && point.0 < (b.0 - a.0) * (point.1 - a.1) / (b.1 - a.1) + a.0
        {
            result = !result;
        }
    }
    result
}

pub fn regions(loops: &[Vec<P2>]) -> Vec<Region> {
    let containers: Vec<Vec<usize>> = loops
        .iter()
        .enumerate()
        .map(|(i, l)| {
            (0..loops.len())
                .filter(|&j| j != i && inside(l[0], &loops[j]))
                .collect()
        })
        .collect();
    let mut out: Vec<(usize, Region)> = Vec::new();
    for (i, l) in loops.iter().enumerate() {
        if containers[i].len().is_multiple_of(2) {
            out.push((
                i,
                Region {
                    outer: counter_clockwise(l.clone()),
                    holes: Vec::new(),
                },
            ));
        }
    }
    for (i, l) in loops.iter().enumerate() {
        if containers[i].len() % 2 == 1 {
            let parent = containers[i]
                .iter()
                .filter(|j| containers[**j].len().is_multiple_of(2))
                .min_by(|a, b| {
                    signed_area(&loops[**a])
                        .abs()
                        .total_cmp(&signed_area(&loops[**b]).abs())
                })
                .copied();
            if let Some(parent) = parent
                && let Some((_, region)) = out.iter_mut().find(|(k, _)| *k == parent)
            {
                region.holes.push(clockwise(l.clone()));
            }
        }
    }
    out.into_iter().map(|(_, r)| r).collect()
}

fn cross(o: P2, a: P2, b: P2) -> f64 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

fn same(a: P2, b: P2) -> bool {
    (a.0 - b.0).abs() < EPS && (a.1 - b.1).abs() < EPS
}

fn crosses(a: P2, b: P2, c: P2, d: P2) -> bool {
    if same(a, c) || same(a, d) || same(b, c) || same(b, d) {
        return false;
    }
    let (d1, d2) = (cross(a, b, c), cross(a, b, d));
    let (d3, d4) = (cross(c, d, a), cross(c, d, b));
    d1 * d2 <= 0.0 && d3 * d4 <= 0.0 && !(d1 == 0.0 && d2 == 0.0)
}

fn hits_loop(a: P2, b: P2, polygon: &[P2]) -> bool {
    let n = polygon.len();
    (0..n).any(|i| crosses(a, b, polygon[i], polygon[(i + 1) % n]))
}

fn bridge(poly: &mut Vec<P2>, hole: &[P2], others: &[&Vec<P2>]) {
    let m = (0..hole.len())
        .max_by(|&a, &b| hole[a].0.total_cmp(&hole[b].0))
        .unwrap_or(0);
    let tip = hole[m];
    let mut order: Vec<usize> = (0..poly.len()).collect();
    order.sort_by(|&a, &b| {
        let da = (poly[a].0 - tip.0).hypot(poly[a].1 - tip.1);
        let db = (poly[b].0 - tip.0).hypot(poly[b].1 - tip.1);
        da.total_cmp(&db)
    });
    let visible = |c: usize| {
        let from = poly[c];
        let mid = ((from.0 + tip.0) / 2.0, (from.1 + tip.1) / 2.0);
        !hits_loop(from, tip, poly)
            && !hits_loop(from, tip, hole)
            && !others.iter().any(|o| hits_loop(from, tip, o))
            && !inside(mid, hole)
    };
    let chosen = order
        .iter()
        .copied()
        .find(|&c| visible(c))
        .unwrap_or(order[0]);
    let mut merged: Vec<P2> = poly[..=chosen].to_vec();
    merged.extend((0..=hole.len()).map(|k| hole[(m + k) % hole.len()]));
    merged.push(poly[chosen]);
    merged.extend_from_slice(&poly[chosen + 1..]);
    *poly = merged;
}

pub fn triangulate(outer: &[P2], holes: &[Vec<P2>]) -> (Vec<P2>, Vec<[u32; 3]>) {
    let mut poly = outer.to_vec();
    let mut pending: Vec<&Vec<P2>> = holes.iter().collect();
    while !pending.is_empty() {
        let next = pending.remove(0);
        bridge(&mut poly, next, &pending);
    }
    let mut ring: Vec<usize> = (0..poly.len()).collect();
    let mut triangles = Vec::new();
    let in_triangle = |p: P2, a: P2, b: P2, c: P2| {
        cross(a, b, p) >= -EPS && cross(b, c, p) >= -EPS && cross(c, a, p) >= -EPS
    };
    while ring.len() > 3 {
        let n = ring.len();
        let mut clipped = false;
        for pass in 0..2 {
            for i in 0..n {
                let (ia, ib, ic) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
                let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
                let turn = cross(a, b, c);
                if turn.abs() <= EPS {
                    ring.remove(i);
                    clipped = true;
                    break;
                }
                if turn < 0.0 {
                    continue;
                }
                let blocked = ring.iter().any(|&k| {
                    let p = poly[k];
                    k != ia
                        && k != ib
                        && k != ic
                        && !same(p, a)
                        && !same(p, b)
                        && !same(p, c)
                        && in_triangle(p, a, b, c)
                });
                if blocked && pass == 0 {
                    continue;
                }
                triangles.push([ia as u32, ib as u32, ic as u32]);
                ring.remove(i);
                clipped = true;
                break;
            }
            if clipped {
                break;
            }
        }
        if !clipped {
            break;
        }
    }
    if ring.len() == 3 && cross(poly[ring[0]], poly[ring[1]], poly[ring[2]]).abs() > EPS {
        triangles.push([ring[0] as u32, ring[1] as u32, ring[2] as u32]);
    }
    (poly, triangles)
}

fn unit(v: P2) -> P2 {
    let l = v.0.hypot(v.1);
    if l < EPS {
        (0.0, 0.0)
    } else {
        (v.0 / l, v.1 / l)
    }
}

struct Corner {
    inward: P2,
    sharp: bool,
}

fn corners(points: &[P2]) -> Vec<Corner> {
    let n = points.len();
    let limit = SHARP_DEGREES.to_radians().cos();
    (0..n)
        .map(|i| {
            let (p, q, r) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
            let d0 = unit((q.0 - p.0, q.1 - p.1));
            let d1 = unit((r.0 - q.0, r.1 - q.1));
            let (n0, n1) = ((-d0.1, d0.0), (-d1.1, d1.0));
            let along = n0.0 * n1.0 + n0.1 * n1.1;
            let scale = 1.0 / (1.0 + along).max(0.35);
            Corner {
                inward: ((n0.0 + n1.0) * scale, (n0.1 + n1.1) * scale),
                sharp: d0.0 * d1.0 + d0.1 * d1.1 < limit,
            }
        })
        .collect()
}

fn inset(points: &[P2], corners: &[Corner], by: f64) -> Vec<P2> {
    points
        .iter()
        .zip(corners)
        .map(|(p, c)| (p.0 + c.inward.0 * by, p.1 + c.inward.1 * by))
        .collect()
}

fn rings(depth: f64, start: f64, end: f64) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    if start > 0.0 {
        for k in 0..=ROUND_RINGS {
            let phi = std::f64::consts::FRAC_PI_2 * k as f64 / ROUND_RINGS as f64;
            out.push((start * (1.0 - phi.sin()), start * (1.0 - phi.cos())));
        }
    } else {
        out.push((0.0, 0.0));
    }
    if end > 0.0 {
        for k in 0..=ROUND_RINGS {
            let phi = std::f64::consts::FRAC_PI_2 * k as f64 / ROUND_RINGS as f64;
            out.push((end * (1.0 - phi.cos()), depth - end + end * phi.sin()));
        }
    } else {
        out.push((0.0, depth));
    }
    out.dedup_by(|a, b| (a.0 - b.0).abs() < EPS && (a.1 - b.1).abs() < EPS);
    out
}

fn runs(corners: &[Corner]) -> (Vec<Vec<usize>>, bool) {
    let n = corners.len();
    let sharp: Vec<usize> = (0..n).filter(|&i| corners[i].sharp).collect();
    if sharp.is_empty() {
        return (vec![(0..n).collect()], true);
    }
    let runs = (0..sharp.len())
        .map(|k| {
            let from = sharp[k];
            let to = sharp[(k + 1) % sharp.len()];
            let length = (to + n - from) % n;
            let length = if length == 0 { n } else { length };
            (0..=length).map(|s| (from + s) % n).collect()
        })
        .collect();
    (runs, false)
}

#[derive(Clone, Debug)]
pub struct Torus {
    pub label: String,
    pub frame: Frame,
    pub centre: P2,
    pub ring: f64,
    pub tube: f64,
    pub around: usize,
    pub sides: usize,
}

pub fn torus_into(mesh: &mut Mesh, t: &Torus) {
    let part = mesh.part_index(&t.label);
    let (major, minor) = (t.ring / 2.0, t.tube / 2.0);
    let base = mesh.positions.len() as u32;
    for i in 0..t.around {
        let a = std::f64::consts::TAU * i as f64 / t.around as f64;
        for j in 0..t.sides {
            let b = std::f64::consts::TAU * j as f64 / t.sides as f64;
            let radius = major + minor * b.cos();
            let flat = (
                t.centre.0 + radius * a.cos(),
                t.centre.1 + radius * a.sin(),
            );
            push(mesh, part, t.frame.at(flat, minor * b.sin()));
        }
    }
    let at = |i: usize, j: usize| base + ((i % t.around) * t.sides + j % t.sides) as u32;
    for i in 0..t.around {
        for j in 0..t.sides {
            quad(
                mesh,
                [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)],
                false,
            );
        }
    }
}

pub fn extrude_into(mesh: &mut Mesh, e: &Extrude) {
    emit(mesh, e, e.frame);
    for by in &e.copies {
        emit(mesh, e, e.frame.moved(*by));
    }
}

fn emit(mesh: &mut Mesh, e: &Extrude, frame: Frame) {
    let part = mesh.part_index(&e.label);
    let sign = if e.depth < 0.0 { -1.0 } else { 1.0 };
    let depth = e.depth.abs();
    let levels = rings(depth, e.start_round, e.end_round);
    let grown = |p: P2, z: f64| {
        let k = 1.0 + (e.scale - 1.0) * z / depth;
        (p.0 * k, p.1 * k)
    };
    for region in &e.regions {
        let mut loops: Vec<&Vec<P2>> = vec![&region.outer];
        loops.extend(region.holes.iter());
        let loop_corners: Vec<Vec<Corner>> = loops.iter().map(|l| corners(l)).collect();
        for (points, around) in loops.iter().zip(&loop_corners) {
            let (runs, closed) = runs(around);
            for run in runs {
                let ids: Vec<Vec<u32>> = run
                    .iter()
                    .map(|&i| {
                        levels
                            .iter()
                            .map(|&(by, z)| {
                                let flat = inset(
                                    &[points[i]],
                                    &[Corner {
                                        inward: around[i].inward,
                                        sharp: false,
                                    }],
                                    by,
                                )[0];
                                push(mesh, part, frame.at(grown(flat, z), sign * z))
                            })
                            .collect()
                    })
                    .collect();
                let columns = if closed { ids.len() } else { ids.len() - 1 };
                for c in 0..columns {
                    let (a, b) = (&ids[c], &ids[(c + 1) % ids.len()]);
                    for j in 0..levels.len() - 1 {
                        quad(mesh, [a[j], b[j], b[j + 1], a[j + 1]], sign < 0.0);
                    }
                }
            }
        }
        for (z, ring_index, up) in [(0.0, 0, false), (depth, levels.len() - 1, true)] {
            if up && e.scale < EPS {
                continue;
            }
            let by = levels[ring_index].0;
            let shrunk: Vec<Vec<P2>> = loops
                .iter()
                .zip(&loop_corners)
                .map(|(l, c)| inset(l, c, by))
                .collect();
            let (points, triangles) = triangulate(&shrunk[0], &shrunk[1..]);
            let base = mesh.positions.len() as u32;
            for p in &points {
                push(mesh, part, frame.at(grown(*p, z), sign * z));
            }
            let flip = up == (sign < 0.0);
            for t in triangles {
                let t = t.map(|i| i + base);
                mesh.triangles
                    .push(if flip { [t[0], t[2], t[1]] } else { t });
            }
        }
    }
}

fn push(mesh: &mut Mesh, part: u16, p: P3) -> u32 {
    mesh.positions.push(p);
    mesh.labels.push(part);
    (mesh.positions.len() - 1) as u32
}

fn quad(mesh: &mut Mesh, q: [u32; 4], flip: bool) {
    let [a, b, c, d] = q;
    if flip {
        mesh.triangles.push([a, c, b]);
        mesh.triangles.push([a, d, c]);
    } else {
        mesh.triangles.push([a, b, c]);
        mesh.triangles.push([a, c, d]);
    }
}
