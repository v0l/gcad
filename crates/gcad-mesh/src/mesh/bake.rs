use super::grid::Surface;
use super::sdf::collect_indexed;
use super::uv::{Expanded, expand, unwrap};
use super::{Mesh, P3, add, cross, dot, length, normalized, scale, sub};

pub const FLAT: [u8; 3] = [128, 128, 255];

#[derive(Clone, Debug)]
pub struct Baked {
    pub size: usize,
    pub pixels: Vec<[u8; 3]>,
    pub covered: usize,
    pub expanded: Expanded,
}

fn barycentric(p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 3]> {
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if d.abs() < 1.0e-18 {
        return None;
    }
    let u = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / d;
    let v = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / d;
    Some([u, v, 1.0 - u - v])
}

fn weights_in(p: P3, a: P3, b: P3, c: P3) -> [f64; 3] {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d00, d01, d11) = (dot(ab, ab), dot(ab, ac), dot(ac, ac));
    let (d20, d21) = (dot(ap, ab), dot(ap, ac));
    let denom = d00 * d11 - d01 * d01;
    if denom.abs() < 1.0e-300 {
        return [1.0, 0.0, 0.0];
    }
    let v = (d11 * d20 - d01 * d21) / denom;
    let w = (d00 * d21 - d01 * d20) / denom;
    [1.0 - v - w, v, w]
}

fn encode(v: f64) -> u8 {
    ((v * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8
}

pub fn bake_normals(low: &Mesh, high: &Mesh, size: usize) -> Baked {
    let layout = unwrap(low, size);
    let expanded = expand(low, &layout);
    let high_normals = high.vertex_normals();
    let surface = Surface::new(high);
    let s = size as f64;
    let rows = collect_indexed(low.triangles.len(), |f| {
        let tri = expanded.triangles[f];
        let uv = tri.map(|i| expanded.uvs[i as usize]);
        let lo = [0, 1].map(|k| uv.iter().map(|q| q[k]).fold(f64::INFINITY, f64::min));
        let hi = [0, 1].map(|k| uv.iter().map(|q| q[k]).fold(f64::NEG_INFINITY, f64::max));
        let x0 = ((lo[0] * s - 0.5).floor().max(0.0)) as usize;
        let y0 = ((lo[1] * s - 0.5).floor().max(0.0)) as usize;
        let x1 = ((hi[0] * s + 0.5).ceil() as usize).min(size - 1);
        let y1 = ((hi[1] * s + 0.5).ceil() as usize).min(size - 1);
        let positions = tri.map(|i| expanded.positions[i as usize]);
        let normals = tri.map(|i| expanded.normals[i as usize]);
        let tangents = tri.map(|i| expanded.tangents[i as usize]);
        let mut out = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let at = [(x as f64 + 0.5) / s, (y as f64 + 0.5) / s];
                let Some(w) = barycentric(at, uv[0], uv[1], uv[2]) else {
                    continue;
                };
                if w.iter().any(|v| *v < -1.0e-9) {
                    continue;
                }
                let mix = |values: [P3; 3]| {
                    add(
                        add(scale(values[0], w[0]), scale(values[1], w[1])),
                        scale(values[2], w[2]),
                    )
                };
                let p = mix(positions);
                let n = normalized(mix(normals));
                let t3 = mix(tangents.map(|t| [t[0], t[1], t[2]]));
                let t = normalized(sub(t3, scale(n, dot(n, t3))));
                let sign = tangents[0][3];
                let b = scale(cross(n, t), sign);
                let Some(hit) = surface.closest(p) else {
                    continue;
                };
                let corners = high.triangles[hit.triangle as usize];
                let [pa, pb, pc] = corners.map(|i| high.positions[i as usize]);
                let bw = weights_in(hit.point, pa, pb, pc);
                let target = normalized(add(
                    add(
                        scale(high_normals[corners[0] as usize], bw[0]),
                        scale(high_normals[corners[1] as usize], bw[1]),
                    ),
                    scale(high_normals[corners[2] as usize], bw[2]),
                ));
                let local = [dot(target, t), dot(target, b), dot(target, n)];
                out.push((
                    y * size + x,
                    [encode(local[0]), encode(local[1]), encode(local[2])],
                ));
            }
        }
        out
    });
    let mut pixels: Vec<Option<[u8; 3]>> = vec![None; size * size];
    for (i, rgb) in rows.into_iter().flatten() {
        pixels[i] = Some(rgb);
    }
    let covered = pixels.iter().flatten().count();
    for _ in 0..3 {
        let before = pixels.clone();
        for y in 0..size {
            for x in 0..size {
                if before[y * size + x].is_some() {
                    continue;
                }
                'near: for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= size as i64 || ny >= size as i64 {
                        continue;
                    }
                    if let Some(rgb) = before[ny as usize * size + nx as usize] {
                        pixels[y * size + x] = Some(rgb);
                        break 'near;
                    }
                }
            }
        }
    }
    Baked {
        size,
        pixels: pixels.into_iter().map(|p| p.unwrap_or(FLAT)).collect(),
        covered,
        expanded,
    }
}

