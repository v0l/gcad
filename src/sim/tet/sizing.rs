use super::fixed::HashMap;
use fieldgrid::Bvh;
use monstertruck::modeling::*;

pub struct SizeField {
    pub top: f64,
    pub least: f64,
    grade: f64,
    cell: f64,
    grid: HashMap<[i64; 3], Vec<(Point3, f64)>>,
    reach: i64,
}

impl SizeField {
    pub fn uniform(top: f64) -> SizeField {
        SizeField {
            top,
            least: top,
            grade: 0.3,
            cell: top,
            grid: HashMap::default(),
            reach: 0,
        }
    }

    pub fn from_gaps(
        top: f64,
        least: f64,
        triangles: Vec<[Point3; 3]>,
        curves: &[(Vec<Point3>, [Point3; 2])],
        bends: Vec<(Point3, f64)>,
    ) -> SizeField {
        let arr = |p: Point3| [p.x, p.y, p.z];
        let bvh = Bvh::new(triangles.iter().map(|t| t.map(arr)).collect());
        let samples: Vec<(Point3, f64)> = {
            use rayon::prelude::*;
            (0..triangles.len())
                .into_par_iter()
                .filter_map(|i| {
                    let [a, b, c] = triangles[i];
                    let n = (b - a).cross(c - a);
                    let length = n.magnitude();
                    if length < 1.0e-18 {
                        return None;
                    }
                    let n = n / length;
                    let centre = Point3::from_vec((a.to_vec() + b.to_vec() + c.to_vec()) / 3.0);
                    let tiny = top * 1.0e-6;
                    let gap = bvh
                        .ray(arr(centre), arr(Point3::from_vec(n)), i, tiny)
                        .min(bvh.ray(arr(centre), arr(Point3::from_vec(-n)), i, tiny));
                    let size = (gap * 0.9).max(least);
                    (size < top).then_some((centre, size))
                })
                .collect()
        };
        let mut samples = samples;
        samples.extend(edge_gaps(top, least, curves));
        samples.extend(bends.into_iter().filter(|b| b.1 < top));
        let mut field = SizeField::uniform(top);
        field.least = least;
        field.cell = top / field.grade / 2.0;
        field.reach = 2;
        samples.sort_by(|a, b| a.1.total_cmp(&b.1));
        for (p, s) in samples {
            if field.at(p) <= s {
                continue;
            }
            let key = field.key(p);
            field.grid.entry(key).or_default().push((p, s));
        }
        field
    }

    pub fn is_uniform(&self) -> bool {
        self.grid.is_empty()
    }

    fn key(&self, p: Point3) -> [i64; 3] {
        [p.x, p.y, p.z].map(|v| (v / self.cell).floor() as i64)
    }

    pub fn at(&self, p: Point3) -> f64 {
        if self.grid.is_empty() {
            return self.top;
        }
        let key = self.key(p);
        let mut best = self.top;
        let r = self.reach;
        for dx in -r..=r {
            for dy in -r..=r {
                for dz in -r..=r {
                    if let Some(list) = self.grid.get(&[key[0] + dx, key[1] + dy, key[2] + dz]) {
                        for &(q, s) in list {
                            best = best.min(s + self.grade * (p - q).magnitude());
                        }
                    }
                }
            }
        }
        best
    }
}

fn segment_distance(p: Point3, a: Point3, b: Point3) -> f64 {
    let d = b - a;
    let t = ((p - a).dot(d) / d.magnitude2().max(1.0e-300)).clamp(0.0, 1.0);
    (a + d * t - p).magnitude()
}

fn edge_gaps(top: f64, least: f64, curves: &[(Vec<Point3>, [Point3; 2])]) -> Vec<(Point3, f64)> {
    use rayon::prelude::*;
    let cell = top;
    let key = |p: Point3| [p.x, p.y, p.z].map(|v| (v / cell).floor() as i64);
    let mut grid: HashMap<[i64; 3], Vec<(usize, Point3, Point3)>> = HashMap::default();
    for (c, (line, _)) in curves.iter().enumerate() {
        for pair in line.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let steps = ((b - a).magnitude() / (cell * 0.5)).ceil().max(1.0) as usize;
            let mut keys = super::fixed::HashSet::default();
            for k in 0..=steps {
                keys.insert(key(a + (b - a) * (k as f64 / steps as f64)));
            }
            for k in keys {
                grid.entry(k).or_default().push((c, a, b));
            }
        }
    }
    let close = |x: Point3, y: Point3| (x - y).magnitude() < least * 1.0e-3;
    let touches = |c: usize, d: usize| {
        let (e, f) = (curves[c].1, curves[d].1);
        e.iter().any(|&x| f.iter().any(|&y| close(x, y)))
    };
    curves
        .par_iter()
        .enumerate()
        .flat_map_iter(|(c, (line, _))| {
            let mut out = Vec::new();
            for pair in line.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let steps = ((b - a).magnitude() / (top * 0.25)).ceil().max(1.0) as usize;
                for k in 0..=steps {
                    let p = a + (b - a) * (k as f64 / steps as f64);
                    let home = key(p);
                    let mut gap = f64::INFINITY;
                    for dx in -1..=1 {
                        for dy in -1..=1 {
                            for dz in -1..=1 {
                                let Some(list) =
                                    grid.get(&[home[0] + dx, home[1] + dy, home[2] + dz])
                                else {
                                    continue;
                                };
                                for &(d, x, y) in list {
                                    if d != c && !touches(c, d) {
                                        gap = gap.min(segment_distance(p, x, y));
                                    }
                                }
                            }
                        }
                    }
                    let size = (gap * 0.9).max(least);
                    if size < top {
                        out.push((p, size));
                    }
                }
            }
            out
        })
        .collect()
}
