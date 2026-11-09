use super::P2;
use super::layout::{add, cross, dot, left, length, meet, scale, signed_area, sub, unit};

pub type P3 = [f64; 3];

const WELD: f64 = 1.0e-6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub c: f64,
    pub g: P2,
}

impl Plane {
    pub fn level(z: f64) -> Plane {
        Plane {
            c: z,
            g: (0.0, 0.0),
        }
    }

    pub fn rising_from(a: P2, b: P2, base: f64, pitch: f64) -> Plane {
        let n = left(unit(sub(b, a)));
        let tan = pitch.to_radians().tan();
        Plane {
            c: base - tan * dot(n, a),
            g: scale(n, tan),
        }
    }

    pub fn at(&self, p: P2) -> f64 {
        self.c + dot(self.g, p)
    }

    pub fn lifted(&self, thickness: f64) -> Plane {
        Plane {
            c: self.c + thickness * (1.0 + dot(self.g, self.g)).sqrt(),
            g: self.g,
        }
    }
}

pub fn envelope(planes: &[Plane], p: P2) -> f64 {
    planes
        .iter()
        .map(|plane| plane.at(p))
        .fold(f64::INFINITY, f64::min)
}

pub fn ccw_ring(mut ring: Vec<P2>) -> Vec<P2> {
    if signed_area(&ring) < 0.0 {
        ring.reverse();
    }
    ring
}

pub fn is_convex(ring: &[P2]) -> bool {
    let n = ring.len();
    let area = signed_area(ring);
    (0..n).all(|i| {
        let (a, b, c) = (ring[i], ring[(i + 1) % n], ring[(i + 2) % n]);
        cross(sub(b, a), sub(c, b)) * area.signum()
            >= -1.0e-9 * length(sub(b, a)) * length(sub(c, b))
    })
}

pub fn offset_ring(ring: &[P2], d: f64) -> Vec<P2> {
    let n = ring.len();
    let line = |i: usize| {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let dir = unit(sub(b, a));
        (add(a, scale(left(dir), -d)), dir)
    };
    (0..n)
        .map(|i| {
            let (p, d0) = line((i + n - 1) % n);
            let (q, d1) = line(i);
            meet(p, d0, q, d1).unwrap_or(q)
        })
        .collect()
}

fn clip(ring: &[P2], keep: impl Fn(P2) -> f64) -> Vec<P2> {
    let mut out = Vec::new();
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let (fa, fb) = (keep(a), keep(b));
        if fa <= 0.0 {
            out.push(a);
        }
        if (fa < 0.0 && fb > 0.0) || (fa > 0.0 && fb < 0.0) {
            out.push(add(a, scale(sub(b, a), fa / (fa - fb))));
        }
    }
    let mut kept: Vec<P2> = Vec::new();
    for p in out {
        if kept.last().is_none_or(|q| length(sub(p, *q)) > WELD) {
            kept.push(p);
        }
    }
    while kept.len() > 1 && length(sub(kept[0], kept[kept.len() - 1])) <= WELD {
        kept.pop();
    }
    kept
}

