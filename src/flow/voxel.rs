use super::lattice::{C, Domain, FLUID, WALL};
use crate::geometry;
use crate::sim::tet::sizing::Bvh;
use monstertruck::modeling::*;

#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub origin: Point3,
    pub spacing: f64,
    pub size: [usize; 3],
}

impl Grid {
    pub fn centre(&self, x: usize, y: usize, z: usize) -> Point3 {
        self.origin + Vector3::new(x as f64, y as f64, z as f64) * self.spacing
    }
}

pub fn surface(solid: &Solid, spacing: f64) -> (Vec<Point3>, Vec<[usize; 3]>) {
    let tolerance = (spacing * 0.05).min(geometry::mesh_tolerance(solid));
    geometry::welded(&geometry::mesh(solid, tolerance))
}

pub fn place(domain: &mut Domain, grid: &Grid, solid: &Solid, body: u16) -> usize {
    let (points, triangles) = surface(solid, grid.spacing);
    place_surface(domain, grid, &points, &triangles, body)
}

pub fn place_surface(
    domain: &mut Domain,
    grid: &Grid,
    points: &[Point3],
    triangles: &[[usize; 3]],
    body: u16,
) -> usize {
    use rayon::prelude::*;
    let triangles: Vec<[Point3; 3]> = triangles.iter().map(|t| t.map(|v| points[v])).collect();
    let bvh = Bvh::new(triangles);
    let [nx, ny, nz] = grid.size;
    let h = grid.spacing;
    let span = |axis: usize, n: usize| {
        let along = |p: &Point3| (p[axis] - grid.origin[axis]) / h;
        let low = points.iter().map(along).fold(f64::INFINITY, f64::min);
        let high = points.iter().map(along).fold(f64::NEG_INFINITY, f64::max);
        let first = (low.floor() - 2.0).max(0.0) as usize;
        let last = ((high.ceil() + 2.0).max(0.0) as usize).min(n - 1);
        first..last + 1
    };
    let (xs, ys, zs) = (span(0, nx), span(1, ny), span(2, nz));
    if points.is_empty() || xs.is_empty() || ys.is_empty() || zs.is_empty() {
        return 0;
    }
    let start = grid.origin.x - 2.0 * h;
    let nudge = Vector3::new(0.0, h * 1.234_567e-6, h * 2.345_678e-6);
    let wide = ys.len();
    let columns: Vec<Vec<usize>> = (0..ys.len() * zs.len())
        .into_par_iter()
        .map(|column| {
            let (y, z) = (ys.start + column % wide, zs.start + column / wide);
            let from = Point3::new(start, grid.centre(0, y, z).y, grid.centre(0, y, z).z) + nudge;
            let mut crossings = Vec::new();
            let mut last = 0.0;
            loop {
                let t = bvh.ray(from, Vector3::unit_x(), usize::MAX, last + h * 1.0e-9);
                if !t.is_finite() {
                    break;
                }
                crossings.push(start + t);
                last = t;
            }
            let mut inside = Vec::new();
            for pair in crossings.chunks(2) {
                let [a, b] = [pair[0], *pair.get(1).unwrap_or(&f64::INFINITY)];
                let first = ((a - grid.origin.x) / h).ceil().max(0.0) as usize;
                let last = (((b - grid.origin.x) / h).floor()).min(nx as f64 - 1.0);
                if last < 0.0 {
                    continue;
                }
                inside.extend((first..=last as usize).map(|x| domain_index(grid, x, y, z)));
            }
            inside
        })
        .collect();
    let mut solid_cells = 0;
    for cells in columns {
        for cell in cells {
            if domain.flags[cell] == FLUID {
                domain.flags[cell] = WALL;
                domain.body[cell] = body;
                solid_cells += 1;
            }
        }
    }
    let (long, wide) = (xs.len(), ys.len());
    let cuts: Vec<(usize, usize, f64)> = (0..xs.len() * ys.len() * zs.len())
        .into_par_iter()
        .map(|k| {
            let (x, y, z) = (
                xs.start + k % long,
                ys.start + (k / long) % wide,
                zs.start + k / (long * wide),
            );
            (domain_index(grid, x, y, z), x, y, z)
        })
        .filter(|&(cell, ..)| domain.flags[cell] == FLUID)
        .flat_map_iter(|(cell, x, y, z)| {
            let centre = grid.centre(x, y, z);
            let flags = &domain.flags;
            let bodies = &domain.body;
            let bvh = &bvh;
            C.iter().enumerate().skip(1).filter_map(move |(j, c)| {
                let (sx, sy, sz) = (
                    x as i64 - c[0] as i64,
                    y as i64 - c[1] as i64,
                    z as i64 - c[2] as i64,
                );
                if sx < 0
                    || sy < 0
                    || sz < 0
                    || sx >= nx as i64
                    || sy >= ny as i64
                    || sz >= nz as i64
                {
                    return None;
                }
                let s = domain_index(grid, sx as usize, sy as usize, sz as usize);
                if flags[s] != WALL || bodies[s] != body {
                    return None;
                }
                let step = Vector3::new(-c[0] as f64, -c[1] as f64, -c[2] as f64) * h;
                let t = bvh.ray(centre, step, usize::MAX, 0.0);
                Some((
                    cell,
                    j,
                    if t.is_finite() {
                        t.clamp(0.0, 1.0)
                    } else {
                        0.5
                    },
                ))
            })
        })
        .collect();
    domain.cuts.extend(cuts);
    solid_cells
}

fn domain_index(grid: &Grid, x: usize, y: usize, z: usize) -> usize {
    x + (y + z * grid.size[1]) * grid.size[0]
}
