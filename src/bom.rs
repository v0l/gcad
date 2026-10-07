use crate::geometry;
use crate::model::{Model, Source};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Item {
    pub file: String,
    pub body: String,
    pub variant: String,
    pub names: Vec<String>,
    pub material: Option<String>,
    pub volume: f64,
    pub grams: Option<f64>,
}

fn shown(file: &Path, base: &Path) -> String {
    let base = base.parent().unwrap_or(Path::new(""));
    pathdiff(file, base)
        .unwrap_or_else(|| file.to_path_buf())
        .display()
        .to_string()
}

fn pathdiff(path: &Path, base: &Path) -> Option<std::path::PathBuf> {
    let path = std::fs::canonicalize(path).ok()?;
    let base = std::fs::canonicalize(if base.as_os_str().is_empty() {
        Path::new(".")
    } else {
        base
    })
    .ok()?;
    let mut ups = std::path::PathBuf::new();
    let mut here = base.as_path();
    loop {
        if let Ok(rest) = path.strip_prefix(here) {
            return Some(ups.join(rest));
        }
        ups.push("..");
        here = here.parent()?;
    }
}

pub fn items(model: &Model, file: &Path) -> Vec<Item> {
    let mut found: Vec<(Source, Item)> = Vec::new();
    for (name, solid) in model.named_solids() {
        let source = model.sources.get(&name).cloned().unwrap_or_else(|| Source {
            file: file.to_path_buf(),
            body: name.clone(),
            vars: Vec::new(),
        });
        if let Some((_, item)) = found.iter_mut().find(|(s, _)| *s == source) {
            item.names.push(name);
            continue;
        }
        let material = model.materials.get(&name);
        let volume = geometry::volume(solid).abs();
        let item = Item {
            file: shown(&source.file, file),
            body: source.body.clone(),
            variant: source
                .vars
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" "),
            names: vec![name],
            material: material.map(|m| m.name.clone()),
            volume,
            grams: material.map(|m| volume * m.density / 1000.0),
        };
        found.push((source, item));
    }
    found.into_iter().map(|(_, item)| item).collect()
}

pub fn table(items: &[Item]) -> String {
    let rows: Vec<[String; 8]> = items
        .iter()
        .map(|item| {
            [
                item.names.len().to_string(),
                item.file.clone(),
                item.body.clone(),
                item.variant.clone(),
                item.material.clone().unwrap_or_else(|| "-".into()),
                format!("{:.1}", item.volume),
                item.grams
                    .map(|g| format!("{g:.2}"))
                    .unwrap_or_else(|| "-".into()),
                item.names.join(" "),
            ]
        })
        .collect();
    let head = [
        "qty",
        "file",
        "body",
        "variables",
        "material",
        "mm³ each",
        "g each",
        "parts",
    ];
    let width = |k: usize| {
        rows.iter()
            .map(|r| r[k].chars().count())
            .chain([head[k].chars().count()])
            .max()
            .unwrap_or(0)
    };
    let widths: Vec<usize> = (0..8).map(width).collect();
    let line = |cells: Vec<String>| {
        cells
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let pad = widths[k] - c.chars().count();
                if k == 0 || k == 5 || k == 6 {
                    format!("{}{c}", " ".repeat(pad))
                } else {
                    format!("{c}{}", " ".repeat(pad))
                }
            })
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    let mut out = vec![line(head.iter().map(|h| h.to_string()).collect())];
    out.extend(rows.into_iter().map(|r| line(r.to_vec())));
    let count: usize = items.iter().map(|i| i.names.len()).sum();
    let grams: f64 = items
        .iter()
        .filter_map(|i| i.grams.map(|g| g * i.names.len() as f64))
        .sum();
    let missing = items.iter().any(|i| i.grams.is_none());
    out.push(format!(
        "{count} parts{}",
        if grams > 0.0 {
            format!(
                ", {grams:.2} g{}",
                if missing {
                    " without the parts that have no material"
                } else {
                    ""
                }
            )
        } else {
            String::new()
        }
    ));
    out.join("\n")
}

pub fn csv(items: &[Item]) -> String {
    let quote = |s: &str| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let mut out = vec!["qty,file,body,variables,material,volume_mm3,mass_g,names".to_string()];
    for item in items {
        out.push(
            [
                item.names.len().to_string(),
                quote(&item.file),
                quote(&item.body),
                quote(&item.variant),
                quote(item.material.as_deref().unwrap_or("")),
                format!("{:.3}", item.volume),
                item.grams.map(|g| format!("{g:.3}")).unwrap_or_default(),
                quote(&item.names.join(" ")),
            ]
            .join(","),
        );
    }
    out.join("\n") + "\n"
}
