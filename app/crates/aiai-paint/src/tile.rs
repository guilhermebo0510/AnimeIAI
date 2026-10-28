//! Tiles raster de 256×256: armazenamento em blob (comprimido com LZ4) e conversão entre tiles e imagem.
//!
//! Dois formatos: `AIT1` (RGBA16 pré-multiplicado sRGB, 0..1, o de sempre) e `AIT2` (RGBA f32 pré-multiplicado, sem
//! corte: HDR, auditoria 4.6h). O tile só vira `AIT2` quando tem valor fora de 0..1 (brilho acima do branco que um
//! efeito ou um pincel em luz linear pôs ali); o resto continua `AIT1`, menor e igual ao que as versões antigas leem.

use crate::image::Image;
use aiai_core::TILE_SIZE;

const MAGIC: &[u8; 4] = b"AIT1";
const MAGIC_HDR: &[u8; 4] = b"AIT2";
pub const TILE_PIXELS: usize = (TILE_SIZE * TILE_SIZE) as usize;

/// Codifica um tile (256×256 pixels f32) em blob.
pub fn encode_tile(px: &[[f32; 4]]) -> Vec<u8> {
    assert_eq!(px.len(), TILE_PIXELS);
    // HDR: algum valor acima de 1 (ou abaixo de 0) guarda o tile em f32, sem cortar
    if px.iter().any(|p| p.iter().any(|c| c.is_finite() && !(0.0..=1.0).contains(c))) {
        let mut raw = Vec::with_capacity(TILE_PIXELS * 16);
        for p in px {
            for c in p {
                let v = if c.is_finite() { *c } else { 0.0 };
                raw.extend_from_slice(&v.to_le_bytes());
            }
        }
        let mut out = MAGIC_HDR.to_vec();
        out.extend_from_slice(&lz4_flex::compress_prepend_size(&raw));
        return out;
    }
    let mut raw = Vec::with_capacity(TILE_PIXELS * 8);
    for p in px {
        for c in p {
            raw.extend_from_slice(&((c.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16).to_le_bytes());
        }
    }
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&lz4_flex::compress_prepend_size(&raw));
    out
}

pub fn decode_tile(blob: &[u8]) -> Result<Vec<[f32; 4]>, String> {
    if blob.len() >= 4 && &blob[..4] == MAGIC_HDR {
        let raw = lz4_flex::decompress_size_prepended(&blob[4..]).map_err(|e| e.to_string())?;
        if raw.len() != TILE_PIXELS * 16 {
            return Err(format!("tile HDR com tamanho errado: {}", raw.len()));
        }
        return Ok(raw
            .chunks_exact(16)
            .map(|c| {
                let v = |i: usize| f32::from_le_bytes([c[i], c[i + 1], c[i + 2], c[i + 3]]);
                [v(0), v(4), v(8), v(12)]
            })
            .collect());
    }
    if blob.len() < 4 || &blob[..4] != MAGIC {
        return Err("blob não é um tile AIT1/AIT2".into());
    }
    let raw = lz4_flex::decompress_size_prepended(&blob[4..]).map_err(|e| e.to_string())?;
    if raw.len() != TILE_PIXELS * 8 {
        return Err(format!("tile com tamanho errado: {}", raw.len()));
    }
    Ok(raw
        .chunks_exact(8)
        .map(|c| {
            let v = |i: usize| u16::from_le_bytes([c[i], c[i + 1]]) as f32 / 65535.0;
            [v(0), v(2), v(4), v(6)]
        })
        .collect())
}

/// Copia um tile decodificado para a imagem (o tile (tx, ty) cobre [tx*256, (tx+1)*256) no canvas; `origin` é a
/// posição do canvas que corresponde ao pixel (0,0) da imagem).
pub fn blit_tile(dst: &mut Image, px: &[[f32; 4]], tx: i32, ty: i32, origin: [i32; 2]) {
    let ts = TILE_SIZE as i32;
    let x0 = tx * ts - origin[0];
    let y0 = ty * ts - origin[1];
    for row in 0..ts {
        let y = y0 + row;
        if y < 0 || y >= dst.height as i32 {
            continue;
        }
        for col in 0..ts {
            let x = x0 + col;
            if x < 0 || x >= dst.width as i32 {
                continue;
            }
            let i = dst.idx(x as u32, y as u32);
            dst.data[i] = px[(row * ts + col) as usize];
        }
    }
}

/// Corta a imagem (posicionada em `origin` no canvas) em tiles; tiles totalmente transparentes viram `None`.
pub fn image_to_tiles(img: &Image, origin: [i32; 2]) -> Vec<(i32, i32, Option<Vec<[f32; 4]>>)> {
    let ts = TILE_SIZE as i32;
    let tx0 = origin[0].div_euclid(ts);
    let ty0 = origin[1].div_euclid(ts);
    let tx1 = (origin[0] + img.width as i32 - 1).div_euclid(ts);
    let ty1 = (origin[1] + img.height as i32 - 1).div_euclid(ts);
    let mut out = Vec::new();
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            let mut px = vec![[0.0f32; 4]; TILE_PIXELS];
            let mut any = false;
            for row in 0..ts {
                let y = ty * ts + row - origin[1];
                if y < 0 || y >= img.height as i32 {
                    continue;
                }
                for col in 0..ts {
                    let x = tx * ts + col - origin[0];
                    if x < 0 || x >= img.width as i32 {
                        continue;
                    }
                    let p = img.get(x as u32, y as u32);
                    if p[3] > 0.0 {
                        any = true;
                    }
                    px[(row * ts + col) as usize] = p;
                }
            }
            out.push((tx, ty, if any { Some(px) } else { None }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr_tiles_keep_values_above_white_and_plain_tiles_stay_ait1() {
        let mut px = vec![[0.25f32, 0.5, 0.75, 1.0]; TILE_PIXELS];
        let plain = encode_tile(&px);
        assert_eq!(&plain[..4], b"AIT1");
        // um brilho acima do branco (efeito de luz em linear) vira AIT2 e volta sem corte
        px[7] = [3.5, 1.25, 0.0, 1.0];
        px[8] = [-0.1, 0.0, 0.0, 0.5];
        let hdr = encode_tile(&px);
        assert_eq!(&hdr[..4], b"AIT2");
        let back = decode_tile(&hdr).unwrap();
        assert_eq!(back[7], [3.5, 1.25, 0.0, 1.0]);
        assert_eq!(back[8], [-0.1, 0.0, 0.0, 0.5]);
        assert_eq!(back[0], [0.25, 0.5, 0.75, 1.0]);
        // o AIT1 continua sendo lido igual
        let b = decode_tile(&plain).unwrap();
        assert!((b[0][1] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn tile_roundtrip_and_compression() {
        let mut px = vec![[0.0f32; 4]; TILE_PIXELS];
        for (i, p) in px.iter_mut().enumerate().take(5000) {
            let v = (i % 256) as f32 / 255.0;
            *p = [v * 0.5, 0.25, 0.0, 0.5];
        }
        let blob = encode_tile(&px);
        assert!(blob.len() < TILE_PIXELS * 8 / 4, "compressão razoável: {}", blob.len());
        let back = decode_tile(&blob).unwrap();
        for (a, b) in px.iter().zip(&back) {
            for c in 0..4 {
                assert!((a[c] - b[c]).abs() < 1.0 / 65535.0 + 1e-7);
            }
        }
    }
}