pub fn cells(footprint: &[P2], planes: &[Plane]) -> Vec<(usize, Vec<P2>)> {
    let mut out = Vec::new();
    for (i, pi) in planes.iter().enumerate() {
        let mut cell = footprint.to_vec();
        for (j, pj) in planes.iter().enumerate() {
            if i == j || cell.len() < 3 {
                continue;
            }
            let dg = sub(pi.g, pj.g);
            let dc = pi.c - pj.c;
            if length(dg) < 1.0e-12 {
                if dc > 1.0e-9 || (dc.abs() <= 1.0e-9 && j < i) {
                    cell.clear();
                }
                continue;
            }
            cell = clip(&cell, |p| dc + dot(dg, p));
        }
        if cell.len() >= 3 && signed_area(&cell).abs() > 1.0e-6 {
            out.push((i, cell));
        }
    }
    out
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shell {
    pub points: Vec<P3>,
    pub faces: Vec<Vec<usize>>,
}

impl Shell {
    pub fn volume(&self) -> f64 {
        self.faces
            .iter()
            .map(|f| {
                let o = self.points[f[0]];
                (1..f.len() - 1)
                    .map(|k| {
                        let (a, b) = (self.points[f[k]], self.points[f[k + 1]]);
                        o[0] * (a[1] * b[2] - a[2] * b[1]) - o[1] * (a[0] * b[2] - a[2] * b[0])
                            + o[2] * (a[0] * b[1] - a[1] * b[0])
                    })
                    .sum::<f64>()
            })
            .sum::<f64>()
            / 6.0
    }

    pub fn is_closed(&self) -> bool {
        let mut edges: std::collections::BTreeMap<(usize, usize), i32> = Default::default();
        for f in &self.faces {
            for k in 0..f.len() {
                let (a, b) = (f[k], f[(k + 1) % f.len()]);
                *edges.entry((a.min(b), a.max(b))).or_default() += if a < b { 1 } else { -1 };
            }
        }
        edges.values().all(|&v| v == 0)
    }

    fn from_polygons(polygons: Vec<Vec<P3>>) -> Shell {
        let mut shell = Shell::default();
        let index = |p: P3, points: &mut Vec<P3>| -> usize {
            match points.iter().position(|q| {
                (q[0] - p[0]).abs() <= WELD
                    && (q[1] - p[1]).abs() <= WELD
                    && (q[2] - p[2]).abs() <= WELD
            }) {
                Some(i) => i,
                None => {
                    points.push(p);
                    points.len() - 1
                }
            }
        };
        let mut faces: Vec<Vec<usize>> = Vec::new();
        for polygon in polygons {
            let ids: Vec<usize> = polygon
                .iter()
                .map(|&p| index(p, &mut shell.points))
                .collect();
            faces.push(ids);
        }
        let points = &shell.points;
        shell.faces = faces
            .into_iter()
            .map(|face| {
                let mut out = Vec::new();
                for k in 0..face.len() {
                    let (a, b) = (face[k], face[(k + 1) % face.len()]);
                    out.push(a);
                    out.extend(between(points, a, b));
                }
                let mut kept: Vec<usize> = Vec::new();
                for id in out {
                    if kept.last() != Some(&id) {
                        kept.push(id);
                    }
                }
                while kept.len() > 1 && kept.first() == kept.last() {
                    kept.pop();
                }
                kept
            })
            .filter(|f| f.len() >= 3)
            .collect();
        shell
    }
}

fn between(points: &[P3], a: usize, b: usize) -> Vec<usize> {
    let (pa, pb) = (points[a], points[b]);
    let d = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
    let len2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if len2 <= WELD * WELD {
        return Vec::new();
    }
    let mut hits: Vec<(f64, usize)> = points
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != a && i != b)
        .filter_map(|(i, p)| {
            let v = [p[0] - pa[0], p[1] - pa[1], p[2] - pa[2]];
            let t = (v[0] * d[0] + v[1] * d[1] + v[2] * d[2]) / len2;
            let off = [v[0] - t * d[0], v[1] - t * d[1], v[2] - t * d[2]];
            let gap = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt();
            let along = t * len2.sqrt();
            (gap <= 10.0 * WELD && along > WELD && along < len2.sqrt() - WELD).then_some((t, i))
        })
        .collect();
    hits.sort_by(|x, y| x.0.total_cmp(&y.0));
    hits.into_iter().map(|(_, i)| i).collect()
}

