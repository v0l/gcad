use super::bodies::Mechanics;
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const LIBRARY: &str = "materials.toml";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Axes {
    pub young: [Option<f64>; 3],
    pub poisson: [Option<f64>; 3],
    pub shear: [Option<f64>; 3],
    pub tension: [Option<f64>; 3],
    pub compression: [Option<f64>; 3],
    pub shear_strength: [Option<f64>; 3],
    pub conductivity: [Option<f64>; 3],
    pub expansion: [Option<f64>; 3],
}

impl Axes {
    fn any(&self) -> bool {
        [
            self.young,
            self.poisson,
            self.shear,
            self.tension,
            self.compression,
            self.shear_strength,
            self.conductivity,
            self.expansion,
        ]
        .iter()
        .any(|set| set.iter().any(Option::is_some))
    }

    pub fn summary(&self) -> String {
        let list = |set: [Option<f64>; 3]| -> String {
            set.iter()
                .map(|v| v.map_or("-".to_string(), crate::plot::label))
                .collect::<Vec<_>>()
                .join("/")
        };
        let mut parts = Vec::new();
        if self.young.iter().any(Option::is_some) {
            parts.push(format!("E1/E2/E3 {} GPa", list(self.young)));
        }
        if self.tension.iter().any(Option::is_some) {
            parts.push(format!("Xt/Yt/Zt {} MPa", list(self.tension)));
        }
        parts.join(", ")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub density: Option<f64>,
    pub mechanics: Mechanics,
    pub axes: Option<Axes>,
    pub source: PathBuf,
}

fn smallest(set: &[Option<f64>; 3]) -> Option<f64> {
    set.iter().flatten().copied().reduce(f64::min)
}

fn largest(set: &[Option<f64>; 3]) -> Option<f64> {
    set.iter().flatten().copied().reduce(f64::max)
}

impl Entry {
    fn settle(mut self) -> Entry {
        if let Some(axes) = self.axes {
            let m = &mut self.mechanics;
            m.young = m.young.or(smallest(&axes.young));
            m.poisson = m.poisson.or(axes.poisson.iter().flatten().copied().next());
            let strengths = [smallest(&axes.tension), smallest(&axes.compression)];
            m.strength = m
                .strength
                .or(strengths.iter().flatten().copied().reduce(f64::min));
            m.conductivity = m.conductivity.or(smallest(&axes.conductivity));
            m.expansion = m.expansion.or(largest(&axes.expansion));
        }
        self
    }
}

pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

pub fn load(path: &Path) -> Result<Vec<Entry>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let entries = match extension.as_str() {
        "toml" => from_toml(&text, path),
        "xml" | "matml" => from_matml(&text, path),
        "fcmat" => from_fcmat(&text, path),
        other => {
            bail!("`.{other}` is not a material library; use .toml, MatML .xml or FreeCAD .FCMat")
        }
    }
    .with_context(|| format!("in {}", path.display()))?;
    Ok(entries.into_iter().map(Entry::settle).collect())
}

pub fn discover(dir: Option<&Path>) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut at = dir.map(Path::to_path_buf);
    while let Some(here) = at {
        let candidate = here.join(LIBRARY);
        if candidate.is_file() {
            found.push(candidate);
        }
        if here.join(".git").exists() {
            break;
        }
        at = here.parent().map(Path::to_path_buf);
    }
    if let Some(config) = config_library().filter(|p| p.is_file())
        && !found.contains(&config)
    {
        found.push(config);
    }
    found
}

fn config_library() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("gcad").join(LIBRARY))
}

pub fn discovered(dir: Option<&Path>) -> Result<HashMap<String, Entry>> {
    let mut library = HashMap::new();
    for file in discover(dir).into_iter().rev() {
        for entry in load(&file)? {
            library.insert(entry.name.clone(), entry);
        }
    }
    Ok(library)
}

