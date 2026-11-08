use crate::sources::{Sources, include_of};
use crate::{Assets, Error, Options, VERSION, cache_key_with, plain};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

pub const INDEX: &str = "index.json";
pub const DEFAULT_MAX_BYTES: u64 = 64 << 20;
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);

#[derive(Clone, Debug)]
pub struct AssetCache {
    dir: PathBuf,
    extension: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Got {
    Hit,
    Built,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    pub removed: usize,
    pub freed: u64,
    pub kept: usize,
    pub bytes: u64,
}

pub fn options_key(options: &Options) -> u64 {
    cache_key_with("", "", options)
}

pub fn file_name(key: u64, extension: &str) -> String {
    format!("{key:016x}.{extension}")
}

pub fn parse_key(text: &str) -> Option<u64> {
    (text.len() == 16)
        .then(|| u64::from_str_radix(text, 16).ok())
        .flatten()
}

pub fn is_glb(bytes: &[u8]) -> bool {
    bytes.len() >= 12
        && &bytes[0..4] == b"glTF"
        && bytes[4..8] == 2u32.to_le_bytes()
        && u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize == bytes.len()
}

fn unique() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn write_atomic(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let partial = file.with_file_name(format!(".{name}.{}.part", unique()));
    let written = std::fs::write(&partial, bytes).and_then(|()| std::fs::rename(&partial, file));
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written
}

pub fn touch(file: &Path) {
    if let Ok(f) = std::fs::File::options().append(true).open(file) {
        let _ = f.set_modified(SystemTime::now());
    }
}

impl AssetCache {
    pub fn new(dir: impl Into<PathBuf>) -> AssetCache {
        AssetCache::with_extension(dir, "glb")
    }

