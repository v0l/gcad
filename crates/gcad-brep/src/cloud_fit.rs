use super::Model;
use super::args::Args;
use crate::geometry::{Frame, Profile};
use crate::parse::Line;
use anyhow::{Result, anyhow, bail};
use gcad_cloud::Cloud;
use gcad_cloud::fit::{Found, Kind, Primitive, fit_all, fit_one};
use gcad_cloud::ops::{self, Orient};
use gcad_cloud::segment::{self, Ground};
use monstertruck::modeling::{Point3, Vector3};
use std::sync::Arc;

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

impl Model {
    fn free_cloud_name(&self, name: &str, source: &str) -> Result<()> {
        if name == source {
            bail!("`into={name}` would replace the cloud it reads; pick another name");
        }
        if self.bodies.iter().any(|(n, _)| n == name)
            || self.scene.mesh_bodies.iter().any(|(n, _)| n == name)
        {
            bail!("there is already a body called `{name}`");
        }
        Ok(())
    }

    fn publish_vector(&mut self, name: &str, field: &str, v: [f64; 3]) {
        for (k, axis) in ["x", "y", "z"].iter().enumerate() {
            self.scope.insert(format!("{name}.{field}.{axis}"), v[k]);
        }
    }

    fn publish_primitive(&mut self, name: &str, found: &Found, cloud: &Cloud) {
        match found.primitive {
            Primitive::Plane { origin, normal } => {
                self.publish_vector(name, "origin", origin);
                self.publish_vector(name, "normal", normal);
                self.fits.insert(
                    format!("{name}.plane"),
                    Frame::from_normal(
                        Point3::new(origin[0], origin[1], origin[2]),
                        Vector3::new(normal[0], normal[1], normal[2]),
                    ),
                );
            }
            Primitive::Sphere { centre, radius } => {
                self.publish_vector(name, "centre", centre);
                self.scope.insert(format!("{name}.radius"), radius);
            }
            Primitive::Cylinder {
                origin,
                axis,
                radius,
            } => {
                self.publish_vector(name, "origin", origin);
                self.publish_vector(name, "axis", axis);
                self.scope.insert(format!("{name}.radius"), radius);
                let high = found
                    .inliers
                    .iter()
                    .map(|&i| {
                        let p = cloud.positions[i];
                        (0..3).map(|k| (p[k] - origin[k]) * axis[k]).sum::<f64>()
                    })
                    .fold(f64::NEG_INFINITY, f64::max);
                let rim = std::array::from_fn::<f64, 3, _>(|k| origin[k] + axis[k] * high);
                self.fits.insert(
                    format!("{name}.cylinder"),
                    Frame::from_normal(
                        Point3::new(rim[0], rim[1], rim[2]),
                        Vector3::new(axis[0], axis[1], axis[2]),
                    ),
                );
            }
            Primitive::Cone {
                apex,
                axis,
                half_angle,
            } => {
                self.publish_vector(name, "apex", apex);
                self.publish_vector(name, "axis", axis);
                self.scope.insert(format!("{name}.angle"), half_angle);
            }
        }
        self.scope.insert(format!("{name}.rms"), found.rms);
        self.scope
            .insert(format!("{name}.inliers"), found.inliers.len() as f64);
    }

