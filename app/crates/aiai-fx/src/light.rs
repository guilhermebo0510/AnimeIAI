//! Efeitos de luz: glow, difusão, halação, luz transmitida, light wrap, rim light, lens flare, god rays.
//!
//! Todos operam em sRGB codificado pré-multiplicado, sem linearizar (as intensidades são perceptuais, como num
//! comp do After Effects em modo não linear). A luz acrescentada é combinada com `add_px`/`screen_px`, que mantêm o
//! pixel válido (cor ≤ alfa ≤ 1); o alfa da camada cresce onde a luz vaza para fora da forma.

use crate::blur::{blur_fast, downsample2, upsample_to};
use crate::util::*;
use aiai_paint::Image;
use aiai_paint::fill::distance_to_boundary;
use rayon::prelude::*;

/// Joelho suave do limiar (evita cintilação quando um pixel cruza o limiar).
const KNEE: f32 = 0.05;

#[inline]
fn soft_excess(x: f32) -> f32 {
    if x <= -KNEE {
        0.0
    } else if x < KNEE {
        let t = x + KNEE;
        t * t / (4.0 * KNEE)
    } else {
        x
    }
}

/// Realces acima do limiar de luma (normalizados para 1 no branco). Mantém a cor (ou usa `tint`).
pub fn extract_highlights(img: &Image, threshold: f32, tint: Option<[f32; 3]>) -> Image {
    let thr = threshold.max(0.0).min(0.999);
    let den = 1.0 - thr;
    let mut out = Image::new(img.width, img.height);
    out.data.par_iter_mut().zip(img.data.par_iter()).for_each(|(o, p)| {
        let a = p[3];
        if a <= 0.0 {
            return;
        }
        let c = unpremul(*p);
        let l = luma(c);
        let e = (soft_excess(l - thr) / den).min(1.0);
        if e <= 0.0 {
            return;
        }
        match tint {
            None => *o = scale(*p, (e / l.max(1e-4)).min(1.0)),
            Some(t) => {
                let s = a * e;
                *o = [t[0] * s, t[1] * s, t[2] * s, s * t[0].max(t[1]).max(t[2]).max(1e-4)];
            }
        }
    });
    out
}

/// Soma ponderada de gaussianas rápidas (σ, peso).
pub fn multi_blur(src: &Image, bands: &[(f32, f32)]) -> Image {
    let mut out = Image::new(src.width, src.height);
    for &(sigma, wgt) in bands {
        let b = blur_fast(src, sigma);
        out.data.par_iter_mut().zip(b.data.par_iter()).for_each(|(o, v)| {
            for k in 0..4 {
                o[k] += wgt * v[k];
            }
        });
    }
    out
}

fn composite_light(img: &mut Image, light: &Image, gain: f32, screen: bool) {
    img.data.par_iter_mut().zip(light.data.par_iter()).for_each(|(p, l)| {
        let s = scale(*l, gain);
        if screen { screen_px(p, s) } else { add_px(p, s) }
    });
}

/// Glow: realces (luma − limiar)+ → soma de gaussianas em r/4, r/2 e r (σ = raio/3 em cada nível, pesos
/// 0,5/0,3/0,2) → soma (ou tela) × intensidade, opcionalmente colorido.
pub fn glow(img: &mut Image, threshold: f32, radius: f32, intensity: f32, color: Option<[f32; 3]>, screen: bool) {
    if is_empty(img) || intensity == 0.0 {
        return;
    }
    let radius = radius.max(0.5);
    let hp = extract_highlights(img, threshold, color);
    let s = radius / 12.0;
    let g = multi_blur(&hp, &[(s, 0.5), (2.0 * s, 0.3), (4.0 * s, 0.2)]);
    composite_light(img, &g, intensity, screen);
}

/// Difusão (soft focus): cópia borrada em tela, ponderada por `amount` e por uma rampa de luminância da própria
/// cópia borrada (limiar 0 = em tudo).
pub fn diffusion(img: &mut Image, radius: f32, amount: f32, threshold: f32) {
    if is_empty(img) || amount <= 0.0 {
        return;
    }
    let b = blur_fast(img, radius.max(0.5) / 3.0);
    let thr = threshold.max(0.0).min(0.99);
    let amount = amount.min(1.0);
    img.data.par_iter_mut().zip(b.data.par_iter()).for_each(|(p, bl)| {
        let m = if thr <= 0.0 { 1.0 } else { smoothstep(thr, (thr + 0.2).min(1.0), luma(unpremul(*bl))) };
        screen_px(p, scale(*bl, amount * m));
    });
}

