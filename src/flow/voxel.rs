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
    let spans = [xs.clone(), ys.clone(), zs.clone()];
    let mut columns: Vec<Vec<usize>> = Vec::new();
    let mut thin: Vec<usize> = Vec::new();
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let (us, vs) = (spans[u].clone(), spans[v].clone());
        let start = grid.origin[axis] - 2.0 * h;
        let mut direction = Vector3::zero();
        direction[axis] = 1.0;
        let mut nudge = Vector3::zero();
        nudge[u] = h * 1.234_567e-6;
        nudge[v] = h * 2.345_678e-6;
        let wide = us.len();
        let filled: Vec<(Vec<usize>, Vec<usize>)> = (0..us.len() * vs.len())
            .into_par_iter()
            .map(|column| {
                let mut cell = [0usize; 3];
                cell[u] = us.start + column % wide;
                cell[v] = vs.start + column / wide;
                let mut from = grid.centre(cell[0], cell[1], cell[2]) + nudge;
                from[axis] = start;
                let mut crossings = Vec::new();
                let mut last = 0.0;
                loop {
                    let t = bvh.ray(from, direction, usize::MAX, last + h * 1.0e-9);
                    if !t.is_finite() {
                        break;
                    }
                    crossings.push(start + t);
                    last = t;
                }
                let n = grid.size[axis] as f64;
                let mut inside = Vec::new();
                let mut skins = Vec::new();
                let mut index = |k: usize| {
                    let mut at = cell;
                    at[axis] = k;
                    domain_index(grid, at[0], at[1], at[2])
                };
                for pair in crossings.chunks(2) {
                    let [a, b] = [pair[0], *pair.get(1).unwrap_or(&f64::INFINITY)];
                    let (a, b) = ((a - grid.origin[axis]) / h, (b - grid.origin[axis]) / h);
                    let (first, last) = (a.ceil().max(0.0), b.floor().min(n - 1.0));
                    if axis == 0 && first <= last {
                        inside.extend((first as usize..=last as usize).map(&mut index));
                    } else if first > last && b.is_finite() {
                        let middle = (0.5 * (a + b)).round();
                        if (0.0..n).contains(&middle) {
                            skins.push(index(middle as usize));
                        }
                    }
                }
                (inside, skins)
            })
            .collect();
        for (inside, skins) in filled {
            columns.push(inside);
            thin.extend(skins);
        }
    }
    let mut solid_cells = 0;
    let mut cored = std::collections::HashSet::new();
    for cells in &columns {
        cored.extend(cells.iter().copied());
    }
    let thin: std::collections::HashSet<usize> =
        thin.into_iter().filter(|c| !cored.contains(c)).collect();
    columns.push(thin.iter().copied().collect());
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
                if flags[s] != WALL || bodies[s] != body || thin.contains(&s) {
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
