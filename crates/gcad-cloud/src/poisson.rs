use crate::cloud::{Cloud, P3};
use crate::ops::{mean_spacing, par_map};
use anyhow::{Result, bail};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Default)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
    }

    fn write_u64(&mut self, v: u64) {
        let mixed = (v ^ (v >> 29)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        self.0 = mixed ^ (mixed >> 32);
    }
}

type Map = HashMap<u64, u32, BuildHasherDefault<KeyHasher>>;

const NONE: u32 = u32::MAX;
const PIN: f64 = 4.0;
const MAX_ITERATIONS: usize = 800;
const TOLERANCE: f64 = 1.0e-6;
const BAND: usize = 2;

#[derive(Clone, Copy, Debug)]
pub struct PoissonOptions {
    pub depth: u32,
    pub trim: f64,
}

pub struct Surface {
    pub positions: Vec<P3>,
    pub triangles: Vec<[u32; 3]>,
    pub cell: f64,
}

fn key(c: [i32; 3]) -> u64 {
    ((c[0] as u64 & 0x1f_ffff) << 42) | ((c[1] as u64 & 0x1f_ffff) << 21) | (c[2] as u64 & 0x1f_ffff)
}

const DIRECTIONS: [[i32; 3]; 6] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
];

struct Grid {
    coords: Vec<[i32; 3]>,
    index: Map,
    around: Vec<[u32; 6]>,
}

impl Grid {
    fn lookup(&self, c: [i32; 3]) -> u32 {
        self.index.get(&key(c)).copied().unwrap_or(NONE)
    }

    fn insert(&mut self, c: [i32; 3]) {
        let next = self.coords.len() as u32;
        if *self.index.entry(key(c)).or_insert(next) == next {
            self.coords.push(c);
        }
    }

    fn dilate(&mut self) {
        let before = self.coords.len();
        for n in 0..before {
            let c = self.coords[n];
            for d in DIRECTIONS {
                self.insert([c[0] + d[0], c[1] + d[1], c[2] + d[2]]);
            }
        }
    }

    fn link(&mut self) {
        let around = par_map(self.coords.len(), |n| {
            let c = self.coords[n];
            std::array::from_fn(|k| {
                let d = DIRECTIONS[k];
                self.lookup([c[0] + d[0], c[1] + d[1], c[2] + d[2]])
            })
        });
        self.around = around;
    }
}

struct Stencil {
    nodes: [[i32; 3]; 8],
    weights: [f64; 8],
}

fn stencil(p: P3, origin: P3, cell: f64) -> Stencil {
    let g: [f64; 3] = std::array::from_fn(|k| (p[k] - origin[k]) / cell);
    let base: [i32; 3] = g.map(|v| v.floor() as i32);
    let f: [f64; 3] = std::array::from_fn(|k| g[k] - base[k] as f64);
    let mut nodes = [[0; 3]; 8];
    let mut weights = [0.0; 8];
    for c in 0..8usize {
        let mut w = 1.0;
        for k in 0..3 {
            let up = (c >> k) & 1;
            nodes[c][k] = base[k] + up as i32;
            w *= if up == 1 { f[k] } else { 1.0 - f[k] };
        }
        weights[c] = w;
    }
    Stencil { nodes, weights }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

struct System<'a> {
    grid: &'a Grid,
    points: &'a [([u32; 8], [f64; 8])],
    pin: &'a [f64],
}

