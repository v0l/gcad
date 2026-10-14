use std::io;
use std::path::{Component, Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;
#[cfg(target_arch = "wasm32")]
pub use web_time::Instant;

pub fn tidy(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
mod files {
    use std::io;
    use std::path::{Path, PathBuf};

    type Stamp = Option<(std::time::SystemTime, u64)>;

    static SEEN: std::sync::Mutex<std::collections::BTreeMap<PathBuf, Stamp>> =
        std::sync::Mutex::new(std::collections::BTreeMap::new());

    fn stamp(path: &Path) -> Stamp {
        let meta = std::fs::metadata(path).ok()?;
        Some((meta.modified().ok()?, meta.len()))
    }

    fn seen() -> std::sync::MutexGuard<'static, std::collections::BTreeMap<PathBuf, Stamp>> {
        SEEN.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn note(path: &Path) {
        seen().insert(path.to_path_buf(), stamp(path));
    }

    pub fn unchanged_except(path: &Path) -> bool {
        let Ok(skip) = std::fs::canonicalize(path) else {
            return false;
        };
        let seen = seen();
        let same = |file: &Path| std::fs::canonicalize(file).is_ok_and(|f| f == skip);
        !seen.is_empty()
            && seen
                .iter()
                .all(|(file, was)| same(file) || stamp(file) == *was)
    }

    pub fn forget_reads() {
        seen().clear();
    }

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        note(path);
        std::fs::read(path)
    }

    pub fn is_file(path: &Path) -> bool {
        note(path);
        path.is_file()
    }

    pub fn is_dir(path: &Path) -> bool {
        path.is_dir()
    }

    pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }

    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::write(path, bytes)
    }

    pub fn save(path: &Path, text: &str) {
        let _ = std::fs::write(path, text);
    }

    pub fn edited(_path: &Path) -> bool {
        false
    }

    pub fn revert(_path: &Path) {}

    pub fn take_open() -> Option<PathBuf> {
        None
    }

    pub fn status() -> Option<String> {
        None
    }

    pub fn opened(_path: &Path) {}

    pub fn built(_seconds: f64) {}

    fn settings_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("gcad").join("view.toml"))
    }

    fn settings() -> toml::Table {
        settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| t.parse().ok())
            .unwrap_or_default()
    }

    pub fn setting(name: &str) -> Option<String> {
        settings().get(name)?.as_str().map(str::to_string)
    }

    pub fn set_setting(name: &str, value: &str) {
        let Some(path) = settings_path() else { return };
        let mut table = settings();
        table.insert(name.to_string(), value.into());
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, table.to_string());
    }
}

#[cfg(target_arch = "wasm32")]
mod files {
    use std::collections::BTreeMap;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

    static FILES: Mutex<BTreeMap<PathBuf, Arc<[u8]>>> = Mutex::new(BTreeMap::new());

