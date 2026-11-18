use super::Model;
use super::args::{Args, direction, label_of, point3};
use crate::parse::{Line, eval, split_top_level};
use anyhow::{Result, anyhow, bail};
use gcad_cloud::colour::{ColourBy, colours};
use gcad_cloud::ops::{self, M4, Orient, Region};
use gcad_cloud::Cloud;
use monstertruck::modeling::Point3;
use std::sync::Arc;

const SEED: u64 = 1;

fn array(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

fn numbers(text: &str, want: usize, what: &str, scope: &crate::parse::Scope) -> Result<Vec<f64>> {
    let parts = split_top_level(text, ',');
    if parts.len() != want {
        bail!("`{text}` is not {what}: give {want} numbers separated by commas");
    }
    parts.iter().map(|p| eval(p, scope)).collect()
}

fn whole(value: f64, name: &str) -> Result<usize> {
    if value.fract() != 0.0 || value < 0.0 {
        bail!("`{name}` must be a whole number from 0, got {value}");
    }
    Ok(value as usize)
}

impl Model {
    pub(crate) fn cloud_op(&mut self, line: &Line) -> Option<Result<String>> {
        let first_is_cloud = line
            .positional
            .first()
            .is_some_and(|name| self.cloud(name).is_some());
        Some(match line.op.as_str() {
            "move" | "rotate" | "scale" if first_is_cloud => self.cloud_move(line),
            "crop" => self.cloud_crop(line),
            "downsample" => self.cloud_downsample(line),
            "denoise" => self.cloud_denoise(line),
            "normals" => self.cloud_normals(line),
            "transform" => self.cloud_transform(line),
            "merge" if first_is_cloud => self.cloud_merge(line),
            "colour" => self.cloud_colour(line),
            "info" => self.cloud_info(line),
            "cloud_add_outliers" => self.cloud_outliers(line),
            "cloud_synthetic" => self.cloud_synthetic(line),
            _ => return None,
        })
    }

    fn named_cloud(&self, name: &str, op: &str) -> Result<Arc<Cloud>> {
        self.cloud(name).cloned().ok_or_else(|| {
            let known: Vec<_> = self.clouds.iter().map(|(n, _)| n.as_str()).collect();
            anyhow!("`{op}` needs a point cloud, but `{name}` is not one; clouds are {known:?}")
        })
    }

    fn replace_cloud(&mut self, name: &str, cloud: Cloud) -> String {
        let report = cloud.summary();
        self.publish_cloud(name, cloud);
        report
    }

    fn cloud_move(&mut self, line: &Line) -> Result<String> {
        let name = line.positional[0].clone();
        let cloud = self.named_cloud(&name, &line.op)?;
        let matrix: M4 = match line.op.as_str() {
            "move" => {
                let args = Args::new(line, &["cloud", "by"], &[], false)?;
                ops::translation(array(point3(args.text("by")?, &self.scope)?))
            }
            "rotate" => {
                let args = Args::new(line, &["cloud", "axis", "angle"], &["about"], false)?;
                let axis = match args.text("axis")? {
                    word @ ("x" | "y" | "z") => direction(word, &self.scope)?,
                    other => direction(other, &self.scope)?,
                };
                let angle = args.number("angle", &self.scope)?;
                let centre = match args.values.get("about") {
                    Some(text) => array(point3(text, &self.scope)?),
                    None => [0.0; 3],
                };
                ops::about(centre, ops::rotation([axis.x, axis.y, axis.z], angle))
            }
            _ => {
                let args = Args::new(line, &["cloud", "factor"], &["about"], false)?;
                let text = args.text("factor")?;
                let factors = if text.contains(',') {
                    numbers(text, 3, "a scale", &self.scope)?
                } else {
                    vec![args.number("factor", &self.scope)?; 3]
                };
                if factors.iter().any(|f| *f <= 0.0) {
                    bail!("scale factor must be positive, got {text}");
                }
                let centre = match args.values.get("about") {
                    Some(text) => array(point3(text, &self.scope)?),
                    None => [0.0; 3],
                };
                ops::about(centre, ops::scaling([factors[0], factors[1], factors[2]]))
            }
        };
        let moved = ops::transform(&cloud, &matrix)?;
        Ok(self.replace_cloud(&name, moved))
    }

    fn cloud_transform(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &[], true)?;
        let name = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&name, "transform")?;
        let text = args.rest.join(",");
        let values = numbers(&text, 16, "a 4x4 matrix", &self.scope)?;
        let matrix: M4 = std::array::from_fn(|r| std::array::from_fn(|c| values[r * 4 + c]));
        if matrix[3] != [0.0, 0.0, 0.0, 1.0] {
            bail!("the last row of a transform must be 0,0,0,1");
        }
        let moved = ops::transform(&cloud, &matrix)?;
        Ok(self.replace_cloud(&name, moved))
    }

    fn cloud_crop(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["cloud"],
            &["box", "sphere", "plane", "keep", "polygon", "offset"],
            true,
        )?;
        let name = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&name, "crop")?;
        let given = ["box", "sphere", "plane", "polygon"]
            .into_iter()
            .filter(|k| args.has(k))
            .count();
        if given != 1 {
            bail!("`crop` needs one of box=x0,y0,z0,x1,y1,z1, sphere=cx,cy,cz,r, plane=XY keep=above|below, or polygon=x,y x,y ...");
        }
        let region = if let Some(text) = args.values.get("box") {
            let v = numbers(text, 6, "a box", &self.scope)?;
            Region::Box {
                lo: std::array::from_fn(|k| v[k].min(v[k + 3])),
                hi: std::array::from_fn(|k| v[k].max(v[k + 3])),
            }
        } else if let Some(text) = args.values.get("sphere") {
            let v = numbers(text, 4, "a sphere", &self.scope)?;
            Region::Sphere {
                centre: [v[0], v[1], v[2]],
                radius: v[3],
            }
        } else if let Some(text) = args.values.get("plane") {
            let above = match args.values.get("keep").copied() {
                Some("above") => true,
                Some("below") => false,
                _ => bail!("a plane crop needs keep=above or keep=below"),
            };
            let frame = self
                .plane_from(text)?
                .offset(args.optional_number("offset", &self.scope)?.unwrap_or(0.0));
            Region::Plane {
                origin: [frame.origin.x, frame.origin.y, frame.origin.z],
                normal: [frame.normal.x, frame.normal.y, frame.normal.z],
                above,
            }
        } else {
            let first = args.text("polygon").unwrap_or_default();
            let corners = std::iter::once(first)
                .chain(args.rest.iter().copied())
                .map(|text| numbers(text, 2, "a polygon corner", &self.scope).map(|v| [v[0], v[1]]))
                .collect::<Result<Vec<_>>>()?;
            Region::Prism(corners)
        };
        let kept = ops::crop(&cloud, &region)?;
        Ok(self.replace_cloud(&name, kept))
    }

    fn cloud_downsample(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &["voxel", "every", "count", "seed"], false)?;
        let name = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&name, "downsample")?;
        let modes = ["voxel", "every", "count"].into_iter().filter(|k| args.has(k)).count();
        if modes != 1 {
            bail!("`downsample` needs one of voxel=size, every=n or count=n");
        }
        let out = if args.has("voxel") {
            ops::voxel_downsample(&cloud, args.number("voxel", &self.scope)?)?
        } else if args.has("every") {
            ops::every(&cloud, whole(args.number("every", &self.scope)?, "every")?)?
        } else {
            let seed = match args.optional_number("seed", &self.scope)? {
                Some(s) => whole(s, "seed")? as u64,
                None => SEED,
            };
            ops::sample_count(&cloud, whole(args.number("count", &self.scope)?, "count")?, seed)?
        };
        Ok(self.replace_cloud(&name, out))
    }

    fn cloud_denoise(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["cloud", "mode"],
            &["k", "sigma", "radius", "min"],
            false,
        )?;
        let name = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&name, "denoise")?;
        let out = match args.values.get("mode").copied() {
            Some("stat") => {
                let k = whole(args.optional_number("k", &self.scope)?.unwrap_or(16.0), "k")?;
                let sigma = args.optional_number("sigma", &self.scope)?.unwrap_or(2.0);
                ops::denoise_stat(&cloud, k, sigma)?
            }
            Some("radius") => {
                let radius = args.number("radius", &self.scope)?;
                let min = whole(args.optional_number("min", &self.scope)?.unwrap_or(5.0), "min")?;
                ops::denoise_radius(&cloud, radius, min)?
            }
            _ if args.has("radius") => {
                let radius = args.number("radius", &self.scope)?;
                let min = whole(args.optional_number("min", &self.scope)?.unwrap_or(5.0), "min")?;
                ops::denoise_radius(&cloud, radius, min)?
            }
            _ => bail!("`denoise scan stat k=16 sigma=2` or `denoise scan radius=10 min=5`"),
        };
        let removed = cloud.len() - out.len();
        let report = format!("removed {removed} of {} points; {}", cloud.len(), out.summary());
        self.publish_cloud(&name, out);
        Ok(report)
    }

    fn cloud_normals(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &["k", "toward"], false)?;
        let name = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&name, "normals")?;
        let k = whole(args.optional_number("k", &self.scope)?.unwrap_or(20.0), "k")?;
        let orient = match args.values.get("toward").copied() {
            Some("origin") => Orient::Origins,
            Some(text) => Orient::Toward(array(point3(text, &self.scope)?)),
            None if !cloud.origins.is_empty() => Orient::Origins,
            None => Orient::Tree,
        };
        let out = ops::estimate_normals(&cloud, k, orient)?;
        Ok(self.replace_cloud(&name, out))
    }

    fn cloud_merge(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &[], true)?;
        let at = args
            .rest
            .iter()
            .position(|w| *w == "into")
            .ok_or_else(|| anyhow!("write `merge a b c into all`"))?;
        let (from, into) = args.rest.split_at(at);
        let target = match into {
            [_, target] => *target,
            _ => bail!("write `merge a b c into all`, with one name after `into`"),
        };
        if from.len() < 2 {
            bail!("`merge` needs at least two clouds before `into`");
        }
        let parts = from
            .iter()
            .map(|n| self.named_cloud(n, "merge"))
            .collect::<Result<Vec<_>>>()?;
        if !from.contains(&target)
            && (self.bodies.iter().any(|(n, _)| n == target)
                || self.scene.mesh_bodies.iter().any(|(n, _)| n == target))
        {
            bail!("there is already a body called `{target}`");
        }
        let refs: Vec<&Cloud> = parts.iter().map(|p| p.as_ref()).collect();
        let merged = ops::merge_scans(&refs);
        Ok(self.replace_cloud(target, merged))
    }

    fn cloud_colour(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &["by"], false)?;
        let name = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&name, "colour")?;
        let text = args.text("by")?;
        let by = ColourBy::parse(text).ok_or_else(|| {
            anyhow!("`by` is height, intensity, normal, class or rgb, got `{text}`")
        })?;
        if !by.available(&cloud) {
            bail!("the cloud has no {text} to colour by");
        }
        let all: Vec<usize> = (0..cloud.len()).collect();
        let mut out = (*cloud).clone();
        if by != ColourBy::Rgb {
            out.colours = Some(colours(&cloud, by, &all));
        }
        Ok(self.replace_cloud(&name, out))
    }

    fn cloud_info(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &[], false)?;
        let cloud = self.named_cloud(args.text("cloud")?, "info")?;
        let mut text = cloud.summary();
        match ops::mean_spacing(&cloud) {
            Some(spacing) if spacing > 0.0 => {
                text += &format!(
                    ", mean spacing {}, density {} points per unit area",
                    crate::parse::label(spacing),
                    crate::parse::label(1.0 / (spacing * spacing))
                );
            }
            _ => text += ", mean spacing unknown",
        }
        Ok(text)
    }

    fn cloud_outliers(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["cloud"], &["count", "distance", "seed"], false)?;
        let name = args.text("cloud")?.to_string();
        let cloud = self.named_cloud(&name, "cloud_add_outliers")?;
        let count = whole(args.number("count", &self.scope)?, "count")?;
        let distance = args.number("distance", &self.scope)?;
        let seed = whole(args.optional_number("seed", &self.scope)?.unwrap_or(1.0), "seed")? as u64;
        let out = ops::add_outliers(&cloud, count, distance, seed)?;
        Ok(self.replace_cloud(&name, out))
    }

    fn cloud_synthetic(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &["count", "seed", "size"], false)?;
        let name = label_of(line);
        if self.clouds.iter().any(|(n, _)| *n == name)
            || self.bodies.iter().any(|(n, _)| *n == name)
            || self.scene.mesh_bodies.iter().any(|(n, _)| *n == name)
        {
            bail!("there is already a body called `{name}`");
        }
        let count = whole(args.number("count", &self.scope)?, "count")?;
        let seed = whole(args.optional_number("seed", &self.scope)?.unwrap_or(1.0), "seed")? as u64;
        let size = args.optional_number("size", &self.scope)?.unwrap_or(100.0);
        if count == 0 || size <= 0.0 {
            bail!("`cloud_synthetic` needs a positive count and size");
        }
        let cloud = ops::synthetic(count, seed, size);
        Ok(self.replace_cloud(&name, cloud))
    }
}