const TOML_KEYS: &[&str] = &[
    "density", "E", "nu", "yield", "k", "alpha", "printed", "E1", "E2", "E3", "nu12", "nu13",
    "nu23", "G12", "G13", "G23", "Xt", "Yt", "Zt", "Xc", "Yc", "Zc", "S12", "S13", "S23", "k1",
    "k2", "k3", "alpha1", "alpha2", "alpha3", "source",
];

fn from_toml(text: &str, path: &Path) -> Result<Vec<Entry>> {
    let table: toml::Table = text.parse().map_err(|e| anyhow!("{e}"))?;
    let mut out = Vec::new();
    for (name, value) in &table {
        let toml::Value::Table(fields) = value else {
            bail!("`{name}` should be a table, like `[{name}]` followed by `density = 1.2`");
        };
        let mut number = |key: &str| -> Result<Option<f64>> {
            match fields.get(key) {
                None => Ok(None),
                Some(toml::Value::Float(v)) => Ok(Some(*v)),
                Some(toml::Value::Integer(v)) => Ok(Some(*v as f64)),
                Some(other) => bail!("`{name}.{key}` should be a number, not {other}"),
            }
        };
        for key in fields.keys() {
            if !TOML_KEYS.contains(&key.as_str()) {
                bail!(
                    "`{name}` has an unknown key `{key}`; keys are {}",
                    TOML_KEYS.join(", ")
                );
            }
        }
        let triple = |number: &mut dyn FnMut(&str) -> Result<Option<f64>>,
                      keys: [&str; 3]|
         -> Result<[Option<f64>; 3]> {
            Ok([number(keys[0])?, number(keys[1])?, number(keys[2])?])
        };
        let axes = Axes {
            young: triple(&mut number, ["E1", "E2", "E3"])?,
            poisson: triple(&mut number, ["nu12", "nu13", "nu23"])?,
            shear: triple(&mut number, ["G12", "G13", "G23"])?,
            tension: triple(&mut number, ["Xt", "Yt", "Zt"])?,
            compression: triple(&mut number, ["Xc", "Yc", "Zc"])?,
            shear_strength: triple(&mut number, ["S12", "S13", "S23"])?,
            conductivity: triple(&mut number, ["k1", "k2", "k3"])?,
            expansion: triple(&mut number, ["alpha1", "alpha2", "alpha3"])?,
        };
        let printed = match fields.get("printed") {
            None => false,
            Some(toml::Value::Boolean(b)) => *b,
            Some(other) => bail!("`{name}.printed` should be true or false, not {other}"),
        };
        let entry = Entry {
            name: slug(name),
            density: number("density")?,
            mechanics: Mechanics {
                young: number("E")?,
                poisson: number("nu")?,
                strength: number("yield")?,
                conductivity: number("k")?,
                expansion: number("alpha")?,
                printed,
            },
            axes: axes.any().then_some(axes),
            source: path.to_path_buf(),
        };
        check(&entry)?;
        out.push(entry);
    }
    Ok(out)
}

fn check(entry: &Entry) -> Result<()> {
    let name = &entry.name;
    let positive = |what: &str, v: Option<f64>| -> Result<()> {
        match v {
            Some(v) if v.is_nan() || v <= 0.0 => bail!("`{name}` has {what} {v}; it must be above zero"),
            _ => Ok(()),
        }
    };
    positive("density", entry.density)?;
    positive("E", entry.mechanics.young)?;
    positive("yield", entry.mechanics.strength)?;
    positive("k", entry.mechanics.conductivity)?;
    if let Some(nu) = entry.mechanics.poisson
        && !(-1.0..0.5).contains(&nu)
    {
        bail!("`{name}` has nu {nu}; an isotropic Poisson's ratio is below 0.5");
    }
    if let Some(axes) = &entry.axes {
        for (what, set) in [
            ("E1/E2/E3", axes.young),
            ("G12/G13/G23", axes.shear),
            ("k1/k2/k3", axes.conductivity),
        ] {
            for v in set.iter().flatten() {
                positive(what, Some(*v))?;
            }
        }
    }
    Ok(())
}

