use gcad2glb::cache::{self, AssetCache, DEFAULT_MAX_AGE, DEFAULT_MAX_BYTES, Index};
use gcad2glb::{Got, Options, Sources};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "usage:
  gcad2glb [OPTIONS] [--cache DIR] in.gcad out.glb
  gcad2glb [OPTIONS] --cache DIR in.gcad          (prints the cached GLB's path)
  gcad2glb key [OPTIONS] in.gcad
  gcad2glb bake [--check] [OPTIONS] SRC_DIR OUT_DIR
  gcad2glb cache prune DIR [--max-bytes N] [--max-age DAYS] [--keep index.json]...
OPTIONS: [--detail N] [--z-up] [--tris N] [--max-error E] [--height H | --scale K] [--centre] [--set NAME=VALUE]";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("key") => key(&args.split_off(1)),
        Some("bake") => bake(&args.split_off(1)),
        Some("cache") if args.get(1).is_some_and(|a| a == "prune") => prune(&args.split_off(2)),
        _ => convert(&args),
    };
    match result {
        Ok(code) => code,
        Err(problem) => usage(&problem),
    }
}

struct Parsed {
    options: Options,
    flags: Vec<(String, String)>,
    switches: Vec<String>,
    positional: Vec<String>,
}

fn parse(args: &[String], valued: &[&str], switches: &[&str]) -> Result<Parsed, String> {
    let mut out = Parsed {
        options: Options::default(),
        flags: Vec::new(),
        switches: Vec::new(),
        positional: Vec::new(),
    };
    let options = &mut out.options;
    let mut args = args.iter().cloned();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--detail" => match args.next().and_then(|v| v.parse().ok()) {
                Some(v) => options.detail = v,
                None => return Err("--detail takes a number".into()),
            },
            "--z-up" => options.y_up = false,
            "--tris" => match args.next().and_then(|v| v.parse().ok()) {
                Some(v) if v >= 4 => options.triangles = Some(v),
                _ => return Err("--tris takes a whole number of triangles, at least 4".into()),
            },
            "--max-error" => match args.next().and_then(|v| v.parse::<f64>().ok()) {
                Some(v) if v > 0.0 => options.max_error = Some(v),
                _ => return Err("--max-error takes a distance above 0 in model units".into()),
            },
            "--height" => match args.next().and_then(|v| v.parse::<f64>().ok()) {
                Some(v) if v > 0.0 => options.fit.height = Some(v),
                _ => return Err("--height takes a height above 0".into()),
            },
            "--scale" => match args.next().and_then(|v| v.parse::<f64>().ok()) {
                Some(v) if v > 0.0 => options.fit.scale = Some(v),
                _ => return Err("--scale takes a factor above 0".into()),
            },
            "--centre" => options.fit.centre = true,
            "--set" => {
                let pair = args.next().unwrap_or_default();
                match pair.split_once('=').map(|(n, v)| (n, v.parse::<f64>())) {
                    Some((name, Ok(value))) if !name.is_empty() => {
                        *options = std::mem::take(options).set(name, value)
                    }
                    _ => return Err("--set takes NAME=VALUE".into()),
                }
            }
            "-h" | "--help" => return Err(String::new()),
            flag if valued.contains(&flag) => match args.next() {
                Some(value) => out.flags.push((arg, value)),
                None => return Err(format!("{arg} takes a value")),
            },
            flag if switches.contains(&flag) => out.switches.push(arg),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ => out.positional.push(arg),
        }
    }
    Ok(out)
}

