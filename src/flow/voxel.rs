use super::lattice::{C, Domain, FLUID, WALL};
use crate::geometry;
use monstertruck::modeling::*;
use sims_field::Bvh;
use sims_field::bvh::Point;
use sims_field::raster::fill;

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
    let corners: Vec<Point> = points.iter().map(|p| [p.x, p.y, p.z]).collect();
    let bvh = Bvh::indexed(&corners, triangles);
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
    let samples: [Vec<f64>; 3] = std::array::from_fn(|a| {
        (0..grid.size[a])
            .map(|k| grid.origin[a] + k as f64 * h)
            .collect()
    });
    let filled = fill(
        &bvh,
        [&samples[0], &samples[1], &samples[2]],
        |[x, y, z]| domain_index(grid, x, y, z),
    );
    let thin = filled.thin;
    let mut solid_cells = 0;
    for cells in [&filled.inside, &thin] {
        for &cell in cells {
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
            let thin = &thin;
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
                if flags[s] != WALL || bodies[s] != body || thin.binary_search(&s).is_ok() {
                    return None;
                }
                let step = [-c[0] as f64 * h, -c[1] as f64 * h, -c[2] as f64 * h];
                let t = bvh.ray([centre.x, centre.y, centre.z], step, usize::MAX, 0.0);
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