fn unit_factor(name: &str) -> Result<f64> {
    Ok(match name.trim() {
        "" | "1" => 1.0,
        "Pa" => 1.0,
        "kPa" => 1.0e3,
        "MPa" => 1.0e6,
        "GPa" => 1.0e9,
        "psi" => 6_894.757_293_168,
        "ksi" => 6.894_757_293_168e6,
        "N" => 1.0,
        "kN" => 1.0e3,
        "m" => 1.0,
        "cm" => 1.0e-2,
        "mm" => 1.0e-3,
        "um" | "µm" | "μm" => 1.0e-6,
        "in" => 0.0254,
        "ft" => 0.3048,
        "kg" => 1.0,
        "g" => 1.0e-3,
        "t" | "tonne" => 1.0e3,
        "lbm" | "lb" => 0.453_592_37,
        "W" => 1.0,
        "mW" => 1.0e-3,
        "J" => 1.0,
        "kJ" => 1.0e3,
        "mJ" => 1.0e-3,
        "s" => 1.0,
        "K" | "C" | "°C" | "degC" | "℃" => 1.0,
        "F" | "°F" | "degF" => 5.0 / 9.0,
        other => bail!("unknown unit `{other}`"),
    })
}

fn unit_expression(text: &str) -> Result<f64> {
    let mut factor = 1.0;
    let mut sign = 1.0;
    let mut token = String::new();
    let flush = |token: &mut String, sign: f64, factor: &mut f64| -> Result<()> {
        if token.is_empty() {
            return Ok(());
        }
        let (name, power) = match token.split_once('^') {
            Some((n, p)) => (
                n.to_string(),
                p.parse::<f64>()
                    .map_err(|_| anyhow!("bad power in `{token}`"))?,
            ),
            None => (token.clone(), 1.0),
        };
        *factor *= unit_factor(&name)?.powf(sign * power);
        token.clear();
        Ok(())
    };
    for c in text.chars() {
        match c {
            '/' => {
                flush(&mut token, sign, &mut factor)?;
                sign = -1.0;
            }
            '*' | '·' | ' ' => flush(&mut token, sign, &mut factor)?,
            _ => token.push(c),
        }
    }
    flush(&mut token, sign, &mut factor)?;
    Ok(factor)
}

fn quantity(text: &str) -> Result<f64> {
    let text = text.trim();
    let split = text
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .map_or(text.len(), |(i, _)| i);
    let (number, unit) = text.split_at(split);
    let value: f64 = number
        .trim()
        .parse()
        .map_err(|_| anyhow!("`{text}` is not a quantity"))?;
    Ok(value * unit_expression(unit.trim())?)
}

#[derive(Default)]
struct Si {
    density: Option<f64>,
    young: Option<f64>,
    poisson: Option<f64>,
    strength: Option<f64>,
    ultimate: Option<f64>,
    conductivity: Option<f64>,
    expansion: Option<f64>,
    axes_young: [Option<f64>; 3],
    axes_poisson: [Option<f64>; 3],
    axes_shear: [Option<f64>; 3],
    tension: [Option<f64>; 3],
    compression: [Option<f64>; 3],
    shear_strength: [Option<f64>; 3],
    axes_conductivity: [Option<f64>; 3],
    axes_expansion: [Option<f64>; 3],
}