impl Baked {
    pub fn decode(&self, x: usize, y: usize) -> P3 {
        let [r, g, b] = self.pixels[y * self.size + x];
        let f = |v: u8| v as f64 / 255.0 * 2.0 - 1.0;
        normalized([f(r), f(g), f(b)])
    }

    pub fn tilt_at(&self, x: usize, y: usize) -> f64 {
        let n = self.decode(x, y);
        length([n[0], n[1], 0.0]).atan2(n[2])
    }

    pub fn png(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(self.size * (self.size * 3 + 1));
        for row in self.pixels.chunks(self.size) {
            raw.push(0);
            raw.extend(row.iter().flatten());
        }
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend((self.size as u32).to_be_bytes());
        header.extend((self.size as u32).to_be_bytes());
        header.extend([8, 2, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &header);
        chunk(
            &mut out,
            b"IDAT",
            &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
        );
        chunk(&mut out, b"IEND", &[]);
        out
    }
}

pub(crate) fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(tilt: f64) -> Mesh {
        let z = |x: f64| x * tilt.tan();
        Mesh {
            positions: vec![
                [0.0, 0.0, z(0.0)],
                [4.0, 0.0, z(4.0)],
                [4.0, 4.0, z(4.0)],
                [0.0, 4.0, z(0.0)],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            labels: vec![0; 4],
            parts: vec!["p".into()],
            ..Default::default()
        }
    }

    #[test]
    fn crc_of_iend_matches_the_png_spec() {
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
    }

    #[test]
    fn a_surface_baked_onto_itself_is_flat() {
        let plane = square(0.0);
        let baked = bake_normals(&plane, &plane, 64);
        assert!(baked.covered > 64 * 64 / 2);
        assert!(
            baked
                .pixels
                .iter()
                .all(|p| p[2] >= 254 && (p[0] as i32 - 128).abs() <= 1)
        );
    }

    #[test]
    fn a_ramp_baked_onto_a_plane_tilts_thirty_degrees() {
        let low = square(0.0);
        let high = square(30f64.to_radians());
        let baked = bake_normals(&low, &high, 64);
        let mut seen = 0;
        for y in 0..64 {
            for x in 0..64 {
                if baked.pixels[y * 64 + x] == FLAT {
                    continue;
                }
                seen += 1;
                let n = baked.decode(x, y);
                assert!((n[2] - 30f64.to_radians().cos()).abs() < 0.01, "{n:?}");
                assert!((baked.tilt_at(x, y) - 30f64.to_radians()).abs() < 0.015);
            }
        }
        assert!(seen > 64 * 64 / 2, "{seen}");
        assert_eq!(baked.pixels[32 * 64 + 32][2], 238);
    }

    #[test]
    fn png_has_the_signature_and_the_size() {
        let baked = bake_normals(&square(0.0), &square(0.0), 16);
        let png = baked.png();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 16);
        assert!(png.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]));
    }
}
