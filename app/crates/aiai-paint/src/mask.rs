//! Máscara de cobertura 8 bits (regiões de preenchimento, seleções): blob "AIM1" + largura + altura + LZ4.

const MAGIC: &[u8; 4] = b"AIM1";

#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Mask {
    pub fn new(width: u32, height: u32) -> Self {
        Mask { width, height, data: vec![0; width as usize * height as usize] }
    }

    pub fn from_coverage(width: u32, height: u32, cov: &[f32]) -> Self {
        Mask { width, height, data: cov.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect() }
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> f32 {
        self.data[y as usize * self.width as usize + x as usize] as f32 / 255.0
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&lz4_flex::compress_prepend_size(&self.data));
        out
    }

    pub fn decode(blob: &[u8]) -> Result<Self, String> {
        if blob.len() < 12 || &blob[..4] != MAGIC {
            return Err("blob não é uma máscara AIM1".into());
        }
        let width = u32::from_le_bytes(blob[4..8].try_into().expect("4 bytes"));
        let height = u32::from_le_bytes(blob[8..12].try_into().expect("4 bytes"));
        let data = lz4_flex::decompress_size_prepended(&blob[12..]).map_err(|e| e.to_string())?;
        if data.len() != width as usize * height as usize {
            return Err("máscara com tamanho errado".into());
        }
        Ok(Mask { width, height, data })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut m = Mask::new(37, 11);
        m.data[5] = 200;
        assert_eq!(Mask::decode(&m.encode()).unwrap(), m);
    }
}