impl Si {
    fn entry(self, name: &str, path: &Path) -> Result<Entry> {
        let gpa = |v: Option<f64>| v.map(|v| v * 1.0e-9);
        let mpa = |v: Option<f64>| v.map(|v| v * 1.0e-6);
        let micro = |v: Option<f64>| v.map(|v| v * 1.0e6);
        let axes = Axes {
            young: self.axes_young.map(gpa),
            poisson: self.axes_poisson,
            shear: self.axes_shear.map(gpa),
            tension: self.tension.map(mpa),
            compression: self.compression.map(|v| mpa(v).map(f64::abs)),
            shear_strength: self.shear_strength.map(mpa),
            conductivity: self.axes_conductivity,
            expansion: self.axes_expansion.map(micro),
        };
        let entry = Entry {
            name: slug(name),
            density: self.density.map(|v| v * 1.0e-3),
            mechanics: Mechanics {
                young: gpa(self.young),
                poisson: self.poisson,
                strength: mpa(self.strength.or(self.ultimate)),
                conductivity: self.conductivity,
                expansion: micro(self.expansion),
                printed: false,
            },
            axes: axes.any().then_some(axes),
            source: path.to_path_buf(),
        };
        check(&entry)?;
        Ok(entry)
    }

    fn set(&mut self, label: &str, value: f64) -> bool {
        let label = label.trim();
        let single = match label {
            "Density" => Some(&mut self.density),
            "Young's Modulus" | "YoungsModulus" => Some(&mut self.young),
            "Poisson's Ratio" | "PoissonRatio" => Some(&mut self.poisson),
            "Tensile Yield Strength" | "YieldStrength" => Some(&mut self.strength),
            "Tensile Ultimate Strength" | "UltimateTensileStrength" => Some(&mut self.ultimate),
            "Thermal Conductivity" | "ThermalConductivity" => Some(&mut self.conductivity),
            "Coefficient of Thermal Expansion" | "ThermalExpansionCoefficient" => {
                Some(&mut self.expansion)
            }
            _ => None,
        };
        if let Some(slot) = single {
            *slot = Some(value);
            return true;
        }
        let prefixes: [(&str, usize); 8] = [
            ("Young's Modulus ", 0),
            ("YoungsModulus", 0),
            ("Shear Modulus ", 1),
            ("ShearModulus", 1),
            ("Thermal Conductivity ", 2),
            ("ThermalConductivity", 2),
            ("Coefficient of Thermal Expansion ", 3),
            ("ThermalExpansionCoefficient", 3),
        ];
        for (prefix, kind) in prefixes {
            let Some(rest) = label.strip_prefix(prefix) else {
                continue;
            };
            let index = if kind == 1 {
                shear_index(rest)
            } else {
                direction(rest)
            };
            let Some(a) = index else { continue };
            let slot = match kind {
                0 => &mut self.axes_young,
                1 => &mut self.axes_shear,
                2 => &mut self.axes_conductivity,
                _ => &mut self.axes_expansion,
            };
            slot[a] = Some(value);
            return true;
        }
        for prefix in ["Poisson's Ratio ", "PoissonRatio"] {
            if let Some(a) = label.strip_prefix(prefix).and_then(shear_index) {
                self.axes_poisson[a] = Some(value);
                return true;
            }
        }
        false
    }

    fn set_limit(&mut self, label: &str, value: f64) -> bool {
        let label = label.trim();
        let slot = if let Some(a) = label.strip_prefix("Tensile ").and_then(direction) {
            &mut self.tension[a]
        } else if let Some(a) = label.strip_prefix("Compressive ").and_then(direction) {
            &mut self.compression[a]
        } else if let Some(a) = label.strip_prefix("Shear ").and_then(shear_index) {
            &mut self.shear_strength[a]
        } else {
            return false;
        };
        *slot = Some(value);
        true
    }
}

fn direction(rest: &str) -> Option<usize> {
    match rest.trim() {
        "X direction" | "X" => Some(0),
        "Y direction" | "Y" => Some(1),
        "Z direction" | "Z" => Some(2),
        _ => None,
    }
}

fn child<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    node.children().find(|c| c.has_tag_name(tag))
}

fn shear_index(pair: &str) -> Option<usize> {
    match pair.trim() {
        "XY" => Some(0),
        "XZ" => Some(1),
        "YZ" => Some(2),
        _ => None,
    }
}