    pub(crate) fn cloud_fit(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["cloud", "kind"],
            &["tol", "into", "max", "min", "seed"],
            true,
        )?;
        let source = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&source, "fit")?;
        let text = args.text("kind")?;
        let kind = Kind::parse(text)
            .ok_or_else(|| anyhow!("`fit` makes a plane, cylinder, sphere or cone, not `{text}`"))?;
        let peel = match args.rest.as_slice() {
            [] => false,
            ["all"] => true,
            _ => bail!("`fit` takes `all` after its options to peel off every primitive"),
        };
        let tol = args
            .optional_number("tol", &self.scope)?
            .unwrap_or(diagonal(&cloud) * 0.01);
        if !(tol > 0.0) {
            bail!("`tol` must be positive, got {tol}");
        }
        let into = args.values.get("into").copied().unwrap_or("fit").to_string();
        self.free_cloud_name(&into, &source)?;
        let seed = args
            .optional_number("seed", &self.scope)?
            .map_or(Ok(1), |s| whole(s, "seed"))? as u64;
        let min = match args.optional_number("min", &self.scope)? {
            Some(m) => whole(m, "min")?,
            None => (cloud.len() / 100).max(10),
        };
        let working = if kind.needs_normals() && cloud.normals.is_none() {
            let n = cloud.len() as f64;
            let centre = cloud
                .positions
                .iter()
                .fold([0.0; 3], |s, p| [s[0] + p[0] / n, s[1] + p[1] / n, s[2] + p[2] / n]);
            Arc::new(ops::estimate_normals(&cloud, 20, Orient::Toward(centre))?)
        } else {
            cloud.clone()
        };
        if peel {
            let max = match args.optional_number("max", &self.scope)? {
                Some(m) => whole(m, "max")?,
                None => 20,
            };
            let found = fit_all(&working, kind, tol, max, min, seed);
            if found.is_empty() {
                bail!("no {text} found within tol={tol}; try a larger tol");
            }
            for (k, one) in found.iter().enumerate() {
                let name = format!("{into}{}", k + 1);
                self.publish_primitive(&name, one, &cloud);
                self.publish_cloud(&name, cloud.select(&one.inliers));
            }
            self.scope.insert(format!("{into}.count"), found.len() as f64);
            let covered: usize = found.iter().map(|f| f.inliers.len()).sum();
            Ok(format!(
                "{} {text}(s), {covered} of {} points",
                found.len(),
                cloud.len()
            ))
        } else {
            let all: Vec<usize> = (0..working.len()).collect();
            let one = fit_one(&working, kind, tol, &all, seed)
                .ok_or_else(|| anyhow!("no {text} found within tol={tol}; try a larger tol"))?;
            self.publish_primitive(&into, &one, &cloud);
            self.scope.insert(format!("{into}.count"), 1.0);
            let inliers = cloud.select(&one.inliers);
            let report = format!(
                "{text} with {} inliers of {}, rms {}",
                one.inliers.len(),
                cloud.len(),
                crate::parse::label(one.rms)
            );
            self.publish_cloud(&into, inliers);
            Ok(report)
        }
    }

    fn publish_groups(&mut self, into: &str, cloud: &Cloud, groups: &[Vec<usize>]) {
        for (k, group) in groups.iter().enumerate() {
            let name = format!("{into}.{}", k + 1);
            self.clouds.retain(|(n, _)| *n != name);
            self.clouds.push((name, Arc::new(cloud.select(group))));
        }
        self.scope.insert(format!("{into}.count"), groups.len() as f64);
    }

    pub(crate) fn cloud_cluster(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &["dist", "min", "into"], false)?;
        let source = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&source, "cluster")?;
        let dist = args.number("dist", &self.scope)?;
        let min = match args.optional_number("min", &self.scope)? {
            Some(m) => whole(m, "min")?,
            None => 1,
        };
        let into = args.values.get("into").copied().unwrap_or("clusters").to_string();
        self.free_cloud_name(&into, &source)?;
        let groups = segment::cluster(&cloud, dist, min)?;
        self.publish_groups(&into, &cloud, &groups);
        Ok(format!(
            "{} cluster(s): {}",
            groups.len(),
            groups
                .iter()
                .map(|g| g.len().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    pub(crate) fn cloud_grow(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &["angle", "curvature", "k", "min", "into"], false)?;
        let source = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&source, "grow")?;
        let angle = args.optional_number("angle", &self.scope)?.unwrap_or(5.0);
        let curvature = args.optional_number("curvature", &self.scope)?.unwrap_or(0.05);
        let k = match args.optional_number("k", &self.scope)? {
            Some(k) => whole(k, "k")?,
            None => 12,
        };
        let min = match args.optional_number("min", &self.scope)? {
            Some(m) => whole(m, "min")?,
            None => 30,
        };
        let into = args.values.get("into").copied().unwrap_or("regions").to_string();
        self.free_cloud_name(&into, &source)?;
        let groups = segment::grow(&cloud, angle, curvature, k, min)?;
        self.publish_groups(&into, &cloud, &groups);
        Ok(format!(
            "{} region(s): {}",
            groups.len(),
            groups
                .iter()
                .map(|g| g.len().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    pub(crate) fn cloud_slice(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &["plane", "thickness", "offset", "tol", "into"], false)?;
        let source = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&source, "slice")?;
        self.require_empty_sketch("slice")?;
        let thickness = args.optional_number("thickness", &self.scope)?.unwrap_or(1.0);
        if !(thickness > 0.0) {
            bail!("`thickness` must be positive, got {thickness}");
        }
        let text = args.text("plane")?;
        let named = match text.to_ascii_lowercase().as_str() {
            "x" => "YZ",
            "y" => "XZ",
            "z" => "XY",
            _ => text,
        };
        let frame = self
            .plane_from(named)?
            .offset(args.optional_number("offset", &self.scope)?.unwrap_or(0.0));
        let tol = args
            .optional_number("tol", &self.scope)?
            .unwrap_or(thickness * 0.05);
        let origin = [frame.origin.x, frame.origin.y, frame.origin.z];
        let vector = |v: Vector3| [v.x, v.y, v.z];
        let points = segment::slab(
            &cloud,
            origin,
            vector(frame.x),
            vector(frame.y),
            vector(frame.normal),
            thickness,
        );
        let corners = segment::outline(&points, tol)?;
        let area = segment::area(&corners);
        if let Some(name) = args.values.get("into") {
            self.scope.insert(format!("{name}.area"), area);
            self.scope.insert(format!("{name}.corners"), corners.len() as f64);
            self.scope.insert(format!("{name}.points"), points.len() as f64);
        }
        self.frame = Some(frame);
        self.sketch.push(Profile::Polygon {
            points: corners.iter().map(|c| (c[0], c[1])).collect(),
        });
        Ok(format!(
            "{} points in the slab, outline of {} corners, area {}",
            points.len(),
            corners.len(),
            crate::parse::label(area)
        ))
    }

    pub(crate) fn cloud_classify(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud", "mode"], &["cell", "tol", "slope"], false)?;
        let name = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&name, "classify")?;
        if args.text("mode")? != "ground" {
            bail!("`classify scan ground` is the only classification");
        }
        let spacing = ops::mean_spacing(&cloud).unwrap_or(1.0);
        let settings = Ground {
            cell: args.optional_number("cell", &self.scope)?.unwrap_or(spacing * 4.0),
            tol: args.optional_number("tol", &self.scope)?.unwrap_or(spacing * 0.5),
            slope: args.optional_number("slope", &self.scope)?.unwrap_or(0.3),
        };
        let ground = segment::classify_ground(&cloud, &settings)?;
        let mut out = (*cloud).clone();
        out.classes = Some(ground.iter().map(|g| if *g { 2 } else { 1 }).collect());
        let count = ground.iter().filter(|g| **g).count();
        self.publish_cloud(&name, out);
        self.scope.insert(format!("{name}.ground"), count as f64);
        Ok(format!("{count} of {} points are ground", ground.len()))
    }
}
