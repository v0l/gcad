use super::steel::Curve;
use anyhow::{Result, bail};
use spade::handles::FixedVertexHandle;
use spade::{
    AngleLimit, ConstrainedDelaunayTriangulation, Point2, RefinementParameters, Triangulation,
};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum Toward {
    Along([f64; 2]),
    Radial(f64),
}

#[derive(Clone, Debug)]
pub enum Stuff {
    Air,
    Iron { mu: f64 },
    Steel(Curve),
    Magnet { br: f64, mu: f64, toward: Toward },
    Coil { amps: f64 },
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub name: String,
    pub loops: Vec<Vec<[f64; 2]>>,
    pub stuff: Stuff,
    pub turning: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Gap {
    pub inner: f64,
    pub outer: f64,
    pub nodes: usize,
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub shapes: Vec<Shape>,
    pub size: f64,
    pub outer: f64,
    pub gap: Option<Gap>,
}

#[derive(Clone, Debug)]
pub struct Plane {
    pub points: Vec<[f64; 2]>,
    pub triangles: Vec<[usize; 3]>,
    pub region: Vec<Option<usize>>,
    pub turning: Vec<bool>,
    pub copies: Vec<(usize, usize)>,
    pub ring: Vec<usize>,
    pub fixed: Vec<usize>,
    pub band: Vec<usize>,
    pub gap: Option<Gap>,
}

pub fn inside(p: [f64; 2], loops: &[Vec<[f64; 2]>]) -> bool {
    let mut odd = false;
    for ring in loops {
        let n = ring.len();
        for k in 0..n {
            let (a, b) = (ring[k], ring[(k + 1) % n]);
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                if x > p[0] {
                    odd = !odd;
                }
            }
        }
    }
    odd
}

fn circle(radius: f64, nodes: usize) -> Vec<[f64; 2]> {
    (0..nodes)
        .map(|j| {
            let angle = std::f64::consts::TAU * j as f64 / nodes as f64;
            [radius * angle.cos(), radius * angle.sin()]
        })
        .collect()
}

struct Builder {
    cdt: ConstrainedDelaunayTriangulation<Point2<f64>>,
    snap: f64,
    cells: HashMap<(i64, i64), Vec<FixedVertexHandle>>,
}

impl Builder {
    fn vertex(&mut self, p: [f64; 2]) -> Result<FixedVertexHandle> {
        let key = |v: f64| (v / self.snap).floor() as i64;
        let (kx, ky) = (key(p[0]), key(p[1]));
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &h in self.cells.get(&(kx + dx, ky + dy)).into_iter().flatten() {
                    let q = self.cdt.vertex(h).position();
                    if (q.x - p[0]).hypot(q.y - p[1]) < self.snap {
                        return Ok(h);
                    }
                }
            }
        }
        let handle = self
            .cdt
            .insert(Point2::new(p[0], p[1]))
            .map_err(|e| anyhow::anyhow!("cannot place a point at {p:?}: {e:?}"))?;
        self.cells.entry((kx, ky)).or_default().push(handle);
        Ok(handle)
    }

    fn ring(&mut self, points: &[[f64; 2]], size: f64) -> Result<Vec<FixedVertexHandle>> {
        let mut dense = Vec::new();
        let n = points.len();
        for k in 0..n {
            let (a, b) = (points[k], points[(k + 1) % n]);
            let length = (b[0] - a[0]).hypot(b[1] - a[1]);
            let pieces = (length / size).ceil().max(1.0) as usize;
            for i in 0..pieces {
                let t = i as f64 / pieces as f64;
                dense.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
            }
        }
        let handles: Vec<FixedVertexHandle> = dense
            .iter()
            .map(|&p| self.vertex(p))
            .collect::<Result<_>>()?;
        for k in 0..handles.len() {
            let (a, b) = (handles[k], handles[(k + 1) % handles.len()]);
            if a == b {
                continue;
            }
            if self.cdt.can_add_constraint(a, b) {
                self.cdt.add_constraint(a, b);
            } else {
                self.cdt.add_constraint_and_split(a, b, |p| p);
            }
        }
        Ok(handles)
    }
}

