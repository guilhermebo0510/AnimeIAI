//! Camada de texto: vetoriza os glifos no render (sempre editável) e pinta contorno, preenchimento e sombra direto na
//! imagem de saída, já com câmera/zoom, então o texto fica nítido em qualquer ampliação.

use aiai_paint::poly::fill_polygons;
use aiai_paint::text_layer::{TextLayout, TextParams};
use aiai_paint::{Affine, Image, StrokeSample, rasterize_stroke};

/// Cores resolvidas (RGBA sRGB não pré-multiplicado).
#[derive(Debug, Clone, Copy)]
pub struct TextColors {
    pub fill: [f32; 4],
    pub stroke: [f32; 4],
    pub shadow: [f32; 4],
}

fn premul(c: [f32; 4]) -> [f32; 4] {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

/// Pinta a cobertura `cov` (janela com origem em x0,y0) na imagem local.
fn paint_cov(img: &mut Image, cov: &[f32], x0: i32, y0: i32, cw: u32, ch: u32, color: [f32; 4]) {
    let pm = premul(color);
    for yy in 0..ch {
        let y = y0 + yy as i32;
        if y < 0 || y >= img.height as i32 {
            continue;
        }
        for xx in 0..cw {
            let x = x0 + xx as i32;
            if x < 0 || x >= img.width as i32 {
                continue;
            }
            let c = cov[(yy * cw + xx) as usize];
            if c > 0.0 {
                let i = img.idx(x as u32, y as u32);
                Image::over(&mut img.data[i], pm, c);
            }
        }
    }
}

/// Imagem do tamanho da saída com o texto desenhado. `xf` leva o canvas à saída (movimento da camada e câmera).
/// `None` se não há nada a desenhar (texto vazio ou fora da imagem).
pub fn render_text_layer(p: &TextParams, lay: &TextLayout, colors: &TextColors, xf: Affine, out_w: u32, out_h: u32) -> Option<Image> {
    let k = (xf.0[0] * xf.0[4] - xf.0[1] * xf.0[3]).abs().sqrt().max(1e-4);
    let mut contours = lay.contours(p, k);
    if contours.is_empty() {
        return None;
    }
    if !xf.is_identity() {
        for c in &mut contours {
            for q in c.iter_mut() {
                let (x, y) = xf.apply(q[0], q[1]);
                *q = [x, y];
            }
        }
    }
    let stroke_r = p.stroke.as_ref().map(|s| s.width.max(0.0) * k).filter(|w| *w > 0.0);
    let bold_r = if lay.synth_bold() { Some(p.size * 0.03 * k) } else { None };
    let shadow = p.shadow.as_ref().filter(|_| colors.shadow[3] > 0.0);
    let (sdx, sdy, sblur) = shadow.map(|s| ((s.offset[0] * k).round() as i32, (s.offset[1] * k).round() as i32, s.blur * k)).unwrap_or((0, 0, 0.0));
    // janela local: caixa dos contornos + margem do contorno, negrito e sombra, recortada à saída
    let (mut bx0, mut by0, mut bx1, mut by1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for c in &contours {
        for q in c {
            bx0 = bx0.min(q[0]);
            by0 = by0.min(q[1]);
            bx1 = bx1.max(q[0]);
            by1 = by1.max(q[1]);
        }
    }
    let pad = stroke_r.unwrap_or(0.0).max(bold_r.unwrap_or(0.0)) + 2.0;
    let sh_pad = if shadow.is_some() { sblur * 1.5 + 2.0 } else { 0.0 };
    // a janela cobre o texto e a sombra deslocada; o que cai fora da saída não interessa
    let wx0 = ((bx0 - pad - sh_pad).floor() as i32 + sdx.min(0)).max(0);
    let wy0 = ((by0 - pad - sh_pad).floor() as i32 + sdy.min(0)).max(0);
    let wx1 = (((bx1 + pad + sh_pad).ceil() as i32) + sdx.max(0)).min(out_w as i32);
    let wy1 = (((by1 + pad + sh_pad).ceil() as i32) + sdy.max(0)).min(out_h as i32);
    if wx0 >= wx1 || wy0 >= wy1 {
        return None;
    }
    let (lw, lh) = ((wx1 - wx0) as u32, (wy1 - wy0) as u32);
    for c in &mut contours {
        for q in c.iter_mut() {
            q[0] -= wx0 as f32;
            q[1] -= wy0 as f32;
        }
    }
    let mut shape = Image::new(lw, lh);
    // 1) contorno por fora: traço centrado de raio = espessura; o preenchimento cobre a metade de dentro
    let stroke_layer = |shape: &mut Image, r: f32, color: [f32; 4]| {
        for c in &contours {
            if c.len() < 2 {
                continue;
            }
            let mut smp: Vec<StrokeSample> = c.iter().map(|q| StrokeSample { x: q[0], y: q[1], r }).collect();
            smp.push(smp[0]);
            if let Some(cov) = rasterize_stroke(&smp, false, lw, lh) {
                paint_cov(shape, &cov.cov, cov.x0, cov.y0, cov.width, cov.height, color);
            }
        }
    };
    if let Some(r) = stroke_r {
        stroke_layer(&mut shape, r, colors.stroke);
    }
    // 2) negrito sintético: engrossa as letras com a própria cor
    if let Some(r) = bold_r {
        stroke_layer(&mut shape, r, colors.fill);
    }
    // 3) preenchimento
    if let Some(cov) = fill_polygons(&contours, false, lw, lh) {
        paint_cov(&mut shape, &cov.cov, cov.x0, cov.y0, cov.width, cov.height, colors.fill);
    }
    let mut out = Image::new(out_w, out_h);
    if shadow.is_some() {
        // sombra: o alfa da forma, deslocado, desfocado e tingido
        let sc = colors.shadow;
        let mut sh = Image::new(lw, lh);
        for (d, s) in sh.data.iter_mut().zip(&shape.data) {
            let a = s[3] * sc[3];
            *d = [sc[0] * a, sc[1] * a, sc[2] * a, a];
        }
        if sblur > 0.1 {
            sh = aiai_fx::blur::gaussian(&sh, sblur / 2.0);
        }
        out.draw_over(&sh, wx0 + sdx, wy0 + sdy, 1.0);
    }
    out.draw_over(&shape, wx0, wy0, 1.0);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_paint::text_layer::{TextParams, TextShadow, TextStroke, layout};

    fn colors() -> TextColors {
        TextColors { fill: [0.0, 0.0, 0.0, 1.0], stroke: [1.0, 0.0, 0.0, 1.0], shadow: [0.0, 0.0, 1.0, 0.5] }
    }

    fn bbox(img: &Image) -> Option<[u32; 4]> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..img.height {
            for x in 0..img.width {
                if img.get(x, y)[3] > 0.0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        (x0 <= x1).then_some([x0, y0, x1, y1])
    }

    #[test]
    fn text_renders_non_empty_pixels_inside_its_layout_box() {
        let p = TextParams { content: "Hello".into(), pos: [40.0, 30.0], size: 50.0, ..Default::default() };
        let lay = layout(&p);
        let img = render_text_layer(&p, &lay, &colors(), Affine::IDENTITY, 400, 200).expect("texto desenhado");
        let b = bbox(&img).expect("pixels não vazios");
        let cb = aiai_paint::text_layer::canvas_bounds(&p, &lay);
        assert!(b[0] as f32 >= cb[0] - 1.5 && b[2] as f32 <= cb[2] + 1.5 && b[1] as f32 >= cb[1] - 1.5 && b[3] as f32 <= cb[3] + 1.5, "pixels {b:?} dentro da caixa {cb:?}");
        assert!(b[0] >= 40 && b[1] >= 30, "começa na âncora ou depois: {b:?}");
        // tinta preta opaca em algum lugar
        assert!(img.data.iter().any(|p| p[3] > 0.99 && p[0] < 0.01));
    }

    #[test]
    fn empty_text_draws_nothing() {
        let p = TextParams { content: "  ".into(), ..Default::default() };
        let lay = layout(&p);
        assert!(render_text_layer(&p, &lay, &colors(), Affine::IDENTITY, 100, 100).is_none());
    }

    #[test]
    fn stroke_grows_the_footprint_and_shadow_shifts_it() {
        let base = TextParams { content: "I".into(), pos: [60.0, 40.0], size: 80.0, ..Default::default() };
        let plain = {
            let lay = layout(&base);
            bbox(&render_text_layer(&base, &lay, &colors(), Affine::IDENTITY, 300, 200).unwrap()).unwrap()
        };
        let stroked = TextParams { stroke: Some(TextStroke { width: 6.0, ..Default::default() }), ..base.clone() };
        let lay = layout(&stroked);
        let s = render_text_layer(&stroked, &lay, &colors(), Affine::IDENTITY, 300, 200).unwrap();
        let sb = bbox(&s).unwrap();
        assert!(plain[0] > sb[0] && sb[2] > plain[2] && sb[1] < plain[1], "o contorno amplia a caixa: {plain:?} → {sb:?}");
        assert!(s.data.iter().any(|p| p[3] > 0.99 && p[0] > 0.9 && p[1] < 0.1), "pixel da cor do contorno");
        let shadowed = TextParams { shadow: Some(TextShadow { offset: [20.0, 12.0], blur: 0.0, ..Default::default() }), ..base.clone() };
        let lay = layout(&shadowed);
        let sh = bbox(&render_text_layer(&shadowed, &lay, &colors(), Affine::IDENTITY, 300, 200).unwrap()).unwrap();
        assert!(sh[2] >= plain[2] + 19 && sh[3] >= plain[3] + 11, "sombra deslocada: {plain:?} → {sh:?}");
    }

    #[test]
    fn camera_transform_scales_the_text_crisply() {
        let p = TextParams { content: "A".into(), pos: [20.0, 20.0], size: 40.0, ..Default::default() };
        let lay = layout(&p);
        let one = bbox(&render_text_layer(&p, &lay, &colors(), Affine::IDENTITY, 400, 400).unwrap()).unwrap();
        let two = bbox(&render_text_layer(&p, &lay, &colors(), Affine::from_trs(0.0, 0.0, 2.0, 0.0, [0.0, 0.0]), 400, 400).unwrap()).unwrap();
        let (w1, w2) = (one[2] - one[0], two[2] - two[0]);
        assert!((w2 as f32 - 2.0 * w1 as f32).abs() <= 3.0, "o dobro de largura: {w1} → {w2}");
    }
}
