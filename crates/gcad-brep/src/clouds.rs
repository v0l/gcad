use super::Model;
use super::args::{Args, label_of};
use crate::parse::Line;
use anyhow::{Context, Result, bail};
use gcad_cloud::{Cloud, Options};
use std::path::Path;
use std::sync::Arc;

pub(crate) fn is_cloud_file(path: &Path) -> bool {
    gcad_cloud::format_of(path).is_ok()
}

fn count(args: &Args<'_>, name: &str, scope: &crate::parse::Scope) -> Result<Option<usize>> {
    match args.optional_number(name, scope)? {
        Some(n) if n.fract() == 0.0 && n >= 0.0 => Ok(Some(n as usize)),
        Some(n) => bail!("`{name}` must be a whole number from 0, got {n}"),
        None => Ok(None),
    }
}

impl Model {
    pub fn named_clouds(&self) -> Vec<(String, Arc<Cloud>)> {
        self.clouds.clone()
    }

    pub fn cloud(&self, name: &str) -> Option<&Arc<Cloud>> {
        self.clouds.iter().find(|(n, _)| n == name).map(|(_, c)| c)
    }

    pub(crate) fn op_cloud(&mut self, line: &Line) -> Result<String> {
        let mut line = line.clone();
        if line.positional.first().is_some_and(|word| word == "import") {
            line.positional.remove(0);
        }
        let args = Args::new(
            &line,
            &["file"],
            &["max_points", "scan"],
            false,
        )?;
        let file = std::path::PathBuf::from(args.text("file")?);
        let path = match &self.dir {
            Some(dir) if file.is_relative() => dir.join(&file),
            _ => file,
        };
        self.import_cloud(&line, &args, &path)
    }

    pub(crate) fn import_cloud(
        &mut self,
        line: &Line,
        args: &Args<'_>,
        path: &Path,
    ) -> Result<String> {
        self.require_empty_sketch("import")?;
        let name = label_of(line);
        if self.clouds.iter().any(|(n, _)| *n == name)
            || self.bodies.iter().any(|(n, _)| *n == name)
            || self.scene.mesh_bodies.iter().any(|(n, _)| *n == name)
        {
            bail!("there is already a body called `{name}`");
        }
        let format = gcad_cloud::format_of(path)?;
        let options = Options {
            max_points: count(args, "max_points", &self.scope)?,
            scan: count(args, "scan", &self.scope)?,
        };
        if options.max_points == Some(0) {
            bail!("`max_points` must be at least 1");
        }
        let source =
            crate::host::open(path).with_context(|| format!("reading {}", path.display()))?;
        let imported = gcad_cloud::import(format, Box::new(source), &options)
            .with_context(|| format!("reading {}", path.display()))?;
        if imported.cloud.is_empty() {
            bail!("{} holds no points", path.display());
        }
        let report = imported.report();
        self.publish_cloud(&name, imported.cloud);
        Ok(report)
    }

    pub(crate) fn publish_cloud(&mut self, name: &str, cloud: Cloud) {
        self.scope.insert(format!("{name}.points"), cloud.len() as f64);
        self.scope
            .insert(format!("{name}.scans"), cloud.scan_ids().len() as f64);
        if let Some((lo, hi)) = cloud.bounds() {
            for (k, axis) in ["x", "y", "z"].iter().enumerate() {
                self.scope.insert(format!("{name}.min.{axis}"), lo[k]);
                self.scope.insert(format!("{name}.max.{axis}"), hi[k]);
                self.scope
                    .insert(format!("{name}.size.{axis}"), hi[k] - lo[k]);
                self.scope
                    .insert(format!("{name}.centre.{axis}"), (hi[k] + lo[k]) / 2.0);
            }
        }
        self.clouds.retain(|(n, _)| n != name);
        self.clouds.push((name.to_string(), Arc::new(cloud)));
    }
}
