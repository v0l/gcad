use super::{Mesh, P3, add, dot, length, scale, sub};
use std::collections::HashMap;

pub struct Grid {
    cell: f64,
    cells: HashMap<[i64; 3], Vec<u32>>,
}

impl Grid {
    pub fn new(points: &[P3], cell: f64) -> Grid {
        let cell = cell.max(1.0e-9);
        let mut cells: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
        for (i, &p) in points.iter().enumerate() {
            cells.entry(key(p, cell)).or_default().push(i as u32);
        }
        Grid { cell, cells }
    }

    pub fn near(&self, points: &[P3], p: P3, r: f64, mut f: impl FnMut(u32, f64)) {
        let lo = key(sub(p, [r; 3]), self.cell);
        let hi = key(super::add(p, [r; 3]), self.cell);
        let r2 = r * r;
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                for z in lo[2]..=hi[2] {
                    let Some(list) = self.cells.get(&[x, y, z]) else {
                        continue;
                    };
                    for &i in list {
                        let d = sub(points[i as usize], p);
                        let d2 = dot(d, d);
                        if d2 <= r2 {
                            f(i, d2.sqrt());
                        }
                    }
                }
            }
        }
    }

    pub fn within(&self, points: &[P3], p: P3, r: f64) -> Vec<(u32, f64)> {
        let mut out = Vec::new();
        self.near(points, p, r, |i, d| out.push((i, d)));
        out
    }

    pub fn nearest(&self, points: &[P3], p: P3) -> Option<(u32, f64)> {
        if self.cells.is_empty() {
            return None;
        }
        let mut r = self.cell;
        loop {
            let mut best: Option<(u32, f64)> = None;
            self.near(points, p, r, |i, d| {
                if best.is_none_or(|(_, b)| d < b) {
                    best = Some((i, d));
                }
            });
            if best.is_some() {
                return best;
            }
            r *= 2.0;
            if r > 1.0e12 {
                return None;
            }
        }
    }
}

fn key(p: P3, cell: f64) -> [i64; 3] {
    p.map(|v| (v / cell).floor() as i64)
}

pub fn closest_on_triangle(p: P3, a: P3, b: P3, c: P3) -> P3 {
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
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return add(a, scale(ab, d1 / (d1 - d3)));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return add(a, scale(ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return add(b, scale(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let denom = 1.0 / (va + vb + vc);
    add(a, add(scale(ab, vb * denom), scale(ac, vc * denom)))
}

pub struct Surface<'a> {
    positions: &'a [P3],
    triangles: &'a [[u32; 3]],
    cell: f64,
    cells: HashMap<[i64; 3], Vec<u32>>,
}

pub struct Hit {
    pub point: P3,
    pub triangle: u32,
    pub distance: f64,
}

impl<'a> Surface<'a> {
    pub fn new(mesh: &'a Mesh) -> Surface<'a> {
        let (sum, count) = mesh.triangles.iter().take(5000).fold((0.0, 0.0), |(s, n), t| {
            let [a, b, _] = mesh.corners(*t);
            (s + length(sub(a, b)), n + 1.0)
        });
        let cell = (2.0 * sum / f64::max(count, 1.0)).max(1.0e-9);
        let mut cells: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
        for (f, &t) in mesh.triangles.iter().enumerate() {
            let [a, b, c] = mesh.corners(t);
            let lo = key(std::array::from_fn(|k| a[k].min(b[k]).min(c[k])), cell);
            let hi = key(std::array::from_fn(|k| a[k].max(b[k]).max(c[k])), cell);
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        cells.entry([x, y, z]).or_default().push(f as u32);
                    }
                }
            }
        }
        Surface {
            positions: &mesh.positions,
            triangles: &mesh.triangles,
            cell,
            cells,
        }
    }

    pub fn closest(&self, p: P3) -> Option<Hit> {
        if self.cells.is_empty() {
            return None;
        }
        let mut r = self.cell;
        let mut seen = std::collections::HashSet::new();
        let mut best: Option<Hit> = None;
        loop {
            let lo = key(sub(p, [r; 3]), self.cell);
            let hi = key(add(p, [r; 3]), self.cell);
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        let Some(list) = self.cells.get(&[x, y, z]) else {
                            continue;
                        };
                        for &f in list {
                            if !seen.insert(f) {
                                continue;
                            }
                            let [a, b, c] = self.triangles[f as usize].map(|i| self.positions[i as usize]);
                            let q = closest_on_triangle(p, a, b, c);
                            let d = length(sub(q, p));
                            if best.as_ref().is_none_or(|h| d < h.distance) {
                                best = Some(Hit {
                                    point: q,
                                    triangle: f,
                                    distance: d,
                                });
                            }
                        }
                    }
                }
            }
            if let Some(hit) = &best
                && hit.distance <= r
            {
                return best;
            }
            r *= 2.0;
            if r > 1.0e9 {
                return best;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_points_within_a_radius_and_the_nearest() {
        let points: Vec<P3> = (0..10).map(|i| [i as f64, 0.0, 0.0]).collect();
        let grid = Grid::new(&points, 0.7);
        let mut found: Vec<u32> = grid
            .within(&points, [4.2, 0.0, 0.0], 1.5)
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        found.sort();
        assert_eq!(found, vec![3, 4, 5]);
        assert_eq!(grid.nearest(&points, [20.0, 1.0, 0.0]).unwrap().0, 9);
    }

    #[test]
    fn closest_point_on_a_tetrahedron() {
        let mesh = crate::mesh::read_obj(
            "v 0 0 0\nv 10 0 0\nv 0 10 0\nv 0 0 10\nf 1 3 2\nf 1 2 4\nf 1 4 3\nf 2 3 4\n",
        )
        .unwrap();
        let surface = Surface::new(&mesh);
        let hit = surface.closest([2.0, 3.0, -4.0]).unwrap();
        assert!((hit.distance - 4.0).abs() < 1.0e-12);
        assert_eq!(hit.point, [2.0, 3.0, 0.0]);
        let hit = surface.closest([12.0, -1.0, 0.0]).unwrap();
        assert!((hit.distance - 5.0f64.sqrt()).abs() < 1.0e-12, "{}", hit.distance);
    }
}
