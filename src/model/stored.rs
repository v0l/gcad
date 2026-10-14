use super::assembly::{Couple, Joint, Mate, Source};
use super::bodies::Material;
use super::{Model, PartGroups};
use crate::select::{GroupEntry, Groups};
use monstertruck::modeling::*;
use monstertruck::topology::compress::CompressedSolid;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

type Body = CompressedSolid<Point3, Curve, Surface>;
type Read = (PathBuf, Option<[u8; 32]>, bool);
type StoredSource = (String, PathBuf, String, Vec<(String, f64)>);
type StoredGroup = (String, String, Surface);
type StoredPartGroups = (String, Vec<StoredGroup>, [[f64; 4]; 4]);

#[derive(Serialize, Deserialize)]
struct Stored {
    key: String,
    reads: Vec<Read>,
    solid: Option<Body>,
    body: String,
    bodies: Vec<(String, Body)>,
    groups: Vec<StoredGroup>,
    colours: Vec<(String, [f64; 3])>,
    opacities: Vec<(String, f64)>,
    materials: Vec<(String, Material)>,
    sources: Vec<StoredSource>,
    placements: Vec<(String, [[f64; 4]; 4])>,
    axes: Vec<(String, [f64; 3], [f64; 3])>,
    filled: Vec<(String, usize)>,
    stand_ins: Vec<(String, String)>,
    joints: Vec<Joint>,
    mates: Vec<Mate>,
    couples: Vec<Couple>,
    explode: Vec<(String, [f64; 3])>,
    part_groups: Vec<StoredPartGroups>,
    assembly: bool,
}

static DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

const KEPT_BYTES: u64 = 1 << 30;

pub fn store_parts_in(dir: Option<PathBuf>) {
    *DIR.lock().unwrap_or_else(|e| e.into_inner()) = dir;
}

pub fn default_dir() -> Option<PathBuf> {
    if std::env::var_os("GCAD_NO_PART_CACHE").is_some() {
        return None;
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("gcad").join("parts"))
}