/// Halação: só os realces mais fortes, tingidos de vermelho/laranja, borrados largo (r/2 e r) e somados.
pub fn halation(img: &mut Image, threshold: f32, radius: f32, intensity: f32) {
    if is_empty(img) || intensity == 0.0 {
        return;
    }
    let hp = extract_highlights(img, threshold, Some([1.0, 0.32, 0.08]));
    let s = radius.max(0.5) / 3.0;
    let g = multi_blur(&hp, &[(s * 0.5, 0.45), (s, 0.55)]);
    composite_light(img, &g, intensity, true);
}

fn alpha_image(img: &Image) -> Image {
    let mut out = Image::new(img.width, img.height);
    out.data.par_iter_mut().zip(img.data.par_iter()).for_each(|(o, p)| *o = [p[3]; 4]);
    out
}

/// 透過光: o alfa da camada é o emissor. Núcleo (cor clareada) em tela dentro da forma + 3 glows empilhados de
/// raios crescentes (r/4, r/2, r), somados.
pub fn transmitted_light(img: &mut Image, radius: f32, intensity: f32, color: [f32; 3]) {
    if is_empty(img) || intensity <= 0.0 {
        return;
    }
    let e = alpha_image(img);
    let s = radius.max(0.5) / 3.0;
    let g = multi_blur(&e, &[(s * 0.25, 0.8 * intensity), (s * 0.5, 0.5 * intensity), (s, 0.3 * intensity)]);
    let core_color = [lerp(color[0], 1.0, 0.7), lerp(color[1], 1.0, 0.7), lerp(color[2], 1.0, 0.7)];
    let k = (intensity * 0.6).min(1.0);
    img.data.par_iter_mut().zip(g.data.par_iter()).zip(e.data.par_iter()).for_each(|((p, gl), em)| {
        let a = em[3];
        screen_px(p, [core_color[0] * a * k, core_color[1] * a * k, core_color[2] * a * k, a * k]);
        let gv = gl[3].min(1.0);
        screen_px(p, [color[0] * gv, color[1] * gv, color[2] * gv, gv]);
    });
}

/// Light wrap: luz do fundo borrado (`below`) × faixa interna da borda do alfa (alfa − alfa erodido, borrada),
/// somada à camada dentro do alfa. A faixa mede `raio/2` px.
pub fn light_wrap(img: &mut Image, below: &Image, radius: f32, amount: f32) -> Result<(), String> {
    if is_empty(img) || amount <= 0.0 {
        return Ok(());
    }
    if below.width != img.width || below.height != img.height {
        return Err("light_wrap: o fundo tem tamanho diferente da camada".into());
    }
    let (w, h) = (img.width as usize, img.height as usize);
    let width = (radius * 0.5).max(1.0);
    let outside: Vec<bool> = img.data.iter().map(|p| p[3] <= 0.5).collect();
    let d = distance_to_boundary(&outside, w, h);
    let mut band = Image::new(img.width, img.height);
    band.data.par_iter_mut().zip(img.data.par_iter()).zip(d.par_iter()).for_each(|((o, p), &dist)| {
        // alfa − alfa erodido: dentro do alfa e a no máximo `width` da borda
        let v = if dist <= width { p[3] } else { 0.0 };
        *o = [v; 4];
    });
    let band = blur_fast(&band, (width / 3.0).max(0.5));
    let wrap = blur_fast(below, radius.max(0.5) / 3.0);
    let amount = amount.min(4.0);
    img.data.par_iter_mut().zip(band.data.par_iter()).zip(wrap.data.par_iter()).for_each(|((p, b), wr)| {
        let a = p[3];
        if a <= 0.0 {
            return;
        }
        let wc = unpremul(*wr);
        let k = b[3].min(1.0) * amount * a;
        p[0] += wc[0] * k;
        p[1] += wc[1] * k;
        p[2] += wc[2] * k;
        fix(p);
    });
    Ok(())
}

/// Rim light 2D: `angle` (graus, horário a partir de +x, em y para baixo) é a direção da luz; a faixa é
/// alfa − alfa deslocado `width` px na direção da luz (crescente), suavizada e somada dentro do alfa.
pub fn rim_light(img: &mut Image, angle: f32, width: f32, color: [f32; 3], intensity: f32) {
    if is_empty(img) || intensity <= 0.0 {
        return;
    }
    let width = width.max(0.5);
    let a = angle.to_radians();
    let (dx, dy) = (a.cos() * width, a.sin() * width);
    let ai = alpha_image(img);
    let mut rim = Image::new(img.width, img.height);
    rim.data.par_chunks_mut(img.width as usize).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            let a0 = ai.data[y * ai.width as usize + x][3];
            let a1 = sample(&ai, x as f32 + 0.5 + dx, y as f32 + 0.5 + dy)[3];
            *o = [(a0 - a1).max(0.0); 4];
        }
    });
    let rim = blur_fast(&rim, (width / 4.0).max(0.4));
    img.data.par_iter_mut().zip(rim.data.par_iter()).for_each(|(p, r)| {
        let a = p[3];
        if a <= 0.0 {
            return;
        }
        let k = (r[3] * intensity).max(0.0) * a;
        p[0] += color[0] * k;
        p[1] += color[1] * k;
        p[2] += color[2] * k;
        fix(p);
    });
}