fn from_matml(text: &str, path: &Path) -> Result<Vec<Entry>> {
    let document = roxmltree::Document::parse(text).map_err(|e| anyhow!("{e}"))?;
    let mut details: HashMap<String, (String, Result<f64, String>)> = HashMap::new();
    for node in document
        .descendants()
        .filter(|n| n.has_tag_name("ParameterDetails") || n.has_tag_name("PropertyDetails"))
    {
        let Some(id) = node.attribute("id") else {
            continue;
        };
        let name = child(node, "Name")
            .and_then(|n| n.text())
            .unwrap_or("")
            .trim();
        let mut factor: Result<f64, String> = Ok(1.0);
        if let Some(units) = child(node, "Units") {
            for unit in units.children().filter(|c| c.has_tag_name("Unit")) {
                let power: f64 = unit
                    .attribute("power")
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(1.0);
                let unit_name = child(unit, "Name").and_then(|n| n.text()).unwrap_or("");
                factor = match (factor, unit_factor(unit_name)) {
                    (Ok(f), Ok(u)) => Ok(f * u.powf(power)),
                    (Err(e), _) => Err(e),
                    (_, Err(_)) => Err(unit_name.to_string()),
                };
            }
        }
        details.insert(id.to_string(), (name.to_string(), factor));
    }
    let mut out = Vec::new();
    for material in document
        .descendants()
        .filter(|n| n.has_tag_name("Material"))
    {
        let Some(bulk) = child(material, "BulkDetails") else {
            continue;
        };
        let name = child(bulk, "Name")
            .and_then(|n| n.text())
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            continue;
        }
        let mut si = Si::default();
        for property in bulk.children().filter(|c| c.has_tag_name("PropertyData")) {
            let Some((property_name, _)) = property
                .attribute("property")
                .and_then(|id| details.get(id))
            else {
                continue;
            };
            let values: Vec<(String, Result<f64, String>, Vec<f64>)> = property
                .children()
                .filter(|c| c.has_tag_name("ParameterValue"))
                .filter_map(|value| {
                    let (label, factor) = details.get(value.attribute("parameter")?)?;
                    let data = child(value, "Data")?.text()?;
                    let numbers: Vec<f64> = data
                        .split(',')
                        .filter_map(|v| v.trim().parse::<f64>().ok())
                        .collect();
                    (!numbers.is_empty()).then(|| (label.clone(), factor.clone(), numbers))
                })
                .collect();
            let room = values
                .iter()
                .find(|(label, ..)| label == "Temperature")
                .map(|(_, _, temperatures)| {
                    temperatures
                        .iter()
                        .enumerate()
                        .min_by(|a, b| (a.1 - 22.0).abs().total_cmp(&(b.1 - 22.0).abs()))
                        .map_or(0, |(k, _)| k)
                })
                .unwrap_or(0);
            for (label, factor, numbers) in &values {
                if label == "Temperature" {
                    continue;
                }
                let raw = numbers
                    .get(room)
                    .or(numbers.first())
                    .copied()
                    .unwrap_or(0.0);
                let value = raw * factor.as_ref().copied().unwrap_or(f64::NAN);
                let used = if property_name == "Stress Limits" {
                    si.set_limit(label, value)
                } else {
                    si.set(label, value)
                };
                if let (true, Err(unit)) = (used, factor) {
                    bail!("`{name}` gives {label} in `{unit}`, a unit gcad does not know");
                }
            }
        }
        out.push(si.entry(name, path)?);
    }
    if out.is_empty() {
        bail!("no materials found in this MatML file");
    }
    Ok(out)
}