pub fn mesh(layout: &Layout) -> Result<Plane> {
    let size = layout.size;
    if !(size > 0.0) || !(layout.outer > 0.0) {
        bail!("the slice needs a positive element size and air radius");
    }
    let reach = layout
        .shapes
        .iter()
        .flat_map(|s| s.loops.iter().flatten())
        .map(|p| p[0].hypot(p[1]))
        .fold(0.0, f64::max);
    if reach >= layout.outer {
        bail!("the parts reach past the air around them");
    }
    if let Some(gap) = layout.gap {
        for shape in &layout.shapes {
            for p in shape.loops.iter().flatten() {
                let r = p[0].hypot(p[1]);
                if shape.turning && r > gap.inner * (1.0 + 1.0e-9) {
                    bail!(
                        "`{}` turns but reaches {:.3} mm from the axis, past the airgap's inside at {:.3} mm",
                        shape.name,
                        r * 1.0e3,
                        gap.inner * 1.0e3
                    );
                }
                if !shape.turning && r < gap.outer * (1.0 - 1.0e-9) {
                    bail!(
                        "`{}` stands still but comes within {:.3} mm of the axis, inside the airgap's outside at {:.3} mm",
                        shape.name,
                        r * 1.0e3,
                        gap.outer * 1.0e3
                    );
                }
            }
        }
    }
    let mut builder = Builder {
        cdt: ConstrainedDelaunayTriangulation::new(),
        snap: size * 0.2,
        cells: HashMap::new(),
    };
    let outer_nodes =
        ((std::f64::consts::TAU * layout.outer / (6.0 * size)).ceil() as usize).clamp(48, 2000);
    let outer_ring = builder.ring(&circle(layout.outer, outer_nodes), f64::INFINITY)?;
    let mut ring = Vec::new();
    if let Some(gap) = layout.gap {
        let middle = 0.5 * (gap.inner + gap.outer);
        let quarter = 0.25 * (gap.outer - gap.inner);
        builder.snap = builder.snap.min(0.2 * quarter);
        builder.ring(&circle(gap.inner + quarter, gap.nodes), f64::INFINITY)?;
        ring = builder.ring(&circle(middle, gap.nodes), f64::INFINITY)?;
        builder.ring(&circle(gap.outer - quarter, gap.nodes), f64::INFINITY)?;
    }
    for shape in &layout.shapes {
        for points in &shape.loops {
            if points.len() >= 3 {
                builder.ring(points, size)?;
            }
        }
    }
    let mut cdt = builder.cdt;
    let largest = 4.0 * size;
    cdt.refine(
        RefinementParameters::<f64>::new()
            .keep_constraint_edges()
            .with_angle_limit(AngleLimit::from_deg(25.0))
            .with_max_allowed_area(largest * largest * 0.433)
            .with_max_additional_vertices(4_000_000),
    );
    let mut points: Vec<[f64; 2]> = cdt
        .vertices()
        .map(|v| [v.position().x, v.position().y])
        .collect();
    let mut triangles = Vec::new();
    let mut region = Vec::new();
    let mut areas = vec![0.0; layout.shapes.len()];
    for face in cdt.inner_faces() {
        let mut t = face.vertices().map(|v| v.fix().index());
        let [a, b, c] = t.map(|i| points[i]);
        let cross = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        if cross < 0.0 {
            t.swap(1, 2);
        }
        let middle = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
        if middle[0].hypot(middle[1]) > layout.outer {
            continue;
        }
        let owner = layout
            .shapes
            .iter()
            .position(|s| !matches!(s.stuff, Stuff::Air) && inside(middle, &s.loops))
            .or_else(|| layout.shapes.iter().position(|s| inside(middle, &s.loops)));
        if let Some(k) = owner {
            areas[k] += 0.5 * cross.abs();
        }
        triangles.push(t);
        region.push(owner);
    }
    for (shape, area) in layout.shapes.iter().zip(&areas) {
        if *area <= 0.0 {
            bail!("`{}` takes no room in the slice", shape.name);
        }
    }
    let fixed: Vec<usize> = outer_ring.iter().map(|h| h.index()).collect();
    let mut turning = vec![false; points.len()];
    let mut copies = Vec::new();
    let mut band = Vec::new();
    let mut stator_ring = Vec::new();
    if let Some(gap) = layout.gap {
        let middle = 0.5 * (gap.inner + gap.outer);
        let quarter = 0.25 * (gap.outer - gap.inner);
        stator_ring = ring.iter().map(|h| h.index()).collect();
        let index_of: HashMap<usize, usize> = stator_ring
            .iter()
            .enumerate()
            .map(|(j, &n)| (n, j))
            .collect();
        let mut copy_of: HashMap<usize, usize> = HashMap::new();
        for (e, t) in triangles.iter_mut().enumerate() {
            let c = t.map(|i| points[i]);
            let middle_of = [
                (c[0][0] + c[1][0] + c[2][0]) / 3.0,
                (c[0][1] + c[1][1] + c[2][1]) / 3.0,
            ];
            let r = middle_of[0].hypot(middle_of[1]);
            if r > gap.inner + quarter && r < gap.outer - quarter && region[e].is_none() {
                band.push(e);
            }
            if r >= middle {
                continue;
            }
            for v in t.iter_mut() {
                if let Some(&j) = index_of.get(v) {
                    let copy = *copy_of.entry(*v).or_insert_with(|| {
                        points.push(points[*v]);
                        turning.push(true);
                        copies.push((points.len() - 1, j));
                        points.len() - 1
                    });
                    *v = copy;
                } else {
                    turning[*v] = true;
                }
            }
        }
    }
    Ok(Plane {
        points,
        triangles,
        region,
        turning,
        copies,
        ring: stator_ring,
        fixed,
        band,
        gap: layout.gap,
    })
}
