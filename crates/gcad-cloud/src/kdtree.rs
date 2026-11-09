use crate::cloud::P3;

const LEAF: usize = 12;

#[derive(Clone, Debug)]
pub struct KdTree {
    points: Vec<P3>,
    order: Vec<u32>,
    nodes: Vec<Node>,
}

#[derive(Clone, Debug)]
enum Node {
    Leaf { from: u32, to: u32 },
    Split { axis: u8, at: f64, left: u32, right: u32 },
}

fn distance_squared(a: P3, b: P3) -> f64 {
    (0..3).map(|k| (a[k] - b[k]) * (a[k] - b[k])).sum()
}

impl KdTree {
    pub fn build(points: &[P3]) -> KdTree {
        let mut tree = KdTree {
            points: points.to_vec(),
            order: (0..points.len() as u32).collect(),
            nodes: Vec::new(),
        };
        if !points.is_empty() {
            let mut order = std::mem::take(&mut tree.order);
            tree.grow(&mut order, 0, points.len());
            tree.order = order;
        }
        tree
    }

    fn grow(&mut self, order: &mut [u32], from: usize, to: usize) -> u32 {
        let id = self.nodes.len() as u32;
        if to - from <= LEAF {
            self.nodes.push(Node::Leaf {
                from: from as u32,
                to: to as u32,
            });
            return id;
        }
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for &i in &order[from..to] {
            let p = self.points[i as usize];
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let axis = (0..3)
            .max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b])))
            .unwrap_or(0);
        let middle = (from + to) / 2;
        let points = &self.points;
        order[from..to].select_nth_unstable_by(middle - from, |&a, &b| {
            points[a as usize][axis].total_cmp(&points[b as usize][axis])
        });
        let at = self.points[order[middle] as usize][axis];
        self.nodes.push(Node::Leaf { from: 0, to: 0 });
        let left = self.grow(order, from, middle);
        let right = self.grow(order, middle, to);
        self.nodes[id as usize] = Node::Split {
            axis: axis as u8,
            at,
            left,
            right,
        };
        id
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn nearest(&self, query: P3, k: usize) -> Vec<(usize, f64)> {
        if self.is_empty() || k == 0 {
            return Vec::new();
        }
        let mut best: Vec<(f64, u32)> = Vec::with_capacity(k + 1);
        self.search_k(0, query, k, &mut best);
        best.into_iter()
            .map(|(d, i)| (i as usize, d.sqrt()))
            .collect()
    }

    fn search_k(&self, node: u32, query: P3, k: usize, best: &mut Vec<(f64, u32)>) {
        match &self.nodes[node as usize] {
            Node::Leaf { from, to } => {
                for &i in &self.order[*from as usize..*to as usize] {
                    let d = distance_squared(query, self.points[i as usize]);
                    if best.len() < k || d < best[best.len() - 1].0 {
                        let at = best.partition_point(|&(e, _)| e <= d);
                        best.insert(at, (d, i));
                        best.truncate(k);
                    }
                }
            }
            Node::Split {
                axis,
                at,
                left,
                right,
            } => {
                let side = query[*axis as usize] - at;
                let (near, far) = if side < 0.0 {
                    (*left, *right)
                } else {
                    (*right, *left)
                };
                self.search_k(near, query, k, best);
                if best.len() < k || side * side < best[best.len() - 1].0 {
                    self.search_k(far, query, k, best);
                }
            }
        }
    }

    pub fn within(&self, query: P3, radius: f64) -> Vec<usize> {
        let mut out = Vec::new();
        if !self.is_empty() {
            self.search_r(0, query, radius * radius, &mut out);
        }
        out
    }

    fn search_r(&self, node: u32, query: P3, r2: f64, out: &mut Vec<usize>) {
        match &self.nodes[node as usize] {
            Node::Leaf { from, to } => {
                for &i in &self.order[*from as usize..*to as usize] {
                    if distance_squared(query, self.points[i as usize]) <= r2 {
                        out.push(i as usize);
                    }
                }
            }
            Node::Split {
                axis,
                at,
                left,
                right,
            } => {
                let side = query[*axis as usize] - at;
                if side < 0.0 || side * side <= r2 {
                    self.search_r(*left, query, r2, out);
                }
                if side >= 0.0 || side * side <= r2 {
                    self.search_r(*right, query, r2, out);
                }
            }
        }
    }
}
