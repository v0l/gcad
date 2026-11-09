pub type P2 = (f64, f64);

const EPS: f64 = 1.0e-6;

pub fn add(a: P2, b: P2) -> P2 {
    (a.0 + b.0, a.1 + b.1)
}

pub fn sub(a: P2, b: P2) -> P2 {
    (a.0 - b.0, a.1 - b.1)
}

pub fn scale(a: P2, k: f64) -> P2 {
    (a.0 * k, a.1 * k)
}

pub fn dot(a: P2, b: P2) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

pub fn cross(a: P2, b: P2) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

pub fn length(a: P2) -> f64 {
    dot(a, a).sqrt()
}

pub fn unit(a: P2) -> P2 {
    scale(a, 1.0 / length(a))
}

pub fn left(d: P2) -> P2 {
    (-d.1, d.0)
}

pub fn meet(p: P2, d: P2, q: P2, e: P2) -> Option<P2> {
    let den = cross(d, e);
    if den.abs() < 1.0e-9 {
        return None;
    }
    let t = cross(sub(q, p), e) / den;
    Some(add(p, scale(d, t)))
}

pub fn signed_area(points: &[P2]) -> f64 {
    (0..points.len())
        .map(|i| cross(points[i], points[(i + 1) % points.len()]))
        .sum::<f64>()
        / 2.0
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub point: P2,
    pub dir: P2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Justify {
    Outside,
    Centre,
    Inside,
}

impl Justify {
    pub fn parse(text: &str) -> Option<Justify> {
        match text {
            "outside" => Some(Justify::Outside),
            "centre" | "center" => Some(Justify::Centre),
            "inside" => Some(Justify::Inside),
            _ => None,
        }
    }

    pub fn start(self, thickness: f64) -> f64 {
        match self {
            Justify::Outside => 0.0,
            Justify::Centre => -thickness / 2.0,
            Justify::Inside => -thickness,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub material: String,
    pub thickness: f64,
}

pub fn bands(layers: &[Layer], justify: Justify, offset: f64) -> Vec<(f64, f64)> {
    let total: f64 = layers.iter().map(|l| l.thickness).sum();
    let mut at = justify.start(total) + offset;
    layers
        .iter()
        .map(|l| {
            let band = (at, at + l.thickness);
            at += l.thickness;
            band
        })
        .collect()
}

pub fn arc_points(a: P2, via: P2, b: P2, pieces: usize) -> Option<Vec<P2>> {
    let (ab, ac) = (sub(via, a), sub(b, a));
    let d = 2.0 * cross(ab, ac);
    if d.abs() < 1.0e-9 {
        return None;
    }
    let (l1, l2) = (dot(ab, ab), dot(ac, ac));
    let centre = add(
        a,
        ((ac.1 * l1 - ab.1 * l2) / d, (ab.0 * l2 - ac.0 * l1) / d),
    );
    let r = length(sub(a, centre));
    let angle = |p: P2| (p.1 - centre.1).atan2(p.0 - centre.0);
    let (t0, tv, t1) = (angle(a), angle(via), angle(b));
    let tau = std::f64::consts::TAU;
    let ccw = |from: f64, to: f64| (to - from).rem_euclid(tau);
    let sweep = if ccw(t0, tv) < ccw(t0, t1) {
        ccw(t0, t1)
    } else {
        -ccw(t1, t0)
    };
    Some(
        (0..=pieces)
            .map(|i| {
                let t = t0 + sweep * i as f64 / pieces as f64;
                (centre.0 + r * t.cos(), centre.1 + r * t.sin())
            })
            .collect(),
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pier {
    pub at: f64,
    pub width: f64,
    pub depth: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub points: Vec<P2>,
    pub closed: bool,
}

impl Chain {
    pub fn new(points: Vec<P2>, closed: bool) -> Result<Chain, String> {
        let mut kept: Vec<P2> = Vec::new();
        for p in points {
            if kept.last().is_none_or(|q| length(sub(p, *q)) > EPS) {
                kept.push(p);
            }
        }
        if closed && kept.len() > 1 && length(sub(kept[0], kept[kept.len() - 1])) < EPS {
            kept.pop();
        }
        match (closed, kept.len()) {
            (false, n) if n < 2 => Err("a wall needs two different points".into()),
            (true, n) if n < 3 => Err("a closed wall needs three points".into()),
            _ => Ok(Chain {
                points: kept,
                closed,
            }),
        }
    }

    pub fn segments(&self) -> usize {
        if self.closed {
            self.points.len()
        } else {
            self.points.len() - 1
        }
    }

    pub fn ends(&self, i: usize) -> (P2, P2) {
        (self.points[i], self.points[(i + 1) % self.points.len()])
    }

    pub fn dir(&self, i: usize) -> P2 {
        let (a, b) = self.ends(i);
        unit(sub(b, a))
    }

    pub fn starts(&self) -> Vec<f64> {
        let mut at = 0.0;
        let mut out = vec![0.0];
        for i in 0..self.segments() {
            let (a, b) = self.ends(i);
            at += length(sub(b, a));
            out.push(at);
        }
        out
    }

    pub fn length(&self) -> f64 {
        *self.starts().last().expect("one start")
    }

    pub fn locate(&self, s: f64) -> (usize, P2, P2) {
        let starts = self.starts();
        let last = self.segments() - 1;
        let i = (0..=last)
            .find(|&i| s <= starts[i + 1] + EPS)
            .unwrap_or(last);
        let d = self.dir(i);
        (i, add(self.points[i], scale(d, s - starts[i])), d)
    }

    pub fn offset(&self, d: f64, start: Option<Cut>, end: Option<Cut>) -> Vec<(P2, f64)> {
        let n = self.segments();
        let starts = self.starts();
        let line = |i: usize| {
            let dir = self.dir(i);
            (add(self.points[i], scale(left(dir), d)), dir)
        };
        let corner = |i: usize, before: usize, after: usize| {
            let ((p, a), (q, b)) = (line(before), line(after));
            meet(p, a, q, b).unwrap_or(add(self.points[i], scale(left(b), d)))
        };
        if self.closed {
            return (0..n)
                .map(|i| (corner(i, (i + n - 1) % n, i), starts[i]))
                .collect();
        }
        let cut_on = |i: usize, cut: Option<Cut>, at: P2| {
            let (p, dir) = line(i);
            let point = cut
                .and_then(|c| meet(p, dir, c.point, c.dir))
                .unwrap_or(add(at, scale(left(dir), d)));
            (point, dot(sub(point, self.points[i]), dir) + starts[i])
        };
        let mut out = vec![cut_on(0, start, self.points[0])];
        out.extend((1..n).map(|i| (corner(i, i - 1, i), starts[i])));
        out.push(cut_on(n - 1, end, self.points[n]));
        out
    }

    fn with_piers(&self, mut line: Vec<(P2, f64)>, d: f64, piers: &[Pier]) -> Vec<(P2, f64)> {
        let starts = self.starts();
        let mut sorted = piers.to_vec();
        sorted.sort_by(|a, b| b.at.total_cmp(&a.at));
        for pier in sorted {
            let (i, _, dir) = self.locate(pier.at);
            let n = left(dir);
            let at = |s: f64, depth: f64| {
                (
                    add(
                        self.points[i],
                        add(scale(dir, s - starts[i]), scale(n, d + depth)),
                    ),
                    s,
                )
            };
            let (s0, s1) = (pier.at - pier.width / 2.0, pier.at + pier.width / 2.0);
            let notch = [
                at(s0, 0.0),
                at(s0, pier.depth),
                at(s1, pier.depth),
                at(s1, 0.0),
            ];
            line.splice(i + 1..i + 1, notch);
        }
        line
    }

    pub fn band(
        &self,
        lo: f64,
        hi: f64,
        start: Option<Cut>,
        end: Option<Cut>,
        piers: &[Pier],
    ) -> (Vec<(P2, f64)>, Option<Vec<(P2, f64)>>) {
        let right = self.offset(lo, start, end);
        let left = self.with_piers(self.offset(hi, start, end), hi, piers);
        if self.closed {
            let area = |ring: &[(P2, f64)]| {
                signed_area(&ring.iter().map(|(p, _)| *p).collect::<Vec<_>>()).abs()
            };
            if area(&right) > area(&left) {
                (right, Some(left))
            } else {
                (left, Some(right))
            }
        } else {
            let mut ring = right;
            ring.extend(left.into_iter().rev());
            (ring, None)
        }
    }

    pub fn band_pieces(
        &self,
        lo: f64,
        hi: f64,
        start: Option<Cut>,
        end: Option<Cut>,
    ) -> Vec<Vec<P2>> {
        let right: Vec<P2> = self
            .offset(lo, start, end)
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        let left: Vec<P2> = self
            .offset(hi, start, end)
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        let n = right.len();
        (0..self.segments())
            .map(|i| {
                let j = (i + 1) % n;
                vec![right[i], right[j], left[j], left[i]]
            })
            .collect()
    }

    pub fn project(&self, p: P2) -> Option<(usize, f64, f64)> {
        (0..self.segments())
            .filter_map(|i| {
                let (a, b) = self.ends(i);
                let dir = self.dir(i);
                let t = dot(sub(p, a), dir);
                let span = length(sub(b, a));
                (t >= -EPS && t <= span + EPS).then(|| (i, t, dot(sub(p, a), left(dir))))
            })
            .min_by(|x, y| x.2.abs().total_cmp(&y.2.abs()))
    }
}

#[derive(Clone, Debug)]
pub struct WallLine {
    pub chain: Chain,
    pub lo: f64,
    pub hi: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Join {
    Mitre,
    Butt,
    Tee,
}

impl Join {
    pub fn parse(text: &str) -> Option<Join> {
        match text {
            "mitre" | "miter" => Some(Join::Mitre),
            "butt" => Some(Join::Butt),
            "tee" => Some(Join::Tee),
            _ => None,
        }
    }
}

struct End {
    point: P2,
    away: P2,
    lo: f64,
    hi: f64,
}

fn end_of(wall: &WallLine, at_end: bool) -> End {
    let chain = &wall.chain;
    if at_end {
        let last = chain.segments() - 1;
        End {
            point: chain.points[chain.points.len() - 1],
            away: scale(chain.dir(last), -1.0),
            lo: -wall.hi,
            hi: -wall.lo,
        }
    } else {
        End {
            point: chain.points[0],
            away: chain.dir(0),
            lo: wall.lo,
            hi: wall.hi,
        }
    }
}

fn mitre(me: &End, other: &End) -> Option<Cut> {
    let (nm, no) = (left(me.away), left(other.away));
    let p = me.point;
    let a = meet(
        add(p, scale(nm, me.hi)),
        me.away,
        add(p, scale(no, other.lo)),
        other.away,
    )?;
    let b = meet(
        add(p, scale(nm, me.lo)),
        me.away,
        add(p, scale(no, other.hi)),
        other.away,
    )?;
    (length(sub(b, a)) > EPS).then(|| Cut {
        point: a,
        dir: unit(sub(b, a)),
    })
}

fn open_end_at(wall: &WallLine, p: P2) -> Option<bool> {
    if wall.chain.closed {
        return None;
    }
    [false, true]
        .into_iter()
        .find(|&at_end| length(sub(end_of(wall, at_end).point, p)) < EPS)
}

fn face(me: &End, other: &WallLine, near: bool) -> Option<Cut> {
    let (i, _, offset) = other.chain.project(me.point)?;
    if offset < other.lo - EPS || offset > other.hi + EPS {
        return None;
    }
    let dir = other.chain.dir(i);
    let n = left(dir);
    let body_on_left = dot(me.away, n) > 0.0;
    let f = if body_on_left == near {
        other.hi
    } else {
        other.lo
    };
    Some(Cut {
        point: add(other.chain.points[i], scale(n, f)),
        dir,
    })
}

pub fn meets(walls: &[WallLine], a: usize, b: usize) -> bool {
    let touches = |me: usize, other: usize| {
        !walls[me].chain.closed
            && [false, true].into_iter().any(|at_end| {
                let end = end_of(&walls[me], at_end);
                open_end_at(&walls[other], end.point).is_some()
                    || face(&end, &walls[other], true).is_some()
            })
    };
    touches(a, b) || touches(b, a)
}

pub fn end_cut(
    walls: &[WallLine],
    me: usize,
    at_end: bool,
    forced: &[(usize, usize, Join)],
) -> Option<Cut> {
    let wall = &walls[me];
    if wall.chain.closed {
        return None;
    }
    let end = end_of(wall, at_end);
    let touching = |other: usize| {
        open_end_at(&walls[other], end.point)
            .map(|their| (true, their))
            .or_else(|| face(&end, &walls[other], true).map(|_| (false, false)))
    };
    for &(first, second, how) in forced {
        let (other, mine_first) = match (first == me, second == me) {
            (true, _) => (second, true),
            (_, true) => (first, false),
            _ => continue,
        };
        let Some((at_their_end, their)) = touching(other) else {
            continue;
        };
        let other_end = end_of(&walls[other], their);
        return match (how, mine_first) {
            (Join::Mitre, _) if at_their_end => mitre(&end, &other_end),
            (Join::Mitre, _) | (Join::Tee, true) => face(&end, &walls[other], true),
            (Join::Butt, true) => face(&end, &walls[other], false),
            (Join::Butt, false) => face(&end, &walls[other], true),
            (Join::Tee, false) => None,
        };
    }
    (0..walls.len()).filter(|&o| o != me).find_map(|other| {
        match open_end_at(&walls[other], end.point) {
            Some(their) => mitre(&end, &end_of(&walls[other], their)),
            None => face(&end, &walls[other], true),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(ring: &[(P2, f64)]) -> f64 {
        signed_area(&ring.iter().map(|(p, _)| *p).collect::<Vec<_>>())
    }

    #[test]
    fn an_l_band_has_the_centre_line_area() {
        let chain = Chain::new(vec![(0.0, 0.0), (4000.0, 0.0), (4000.0, 3000.0)], false).unwrap();
        let (ring, hole) = chain.band(-50.0, 50.0, None, None, &[]);
        assert!(hole.is_none());
        assert!((area(&ring) - 7000.0 * 100.0).abs() < 1.0e-6);
    }

    #[test]
    fn a_pier_adds_its_area() {
        let chain = Chain::new(vec![(0.0, 0.0), (4000.0, 0.0)], false).unwrap();
        let pier = Pier {
            at: 2000.0,
            width: 300.0,
            depth: 200.0,
        };
        let (ring, _) = chain.band(-50.0, 50.0, None, None, &[pier]);
        assert!((area(&ring) - (4000.0 * 100.0 + 300.0 * 200.0)).abs() < 1.0e-6);
    }

    #[test]
    fn closed_band_pieces_add_up_to_the_band() {
        let chain = Chain::new(
            vec![(0.0, 0.0), (8000.0, 0.0), (8000.0, 6000.0), (0.0, 6000.0)],
            true,
        )
        .unwrap();
        let total: f64 = chain
            .band_pieces(0.0, 300.0, None, None)
            .iter()
            .map(|p| signed_area(p).abs())
            .sum();
        assert!((total - (8000.0 * 6000.0 - 7400.0 * 5400.0)).abs() < 1.0e-6);
    }

    #[test]
    fn a_corner_mitres_both_walls() {
        let line = |points| WallLine {
            chain: Chain::new(points, false).unwrap(),
            lo: -50.0,
            hi: 50.0,
        };
        let walls = [
            line(vec![(0.0, 0.0), (4000.0, 0.0)]),
            line(vec![(0.0, 0.0), (0.0, 3000.0)]),
        ];
        let cut = end_cut(&walls, 0, false, &[]).unwrap();
        let (ring, _) = walls[0].chain.band(-50.0, 50.0, Some(cut), None, &[]);
        assert!((area(&ring) - 4000.0 * 100.0).abs() < 1.0e-6);
        let butt = end_cut(&walls, 0, false, &[(0, 1, Join::Butt)]).unwrap();
        let (ring, _) = walls[0].chain.band(-50.0, 50.0, Some(butt), None, &[]);
        assert!((area(&ring) - 4050.0 * 100.0).abs() < 1.0e-6);
    }
}
