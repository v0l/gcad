use crate::Model;
use crate::parse::Line;
use anyhow::{Context, Result, anyhow, bail};

#[derive(Clone, Debug)]
pub struct Underlay {
    pub file: std::path::PathBuf,
    pub width: f64,
    pub height: f64,
}

fn ratio(text: &str) -> Result<f64> {
    let (a, b) = text
        .split_once(':')
        .ok_or_else(|| anyhow!("scale is a ratio like `1:100`, not `{text}`"))?;
    let (a, b): (f64, f64) = (a.parse()?, b.parse()?);
    if a <= 0.0 || b <= 0.0 {
        bail!("scale `{text}` must be positive");
    }
    Ok(b / a)
}

impl Model {
    pub(crate) fn op_underlay(&mut self, line: &Line) -> Result<String> {
        let [file] = line.positional.as_slice() else {
            bail!("write `underlay plan.png scale=1:100 dpi=100`");
        };
        let mut scale = 1.0;
        let mut dpi = 96.0;
        for (key, text) in &line.named {
            match key.as_str() {
                "scale" => scale = ratio(text)?,
                "dpi" => {
                    dpi = crate::parse::eval(text, &self.scope)
                        .with_context(|| format!("`dpi={text}`"))?
                }
                other => bail!("`underlay` takes scale= and dpi=, not `{other}=`"),
            }
        }
        if dpi <= 0.0 {
            bail!("dpi must be above zero");
        }
        let path = self.relative(file);
        let bytes = crate::host::read(&path)
            .map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
        let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()?
            .into_dimensions()
            .map_err(|e| anyhow!("{}: {e}", path.display()))?;
        let mm = 25.4 * scale / dpi;
        let (width, height) = (w as f64 * mm, h as f64 * mm);
        self.arch.underlays.push(Underlay {
            file: path,
            width,
            height,
        });
        Ok(format!(
            "underlay {file}: {} x {} mm at {}",
            crate::parse::label(width),
            crate::parse::label(height),
            line.named
                .iter()
                .find(|(k, _)| k == "scale")
                .map_or("1:1", |(_, v)| v.as_str())
        ))
    }
}