    fn hold<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
        loop {
            match lock.try_lock() {
                Ok(guard) => return guard,
                Err(TryLockError::Poisoned(poisoned)) => return poisoned.into_inner(),
                Err(TryLockError::WouldBlock) => std::hint::spin_loop(),
            }
        }
    }

    fn key(path: &Path) -> PathBuf {
        let path = super::tidy(path);
        path.strip_prefix("/")
            .map(Path::to_path_buf)
            .unwrap_or(path)
    }

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        hold(&FILES)
            .get(&key(path))
            .map(|b| b.to_vec())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such file"))
    }

    pub fn is_file(path: &Path) -> bool {
        hold(&FILES).contains_key(&key(path))
    }

    pub fn unchanged_except(_path: &Path) -> bool {
        false
    }

    pub fn forget_reads() {}

    pub fn is_dir(path: &Path) -> bool {
        let dir = key(path);
        hold(&FILES)
            .keys()
            .any(|k| k != &dir && k.starts_with(&dir))
    }

    pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
        Ok(key(path))
    }

    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        hold(&FILES).insert(key(path), bytes.into());
        Ok(())
    }

    pub fn list() -> Vec<PathBuf> {
        hold(&FILES).keys().cloned().collect()
    }

    static BUNDLED: Mutex<BTreeMap<PathBuf, Arc<[u8]>>> = Mutex::new(BTreeMap::new());

    const PREFIX: &str = "gcad:";

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }

    pub fn bundle(path: &Path, bytes: &[u8]) {
        let path = key(path);
        hold(&BUNDLED).insert(path.clone(), bytes.into());
        let saved = storage().and_then(|s| {
            s.get_item(&format!("{PREFIX}{}", path.display()))
                .ok()
                .flatten()
        });
        let current: Arc<[u8]> = match saved {
            Some(text) => text.as_bytes().into(),
            None => bytes.into(),
        };
        hold(&FILES).insert(path, current);
    }

    static INBOX: Mutex<Option<PathBuf>> = Mutex::new(None);
    static STATUS: Mutex<Option<String>> = Mutex::new(None);
    static OPENED: Mutex<Option<fn(&Path)>> = Mutex::new(None);
    static BUILT: Mutex<Option<fn(f64)>> = Mutex::new(None);

    pub fn on_built(hook: fn(f64)) {
        *hold(&BUILT) = Some(hook);
    }

    pub fn built(seconds: f64) {
        let hook = *hold(&BUILT);
        if let Some(hook) = hook {
            hook(seconds);
        }
    }

    pub fn ask_open(path: &Path) {
        *hold(&INBOX) = Some(key(path));
    }

    pub fn take_open() -> Option<PathBuf> {
        hold(&INBOX).take()
    }

    pub fn set_status(status: Option<String>) {
        *hold(&STATUS) = status;
    }

    pub fn status() -> Option<String> {
        hold(&STATUS).clone()
    }

    pub fn on_open(hook: fn(&Path)) {
        *hold(&OPENED) = Some(hook);
    }

    pub fn opened(path: &Path) {
        let hook = *hold(&OPENED);
        if let Some(hook) = hook {
            hook(&key(path));
        }
    }

    pub fn edited(path: &Path) -> bool {
        let path = key(path);
        let bundled = hold(&BUNDLED).get(&path).cloned();
        let current = hold(&FILES).get(&path).cloned();
        bundled != current
    }

    pub fn save(path: &Path, text: &str) {
        let _ = write(path, text.as_bytes());
        if let Some(storage) = storage() {
            let _ = storage.set_item(&format!("{PREFIX}{}", key(path).display()), text);
        }
    }

    pub fn revert(path: &Path) {
        let path = key(path);
        if let Some(storage) = storage() {
            let _ = storage.remove_item(&format!("{PREFIX}{}", path.display()));
        }
        match hold(&BUNDLED).get(&path).cloned() {
            Some(bytes) => hold(&FILES).insert(path, bytes),
            None => hold(&FILES).remove(&path),
        };
    }

    const SETTING: &str = "gcad-setting:";

    pub fn setting(name: &str) -> Option<String> {
        storage()?
            .get_item(&format!("{SETTING}{name}"))
            .ok()
            .flatten()
    }

    pub fn set_setting(name: &str, value: &str) {
        if let Some(storage) = storage() {
            let _ = storage.set_item(&format!("{SETTING}{name}"), value);
        }
    }

    pub fn restore() {
        let Some(storage) = storage() else {
            return;
        };
        let count = storage.length().unwrap_or(0);
        for index in 0..count {
            let Some(name) = storage.key(index).ok().flatten() else {
                continue;
            };
            let Some(path) = name.strip_prefix(PREFIX) else {
                continue;
            };
            if let Some(text) = storage.get_item(&name).ok().flatten() {
                let _ = write(Path::new(path), text.as_bytes());
            }
        }
    }
}

pub use files::*;

pub fn read_to_string(path: &Path) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn exists(path: &Path) -> bool {
    is_file(path) || is_dir(path)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn each<T: Send, R: Send>(items: Vec<T>, f: impl Fn(T) -> R + Sync) -> Vec<Option<R>> {
    std::thread::scope(|scope| {
        let f = &f;
        let jobs: Vec<_> = items
            .into_iter()
            .map(|item| scope.spawn(move || f(item)))
            .collect();
        jobs.into_iter().map(|job| job.join().ok()).collect()
    })
}

#[cfg(target_arch = "wasm32")]
pub fn each<T: Send, R: Send>(items: Vec<T>, f: impl Fn(T) -> R + Sync) -> Vec<Option<R>> {
    use rayon::prelude::*;
    items.into_par_iter().map(|item| Some(f(item))).collect()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn(job: impl FnOnce() + Send + 'static) {
    std::thread::spawn(job);
}

#[cfg(target_arch = "wasm32")]
pub fn spawn(job: impl FnOnce() + Send + 'static) {
    rayon::spawn(job);
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn release_memory() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> i32;
    }
    unsafe {
        malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn release_memory() {}

pub fn wake(ctx: &egui::Context) {
    if cfg!(not(target_arch = "wasm32")) {
        ctx.request_repaint();
    }
}
