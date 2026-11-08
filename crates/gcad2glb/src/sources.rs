use crate::{Error, Fnv, Options, cache_key_with, plain};
use std::collections::BTreeMap;
use std::path::Path;

const REFERENCED: &[&str] = &[
    ".gcad", ".gasm", ".png", ".svg", ".dxf", ".step", ".stp", ".stl", ".ttf",
];
const MOST_FILES: usize = 256;
const DEEPEST_INCLUDE: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Include,
    Reference,
}

#[derive(Clone, Debug)]
pub struct Sources {
    dir: String,
    root: String,
    files: BTreeMap<String, Option<Vec<u8>>>,
    roles: BTreeMap<String, Role>,
}

pub fn include_of(line: &str) -> Option<&str> {
    let name = line
        .trim()
        .strip_prefix("include ")?
        .split_whitespace()
        .next()?;
    (!name.contains('=')).then_some(name)
}

pub fn references(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text
        .lines()
        .filter(|line| include_of(line).is_none() && !line.trim_start().starts_with('#'))
        .flat_map(|line| line.split_whitespace())
        .map(|token| {
            token
                .rsplit('=')
                .next()
                .unwrap_or(token)
                .trim_matches(|c| c == '"' || c == ',')
        })
        .filter(|token| {
            let lower = token.to_ascii_lowercase();
            REFERENCED.iter().any(|ext| lower.ends_with(ext)) && !token.starts_with("std:")
        })
        .map(str::to_string)
        .collect();
    out.sort();
    out.dedup();
    out
}

pub fn join(dir: &str, name: &str) -> String {
    if name.starts_with('/') || Path::new(name).is_absolute() {
        return name.to_string();
    }
    let mut parts: Vec<&str> = dir
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let joined = parts.join("/");
    match dir.starts_with('/') {
        true => format!("/{joined}"),
        false => joined,
    }
}

fn parent(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

fn is_text(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".gcad") || lower.ends_with(".gasm")
}

impl Sources {
    pub fn new(name: &str, text: &str) -> Sources {
        let name = name.replace('\\', "/");
        let mut sources = Sources {
            dir: parent(&name).to_string(),
            root: name.rsplit('/').next().unwrap_or(&name).to_string(),
            files: BTreeMap::new(),
            roles: BTreeMap::new(),
        };
        sources
            .files
            .insert(sources.root.clone(), Some(text.as_bytes().to_vec()));
        sources.scan(&sources.root.clone());
        sources
    }

    pub fn read(
        name: &str,
        text: &str,
        mut read: impl FnMut(&str) -> Option<Vec<u8>>,
    ) -> Result<Sources, Error> {
        let mut sources = Sources::new(name, text);
        loop {
            let wanted = sources.wanted();
            if wanted.is_empty() {
                return Ok(sources);
            }
            for path in wanted {
                let bytes = read(&path);
                sources.supply(&path, bytes)?;
            }
        }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Sources, Error> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|e| plain(format!("reading {}: {e}", path.display())))?;
        let dir = path.parent().unwrap_or(Path::new(""));
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Sources::read(&name, &text, |wanted| std::fs::read(dir.join(wanted)).ok())
    }

    pub fn name(&self) -> String {
        join(&self.dir, &self.root)
    }

    pub fn wanted(&self) -> Vec<String> {
        self.roles
            .keys()
            .filter(|p| !self.files.contains_key(*p))
            .map(|p| join(&self.dir, p))
            .collect()
    }

    pub fn supply(&mut self, path: &str, bytes: Option<Vec<u8>>) -> Result<(), Error> {
        let local = self
            .roles
            .keys()
            .find(|p| join(&self.dir, p) == path.replace('\\', "/"))
            .cloned()
            .ok_or_else(|| plain(format!("{path} was not asked for")))?;
        if bytes.is_none() && self.roles[&local] == Role::Include {
            return Err(plain(format!("include {path} not found")));
        }
        if self.files.len() >= MOST_FILES {
            return Err(plain(format!("more than {MOST_FILES} files")));
        }
        self.files.insert(local.clone(), bytes);
        self.scan(&local);
        Ok(())
    }

    fn scan(&mut self, local: &str) {
        let Some(Some(bytes)) = self.files.get(local) else {
            return;
        };
        if !is_text(local) {
            return;
        }
        let text = String::from_utf8_lossy(bytes).into_owned();
        let dir = parent(local).to_string();
        for line in text.lines() {
            if let Some(name) = include_of(line) {
                self.want(join(&dir, name), Role::Include);
            }
        }
        for name in references(&text) {
            self.want(join(&dir, &name), Role::Reference);
        }
    }

    fn want(&mut self, local: String, role: Role) {
        let entry = self.roles.entry(local).or_insert(role.clone());
        if role == Role::Include {
            *entry = Role::Include;
        }
    }

    fn text(&self, local: &str) -> Result<String, Error> {
        match self.files.get(local) {
            Some(Some(bytes)) => String::from_utf8(bytes.clone())
                .map_err(|e| plain(format!("{}: {e}", join(&self.dir, local)))),
            _ => Err(plain(format!(
                "include {} not found",
                join(&self.dir, local)
            ))),
        }
    }

    pub fn has_references(&self) -> bool {
        self.roles.values().any(|r| *r == Role::Reference)
    }

    pub fn source(&self) -> Result<String, Error> {
        self.expand(&self.root, 0)
    }

    fn expand(&self, local: &str, depth: usize) -> Result<String, Error> {
        if depth > DEEPEST_INCLUDE {
            return Err(plain(format!(
                "includes are nested more than {DEEPEST_INCLUDE} deep"
            )));
        }
        let text = self.text(local)?;
        let dir = parent(local);
        let mut out = String::with_capacity(text.len());
        for line in text.lines() {
            match include_of(line) {
                Some(name) => out.push_str(&self.expand(&join(dir, name), depth + 1)?),
                None => {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        Ok(out)
    }

    pub fn files(&self) -> impl Iterator<Item = (&str, Option<u64>)> {
        self.files
            .iter()
            .filter(|(p, _)| **p != self.root)
            .map(|(p, b)| {
                (
                    p.as_str(),
                    b.as_ref().map(|b| {
                        let mut h = Fnv::default();
                        h.bytes(b);
                        h.0
                    }),
                )
            })
    }

    pub fn key(&self, options: &Options) -> Result<u64, Error> {
        self.key_with(crate::VERSION, options)
    }

    pub fn key_with(&self, version: &str, options: &Options) -> Result<u64, Error> {
        if !self.wanted().is_empty() {
            return Err(plain(format!(
                "{}: files not read yet: {}",
                self.name(),
                self.wanted().join(", ")
            )));
        }
        let mut hash = Fnv(cache_key_with(version, &self.source()?, options));
        for (path, bytes) in self.files.iter().filter(|(p, _)| **p != self.root) {
            hash.text(path);
            match bytes {
                None => hash.bytes(&[0]),
                Some(b) => {
                    hash.bytes(&[1]);
                    hash.count(b.len());
                    hash.bytes(b);
                }
            }
        }
        Ok(hash.0)
    }
}

pub fn fold(key: u64, parts: &[&[u8]]) -> u64 {
    let mut hash = Fnv(key);
    for part in parts {
        hash.count(part.len());
        hash.bytes(part);
    }
    hash.0
}
