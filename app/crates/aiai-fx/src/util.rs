//! Utilidades comuns dos efeitos: luminância, suavização, amostragem bilinear, mistura pré-multiplicada.
//!
//! Espaço de trabalho: sRGB codificado, pré-multiplicado (como o padrão do After Effects). Nenhum efeito linearize cor
//! internamente; as intensidades de glow/difusão são, portanto, perceptuais (documentado em cada efeito).

use aiai_paint::Image;
use aiai_paint::blend::blend_pixel;
use aiai_core::BlendMode;
use rayon::prelude::*;

pub type Px = [f32; 4];

pub const ZERO: Px = [0.0; 4];

/// Luminância Rec.709 sobre valores codificados (luma, não luminância linear).
#[inline]
pub fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

#[inline]
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return if x >= e1 { 1.0 } else { 0.0 };
    }
    let t = ((x - e0) / (e1 - e0)).max(0.0).min(1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
pub fn unpremul(p: Px) -> [f32; 3] {
    if p[3] > 1e-12 { [p[0] / p[3], p[1] / p[3], p[2] / p[3]] } else { [0.0; 3] }
}

/// Garante pixel válido: alfa em 0..1, cor em 0..alfa, sem NaN.
#[inline]
pub fn fix(p: &mut Px) {
    let a = p[3].max(0.0).min(1.0);
    p[3] = a;
    for c in 0..3 {
        p[c] = p[c].max(0.0).min(a);
    }
}

#[inline]
pub fn scale(p: Px, k: f32) -> Px {
    [p[0] * k, p[1] * k, p[2] * k, p[3] * k]
}

/// Tela pré-multiplicada: `dst + src − dst·src` em cor e alfa (igual ao Screen sobre cor reta quando dst é opaco).
#[inline]
pub fn screen_px(dst: &mut Px, src: Px) {
    for c in 0..4 {
        let s = src[c].max(0.0).min(1.0);
        dst[c] = dst[c] + s - dst[c] * s;
    }
    fix(dst);
}

/// Soma de luz pré-multiplicada (limitada pelo alfa resultante).
#[inline]
pub fn add_px(dst: &mut Px, src: Px) {
    for c in 0..4 {
        dst[c] += src[c].max(0.0);
    }
    fix(dst);
}

/// Fonte `src` (pré-multiplicada) ATRÁS de `dst` (sombras e contornos).
#[inline]
pub fn under_px(dst: &mut Px, src: Px) {
    let k = 1.0 - dst[3];
    for c in 0..4 {
        dst[c] += src[c] * k;
    }
}

/// Mistura uma cor reta (RGBA) sobre o conteúdo recortada pelo alfa do conteúdo (o alfa do pixel não muda).
#[inline]
pub fn blend_clipped(p: &mut Px, col: [f32; 4], mode: BlendMode) {
    let a = p[3];
    if a <= 0.0 {
        return;
    }
    let cb = unpremul(*p);
    let mut dst = [cb[0], cb[1], cb[2], 1.0];
    let ca = col[3].max(0.0).min(1.0);
    blend_pixel(&mut dst, [col[0] * ca, col[1] * ca, col[2] * ca, ca], mode, 1.0);
    p[0] = dst[0] * a;
    p[1] = dst[1] * a;
    p[2] = dst[2] * a;
    fix(p);
}

#[inline]
fn at(img: &Image, x: i32, y: i32) -> Px {
    let xi = x.max(0).min(img.width as i32 - 1) as usize;
    let yi = y.max(0).min(img.height as i32 - 1) as usize;
    img.data[yi * img.width as usize + xi]
}

/// Amostra bilinear com borda repetida. Coordenadas em pixels com o centro do pixel em (x+0.5, y+0.5).
#[inline]
pub fn sample(img: &Image, x: f32, y: f32) -> Px {
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = fx.floor();
    let y0 = fy.floor();
    let tx = fx - x0;
    let ty = fy - y0;
    let (xi, yi) = (x0 as i32, y0 as i32);
    let (w, h) = (img.width as i32, img.height as i32);
    let (a, b, c, d) = if xi >= 0 && yi >= 0 && xi + 1 < w && yi + 1 < h {
        // caminho rápido: os 4 vizinhos estão dentro da imagem
        let i = (yi * w + xi) as usize;
        (img.data[i], img.data[i + 1], img.data[i + w as usize], img.data[i + w as usize + 1])
    } else {
        (at(img, xi, yi), at(img, xi + 1, yi), at(img, xi, yi + 1), at(img, xi + 1, yi + 1))
    };
    let mut o = ZERO;
    for k in 0..4 {
        let top = a[k] + (b[k] - a[k]) * tx;
        let bot = c[k] + (d[k] - c[k]) * tx;
        o[k] = top + (bot - top) * ty;
    }
    o
}

/// Como `sample`, mas fora da imagem vale transparente (sombras, deslocamentos de máscara).
#[inline]
pub fn sample_zero(img: &Image, x: f32, y: f32) -> Px {
    let w = img.width as f32;
    let h = img.height as f32;
    // fora da imagem por mais de um pixel: zero; perto da borda: interpola com zero
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = fx.floor();
    let y0 = fy.floor();
    let tx = fx - x0;
    let ty = fy - y0;
    let (xi, yi) = (x0 as i32, y0 as i32);
    let get = |xx: i32, yy: i32| -> Px {
        if xx < 0 || yy < 0 || xx as f32 >= w || yy as f32 >= h { ZERO } else { img.data[yy as usize * img.width as usize + xx as usize] }
    };
    let (a, b, c, d) = (get(xi, yi), get(xi + 1, yi), get(xi, yi + 1), get(xi + 1, yi + 1));
    let mut o = ZERO;
    for k in 0..4 {
        let top = a[k] + (b[k] - a[k]) * tx;
        let bot = c[k] + (d[k] - c[k]) * tx;
        o[k] = top + (bot - top) * ty;
    }
    o
}

/// Percorre todos os pixels em paralelo (x, y, pixel).
pub fn for_each_pixel<F: Fn(u32, u32, &mut Px) + Sync + Send>(img: &mut Image, f: F) {
    let w = img.width as usize;
    if w == 0 || img.height == 0 {
        return;
    }
    img.data.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, p) in row.iter_mut().enumerate() {
            f(x as u32, y as u32, p);
        }
    });
}

