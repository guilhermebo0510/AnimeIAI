//! Efeitos de estilo/acabamento: para, aberração cromática, vinheta, grão, deband, sombra, contorno, sobreposição de
//! cor e duas cores (impact frame).

use crate::blur::blur_fast;
use crate::util::*;
use aiai_core::BlendMode;
use aiai_paint::Image;
use aiai_paint::fill::distance_to_boundary;
use aiai_paint::noise::value_noise;
use rayon::prelude::*;

/// パラ: degradê linear de `from` a `to` (px) com cores retas RGBA, no modo de mistura, recortado pelo alfa.
pub fn para(img: &mut Image, from: [f32; 2], to: [f32; 2], c0: [f32; 4], c1: [f32; 4], mode: BlendMode) {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let len2 = dx * dx + dy * dy;
    for_each_pixel(img, |x, y, p| {
        if p[3] <= 0.0 {
            return;
        }
        let t = if len2 > 0.0 { (((x as f32 + 0.5 - from[0]) * dx + (y as f32 + 0.5 - from[1]) * dy) / len2).max(0.0).min(1.0) } else { 0.0 };
        let col = [lerp(c0[0], c1[0], t), lerp(c0[1], c1[1], t), lerp(c0[2], c1[2], t), lerp(c0[3], c1[3], t)];
        blend_clipped(p, col, mode);
    });
}

pub fn color_overlay(img: &mut Image, color: [f32; 4], mode: BlendMode) {
    for_each_pixel(img, |_, _, p| blend_clipped(p, color, mode));
}

/// Aberração cromática radial: R amostrado em escala (1+s) e B em (1−s) a partir do centro; `amount` = deslocamento
/// em px no canto mais distante. O alfa resultante é o máximo dos três canais.
pub fn chromatic_aberration(img: &mut Image, amount: f32, center: Option<[f32; 2]>) {
    if is_empty(img) || amount == 0.0 {
        return;
    }
    let (w, h) = (img.width as f32, img.height as f32);
    let c = center.unwrap_or([w * 0.5, h * 0.5]);
    let rmax = [(0.0 - c[0]).abs().max((w - c[0]).abs()), (0.0 - c[1]).abs().max((h - c[1]).abs())];
    let rmax = (rmax[0] * rmax[0] + rmax[1] * rmax[1]).sqrt().max(1.0);
    let s = amount / rmax;
    let src = img.clone();
    for_each_pixel(img, |x, y, p| {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let (vx, vy) = (px - c[0], py - c[1]);
        let r = sample(&src, c[0] + vx * (1.0 + s), c[1] + vy * (1.0 + s));
        let b = sample(&src, c[0] + vx * (1.0 - s), c[1] + vy * (1.0 - s));
        let g = src.data[y as usize * src.width as usize + x as usize];
        let mut o = [r[0], g[1], b[2], r[3].max(g[3]).max(b[3])];
        fix(&mut o);
        *p = o;
    });
}

/// Vinheta elíptica (passa pelos cantos): escurece (ou multiplica por `color`) com smoothstep entre
/// `1 − softness` e 1 do raio normalizado.
pub fn vignette(img: &mut Image, amount: f32, softness: f32, color: Option<[f32; 3]>) {
    if is_empty(img) || amount == 0.0 {
        return;
    }
    let (w, h) = (img.width as f32, img.height as f32);
    let (hw, hh) = (w * 0.5, h * 0.5);
    let col = color.unwrap_or([0.0; 3]);
    let inner = (1.0 - softness.max(0.0).min(1.0)).max(0.0);
    let amount = amount.max(0.0).min(1.0);
    for_each_pixel(img, |x, y, p| {
        let nx = (x as f32 + 0.5 - hw) / hw;
        let ny = (y as f32 + 0.5 - hh) / hh;
        let r = (nx * nx + ny * ny).sqrt() * std::f32::consts::FRAC_1_SQRT_2;
        let v = amount * smoothstep(inner, 1.0, r);
        if v <= 0.0 {
            return;
        }
        for k in 0..3 {
            p[k] *= lerp(1.0, col[k], v);
        }
        fix(p);
    });
}

