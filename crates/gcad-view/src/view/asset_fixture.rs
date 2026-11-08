use crate::asset::{
    Mesh, atlas,
    material::{Material, NormalConvention},
};
use crate::model::texture::Map;
use anyhow::{Result, ensure};

const SLOTS: usize = 3;

pub fn encode(atlas: &atlas::Atlas, maps: [Vec<Map>; 5]) -> Result<Vec<u8>> {
    ensure!(
        maps[SLOTS..].iter().all(Vec::is_empty),
        "GCADAT01 defines only three map slots"
    );
    let mut bytes = b"GCADAT01".to_vec();
    bytes.extend(6u32.to_le_bytes());
    for i in 0..6 {
        for v in atlas.mesh.positions[i]
            .into_iter()
            .chain(atlas.mesh.normals[i])
            .chain(atlas.mesh.texcoords.as_ref().unwrap()[i])
        {
            bytes.extend((v as f32).to_le_bytes());
        }
    }
    for slot in maps.into_iter().take(SLOTS) {
        bytes.extend((slot.len() as u32).to_le_bytes());
        for map in slot {
            bytes.extend((map.width as u32).to_le_bytes());
            bytes.extend((map.height as u32).to_le_bytes());
            bytes.extend(map.rgba);
        }
    }
    ensure!(bytes.len() < 1024 * 1024, "fixture exceeds bound");
    Ok(bytes)
}

pub fn source() -> Result<(atlas::Atlas, [Vec<Map>; 5])> {
    let mesh = Mesh {
        positions: vec![
            [-1., -1., 0.],
            [0., -1., 0.],
            [-0.5, 1., 0.],
            [0., -1., 0.],
            [1., -1., 0.],
            [0.5, 1., 0.],
        ],
        normals: vec![[0., 0., 1.]; 6],
        texcoords: Some(vec![
            [0.1, 0.1],
            [0.2, 0.1],
            [0.1, 0.2],
            [0.8, 0.1],
            [0.9, 0.1],
            [0.8, 0.2],
        ]),
        triangles: vec![[0, 1, 2], [3, 4, 5]],
        tangents: None,
    };
    let atlas = atlas::pack(&mesh, 128, 64, 8, 200.)?;
    let flat = |rgba| {
        Some(Map {
            width: 1,
            height: 1,
            rgba: Vec::from(rgba),
        })
    };
    let material = Material {
        base_colour: [0.25, 0.5, 1., 1.],
        colour_map: Some(Map {
            width: 64,
            height: 1,
            rgba: (0..64)
                .flat_map(|x| {
                    if x < 32 {
                        [255, 0, 0, 255]
                    } else {
                        [0, 0, 255, 255]
                    }
                })
                .collect(),
        }),
        normal_map: flat([128, 64, 255, 255]),
        normal_convention: NormalConvention::NegativeY,
        roughness_map: flat([64; 4]),
        metallic_map: flat([192; 4]),
        ..Material::default()
    };
    let chain = atlas.rebake_chain(&material, 3)?;
    let maps = super::mip_images(&chain)?;
    Ok((atlas, maps))
}

pub fn renderer_fixture() -> Result<Vec<u8>> {
    let (atlas, maps) = source()?;
    encode(&atlas, maps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(bytes: &[u8], at: usize) -> usize {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
    }

    #[test]
    fn renderer_fixture_has_three_slots_of_four_levels() {
        let bytes = renderer_fixture().unwrap();
        assert_eq!(bytes.len(), 130872);
        assert_eq!(&bytes[..8], b"GCADAT01");
        assert_eq!(u32_at(&bytes, 8), 6);
        let mut at = 12 + 6 * 8 * 4;
        for _ in 0..SLOTS {
            assert_eq!(u32_at(&bytes, at), 4);
            at += 4;
            for _ in 0..4 {
                let size = u32_at(&bytes, at) * u32_at(&bytes, at + 4) * 4;
                at += 8 + size;
            }
        }
        assert_eq!(at, bytes.len());
    }

    #[test]
    fn encode_rejects_filled_fourth_or_fifth_slot() {
        let (atlas, maps) = source().unwrap();
        for slot in [3, 4] {
            let mut filled = maps.clone();
            filled[slot] = maps[0].clone();
            let error = encode(&atlas, filled).unwrap_err().to_string();
            assert!(error.contains("only three map slots"), "{error}");
        }
    }
}
