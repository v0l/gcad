use super::lattice::{Domain, FLUID, MOVING, WALL};
use super::voxel::{Grid, place_surface};
use monstertruck::modeling::*;

pub const SUBCELLS: usize = 2;
pub const COVERED: u32 = 4;

#[derive(Clone, Debug)]
pub struct Rotor {
    pub body: u16,
    pub centre: [f64; 3],
    pub axis: [f64; 3],
    pub omega: f64,
    pub start: [f64; 3],
    pub size: [usize; 3],
    pub solid: Vec<bool>,
    pub cells: Vec<usize>,
}

fn rotate(v: [f64; 3], k: [f64; 3], cos: f64, sin: f64) -> [f64; 3] {
    let cross = [
        k[1] * v[2] - k[2] * v[1],
        k[2] * v[0] - k[0] * v[2],
        k[0] * v[1] - k[1] * v[0],
    ];
    let along = k[0] * v[0] + k[1] * v[1] + k[2] * v[2];
    [0, 1, 2].map(|d| v[d] * cos + cross[d] * sin + k[d] * along * (1.0 - cos))
}

impl Rotor {
    pub fn spacing(&self) -> f64 {
        1.0 / SUBCELLS as f64
    }

    fn occupied(&self, q: [f64; 3]) -> bool {
        let mut at = [0usize; 3];
        for d in 0..3 {
            let i = ((q[d] - self.start[d]) / self.spacing()).round();
            if i < 0.0 || i >= self.size[d] as f64 {
                return false;
            }
            at[d] = i as usize;
        }
        self.solid[at[0] + (at[1] + at[2] * self.size[1]) * self.size[0]]
    }

    pub fn covers(&self, cell: [usize; 3], angle: f64) -> bool {
        let (sin, cos) = (-angle).sin_cos();
        let quarter = 0.5 * self.spacing();
        let mut count = 0;
        for corner in 0..8 {
            let mut p = [0.0; 3];
            for d in 0..3 {
                let sign = if corner >> d & 1 == 1 { 1.0 } else { -1.0 };
                p[d] = cell[d] as f64 + sign * quarter - self.centre[d];
            }
            let q = rotate(p, self.axis, cos, sin);
            if self.occupied([0, 1, 2].map(|d| q[d] + self.centre[d])) {
                count += 1;
            }
        }
        count >= COVERED
    }

    pub fn wall_velocity(&self, at: [f64; 3]) -> [f64; 3] {
        let w = self.axis.map(|v| v * self.omega);
        let r = [0, 1, 2].map(|d| at[d] - self.centre[d]);
        [
            w[1] * r[2] - w[2] * r[1],
            w[2] * r[0] - w[0] * r[2],
            w[0] * r[1] - w[1] * r[0],
        ]
    }

    pub fn bits(&self) -> Vec<u32> {
        let mut bits = vec![0u32; self.solid.len().div_ceil(32).max(1)];
        for (i, &s) in self.solid.iter().enumerate() {
            if s {
                bits[i / 32] |= 1 << (i % 32);
            }
        }
        bits
    }

    pub fn place(&self, domain: &mut Domain, angle: f64) {
        let [nx, ny, _] = domain.size;
        for &cell in &self.cells {
            let at = [cell % nx, (cell / nx) % ny, cell / (nx * ny)];
            if self.covers(at, angle) {
                domain.flags[cell] = MOVING;
                domain.body[cell] = self.body;
            } else {
                domain.flags[cell] = FLUID;
                domain.body[cell] = 0;
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Axle {
    pub centre: Point3,
    pub axis: Vector3,
    pub omega: f64,
}

pub fn rotor(
    domain: &mut Domain,
    grid: &Grid,
    (points, triangles): (&[Point3], &[[usize; 3]]),
    body: u16,
    axle: Axle,
) -> Rotor {
    let Axle {
        centre,
        axis,
        omega,
    } = axle;
    let h = grid.spacing;
    let fine = h / SUBCELLS as f64;
    let in_cells = |p: Point3| [0, 1, 2].map(|d| (p[d] - grid.origin[d]) / h);
    let local: Vec<[f64; 3]> = points.iter().map(|&p| in_cells(p)).collect();
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for p in &local {
        for d in 0..3 {
            low[d] = low[d].min(p[d]);
            high[d] = high[d].max(p[d]);
        }
    }
    let axis = axis.normalize();
    let k = [axis.x, axis.y, axis.z];
    let c = in_cells(centre);
    let mut reach: f64 = 0.0;
    let (mut first, mut last) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in &local {
        let v = [0, 1, 2].map(|d| p[d] - c[d]);
        let along = v[0] * k[0] + v[1] * k[1] + v[2] * k[2];
        let radial = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2] - along * along)
            .max(0.0)
            .sqrt();
        reach = reach.max(radial);
        first = first.min(along);
        last = last.max(along);
    }
    let start = [0, 1, 2].map(|d| (low[d] - 1.0).floor() - 0.5 + 0.5 / SUBCELLS as f64);
    let size =
        [0, 1, 2].map(|d| ((high[d] + 1.0 - start[d]) * SUBCELLS as f64).ceil() as usize + 1);
    let fine_grid = Grid {
        origin: Point3::new(
            grid.origin.x + start[0] * h,
            grid.origin.y + start[1] * h,
            grid.origin.z + start[2] * h,
        ),
        spacing: fine,
        size,
    };
    let mut scratch = Domain::new(size);
    place_surface(&mut scratch, &fine_grid, points, triangles, 1);
    let solid: Vec<bool> = scratch.flags.iter().map(|&f| f == WALL).collect();
    let margin = 2.0;
    let [nx, ny, _] = domain.size;
    let extent = reach + first.abs().max(last.abs()) + margin + 1.0;
    let span = |d: usize| {
        let lo = (c[d] - extent).floor().max(0.0) as usize;
        let hi = ((c[d] + extent).ceil().max(0.0) as usize).min(domain.size[d] - 1);
        lo..hi + 1
    };
    let mut cells = Vec::new();
    for z in span(2) {
        for y in span(1) {
            for x in span(0) {
                let v = [x as f64 - c[0], y as f64 - c[1], z as f64 - c[2]];
                let along = v[0] * k[0] + v[1] * k[1] + v[2] * k[2];
                let radial = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2] - along * along)
                    .max(0.0)
                    .sqrt();
                if radial > reach + margin || along < first - margin || along > last + margin {
                    continue;
                }
                let cell = x + (y + z * ny) * nx;
                if domain.flags[cell] == FLUID {
                    cells.push(cell);
                }
            }
        }
    }
    let rotor = Rotor {
        body,
        centre: c,
        axis: k,
        omega,
        start,
        size,
        solid,
        cells,
    };
    rotor.place(domain, 0.0);
    rotor
}
