use super::args::{Args, point3, positive};
use super::{Model, load_model, posed};
use crate::parse::Line;
use crate::sim::fem::STRIDE;
use crate::sim::fem::elastic::{Hold, Load, Push, Support};
use crate::sim::fem::thermal::Boundary;
use crate::sim::{Expect, Kind, Payload, Spec, Sweep};
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

fn flat_axis(solid: &Solid, faces: &[usize]) -> Option<(Point3, Vector3)> {
    let all = crate::select::faces(solid);
    let mut normal: Option<Vector3> = None;
    let mut points = Vec::new();
    for &f in faces {
        let Surface::Plane(plane) = all.get(f)?.oriented_surface() else {
            return None;
        };
        let n = plane.normal();
        if normal.is_some_and(|m: Vector3| m.dot(n) < 1.0 - 1.0e-9) {
            return None;
        }
        normal = Some(n);
        for edge in all[f].edge_iter() {
            points.extend(crate::geometry::curve_samples(&edge.curve()));
        }
    }
    let count = points.len().max(1) as f64;
    let centre = points.iter().fold(Vector3::zero(), |a, p| a + p.to_vec()) / count;
    Some((Point3::from_vec(centre), normal?))
}

pub const SIM_OPERATIONS: &[&str] = &[
    "let",
    "if",
    "include",
    "sim",
    "part",
    "pose",
    "material",
    "gravity",
    "payload",
    "sweep",
    "expect",
    "mesh",
    "fix",
    "slide",
    "load",
    "pressure",
    "probe",
    "temperature",
    "heat",
    "convect",
    "source",
    "modes",
    "wind",
    "air",
    "average",
    "materials",
];

impl Model {
    fn spec(&mut self) -> &mut Spec {
        self.sim.get_or_insert_with(Default::default)
    }

    fn target_mut(&mut self) -> Result<&mut Model> {
        match self.spec().target.as_deref_mut() {
            Some(model) => Ok(model),
            None => bail!("bring in what is simulated first, with `part file.gasm`"),
        }
    }

    fn with_target<T>(&mut self, run: impl FnOnce(&mut Model) -> Result<T>) -> Result<T> {
        let scope = self.scope.clone();
        let target = self.target_mut()?;
        let saved = target.scope.clone();
        target.scope.extend(scope);
        let result = run(target);
        target.scope = saved;
        result
    }

