use gcad2glb::cache::{self, AssetCache, Index};
use gcad2glb::{Got, Options, Sources};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

const BODY: &str = "blob 0,0,0 r=2,2,2\nremesh 0.6\n";

fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gcad2glb-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn key(path: &Path, options: &Options) -> u64 {
    Sources::from_path(path).unwrap().key(options).unwrap()
}

fn model(dir: &Path) -> PathBuf {
    write(&dir.join("model.gcad"), "include parts/body.gcad\n");
    write(&dir.join("parts/body.gcad"), "include cap.gcad\n");
    write(&dir.join("parts/cap.gcad"), BODY);
    dir.join("model.gcad")
}

fn age(file: &Path, days: u64) {
    std::fs::File::options()
        .append(true)
        .open(file)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(days * 86400))
        .unwrap();
}

#[test]
fn includes_expand_from_the_folder_of_each_file() {
    let dir = tempdir("expand");
    let sources = Sources::from_path(model(&dir)).unwrap();
    assert_eq!(sources.source().unwrap(), BODY);
    let files: Vec<&str> = sources.files().map(|(p, _)| p).collect();
    assert_eq!(files, ["parts/body.gcad", "parts/cap.gcad"]);
}

#[test]
fn a_source_read_through_a_callback_gives_the_same_key_as_from_disk() {
    let dir = tempdir("callback");
    let path = model(&dir);
    let text = std::fs::read_to_string(&path).unwrap();
    let mut asked = Vec::new();
    let sources = Sources::read("models/model.gcad", &text, |wanted| {
        asked.push(wanted.to_string());
        std::fs::read(dir.join(wanted.strip_prefix("models/").unwrap())).ok()
    })
    .unwrap();
    assert_eq!(asked, ["models/parts/body.gcad", "models/parts/cap.gcad"]);
    assert_eq!(
        sources.key(&Options::default()).unwrap(),
        key(&path, &Options::default())
    );
}

#[test]
fn a_missing_include_is_an_error_and_a_missing_reference_is_not() {
    let dir = tempdir("missing");
    write(&dir.join("a.gcad"), "include gone.gcad\n");
    assert!(Sources::from_path(dir.join("a.gcad")).is_err());
    write(&dir.join("b.gcad"), "part bolt.gcad\nstamp grit.png\n");
    let sources = Sources::from_path(dir.join("b.gcad")).unwrap();
    assert!(sources.files().all(|(_, hash)| hash.is_none()));
    let before = sources.key(&Options::default()).unwrap();
    write(&dir.join("bolt.gcad"), BODY);
    assert_ne!(before, key(&dir.join("b.gcad"), &Options::default()));
}

#[test]
fn every_input_of_the_build_changes_the_key() {
    let dir = tempdir("inputs");
    let path = model(&dir);
    let base = Options::default();
    let first = key(&path, &base);
    assert_eq!(first, key(&path, &base), "same inputs, same key");

    write(
        &dir.join("parts/cap.gcad"),
        "blob 0,0,0 r=2,2,3\nremesh 0.6\n",
    );
    let included = key(&path, &base);
    assert_ne!(first, included, "an edit in a nested include");

    write(
        &dir.join("parts/body.gcad"),
        "include cap.gcad\npart bolt.gcad\n",
    );
    write(&dir.join("parts/bolt.gcad"), "rect 2 2\nextrude 1\n");
    let with_part = key(&path, &base);
    assert_ne!(included, with_part);
    write(&dir.join("parts/bolt.gcad"), "rect 2 3\nextrude 1\n");
    let part_edited = key(&path, &base);
    assert_ne!(with_part, part_edited, "an edit in a part file");

    let mut tall = base.clone();
    tall.fit.height = Some(2.0);
    assert_ne!(part_edited, key(&path, &tall), "the fit height");
    assert_ne!(
        part_edited,
        key(&path, &base.clone().set("r", 3.0)),
        "a variable"
    );

    let sources = Sources::from_path(&path).unwrap();
    assert_ne!(
        sources.key(&base).unwrap(),
        sources.key_with("0.0.0-other", &base).unwrap(),
        "the gcad version"
    );
}

