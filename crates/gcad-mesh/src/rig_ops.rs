use crate::args::{Args, direction, point3, positive};
use crate::interp::{Scene, mirrored_name};
use crate::mesh::rig::{self, Bone, Clip, IDENTITY_QUAT, Key, MoveKey, Quat, Rig};
use crate::mesh::skin_heat;
use crate::mesh::skin_paint;
use crate::mesh::skin_smooth::{self, SmoothOptions};
use crate::mesh::{self, P3};
use crate::parse::{Line, Scope};
use anyhow::{Result, anyhow, bail};

fn passes_of(args: &Args<'_>, scope: &Scope, default: usize) -> Result<usize> {
    match args.optional_number("passes", scope)? {
        Some(p) if p >= 0.0 && p.fract() == 0.0 && p <= 10000.0 => Ok(p as usize),
        Some(p) => bail!("passes is a whole number from 0 to 10000, got {p}"),
        None => Ok(default),
    }
}

fn step_of(args: &Args<'_>, scope: &Scope) -> Result<f64> {
    match args.optional_number("step", scope)? {
        Some(s) if s > 0.0 && s <= 1.0 => Ok(s),
        Some(s) => bail!("step is above 0 and at most 1, got {s}"),
        None => Ok(SmoothOptions::default().step),
    }
}

fn flag(args: &Args<'_>, name: &str) -> Result<bool> {
    match args.values.get(name).copied() {
        None | Some("false") | Some("0") => Ok(false),
        Some("true") | Some("1") => Ok(true),
        Some(other) => bail!("{name} is true or false, got `{other}`"),
    }
}

