use super::Spec;
use super::tet::TetMesh;
use crate::geometry;
use anyhow::{Context, Result, bail};
use monstertruck::modeling::*;

pub struct Body {
    pub name: String,
    pub mesh: TetMesh,
    pub volume: f64,
    pub took: std::time::Duration,
}

pub fn size(spec: &Spec) -> Result<f64> {
    if let Some(size) = spec.mesh {
        return Ok(size);
    }
    let model = spec.target()?;
    let bounds: BoundingBox<Point3> = model
        .solids()
        .iter()
        .flat_map(|s| {
            let b = geometry::bounds(s);
            [b.min(), b.max()]
        })
        .collect();
    Ok(bounds.diameter() / 20.0)
}

pub fn bodies(spec: &Spec) -> Result<Vec<Body>> {
    let size = size(spec)?;
    let named = spec.named()?;
    if named.is_empty() {
        bail!("there is nothing to mesh");
    }
    use rayon::prelude::*;
    let meshing = crate::progress::stage("meshing", Some(named.len()), "parts");
    named
        .par_iter()
        .map(|(name, solid)| {
            let clock = crate::host::Instant::now();
            let (mesh, _) =
                mesh_named(name, solid, size).with_context(|| format!("meshing `{name}`"))?;
            meshing.advance(1.0);
            meshing.detail(|| format!("`{name}` has {} nodes", mesh.nodes.len()));
            Ok(Body {
                name: name.clone(),
                volume: geometry::volume(solid),
                mesh,
                took: clock.elapsed(),
            })
        })
        .collect()
}

const FLATTEST: f64 = 0.5;

pub fn mesh_part(solid: &Solid, size: f64) -> Result<(TetMesh, f64)> {
    mesh_reporting(solid, size, &crate::progress::Stage::silent())
}

pub fn mesh_named(name: &str, solid: &Solid, size: f64) -> Result<(TetMesh, f64)> {
    let stage = crate::progress::stage_after(
        format!("meshing `{name}`"),
        None,
        "",
        std::time::Duration::from_secs(2),
    );
    mesh_reporting(solid, size, &stage)
}

fn mesh_reporting(
    solid: &Solid,
    size: f64,
    stage: &crate::progress::Stage,
) -> Result<(TetMesh, f64)> {
    let own = size.min(geometry::bounds(solid).diameter() / 6.0);
    let mut tried = own;
    let mut best: Option<(TetMesh, f64, f64)> = None;
    let mut last = None;
    for attempt in 1..=5 {
        let at = crate::plot::label(tried);
        let retry = match &best {
            Some((_, _, worst)) => format!(", try {attempt} (worst angle {worst:.1}°)"),
            None if attempt > 1 => format!(", try {attempt} (the last failed)"),
            None => String::new(),
        };
        let report = |what: String| stage.detail(|| format!("{at} mm{retry}: {what}"));
        match super::tet::mesh_solid_reporting(solid, tried, &report) {
            Ok(mesh) => {
                let worst = mesh.quality().min_dihedral;
                if worst >= FLATTEST {
                    return Ok((mesh, tried));
                }
                if best.as_ref().is_none_or(|b| worst > b.2) {
                    best = Some((mesh, tried, worst));
                }
            }
            Err(error) => last = Some(error),
        }
        tried *= 0.6;
    }
    match (best, last) {
        (Some((mesh, used, _)), _) => Ok((mesh, used)),
        (None, Some(error)) => Err(error),
        (None, None) => unreachable!("five tries"),
    }
}

pub fn plan(spec: &Spec) -> Result<String> {
    let size = size(spec)?;
    let bodies = bodies(spec)?;
    let mut lines = vec![format!("mesh size {}", crate::plot::label(size))];
    for body in &bodies {
        let q = body.mesh.quality();
        lines.push(format!(
            "{}: {} nodes, {} tetrahedra ({} nodes once quadratic), volume {:+.2}% of the solid, worst dihedral {:.1}°, {} below 10°, meshed in {:.2} s",
            body.name,
            q.nodes,
            q.tets,
            body.mesh.quadratic_nodes(),
            (q.volume - body.volume) / body.volume * 100.0,
            q.min_dihedral,
            q.slivers,
            body.took.as_secs_f64()
        ));
    }
    Ok(lines.join("\n"))
}

pub fn render(bodies: &[Body], path: &str) -> Result<()> {
    let bounds: BoundingBox<Point3> = bodies
        .iter()
        .flat_map(|b| b.mesh.nodes.iter().copied())
        .collect();
    let cut = bounds.center().y;
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    let mut edges = Vec::new();
    let skin = [150.0 / 255.0, 175.0 / 255.0, 205.0 / 255.0];
    let inner = [230.0 / 255.0, 160.0 / 255.0, 90.0 / 255.0];
    for body in bodies {
        let mesh = &body.mesh;
        let base = positions.len();
        positions.extend(mesh.nodes.iter().copied());
        let kept: Vec<bool> = (0..mesh.tets.len())
            .map(|t| {
                let c = mesh.tets[t]
                    .iter()
                    .map(|&v| mesh.nodes[v as usize].y)
                    .sum::<f64>()
                    / 4.0;
                c >= cut
            })
            .collect();
        let outer: std::collections::HashSet<[u32; 3]> = mesh
            .boundary
            .iter()
            .map(|(f, _)| {
                let mut k = *f;
                k.sort_unstable();
                k
            })
            .collect();
        let mut count: std::collections::HashMap<[u32; 3], ([u32; 3], usize)> = Default::default();
        for (t, tet) in mesh.tets.iter().enumerate() {
            if !kept[t] {
                continue;
            }
            for i in 0..4 {
                let face: Vec<u32> = (0..4).filter(|&k| k != i).map(|k| tet[k]).collect();
                let face = [face[0], face[1], face[2]];
                let mut key = face;
                key.sort_unstable();
                count.entry(key).or_insert((face, 0)).1 += 1;
            }
        }
        for (key, (face, n)) in count {
            if n != 1 {
                continue;
            }
            let tint = if outer.contains(&key) { skin } else { inner };
            triangles.push((face.map(|v| v as usize + base), [tint; 3]));
            for k in 0..3 {
                let (a, b) = (face[k], face[(k + 1) % 3]);
                if a < b {
                    edges.push(vec![mesh.nodes[a as usize], mesh.nodes[b as usize]]);
                }
            }
        }
    }
    crate::render::render_triangles(&positions, &triangles, &edges, bounds, path)
}