#[test]
fn a_hit_builds_nothing_and_returns_the_same_glb() {
    let dir = tempdir("hit");
    let store = AssetCache::new(dir.join("cache"));
    let path = model(&dir);
    let (file, got) = store.get_or_build_path(&path, &Options::default()).unwrap();
    assert_eq!(got, Got::Built);
    let built = std::fs::read(&file).unwrap();
    assert!(cache::is_glb(&built));
    let direct = gcad2glb::Assets::from_source(BODY, &Options::default())
        .unwrap()
        .to_glb()
        .unwrap();
    assert_eq!(built, direct, "the cache stores what to_glb writes");
    let meta = std::fs::read_to_string(file.with_extension("json")).unwrap();
    let meta: serde_json::Value = serde_json::from_str(&meta).unwrap();
    assert_eq!(meta["gcad"], gcad2glb::VERSION);
    assert!(meta["files"]["parts/cap.gcad"].is_string());
    let (again, got) = store.get_or_build_path(&path, &Options::default()).unwrap();
    assert_eq!((again, got), (file.clone(), Got::Hit));
    write(
        &dir.join("parts/cap.gcad"),
        "blob 0,0,0 r=2,2,3\nremesh 0.6\n",
    );
    let (edited, got) = store.get_or_build_path(&path, &Options::default()).unwrap();
    assert_eq!(got, Got::Built);
    assert_ne!(edited, file);
}

#[test]
fn a_truncated_or_foreign_file_is_ignored_and_rebuilt() {
    let dir = tempdir("corrupt");
    let store = AssetCache::new(dir.join("cache"));
    let path = model(&dir);
    let (file, _) = store.get_or_build_path(&path, &Options::default()).unwrap();
    let good = std::fs::read(&file).unwrap();
    for bad in [
        good[..good.len() / 2].to_vec(),
        b"not a glb at all".to_vec(),
        Vec::new(),
    ] {
        std::fs::write(&file, &bad).unwrap();
        let (_, got) = store.get_or_build_path(&path, &Options::default()).unwrap();
        assert_eq!(got, Got::Built);
        assert_eq!(std::fs::read(&file).unwrap(), good);
    }
}

#[test]
fn threads_writing_one_key_leave_one_valid_file() {
    let dir = tempdir("race");
    let path = model(&dir);
    let cache_dir = dir.join("cache");
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let (path, cache_dir) = (path.clone(), cache_dir.clone());
            std::thread::spawn(move || {
                AssetCache::new(cache_dir)
                    .get_or_build_path(path, &Options::default())
                    .unwrap()
                    .0
            })
        })
        .collect();
    let files: HashSet<PathBuf> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(files.len(), 1);
    let names: Vec<_> = std::fs::read_dir(&cache_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "one glb and its sidecar: {names:?}");
    assert!(cache::is_glb(
        &std::fs::read(files.iter().next().unwrap()).unwrap()
    ));
}

#[test]
fn processes_writing_one_key_leave_one_valid_file() {
    let dir = tempdir("procs");
    let path = model(&dir);
    let cache_dir = dir.join("cache");
    let children: Vec<_> = (0..4)
        .map(|i| {
            Command::new(env!("CARGO_BIN_EXE_gcad2glb"))
                .arg("--cache")
                .arg(&cache_dir)
                .arg(&path)
                .arg(dir.join(format!("out{i}.glb")))
                .spawn()
                .unwrap()
        })
        .collect();
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let entries = AssetCache::new(&cache_dir).entries();
    assert_eq!(entries.len(), 1);
    let one = std::fs::read(dir.join("out0.glb")).unwrap();
    assert!(cache::is_glb(&one));
    for i in 1..4 {
        assert_eq!(std::fs::read(dir.join(format!("out{i}.glb"))).unwrap(), one);
    }
}

#[test]
fn the_cli_reports_a_hit_and_prints_the_library_key() {
    let dir = tempdir("cli");
    let path = model(&dir);
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_gcad2glb"))
            .args(args)
            .output()
            .unwrap()
    };
    let cache_dir = dir.join("cache");
    let (c, p, o) = (
        cache_dir.to_str().unwrap(),
        path.to_str().unwrap(),
        dir.join("out.glb"),
    );
    let first = run(&["--height", "1", "--cache", c, p, o.to_str().unwrap()]);
    assert!(first.status.success(), "{first:?}");
    assert!(!String::from_utf8_lossy(&first.stderr).contains("cached"));
    let second = run(&["--height", "1", "--cache", c, p, o.to_str().unwrap()]);
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("cached"),
        "{second:?}"
    );
    let printed = run(&["key", "--height", "1", p]);
    let mut options = Options::default();
    options.fit.height = Some(1.0);
    assert_eq!(
        String::from_utf8_lossy(&printed.stdout).trim(),
        format!("{:016x}", key(&path, &options))
    );
    assert!(
        AssetCache::new(&cache_dir)
            .path(key(&path, &options))
            .is_file()
    );
}