    pub(crate) fn apply_sim(&mut self, line: &Line) -> Result<String> {
        let op = line.op.as_str();
        if !matches!(op, "let" | "if" | "include" | "sim") && self.spec().kind.is_none() {
            bail!(
                "a simulation starts with `sim kind`; kinds are {}",
                crate::sim::KINDS.join(", ")
            );
        }
        match op {
            "let" => self.op_let(line),
            "if" | "include" => self.apply_shared(line),
            "sim" => self.op_sim(line),
            "part" => self.op_sim_part(line),
            "pose" => self.op_sim_pose(line),
            "material" => self.with_target(|t| t.op_material(line)),
            "materials" => {
                let args = Args::new(line, &["file"], &[], false)?;
                let file = self.relative(args.text("file")?);
                self.with_target(|t| t.load_library(&file))
            }
            "gravity" => {
                let args = Args::new(line, &["vector"], &[], false)?;
                let g = point3(args.text("vector")?, &self.scope)?.to_vec();
                self.spec().gravity = Some(g);
                Ok(format!("gravity {},{},{} m/s²", g.x, g.y, g.z))
            }
            "payload" => self.op_payload(line),
            "sweep" => self.op_sim_sweep(line),
            "expect" => self.op_expect(line),
            "fix" => self.op_support(line, Hold::Fixed),
            "slide" => self.op_support(line, Hold::Slide),
            "pin" => {
                let args = Args::new(line, &["faces"], &[], false)?;
                let (point, axis) = self.target_axis(args.text("faces")?)?;
                self.op_support(line, Hold::Pin { point, axis })
            }
            "torque" => self.op_torque(line),
            "load" => self.op_load(line),
            "pressure" => self.op_load(line),
            "probe" => {
                let args = Args::new(line, &["at"], &[], false)?;
                let at = point3(args.text("at")?, &self.scope)?;
                let name = line
                    .label
                    .clone()
                    .ok_or_else(|| anyhow!("name the probe, like `top: probe 100,0,10`"))?;
                self.spec().probes.push((name.clone(), at));
                Ok(format!(
                    "reads `{name}.stress` and `{name}.displacement`, or `{name}.temperature`"
                ))
            }
            "temperature" => {
                let args = Args::new(line, &["faces", "value"], &[], false)?;
                let value = args.number("value", &self.scope)?;
                let faces = self.target_faces(args.text("faces")?)?;
                let label = self.boundary_label(line);
                self.spec().thermal.push(Boundary::Temperature {
                    label: label.clone(),
                    faces,
                    value,
                });
                Ok(format!(
                    "held at {value} °C; the heat leaving reads as `{label}`"
                ))
            }
            "convect" => {
                let args = Args::new(line, &["faces"], &["h", "ambient"], false)?;
                let film = positive(args.number("h", &self.scope)?, "the film coefficient")?;
                let ambient = args
                    .optional_number("ambient", &self.scope)?
                    .unwrap_or(20.0);
                let faces = self.target_faces(args.text("faces")?)?;
                let label = self.boundary_label(line);
                self.spec().thermal.push(Boundary::Convect {
                    label: label.clone(),
                    faces,
                    film,
                    ambient,
                });
                Ok(format!(
                    "{film} W/m²·K to {ambient} °C air; the heat leaving reads as `{label}`"
                ))
            }
            "heat" => {
                let args = Args::new(line, &["faces", "watts"], &[], false)?;
                let watts = args.number("watts", &self.scope)?;
                let faces = self.target_faces(args.text("faces")?)?;
                let label = self.boundary_label(line);
                self.spec().thermal.push(Boundary::Flux {
                    label,
                    faces,
                    watts,
                });
                Ok(format!("{watts} W into the faces"))
            }
            "modes" => {
                let args = Args::new(line, &["count"], &[], false)?;
                let count = args.number("count", &self.scope)?;
                if !(1.0..=50.0).contains(&count) || count.fract() != 0.0 {
                    bail!("`modes` takes a whole number from 1 to 50");
                }
                self.spec().modes = Some(count as usize);
                Ok(format!(
                    "the lowest {count} modes; they read as `mode1`, `mode2`, ..."
                ))
            }
            "warm" => {
                let args = Args::new(line, &["by"], &["from", "part"], false)?;
                let by = args.text("by")?;
                if crate::sim::is_sim(std::path::Path::new(by)) {
                    let reference = args.optional_number("from", &self.scope)?.unwrap_or(20.0);
                    let file = self.relative(by);
                    if !file.is_file() {
                        bail!("no file {}", file.display());
                    }
                    self.spec().warming = Some(crate::sim::Warm::From { file, reference });
                    Ok(format!(
                        "temperatures from {by}, expanding from {reference} °C"
                    ))
                } else {
                    if args.has("from") {
                        bail!("`from=` goes with a heat simulation, like `warm hot.gsim from=20`");
                    }
                    let by = args.number("by", &self.scope)?;
                    let part = args
                        .has("part")
                        .then(|| args.text("part"))
                        .transpose()?
                        .map(str::to_string);
                    if let Some(name) = &part {
                        let names: Vec<String> = self
                            .target_mut()?
                            .named_solids()
                            .into_iter()
                            .map(|(n, _)| n)
                            .collect();
                        if !names.contains(name) {
                            bail!("no part called `{name}`; parts are {names:?}");
                        }
                    }
                    let what = part
                        .as_ref()
                        .map_or("the whole part".to_string(), |n| format!("`{n}`"));
                    let spec = self.spec();
                    match &mut spec.warming {
                        Some(crate::sim::Warm::By(list)) => list.push((part, by)),
                        Some(crate::sim::Warm::From { .. }) => {
                            bail!(
                                "this simulation already takes its temperatures from a heat simulation"
                            )
                        }
                        None => spec.warming = Some(crate::sim::Warm::By(vec![(part, by)])),
                    }
                    Ok(format!(
                        "{what} {} by {} °C",
                        if by < 0.0 { "cools" } else { "warms" },
                        by.abs()
                    ))
                }
            }
            "contact" => {
                let args = Args::new(line, &["a", "b"], &["friction"], false)?;
                let (a, b) = (args.text("a")?.to_string(), args.text("b")?.to_string());
                let friction = args
                    .optional_number("friction", &self.scope)?
                    .unwrap_or(0.0);
                if !(0.0..=2.0).contains(&friction) {
                    bail!(
                        "`friction` is a coefficient from 0 to 2, like 0.3 for dry steel on steel"
                    );
                }
                let names: Vec<String> = self
                    .target_mut()?
                    .named_solids()
                    .into_iter()
                    .map(|(n, _)| n)
                    .collect();
                for part in [&a, &b] {
                    if !names.contains(part) {
                        bail!("no part called `{part}`; parts are {names:?}");
                    }
                }
                self.spec().contacts.push((a.clone(), b.clone(), friction));
                Ok(if friction > 0.0 {
                    format!(
                        "`{a}` and `{b}` press on each other where they touch, slide once the sideways force passes {friction} times the pressing force, and part"
                    )
                } else {
                    format!(
                        "`{a}` and `{b}` press on each other where they touch but can slide and part"
                    )
                })
            }
            "source" => {
                let args = Args::new(line, &["watts"], &[], false)?;
                let watts = args.number("watts", &self.scope)?;
                self.spec().source += watts;
                Ok(format!("{watts} W made evenly through the part"))
            }
            "leave" => {
                let args = Args::new(line, &[], &[], true)?;
                if args.rest.is_empty() {
                    bail!("name the parts to leave out, like `leave ctl* cable`");
                }
                let names: Vec<String> = self
                    .target_mut()?
                    .named_solids()
                    .into_iter()
                    .map(|(n, _)| n)
                    .collect();
                let mut left = Vec::new();
                for pattern in &args.rest {
                    let hits: Vec<&String> = names
                        .iter()
                        .filter(|n| crate::sim::matches(pattern, n))
                        .collect();
                    if hits.is_empty() {
                        bail!("`{pattern}` matches no part; parts are {names:?}");
                    }
                    left.extend(hits.into_iter().cloned());
                    self.spec().leave.push(pattern.to_string());
                }
                Ok(format!("leaves out {}", left.join(", ")))
            }
            "converge" => {
                let args = Args::new(line, &[], &["within"], false)?;
                let within = positive(
                    args.optional_number("within", &self.scope)?.unwrap_or(5.0),
                    "`within`",
                )?;
                if self.spec().kind == Some(crate::sim::Kind::Torque) {
                    bail!("`converge` checks a mesh, and a torque simulation has none");
                }
                self.spec().converge = Some(within);
                Ok(format!(
                    "solves once more on a mesh 1.5 times coarser and flags readings that move more than {within}%"
                ))
            }
            "wind"
                if line
                    .positional
                    .first()
                    .is_some_and(|a| crate::sim::is_sim(std::path::Path::new(a))) =>
            {
                let args = Args::new(line, &["file"], &[], false)?;
                let name = args.text("file")?;
                let file = self.relative(name);
                if !file.is_file() {
                    bail!("no file {}", file.display());
                }
                if self.spec().kind == Some(crate::sim::Kind::Flow) {
                    bail!(
                        "`wind flow.gsim` loads a static simulation; a flow simulation takes `wind speed toward=x,y,z`"
                    );
                }
                self.spec().wind_load = Some(file);
                Ok(format!("the mean surface pressure from {name}"))
            }
            "wind" => {
                let args = Args::new(line, &["speed"], &["toward", "about"], false)?;
                let speed = positive(args.number("speed", &self.scope)?, "the wind speed")?;
                let toward = if args.has("toward") {
                    point3(args.text("toward")?, &self.scope)?.to_vec()
                } else {
                    Vector3::unit_x()
                };
                if toward.magnitude() < 1.0e-9 {
                    bail!("`toward` is the direction the wind blows, like toward=1,0,0");
                }
                let about = if args.has("about") {
                    point3(args.text("about")?, &self.scope)?
                } else {
                    Point3::origin()
                };
                self.spec().wind = Some(crate::sim::wind::Wind {
                    speed,
                    toward: toward.normalize(),
                    about,
                });
                Ok(format!(
                    "{speed} m/s toward {},{},{}; moments about {},{},{}",
                    toward.x, toward.y, toward.z, about.x, about.y, about.z
                ))
            }
            "air" => {
                let args = Args::new(line, &[], &["density", "viscosity"], false)?;
                let (density, viscosity) = crate::sim::wind::AIR;
                let density = positive(
                    args.optional_number("density", &self.scope)?
                        .unwrap_or(density),
                    "the air density",
                )?;
                let viscosity = positive(
                    args.optional_number("viscosity", &self.scope)?
                        .unwrap_or(viscosity),
                    "the kinematic viscosity",
                )?;
                self.spec().air = Some((density, viscosity));
                Ok(format!("{density} kg/m³, {viscosity} m²/s"))
            }
            "average" => {
                let args = Args::new(line, &["passes"], &[], false)?;
                let passes = positive(args.number("passes", &self.scope)?, "`average`")?;
                self.spec().average = Some(passes);
                Ok(format!(
                    "forces averaged while the wind travels {passes} times the width it meets"
                ))
            }
            "mesh" => {
                let args = Args::new(line, &["size"], &[], false)?;
                let size = positive(args.number("size", &self.scope)?, "the mesh size")?;
                self.spec().mesh = Some(size);
                Ok(if self.spec().kind == Some(crate::sim::Kind::Flow) {
                    format!("cells {size} mm across")
                } else {
                    format!("elements about {size} mm across")
                })
            }
            other => bail!(
                "unknown simulation operation `{other}`; simulations use {}",
                SIM_OPERATIONS.join(", ")
            ),
        }
    }