impl Parsed {
    fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn all(&self, name: &str) -> impl Iterator<Item = &str> {
        self.flags
            .iter()
            .filter(move |(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn convert(args: &[String]) -> Result<ExitCode, String> {
    let parsed = parse(args, &["--cache"], &[])?;
    if let (Some(dir), [input]) = (parsed.flag("--cache"), parsed.positional.as_slice()) {
        return Ok(
            match AssetCache::new(dir).get_or_build_path(input, &parsed.options) {
                Ok((file, got)) => {
                    eprintln!(
                        "{} {}",
                        if got == Got::Hit { "cached" } else { "built" },
                        file.display()
                    );
                    println!("{}", file.display());
                    ExitCode::SUCCESS
                }
                Err(e) => fail(&format!("{input}: {e}")),
            },
        );
    }
    let [input, output] = parsed.positional.as_slice() else {
        return Err(match parsed.positional.len() {
            n if n > 2 => "too many arguments".into(),
            _ => "needs an input and an output".into(),
        });
    };
    if let Some(dir) = parsed.flag("--cache") {
        return Ok(
            match AssetCache::new(dir).get_or_build_path(input, &parsed.options) {
                Ok((file, got)) => match std::fs::copy(&file, output) {
                    Ok(_) => {
                        if got == Got::Hit {
                            eprintln!("cached {}", file.display());
                        }
                        ExitCode::SUCCESS
                    }
                    Err(e) => fail(&format!("{output}: {e}")),
                },
                Err(e) => fail(&format!("{input}: {e}")),
            },
        );
    }
    let built = gcad2glb::Assets::from_path(input, &parsed.options).and_then(|assets| {
        let bytes = assets.to_glb()?;
        Ok((assets.notes, bytes))
    });
    Ok(match built {
        Ok((notes, bytes)) => match std::fs::write(output, bytes) {
            Ok(()) => {
                for note in notes {
                    eprintln!("{note}");
                }
                ExitCode::SUCCESS
            }
            Err(e) => fail(&format!("{output}: {e}")),
        },
        Err(message) => fail(&format!("{input}: {message}")),
    })
}

fn key(args: &[String]) -> Result<ExitCode, String> {
    let parsed = parse(args, &[], &[])?;
    let [input] = parsed.positional.as_slice() else {
        return Err("key takes one file".into());
    };
    Ok(
        match Sources::from_path(input).and_then(|s| s.key(&parsed.options)) {
            Ok(key) => {
                println!("{key:016x}");
                ExitCode::SUCCESS
            }
            Err(e) => fail(&format!("{input}: {e}")),
        },
    )
}

fn bake(args: &[String]) -> Result<ExitCode, String> {
    let parsed = parse(args, &[], &["--check"])?;
    let [src, out] = parsed.positional.as_slice() else {
        return Err("bake takes a source folder and an output folder".into());
    };
    let (src, out) = (PathBuf::from(src), PathBuf::from(out));
    let options = parsed.options.clone();
    if parsed.switches.iter().any(|s| s == "--check") {
        let problems = cache::check(&src, &out, |_| Some(options.clone()));
        for problem in &problems {
            eprintln!("{problem}");
        }
        return Ok(match problems.is_empty() {
            true => ExitCode::SUCCESS,
            false => ExitCode::FAILURE,
        });
    }
    Ok(match cache::bake(&src, &out, |_| Some(options.clone())) {
        Ok(baked) => {
            eprintln!(
                "baked {} file(s): {} built, {} unchanged, {} old entr(ies) removed",
                baked.index.entries.len(),
                baked.built.len(),
                baked.reused.len(),
                baked.removed
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    })
}

fn prune(args: &[String]) -> Result<ExitCode, String> {
    let parsed = parse(args, &["--max-bytes", "--max-age", "--keep"], &[])?;
    let [dir] = parsed.positional.as_slice() else {
        return Err("cache prune takes one folder".into());
    };
    let max_bytes = match parsed.flag("--max-bytes") {
        Some(v) => v
            .parse()
            .map_err(|_| "--max-bytes takes a number of bytes")?,
        None => DEFAULT_MAX_BYTES,
    };
    let max_age = match parsed.flag("--max-age") {
        Some(v) => Duration::from_secs_f64(
            v.parse::<f64>()
                .map_err(|_| "--max-age takes a number of days")?
                * 86400.0,
        ),
        None => DEFAULT_MAX_AGE,
    };
    let mut keep = HashSet::new();
    for index in parsed.all("--keep") {
        let text = std::fs::read_to_string(index).map_err(|e| format!("{index}: {e}"))?;
        keep.extend(
            Index::from_json(&text)
                .map_err(|e| format!("{index}: {e}"))?
                .keys(),
        );
    }
    let pruned = AssetCache::new(dir).prune(max_bytes, max_age, &keep);
    eprintln!(
        "removed {} entr(ies), {} bytes; kept {}, {} bytes",
        pruned.removed, pruned.freed, pruned.kept, pruned.bytes
    );
    Ok(ExitCode::SUCCESS)
}

fn fail(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::FAILURE
}

fn usage(problem: &str) -> ExitCode {
    if !problem.is_empty() {
        eprintln!("{problem}");
    }
    eprintln!("{USAGE}");
    if problem.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
