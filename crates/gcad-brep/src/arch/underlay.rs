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

const CELLS: u32 = 96;

impl Model {
    pub fn underlay_shading(
        &self,
    ) -> Vec<(
        String,
        std::sync::Arc<gcad_mesh::mesh::Mesh>,
        Vec<[f64; 3]>,
        Vec<gcad_mesh::interp::Shine>,
    )> {
        self.arch
            .underlays
            .iter()
            .filter_map(|underlay| {
                let bytes = crate::host::read(&underlay.file).ok()?;
                let picture = image::load_from_memory(&bytes).ok()?.to_rgba8();
                let (columns, rows) = (CELLS.min(picture.width()), CELLS.min(picture.height()));
                let small = image::imageops::resize(
                    &picture,
                    columns,
                    rows,
                    image::imageops::FilterType::Triangle,
                );
                let mut mesh = gcad_mesh::mesh::Mesh::default();
                let mut tints = Vec::new();
                for row in 0..rows {
                    for column in 0..columns {
                        let x0 = underlay.width * column as f64 / columns as f64;
                        let x1 = underlay.width * (column + 1) as f64 / columns as f64;
                        let y0 = underlay.height * (rows - row - 1) as f64 / rows as f64;
                        let y1 = underlay.height * (rows - row) as f64 / rows as f64;
                        let base = mesh.positions.len() as u32;
                        mesh.positions.extend([
                            [x0, y0, 0.0],
                            [x1, y0, 0.0],
                            [x1, y1, 0.0],
                            [x0, y1, 0.0],
                        ]);
                        mesh.triangles
                            .extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
                        let pixel = small.get_pixel(column, row).0;
                        let tint = [pixel[0] as f64 / 255.0, pixel[1] as f64 / 255.0, pixel[2] as f64 / 255.0];
                        tints.extend([tint; 4]);
                    }
                }
                let shine = vec![Default::default(); mesh.positions.len()];
                Some((
                    underlay.file.display().to_string(),
                    std::sync::Arc::new(mesh),
                    tints,
                    shine,
                ))
            })
            .collect()
    }

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