    pub fn with_extension(dir: impl Into<PathBuf>, extension: &str) -> AssetCache {
        AssetCache {
            dir: dir.into(),
            extension: extension.to_string(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self, key: u64) -> PathBuf {
        self.dir.join(file_name(key, &self.extension))
    }

    pub fn get(&self, key: u64, valid: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
        let file = self.path(key);
        let bytes = std::fs::read(&file).ok()?;
        if !valid(&bytes) {
            self.remove(key);
            return None;
        }
        touch(&file);
        Some(bytes)
    }

    pub fn put(&self, key: u64, bytes: &[u8], meta: &str) -> std::io::Result<PathBuf> {
        let file = self.path(key);
        write_atomic(&file, bytes)?;
        let _ = write_atomic(&file.with_extension("json"), meta.as_bytes());
        Ok(file)
    }

    pub fn remove(&self, key: u64) {
        let file = self.path(key);
        let _ = std::fs::remove_file(file.with_extension("json"));
        let _ = std::fs::remove_file(file);
    }

    pub fn get_or_build_path(
        &self,
        path: impl AsRef<Path>,
        options: &Options,
    ) -> Result<(PathBuf, Got), Error> {
        let path = path.as_ref();
        let sources = Sources::from_path(path)?;
        let key = sources.key(options)?;
        if self.get(key, is_glb).is_some() {
            return Ok((self.path(key), Got::Hit));
        }
        let bytes = Assets::from_sources(&sources, Some(path), options)?.to_glb()?;
        let meta = meta(&sources, &path.display().to_string(), options);
        let file = self
            .put(key, &bytes, &meta)
            .map_err(|e| plain(format!("{}: {e}", self.path(key).display())))?;
        Ok((file, Got::Built))
    }

    pub fn entries(&self) -> Vec<(u64, u64, SystemTime)> {
        let Ok(read) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        read.flatten()
            .filter_map(|entry| {
                let path = entry.path();
                (path.extension()? == self.extension.as_str()).then_some(())?;
                let key = parse_key(&path.file_stem()?.to_string_lossy())?;
                let meta = entry.metadata().ok()?;
                let sidecar = std::fs::metadata(path.with_extension("json")).map_or(0, |m| m.len());
                Some((key, meta.len() + sidecar, meta.modified().ok()?))
            })
            .collect()
    }

    pub fn bytes(&self) -> u64 {
        self.entries().iter().map(|(_, n, _)| n).sum()
    }

    pub fn prune(&self, max_bytes: u64, max_age: Duration, keep: &HashSet<u64>) -> Pruned {
        let now = SystemTime::now();
        self.drop_stale_partials(now);
        let mut entries = self.entries();
        entries.sort_by_key(|(key, _, at)| (!keep.contains(key), std::cmp::Reverse(*at)));
        let mut out = Pruned::default();
        let mut total = 0;
        for (key, size, at) in entries {
            let old = now.duration_since(at).unwrap_or_default() > max_age;
            let kept = keep.contains(&key);
            if kept || (!old && total + size <= max_bytes) {
                total += size;
                out.kept += 1;
            } else {
                self.remove(key);
                out.removed += 1;
                out.freed += size;
            }
        }
        out.bytes = total;
        out
    }

    fn drop_stale_partials(&self, now: SystemTime) {
        let Ok(read) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let partial = path.extension().is_some_and(|e| e == "part");
            let stale = entry.metadata().and_then(|m| m.modified()).is_ok_and(|at| {
                now.duration_since(at).unwrap_or_default() > Duration::from_secs(3600)
            });
            if partial && stale {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn json_text(text: &str) -> String {
    serde_json::Value::String(text.to_string()).to_string()
}

pub fn meta(sources: &Sources, source: &str, options: &Options) -> String {
    let files: Vec<String> = sources
        .files()
        .map(|(path, hash)| {
            let hash = hash.map_or("null".to_string(), |h| json_text(&format!("{h:016x}")));
            format!("{}: {hash}", json_text(path))
        })
        .collect();
    format!(
        "{{\"gcad\": {}, \"source\": {}, \"options\": {}, \"files\": {{{}}}}}\n",
        json_text(VERSION),
        json_text(source),
        json_text(&format!("{:016x}", options_key(options))),
        files.join(", ")
    )
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub key: u64,
    pub options: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Index {
    pub gcad: String,
    pub format: String,
    pub entries: BTreeMap<String, Entry>,
}

impl Index {
    pub fn to_json(&self) -> String {
        let entries: Vec<String> = self
            .entries
            .iter()
            .map(|(path, e)| {
                format!(
                    "    {}: {{\"key\": \"{:016x}\", \"options\": \"{:016x}\"}}",
                    json_text(path),
                    e.key,
                    e.options
                )
            })
            .collect();
        format!(
            "{{\n  \"gcad\": {},\n  \"format\": {},\n  \"entries\": {{\n{}\n  }}\n}}\n",
            json_text(&self.gcad),
            json_text(&self.format),
            entries.join(",\n")
        )
    }

    pub fn from_json(text: &str) -> Result<Index, Error> {
        let bad = |what: &str| plain(format!("{INDEX}: {what}"));
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| bad(&e.to_string()))?;
        let field = |name: &str| value.get(name).and_then(|v| v.as_str()).map(str::to_string);
        let mut index = Index {
            gcad: field("gcad").ok_or_else(|| bad("no gcad version"))?,
            format: field("format").ok_or_else(|| bad("no format"))?,
            entries: BTreeMap::new(),
        };
        let entries = value
            .get("entries")
            .and_then(|e| e.as_object())
            .ok_or_else(|| bad("no entries"))?;
        for (path, entry) in entries {
            let hex = |name: &str| entry.get(name).and_then(|v| v.as_str()).and_then(parse_key);
            let (Some(key), Some(options)) = (hex("key"), hex("options")) else {
                return Err(bad(&format!("{path} has no key")));
            };
            index.entries.insert(path.clone(), Entry { key, options });
        }
        Ok(index)
    }

    pub fn read(dir: &Path) -> Option<Index> {
        Index::from_json(&std::fs::read_to_string(dir.join(INDEX)).ok()?).ok()
    }

    pub fn keys(&self) -> HashSet<u64> {
        self.entries.values().map(|e| e.key).collect()
    }
}

pub fn sources_in(dir: &Path, skip: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::new()];
    let skip = skip.canonicalize().unwrap_or_else(|_| skip.to_path_buf());
    while let Some(rel) = pending.pop() {
        let Ok(read) = std::fs::read_dir(dir.join(&rel)) else {
            continue;
        };
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name.starts_with('.') || path.canonicalize().is_ok_and(|p| p == skip) {
                continue;
            }
            let rel = rel.join(&name);
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                pending.push(rel);
            } else if matches!(
                path.extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
                    .as_deref(),
                Some("gcad" | "gasm")
            ) {
                found.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let included: HashSet<String> = found
        .iter()
        .flat_map(|rel| {
            let text = std::fs::read_to_string(dir.join(rel)).unwrap_or_default();
            let at = rel.rfind('/').map_or("", |i| &rel[..i]).to_string();
            text.lines()
                .filter_map(include_of)
                .map(|name| crate::sources::join(&at, name))
                .collect::<Vec<_>>()
        })
        .collect();
    found.retain(|rel| !included.contains(rel));
    found.sort();
    found
}

#[derive(Clone, Debug, Default)]
pub struct Baked {
    pub index: Index,
    pub built: Vec<String>,
    pub reused: Vec<String>,
    pub removed: usize,
}

pub fn bake(
    src: &Path,
    out: &Path,
    options_for: impl Fn(&str) -> Option<Options>,
) -> Result<Baked, Error> {
    let cache = AssetCache::new(out);
    let mut baked = Baked {
        index: Index {
            gcad: VERSION.to_string(),
            format: "glb".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    for rel in sources_in(src, out) {
        let Some(options) = options_for(&rel) else {
            continue;
        };
        let (file, got) = cache
            .get_or_build_path(src.join(&rel), &options)
            .map_err(|e| plain(format!("{rel}: {e}")))?;
        let key = file
            .file_stem()
            .and_then(|s| parse_key(&s.to_string_lossy()))
            .expect("cache files are named by key");
        match got {
            Got::Hit => baked.reused.push(rel.clone()),
            Got::Built => baked.built.push(rel.clone()),
        }
        baked.index.entries.insert(
            rel,
            Entry {
                key,
                options: options_key(&options),
            },
        );
    }
    let keep = baked.index.keys();
    for (key, _, _) in cache.entries() {
        if !keep.contains(&key) {
            cache.remove(key);
            baked.removed += 1;
        }
    }
    write_atomic(&out.join(INDEX), baked.index.to_json().as_bytes())
        .map_err(|e| plain(format!("{}: {e}", out.join(INDEX).display())))?;
    Ok(baked)
}

pub fn check(src: &Path, out: &Path, options_for: impl Fn(&str) -> Option<Options>) -> Vec<String> {
    let index = match std::fs::read_to_string(out.join(INDEX)) {
        Ok(text) => match Index::from_json(&text) {
            Ok(index) => index,
            Err(e) => return vec![e.message],
        },
        Err(e) => return vec![format!("{}: {e}", out.join(INDEX).display())],
    };
    let cache = AssetCache::new(out);
    let mut problems = Vec::new();
    for rel in sources_in(src, out) {
        let Some(options) = options_for(&rel) else {
            continue;
        };
        let key = match Sources::from_path(src.join(&rel)).and_then(|s| s.key(&options)) {
            Ok(key) => key,
            Err(e) => {
                problems.push(format!("{rel}: {e}"));
                continue;
            }
        };
        match index.entries.get(&rel) {
            None => problems.push(format!("{rel}: not baked")),
            Some(e) if e.key != key => problems.push(format!(
                "{rel}: stale, baked {:016x}, source is {key:016x}",
                e.key
            )),
            Some(_) if !std::fs::read(cache.path(key)).is_ok_and(|b| is_glb(&b)) => {
                problems.push(format!("{rel}: {} missing or bad", file_name(key, "glb")))
            }
            Some(_) => {}
        }
    }
    problems
}
