use super::Model;
use super::args::{Args, label_of, point3};
use crate::mesh::{Mesh, Region, Selection};
use crate::parse::{Line, eval, split_top_level};
use anyhow::{Result, anyhow, bail};
use gcad_cloud::pivot::ball_pivoting;
use gcad_cloud::poisson::{PoissonOptions, poisson};
use gcad_cloud::{Cloud, KdTree, Point};
use std::sync::Arc;

const TAUBIN_LAMBDA: f64 = 0.5;
const TAUBIN_MU: f64 = -0.53;
const ISLAND_SHARE: f64 = 0.02;
const SEED: u64 = 1;
const POISSON_FILL_EDGES: usize = 400;

fn whole(value: f64, name: &str) -> Result<usize> {
    if value.fract() != 0.0 || value < 0.0 {
        bail!("`{name}` must be a whole number from 0, got {value}");
    }
    Ok(value as usize)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

struct Random(u64);

impl Random {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
}

impl Model {
    pub(crate) fn recon_op(&mut self, line: &Line) -> Option<Result<String>> {
        let names_mesh = |model: &Model| {
            line.positional
                .first()
                .is_some_and(|n| model.mesh_named(n).is_some())
        };
        Some(match line.op.as_str() {
            "mesh" => self.mesh_from_cloud(line),
            "fill" => self.mesh_fill(line),
            "clean" if names_mesh(self) => self.mesh_clean(line),
            "open_hole" => self.mesh_open_hole(line),
            "points" => self.mesh_points(line),
            "sample" => self.mesh_sample(line),
            "smooth" if line.positional.iter().any(|w| w == "taubin") => self.mesh_taubin(line),
            "decimate"
                if line.named.iter().any(|(k, _)| k == "to" || k == "ratio") || names_mesh(self) =>
            {
                self.mesh_decimate(line)
            }
            _ => return None,
        })
    }

    pub(crate) fn mesh_named(&self, name: &str) -> Option<Arc<Mesh>> {
        if let Some((_, mesh)) = self.scene.mesh_bodies.iter().find(|(n, _)| n == name) {
            return Some(mesh.clone());
        }
        let mesh = self.scene.mesh_body.mesh.as_ref()?;
        (name == self.scene.body_name() || mesh.parts.iter().any(|p| p == name))
            .then(|| mesh.clone())
    }

    fn mesh_edit(
        &mut self,
        name: &str,
        op: &str,
        edit: impl FnOnce(&mut Mesh) -> Result<String>,
    ) -> Result<String> {
        if let Some(at) = self.scene.mesh_bodies.iter().position(|(n, _)| n == name) {
            let mesh = Arc::make_mut(&mut self.scene.mesh_bodies[at].1);
            return edit(mesh);
        }
        let current = self.scene.body_name();
        let in_current = self
            .scene
            .mesh_body
            .mesh
            .as_ref()
            .is_some_and(|m| name == current || m.parts.iter().any(|p| p == name));
        if !in_current {
            let known: Vec<String> = self.scene.rest_meshes().into_iter().map(|(n, _)| n).collect();
            bail!("`{op}` needs a mesh, but `{name}` is not one; meshes are {known:?}");
        }
        self.scene.mesh_body.selection = None;
        let mesh = Arc::make_mut(self.scene.mesh_body.mesh.as_mut().expect("checked"));
        edit(mesh)
    }

    fn taken(&self, name: &str) -> bool {
        self.clouds.iter().any(|(n, _)| n == name)
            || self.bodies.iter().any(|(n, _)| n == name)
            || self.scene.mesh_bodies.iter().any(|(n, _)| n == name)
            || self.scene.body_name() == name && self.scene.has_mesh()
    }

    fn mesh_from_cloud(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["cloud", "method"],
            &["depth", "trim", "r", "into"],
            false,
        )?;
        let source = args.text("cloud")?;
        let cloud = self.named_cloud(source, "mesh")?;
        let into = args
            .values
            .get("into")
            .map(|s| s.to_string())
            .unwrap_or_else(|| label_of(line));
        if self.taken(&into) {
            bail!("there is already a body called `{into}`");
        }
        let mut mesh = match args.text("method")? {
            "poisson" => {
                let depth = whole(
                    args.optional_number("depth", &self.scope)?.unwrap_or(8.0),
                    "depth",
                )? as u32;
                let trim = args.optional_number("trim", &self.scope)?.unwrap_or(0.1);
                let surface = poisson(&cloud, PoissonOptions { depth, trim })?;
                let mut mesh = Mesh {
                    labels: vec![0; surface.positions.len()],
                    positions: surface.positions,
                    triangles: surface.triangles,
                    parts: vec![into.clone()],
                    ..Default::default()
                };
                mesh.clean(0.0);
                for _ in 0..3 {
                    if mesh.fill_holes(POISSON_FILL_EDGES) == 0 {
                        break;
                    }
                    mesh.clean(0.0);
                }
                mesh
            }
            "pivot" => {
                let text = args.text("r")?;
                let radii = split_top_level(text, ',')
                    .iter()
                    .map(|p| eval(p, &self.scope))
                    .collect::<Result<Vec<f64>>>()?;
                let triangles = ball_pivoting(&cloud, &radii)?;
                if triangles.is_empty() {
                    bail!("no ball of radius {text} touches three points, try larger radii");
                }
                let mut mesh = Mesh {
                    labels: vec![0; cloud.len()],
                    positions: cloud.positions.clone(),
                    triangles,
                    parts: vec![into.clone()],
                    ..Default::default()
                };
                mesh.compact();
                mesh
            }
            other => bail!("`mesh` reconstructs with `poisson` or `pivot`, not `{other}`"),
        };
        if let Some(colours) = &cloud.colours {
            let tree = KdTree::build(&cloud.positions);
            mesh.vertex_colours = mesh
                .positions
                .iter()
                .map(|&p| {
                    tree.nearest(p, 1)
                        .first()
                        .map_or([200; 3], |&(i, _)| colours[i])
                })
                .collect();
        }
        let report = mesh.summary();
        self.scene.mesh_bodies.push((into, Arc::new(mesh)));
        Ok(report)
    }

    fn mesh_fill(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["mesh"], &["max_edges"], false)?;
        let name = args.text("mesh")?.to_string();
        let max = whole(
            args.optional_number("max_edges", &self.scope)?.unwrap_or(100.0),
            "max_edges",
        )?;
        self.mesh_edit(&name, "fill", |mesh| {
            let filled = mesh.fill_holes(max);
            Ok(format!("filled {filled} hole(s); {}", mesh.summary()))
        })
    }

    fn mesh_clean(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["mesh"], &["islands"], false)?;
        let name = args.text("mesh")?.to_string();
        let share = args
            .optional_number("islands", &self.scope)?
            .unwrap_or(ISLAND_SHARE);
        self.mesh_edit(&name, "clean", |mesh| {
            let done = mesh.clean(share);
            Ok(format!(
                "welded {} vertices, dropped {} degenerate or duplicate, {} non-manifold and {} island triangles; {}",
                done.welded, done.degenerate, done.non_manifold, done.islands, mesh.summary()
            ))
        })
    }

    fn mesh_open_hole(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["mesh"], &["at", "r"], false)?;
        let name = args.text("mesh")?.to_string();
        let at = point3(args.text("at")?, &self.scope)?;
        let radius = args.number("r", &self.scope)?;
        if radius <= 0.0 {
            bail!("`open_hole` needs a positive `r`");
        }
        self.mesh_edit(&name, "open_hole", |mesh| {
            let removed = mesh.open_hole([at.x, at.y, at.z], radius);
            Ok(format!("removed {removed} triangles; {}", mesh.summary()))
        })
    }

    fn mesh_taubin(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["mesh", "mode"], &["iters", "lambda", "mu"], false)?;
        let name = args.text("mesh")?.to_string();
        let iters = whole(
            args.optional_number("iters", &self.scope)?.unwrap_or(10.0),
            "iters",
        )?;
        let lambda = args.optional_number("lambda", &self.scope)?.unwrap_or(TAUBIN_LAMBDA);
        let mu = args.optional_number("mu", &self.scope)?.unwrap_or(TAUBIN_MU);
        if !(lambda > 0.0 && lambda < 1.0 && mu < -lambda) {
            bail!("Taubin smoothing needs 0 < lambda < 1 and mu below -lambda");
        }
        self.mesh_edit(&name, "smooth", |mesh| {
            mesh.taubin(iters, lambda, mu);
            Ok(mesh.summary())
        })
    }

    fn mesh_decimate(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["mesh"], &["to", "ratio"], false)?;
        let name = args.text("mesh")?.to_string();
        let to = args.optional_number("to", &self.scope)?;
        let ratio = args.optional_number("ratio", &self.scope)?;
        if to.is_some() == ratio.is_some() {
            bail!("`decimate` needs one of to=triangles or ratio=share");
        }
        if let Some(r) = ratio
            && !(r > 0.0 && r < 1.0)
        {
            bail!("`ratio` is a share between 0 and 1, got {r}");
        }
        self.mesh_edit(&name, "decimate", |mesh| {
            let before = mesh.triangles.len();
            let target = match (to, ratio) {
                (Some(n), _) => whole(n, "to")?.max(4),
                (_, Some(r)) => ((before as f64 * r).round() as usize).max(4),
                _ => unreachable!(),
            };
            let all = Selection::new(mesh, vec![Region::All], 0.0, None)?;
            let free = Selection {
                weights: vec![1.0; mesh.positions.len()],
                ..all
            };
            mesh.decimate(&free, target);
            Ok(format!("{} triangles (was {before}); {}", mesh.triangles.len(), mesh.summary()))
        })
    }

    fn cloud_target(&mut self, line: &Line, op: &str, cloud: Cloud) -> Result<String> {
        let name = label_of(line);
        if self.taken(&name) {
            bail!("there is already a body called `{name}` for `{op}` to make");
        }
        let report = cloud.summary();
        self.publish_cloud(&name, cloud);
        Ok(report)
    }

    fn mesh_points(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["mesh"], &["every"], false)?;
        let mesh = self
            .mesh_named(args.text("mesh")?)
            .ok_or_else(|| anyhow!("`points` needs a mesh, but `{}` is not one", args.text("mesh").unwrap_or("")))?;
        let every = whole(
            args.optional_number("every", &self.scope)?.unwrap_or(1.0),
            "every",
        )?;
        if every == 0 {
            bail!("`every` must be at least 1");
        }
        let normals = mesh.vertex_normals();
        let layout = gcad_cloud::Layout {
            normals: true,
            colours: !mesh.vertex_colours.is_empty(),
            intensity: false,
            classes: false,
            deviation: false,
        };
        let mut cloud = Cloud::with_layout(layout, mesh.positions.len() / every + 1);
        for i in (0..mesh.positions.len()).step_by(every) {
            let mut point = Point::at(mesh.positions[i]);
            point.normal = Some(normals[i].map(|v| v as f32));
            point.colour = mesh.vertex_colours.get(i).copied();
            cloud.push(point);
        }
        self.cloud_target(line, "points", cloud)
    }

    fn mesh_sample(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["mesh"], &["count", "seed"], false)?;
        let mesh = self
            .mesh_named(args.text("mesh")?)
            .ok_or_else(|| anyhow!("`sample` needs a mesh, but `{}` is not one", args.text("mesh").unwrap_or("")))?;
        let count = whole(args.number("count", &self.scope)?, "count")?;
        let seed = match args.optional_number("seed", &self.scope)? {
            Some(s) => whole(s, "seed")? as u64,
            None => SEED,
        };
        if count == 0 || mesh.triangles.is_empty() {
            bail!("`sample` needs a count of at least 1 and a mesh with triangles");
        }
        let mut running = Vec::with_capacity(mesh.triangles.len());
        let mut total = 0.0;
        for &t in &mesh.triangles {
            let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
            total += length(cross(sub(b, a), sub(c, a))) / 2.0;
            running.push(total);
        }
        let layout = gcad_cloud::Layout {
            normals: true,
            colours: !mesh.vertex_colours.is_empty(),
            intensity: false,
            classes: false,
            deviation: false,
        };
        let mut cloud = Cloud::with_layout(layout, count);
        let mut rng = Random(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0xd1b5_4a32_d192_ed03);
        for _ in 0..count {
            let pick = rng.next() * total;
            let f = running.partition_point(|&v| v < pick).min(mesh.triangles.len() - 1);
            let t = mesh.triangles[f];
            let (mut u, mut v) = (rng.next(), rng.next());
            if u + v > 1.0 {
                (u, v) = (1.0 - u, 1.0 - v);
            }
            let w = [1.0 - u - v, u, v];
            let corners = t.map(|i| mesh.positions[i as usize]);
            let position: [f64; 3] =
                std::array::from_fn(|k| (0..3).map(|c| w[c] * corners[c][k]).sum());
            let n = cross(sub(corners[1], corners[0]), sub(corners[2], corners[0]));
            let len = length(n).max(1.0e-300);
            let mut point = Point::at(position);
            point.normal = Some([(n[0] / len) as f32, (n[1] / len) as f32, (n[2] / len) as f32]);
            if !mesh.vertex_colours.is_empty() {
                let nearest = (0..3).max_by(|&a, &b| w[a].total_cmp(&w[b])).unwrap_or(0);
                point.colour = mesh.vertex_colours.get(t[nearest] as usize).copied();
            }
            cloud.push(point);
        }
        self.cloud_target(line, "sample", cloud)
    }
}