pub fn roof_shell(footprint: &[P2], planes: &[Plane], thickness: f64) -> Result<Shell, String> {
    if planes.is_empty() {
        return Err("a roof needs at least one roof plane".into());
    }
    if !(thickness > 0.0) {
        return Err("a roof's thickness must be above zero".into());
    }
    let ring = ccw_ring(footprint.to_vec());
    if ring.len() < 3 || signed_area(&ring).abs() < 1.0e-6 {
        return Err("a roof outline needs three or more points around an area".into());
    }
    if !is_convex(&ring) {
        return Err(
            "this roof outline is not convex; build it as several roofs and `roof_join` them"
                .into(),
        );
    }
    let tops: Vec<Plane> = planes.iter().map(|p| p.lifted(thickness)).collect();
    let under = cells(&ring, planes);
    let over = cells(&ring, &tops);
    let lift = |cell: &[P2], plane: &Plane| -> Vec<P3> {
        cell.iter().map(|&p| [p.0, p.1, plane.at(p)]).collect()
    };
    let mut polygons: Vec<Vec<P3>> = Vec::new();
    for (i, cell) in &under {
        let mut face = lift(cell, &planes[*i]);
        face.reverse();
        polygons.push(face);
    }
    for (i, cell) in &over {
        polygons.push(lift(cell, &tops[*i]));
    }
    let breaks: Vec<P2> = under
        .iter()
        .chain(&over)
        .flat_map(|(_, cell)| cell.iter().copied())
        .collect();
    let n = ring.len();
    for e in 0..n {
        let (a, b) = (ring[e], ring[(e + 1) % n]);
        let d = sub(b, a);
        let l = length(d);
        let mut along: Vec<f64> = vec![0.0, l];
        for &p in &breaks {
            let v = sub(p, a);
            let t = dot(v, d) / l;
            if cross(d, v).abs() / l <= 10.0 * WELD && t > WELD && t < l - WELD {
                along.push(t);
            }
        }
        along.sort_by(f64::total_cmp);
        along.dedup_by(|x, y| (*x - *y).abs() <= WELD);
        let at = |t: f64| add(a, scale(d, t / l));
        let mut face: Vec<P3> = along
            .iter()
            .map(|&t| {
                let p = at(t);
                [p.0, p.1, envelope(planes, p)]
            })
            .collect();
        face.extend(along.iter().rev().map(|&t| {
            let p = at(t);
            [p.0, p.1, envelope(&tops, p)]
        }));
        polygons.push(face);
    }
    let shell = Shell::from_polygons(polygons);
    if !shell.is_closed() {
        return Err("the roof planes do not close into a solid".into());
    }
    Ok(shell)
}

pub fn column_shell(
    pieces: &[Vec<P2>],
    z0: f64,
    floor: f64,
    planes: &[Plane],
) -> Result<Shell, String> {
    let pieces: Vec<Vec<P2>> = pieces.iter().map(|p| ccw_ring(p.clone())).collect();
    let mut tops: Vec<(Vec<P2>, Plane)> = Vec::new();
    for piece in &pieces {
        if !is_convex(piece) {
            return Err("a wall piece under the roof is not convex".into());
        }
        for (i, cell) in cells(piece, planes) {
            let plane = planes[i];
            let level = Plane::level(floor);
            let dg = plane.g;
            let dc = plane.c - floor;
            let above = clip(&cell, |p| -(dc + dot(dg, p)));
            let below = clip(&cell, |p| dc + dot(dg, p));
            for (part, on) in [(above, plane), (below, level)] {
                if part.len() >= 3 && signed_area(&part).abs() > 1.0e-6 {
                    tops.push((part, on));
                }
            }
        }
    }
    let top_at = |p: P2| envelope(planes, p).max(floor);
    let mut polygons: Vec<Vec<P3>> = Vec::new();
    for piece in &pieces {
        polygons.push(piece.iter().rev().map(|&p| [p.0, p.1, z0]).collect());
    }
    for (part, plane) in &tops {
        polygons.push(part.iter().map(|&p| [p.0, p.1, plane.at(p)]).collect());
    }
    let breaks: Vec<P2> = tops
        .iter()
        .flat_map(|(part, _)| part.iter().copied())
        .collect();
    for (k, piece) in pieces.iter().enumerate() {
        let m = piece.len();
        for e in 0..m {
            let (a, b) = (piece[e], piece[(e + 1) % m]);
            let shared = pieces.iter().enumerate().any(|(j, other)| {
                j != k
                    && (0..other.len())
                        .any(|f| same_segment((other[f], other[(f + 1) % other.len()]), a, b))
            });
            if shared {
                continue;
            }
            let d = sub(b, a);
            let l = length(d);
            let mut along: Vec<f64> = vec![0.0, l];
            for &p in &breaks {
                let v = sub(p, a);
                let t = dot(v, d) / l;
                if cross(d, v).abs() / l <= 10.0 * WELD && t > WELD && t < l - WELD {
                    along.push(t);
                }
            }
            along.sort_by(f64::total_cmp);
            along.dedup_by(|x, y| (*x - *y).abs() <= WELD);
            let at = |t: f64| add(a, scale(d, t / l));
            let mut face: Vec<P3> = vec![[a.0, a.1, z0], [b.0, b.1, z0]];
            face.extend(along.iter().rev().map(|&t| {
                let p = at(t);
                [p.0, p.1, top_at(p)]
            }));
            polygons.push(face);
        }
    }
    let shell = Shell::from_polygons(polygons);
    if !shell.is_closed() {
        return Err("the wall does not close under the roof".into());
    }
    Ok(shell)
}