/// Constrói uma nova imagem do mesmo tamanho a partir de `f(x, y)`.
pub fn generate<F: Fn(u32, u32) -> Px + Sync + Send>(w: u32, h: u32, f: F) -> Image {
    let mut out = Image::new(w, h);
    for_each_pixel(&mut out, |x, y, p| *p = f(x, y));
    out
}

pub fn check_finite(name: &str, vals: &[f32]) -> Result<(), String> {
    if vals.iter().all(|v| v.is_finite()) { Ok(()) } else { Err(format!("{name}: parâmetro não finito")) }
}

pub fn is_empty(img: &Image) -> bool {
    img.width == 0 || img.height == 0
}

/// Hash inteiro de 2D + semente (ruído branco determinístico).
#[inline]
pub fn hash32(x: u32, y: u32, seed: u32) -> u32 {
    let mut h = x.wrapping_mul(0x85eb_ca6b) ^ y.wrapping_mul(0xc2b2_ae35).rotate_left(13) ^ seed.wrapping_mul(0x27d4_eb2f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^ (h >> 15)
}

/// Ruído branco em [0, 1).
#[inline]
pub fn hash01(x: u32, y: u32, seed: u32) -> f32 {
    (hash32(x, y, seed) >> 8) as f32 / 16_777_216.0
}

/// Semente de 32 bits a partir da semente do contexto e do quadro.
pub fn seed32(seed: u64, frame: u32) -> u32 {
    let s = seed ^ (frame as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    ((s >> 32) as u32) ^ (s as u32).wrapping_mul(0x85eb_ca6b)
}
