use anyhow::{Context, Result, anyhow, bail};
use gcad_sims_proto::{self as proto, ApiError, Job, JobState, Submit};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use proto::DEFAULT_SERVER;
const POLL: Duration = Duration::from_secs(2);

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Saved {
    server: Option<String>,
    token: Option<String>,
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("gcad").join("remote.toml"))
}

fn saved() -> Saved {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(server: &str, token: &str) -> Result<PathBuf> {
    let path = config_path().ok_or_else(|| anyhow!("there is no HOME to keep the token in"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string(&Saved {
        server: Some(server.to_string()),
        token: Some(token.to_string()),
    })?;
    std::fs::write(&path, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

fn server_of(given: Option<&str>) -> String {
    given
        .map(str::to_string)
        .or_else(|| std::env::var("GCAD_SIMS_SERVER").ok())
        .or_else(|| saved().server)
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
        .trim_end_matches('/')
        .to_string()
}

fn explain(error: ureq::Error) -> anyhow::Error {
    match error {
        ureq::Error::Status(code, response) => {
            let text = response.into_string().unwrap_or_default();
            let reply: ApiError = serde_json::from_str(&text).unwrap_or(ApiError {
                error: text,
                ..Default::default()
            });
            let mut out = format!("the sims server said: {}", reply.error);
            if code == 402
                && let Some(shop) = reply.shop_url
            {
                out.push_str(&format!(
                    "\ntop up at {shop} with the id from `gcad remote account`"
                ));
            }
            anyhow!(out)
        }
        other => anyhow!("cannot reach the sims server: {other}"),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(300))
        .build()
}

fn read<T: DeserializeOwned>(response: ureq::Response) -> Result<T> {
    response
        .into_json()
        .context("reading the sims server's reply")
}

fn status_at(server: &str) -> Result<proto::Status> {
    read(
        agent()
            .get(&format!("{server}/api/status"))
            .call()
            .map_err(explain)?,
    )
}

pub struct Client {
    agent: ureq::Agent,
    server: String,
    token: String,
}

impl Client {
    pub fn open() -> Result<Client> {
        let token = std::env::var("GCAD_SIMS_TOKEN")
            .ok()
            .or_else(|| saved().token)
            .ok_or_else(|| {
                anyhow!(
                    "no sims account yet; run `gcad remote signup` or `gcad remote login TOKEN`"
                )
            })?;
        Ok(Client {
            agent: agent(),
            server: server_of(None),
            token,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/{path}", self.server)
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        read(
            self.agent
                .get(&self.url(path))
                .set("Authorization", &format!("Bearer {}", self.token))
                .call()
                .map_err(explain)?,
        )
    }

    fn post<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        read(
            self.agent
                .post(&self.url(path))
                .set("Authorization", &format!("Bearer {}", self.token))
                .call()
                .map_err(explain)?,
        )
    }

    fn download(&self, job: &str, name: &str) -> Result<Vec<u8>> {
        let response = self
            .agent
            .get(&self.url(&format!("jobs/{job}/files/{name}")))
            .set("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(explain)?;
        let mut bytes = Vec::new();
        response.into_reader().read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

fn euros(cents: i64) -> String {
    format!("€{}.{:02}", cents / 100, cents % 100)
}

fn duration(seconds: f64) -> String {
    let s = seconds.round() as u64;
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{}m{:02}s", s / 60, s % 60),
        _ => format!("{}h{:02}m", s / 3600, s % 3600 / 60),
    }
}

pub fn signup(server: Option<&str>) -> Result<()> {
    if saved().token.is_some() && server.is_none() {
        bail!(
            "there is already an account in {}; `gcad remote account` shows it",
            config_path().unwrap_or_default().display()
        );
    }
    let server = server_of(server);
    let made: proto::NewAccount = read(
        agent()
            .post(&format!("{server}/api/accounts"))
            .call()
            .map_err(explain)?,
    )?;
    let path = save(&server, &made.token)?;
    println!("account {} on {server}", made.id);
    println!("token saved in {}", path.display());
    println!("top it up with that account id, then run `gcad sim FILE --remote`");
    Ok(())
}

pub fn login(token: &str, server: Option<&str>) -> Result<()> {
    let server = server_of(server);
    let path = save(&server, token)?;
    let me: proto::Account = Client::open()?.get("account")?;
    println!("account {} on {server}, saved in {}", me.id, path.display());
    Ok(())
}

pub fn account() -> Result<()> {
    let client = Client::open()?;
    let me: proto::Account = client.get("account")?;
    println!("account  {}", me.id);
    println!("server   {}", client.server);
    println!(
        "balance  {}{}",
        euros(me.balance_cents),
        if me.held_cents > 0 {
            format!(" ({} held by running jobs)", euros(me.held_cents))
        } else {
            String::new()
        }
    );
    println!("runners  `gcad remote runners` lists them and their prices");
    if let Some(shop) = me.shop_url {
        println!("top up   {shop}");
    }
    Ok(())
}

pub fn jobs() -> Result<()> {
    let list: proto::Jobs = Client::open()?.get("jobs")?;
    if list.jobs.is_empty() {
        println!("no jobs yet");
    }
    for job in list.jobs {
        println!(
            "{}  {:<10} {:<24} {:<12} {:>8} {:>7}  {}",
            job.id,
            job.state.name(),
            job.spec,
            job.runner.as_deref().unwrap_or("-"),
            job.seconds.map(duration).unwrap_or_default(),
            job.charged_cents.map(euros).unwrap_or_default(),
            job.error.as_deref().unwrap_or_default()
        );
    }
    Ok(())
}

pub fn cancel(id: &str) -> Result<()> {
    let job: Job = Client::open()?.post(&format!("jobs/{id}/cancel"))?;
    println!("{id} {}", job.state.name());
    Ok(())
}

fn hash_of(name: &str, stamp: &[(String, String)]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    for (file, hash) in stamp {
        hasher.update([0]);
        hasher.update(file.as_bytes());
        hasher.update([0]);
        hasher.update(hash.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub struct Limits {
    pub minutes: u64,
    pub price_cents: Option<u64>,
}

pub fn simulate(
    file: &Path,
    vars: &[(String, f64)],
    limits: &Limits,
    build: impl FnOnce() -> Result<crate::model::Run>,
) -> Result<()> {
    let client = Client::open()?;
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .context("the spec has no file name")?;
    let stamp = crate::sim::stamp(file, vars);
    let hash = hash_of(&name, &stamp);
    let found: proto::Jobs = client.get(&format!("jobs?hash={hash}"))?;
    let fresh_here = crate::sim::stale(file, vars).is_none();
    let existing = found.jobs.into_iter().find(|j| match j.state {
        JobState::Succeeded => !fresh_here,
        state => state.active(),
    });
    let job = match existing {
        Some(job) => {
            eprintln!("picking up job {} ({})", job.id, job.state.name());
            job
        }
        None => submit(
            &client,
            file,
            Submit {
                spec: name,
                version: env!("GCAD_BUILD_VERSION").to_string(),
                hash,
                max_seconds: limits.minutes * 60,
                max_price: None,
                vars: vars.to_vec(),
            },
            &stamp,
            limits,
            build,
        )?,
    };
    let done = wait(&client, &job.id)?;
    finish(&client, file, &done)
}

fn submit(
    client: &Client,
    file: &Path,
    mut submit: Submit,
    stamp: &[(String, String)],
    limits: &Limits,
    build: impl FnOnce() -> Result<crate::model::Run>,
) -> Result<Job> {
    let price = ready(client, limits)?;
    submit.max_price = Some(price as i64);
    submit.check().map_err(|e| anyhow!(e))?;
    let run = build()?;
    let spec = run
        .model
        .sim
        .as_deref()
        .context("a .gsim file runs as a simulation")?;
    let packed = crate::sim::job::pack(spec, stamp.to_vec())?;
    eprintln!(
        "sending {} ({} kB) to {}",
        file.display(),
        packed.len().div_ceil(1024),
        client.server
    );
    let job: Job = read(
        client
            .agent
            .post(&client.url(&format!("jobs?{}", submit.query())))
            .set("Authorization", &format!("Bearer {}", client.token))
            .set("Content-Type", "application/gzip")
            .send_bytes(&packed)
            .map_err(explain)?,
    )?;
    eprintln!(
        "job {} holds {} for up to {} minutes at up to {} an hour; if this stops, rerun the same command to pick it up, or `gcad remote cancel {}`",
        job.id,
        euros(job.held_cents),
        limits.minutes,
        euros(price as i64),
        job.id
    );
    Ok(job)
}

fn ready(client: &Client, limits: &Limits) -> Result<u64> {
    let version = env!("GCAD_BUILD_VERSION");
    let status = status_at(&client.server)?;
    let able: Vec<&proto::Runner> = status
        .runners
        .iter()
        .filter(|r| r.version == version)
        .collect();
    if able.is_empty() {
        bail!(
            "no remote runner runs gcad {version}{}",
            if status.versions.is_empty() {
                String::new()
            } else {
                format!(" (they run {})", status.versions.join(", "))
            }
        );
    }
    let price_of = |r: &&proto::Runner| r.price_cents_per_hour.max(0) as u64;
    let cheapest = able.iter().map(price_of).min().unwrap_or(0);
    let price = match limits.price_cents {
        Some(cap) if cap < cheapest => bail!(
            "the cheapest runner on gcad {version} asks {} an hour, above --max-price {}; `gcad remote runners` lists them",
            euros(cheapest as i64),
            euros(cap as i64)
        ),
        Some(cap) => cap,
        None => able.iter().map(price_of).max().unwrap_or(0),
    };
    let me: proto::Account = client.get("account")?;
    let hold = (limits.minutes * price).div_ceil(60) as i64;
    if me.balance_cents < hold {
        bail!(
            "a {} minute run at up to {} an hour holds {}, the balance is {}; top up at {} with account id {}, or lower --max-minutes or --max-price",
            limits.minutes,
            euros(price as i64),
            euros(hold),
            euros(me.balance_cents),
            me.shop_url.as_deref().unwrap_or("the shop"),
            me.id
        );
    }
    Ok(price)
}

pub fn runners() -> Result<()> {
    let status = status_at(&server_of(None))?;
    if status.runners.is_empty() {
        println!("no runner has been seen in the last day");
    }
    let number = |value: Option<f64>, unit: &str| {
        value
            .map(|v| format!("{v} {unit}"))
            .unwrap_or_else(|| "-".into())
    };
    for runner in &status.runners {
        let info = &runner.info;
        println!(
            "{:<12} {}/h  gcad {}  {}{}",
            runner.name,
            euros(runner.price_cents_per_hour),
            runner.version,
            match (runner.online, runner.busy) {
                (true, true) => "busy",
                (true, false) => "idle",
                _ => "offline",
            },
            if info.sandbox == Some(false) {
                ", not sandboxed"
            } else {
                ""
            }
        );
        println!(
            "             {}, {} FP32, {}, {} video memory ({} free)",
            info.gpu.as_deref().unwrap_or("no GPU"),
            number(info.tflops, "TFLOPS"),
            number(info.bandwidth_gbs, "GB/s"),
            number(info.vram_gb, "GB"),
            number(info.vram_free_gb, "GB")
        );
        println!(
            "             {}, {} threads, {} RAM",
            info.cpu.as_deref().unwrap_or("unknown CPU"),
            info.threads
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into()),
            number(info.ram_gb, "GB")
        );
    }
    println!("{} queued", status.queued);
    Ok(())
}

fn wait(client: &Client, id: &str) -> Result<Job> {
    use std::io::{IsTerminal, Write};
    let terminal = std::io::stderr().is_terminal();
    let mut shown = String::new();
    loop {
        let job: Job = client.get(&format!("jobs/{id}"))?;
        let (key, line) = match job.state {
            JobState::Queued => {
                let line = format!("queued, {} ahead", job.queue_position.unwrap_or(0));
                (line.clone(), line)
            }
            JobState::Running | JobState::Cancelling => {
                let progress = job.progress.as_deref().unwrap_or("starting");
                let state = job.state.name();
                (
                    format!("{state} {progress}"),
                    format!(
                        "{} {state} {}: {progress}",
                        job.runner.as_deref().unwrap_or("-"),
                        duration(job.seconds.unwrap_or(0.0))
                    ),
                )
            }
            _ => {
                if terminal {
                    eprint!("\r\x1b[2K");
                }
                return Ok(job);
            }
        };
        if terminal {
            let shown_line: String = line.chars().take(200).collect();
            eprint!("\r\x1b[2K{shown_line}");
            let _ = std::io::stderr().flush();
        } else if key != shown {
            eprintln!("{line}");
        }
        shown = key;
        std::thread::sleep(POLL);
    }
}

fn finish(client: &Client, file: &Path, job: &Job) -> Result<()> {
    let has = |name: &str| job.files.iter().any(|f| f == name);
    let mut wrote = None;
    if has("result.json.gz") {
        let out = crate::sim::result_path(file);
        let packed = client.download(&job.id, "result.json.gz")?;
        let mut text = Vec::new();
        flate2::read::GzDecoder::new(packed.as_slice())
            .read_to_end(&mut text)
            .context("unpacking the result")?;
        std::fs::write(&out, text)?;
        let frames = crate::sim::movie_path(file);
        if has("frames.bin") {
            std::fs::write(&frames, client.download(&job.id, "frames.bin")?)?;
        } else if frames.is_file() {
            std::fs::remove_file(&frames)?;
        }
        wrote = Some(out);
    }
    if has("stdout.txt") {
        print!(
            "{}",
            String::from_utf8_lossy(&client.download(&job.id, "stdout.txt")?)
        );
    }
    eprintln!(
        "{} on runner {} in {} at {} an hour, charged {}",
        job.state.name(),
        job.runner.as_deref().unwrap_or("-"),
        duration(job.seconds.unwrap_or(0.0)),
        euros(job.price_cents_per_hour),
        euros(job.charged_cents.unwrap_or(0))
    );
    if let Some(out) = &wrote {
        println!("wrote {}", out.display());
    }
    match job.state {
        JobState::Succeeded => Ok(()),
        JobState::Cancelled => bail!("job {} was cancelled", job.id),
        _ => bail!(
            "{}",
            job.error.as_deref().unwrap_or("the remote run failed")
        ),
    }
}

pub fn run_job(job: &Path, out: &Path) -> Result<()> {
    let bytes = std::fs::read(job).with_context(|| format!("reading {}", job.display()))?;
    let unpacked = crate::sim::job::unpack(&bytes)?;
    let outcome = crate::sim::simulate(&unpacked.spec)?;
    let spec_path = out.join("result.gsim");
    crate::sim::write(&spec_path, &outcome, &unpacked.stamp)?;
    std::fs::rename(crate::sim::result_path(&spec_path), out.join("result.json"))?;
    let frames = crate::sim::movie_path(&spec_path);
    if frames.is_file() {
        std::fs::rename(&frames, out.join("frames.bin"))?;
    }
    print!("{}", outcome.table());
    let failed = outcome.failed();
    if !failed.is_empty() {
        bail!("expectation not met: {}", failed.join("; "));
    }
    Ok(())
}
