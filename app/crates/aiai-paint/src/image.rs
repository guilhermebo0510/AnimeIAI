//! Imagem RGBA f32 pré-multiplicada em sRGB codificado.

use rayon::prelude::*;

#[derive(Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub data: Vec<[f32; 4]>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}x{})", self.width, self.height)
    }
}

impl Image {
    pub fn new(width: u32, height: u32) -> Self {
        Image { width, height, data: vec![[0.0; 4]; width as usize * height as usize] }
    }

    pub fn filled(width: u32, height: u32, rgba_straight: [f32; 4]) -> Self {
        let a = rgba_straight[3];
        let px = [rgba_straight[0] * a, rgba_straight[1] * a, rgba_straight[2] * a, a];
        Image { width, height, data: vec![px; width as usize * height as usize] }
    }

    #[inline]
    pub fn idx(&self, x: u32, y: u32) -> usize {
        y as usize * self.width as usize + x as usize
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> [f32; 4] {
        self.data[self.idx(x, y)]
    }

    pub fn is_blank(&self) -> bool {
        self.data.iter().all(|p| p[3] == 0.0)
    }

    /// Fonte sobre destino (pré-multiplicado), com opacidade extra.
    #[inline]
    pub fn over(dst: &mut [f32; 4], src: [f32; 4], opacity: f32) {
        let a = src[3] * opacity;
        if a <= 0.0 {
            return;
        }
        let k = 1.0 - a;
        dst[0] = src[0] * opacity + dst[0] * k;
        dst[1] = src[1] * opacity + dst[1] * k;
        dst[2] = src[2] * opacity + dst[2] * k;
        dst[3] = a + dst[3] * k;
    }

    /// Desenha `src` sobre esta imagem com deslocamento inteiro (source-over).
    pub fn draw_over(&mut self, src: &Image, dx: i32, dy: i32, opacity: f32) {
        let w = self.width as i32;
        let sw = src.width as i32;
        let x0 = dx.max(0);
        let x1 = (dx + sw).min(w);
        if x0 >= x1 {
            return;
        }
        let y0 = dy.max(0);
        let y1 = (dy + src.height as i32).min(self.height as i32);
        if y0 >= y1 {
            return;
        }
        let rows: Vec<(usize, &mut [[f32; 4]])> = self
            .data
            .par_chunks_mut(w as usize)
            .enumerate()
            .filter(|(y, _)| (*y as i32) >= y0 && (*y as i32) < y1)
            .collect();
        rows.into_par_iter().for_each(|(y, row)| {
            let sy = y as i32 - dy;
            let srow = &src.data[(sy * sw) as usize..((sy + 1) * sw) as usize];
            for x in x0..x1 {
                Image::over(&mut row[x as usize], srow[(x - dx) as usize], opacity);
            }
        });
    }

    /// Converte para RGBA8 não pré-multiplicado (PNG).
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.data.len() * 4);
        for p in &self.data {
            let a = p[3].clamp(0.0, 1.0);
            let un = |c: f32| if a > 0.0 { (c / a).clamp(0.0, 1.0) } else { 0.0 };
            out.push((un(p[0]) * 255.0 + 0.5) as u8);
            out.push((un(p[1]) * 255.0 + 0.5) as u8);
            out.push((un(p[2]) * 255.0 + 0.5) as u8);
            out.push((a * 255.0 + 0.5) as u8);
        }
        out
    }

    /// RGBA8 achatado sobre um fundo opaco (para exibir e comparar com quadros de vídeo).
    pub fn to_rgb8_over(&self, bg: [f32; 3]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.data.len() * 3);
        for p in &self.data {
            let k = 1.0 - p[3].clamp(0.0, 1.0);
            for c in 0..3 {
                out.push(((p[c] + bg[c] * k).clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
            }
        }
        out
    }

    pub fn from_rgba8(width: u32, height: u32, bytes: &[u8]) -> Self {
        let mut img = Image::new(width, height);
        for (i, px) in bytes.chunks_exact(4).enumerate().take(img.data.len()) {
            let a = px[3] as f32 / 255.0;
            img.data[i] = [px[0] as f32 / 255.0 * a, px[1] as f32 / 255.0 * a, px[2] as f32 / 255.0 * a, a];
        }
        img
    }

    pub fn save_png(&self, path: &std::path::Path) -> Result<(), String> {
        ::image::save_buffer(path, &self.to_rgba8(), self.width, self.height, ::image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())
    }

    pub fn load_png(path: &std::path::Path) -> Result<Self, String> {
        let img = ::image::open(path).map_err(|e| e.to_string())?.to_rgba8();
        Ok(Image::from_rgba8(img.width(), img.height(), img.as_raw()))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let img = ::image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_rgba8();
        Ok(Image::from_rgba8(img.width(), img.height(), img.as_raw()))
    }

    pub fn encode_png(&self) -> Result<Vec<u8>, String> {
        let mut out = std::io::Cursor::new(Vec::new());
        ::image::write_buffer_with_format(
            &mut out,
            &self.to_rgba8(),
            self.width,
            self.height,
            ::image::ExtendedColorType::Rgba8,
            ::image::ImageFormat::Png,
        )
        .map_err(|e| e.to_string())?;
        Ok(out.into_inner())
    }

    /// Hash de conteúdo (golden tests): quantiza em 16 bits para não depender de ruído de última casa.
    pub fn content_hash(&self) -> String {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut feed = |v: u16| {
            for b in v.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        };
        feed(self.width as u16);
        feed(self.height as u16);
        for p in &self.data {
            for c in p {
                feed((c.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16);
            }
        }
        format!("{h:016x}")
    }
}
