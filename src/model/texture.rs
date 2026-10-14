use anyhow::{Context, Result, bail};
use std::path::Path;

pub const PATTERNS: &[&str] = &[
    "none",
    "wood",
    "osb",
    "concrete",
    "stone",
    "brick",
    "sand",
    "fabric",
    "foam",
    "rubber",
    "brushed",
    "carbon-fibre",
    "polished",
    "cast",
    "plaster",
    "granulate",
    "marble",
    "asphalt",
];

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Map {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Map {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = crate::host::read(path)
            .with_context(|| format!("reading texture {}", path.display()))?;
        let image = image::load_from_memory(&bytes)
            .with_context(|| format!("decoding texture {}", path.display()))?
            .into_rgba8();
        Ok(Self {
            width: image.width() as usize,
            height: image.height() as usize,
            rgba: image.into_raw(),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Texture {
    pub pattern: Option<String>,
    pub tile: Option<f64>,
    pub turn: Option<f64>,
    pub colour: Option<Map>,
    pub normal: Option<Map>,
    pub roughness: Option<Map>,
    pub metallic: Option<Map>,
}

impl Texture {
    pub fn shown(&self) -> bool {
        self.pattern.as_deref().is_some_and(|p| p != "none")
            || self.colour.is_some()
            || self.normal.is_some()
            || self.roughness.is_some()
            || self.metallic.is_some()
    }

    pub fn scale(&self) -> f64 {
        self.tile.unwrap_or(100.0)
    }

    pub fn rotation(&self) -> f64 {
        self.turn.unwrap_or(0.0)
    }

    pub fn over(self, base: Texture) -> Texture {
        Texture {
            pattern: self.pattern.or(base.pattern),
            tile: self.tile.or(base.tile),
            turn: self.turn.or(base.turn),
            colour: self.colour.or(base.colour),
            normal: self.normal.or(base.normal),
            roughness: self.roughness.or(base.roughness),
            metallic: self.metallic.or(base.metallic),
        }
    }

    pub fn check(&self) -> Result<()> {
        if let Some(pattern) = &self.pattern
            && !PATTERNS.contains(&pattern.as_str())
        {
            bail!(
                "unknown texture `{pattern}`; textures are {}",
                PATTERNS.join(", ")
            );
        }
        if !self.scale().is_finite() || self.scale() <= 0.0 {
            bail!("texture_scale must be a finite positive size in mm");
        }
        if !self.rotation().is_finite() {
            bail!("texture_rotation must be finite");
        }
        Ok(())
    }
}
