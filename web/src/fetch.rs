use crate::link::{self, Repo};
use std::path::Path;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

const MOST: usize = 300;

fn js(error: wasm_bindgen::JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}

async fn get(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(js)?
        .dyn_into()
        .map_err(js)?;
    if !response.ok() {
        return Err(format!("{} from {url}", response.status()));
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(js)?)
        .await
        .map_err(js)?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

async fn listing(repo: &Repo) -> Result<Vec<String>, String> {
    let bytes = get(&repo.tree()).await?;
    let tree: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let entries = tree["tree"]
        .as_array()
        .ok_or("GitHub sent no file list")?
        .iter()
        .filter(|e| e["type"] == "blob")
        .filter_map(|e| e["path"].as_str().map(str::to_string))
        .collect();
    Ok(entries)
}

fn materials_above(file: &str, known: &[String]) -> Vec<String> {
    let mut dirs = vec![""];
    let mut at = file;
    while let Some((up, _)) = at.rsplit_once('/') {
        dirs.push(up);
        at = up;
    }
    dirs.into_iter()
        .filter_map(|dir| link::join(dir, "materials.toml"))
        .filter(|p| known.contains(p))
        .collect()
}

async fn fetch_all(repo: &Repo, start: Vec<String>, known: Option<&[String]>) {
    let root = repo.root();
    let mut wanted = start;
    let mut seen: Vec<String> = Vec::new();
    while !wanted.is_empty() && seen.len() < MOST {
        let mut batch: Vec<String> = Vec::new();
        for path in std::mem::take(&mut wanted) {
            if !seen.contains(&path) && !batch.contains(&path) && seen.len() + batch.len() < MOST {
                batch.push(path);
            }
        }
        seen.extend(batch.iter().cloned());
        let got = futures::future::join_all(
            batch
                .iter()
                .map(|path| async move { (path.clone(), get(&repo.raw(path)).await) }),
        )
        .await;
        for (path, result) in got {
            let Ok(bytes) = result else {
                continue;
            };
            gcad::host::bundle(Path::new(&format!("{root}/{path}")), &bytes);
            if !link::READ_INTO.contains(&link::extension(&path)) {
                continue;
            }
            let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
            let text = String::from_utf8_lossy(&bytes);
            let mut next = link::references(&text, dir);
            if let Some(known) = known {
                next.retain(|p| known.contains(p));
                next.extend(materials_above(&path, known));
            }
            wanted.extend(next.into_iter().filter(|p| !seen.contains(p)));
        }
    }
}

pub async fn open(repo: Repo) -> Result<String, String> {
    let root = repo.root();
    gcad::host::bundle(Path::new(&format!("{root}/.git")), b"");
    let open = if repo.folder {
        let files = listing(&repo).await?;
        let under = repo.path.clone();
        let inside: Vec<String> = files
            .iter()
            .filter(|p| under.is_empty() || p.starts_with(&format!("{under}/")))
            .filter(|p| link::READ_INTO.contains(&link::extension(p)))
            .cloned()
            .collect();
        let first = link::first_to_open(&inside, &under)
            .ok_or_else(|| format!("no .gcad or .gasm files in {}", repo.path))?;
        fetch_all(&repo, inside, Some(&files)).await;
        first
    } else {
        fetch_all(&repo, vec![repo.path.clone()], None).await;
        repo.path.clone()
    };
    let path = format!("{root}/{open}");
    if !gcad::host::is_file(Path::new(&path)) {
        return Err(format!("could not fetch {}", repo.raw(&open)));
    }
    Ok(path)
}
