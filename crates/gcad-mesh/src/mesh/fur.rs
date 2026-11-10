use super::{Mesh, P3, add, cross, dot, length, normalized, scale, sub};
use anyhow::{Result, bail};

#[derive(Clone, Debug, PartialEq)]
pub struct FurSpec {
    pub part: Option<String>,
    pub length: f64,
    pub width: f64,
    pub count: usize,
    pub clump: f64,
    pub droop: f64,
    pub seed: u32,
    pub colour: Option<[f64; 3]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cards {
    pub corners: Vec<[P3; 4]>,
    pub surface: Vec<P3>,
    pub anchor: Vec<u32>,
}

fn unit(seed: u32, a: u64, b: u64) -> f64 {
    let mut h = (a.wrapping_add(1)).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (b.wrapping_add(1)).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (seed as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 32;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

fn perpendicular(v: P3) -> (P3, P3) {
    let helper = if v[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let a = normalized(cross(v, helper));
    (a, normalized(cross(v, a)))
}

fn lerp(a: P3, b: P3, t: f64) -> P3 {
    add(scale(a, 1.0 - t), scale(b, t))
}

impl FurSpec {
    pub fn triangles(&self) -> usize {
        self.count * 2
    }

    pub fn area(&self, mesh: &Mesh) -> Result<f64> {
        Ok(self.surface(mesh)?.2)
    }

    fn surface(&self, mesh: &Mesh) -> Result<(Vec<usize>, Vec<f64>, f64)> {
        let part = match &self.part {
            Some(name) => match mesh.parts.iter().position(|p| p == name) {
                Some(i) => Some(i as u16),
                None => bail!(
                    "the mesh has no part `{name}`; its parts are {}",
                    mesh.parts.join(", ")
                ),
            },
            None => None,
        };
        let mut chosen = Vec::new();
        let mut running = Vec::new();
        let mut total = 0.0;
        for (f, t) in mesh.triangles.iter().enumerate() {
            if part.is_some_and(|p| mesh.labels[t[0] as usize] != p) {
                continue;
            }
            let [a, b, c] = mesh.corners(*t);
            total += length(cross(sub(b, a), sub(c, a))) / 2.0;
            chosen.push(f);
            running.push(total);
        }
        if total <= 0.0 {
            bail!("there is no surface to grow fur on");
        }
        Ok((chosen, running, total))
    }

    pub fn grow(&self, mesh: &Mesh) -> Result<Cards> {
        let (chosen, running, total) = self.surface(mesh)?;
        let normals = mesh.vertex_normals();
        let seed = self.seed;
        let mut cards = Cards::default();
        for k in 0..self.count {
            let k64 = k as u64;
            let u = (k as f64 + unit(seed, k64, 1)) / self.count as f64 * total;
            let at = running.partition_point(|&r| r < u).min(chosen.len() - 1);
            let t = mesh.triangles[chosen[at]];
            let [a, b, c] = mesh.corners(t);
            let (r1, r2) = (unit(seed, k64, 2).sqrt(), unit(seed, k64, 3));
            let w = [1.0 - r1, r1 * (1.0 - r2), r1 * r2];
            let mix =
                |p: [P3; 3]| add(add(scale(p[0], w[0]), scale(p[1], w[1])), scale(p[2], w[2]));
            let point = mix([a, b, c]);
            let normal = normalized(mix(t.map(|i| normals[i as usize])));
            let (t1, t2) = perpendicular(normal);
            let spin = unit(seed, k64, 4) * std::f64::consts::TAU;
            let lean = 0.1 + 0.5 * unit(seed, k64, 5);
            let own = add(
                normal,
                scale(add(scale(t1, spin.cos()), scale(t2, spin.sin())), lean),
            );
            let cell = point.map(|c| (c / self.length.max(1.0e-9)).floor() as i64);
            let key = (cell[0].wrapping_mul(73_856_093)
                ^ cell[1].wrapping_mul(19_349_663)
                ^ cell[2].wrapping_mul(83_492_791)) as u64;
            let (c1, c2) = (
                unit(seed, key, 6) * 2.0 - 1.0,
                unit(seed, key, 7) * 2.0 - 1.0,
            );
            let clumped = add(normal, add(scale(t1, c1 * 0.5), scale(t2, c2 * 0.5)));
            let down = [0.0, 0.0, -self.droop];
            let direction = normalized(add(
                lerp(normalized(own), normalized(clumped), self.clump),
                down,
            ));
            let len = self.length * (0.75 + 0.25 * unit(seed, k64, 8));
            let (p1, p2) = perpendicular(direction);
            let roll = unit(seed, k64, 9) * std::f64::consts::PI;
            let side = add(scale(p1, roll.cos()), scale(p2, roll.sin()));
            let half = scale(side, self.width / 2.0);
            let root = sub(point, scale(normal, 0.05 * len));
            let tip = add(point, scale(direction, len));
            cards.corners.push([
                sub(root, half),
                add(root, half),
                add(tip, half),
                sub(tip, half),
            ]);
            cards.surface.push(normal);
            let nearest = (0..3).max_by(|&i, &j| w[i].total_cmp(&w[j])).unwrap_or(0);
            cards.anchor.push(t[nearest]);
        }
        Ok(cards)
    }
}

impl FurSpec {
    pub fn tint(&self, mesh: &Mesh, tints: &[[f64; 3]]) -> [f64; 3] {
        if let Some(c) = self.colour {
            return c;
        }
        let part = self
            .part
            .as_ref()
            .and_then(|name| mesh.parts.iter().position(|p| p == name));
        part.and_then(|i| tints.get(i))
            .or_else(|| tints.first())
            .copied()
            .unwrap_or([0.7, 0.55, 0.35])
    }
}

impl Cards {
    pub fn len(&self) -> usize {
        self.corners.len()
    }

    pub fn is_empty(&self) -> bool {
        self.corners.is_empty()
    }

    pub fn quads(&self) -> Mesh {
        let mut mesh = Mesh {
            parts: vec!["fur".to_string()],
            ..Default::default()
        };
        for (k, quad) in self.corners.iter().enumerate() {
            let base = (k * 4) as u32;
            mesh.positions.extend(quad);
            mesh.triangles
                .extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
        }
        mesh.labels = vec![0; mesh.positions.len()];
        mesh
    }

    pub fn shaded(&self, tint: [f64; 3]) -> (Mesh, Vec<[f64; 3]>) {
        let mesh = self.blades();
        let colours = (0..mesh.positions.len())
            .map(|i| match i % 3 {
                2 => tint.map(|c| (c * 1.15).min(1.0)),
                _ => tint.map(|c| c * 0.6),
            })
            .collect();
        (mesh, colours)
    }

    pub fn blades(&self) -> Mesh {
        let mut mesh = Mesh {
            parts: vec!["fur".to_string()],
            ..Default::default()
        };
        for (k, quad) in self.corners.iter().enumerate() {
            let tip = scale(add(quad[2], quad[3]), 0.5);
            let mut corners = [quad[0], quad[1], tip];
            let face = cross(sub(corners[1], corners[0]), sub(corners[2], corners[0]));
            let base = (k * 3) as u32;
            if dot(face, self.surface[k]) < 0.0 {
                corners.swap(0, 1);
            }
            mesh.positions.extend(corners);
            mesh.triangles.push([base, base + 1, base + 2]);
        }
        mesh.labels = vec![0; mesh.positions.len()];
        mesh
    }
}

const STRAND_WIDTH: usize = 64;
const STRAND_HEIGHT: usize = 128;
const STRANDS: usize = 5;

pub fn strand_png() -> Vec<u8> {
    let mut raw = Vec::with_capacity(STRAND_HEIGHT * (STRAND_WIDTH * 4 + 1));
    for row in 0..STRAND_HEIGHT {
        raw.push(0);
        let t = 1.0 - (row as f64 + 0.5) / STRAND_HEIGHT as f64;
        for col in 0..STRAND_WIDTH {
            let x = (col as f64 + 0.5) / STRAND_WIDTH as f64;
            let covered = (0..STRANDS).any(|s| {
                let base = (s as f64 + 0.5) / STRANDS as f64;
                let lean = (s as f64 - (STRANDS as f64 - 1.0) / 2.0) * 0.03;
                let centre = base + lean * t + 0.03 * (t * 9.0 + s as f64 * 2.1).sin() * t;
                let half = 0.075 * (1.0 - t).max(0.0) + 0.004;
                (x - centre).abs() < half
            });
            let shade = (255.0 * (0.55 + 0.45 * t)) as u8;
            raw.extend([shade, shade, shade, if covered { 255 } else { 0 }]);
        }
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend((STRAND_WIDTH as u32).to_be_bytes());
    header.extend((STRAND_HEIGHT as u32).to_be_bytes());
    header.extend([8, 6, 0, 0, 0]);
    super::bake::chunk(&mut out, b"IHDR", &header);
    super::bake::chunk(
        &mut out,
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
    );
    super::bake::chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Mesh {
        Mesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            labels: vec![0; 4],
            parts: vec!["pad".to_string()],
            ..Default::default()
        }
    }

    fn spec(count: usize) -> FurSpec {
        FurSpec {
            part: Some("pad".to_string()),
            length: 0.2,
            width: 0.05,
            count,
            clump: 0.5,
            droop: 0.0,
            seed: 1,
            colour: None,
        }
    }

    #[test]
    fn grows_the_requested_cards_standing_up_from_the_surface() {
        let cards = spec(50).grow(&square()).unwrap();
        assert_eq!(cards.len(), 50);
        for quad in &cards.corners {
            let tip = (quad[2][2] + quad[3][2]) / 2.0;
            let root = (quad[0][2] + quad[1][2]) / 2.0;
            assert!(tip > 0.05, "tip height {tip}");
            assert!(root <= 1.0e-9, "root height {root}");
        }
    }

    #[test]
    fn same_seed_same_cards() {
        let a = spec(20).grow(&square()).unwrap();
        assert_eq!(a, spec(20).grow(&square()).unwrap());
        let other = FurSpec {
            seed: 2,
            ..spec(20)
        };
        assert_ne!(a, other.grow(&square()).unwrap());
    }

    #[test]
    fn unknown_part_lists_the_parts() {
        let e = FurSpec {
            part: Some("tail".into()),
            ..spec(5)
        }
        .grow(&square())
        .unwrap_err()
        .to_string();
        assert!(e.contains("pad"), "{e}");
    }

    #[test]
    fn strand_texture_is_a_png_with_holes() {
        let png = strand_png();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(png[25], 6);
    }
}