fn quat_mul(a: Quat, b: Quat) -> Quat {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

impl Scene {
    pub fn rig_of(&self, body: &str) -> Option<&Rig> {
        if let Some((_, rig)) = self.mesh_rigs.iter().find(|(name, _)| name == body) {
            return Some(rig);
        }
        (body == self.body_name() && !self.mesh_body.rig.is_empty()).then_some(&self.mesh_body.rig)
    }

    pub fn rigs(&self) -> Vec<(String, &Rig)> {
        let mut out: Vec<(String, &Rig)> = self
            .mesh_rigs
            .iter()
            .map(|(name, rig)| (name.clone(), rig))
            .collect();
        if !self.mesh_body.rig.is_empty() {
            out.push((self.body_name(), &self.mesh_body.rig));
        }
        out
    }

    fn rest_mesh_for_rig(&mut self, op: &str) -> Result<mesh::Mesh> {
        if self.mesh_body.rig.is_empty() {
            bail!("`{op}` needs bones; add them with `bone name head=x,y,z tail=x,y,z`");
        }
        Ok(self.sculpted(op)?.clone())
    }

    pub fn op_bone(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        self.start_mesh("bone")?;
        let args = Args::new(line, &["name"], &["parent", "head", "tail"], false)?;
        let name = args.text("name")?.to_string();
        let rig = &self.mesh_body.rig;
        if rig.bone_index(&name).is_some() {
            bail!("there is already a bone called `{name}`");
        }
        let parent = args.values.get("parent").map(|p| rig.bone(p)).transpose()?;
        let tail = point3(args.text("tail")?, scope)?;
        let head = match args.values.get("head") {
            Some(text) => point3(text, scope)?,
            None => match parent {
                Some(p) => rig.bones[p].tail,
                None => bail!("a bone with no parent needs `head=x,y,z`"),
            },
        };
        if mesh::length(mesh::sub(tail, head)) < 1.0e-9 {
            bail!("bone `{name}` has its head and tail at the same point");
        }
        let skinned = self.mesh_body.rig.skin.is_some();
        self.mesh_body.rig.bones.push(Bone {
            name,
            parent,
            head,
            tail,
        });
        Ok(format!(
            "{} bone(s){}",
            self.mesh_body.rig.bones.len(),
            if skinned {
                "; the new bone has no skin weights until `skin` binds something to it"
            } else {
                ""
            }
        ))
    }

    pub fn mirror_bone(&mut self, axis: usize, at: f64, of: &str) -> Result<String> {
        let rig = &self.mesh_body.rig;
        let root = rig.bone(of)?;
        let mut chain = vec![root];
        let mut next = 0;
        while next < chain.len() {
            chain.extend(rig.children(chain[next]));
            next += 1;
        }
        let flip = |p: P3| {
            let mut q = p;
            q[axis] = 2.0 * at - q[axis];
            q
        };
        let mut added: Vec<Bone> = Vec::new();
        let mut index_of = std::collections::HashMap::new();
        for &i in &chain {
            let bone = &rig.bones[i];
            let name = mirrored_name(&bone.name, axis).ok_or_else(|| {
                anyhow!(
                    "bone `{}` has no other side: name it with an `_l` or `_r` ending and mirror across x",
                    bone.name
                )
            })?;
            if rig.bone_index(&name).is_some() {
                bail!("there is already a bone called `{name}`");
            }
            let parent = match bone.parent {
                Some(p) if index_of.contains_key(&p) => Some(index_of[&p]),
                Some(p) => Some(
                    mirrored_name(&rig.bones[p].name, axis)
                        .and_then(|m| rig.bone_index(&m))
                        .unwrap_or(p),
                ),
                None => None,
            };
            index_of.insert(i, rig.bones.len() + added.len());
            added.push(Bone {
                name,
                parent,
                head: flip(bone.head),
                tail: flip(bone.tail),
            });
        }
        let count = added.len();
        self.mesh_body.rig.bones.extend(added);
        Ok(format!(
            "{count} bone(s) mirrored; {} bone(s) in the skeleton",
            self.mesh_body.rig.bones.len()
        ))
    }

    fn rotation_quat(&self, scope: &Scope, args: &Args<'_>) -> Result<Quat> {
        let text = args.text("rotate")?;
        let parts = crate::parse::split_top_level(text, ',');
        if parts.len() == 3 {
            let angle = |k: usize| crate::parse::eval(parts[k], scope);
            let q = |axis: P3, deg: f64| rig::quat_axis_angle(axis, deg);
            let x = q([1.0, 0.0, 0.0], angle(0)?)?;
            let y = q([0.0, 1.0, 0.0], angle(1)?)?;
            let z = q([0.0, 0.0, 1.0], angle(2)?)?;
            return Ok(quat_mul(z, quat_mul(y, x)));
        }
        let angle = args.number("rotate", scope)?;
        let axis = direction(
            args.values.get("axis").copied().ok_or_else(|| {
                anyhow!("`rotate=angle` needs `axis=x|y|z|x,y,z`, or give `rotate=rx,ry,rz`")
            })?,
            scope,
        )?;
        rig::quat_axis_angle(axis, angle)
    }

    pub fn op_skin(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["mode"],
            &[
                "part",
                "bone",
                "sphere",
                "r",
                "falloff",
                "strength",
                "power",
                "method",
                "passes",
                "core",
                "unrelated",
                "step",
                "zone",
                "heat",
                "cut",
                "snap",
                "mirror",
                "from",
                "joint",
            ],
            true,
        )?;
        let mesh = self.rest_mesh_for_rig("skin")?;
        let mode = args.values.get("mode").copied();
        if !args.rest.is_empty() && !matches!(mode, Some("paint") | Some("smooth")) {
            bail!(
                "only `skin paint` and `skin smooth` take more points after `sphere=`, got `{}`",
                args.rest[0]
            );
        }
        let rig = &mut self.mesh_body.rig;
        match mode {
            Some("auto") => {
                let power = args.optional_number("power", scope)?.unwrap_or(4.0);
                positive(power, "power")?;
                let method = args.values.get("method").copied().unwrap_or("heat");
                if method == "distance" {
                    rig.auto_skin(&mesh, power);
                    return Ok(self.skin_summary(&mesh));
                }
                let defaults = SmoothOptions::default();
                let options = SmoothOptions {
                    power,
                    passes: passes_of(&args, scope, defaults.passes)?,
                    step: step_of(&args, scope)?,
                    core: match args.optional_number("core", scope)? {
                        Some(c) if (0.0..=1.0).contains(&c) => c,
                        Some(c) => bail!("core is a fraction of each bone, 0 to 1, got {c}"),
                        None => defaults.core,
                    },
                    unrelated: flag(&args, "unrelated")?,
                    zone: match args.optional_number("zone", scope)? {
                        Some(z) if z >= 0.0 => z,
                        Some(z) => {
                            bail!("zone is a fraction of a bone's length, 0 or more, got {z}")
                        }
                        None => defaults.zone,
                    },
                    joint: match args.optional_number("joint", scope)? {
                        Some(j) if j >= 0.0 => j,
                        Some(j) => {
                            bail!("joint is a multiple of the limb's thickness, 0 or more, got {j}")
                        }
                        None => defaults.joint,
                    },
                };
                match method {
                    "heat" => {
                        let strength = match args.optional_number("heat", scope)? {
                            Some(h) if h > 0.0 => h,
                            Some(h) => bail!("heat is above 0, got {h}"),
                            None => skin_heat::HEAT,
                        };
                        let cut = match args.optional_number("cut", scope)? {
                            Some(c) if (0.0..0.5).contains(&c) => c,
                            Some(c) => bail!("cut is from 0 to below 0.5, got {c}"),
                            None => skin_heat::CUT,
                        };
                        let snap = match args.optional_number("snap", scope)? {
                            Some(s) if (0.0..=1.0).contains(&s) => s,
                            Some(s) => bail!("snap is from 0 to 1, got {s}"),
                            None => skin_heat::SNAP,
                        };
                        skin_heat::heat_core_skin(rig, &mesh, strength, cut, snap, options);
                    }
                    "smooth" => skin_smooth::smooth_skin(rig, &mesh, options),
                    other => {
                        bail!("`skin auto method=` takes smooth, heat or distance, got `{other}`")
                    }
                }
                Ok(self.skin_summary(&mesh))
            }
            Some("smooth") => {
                if rig.skin.is_none() {
                    bail!(
                        "`skin smooth` smooths weights that are there; bind them first with `skin auto`"
                    );
                }
                let passes = passes_of(&args, scope, 10)?;
                let step = step_of(&args, scope)?;
                let bone = args.values.get("bone").map(|b| rig.bone(b)).transpose()?;
                let sphere = match args.values.get("sphere") {
                    Some(text) => {
                        let mut points = vec![point3(text, scope)?];
                        for text in &args.rest {
                            points.push(point3(text, scope)?);
                        }
                        Some((points, positive(args.number("r", scope)?, "r")?))
                    }
                    None if !args.rest.is_empty() => {
                        bail!("`skin smooth` takes extra points only after `sphere=`")
                    }
                    None => None,
                };
                let mirror = match args.values.get("mirror").copied() {
                    None => false,
                    Some("x") => true,
                    Some(other) => bail!("`skin smooth mirror=` takes x, got `{other}`"),
                };
                let sphere = sphere.map(|(mut points, r)| {
                    if mirror {
                        let flipped: Vec<P3> = points.iter().map(|p| [-p[0], p[1], p[2]]).collect();
                        points.extend(flipped);
                    }
                    (points, r)
                });
                let skin = rig.skin.as_ref().expect("skinned");
                let inside = sphere
                    .as_ref()
                    .map(|(points, r)| skin_paint::stroke_region(&mesh, points, *r));
                let region: Vec<bool> = skin
                    .iter()
                    .enumerate()
                    .map(|(v, w)| {
                        bone.is_none_or(|b| w.iter().any(|(x, val)| *x == b && *val > 0.0))
                            && inside.as_ref().is_none_or(|i| i[v])
                    })
                    .collect();
                let touched = skin_smooth::smooth_existing(rig, &mesh, &region, passes, step);
                Ok(format!(
                    "smoothed {touched} vertices over {passes} pass(es); {}",
                    self.skin_summary(&mesh)
                ))
            }
            Some("paint") => {
                let bone = rig.bone(args.text("bone")?)?;
                let mut points = vec![point3(args.text("sphere")?, scope)?];
                for text in &args.rest {
                    points.push(point3(text, scope)?);
                }
                let r = positive(args.number("r", scope)?, "r")?;
                let falloff = args.optional_number("falloff", scope)?.unwrap_or(0.0);
                if falloff < 0.0 {
                    bail!("falloff must be 0 or more, got {falloff}");
                }
                let strength = args.optional_number("strength", scope)?.unwrap_or(1.0);
                if !(-1.0..=1.0).contains(&strength) {
                    bail!("strength is from -1 to 1, got {strength}");
                }
                let mirror = match args.values.get("mirror").copied() {
                    None => false,
                    Some("x") => true,
                    Some(other) => bail!("`skin paint mirror=` takes x, got `{other}`"),
                };
                let mut strokes = vec![skin_paint::Stroke {
                    bone,
                    points,
                    radius: r,
                    falloff,
                    strength,
                }];
                if mirror {
                    let other = skin_paint::mirror_bone(rig, bone, 0);
                    let points = strokes[0]
                        .points
                        .iter()
                        .map(|p| [-p[0], p[1], p[2]])
                        .collect();
                    strokes.push(skin_paint::Stroke {
                        bone: other,
                        points,
                        ..strokes[0].clone()
                    });
                }
                let touched: usize = strokes
                    .iter()
                    .map(|s| skin_paint::paint_stroke(rig, &mesh, s))
                    .sum();
                Ok(format!(
                    "painted {touched} vertices; {}",
                    self.skin_summary(&mesh)
                ))
            }
            Some("mirror") => {
                let from_positive = match args.values.get("from").copied() {
                    None | Some("+x") | Some("x") => true,
                    Some("-x") => false,
                    Some(other) => bail!("`skin mirror from=` takes +x or -x, got `{other}`"),
                };
                if rig.skin.is_none() {
                    bail!(
                        "`skin mirror` copies weights that are there; bind them first with `skin auto`"
                    );
                }
                let n = skin_paint::mirror_skin(rig, &mesh, 0, from_positive);
                Ok(format!(
                    "mirrored {n} vertices across x, `_l` bones to `_r`; {}",
                    self.skin_summary(&mesh)
                ))
            }
            Some("normalize") | Some("normalise") => {
                if rig.skin.is_none() {
                    bail!("`skin normalize` needs weights; bind them first with `skin auto`");
                }
                let n = skin_paint::normalise(rig);
                Ok(format!(
                    "normalised {n} vertices to sum 1, at most 4 bones, none under 0.01; {}",
                    self.skin_summary(&mesh)
                ))
            }
            None => {
                let part = args.text("part")?;
                let Some(label) = mesh.parts.iter().position(|p| p == part) else {
                    bail!(
                        "the mesh has no part `{part}`; its parts are {}",
                        mesh.parts.join(", ")
                    );
                };
                let bone = rig.bone(args.text("bone")?)?;
                let bound = rig.bind_part(&mesh, label as u16, bone);
                Ok(format!(
                    "bound {bound} vertices of `{part}` to `{}`",
                    self.mesh_body.rig.bones[bone].name
                ))
            }
            Some(other) => {
                bail!(
                    "`skin` takes auto, smooth, paint, mirror, normalize, or `part=name bone=name`, got `{other}`"
                )
            }
        }
    }

    fn skin_summary(&self, mesh: &mesh::Mesh) -> String {
        let skin = self.mesh_body.rig.skin.as_ref().expect("skinned");
        let most = skin.iter().map(Vec::len).max().unwrap_or(0);
        let bound = skin.iter().filter(|w| !w.is_empty()).count();
        format!(
            "{bound} of {} vertices skinned to {} bone(s), at most {most} influences each",
            mesh.positions.len(),
            self.mesh_body.rig.bones.len()
        )
    }

    pub fn op_mesh_pose(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        let args = Args::new(
            line,
            &["mode"],
            &["bone", "rotate", "axis", "clip", "t"],
            false,
        )?;
        if self.mesh_body.rig.is_empty() {
            bail!("`pose` needs bones; add them with `bone name head=x,y,z tail=x,y,z`");
        }
        match args.values.get("mode").copied() {
            Some("reset") => {
                self.mesh_body.rig.pose.clear();
                self.mesh_body.rig.moved.clear();
                return self.pose_summary("pose reset");
            }
            Some(other) => bail!("`pose` takes `reset`, `bone=`, or `clip=`, got `{other}`"),
            None => {}
        }
        if let Some(name) = args.values.get("clip") {
            let rig = &mut self.mesh_body.rig;
            let clip = rig.clip(name)?;
            let t = args.optional_number("t", scope)?.unwrap_or(0.0);
            rig.pose = rig.sample(clip, t);
            rig.moved = rig.sample_moves(clip, t);
            return self.pose_summary(&format!("pose of clip `{name}` at {t} s"));
        }
        let name = args.text("bone")?;
        let bone = self.mesh_body.rig.bone(name)?;
        let q = self.rotation_quat(scope, &args)?;
        self.mesh_body.rig.pose.insert(bone, q);
        self.pose_summary(&format!("bone `{name}` posed"))
    }

    fn pose_summary(&mut self, what: &str) -> Result<String> {
        let rest = self.sculpted("pose")?.clone();
        let rig = &self.mesh_body.rig;
        if rig.skin.is_some() && !rig.skin_fits(&rest) {
            bail!(
                "the mesh changed its vertex count after `skin`; sculpt first, then bind the skin"
            );
        }
        let posed = rig.posed(&rest, &rig.pose);
        Ok(format!(
            "{what}; volume {:.3}{}",
            posed.volume(),
            if rig.skin.is_none() {
                " (no skin yet, the mesh does not follow; `skin auto`)"
            } else {
                ""
            }
        ))
    }

    pub fn op_clip(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        self.start_mesh("clip")?;
        let args = Args::new(line, &["name"], &["fps"], false)?;
        let name = args.text("name")?.to_string();
        let fps = positive(args.optional_number("fps", scope)?.unwrap_or(24.0), "fps")?;
        let rig = &mut self.mesh_body.rig;
        if rig.clips.iter().any(|c| c.name == name) {
            bail!("there is already a clip called `{name}`");
        }
        rig.clips.push(Clip {
            name: name.clone(),
            fps,
            keys: Vec::new(),
            moves: Vec::new(),
        });
        self.mesh_body.clip = Some(rig.clips.len() - 1);
        Ok(format!(
            "clip `{name}` at {fps} fps; add `key t= bone= rotate=`"
        ))
    }

    pub fn op_key(&mut self, scope: &Scope, line: &Line) -> Result<String> {
        self.start_mesh("key")?;
        let args = Args::new(line, &[], &["t", "bone", "rotate", "axis", "move"], false)?;
        let Some(clip) = self.mesh_body.clip else {
            bail!("`key` adds to a clip; start one with `clip name fps=24`");
        };
        let t = args.number("t", scope)?;
        if t < 0.0 {
            bail!("a key's time is 0 or more, got {t}");
        }
        let bone = self.mesh_body.rig.bone(args.text("bone")?)?;
        let offset = args
            .values
            .get("move")
            .map(|m| point3(m, scope))
            .transpose()?;
        let rotation = if args.has("rotate") || offset.is_none() {
            Some(if args.has("rotate") {
                self.rotation_quat(scope, &args)?
            } else {
                IDENTITY_QUAT
            })
        } else {
            None
        };
        let clip = &mut self.mesh_body.rig.clips[clip];
        if let Some(rotation) = rotation {
            clip.keys
                .retain(|k| !(k.bone == bone && (k.t - t).abs() < 1.0e-9));
            clip.keys.push(Key { t, bone, rotation });
        }
        if let Some(offset) = offset {
            clip.moves
                .retain(|k| !(k.bone == bone && (k.t - t).abs() < 1.0e-9));
            clip.moves.push(MoveKey { t, bone, offset });
        }
        Ok(format!(
            "clip `{}` has {} key(s), {} s long",
            clip.name,
            clip.keys.len() + clip.moves.len(),
            clip.duration()
        ))
    }
}
