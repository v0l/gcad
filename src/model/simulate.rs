use super::args::{Args, point3, positive};
use super::{Model, load_model, posed};
use crate::parse::Line;
use crate::sim::{Expect, Kind, Payload, Spec, Sweep};
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;

pub const SIM_OPERATIONS: &[&str] = &[
    "let", "if", "include", "sim", "part", "pose", "material", "gravity", "payload", "sweep",
    "expect",
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
            "gravity" => {
                let args = Args::new(line, &["vector"], &[], false)?;
                let g = point3(args.text("vector")?, &self.scope)?.to_vec();
                self.spec().gravity = Some(g);
                Ok(format!("gravity {},{},{} m/s²", g.x, g.y, g.z))
            }
            "payload" => self.op_payload(line),
            "sweep" => self.op_sim_sweep(line),
            "expect" => self.op_expect(line),
            other => bail!(
                "unknown simulation operation `{other}`; simulations use {}",
                SIM_OPERATIONS.join(", ")
            ),
        }
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