/// Grão de filme: ruído de luma (valor com célula de `size` px; branco se `size` ≤ 1), semente = semente do contexto
/// ^ quadro, ponderado levemente por luminância (mais nos meios-tons), só onde há alfa.
pub fn grain(img: &mut Image, amount: f32, size: f32, seed: u64, frame: u32) {
    if is_empty(img) || amount == 0.0 {
        return;
    }
    let s = seed32(seed, frame);
    let size = size.max(0.25);
    for_each_pixel(img, |x, y, p| {
        let a = p[3];
        if a <= 0.0 {
            return;
        }
        let n = if size <= 1.0 {
            hash01(x, y, s) * 2.0 - 1.0
        } else {
            // o ruído de valor tem variância menor que o branco; compensa
            ((value_noise(x as f32 / size + 0.37, y as f32 / size + 0.61, s) - 0.5) * 3.2).max(-1.0).min(1.0)
        };
        let l = luma(unpremul(*p));
        let wgt = lerp(0.4, 1.0, 4.0 * l * (1.0 - l));
        let d = n * amount * wgt * a;
        p[0] += d;
        p[1] += d;
        p[2] += d;
        fix(p);
    });
}

/// Deband: dither ordenado por "interleaved gradient noise" (estático: não cintila em quadros repetidos), amplitude
/// ±0,5/255 × `amount`, somado antes da quantização para 8 bits.
pub fn deband(img: &mut Image, amount: f32) {
    if is_empty(img) || amount == 0.0 {
        return;
    }
    let k = amount / 255.0;
    for_each_pixel(img, |x, y, p| {
        let a = p[3];
        if a <= 0.0 {
            return;
        }
        let ign = (52.982_918_9 * ((0.067_110_56 * x as f32 + 0.005_837_15 * y as f32).fract())).fract();
        let d = (ign - 0.5) * k * a;
        p[0] += d;
        p[1] += d;
        p[2] += d;
        fix(p);
    });
}

/// Sombra projetada: alfa borrado (σ = blur/3), deslocado por `offset` e colorido, ATRÁS do conteúdo.
pub fn drop_shadow(img: &mut Image, offset: [f32; 2], blur: f32, color: [f32; 4]) {
    if is_empty(img) {
        return;
    }
    let mut sh = Image::new(img.width, img.height);
    for (o, p) in sh.data.iter_mut().zip(img.data.iter()) {
        *o = [p[3]; 4];
    }
    let sh = blur_fast(&sh, blur.max(0.0) / 3.0);
    let ca = color[3].max(0.0).min(1.0);
    for_each_pixel(img, |x, y, p| {
        let s = sample_zero(&sh, x as f32 + 0.5 - offset[0], y as f32 + 0.5 - offset[1])[3].max(0.0).min(1.0) * ca;
        if s <= 0.0 {
            return;
        }
        under_px(p, [color[0] * s, color[1] * s, color[2] * s, s]);
        fix(p);
    });
}

/// Contorno: anel de alfa dilatado `width` px (transformada de distância euclidiana ao alfa > 0,5, borda com 1 px de
/// anti-aliasing), colorido, atrás do conteúdo.
pub fn outline(img: &mut Image, width: f32, color: [f32; 4]) {
    if is_empty(img) || width <= 0.0 {
        return;
    }
    let (w, h) = (img.width as usize, img.height as usize);
    let inside: Vec<bool> = img.data.iter().map(|p| p[3] > 0.5).collect();
    let d = distance_to_boundary(&inside, w, h);
    let ca = color[3].max(0.0).min(1.0);
    img.data.par_iter_mut().zip(d.par_iter()).for_each(|(p, &dist)| {
        let cov = (width + 0.5 - dist).max(0.0).min(1.0) * ca;
        if cov <= 0.0 {
            return;
        }
        under_px(p, [color[0] * cov, color[1] * cov, color[2] * cov, cov]);
        fix(p);
    });
}

/// Duas cores por limiar de luma (impact frame): `dark` abaixo, `light` acima; alfa preservado. Sem anti-aliasing.
pub fn two_tone(img: &mut Image, threshold: f32, dark: [f32; 3], light: [f32; 3]) {
    for_each_pixel(img, |_, _, p| {
        let a = p[3];
        if a <= 0.0 {
            return;
        }
        let c = if luma(unpremul(*p)) >= threshold { light } else { dark };
        p[0] = c[0] * a;
        p[1] = c[1] * a;
        p[2] = c[2] * a;
        fix(p);
    });
}
