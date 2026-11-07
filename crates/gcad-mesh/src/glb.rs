use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub const ARRAY_BUFFER: u32 = 34962;
pub const ELEMENT_ARRAY_BUFFER: u32 = 34963;
pub const FLOAT: u32 = 5126;
pub const UNSIGNED_SHORT: u32 = 5123;
pub const UNSIGNED_INT: u32 = 5125;

#[derive(Default)]
pub struct Buffer {
    pub bin: Vec<u8>,
    pub views: Vec<Value>,
    pub accessors: Vec<Value>,
}

impl Buffer {
    pub fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let mut view =
            json!({"buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len()});
        if let Some(t) = target {
            view["target"] = json!(t);
        }
        self.bin.extend_from_slice(bytes);
        self.views.push(view);
        self.views.len() - 1
    }

    pub fn accessor(
        &mut self,
        bytes: &[u8],
        target: Option<u32>,
        component: u32,
        count: usize,
        kind: &str,
        bounds: Option<(Vec<f64>, Vec<f64>)>,
    ) -> usize {
        let view = self.view(bytes, target);
        let mut a =
            json!({"bufferView": view, "componentType": component, "count": count, "type": kind});
        if let Some((lo, hi)) = bounds {
            a["min"] = json!(lo);
            a["max"] = json!(hi);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    pub fn floats(
        &mut self,
        values: &[f32],
        width: usize,
        kind: &str,
        target: Option<u32>,
        bounds: bool,
    ) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let b = bounds.then(|| {
            let mut lo = vec![f64::INFINITY; width];
            let mut hi = vec![f64::NEG_INFINITY; width];
            for chunk in values.chunks(width) {
                for (k, v) in chunk.iter().enumerate() {
                    lo[k] = lo[k].min(*v as f64);
                    hi[k] = hi[k].max(*v as f64);
                }
            }
            (lo, hi)
        });
        self.accessor(&bytes, target, FLOAT, values.len() / width, kind, b)
    }

    pub fn checked_floats<const N: usize>(
        &mut self,
        values: &[[f64; N]],
        kind: &str,
        target: Option<u32>,
        bounds: bool,
    ) -> Result<usize> {
        let mut flat = Vec::with_capacity(values.len() * N);
        for value in values {
            for v in value {
                let v = *v as f32;
                ensure!(v.is_finite(), "attribute exceeds float32 range");
                flat.push(v);
            }
        }
        Ok(self.floats(&flat, N, kind, target, bounds))
    }

    pub fn indices(&mut self, triangles: &[u32]) -> usize {
        let bytes: Vec<u8> = triangles.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.accessor(
            &bytes,
            Some(ELEMENT_ARRAY_BUFFER),
            UNSIGNED_INT,
            triangles.len(),
            "SCALAR",
            None,
        )
    }

    pub fn finish(mut self, mut root: Value) -> Result<Vec<u8>> {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        root["buffers"] = json!([{"byteLength": self.bin.len()}]);
        root["bufferViews"] = json!(self.views);
        root["accessors"] = json!(self.accessors);
        let mut json_bytes = serde_json::to_vec(&root)?;
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(b' ');
        }
        let total = u32::try_from(12 + 8 + json_bytes.len() + 8 + self.bin.len())?;
        let mut out = Vec::with_capacity(total as usize);
        out.extend(b"glTF");
        out.extend(2u32.to_le_bytes());
        out.extend(total.to_le_bytes());
        out.extend(u32::try_from(json_bytes.len())?.to_le_bytes());
        out.extend(b"JSON");
        out.extend(&json_bytes);
        out.extend((self.bin.len() as u32).to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(&self.bin);
        Ok(out)
    }
}
