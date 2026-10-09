use super::fixed::HashMap;
use monstertruck::modeling::*;

struct Node {
    low: Point3,
    high: Point3,
    start: usize,
    count: usize,
    children: Option<(usize, usize)>,
}

pub struct Bvh {
    triangles: Vec<[Point3; 3]>,
    order: Vec<usize>,
    nodes: Vec<Node>,
}

fn bounds_of(triangles: &[[Point3; 3]], items: &[usize]) -> (Point3, Point3) {
    let mut low = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut high = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &i in items {
        for p in triangles[i] {
            low = Point3::new(low.x.min(p.x), low.y.min(p.y), low.z.min(p.z));
            high = Point3::new(high.x.max(p.x), high.y.max(p.y), high.z.max(p.z));
        }
    }
    (low, high)
}

impl Bvh {
    pub fn new(triangles: Vec<[Point3; 3]>) -> Bvh {
        let mut bvh = Bvh {
            order: (0..triangles.len()).collect(),
            triangles,
            nodes: Vec::new(),
        };
        if !bvh.order.is_empty() {
            bvh.build(0, bvh.order.len());
        }
        bvh
    }

    fn build(&mut self, start: usize, end: usize) -> usize {
        let (low, high) = bounds_of(&self.triangles, &self.order[start..end]);
        let index = self.nodes.len();
        self.nodes.push(Node {
            low,
            high,
            start,
            count: end - start,
            children: None,
        });
        if end - start <= 4 {
            return index;
        }
        let extent = high - low;
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };
        let triangles = &self.triangles;
        let centre = |i: usize| {
            let [a, b, c] = triangles[i];
            (a[axis] + b[axis] + c[axis]) / 3.0
        };
        let middle = (start + end) / 2;
        self.order[start..end]
            .select_nth_unstable_by(middle - start, |&a, &b| centre(a).total_cmp(&centre(b)));
        let left = self.build(start, middle);
        let right = self.build(middle, end);
        self.nodes[index].children = Some((left, right));
        index
    }

    fn slab(node: &Node, origin: Point3, inverse: Vector3, limit: f64) -> bool {
        let mut near: f64 = 0.0;
        let mut far = limit;
        for k in 0..3 {
            let t1 = (node.low[k] - origin[k]) * inverse[k];
            let t2 = (node.high[k] - origin[k]) * inverse[k];
            let (a, b) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
            near = near.max(a);
            far = far.min(b);
            if near > far {
                return false;
            }
        }
        true
    }

    pub fn ray(&self, origin: Point3, direction: Vector3, skip: usize, least: f64) -> f64 {
        let mut best = f64::INFINITY;
        if self.nodes.is_empty() {
            return best;
        }
        let inverse = Vector3::new(1.0 / direction.x, 1.0 / direction.y, 1.0 / direction.z);
        let mut stack = vec![0];
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n];
            if !Self::slab(node, origin, inverse, best) {
                continue;
            }
            match node.children {
                Some((l, r)) => {
                    stack.push(l);
                    stack.push(r);
                }
                None => {
                    for &i in &self.order[node.start..node.start + node.count] {
                        if i == skip {
                            continue;
                        }
                        if let Some(t) = hit(self.triangles[i], origin, direction)
                            && t > least
                            && t < best
                        {
                            best = t;
                        }
                    }
                }
            }
        }
        best
    }
}

fn hit([a, b, c]: [Point3; 3], origin: Point3, direction: Vector3) -> Option<f64> {
    let (e1, e2) = (b - a, c - a);
    let p = direction.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1.0e-18 {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - a;
    let u = s.dot(p) * inv;
    if !(-1.0e-9..=1.0 + 1.0e-9).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = direction.dot(q) * inv;
    if v < -1.0e-9 || u + v > 1.0 + 1.0e-9 {
        return None;
    }
    Some(e2.dot(q) * inv)
}

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
    ) -> SizeField {
        let bvh = Bvh::new(triangles);
        let samples: Vec<(Point3, f64)> = {
            use rayon::prelude::*;
            (0..bvh.triangles.len())
                .into_par_iter()
                .filter_map(|i| {
                    let [a, b, c] = bvh.triangles[i];
                    let n = (b - a).cross(c - a);
                    let length = n.magnitude();
                    if length < 1.0e-18 {
                        return None;
                    }
                    let n = n / length;
                    let centre = Point3::from_vec((a.to_vec() + b.to_vec() + c.to_vec()) / 3.0);
                    let tiny = top * 1.0e-6;
                    let gap = bvh
                        .ray(centre, n, i, tiny)
                        .min(bvh.ray(centre, -n, i, tiny));
                    let size = (gap * 0.9).max(least);
                    (size < top).then_some((centre, size))
                })
                .collect()
        };
        let mut samples = samples;
        samples.extend(edge_gaps(top, least, curves));
        let mut field = SizeField::uniform(top);
        field.least = least;
        field.cell = top / field.grade / 2.0;
        field.reach = 2;
        for (p, s) in samples {
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
