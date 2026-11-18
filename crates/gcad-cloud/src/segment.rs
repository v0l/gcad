use crate::cloud::{Cloud, P3};
use crate::kdtree::KdTree;
use crate::ops::{dot, fit_plane, par_map, sub};
use anyhow::{Result, bail};

pub fn cluster(cloud: &Cloud, dist: f64, min: usize) -> Result<Vec<Vec<usize>>> {
    if !(dist > 0.0) {
        bail!("`dist` must be positive, got {dist}");
    }
    let tree = KdTree::build(&cloud.positions);
    let mut seen = vec![false; cloud.len()];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for start in 0..cloud.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut group = vec![start];
        let mut at = 0;
        while at < group.len() {
            for j in tree.within(cloud.positions[group[at]], dist) {
                if !seen[j] {
                    seen[j] = true;
                    group.push(j);
                }
            }
            at += 1;
        }
        if group.len() >= min {
            groups.push(group);
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    Ok(groups)
}

pub fn grow(
    cloud: &Cloud,
    angle: f64,
    curvature: f64,
    k: usize,
    min: usize,
) -> Result<Vec<Vec<usize>>> {
    let Some(normals) = &cloud.normals else {
        bail!("`grow` needs normals; run `normals scan` first");
    };
    if k < 3 {
        bail!("`k` must be at least 3, got {k}");
    }
    let tree = KdTree::build(&cloud.positions);
    let near: Vec<Vec<usize>> = par_map(cloud.len(), |i| {
        tree.nearest(cloud.positions[i], k + 1)
            .into_iter()
            .map(|(j, _)| j)
            .filter(|&j| j != i)
            .collect()
    });
    let flat: Vec<f64> = par_map(cloud.len(), |i| {
        let mut points: Vec<P3> = near[i].iter().map(|&j| cloud.positions[j]).collect();
        points.push(cloud.positions[i]);
        fit_plane(&points).variation
    });
    let cos = angle.to_radians().cos();
    let normal = |i: usize| normals[i].map(|v| v as f64);
    let mut order: Vec<usize> = (0..cloud.len()).collect();
    order.sort_by(|&a, &b| flat[a].total_cmp(&flat[b]));
    let mut seen = vec![false; cloud.len()];
    let mut groups = Vec::new();
    for seed in order {
        if seen[seed] {
            continue;
        }
        seen[seed] = true;
        let mut group = vec![seed];
        let mut queue = vec![seed];
        while let Some(current) = queue.pop() {
            for &j in &near[current] {
                if seen[j] || dot(normal(current), normal(j)) < cos {
                    continue;
                }
                seen[j] = true;
                group.push(j);
                if flat[j] <= curvature {
                    queue.push(j);
                }
            }
        }
        if group.len() >= min {
            groups.push(group);
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    Ok(groups)
}

pub fn slab(cloud: &Cloud, origin: P3, x: P3, y: P3, normal: P3, thickness: f64) -> Vec<[f64; 2]> {
    let half = thickness / 2.0;
    cloud
        .positions
        .iter()
        .filter_map(|p| {
            let d = sub(*p, origin);
            (dot(d, normal).abs() <= half).then(|| [dot(d, x), dot(d, y)])
        })
        .collect()
}

fn turn(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

fn hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    sorted.dedup();
    if sorted.len() < 3 {
        return sorted;
    }
    let mut chain: Vec<[f64; 2]> = Vec::new();
    for pass in 0..2 {
        let floor = chain.len() + 1;
        let order: Vec<[f64; 2]> = if pass == 0 {
            sorted.clone()
        } else {
            sorted.iter().rev().copied().collect()
        };
        for p in order {
            while chain.len() >= floor.max(2) && turn(chain[chain.len() - 2], chain[chain.len() - 1], p) <= 0.0 {
                chain.pop();
            }
            chain.push(p);
        }
        chain.pop();
    }
    chain
}

fn deviation(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx.hypot(dy);
    if length < 1.0e-12 {
        return (p[0] - a[0]).hypot(p[1] - a[1]);
    }
    ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / length
}

fn simplify(points: &[[f64; 2]], tol: f64, keep: &mut Vec<[f64; 2]>) {
    let (first, last) = (points[0], points[points.len() - 1]);
    let far = (1..points.len().saturating_sub(1))
        .map(|i| (i, deviation(points[i], first, last)))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    match far {
        Some((i, d)) if d > tol => {
            simplify(&points[..=i], tol, keep);
            simplify(&points[i..], tol, keep);
        }
        _ => keep.push(first),
    }
}

pub fn outline(points: &[[f64; 2]], tol: f64) -> Result<Vec<[f64; 2]>> {
    let ordered = hull(points);
    if ordered.len() < 3 {
        bail!("the slab holds too few points for a profile ({} on the outline)", ordered.len());
    }
    let first = ordered[0];
    let opposite = (0..ordered.len())
        .max_by(|&a, &b| {
            let d = |i: usize| (ordered[i][0] - first[0]).hypot(ordered[i][1] - first[1]);
            d(a).total_cmp(&d(b))
        })
        .unwrap_or(0);
    let mut keep = Vec::new();
    simplify(&ordered[..=opposite], tol, &mut keep);
    let mut back = ordered[opposite..].to_vec();
    back.push(first);
    simplify(&back, tol, &mut keep);
    if keep.len() < 3 {
        bail!("the slab outline collapsed to {} corner(s)", keep.len());
    }
    Ok(keep)
}

pub fn area(polygon: &[[f64; 2]]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|i| {
            let (a, b) = (polygon[i], polygon[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

#[derive(Clone, Copy)]
pub struct Ground {
    pub cell: f64,
    pub tol: f64,
    pub slope: f64,
}

pub fn classify_ground(cloud: &Cloud, settings: &Ground) -> Result<Vec<bool>> {
    let Ground { cell, tol, slope } = *settings;
    if !(cell > 0.0) || !(tol > 0.0) {
        bail!("`cell` and `tol` must be positive");
    }
    let Some((lo, hi)) = cloud.bounds() else {
        bail!("there are no points to classify");
    };
    let nx = (((hi[0] - lo[0]) / cell).floor() as usize + 1).min(8192);
    let ny = (((hi[1] - lo[1]) / cell).floor() as usize + 1).min(8192);
    let index = |p: &P3| {
        let i = (((p[0] - lo[0]) / cell) as usize).min(nx - 1);
        let j = (((p[1] - lo[1]) / cell) as usize).min(ny - 1);
        j * nx + i
    };
    let mut floor = vec![f64::INFINITY; nx * ny];
    for p in &cloud.positions {
        let c = index(p);
        floor[c] = floor[c].min(p[2]);
    }
    let base = floor.clone();
    let mut surface = floor;
    let mut obstacle = vec![false; nx * ny];
    let mut radius = 1usize;
    while radius <= 16 && radius < nx.max(ny) {
        let eroded = sweep(&surface, nx, ny, radius, f64::min, f64::INFINITY);
        let opened = sweep(&eroded, nx, ny, radius, f64::max, f64::NEG_INFINITY);
        let limit = tol + slope * radius as f64 * cell;
        for c in 0..nx * ny {
            if surface[c].is_finite() && opened[c].is_finite() {
                if surface[c] - opened[c] > limit {
                    obstacle[c] = true;
                }
                surface[c] = opened[c];
            }
        }
        radius *= 2;
    }
    Ok(cloud
        .positions
        .iter()
        .map(|p| {
            let c = index(p);
            !obstacle[c] && p[2] - base[c] <= tol
        })
        .collect())
}

fn sweep(
    grid: &[f64],
    nx: usize,
    ny: usize,
    radius: usize,
    pick: fn(f64, f64) -> f64,
    empty: f64,
) -> Vec<f64> {
    let pass = |source: &[f64], along_x: bool| -> Vec<f64> {
        let mut out = vec![empty; source.len()];
        for j in 0..ny {
            for i in 0..nx {
                let mut value = empty;
                for d in 0..=2 * radius {
                    let (a, b) = if along_x {
                        ((i + d).checked_sub(radius), Some(j))
                    } else {
                        (Some(i), (j + d).checked_sub(radius))
                    };
                    if let (Some(a), Some(b)) = (a, b) {
                        if a < nx && b < ny {
                            value = pick(value, source[b * nx + a]);
                        }
                    }
                }
                out[j * nx + i] = value;
            }
        }
        out
    };
    let rows = pass(grid, true);
    pass(&rows, false)
}
