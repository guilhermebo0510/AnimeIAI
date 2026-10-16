//! Modos de mistura (W3C Compositing and Blending Level 1), em sRGB codificado como Photoshop/After Effects.
//! Entrada e saída pré-multiplicadas; a função de mistura opera sobre cores não pré-multiplicadas.

use aiai_core::BlendMode;

#[inline]
fn unpremul(p: [f32; 4]) -> [f32; 3] {
    if p[3] > 0.0 { [p[0] / p[3], p[1] / p[3], p[2] / p[3]] } else { [0.0; 3] }
}

#[inline]
fn sep(mode: BlendMode, cb: f32, cs: f32) -> f32 {
    match mode {
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::Overlay => hard_light(cs, cb),
        BlendMode::HardLight => hard_light(cb, cs),
        BlendMode::SoftLight => soft_light(cb, cs),
        BlendMode::Add => (cb + cs).min(1.0),
        BlendMode::Subtract => (cb - cs).max(0.0),
        BlendMode::Darken => cb.min(cs),
        BlendMode::Lighten => cb.max(cs),
        BlendMode::Difference => (cb - cs).abs(),
        _ => cs,
    }
}

#[inline]
fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 { cb * 2.0 * cs } else { let s = 2.0 * cs - 1.0; cb + s - cb * s }
}

#[inline]
fn soft_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
    } else {
        let d = if cb <= 0.25 { ((16.0 * cb - 12.0) * cb + 4.0) * cb } else { cb.sqrt() };
        cb + (2.0 * cs - 1.0) * (d - cb)
    }
}

#[inline]
fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 {
        for v in &mut out {
            *v = l + (*v - l) * l / (l - n).max(1e-9);
        }
    }
    if x > 1.0 {
        for v in &mut out {
            *v = l + (*v - l) * (1.0 - l) / (x - l).max(1e-9);
        }
    }
    out
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|&a, &b| c[a].partial_cmp(&c[b]).unwrap_or(std::cmp::Ordering::Equal));
    let (mn, md, mx) = (idx[0], idx[1], idx[2]);
    let mut out = [0.0f32; 3];
    if c[mx] > c[mn] {
        out[md] = (c[md] - c[mn]) * s / (c[mx] - c[mn]);
        out[mx] = s;
    }
    out[mn] = 0.0;
    out
}

fn non_separable(mode: BlendMode, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    match mode {
        BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        BlendMode::Color => set_lum(cs, lum(cb)),
        BlendMode::Luminosity => set_lum(cb, lum(cs)),
        _ => cs,
    }
}

/// Mistura `src` (pré-multiplicado) sobre `dst` com o modo e um fator de alfa extra (opacidade × máscara × recorte).
#[inline]
pub fn blend_pixel(dst: &mut [f32; 4], src: [f32; 4], mode: BlendMode, alpha: f32) {
    let a_s = src[3] * alpha;
    if a_s <= 0.0 {
        return;
    }
    let a_b = dst[3];
    match mode {
        BlendMode::Normal | BlendMode::PassThrough => {
            let k = 1.0 - a_s;
            for c in 0..3 {
                dst[c] = src[c] * alpha + dst[c] * k;
            }
            dst[3] = a_s + a_b * k;
        }
        BlendMode::Behind => {
            // destino sobre a fonte
            let k = 1.0 - a_b;
            for c in 0..3 {
                dst[c] += src[c] * alpha * k;
            }
            dst[3] = a_b + a_s * k;
        }
        _ => {
            let cs = unpremul(src);
            let cb = unpremul(*dst);
            let b = match mode {
                BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => non_separable(mode, cb, cs),
                _ => [sep(mode, cb[0], cs[0]), sep(mode, cb[1], cs[1]), sep(mode, cb[2], cs[2])],
            };
            for c in 0..3 {
                dst[c] = (1.0 - a_b) * cs[c] * a_s + (1.0 - a_s) * cb[c] * a_b + a_s * a_b * b[c];
            }
            dst[3] = a_s + a_b - a_s * a_b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(r: f32, g: f32, b: f32, a: f32) -> [f32; 4] {
        [r * a, g * a, b * a, a]
    }

    #[test]
    fn known_values_on_opaque_backdrop() {
        let bg = px(0.5, 0.25, 1.0, 1.0);
        let fg = px(0.5, 0.5, 0.5, 1.0);
        let run = |m| {
            let mut d = bg;
            blend_pixel(&mut d, fg, m, 1.0);
            d
        };
        let close = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
        assert!(close(run(BlendMode::Normal), fg));
        assert!(close(run(BlendMode::Multiply), px(0.25, 0.125, 0.5, 1.0)));
        assert!(close(run(BlendMode::Screen), px(0.75, 0.625, 1.0, 1.0)));
        assert!(close(run(BlendMode::Difference), px(0.0, 0.25, 0.5, 1.0)));
        assert!(close(run(BlendMode::Add), px(1.0, 0.75, 1.0, 1.0)));
        // atrás de um fundo opaco não aparece nada
        assert!(close(run(BlendMode::Behind), bg));
    }

    #[test]
    fn transparent_backdrop_keeps_source_color() {
        for m in [BlendMode::Multiply, BlendMode::Screen, BlendMode::Overlay, BlendMode::Color, BlendMode::SoftLight] {
            let mut d = [0.0; 4];
            let s = px(0.2, 0.4, 0.6, 0.5);
            blend_pixel(&mut d, s, m, 1.0);
            assert!(d.iter().zip(s).all(|(x, y)| (x - y).abs() < 1e-5), "{m:?}: {d:?}");
        }
    }
}
