use serde::{Deserialize, Serialize};

pub const DEFAULT_SERVER: &str = "https://sims.gcad.sh";
pub const DEFAULT_SHOP: &str = "https://apexstrata.ie/shop/gcad-sim-credit/";

pub fn shop_or_default(given: Option<&str>) -> &str {
    given
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(DEFAULT_SHOP)
}
pub const ACCOUNT_TOKEN: &str = "gsa_";
pub const WORKER_TOKEN: &str = "gsw_";
pub const MAX_UPLOAD: u64 = 95 * 1024 * 1024;
pub const PART_SIZE: u64 = 64 * 1024 * 1024;
pub const MAX_JOB_SECONDS: u64 = 24 * 3600;
pub const DEFAULT_JOB_SECONDS: u64 = 3600;
pub const MAX_PRICE_CENTS: i64 = 100_000;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobState {
    pub const ALL: [JobState; 6] = [
        JobState::Queued,
        JobState::Running,
        JobState::Cancelling,
        JobState::Succeeded,
        JobState::Failed,
        JobState::Cancelled,
    ];

    pub fn name(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::Cancelling => "cancelling",
            JobState::Succeeded => "succeeded",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }

    pub fn named(name: &str) -> Option<JobState> {
        JobState::ALL.into_iter().find(|s| s.name() == name)
    }

    pub fn active(self) -> bool {
        matches!(
            self,
            JobState::Queued | JobState::Running | JobState::Cancelling
        )
    }

    pub fn on_runner(self) -> bool {
        matches!(self, JobState::Running | JobState::Cancelling)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct RunnerInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tflops: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bandwidth_gbs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_gb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_free_gb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threads: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ram_gb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GpuBench {
    pub gpu: String,
    pub tflops: f64,
    pub bandwidth_gbs: f64,
}

pub fn price_label(cents_per_hour: i64) -> String {
    if cents_per_hour == 0 {
        "free".into()
    } else {
        format!("€{}.{:02}/h", cents_per_hour / 100, cents_per_hour % 100)
    }
}

pub fn default_price<'a>(runners: impl IntoIterator<Item = &'a Runner>) -> Option<i64> {
    let runners: Vec<&Runner> = runners.into_iter().collect();
    let online: Vec<&&Runner> = runners.iter().filter(|r| r.online).collect();
    if online.is_empty() {
        runners.iter().map(|r| r.price_cents_per_hour).max()
    } else {
        online.iter().map(|r| r.price_cents_per_hour).max()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Runner {
    pub name: String,
    pub version: String,
    pub online: bool,
    pub busy: bool,
    pub last_seen: Option<i64>,
    pub price_cents_per_hour: i64,
    pub info: RunnerInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Status {
    pub runners: Vec<Runner>,
    pub versions: Vec<String>,
    pub online: Vec<String>,
    pub queued: i64,
    pub shop_url: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NewAccount {
    pub id: String,
    pub token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Account {
    pub id: String,
    pub balance_cents: i64,
    pub held_cents: i64,
    pub shop_url: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Job {
    pub id: String,
    pub spec: String,
    pub vars: Vec<(String, f64)>,
    pub version: String,
    pub hash: String,
    pub state: JobState,
    pub progress: Option<String>,
    pub error: Option<String>,
    pub files: Vec<String>,
    pub max_seconds: i64,
    pub price_cents_per_hour: i64,
    pub max_price_cents_per_hour: i64,
    pub runner: Option<String>,
    pub held_cents: i64,
    pub charged_cents: Option<i64>,
    pub seconds: Option<f64>,
    pub queue_position: Option<i64>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Jobs {
    pub jobs: Vec<Job>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Submit {
    pub spec: String,
    pub version: String,
    pub hash: String,
    pub max_seconds: u64,
    pub max_price: Option<i64>,
    pub vars: Vec<(String, f64)>,
}

pub fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn var_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn version_ok(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 32
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
}

impl Submit {
    pub fn query(&self) -> String {
        let mut out = format!(
            "spec={}&version={}&hash={}&max_seconds={}",
            encode(&self.spec),
            encode(&self.version),
            self.hash,
            self.max_seconds
        );
        if let Some(price) = self.max_price {
            out.push_str(&format!("&max_price={price}"));
        }
        for (name, value) in &self.vars {
            out.push_str(&format!("&set={}", encode(&format!("{name}={value}"))));
        }
        out
    }

    pub fn from_pairs<'a>(
        pairs: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Submit, String> {
        let mut submit = Submit {
            spec: String::new(),
            version: String::new(),
            hash: String::new(),
            max_seconds: DEFAULT_JOB_SECONDS,
            max_price: None,
            vars: Vec::new(),
        };
        for (key, value) in pairs {
            match key {
                "spec" => submit.spec = value.to_string(),
                "version" => submit.version = value.to_string(),
                "hash" => submit.hash = value.to_string(),
                "max_seconds" => {
                    submit.max_seconds = value.parse().map_err(|_| {
                        format!(
                            "`max_seconds` should be a whole number from 60 to {MAX_JOB_SECONDS}"
                        )
                    })?
                }
                "max_price" => {
                    submit.max_price = Some(
                        value
                            .parse()
                            .map_err(|_| "`max_price` should be whole cents an hour".to_string())?,
                    )
                }
                "set" => {
                    let pair = value
                        .split_once('=')
                        .and_then(|(n, v)| Some((n, v.trim().parse::<f64>().ok()?)))
                        .filter(|(n, v)| var_name(n) && v.is_finite())
                        .ok_or_else(|| format!("`set={value}` should be name=number"))?;
                    submit.vars.push((pair.0.to_string(), pair.1));
                }
                _ => {}
            }
        }
        submit.check()?;
        Ok(submit)
    }

    pub fn check(&self) -> Result<(), String> {
        let parts: Vec<&str> = self.spec.split('/').collect();
        if !self.spec.ends_with(".gsim")
            || self.spec.len() > 512
            || self.spec.starts_with('/')
            || parts.iter().any(|p| matches!(*p, "" | "." | ".."))
        {
            return Err("`spec` should be the relative path of the .gsim inside the bundle".into());
        }
        if !version_ok(&self.version) {
            return Err("`version` should be the gcad version that built the bundle".into());
        }
        if self.hash.len() != 64
            || !self
                .hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("`hash` should be the sha256 of the bundle in hex".into());
        }
        if !(60..=MAX_JOB_SECONDS).contains(&self.max_seconds) {
            return Err(format!(
                "`max_seconds` should be a whole number from 60 to {MAX_JOB_SECONDS}"
            ));
        }
        if self
            .max_price
            .is_some_and(|p| !(0..=MAX_PRICE_CENTS).contains(&p))
        {
            return Err("`max_price` should be whole cents an hour".into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Claim {
    pub version: String,
    pub price_cents_per_hour: i64,
    #[serde(default)]
    pub info: RunnerInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClaimedJob {
    pub id: String,
    pub spec: String,
    pub vars: Vec<(String, f64)>,
    pub max_seconds: u64,
    pub bundle_size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Claimed {
    pub job: Option<ClaimedJob>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Progress {
    pub progress: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProgressReply {
    pub cancel: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Finish {
    pub ok: bool,
    pub error: Option<String>,
    pub seconds: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Release {
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Files {
    pub files: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PartsStarted {
    pub upload: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Part {
    pub part: u16,
    pub etag: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PartsDone {
    pub parts: Vec<Part>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Credit {
    pub account: String,
    pub cents: i64,
    pub reference: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Credited {
    pub credited: bool,
    pub balance_cents: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NewWorker {
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkerCreated {
    pub id: String,
    pub name: String,
    pub token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkerEntry {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub last_seen: Option<i64>,
    pub disabled: bool,
    pub price_cents_per_hour: Option<i64>,
    pub info: RunnerInfo,
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkerList {
    pub workers: Vec<WorkerEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ApiError {
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shop_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needed_cents: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance_cents: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cheapest_cents_per_hour: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submit() -> Submit {
        Submit {
            spec: "sims/wind.gsim".into(),
            version: "0.3.1".into(),
            hash: "a".repeat(64),
            max_seconds: 600,
            max_price: Some(250),
            vars: vec![("width".into(), 20.5), ("n".into(), -3.0)],
        }
    }

    fn pairs(query: &str) -> Vec<(String, String)> {
        query
            .split('&')
            .map(|kv| {
                let (k, v) = kv.split_once('=').unwrap();
                let mut bytes = Vec::new();
                let mut it = v.bytes();
                while let Some(b) = it.next() {
                    if b == b'%' {
                        let hex: String = [it.next().unwrap(), it.next().unwrap()]
                            .iter()
                            .map(|&c| c as char)
                            .collect();
                        bytes.push(u8::from_str_radix(&hex, 16).unwrap());
                    } else {
                        bytes.push(b);
                    }
                }
                (k.to_string(), String::from_utf8(bytes).unwrap())
            })
            .collect()
    }

    #[test]
    fn submit_round_trips_through_its_query() {
        let query = submit().query();
        let decoded = pairs(&query);
        let back =
            Submit::from_pairs(decoded.iter().map(|(k, v)| (k.as_str(), v.as_str()))).unwrap();
        assert_eq!(back, submit());
    }

    #[test]
    fn submit_refuses_bad_fields() {
        for (key, value) in [
            ("spec", "../x.gsim"),
            ("spec", "/etc/x.gsim"),
            ("spec", "x.gcad"),
            ("version", "0.3 1"),
            ("hash", "xyz"),
            ("max_seconds", "10"),
            ("max_price", "-1"),
            ("set", "1x=2"),
            ("set", "w=nan"),
        ] {
            let mut fields = pairs(&submit().query());
            fields.retain(|(k, _)| k != key && k != "set");
            fields.push((key.into(), value.into()));
            assert!(
                Submit::from_pairs(fields.iter().map(|(k, v)| (k.as_str(), v.as_str()))).is_err(),
                "{key}={value} passed"
            );
        }
    }

    #[test]
    fn states_name_themselves() {
        for state in JobState::ALL {
            assert_eq!(JobState::named(state.name()), Some(state));
        }
        assert_eq!(
            serde_json::to_string(&JobState::Cancelling).unwrap(),
            "\"cancelling\""
        );
    }
}
