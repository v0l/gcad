use super::Model;
use super::args::Args;
use crate::geometry;
use crate::parse::{Line, Scope, eval, label, split_top_level};
use anyhow::{Result, anyhow, bail};
use gcad_cloud::Cloud;
use gcad_cloud::ops::{self, M4};
use gcad_cloud::register::{self, Registration, Settings};
use monstertruck::meshing::prelude::*;
use std::sync::Arc;

const SEED: u64 = 1;
const CAD_POINTS: usize = 40_000;
const DEFAULT_ITERS: f64 = 50.0;
const DEFAULT_TRIM: f64 = 0.9;
const DEFAULT_REACH: f64 = 0.1;

fn whole(value: f64, name: &str) -> Result<usize> {
    if value.fract() != 0.0 || value < 0.0 {
        bail!("`{name}` must be a whole number from 0, got {value}");
    }
    Ok(value as usize)
}

fn diagonal(cloud: &Cloud) -> f64 {
    cloud.bounds().map_or(1.0, |(lo, hi)| {
        (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt()
    })
}

fn pair_point(text: &str, scope: &Scope) -> Result<[f64; 3]> {
    let parts = split_top_level(text, ',');
    if parts.len() != 3 {
        bail!("`{text}` is not a point: write x,y,z");
    }
    Ok([eval(&parts[0], scope)?, eval(&parts[1], scope)?, eval(&parts[2], scope)?])
}

fn pair(text: &str, scope: &Scope) -> Result<([f64; 3], [f64; 3])> {
    let (from, to) = text
        .split_once('>')
        .ok_or_else(|| anyhow!("`{text}` is not a point pair: write bx,by,bz>ax,ay,az"))?;
    Ok((pair_point(from, scope)?, pair_point(to, scope)?))
}

fn report(name: &str, r: &Registration, how: &str) -> String {
    format!(
        "{how}: {} iterations, {} pairs, rms {}, rotation {} degrees; {}",
        r.iterations,
        r.pairs,
        label(r.rms),
        label(register::rotation_angle(&r.matrix)),
        register::matrix_line(name, &r.matrix)
    )
}

impl Model {
    pub(crate) fn cloud_align(&mut self, line: &Line) -> Result<String> {
        if line.positional.first().is_some_and(|w| w == "scans") {
            return self.align_scans(line);
        }
        if line.named.iter().any(|(k, _)| k == "pairs") {
            return self.cloud_align_pairs(line);
        }
        let args = Args::new(
            line,
            &["cloud", "mode"],
            &["to", "pairs", "max_dist", "iters", "trim", "voxel", "count", "seed"],
            true,
        )?;
        let name = args.text("cloud")?.to_string();
        let moving = self.named_cloud(&name, "align")?;
        let to = args.text("to")?.to_string();
        let target = self.align_target(&to, &args)?;
        let mode = args.text("mode").unwrap_or("icp");
        let (matrix, text) = match mode {
            "icp" => {
                let plane = match args.rest.first().copied() {
                    None | Some("point") => false,
                    Some("plane") => true,
                    Some(other) => bail!("ICP is `icp point` or `icp plane`, not `icp {other}`"),
                };
                let result = self.run_icp(&moving, &target, plane, register_start(), &args)?;
                (result.matrix, report(&name, &result, if plane { "icp plane" } else { "icp point" }))
            }
            "coarse" => {
                let voxel = args.number("voxel", &self.scope)?;
                let seed = whole(args.optional_number("seed", &self.scope)?.unwrap_or(SEED as f64), "seed")? as u64;
                let start = register::coarse(&moving, &target, voxel, seed)?;
                let result = self.run_icp(&moving, &target, true, start, &args)?;
                (result.matrix, report(&name, &result, "coarse then icp plane"))
            }
            _ => bail!("`align` is `icp point|plane`, `coarse voxel=N`, `pairs=...` or `scans`, not `{mode}`"),
        };
        self.apply_alignment(&name, &moving, &matrix, text)
    }

    pub(crate) fn cloud_align_pairs(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &["to", "pairs"], true)?;
        let name = args.text("cloud")?.to_string();
        let moving = self.named_cloud(&name, "align")?;
        let to = args.text("to")?.to_string();
        self.named_cloud(&to, "align")?;
        let pairs = std::iter::once(args.text("pairs")?)
            .chain(args.rest.iter().copied())
            .map(|text| pair(text, &self.scope))
            .collect::<Result<Vec<_>>>()?;
        if pairs.len() < 3 {
            bail!("`align` needs at least 3 point pairs, got {}", pairs.len());
        }
        let from: Vec<[f64; 3]> = pairs.iter().map(|p| p.0).collect();
        let onto: Vec<[f64; 3]> = pairs.iter().map(|p| p.1).collect();
        let matrix = register::fit_rigid(&from, &onto)?;
        let rms = register::pair_rms(&matrix, &from, &onto);
        let text = format!(
            "pairs: {} pairs, rms {}; {}",
            pairs.len(),
            label(rms),
            register::matrix_line(&name, &matrix)
        );
        self.apply_alignment(&name, &moving, &matrix, text)
    }

    fn apply_alignment(&mut self, name: &str, cloud: &Cloud, matrix: &M4, text: String) -> Result<String> {
        let moved = ops::transform(cloud, matrix)?;
        self.publish_cloud(name, moved);
        Ok(text)
    }

    fn run_icp(
        &self,
        moving: &Cloud,
        target: &Cloud,
        plane: bool,
        start: M4,
        args: &Args<'_>,
    ) -> Result<Registration> {
        let settings = Settings {
            plane,
            max_dist: args
                .optional_number("max_dist", &self.scope)?
                .unwrap_or(diagonal(target) * DEFAULT_REACH),
            iters: whole(args.optional_number("iters", &self.scope)?.unwrap_or(DEFAULT_ITERS), "iters")?,
            trim: args.optional_number("trim", &self.scope)?.unwrap_or(DEFAULT_TRIM),
        };
        register::icp(moving, target, start, &settings)
    }

    fn align_target(&self, to: &str, args: &Args<'_>) -> Result<Cloud> {
        let needs_normals = args.rest.first().copied() == Some("plane")
            || args.values.get("mode").copied() == Some("coarse");
        if let Some(cloud) = self.cloud(to) {
            return if needs_normals {
                register::with_normals(cloud)
            } else {
                Ok((**cloud).clone())
            };
        }
        let count = whole(args.optional_number("count", &self.scope)?.unwrap_or(CAD_POINTS as f64), "count")?;
        self.cad_points(to, count)
    }

    pub(crate) fn cloud_sample_solid(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["solid"], &["count", "seed", "noise", "box"], false)?;
        let name = super::args::label_of(line);
        if self.clouds.iter().any(|(n, _)| *n == name)
            || self.bodies.iter().any(|(n, _)| *n == name)
            || self.scene.mesh_bodies.iter().any(|(n, _)| *n == name)
        {
            bail!("there is already a body called `{name}`");
        }
        let count = whole(args.number("count", &self.scope)?, "count")?;
        let seed = whole(args.optional_number("seed", &self.scope)?.unwrap_or(SEED as f64), "seed")? as u64;
        let noise = args.optional_number("noise", &self.scope)?.unwrap_or(0.0);
        let mut cloud = self.cad_points(args.text("solid")?, count)?;
        if let Some(text) = args.values.get("box") {
            let parts = split_top_level(text, ',');
            if parts.len() != 6 {
                bail!("`box` is x0,y0,z0,x1,y1,z1");
            }
            let v = parts.iter().map(|p| eval(p, &self.scope)).collect::<Result<Vec<_>>>()?;
            cloud = ops::crop(
                &cloud,
                &ops::Region::Box {
                    lo: std::array::from_fn(|k| v[k].min(v[k + 3])),
                    hi: std::array::from_fn(|k| v[k].max(v[k + 3])),
                },
            )?;
        }
        if noise > 0.0 {
            cloud = register::add_noise(&cloud, noise, seed);
        }
        let report = cloud.summary();
        self.publish_cloud(&name, cloud);
        Ok(report)
    }

    pub fn cad_points(&self, name: &str, count: usize) -> Result<Cloud> {
        let mut triangles: Vec<[[f64; 3]; 3]> = Vec::new();
        if let Some((_, solid)) = self.named_solids().into_iter().find(|(n, _)| n == name) {
            let tolerance = geometry::mesh_tolerance(solid);
            for shell in solid.boundaries() {
                for face in shell.robust_triangulation(tolerance).face_iter() {
                    let Some(mesh) = face.surface() else { continue };
                    let positions = mesh.positions();
                    for t in mesh.faces().triangle_iter() {
                        triangles.push(t.map(|v| {
                            let p = positions[v.pos];
                            [p.x, p.y, p.z]
                        }));
                    }
                }
            }
        } else if let Some((_, mesh)) = self.meshes().into_iter().find(|(n, _)| n == name) {
            triangles.extend(
                mesh.triangles
                    .iter()
                    .map(|t| t.map(|i| mesh.positions[i as usize])),
            );
        } else {
            let known: Vec<_> = self.clouds.iter().map(|(n, _)| n.as_str()).collect();
            bail!("`align to={name}` needs a cloud, a solid or a mesh body; clouds are {known:?}");
        }
        if triangles.is_empty() {
            bail!("`{name}` has no surface to align to");
        }
        Ok(register::sample_triangles(&triangles, count, SEED))
    }

    fn align_scans(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["scans"],
            &["max_dist", "iters", "trim", "voxel"],
            true,
        )?;
        let names = args.rest.clone();
        if names.len() < 2 {
            bail!("`align scans` needs at least two clouds: `align scans s1 s2 s3`");
        }
        let mut reports = Vec::new();
        let mut fixed: Arc<Cloud> = self.named_cloud(names[0], "align")?;
        for name in &names[1..] {
            let moving = self.named_cloud(name, "align")?;
            let target = register::with_normals(&fixed)?;
            let start = match args.optional_number("voxel", &self.scope)? {
                Some(voxel) => register::coarse(&moving, &target, voxel, SEED)?,
                None => register_start(),
            };
            let result = self.run_icp(&moving, &target, true, start, &args)?;
            let text = report(name, &result, &format!("{name} to previous"));
            self.apply_alignment(name, &moving, &result.matrix, text.clone())?;
            reports.push(text);
            fixed = self.named_cloud(name, "align")?;
        }
        Ok(format!(
            "sequential registration of {} scans, each to the one before, no loop closure\n{}",
            names.len(),
            reports.join("\n")
        ))
    }
}

fn register_start() -> M4 {
    ops::IDENTITY
}