#[test]
fn bake_writes_an_index_and_check_finds_unbaked_and_stale_sources() {
    let dir = tempdir("bake");
    let src = dir.join("models");
    let out = src.join(".baked");
    model(&src);
    write(&src.join("rock.gcad"), BODY);
    let options = |_: &str| Some(Options::default());
    let baked = cache::bake(&src, &out, options).unwrap();
    let paths: Vec<&str> = baked.index.entries.keys().map(String::as_str).collect();
    assert_eq!(
        paths,
        ["model.gcad", "rock.gcad"],
        "included files are not baked alone"
    );
    assert_eq!(baked.built.len(), 2);
    let index = Index::read(&out).unwrap();
    assert_eq!(index, baked.index);
    assert_eq!(index.gcad, gcad2glb::VERSION);
    assert_eq!(
        index.entries["rock.gcad"].key,
        key(&src.join("rock.gcad"), &Options::default())
    );
    assert!(cache::check(&src, &out, options).is_empty());

    let again = cache::bake(&src, &out, options).unwrap();
    assert_eq!((again.built.len(), again.reused.len()), (0, 2));

    write(
        &src.join("tree.gcad"),
        BODY.replace("2,2,2", "1,3,1").as_str(),
    );
    write(
        &src.join("parts/cap.gcad"),
        "blob 0,0,0 r=3,2,2\nremesh 0.6\n",
    );
    let problems = cache::check(&src, &out, options);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems[0].starts_with("model.gcad: stale"), "{problems:?}");
    assert_eq!(problems[1], "tree.gcad: not baked");

    let rebaked = cache::bake(&src, &out, options).unwrap();
    assert_eq!(rebaked.built, ["model.gcad", "tree.gcad"]);
    assert_eq!(rebaked.removed, 1, "the old model entry goes");
    assert_eq!(AssetCache::new(&out).entries().len(), 3);
    assert!(cache::check(&src, &out, options).is_empty());

    std::fs::write(
        AssetCache::new(&out).path(index.entries["rock.gcad"].key),
        b"bad",
    )
    .unwrap();
    assert_eq!(cache::check(&src, &out, options).len(), 1);
}

#[test]
fn the_bake_cli_check_fails_on_an_unbaked_source() {
    let dir = tempdir("bake-cli");
    let src = dir.join("models");
    let out = src.join(".baked");
    write(&src.join("rock.gcad"), BODY);
    let run = |args: &[&Path]| {
        Command::new(env!("CARGO_BIN_EXE_gcad2glb"))
            .args(args)
            .status()
            .unwrap()
            .success()
    };
    let (bake, check) = (Path::new("bake"), Path::new("--check"));
    assert!(!run(&[bake, check, &src, &out]), "nothing baked yet");
    assert!(run(&[bake, &src, &out]));
    assert!(run(&[bake, check, &src, &out]));
    write(&src.join("tree.gcad"), BODY);
    assert!(!run(&[bake, check, &src, &out]));
    assert!(!run(&[
        bake,
        check,
        Path::new("--height"),
        Path::new("1"),
        &src,
        &out
    ]));
}

#[test]
fn prune_drops_old_and_oversize_entries_but_keeps_the_kept_ones() {
    let dir = tempdir("prune");
    let store = AssetCache::new(dir.join("cache"));
    let glb = |n: usize| {
        let mut bytes = vec![0u8; 1000 * n];
        bytes[0..4].copy_from_slice(b"glTF");
        bytes
    };
    for key in 1..=5u64 {
        store.put(key, &glb(key as usize), "{}").unwrap();
    }
    age(&store.path(1), 40);
    age(&store.path(2), 40);
    age(&store.path(3), 2);
    age(&store.path(4), 1);
    std::fs::write(dir.join("cache/.0000000000000009.glb.1-0.part"), b"x").unwrap();
    age(&dir.join("cache/.0000000000000009.glb.1-0.part"), 1);
    let keep: HashSet<u64> = [2].into();
    let pruned = store.prune(9_100, cache::DEFAULT_MAX_AGE, &keep);
    let mut left: Vec<u64> = store.entries().iter().map(|e| e.0).collect();
    left.sort();
    assert_eq!(
        left,
        [2, 5],
        "2 is kept, 5 is newest, 4 and 3 pass the cap, 1 is old"
    );
    assert_eq!(pruned.removed, 3);
    assert_eq!(pruned.bytes, 2002 + 5002);
    assert!(!store.path(1).with_extension("json").exists());
    assert!(!dir.join("cache/.0000000000000009.glb.1-0.part").exists());

    assert!(
        store.get(5, cache::is_glb).is_none(),
        "a bad file reads as a miss"
    );
    assert!(!store.path(5).exists(), "and is removed");
}

#[test]
fn a_read_touches_the_entry_so_prune_keeps_it() {
    let dir = tempdir("touch");
    let store = AssetCache::new(dir.join("cache"));
    let path = model(&dir);
    let (file, _) = store.get_or_build_path(&path, &Options::default()).unwrap();
    age(&file, 40);
    store.get_or_build_path(&path, &Options::default()).unwrap();
    let pruned = store.prune(u64::MAX, cache::DEFAULT_MAX_AGE, &HashSet::new());
    assert_eq!(pruned.removed, 0);
    assert!(file.exists());
}