fn dir() -> Option<PathBuf> {
    DIR.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn key(path: &Path, vars: &[(String, f64)]) -> Option<String> {
    let path = std::fs::canonicalize(path).ok()?;
    let exe = std::env::current_exe()
        .and_then(std::fs::metadata)
        .ok()
        .map(|meta| (meta.len(), meta.modified().ok()));
    Some(format!(
        "{}|{exe:?}|{}|{vars:?}",
        env!("GCAD_BUILD_VERSION"),
        path.display()
    ))
}

fn file_of(dir: &Path, key: &str) -> PathBuf {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(key.as_bytes());
    let name: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    dir.join(format!("{name}.part"))
}

pub(crate) fn fetch(path: &Path, vars: &[(String, f64)]) -> Option<Model> {
    let dir = dir()?;
    let key = key(path, vars)?;
    let file = file_of(&dir, &key);
    let stored: Stored = {
        let bytes = std::fs::read(&file).ok()?;
        ciborium::from_reader(flate2::read::GzDecoder::new(bytes.as_slice())).ok()?
    };
    if stored.key != key
        || !stored
            .reads
            .iter()
            .all(|(read, content, there)| current(read, content, *there))
    {
        return None;
    }
    let model = restored(stored)?;
    if let Ok(handle) = std::fs::File::options().append(true).open(&file) {
        let _ = handle.set_modified(std::time::SystemTime::now());
    }
    Some(model)
}

fn current(path: &Path, content: &Option<[u8; 32]>, there: bool) -> bool {
    match content {
        Some(digest) => crate::host::read(path).is_ok_and(|bytes| {
            use sha2::Digest;
            <[u8; 32]>::from(sha2::Sha256::digest(&bytes)) == *digest
        }),
        None => crate::host::is_file(path) == there,
    }
}

pub(crate) fn put(path: &Path, vars: &[(String, f64)], model: &Model, reads: &crate::host::Reads) {
    let Some(dir) = dir() else { return };
    let Some(key) = key(path, vars) else { return };
    let Some(stored) = storable(model, key.clone(), reads.contents()) else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let file = file_of(&dir, &key);
    let partial = file.with_extension(format!("part.{}", std::process::id()));
    let written = std::fs::File::create(&partial).ok().and_then(|handle| {
        let mut gz = flate2::write::GzEncoder::new(handle, flate2::Compression::fast());
        ciborium::into_writer(&stored, &mut gz).ok()?;
        gz.finish().ok()
    });
    if written.is_some() && std::fs::rename(&partial, &file).is_ok() {
        prune(&dir);
    } else {
        let _ = std::fs::remove_file(&partial);
    }
}

fn prune(dir: &Path) {
    let Ok(listing) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = listing
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let meta = entry.metadata().ok()?;
            Some((meta.modified().ok()?, meta.len(), entry.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    if total <= KEPT_BYTES {
        return;
    }
    files.sort();
    for (_, len, file) in files {
        if total <= KEPT_BYTES * 3 / 4 {
            break;
        }
        if std::fs::remove_file(&file).is_ok() {
            total -= len;
        }
    }
}

fn storable(model: &Model, key: String, reads: Vec<Read>) -> Option<Stored> {
    if model.sim.is_some() || reads.is_empty() {
        return None;
    }
    Some(Stored {
        key,
        reads,
        solid: model.solid.as_ref().map(|solid| solid.compress()),
        body: model.body.clone(),
        bodies: model
            .bodies
            .iter()
            .map(|(name, solid)| (name.clone(), solid.compress()))
            .collect(),
        groups: stored_groups(&model.groups),
        colours: model.colours.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        opacities: model
            .opacities
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect(),
        materials: model
            .materials
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        sources: model
            .sources
            .iter()
            .map(|(k, s)| (k.clone(), s.file.clone(), s.body.clone(), s.vars.clone()))
            .collect(),
        placements: model
            .placements
            .iter()
            .map(|(k, m)| (k.clone(), (*m).into()))
            .collect(),
        axes: model
            .axes
            .iter()
            .map(|(k, (p, v))| (k.clone(), (*p).into(), (*v).into()))
            .collect(),
        filled: model.filled.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        stand_ins: model
            .stand_ins
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        assembly: model.assembly,
        joints: model.joints.clone(),
        mates: model.mates.clone(),
        couples: model.couples.clone(),
        explode: model
            .explode
            .iter()
            .map(|(k, v)| (k.clone(), (*v).into()))
            .collect(),
        part_groups: model
            .part_groups
            .iter()
            .map(|(k, g)| {
                let (groups, placed) = g.parts();
                (k.clone(), stored_groups(groups), placed.into())
            })
            .collect(),
    })
}

fn restored(stored: Stored) -> Option<Model> {
    let solid = match stored.solid {
        Some(body) => Some(Solid::extract(body).ok()?),
        None => None,
    };
    let bodies = stored
        .bodies
        .into_iter()
        .map(|(name, body)| Some((name, Solid::extract(body).ok()?)))
        .collect::<Option<Vec<_>>>()?;
    Some(Model {
        solid,
        body: stored.body,
        bodies,
        groups: restored_groups(stored.groups),
        colours: stored.colours.into_iter().collect(),
        opacities: stored.opacities.into_iter().collect(),
        materials: stored.materials.into_iter().collect(),
        sources: stored
            .sources
            .into_iter()
            .map(|(k, file, body, vars)| (k, Source { file, body, vars }))
            .collect(),
        placements: stored
            .placements
            .into_iter()
            .map(|(k, m)| (k, m.into()))
            .collect(),
        axes: stored
            .axes
            .into_iter()
            .map(|(k, p, v)| (k, (p.into(), v.into())))
            .collect(),
        filled: stored.filled.into_iter().collect(),
        stand_ins: stored.stand_ins.into_iter().collect(),
        joints: stored.joints,
        mates: stored.mates,
        couples: stored.couples,
        explode: stored
            .explode
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect(),
        part_groups: stored
            .part_groups
            .into_iter()
            .map(|(k, groups, placed)| {
                (
                    k,
                    PartGroups::from_parts(restored_groups(groups), placed.into()),
                )
            })
            .collect(),
        assembly: stored.assembly,
        ..Model::default()
    })
}

fn stored_groups(groups: &Groups) -> Vec<StoredGroup> {
    groups
        .0
        .iter()
        .map(|entry| {
            (
                entry.label.clone(),
                entry.group.clone(),
                entry.surface.clone(),
            )
        })
        .collect()
}

fn restored_groups(groups: Vec<StoredGroup>) -> Groups {
    Groups(
        groups
            .into_iter()
            .map(|(label, group, surface)| GroupEntry {
                label,
                group,
                surface,
            })
            .collect(),
    )
}
