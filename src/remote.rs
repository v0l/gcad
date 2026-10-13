use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_SERVER: &str = "https://sims.gcad.sh";
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

pub struct Client {
    agent: ureq::Agent,
    server: String,
    token: String,
}

fn explain(error: ureq::Error) -> anyhow::Error {
    match error {
        ureq::Error::Status(code, response) => {
            let text = response.into_string().unwrap_or_default();
            let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let message = value["error"].as_str().unwrap_or(&text).to_string();
            let mut out = format!("the sims server said: {message}");
            if code == 402
                && let Some(shop) = value["shop_url"].as_str()
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

    fn get(&self, path: &str) -> Result<Value> {
        self.agent
            .get(&self.url(path))
            .set("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(explain)?
            .into_json()
            .context("reading the sims server's reply")
    }

    fn post(&self, path: &str) -> Result<Value> {
        self.agent
            .post(&self.url(path))
            .set("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(explain)?
            .into_json()
            .context("reading the sims server's reply")
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
    let reply: Value = agent()
        .post(&format!("{server}/api/accounts"))
        .call()
        .map_err(explain)?
        .into_json()?;
    let token = reply["token"]
        .as_str()
        .context("the server sent no token")?;
    let path = save(&server, token)?;
    println!(
        "account {} on {server}",
        reply["id"].as_str().unwrap_or_default()
    );
    println!("token saved in {}", path.display());
    println!("top it up with that account id, then run `gcad sim FILE --remote`");
    Ok(())
}

pub fn login(token: &str, server: Option<&str>) -> Result<()> {
    let server = server_of(server);
    let path = save(&server, token)?;
    let client = Client::open()?;
    let me = client.get("account")?;
    println!(
        "account {} on {server}, saved in {}",
        me["id"].as_str().unwrap_or_default(),
        path.display()
    );
    Ok(())
}

pub fn account() -> Result<()> {
    let client = Client::open()?;
    let me = client.get("account")?;
    let held = me["held_cents"].as_i64().unwrap_or(0);
    println!("account  {}", me["id"].as_str().unwrap_or_default());
    println!("server   {}", client.server);
    println!(
        "balance  {}{}",
        euros(me["balance_cents"].as_i64().unwrap_or(0)),
        if held > 0 {
            format!(" ({} held by running jobs)", euros(held))
        } else {
            String::new()
        }
    );
    println!("runners  `gcad remote runners` lists them and their prices");
    if let Some(shop) = me["shop_url"].as_str() {
        println!("top up   {shop}");
    }
    Ok(())
}

pub fn jobs() -> Result<()> {
    let client = Client::open()?;
    let list = client.get("jobs")?;
    let jobs = list["jobs"].as_array().cloned().unwrap_or_default();
    if jobs.is_empty() {
        println!("no jobs yet");
    }
    for job in jobs {
        let charged = job["charged_cents"].as_i64().map(euros).unwrap_or_default();
        let took = job["seconds"].as_f64().map(duration).unwrap_or_default();
        println!(
            "{}  {:<10} {:<24} {:<12} {:>8} {:>7}  {}",
            job["id"].as_str().unwrap_or_default(),
            job["state"].as_str().unwrap_or_default(),
            job["spec"].as_str().unwrap_or_default(),
            job["runner"].as_str().unwrap_or("-"),
            took,
            charged,
            job["error"].as_str().unwrap_or_default()
        );
    }
    Ok(())
}

pub fn cancel(id: &str) -> Result<()> {
    let client = Client::open()?;
    let job = client.post(&format!("jobs/{id}/cancel"))?;
    println!("{id} {}", job["state"].as_str().unwrap_or_default());
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

const LIVE: &[&str] = &["queued", "running", "cancelling", "succeeded"];

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
    let found = client.get(&format!("jobs?hash={hash}"))?;
    let fresh_here = crate::sim::stale(file, vars).is_none();
    let existing = found["jobs"]
        .as_array()
        .and_then(|jobs| {
            jobs.iter()
                .find(|j| match j["state"].as_str().unwrap_or_default() {
                    "succeeded" => !fresh_here,
                    state => LIVE.contains(&state),
                })
        })
        .cloned();
    let job = match existing {
        Some(job) => {
            eprintln!(
                "picking up job {} ({})",
                job["id"].as_str().unwrap_or_default(),
                job["state"].as_str().unwrap_or_default()
            );
            job
        }
        None => submit(&client, file, &name, vars, &stamp, &hash, limits, build)?,
    };
    let id = job["id"].as_str().context("the job has no id")?.to_string();
    let done = wait(&client, &id)?;
    finish(&client, file, &done)
}

#[allow(clippy::too_many_arguments)]
fn submit(
    client: &Client,
    file: &Path,
    name: &str,
    vars: &[(String, f64)],
    stamp: &[(String, String)],
    hash: &str,
    limits: &Limits,
    build: impl FnOnce() -> Result<crate::model::Run>,
) -> Result<Value> {
    let price = ready(client, limits)?;
    let run = build()?;
    let spec = run
        .model
        .sim
        .as_deref()
        .context("a .gsim file runs as a simulation")?;
    let packed = crate::sim::job::pack(spec, stamp.to_vec())?;
    let mut url = format!(
        "jobs?spec={}&version={}&hash={hash}&max_seconds={}&max_price={price}",
        encode(name),
        env!("CARGO_PKG_VERSION"),
        limits.minutes * 60
    );
    for (key, value) in vars {
        url.push_str(&format!("&set={}", encode(&format!("{key}={value}"))));
    }
    eprintln!(
        "sending {} ({} kB) to {}",
        file.display(),
        packed.len().div_ceil(1024),
        client.server
    );
    let job: Value = client
        .agent
        .post(&client.url(&url))
        .set("Authorization", &format!("Bearer {}", client.token))
        .set("Content-Type", "application/gzip")
        .send_bytes(&packed)
        .map_err(explain)?
        .into_json()?;
    let id = job["id"].as_str().unwrap_or_default();
    eprintln!(
        "job {id} holds {} for up to {} minutes at up to {} an hour; if this stops, rerun the same command to pick it up, or `gcad remote cancel {id}`",
        euros(job["held_cents"].as_i64().unwrap_or(0)),
        limits.minutes,
        euros(price as i64)
    );
    Ok(job)
}

fn runners_of(status: &Value) -> Vec<Value> {
    status["runners"].as_array().cloned().unwrap_or_default()
}

fn price_of(runner: &Value) -> u64 {
    runner["price_cents_per_hour"].as_u64().unwrap_or(0)
}

fn ready(client: &Client, limits: &Limits) -> Result<u64> {
    let version = env!("CARGO_PKG_VERSION");
    let status = client.get("status")?;
    let all = runners_of(&status);
    let able: Vec<&Value> = all
        .iter()
        .filter(|r| r["version"].as_str() == Some(version))
        .collect();
    if able.is_empty() {
        let mut versions: Vec<&str> = all.iter().filter_map(|r| r["version"].as_str()).collect();
        versions.dedup();
        bail!(
            "no remote runner runs gcad {version}{}",
            if versions.is_empty() {
                String::new()
            } else {
                format!(" (they run {})", versions.join(", "))
            }
        );
    }
    let cheapest = able.iter().map(|r| price_of(r)).min().unwrap_or(0);
    let price = match limits.price_cents {
        Some(cap) if cap < cheapest => bail!(
            "the cheapest runner on gcad {version} asks {} an hour, above --max-price {}; `gcad remote runners` lists them",
            euros(cheapest as i64),
            euros(cap as i64)
        ),
        Some(cap) => cap,
        None => able.iter().map(|r| price_of(r)).max().unwrap_or(0),
    };
    let me = client.get("account")?;
    let hold = (limits.minutes * price).div_ceil(60) as i64;
    let balance = me["balance_cents"].as_i64().unwrap_or(0);
    if balance < hold {
        bail!(
            "a {} minute run at up to {} an hour holds {}, the balance is {}; top up at {} with account id {}, or lower --max-minutes or --max-price",
            limits.minutes,
            euros(price as i64),
            euros(hold),
            euros(balance),
            me["shop_url"].as_str().unwrap_or("the shop"),
            me["id"].as_str().unwrap_or_default()
        );
    }
    Ok(price)
}

pub fn runners() -> Result<()> {
    let status: Value = agent()
        .get(&format!("{}/api/status", server_of(None)))
        .call()
        .map_err(explain)?
        .into_json()?;
    let all = runners_of(&status);
    if all.is_empty() {
        println!("no runner has been seen in the last day");
    }
    for runner in all {
        let info = &runner["info"];
        let number = |key: &str, unit: &str| {
            info[key]
                .as_f64()
                .map(|v| format!("{v} {unit}"))
                .unwrap_or_else(|| "-".into())
        };
        println!(
            "{:<12} {}/h  gcad {}  {}{}",
            runner["name"].as_str().unwrap_or_default(),
            euros(price_of(&runner) as i64),
            runner["version"].as_str().unwrap_or_default(),
            if runner["online"].as_bool() == Some(true) {
                if runner["busy"].as_bool() == Some(true) {
                    "busy"
                } else {
                    "idle"
                }
            } else {
                "offline"
            },
            if runner["info"]["sandbox"].as_bool() == Some(false) {
                ", not sandboxed"
            } else {
                ""
            }
        );
        println!(
            "             {}, {} FP32, {}, {} video memory ({} free)",
            info["gpu"].as_str().unwrap_or("no GPU"),
            number("tflops", "TFLOPS"),
            number("bandwidth_gbs", "GB/s"),
            number("vram_gb", "GB"),
            number("vram_free_gb", "GB")
        );
        println!(
            "             {}, {} threads, {} RAM",
            info["cpu"].as_str().unwrap_or("unknown CPU"),
            info["threads"]
                .as_u64()
                .map(|v| v.to_string())
                .unwrap_or("-".into()),
            number("ram_gb", "GB")
        );
    }
    let queued = status["queued"].as_u64().unwrap_or(0);
    println!("{queued} queued");
    Ok(())
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn wait(client: &Client, id: &str) -> Result<Value> {
    use std::io::{IsTerminal, Write};
    let terminal = std::io::stderr().is_terminal();
    let mut shown = String::new();
    loop {
        let job = client.get(&format!("jobs/{id}"))?;
        let state = job["state"].as_str().unwrap_or_default();
        let (key, line) = match state {
            "queued" => {
                let line = format!(
                    "queued, {} ahead",
                    job["queue_position"].as_i64().unwrap_or(0)
                );
                (line.clone(), line)
            }
            "running" | "cancelling" => {
                let progress = job["progress"].as_str().unwrap_or("starting");
                let took = duration(job["seconds"].as_f64().unwrap_or(0.0));
                let runner = job["runner"].as_str().unwrap_or("-");
                (
                    format!("{state} {progress}"),
                    format!("{runner} {state} {took}: {progress}"),
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

fn finish(client: &Client, file: &Path, job: &Value) -> Result<()> {
    let id = job["id"].as_str().unwrap_or_default();
    let state = job["state"].as_str().unwrap_or_default();
    let files: Vec<&str> = job["files"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut wrote = None;
    if files.contains(&"result.json.gz") {
        let out = crate::sim::result_path(file);
        let packed = client.download(id, "result.json.gz")?;
        let mut text = Vec::new();
        flate2::read::GzDecoder::new(packed.as_slice())
            .read_to_end(&mut text)
            .context("unpacking the result")?;
        std::fs::write(&out, text)?;
        let frames = crate::sim::movie_path(file);
        if files.contains(&"frames.bin") {
            std::fs::write(&frames, client.download(id, "frames.bin")?)?;
        } else if frames.is_file() {
            std::fs::remove_file(&frames)?;
        }
        wrote = Some(out);
    }
    if files.contains(&"stdout.txt") {
        let table = String::from_utf8_lossy(&client.download(id, "stdout.txt")?).to_string();
        print!("{table}");
    }
    eprintln!(
        "{state} on runner {} in {} at {} an hour, charged {}",
        job["runner"].as_str().unwrap_or("-"),
        duration(job["seconds"].as_f64().unwrap_or(0.0)),
        euros(job["price_cents_per_hour"].as_i64().unwrap_or(0)),
        euros(job["charged_cents"].as_i64().unwrap_or(0))
    );
    if let Some(out) = &wrote {
        println!("wrote {}", out.display());
    }
    match state {
        "succeeded" => Ok(()),
        "cancelled" => bail!("job {id} was cancelled"),
        _ => bail!(
            "{}",
            job["error"].as_str().unwrap_or("the remote run failed")
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