/// Lens flare procedural: núcleo + halo + estria anamórfica em `position`, e fantasmas (anéis/discos) ao longo da
/// reta que passa pelo centro da imagem. Somado em tela. Tamanhos relativos à diagonal.
pub fn lens_flare(img: &mut Image, position: [f32; 2], intensity: f32, color: [f32; 3]) {
    if is_empty(img) || intensity <= 0.0 {
        return;
    }
    let (w, h) = (img.width as f32, img.height as f32);
    let diag = (w * w + h * h).sqrt();
    let c = [w * 0.5, h * 0.5];
    let v = [position[0] - c[0], position[1] - c[1]];
    // (posição ao longo do eixo, raio, intensidade, matiz: 0 = cor da fonte, 1 = complementar)
    let ghosts: [(f32, f32, f32, f32); 5] = [(-0.35, 0.040, 0.30, 0.0), (0.55, 0.025, 0.22, 0.5), (-0.80, 0.060, 0.26, 1.0), (-1.30, 0.090, 0.16, 0.3), (1.50, 0.045, 0.14, 0.8)];
    let comp = [1.0 - color[0] * 0.6, 1.0 - color[1] * 0.3, 1.0 - color[2] * 0.0];
    let tints: Vec<[f32; 3]> = ghosts.iter().map(|g| [lerp(color[0], comp[0], g.3), lerp(color[1], comp[1], g.3), lerp(color[2], comp[2], g.3)]).collect();
    let gpos: Vec<[f32; 2]> = ghosts.iter().map(|g| [c[0] + v[0] * g.0, c[1] + v[1] * g.0]).collect();
    for_each_pixel(img, |x, y, p| {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let (dx, dy) = (px - position[0], py - position[1]);
        let d = (dx * dx + dy * dy).sqrt() / diag;
        // núcleo branco-quente, halo largo e estria horizontal
        let core = (-(d / 0.010).powi(2)).exp() + 0.35 * (-(d / 0.045).powi(2)).exp();
        let halo = 0.16 * (-(d / 0.17).powi(2)).exp();
        let streak = 0.45 * (-(dy / diag / 0.0035).powi(2)).exp() * (-(dx.abs() / diag) / 0.22).exp();
        let mut rgb = [0.0f32; 3];
        for k in 0..3 {
            rgb[k] = core * lerp(color[k], 1.0, 0.65) + (halo + streak) * color[k];
        }
        for (i, g) in ghosts.iter().enumerate() {
            let (gx, gy) = (px - gpos[i][0], py - gpos[i][1]);
            let gd = (gx * gx + gy * gy).sqrt() / diag;
            let ring = (-((gd - g.1) / 0.006).powi(2)).exp();
            let disc = 0.35 * (1.0 - smoothstep(g.1 * 0.7, g.1, gd));
            let a = (ring * 0.9 + disc) * g.2;
            for k in 0..3 {
                rgb[k] += a * tints[i][k];
            }
        }
        let f = [rgb[0] * intensity, rgb[1] * intensity, rgb[2] * intensity];
        let al = f[0].max(f[1]).max(f[2]).min(1.0);
        if al > 0.0 {
            screen_px(p, [f[0], f[1], f[2], al]);
        }
    });
}

/// God rays: realces acima do limiar, desfoque radial acumulativo em direção ao centro (a luz se espalha a partir
/// dele; `length` = fração 0..1 da distância ao centro percorrida), calculado em meia resolução e somado.
pub fn god_rays(img: &mut Image, center: [f32; 2], threshold: f32, length: f32, intensity: f32) {
    if is_empty(img) || intensity <= 0.0 {
        return;
    }
    let hp = extract_highlights(img, threshold, None);
    let low = downsample2(&hp);
    let length = length.max(0.0).min(1.0);
    let c = [center[0] * 0.5, center[1] * 0.5];
    const N: usize = 48;
    let weights: Vec<f32> = (0..N).map(|k| (-1.5 * k as f32 / N as f32).exp()).collect();
    let wsum: f32 = weights.iter().sum();
    let rays = generate(low.width, low.height, |x, y| {
        let (qx, qy) = (x as f32 + 0.5, y as f32 + 0.5);
        let (vx, vy) = (c[0] - qx, c[1] - qy);
        let mut acc = ZERO;
        for (k, wk) in weights.iter().enumerate() {
            let t = length * k as f32 / N as f32;
            let s = sample(&low, qx + vx * t, qy + vy * t);
            for j in 0..4 {
                acc[j] += wk * s[j];
            }
        }
        scale(acc, 1.0 / wsum)
    });
    let rays = upsample_to(&rays, img.width, img.height);
    composite_light(img, &rays, intensity, false);
}
