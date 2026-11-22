use crate::cloud::{draw_views, legend};
use crate::model::Model;
use anyhow::{Result, bail};
use gcad_cloud::colour::deviation_limit;
use gcad_parse::label;
use gcad_plot::{Anchor, Canvas};

const PAGE_W: u32 = 1240;
const PAGE_H: u32 = 1754;
const VIEW: u32 = 500;
const INK: [u8; 3] = [40, 40, 40];
const PASS: [u8; 3] = [30, 140, 60];
const FAIL: [u8; 3] = [200, 40, 40];

pub fn csv(model: &Model, title: &str) -> String {
    let mut out = String::from("section,name,key,value\n");
    out.push_str(&format!("report,title,file,{title}\n"));
    for i in &model.inspections {
        let s = &i.stats;
        let rows = [
            ("points", s.count as f64),
            ("mean", s.mean),
            ("std", s.std),
            ("min", s.min),
            ("max", s.max),
            ("rms", s.rms),
            ("tolerance", i.tolerance),
            ("within_percent", s.within),
        ];
        for (key, value) in rows {
            out.push_str(&format!("deviation,{},{key},{}\n", i.into, label(value)));
        }
        out.push_str(&format!("deviation,{},target,{}\n", i.into, i.target));
        out.push_str(&format!("deviation,{},surface_triangles,{}\n", i.into, i.triangles));
    }
    for m in &model.measures {
        out.push_str(&format!("measure,{},what,{}\n", m.name, m.what));
        out.push_str(&format!("measure,{},distance,{}\n", m.name, label(m.distance)));
        if let Some(s) = m.signed {
            out.push_str(&format!("measure,{},signed,{}\n", m.name, label(s)));
        }
    }
    for c in &model.checks {
        let text = c.text.replace(',', ";");
        let detail = c.detail.replace(',', ";");
        out.push_str(&format!(
            "check,line {},{text},{} ({detail})\n",
            c.line,
            if c.passed { "pass" } else { "FAIL" }
        ));
    }
    let failed = model.checks.iter().filter(|c| !c.passed).count();
    out.push_str(&format!(
        "summary,checks,passed,{}\nsummary,checks,failed,{failed}\n",
        model.checks.len() - failed
    ));
    out
}

pub fn failed_checks(model: &Model) -> usize {
    model.checks.iter().filter(|c| !c.passed).count()
}

pub fn pdf(model: &Model, title: &str) -> Result<Vec<u8>> {
    let (width, height, pixels) = page(model, title)?;
    let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, body: Vec<u8>| {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", offsets.len()).bytes());
        out.extend(body);
        out.extend(b"\nendobj\n");
    };
    object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    object(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    object(
        &mut out,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>".to_vec(),
    );
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    std::io::Write::write_all(&mut encoder, &pixels)?;
    let packed = encoder.finish()?;
    let mut image = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
        packed.len()
    )
    .into_bytes();
    image.extend(&packed);
    image.extend(b"\nendstream");
    object(&mut out, image);
    let content = b"q 595 0 0 842 0 0 cm /Im0 Do Q";
    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend(content);
    stream.extend(b"\nendstream");
    object(&mut out, stream);
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).bytes());
    for offset in &offsets {
        out.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .bytes(),
    );
    Ok(out)
}

pub fn page(model: &Model, title: &str) -> Result<(u32, u32, Vec<u8>)> {
    let clouds = model.named_clouds();
    let Some((name, cloud)) = clouds
        .iter()
        .find(|(_, c)| c.deviation.is_some())
        .or_else(|| clouds.first())
    else {
        bail!("the model has no point cloud to report on");
    };
    let mut canvas = Canvas::new(PAGE_W, PAGE_H);
    canvas.text((60.0, 80.0), 40.0, "Inspection report", Anchor::Left, INK);
    canvas.text((60.0, 120.0), 22.0, &format!("{title}, scan {name}, {} points", cloud.len()), Anchor::Left, INK);
    let views = draw_views(cloud, VIEW);
    canvas.paste(&views, (60, 160));
    if cloud.deviation.is_some() {
        let tolerance = model.inspections.first().map_or(0.0, |i| i.tolerance);
        legend(
            &mut canvas,
            (60 + VIEW * 2 + 30) as f64,
            210.0,
            (160 + VIEW * 2 - 30) as f64,
            deviation_limit(cloud),
            tolerance,
        );
    }
    let mut y = (160 + VIEW * 2 + 60) as f64;
    canvas.text((60.0, y), 28.0, "Deviation statistics", Anchor::Left, INK);
    y += 36.0;
    let columns = [60.0, 260.0, 380.0, 500.0, 620.0, 740.0, 860.0, 1000.0];
    for (x, head) in columns.iter().zip(["scan to", "mean", "std", "min", "max", "rms", "tol", "within %"]) {
        canvas.text((*x, y), 20.0, head, Anchor::Left, INK);
    }
    y += 30.0;
    for i in &model.inspections {
        let s = &i.stats;
        let cells = [
            i.target.clone(),
            label(s.mean),
            label(s.std),
            label(s.min),
            label(s.max),
            label(s.rms),
            label(i.tolerance),
            label(s.within),
        ];
        for (x, cell) in columns.iter().zip(&cells) {
            canvas.text((*x, y), 20.0, cell, Anchor::Left, INK);
        }
        y += 28.0;
    }
    for m in &model.measures {
        canvas.text((60.0, y), 20.0, &format!("{}: {} = {}", m.name, m.what, label(m.distance)), Anchor::Left, INK);
        y += 28.0;
    }
    y += 24.0;
    canvas.text((60.0, y), 28.0, "Checks", Anchor::Left, INK);
    y += 36.0;
    if model.checks.is_empty() {
        canvas.text((60.0, y), 20.0, "no check lines in the file", Anchor::Left, INK);
    }
    for c in &model.checks {
        let (word, colour) = if c.passed { ("PASS", PASS) } else { ("FAIL", FAIL) };
        canvas.text((60.0, y), 20.0, word, Anchor::Left, colour);
        canvas.text((150.0, y), 20.0, &format!("{}   ({})", c.text, c.detail), Anchor::Left, INK);
        y += 28.0;
        if y > PAGE_H as f64 - 40.0 {
            break;
        }
    }
    let image = canvas.image.clone();
    Ok((image.width(), image.height(), image.into_raw()))
}