pub fn eaves_along(footprint: &[P2], ridge: P2) -> Vec<bool> {
    let n = footprint.len();
    let r = unit(ridge);
    (0..n)
        .map(|i| {
            let d = unit(sub(footprint[(i + 1) % n], footprint[i]));
            dot(d, r).abs() >= std::f64::consts::FRAC_1_SQRT_2 - 1.0e-9
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoofEdge {
    pub kind: &'static str,
    pub a: P3,
    pub b: P3,
    pub outline_edge: Option<usize>,
}

fn on_segment(p: P2, a: P2, b: P2) -> bool {
    let d = sub(b, a);
    let l = length(d);
    let t = dot(sub(p, a), d) / l;
    cross(d, sub(p, a)).abs() / l <= 10.0 * WELD && t >= -WELD && t <= l + WELD
}

fn same_segment(s: (P2, P2), p: P2, q: P2) -> bool {
    let close = |a: P2, b: P2| length(sub(a, b)) <= 10.0 * WELD;
    (close(s.0, p) && close(s.1, q)) || (close(s.0, q) && close(s.1, p))
}

pub fn roof_edges(footprint: &[P2], planes: &[Plane], thickness: f64) -> Vec<RoofEdge> {
    let ring = ccw_ring(footprint.to_vec());
    let tops: Vec<Plane> = planes.iter().map(|p| p.lifted(thickness)).collect();
    let lift = |p: P2| [p.0, p.1, envelope(&tops, p)];
    let n = ring.len();
    let side = |e: usize| (ring[e], ring[(e + 1) % n]);
    let mut out = Vec::new();
    for e in 0..n {
        let (a, b) = side(e);
        let mid = scale(add(a, b), 0.5);
        let (za, zb, zm) = (lift(a)[2], lift(b)[2], lift(mid)[2]);
        let level = (za - zb).abs() <= 1.0e-6 && (za - zm).abs() <= 1.0e-6;
        out.push(RoofEdge {
            kind: if level { "eave" } else { "verge" },
            a: lift(a),
            b: lift(b),
            outline_edge: Some(e),
        });
    }
    let mut seen: Vec<(P2, P2)> = Vec::new();
    for (i, cell) in cells(&ring, &tops) {
        let m = cell.len();
        for k in 0..m {
            let (p, q) = (cell[k], cell[(k + 1) % m]);
            let boundary = (0..n).any(|e| {
                let (a, b) = side(e);
                on_segment(p, a, b) && on_segment(q, a, b)
            });
            if length(sub(q, p)) <= 10.0 * WELD
                || boundary
                || seen.iter().any(|&s| same_segment(s, p, q))
            {
                continue;
            }
            seen.push((p, q));
            let mid = scale(add(p, q), 0.5);
            let other = tops
                .iter()
                .enumerate()
                .find(|&(j, plane)| j != i && (plane.at(mid) - tops[i].at(mid)).abs() <= 1.0e-6);
            let (pa, pb) = (lift(p), lift(q));
            let parallel = other.is_some_and(|(_, o)| {
                cross(o.g, tops[i].g).abs() <= 1.0e-9 && dot(o.g, tops[i].g) > 0.0
            });
            let kind = if (pa[2] - pb[2]).abs() > 1.0e-6 {
                "hip"
            } else if parallel {
                "curb"
            } else {
                "ridge"
            };
            out.push(RoofEdge {
                kind,
                a: pa,
                b: pb,
                outline_edge: None,
            });
        }
    }
    out
}

pub fn longest_edge(footprint: &[P2]) -> P2 {
    let n = footprint.len();
    (0..n)
        .map(|i| sub(footprint[(i + 1) % n], footprint[i]))
        .max_by(|a, b| length(*a).total_cmp(&length(*b)))
        .unwrap_or((1.0, 0.0))
}

pub fn gable_planes(footprint: &[P2], base: f64, pitch: f64, ridge: P2) -> Vec<Plane> {
    let ring = ccw_ring(footprint.to_vec());
    roof_planes("gable", &ring, base, &vec![pitch; ring.len()], ridge).unwrap_or_default()
}

pub const ROOF_KINDS: &[&str] = &[
    "gable", "hip", "half_hip", "shed", "flat", "mansard", "gambrel", "barrel",
];

const BARREL_PIECES: usize = 8;

fn steep(pitch: f64) -> f64 {
    pitch + (90.0 - pitch) / 2.0
}

fn broken(a: P2, b: P2, base: f64, pitch: f64, inset: f64) -> [Plane; 2] {
    let low = steep(pitch);
    let curb = base + inset * low.to_radians().tan();
    [
        Plane::rising_from(a, b, base, low),
        Plane::rising_from(a, b, curb - inset * pitch.to_radians().tan(), pitch),
    ]
}

fn across(ring: &[P2], ridge: P2) -> (P2, f64, f64) {
    let n = left(unit(ridge));
    let (lo, hi) = ring
        .iter()
        .map(|&p| dot(n, p))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), u| {
            (lo.min(u), hi.max(u))
        });
    (n, lo, hi)
}

fn barrel_planes(ring: &[P2], base: f64, pitch: f64, ridge: P2) -> Vec<Plane> {
    let (n, lo, hi) = across(ring, ridge);
    let half = (hi - lo) / 2.0;
    let spring = pitch.to_radians();
    let r = half / spring.sin();
    let zc = base - r * spring.cos();
    let mid = lo + half;
    let mut planes = vec![Plane::level(zc + r)];
    for k in 1..=BARREL_PIECES {
        let theta = spring * k as f64 / BARREL_PIECES as f64;
        for side in [-1.0, 1.0] {
            let u = mid + side * r * theta.sin();
            let z = zc + r * theta.cos();
            let slope = -side * theta.tan();
            planes.push(Plane {
                c: z - slope * u,
                g: scale(n, slope),
            });
        }
    }
    planes
}

pub fn roof_planes(
    kind: &str,
    ring: &[P2],
    base: f64,
    pitches: &[f64],
    ridge: P2,
) -> Result<Vec<Plane>, String> {
    let n = ring.len();
    if pitches.len() != n {
        return Err("a roof needs one pitch per outline edge".into());
    }
    let eaves = eaves_along(ring, ridge);
    let edge = |i: usize| (ring[i], ring[(i + 1) % n]);
    let (_, lo, hi) = across(ring, ridge);
    let inset = (hi - lo) / 6.0;
    let rise_of = |i: usize| (hi - lo) / 2.0 * pitches[i].to_radians().tan();
    let mut planes = Vec::new();
    match kind {
        "flat" => planes.push(Plane::level(base)),
        "gable" | "hip" | "half_hip" => {
            for i in 0..n {
                let (a, b) = edge(i);
                if eaves[i] || kind == "hip" {
                    planes.push(Plane::rising_from(a, b, base, pitches[i]));
                } else if kind == "half_hip" {
                    let main = (0..n).find(|&j| eaves[j]).unwrap_or(i);
                    planes.push(Plane::rising_from(
                        a,
                        b,
                        base + rise_of(main) / 2.0,
                        pitches[i],
                    ));
                }
            }
        }
        "shed" => {
            let r = unit(ridge);
            let low = (0..n)
                .filter(|&i| eaves[i])
                .find(|&i| dot(sub(ring[(i + 1) % n], ring[i]), r) > 0.0)
                .ok_or("a shed roof needs an eave edge along the ridge direction")?;
            let (a, b) = edge(low);
            planes.push(Plane::rising_from(a, b, base, pitches[low]));
        }
        "mansard" | "gambrel" => {
            for i in 0..n {
                if eaves[i] || kind == "mansard" {
                    let (a, b) = edge(i);
                    planes.extend(broken(a, b, base, pitches[i], inset));
                }
            }
        }
        "barrel" => {
            let main = (0..n).find(|&j| eaves[j]).unwrap_or(0);
            planes = barrel_planes(ring, base, pitches[main], ridge);
        }
        other => {
            return Err(format!(
                "no roof type `{other}`; roof types are {}",
                ROOF_KINDS.join(", ")
            ));
        }
    }
    if planes.is_empty() {
        return Err("this outline has no eave along the ridge direction".into());
    }
    Ok(planes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64) -> Vec<P2> {
        vec![(0.0, 0.0), (x, 0.0), (x, y), (0.0, y)]
    }

    fn top(shell: &Shell) -> f64 {
        shell.points.iter().map(|p| p[2]).fold(f64::MIN, f64::max)
    }

    #[test]
    fn a_gable_is_two_sloped_slabs() {
        let ring = rect(8000.0, 6000.0);
        let planes = gable_planes(&ring, 2700.0, 45.0, (1.0, 0.0));
        assert_eq!(planes.len(), 2);
        let shell = roof_shell(&ring, &planes, 200.0).unwrap();
        assert!(shell.is_closed());
        let slope = 3000.0 * 2f64.sqrt();
        assert!((shell.volume() - 2.0 * 8000.0 * slope * 200.0).abs() < 1.0e-3);
        assert!((top(&shell) - (5700.0 + 200.0 * 2f64.sqrt())).abs() < 1.0e-9);
    }

    #[test]
    fn an_overhang_drops_the_eave() {
        let ring = rect(8000.0, 6000.0);
        let planes = gable_planes(&ring, 2700.0, 45.0, (1.0, 0.0));
        let wide = offset_ring(&ring, 450.0);
        assert!((signed_area(&wide) - 8900.0 * 6900.0).abs() < 1.0e-6);
        let shell = roof_shell(&wide, &planes, 200.0).unwrap();
        let low = shell.points.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
        assert!((low - 2250.0).abs() < 1.0e-9);
        let slope = 3450.0 * 2f64.sqrt();
        assert!((shell.volume() - 2.0 * 8900.0 * slope * 200.0).abs() < 1.0e-3);
    }

    #[test]
    fn all_four_planes_make_a_hip() {
        let ring = rect(8000.0, 6000.0);
        let n = ring.len();
        let planes: Vec<Plane> = (0..n)
            .map(|i| Plane::rising_from(ring[i], ring[(i + 1) % n], 2700.0, 45.0))
            .collect();
        let shell = roof_shell(&ring, &planes, 200.0).unwrap();
        assert!(shell.is_closed());
        assert!((top(&shell) - (5700.0 + 200.0 * 2f64.sqrt())).abs() < 1.0e-9);
        assert!(shell.volume() > 0.0);
    }

    #[test]
    fn every_kind_closes_and_tops_out_sensibly() {
        let ring = rect(8000.0, 6000.0);
        for kind in ROOF_KINDS {
            let planes = roof_planes(kind, &ring, 2700.0, &[35.0; 4], (1.0, 0.0)).unwrap();
            let shell = roof_shell(&offset_ring(&ring, 450.0), &planes, 200.0).unwrap();
            assert!(shell.is_closed(), "{kind}");
            assert!(shell.volume() > 0.0, "{kind}");
            let high = top(&shell);
            assert!(
                high > 2700.0 && high < 2700.0 + 3000.0 * 2.0,
                "{kind} {high}"
            );
        }
    }

    #[test]
    fn a_flat_roof_is_a_slab() {
        let ring = rect(8000.0, 6000.0);
        let planes = roof_planes("flat", &ring, 2700.0, &[0.0; 4], (1.0, 0.0)).unwrap();
        let shell = roof_shell(&ring, &planes, 200.0).unwrap();
        assert!((shell.volume() - 8000.0 * 6000.0 * 200.0).abs() < 1.0e-3);
    }

    #[test]
    fn a_shed_rises_across_the_whole_depth() {
        let ring = rect(8000.0, 6000.0);
        let planes = roof_planes("shed", &ring, 2700.0, &[45.0; 4], (1.0, 0.0)).unwrap();
        let shell = roof_shell(&ring, &planes, 200.0).unwrap();
        assert!((top(&shell) - (2700.0 + 6000.0 + 200.0 * 2f64.sqrt())).abs() < 1.0e-9);
        let slope = 6000.0 * 2f64.sqrt();
        assert!((shell.volume() - 8000.0 * slope * 200.0).abs() < 1.0e-3);
    }

    #[test]
    fn a_barrel_springs_from_the_eaves() {
        let ring = rect(8000.0, 6000.0);
        let planes = roof_planes("barrel", &ring, 2700.0, &[45.0; 4], (1.0, 0.0)).unwrap();
        let r = 3000.0 / 45f64.to_radians().sin();
        let crown = 2700.0 - r * 45f64.to_radians().cos() + r;
        assert!((envelope(&planes, (4000.0, 3000.0)) - crown).abs() < 1.0e-9);
        assert!((envelope(&planes, (4000.0, 0.0)) - 2700.0).abs() < 1.0e-9);
    }

    #[test]
    fn a_hip_has_one_ridge_four_hips_and_four_eaves() {
        let ring = rect(8000.0, 6000.0);
        let planes = roof_planes("hip", &ring, 2700.0, &[45.0; 4], (1.0, 0.0)).unwrap();
        let edges = roof_edges(&ring, &planes, 200.0);
        let count = |kind: &str| edges.iter().filter(|e| e.kind == kind).count();
        assert_eq!((count("ridge"), count("hip"), count("eave")), (1, 4, 4));
        let ridge = edges.iter().find(|e| e.kind == "ridge").unwrap();
        assert!(((ridge.a[0] - ridge.b[0]).abs() - 2000.0).abs() < 1.0e-6);
        let gable = roof_planes("gable", &ring, 2700.0, &[45.0; 4], (1.0, 0.0)).unwrap();
        let edges = roof_edges(&ring, &gable, 200.0);
        let count = |kind: &str| edges.iter().filter(|e| e.kind == kind).count();
        assert_eq!((count("ridge"), count("verge"), count("eave")), (1, 2, 2));
    }

    #[test]
    fn a_gable_end_wall_fills_the_triangle() {
        let ring = rect(8000.0, 6000.0);
        let planes = roof_planes("gable", &ring, 2700.0, &[45.0; 4], (1.0, 0.0)).unwrap();
        let end = vec![(0.0, 0.0), (300.0, 0.0), (300.0, 6000.0), (0.0, 6000.0)];
        let shell = column_shell(&[end], 0.0, 2700.0, &planes).unwrap();
        assert!(shell.is_closed());
        let expected = 300.0 * 6000.0 * 2700.0 + 300.0 * 3000.0 * 3000.0;
        assert!((shell.volume() - expected).abs() < 1.0e-3);
        let halves = vec![
            vec![(0.0, 0.0), (300.0, 0.0), (300.0, 3000.0), (0.0, 3000.0)],
            vec![
                (0.0, 3000.0),
                (300.0, 3000.0),
                (300.0, 6000.0),
                (0.0, 6000.0),
            ],
        ];
        let split = column_shell(&halves, 0.0, 2700.0, &planes).unwrap();
        assert!(split.is_closed());
        assert!((split.volume() - expected).abs() < 1.0e-3);
        let low = vec![
            (4000.0, 2000.0),
            (4100.0, 2000.0),
            (4100.0, 2100.0),
            (4000.0, 2100.0),
        ];
        let lifted = column_shell(&[low], 0.0, 9000.0, &planes).unwrap();
        assert!((lifted.volume() - 100.0 * 100.0 * 9000.0).abs() < 1.0e-3);
    }

    #[test]
    fn a_concave_outline_is_refused() {
        let ring = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (2.0, 2.0), (0.0, 4.0)];
        assert!(roof_shell(&ring, &[Plane::level(0.0)], 1.0).is_err());
    }
}