impl System<'_> {
    fn apply(&self, x: &[f64]) -> Vec<f64> {
        let mut y = par_map(x.len(), |c| {
            self.grid.around[c]
                .iter()
                .filter(|&&n| n != NONE)
                .map(|&n| x[c] - x[n as usize])
                .sum::<f64>()
        });
        for (p, (nodes, weights)) in self.points.iter().enumerate() {
            let v: f64 = (0..8).map(|k| weights[k] * x[nodes[k] as usize]).sum();
            let s = self.pin[p] * v;
            for k in 0..8 {
                y[nodes[k] as usize] += weights[k] * s;
            }
        }
        y
    }

    fn diagonal(&self) -> Vec<f64> {
        let mut d: Vec<f64> = self
            .grid
            .around
            .iter()
            .map(|a| a.iter().filter(|&&n| n != NONE).count() as f64)
            .collect();
        for (p, (nodes, weights)) in self.points.iter().enumerate() {
            for k in 0..8 {
                d[nodes[k] as usize] += self.pin[p] * weights[k] * weights[k];
            }
        }
        d
    }

    fn solve(&self, b: &[f64]) -> Vec<f64> {
        let n = b.len();
        let diagonal = self.diagonal();
        let mut x = vec![0.0; n];
        let mut r = b.to_vec();
        let mut z: Vec<f64> = r.iter().zip(&diagonal).map(|(r, d)| r / d).collect();
        let mut p = z.clone();
        let mut rz = dot(&r, &z);
        let stop = TOLERANCE * dot(b, b).sqrt().max(1.0e-30);
        for _ in 0..MAX_ITERATIONS {
            let ap = self.apply(&p);
            let alpha = rz / dot(&p, &ap).max(1.0e-300);
            for i in 0..n {
                x[i] += alpha * p[i];
                r[i] -= alpha * ap[i];
            }
            if dot(&r, &r).sqrt() < stop {
                break;
            }
            for i in 0..n {
                z[i] = r[i] / diagonal[i];
            }
            let next = dot(&r, &z);
            let beta = next / rz;
            rz = next;
            for i in 0..n {
                p[i] = z[i] + beta * p[i];
            }
        }
        x
    }
}

pub fn cell_size(cloud: &Cloud, depth: u32) -> Result<(f64, P3)> {
    let Some((lo, hi)) = cloud.bounds() else {
        bail!("there are no points to reconstruct");
    };
    let extent = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max);
    let spacing = mean_spacing(cloud).unwrap_or(extent / 100.0);
    let cell = (extent * 1.1 / (1u64 << depth) as f64).max(2.0 * spacing);
    if !(cell > 0.0) {
        bail!("the points are all in one place");
    }
    Ok((cell, lo))
}

pub fn poisson(cloud: &Cloud, options: PoissonOptions) -> Result<Surface> {
    let Some(normals) = &cloud.normals else {
        bail!("Poisson reconstruction needs oriented normals, run `normals` first");
    };
    if !(1..=12).contains(&options.depth) {
        bail!("`depth` is a whole number from 1 to 12, got {}", options.depth);
    }
    if !(0.0..1.0).contains(&options.trim) {
        bail!("`trim` is a share from 0 up to 1, got {}", options.trim);
    }
    if cloud.len() < 8 {
        bail!("Poisson reconstruction needs at least 8 points, got {}", cloud.len());
    }
    let (cell, lo) = cell_size(cloud, options.depth)?;
    let margin = (BAND + 3) as f64 * cell;
    let origin: P3 = lo.map(|v| v - margin);
    let spacing = mean_spacing(cloud).unwrap_or(cell / 2.0);
    let share = ((2.0 * spacing) / cell).powi(2);

    let stencils: Vec<Stencil> = par_map(cloud.len(), |i| stencil(cloud.positions[i], origin, cell));
    let mut grid = Grid {
        coords: Vec::new(),
        index: Map::default(),
        around: Vec::new(),
    };
    for s in &stencils {
        for node in s.nodes {
            grid.insert(node);
        }
    }
    for _ in 0..BAND {
        grid.dilate();
    }
    grid.link();
    let n = grid.coords.len();

    let points: Vec<([u32; 8], [f64; 8])> = stencils
        .iter()
        .map(|s| (std::array::from_fn(|k| grid.lookup(s.nodes[k])), s.weights))
        .collect();
    let mut field = vec![[0.0f64; 3]; n];
    let mut density = vec![0.0f64; n];
    for (i, (nodes, weights)) in points.iter().enumerate() {
        let normal = normals[i];
        for k in 0..8 {
            let at = nodes[k] as usize;
            density[at] += weights[k];
            for d in 0..3 {
                field[at][d] += weights[k] * share * normal[d] as f64;
            }
        }
    }
    let rhs: Vec<f64> = par_map(n, |c| {
        let mut divergence = 0.0;
        for axis in 0..3 {
            let (lower, upper) = (grid.around[c][2 * axis], grid.around[c][2 * axis + 1]);
            let up = if upper == NONE { 0.0 } else { field[upper as usize][axis] };
            let down = if lower == NONE { 0.0 } else { field[lower as usize][axis] };
            divergence += (up - down) * 0.5;
        }
        -divergence
    });
    let pin = vec![PIN * share; points.len()];
    let system = System {
        grid: &grid,
        points: &points,
        pin: &pin,
    };
    let phi = system.solve(&rhs);
    mesh_of(&grid, &phi, &density, origin, cell, options.trim)
}