    fn target_faces(&mut self, selector: &str) -> Result<Vec<usize>> {
        let target = self.target_mut()?;
        let names: Vec<String> = target
            .named_solids()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let (body, faces) = if target.assembly || selector.contains(':') {
            let (part, _, faces) = target.reference(selector)?;
            (part, faces)
        } else {
            let tolerance = target.tolerance();
            let solid = target
                .solid
                .as_ref()
                .ok_or_else(|| anyhow!("the part has no solid"))?;
            let faces = crate::select::select_faces(selector, solid, &target.groups, tolerance)?;
            (target.current_body(), faces)
        };
        if faces.is_empty() {
            bail!("`{selector}` picks no faces");
        }
        let index = names
            .iter()
            .position(|n| *n == body)
            .ok_or_else(|| anyhow!("no body called `{body}`"))?;
        Ok(faces.into_iter().map(|f| index * STRIDE + f).collect())
    }

    fn target_axis(&mut self, selector: &str) -> Result<(Point3, Vector3)> {
        let target = self.target_mut()?;
        let (solid, faces) = if target.assembly || selector.contains(':') {
            let (_, solid, faces) = target.reference(selector)?;
            (solid, faces)
        } else {
            let tolerance = target.tolerance();
            let solid = target
                .solid
                .clone()
                .ok_or_else(|| anyhow!("the part has no solid"))?;
            let faces = crate::select::select_faces(selector, &solid, &target.groups, tolerance)?;
            (solid, faces)
        };
        match super::mate::cylinders(&solid, &faces).as_slice() {
            [one] => Ok((one.point, one.axis)),
            [] => flat_axis(&solid, &faces).ok_or_else(|| {
                anyhow!("`{selector}` has no round face or single flat face to find an axis from")
            }),
            many => bail!(
                "`{selector}` has {} round faces on different axes",
                many.len()
            ),
        }
    }

