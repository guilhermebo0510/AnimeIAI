//! Buffer RGB8 usado por todas as métricas (valores sRGB codificados, como um quadro de vídeo).

use aiai_paint::Image;
use std::borrow::Cow;

/// Imagem RGB de 8 bits por canal, entrelaçada, valores sRGB codificados.
#[derive(Clone, Debug, PartialEq)]
pub struct Rgb8 {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Rgb8 {
    /// `data` precisa ter `width*height*3` bytes.
    pub fn from_rgb8(width: u32, height: u32, data: &[u8]) -> Result<Self, String> {
        if data.len() != width as usize * height as usize * 3 {
            return Err(format!("buffer RGB8 de {} bytes não corresponde a {width}x{height}", data.len()));
        }
        Ok(Rgb8 { width, height, data: data.to_vec() })
    }

    /// RGBA8 não pré-multiplicado, achatado sobre branco.
    pub fn from_rgba8(width: u32, height: u32, data: &[u8]) -> Result<Self, String> {
        if data.len() != width as usize * height as usize * 4 {
            return Err(format!("buffer RGBA8 de {} bytes não corresponde a {width}x{height}", data.len()));
        }
        let mut out = Vec::with_capacity(width as usize * height as usize * 3);
        for p in data.chunks_exact(4) {
            let a = p[3] as f32 / 255.0;
            for c in 0..3 {
                out.push((p[c] as f32 * a + 255.0 * (1.0 - a) + 0.5) as u8);
            }
        }
        Ok(Rgb8 { width, height, data: out })
    }

    /// `aiai_paint::Image` (RGBA f32 pré-multiplicado) achatada sobre branco.
    pub fn from_image(img: &Image) -> Self {
        Rgb8 { width: img.width, height: img.height, data: img.to_rgb8_over([1.0, 1.0, 1.0]) }
    }

    pub fn to_image(&self) -> Image {
        let mut rgba = Vec::with_capacity(self.data.len() / 3 * 4);
        for p in self.data.chunks_exact(3) {
            rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
        }
        Image::from_rgba8(self.width, self.height, &rgba)
    }

    #[inline]
    pub fn len_px(&self) -> usize {
        self.width as usize * self.height as usize
    }

    #[inline]
    pub fn px(&self, i: usize) -> [u8; 3] {
        [self.data[i * 3], self.data[i * 3 + 1], self.data[i * 3 + 2]]
    }

    /// Luma BT.709 sobre os valores sRGB codificados (0..255), como pede o protocolo para o SSIM.
    pub fn luma(&self) -> Vec<f32> {
        self.data
            .chunks_exact(3)
            .map(|p| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32)
            .collect()
    }
}

/// Qualquer coisa que se converta em `Rgb8` (sem cópia quando já é `Rgb8`).
pub trait AsRgb8 {
    fn as_rgb8(&self) -> Cow<'_, Rgb8>;
}

impl AsRgb8 for Rgb8 {
    fn as_rgb8(&self) -> Cow<'_, Rgb8> {
        Cow::Borrowed(self)
    }
}

impl AsRgb8 for Image {
    fn as_rgb8(&self) -> Cow<'_, Rgb8> {
        Cow::Owned(Rgb8::from_image(self))
    }
}

pub(crate) fn same_size(a: &Rgb8, b: &Rgb8) -> Result<(), String> {
    if a.width != b.width || a.height != b.height {
        Err(format!("tamanhos diferentes: {}x{} vs {}x{}", a.width, a.height, b.width, b.height))
    } else {
        Ok(())
    }
}