fn from_fcmat(text: &str, path: &Path) -> Result<Vec<Entry>> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut si = Si::default();
    let mut name = None;
    let take = |key: &str, value: &str, si: &mut Si| -> Result<()> {
        if value.trim().is_empty() {
            return Ok(());
        }
        let known = [
            "Density",
            "YoungsModulus",
            "PoissonRatio",
            "YieldStrength",
            "UltimateTensileStrength",
            "ThermalConductivity",
            "ThermalExpansionCoefficient",
        ];
        let directional = [
            "YoungsModulus",
            "PoissonRatio",
            "ShearModulus",
            "ThermalConductivity",
            "ThermalExpansionCoefficient",
        ];
        if known.contains(&key)
            || directional
                .iter()
                .any(|p| key.strip_prefix(p).is_some_and(|r| !r.is_empty()))
        {
            si.set(key, quantity(value).with_context(|| format!("`{key}`"))?);
        }
        Ok(())
    };
    let yaml = text.lines().any(|l| l.trim_start().starts_with("Models:"));
    if yaml {
        let docs = yaml_rust2::YamlLoader::load_from_str(text).map_err(|e| anyhow!("{e}"))?;
        let doc = docs.first().ok_or_else(|| anyhow!("empty file"))?;
        name = doc["General"]["Name"].as_str().map(str::to_string);
        if let Some(models) = doc["Models"].as_hash() {
            for (_, model) in models {
                let Some(fields) = model.as_hash() else {
                    continue;
                };
                for (key, value) in fields {
                    let (Some(key), Some(value)) = (key.as_str(), yaml_text(value)) else {
                        continue;
                    };
                    take(key, &value, &mut si)?;
                }
            }
        }
    } else {
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with(';') || line.starts_with('[') || line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            if key == "Name" {
                name = Some(value.to_string());
                continue;
            }
            take(key, value, &mut si)?;
        }
    }
    Ok(vec![si.entry(name.as_deref().unwrap_or(&stem), path)?])
}

fn yaml_text(value: &yaml_rust2::Yaml) -> Option<String> {
    use yaml_rust2::Yaml;
    match value {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Real(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        _ => None,
    }
}

pub fn table(target: Option<&Path>) -> Result<String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let here = target.unwrap_or(&cwd);
    let library_file = here.is_file()
        && matches!(
            here.extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .as_deref(),
            Some("toml" | "xml" | "matml" | "fcmat")
        );
    let mut rows: Vec<Entry> = if library_file {
        load(here)?
    } else {
        let dir = if here.is_dir() {
            here.to_path_buf()
        } else {
            here.parent().map(Path::to_path_buf).unwrap_or(cwd.clone())
        };
        let mut found: Vec<Entry> = discovered(Some(&dir))?.into_values().collect();
        for stock in super::bodies::MATERIALS {
            if !found.iter().any(|e| e.name == stock.name) {
                found.push(Entry {
                    name: stock.name.to_string(),
                    density: Some(stock.density),
                    mechanics: stock.mechanics(),
                    axes: None,
                    source: PathBuf::new(),
                });
            }
        }
        found
    };
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    let value = |v: Option<f64>| v.map_or("-".to_string(), crate::plot::label);
    let mut lines = vec![format!(
        "{:<24} {:>8} {:>8} {:>6} {:>8} {:>8} {:>8}  {}",
        "material", "g/cm³", "E GPa", "nu", "yield", "k", "alpha", "from"
    )];
    for e in &rows {
        let m = &e.mechanics;
        let from = if e.source.as_os_str().is_empty() {
            "built in".to_string()
        } else {
            e.source
                .strip_prefix(&cwd)
                .unwrap_or(&e.source)
                .display()
                .to_string()
        };
        lines.push(format!(
            "{:<24} {:>8} {:>8} {:>6} {:>8} {:>8} {:>8}  {}{}",
            e.name,
            value(e.density),
            value(m.young),
            value(m.poisson),
            value(m.strength),
            value(m.conductivity),
            value(m.expansion),
            from,
            e.axes
                .map(|a| format!(", along three axes: {}", a.summary()))
                .unwrap_or_default()
        ));
    }
    Ok(lines.join("\n"))
}