    fn op_torque(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["faces", "moment"], &["axis", "at"], false)?;
        let moment = args.number("moment", &self.scope)?;
        let selector = args.text("faces")?;
        let faces = self.target_faces(selector)?;
        let (point, axis) = match (args.values.get("axis"), args.values.get("at")) {
            (Some(axis), at) => {
                let axis = point3(axis, &self.scope)?.to_vec();
                if axis.magnitude() == 0.0 {
                    bail!("`axis=` needs a direction");
                }
                let point = match at {
                    Some(at) => point3(at, &self.scope)?,
                    None => Point3::origin(),
                };
                (point, axis.normalize())
            }
            (None, None) => self.target_axis(selector)?,
            (None, Some(_)) => bail!("give `axis=` with `at=`"),
        };
        let n = faces.len();
        let label = line.label.clone().unwrap_or_else(|| selector.to_string());
        self.spec().loads.push(Load {
            label,
            faces,
            push: Push::Torque {
                point,
                axis,
                moment: moment * 1000.0,
            },
        });
        Ok(format!(
            "{moment} N·m about {},{},{} through {},{},{} on {n} face(s)",
            crate::plot::label(axis.x),
            crate::plot::label(axis.y),
            crate::plot::label(axis.z),
            crate::plot::label(point.x),
            crate::plot::label(point.y),
            crate::plot::label(point.z)
        ))
    }

    fn boundary_label(&mut self, line: &Line) -> String {
        let count = self.spec().thermal.len() + 1;
        line.label
            .clone()
            .unwrap_or_else(|| format!("{}{count}", line.op))
    }

    fn op_support(&mut self, line: &Line, hold: Hold) -> Result<String> {
        let args = Args::new(line, &["faces"], &[], false)?;
        let faces = self.target_faces(args.text("faces")?)?;
        let count = self.spec().supports.len() + 1;
        let label = line
            .label
            .clone()
            .unwrap_or_else(|| format!("{}{count}", line.op));
        let n = faces.len();
        self.spec().supports.push(Support {
            label: label.clone(),
            faces,
            hold,
        });
        Ok(format!("{n} face(s); its reaction reads as `{label}`"))
    }

    fn op_load(&mut self, line: &Line) -> Result<String> {
        let pressure = line.op == "pressure";
        let args = if pressure {
            Args::new(line, &["faces", "value"], &[], false)?
        } else {
            Args::new(line, &["faces"], &["force"], false)?
        };
        let selector = args.text("faces")?;
        let faces = self.target_faces(selector)?;
        let label = line.label.clone().unwrap_or_else(|| selector.to_string());
        let n = faces.len();
        let push = if pressure {
            let p = args.number("value", &self.scope)?;
            Push::Pressure(p)
        } else {
            let text = args
                .values
                .get("force")
                .ok_or_else(|| anyhow!("write `load faces force=x,y,z` in newtons"))?;
            Push::Force(point3(text, &self.scope)?.to_vec())
        };
        self.spec().loads.push(Load { label, faces, push });
        Ok(match push {
            Push::Force(f) => format!("{} N over {n} face(s)", crate::plot::label(f.magnitude())),
            Push::Pressure(p) => format!("{p} MPa on {n} face(s)"),
            Push::Torque { .. } => String::new(),
        })
    }

    fn op_sim(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["kind"], &[], false)?;
        let kind = Kind::named(args.text("kind")?)?;
        let spec = self.spec();
        if spec.kind.is_some() {
            bail!("the simulation kind is already set");
        }
        spec.kind = Some(kind);
        Ok(format!("{} simulation", kind.name()))
    }

    fn op_sim_part(&mut self, line: &Line) -> Result<String> {
        let [file] = line.positional.as_slice() else {
            bail!("write `part file.gasm [variable=value ...]`");
        };
        if self.spec().target.is_some() {
            bail!("a simulation looks at one file; it already has a `part`");
        }
        let path = self.relative(file);
        let vars = line
            .named
            .iter()
            .map(|(key, text)| Ok((key.clone(), crate::parse::eval(text, &self.scope)?)))
            .collect::<Result<Vec<_>>>()?;
        let model = load_model(&path, &vars, self.depth + 1, &self.cache)?;
        let bodies = model.body_names().len();
        let joints = model.joints.iter().filter(|j| j.movable()).count();
        let spec = self.spec();
        spec.target = Some(Box::new(model));
        spec.file = Some(path);
        Ok(format!("{bodies} bodies, {joints} moving joints"))
    }

    fn op_sim_pose(&mut self, line: &Line) -> Result<String> {
        let joints = self.target_mut()?.joints.clone();
        let said = self.with_target(|t| t.op_pose(line))?;
        let after = self.target_mut()?.rig().values();
        let moves = posed(&joints, &after);
        for payload in &mut self.spec().payloads {
            if let (Some(at), Some(m)) = (payload.at, moves.get(&payload.part)) {
                payload.at = Some(m.transform_point(at));
            }
        }
        Ok(said)
    }

    fn op_payload(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &["part", "grams"], &["at"], false)?;
        let part = args.text("part")?.to_string();
        let grams = positive(args.number("grams", &self.scope)?, "the payload's mass")?;
        let at = match args.values.get("at") {
            Some(text) => Some(point3(text, &self.scope)?),
            None => None,
        };
        let names = self.target_mut()?.body_names();
        if !names.contains(&part) {
            bail!("no part called `{part}`; parts are {names:?}");
        }
        let place = match at {
            Some(p) => format!("at {},{},{}", p.x, p.y, p.z),
            None => "at its centre of mass".to_string(),
        };
        self.spec().payloads.push(Payload {
            part: part.clone(),
            grams,
            at,
        });
        Ok(format!("{grams} g on `{part}` {place}"))
    }

    fn op_sim_sweep(&mut self, line: &Line) -> Result<String> {
        let args = Args::new(line, &[], &["steps"], true)?;
        let steps = match args.optional_number("steps", &self.scope)? {
            Some(n) if n >= 1.0 && n.fract() == 0.0 && n <= 1000.0 => n as usize,
            Some(n) => bail!("`steps={n}` must be a whole number from 1 to 1000"),
            None => 24,
        };
        if args.rest.is_empty() {
            bail!("write `sweep joint [joint ...] steps=24`");
        }
        let rig = self.target_mut()?.rig();
        let joints = args
            .rest
            .iter()
            .map(|name| {
                let index = rig
                    .joints
                    .iter()
                    .position(|j| j.name == *name)
                    .ok_or_else(|| anyhow!("no joint called `{name}`"))?;
                if !rig.joints[index].movable() {
                    bail!("`{name}` does not move");
                }
                if let Some(couple) = rig.driver_of(name) {
                    bail!("`{name}` follows `{}`; sweep that instead", couple.driver);
                }
                Ok(index)
            })
            .collect::<Result<Vec<_>>>()?;
        let poses = (steps + 1).pow(joints.len() as u32);
        if poses > 100_000 {
            bail!("that is {poses} poses; use fewer steps or fewer joints on one line");
        }
        self.spec().sweeps.push(Sweep { joints, steps });
        Ok(format!("{poses} poses"))
    }

    fn op_expect(&mut self, line: &Line) -> Result<String> {
        if !line.named.is_empty() || line.positional.is_empty() {
            bail!(
                "write `expect reading<limit` or `expect reading>limit`, like `expect shoulder<2`"
            );
        }
        let scope = self.scope.clone();
        let parsed = line
            .positional
            .iter()
            .map(|word| {
                let at = word
                    .find(['<', '>'])
                    .ok_or_else(|| anyhow!("`{word}` needs `<` or `>`"))?;
                let below = word[at..].starts_with('<');
                let reading = word[..at].to_string();
                let limit_text = word[at + 1..].trim_start_matches('=');
                if reading.is_empty() || limit_text.is_empty() {
                    bail!("write `reading<limit`, not `{word}`");
                }
                let limit = crate::parse::eval(limit_text, &scope)?;
                Ok(Expect {
                    text: word.clone(),
                    reading,
                    below,
                    limit,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let count = parsed.len();
        self.spec().expects.extend(parsed);
        Ok(format!("{count} check(s)"))
    }
}