const CORNERS: [[i32; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [0, 1, 0],
    [1, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [0, 1, 1],
    [1, 1, 1],
];

const CUBE_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

fn mesh_of(
    grid: &Grid,
    phi: &[f64],
    density: &[f64],
    origin: P3,
    cell: f64,
    trim: f64,
) -> Result<Surface> {
    let mut vertex_of: Map = Map::default();
    let mut positions: Vec<P3> = Vec::new();
    let mut weights: Vec<f64> = Vec::new();
    for (n, &c) in grid.coords.iter().enumerate() {
        let corner: Vec<u32> = CORNERS
            .iter()
            .map(|d| grid.lookup([c[0] + d[0], c[1] + d[1], c[2] + d[2]]))
            .collect();
        if corner.contains(&NONE) {
            continue;
        }
        let values: Vec<f64> = corner.iter().map(|&i| phi[i as usize]).collect();
        let inside = values.iter().filter(|v| **v < 0.0).count();
        if inside == 0 || inside == 8 {
            continue;
        }
        let mut sum = [0.0; 3];
        let mut crossings = 0.0;
        for &(a, b) in &CUBE_EDGES {
            if (values[a] < 0.0) == (values[b] < 0.0) {
                continue;
            }
            let t = values[a] / (values[a] - values[b]);
            for k in 0..3 {
                sum[k] += CORNERS[a][k] as f64 + (CORNERS[b][k] - CORNERS[a][k]) as f64 * t;
            }
            crossings += 1.0;
        }
        let position: P3 =
            std::array::from_fn(|k| origin[k] + cell * (c[k] as f64 + sum[k] / crossings));
        vertex_of.insert(key(c), positions.len() as u32);
        positions.push(position);
        weights.push(corner.iter().map(|&i| density[i as usize]).sum::<f64>() / 8.0);
        let _ = n;
    }

    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for (n, &c) in grid.coords.iter().enumerate() {
        for axis in 0..3 {
            let up = grid.around[n][2 * axis + 1];
            if up == NONE {
                continue;
            }
            let (here, there) = (phi[n] < 0.0, phi[up as usize] < 0.0);
            if here == there {
                continue;
            }
            let (b, d) = ((axis + 1) % 3, (axis + 2) % 3);
            let cells = [(0, 0), (-1, 0), (-1, -1), (0, -1)].map(|(ob, od)| {
                let mut at = c;
                at[b] += ob;
                at[d] += od;
                vertex_of.get(&key(at)).copied().unwrap_or(NONE)
            });
            if cells.contains(&NONE) {
                continue;
            }
            let quad = if here {
                cells
            } else {
                [cells[0], cells[3], cells[2], cells[1]]
            };
            let gap = |a: u32, b: u32| {
                let (p, q) = (positions[a as usize], positions[b as usize]);
                (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f64>()
            };
            if gap(quad[0], quad[2]) <= gap(quad[1], quad[3]) {
                triangles.push([quad[0], quad[1], quad[2]]);
                triangles.push([quad[0], quad[2], quad[3]]);
            } else {
                triangles.push([quad[0], quad[1], quad[3]]);
                triangles.push([quad[1], quad[2], quad[3]]);
            }
        }
    }
    if triangles.is_empty() {
        bail!("the points did not make a surface, check the normals and the depth");
    }

    let mut sorted = weights.clone();
    sorted.sort_by(f64::total_cmp);
    let floor = trim * sorted[sorted.len() / 2];
    if trim > 0.0 {
        triangles.retain(|t| t.iter().all(|&i| weights[i as usize] >= floor));
    }
    let mut remap = vec![NONE; positions.len()];
    let mut kept: Vec<P3> = Vec::new();
    for t in triangles.iter_mut() {
        for i in t.iter_mut() {
            if remap[*i as usize] == NONE {
                remap[*i as usize] = kept.len() as u32;
                kept.push(positions[*i as usize]);
            }
            *i = remap[*i as usize];
        }
    }
    if triangles.is_empty() {
        bail!("trimming removed the whole surface, lower `trim`");
    }
    let volume: f64 = triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| kept[i as usize]);
            let cross = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]
        })
        .sum();
    if volume < 0.0 {
        triangles.iter_mut().for_each(|t| t.swap(1, 2));
    }
    Ok(Surface {
        positions: kept,
        triangles,
        cell,
    })
}
