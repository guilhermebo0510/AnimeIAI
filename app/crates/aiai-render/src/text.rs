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
/// `None` se não há nada a desenhar (texto vazio ou fora da imagem). Cores de paleta dentro de degradês/efeitos saem
/// magenta e padrões de imagem não aparecem: o compositor usa `render_text_layer_with` com o projeto.
pub fn render_text_layer(p: &TextParams, lay: &TextLayout, colors: &TextColors, xf: Affine, out_w: u32, out_h: u32) -> Option<Image> {
    render_text_layer_with(p, lay, colors, &mut crate::text_fx::DirectResolver, xf, out_w, out_h)
}

/// `render_text_layer` com quem resolve as cores de paleta e as imagens dos estilos.
pub fn render_text_layer_with(p: &TextParams, lay: &TextLayout, colors: &TextColors, res: &mut dyn crate::text_fx::PaintResolver, xf: Affine, out_w: u32, out_h: u32) -> Option<Image> {
    if p.has_styles() {
        return render_styled(p, lay, colors, res, xf, out_w, out_h);
    }
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

/// Texto com estilos (efeitos, tinta especial, distorção): o mesmo enquadramento, com a margem dos efeitos.
fn render_styled(p: &TextParams, lay: &TextLayout, colors: &TextColors, res: &mut dyn crate::text_fx::PaintResolver, xf: Affine, out_w: u32, out_h: u32) -> Option<Image> {
    let k = (xf.0[0] * xf.0[4] - xf.0[1] * xf.0[3]).abs().sqrt().max(1e-4);
    let mut contours = lay.contours(p, k);
    if contours.is_empty() {
        return None;
    }
    // caixa das letras no espaço local (com a distorção): onde os degradês se apoiam
    let mut gb = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for q in contours.iter().flatten() {
        let l = p.canvas_to_local(*q);
        gb = [gb[0].min(l[0]), gb[1].min(l[1]), gb[2].max(l[0]), gb[3].max(l[1])];
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
    let shadow_reach = p.shadow.as_ref().filter(|_| colors.shadow[3] > 0.0).map(|s| (s.offset[0].abs().max(s.offset[1].abs()) + s.blur * 1.5) * k + stroke_r.unwrap_or(0.0)).unwrap_or(0.0);
    let (mut bx0, mut by0, mut bx1, mut by1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for q in contours.iter().flatten() {
        bx0 = bx0.min(q[0]);
        by0 = by0.min(q[1]);
        bx1 = bx1.max(q[0]);
        by1 = by1.max(q[1]);
    }
    let pad = stroke_r.unwrap_or(0.0).max(bold_r.unwrap_or(0.0)).max(shadow_reach).max(crate::text_fx::styles_reach(p, k)) + 3.0;
    let wx0 = ((bx0 - pad).floor() as i32).max(0);
    let wy0 = ((by0 - pad).floor() as i32).max(0);
    let wx1 = ((bx1 + pad).ceil() as i32).min(out_w as i32);
    let wy1 = ((by1 + pad).ceil() as i32).min(out_h as i32);
    if wx0 >= wx1 || wy0 >= wy1 {
        return None;
    }
    let (lw, lh) = ((wx1 - wx0) as u32, (wy1 - wy0) as u32);
    for q in contours.iter_mut().flatten() {
        q[0] -= wx0 as f32;
        q[1] -= wy0 as f32;
    }
    let img = crate::text_fx::render_styles(p, &contours, colors, res, xf, k, [wx0, wy0], lw, lh, stroke_r, bold_r, gb);
    let mut out = Image::new(out_w, out_h);
    out.draw_over(&img, wx0, wy0, 1.0);
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
    // ------------------------------------------------------------------------------------------------ estilos

    use aiai_core::{BlendMode, Gradient, Paint};
    use aiai_paint::text_layer::*;

    fn base() -> TextParams {
        TextParams { content: "HI".into(), pos: [80.0, 60.0], size: 90.0, bold: true, ..Default::default() }
    }

    fn render(p: &TextParams) -> Image {
        render_text_layer(p, &layout(p), &colors(), Affine::IDENTITY, 420, 260).expect("desenhou")
    }

    fn with_fx(kind: TextEffectKind) -> TextParams {
        TextParams { effects: vec![TextEffect::new(kind)], ..base() }
    }

    /// Pixels onde o texto liso tem tinta (a silhueta).
    fn silhouette() -> Vec<bool> {
        render(&base()).data.iter().map(|q| q[3] > 0.5).collect()
    }

    fn unpremul(q: [f32; 4]) -> [f32; 3] {
        if q[3] <= 0.0 { [0.0; 3] } else { [q[0] / q[3], q[1] / q[3], q[2] / q[3]] }
    }

    fn green() -> aiai_core::ColorRef {
        aiai_core::ColorRef::Direct { rgba: aiai_core::Rgba([0.0, 1.0, 0.0, 1.0]) }
    }

    #[test]
    fn disabled_effects_take_the_legacy_path_bit_for_bit() {
        let plain = render(&base());
        let mut off = with_fx(TextEffectKind::OuterGlow(FxGlow::default()));
        off.effects[0].enabled = false;
        assert!(!off.has_styles());
        assert_eq!(render(&off).content_hash(), plain.content_hash());
    }

    #[test]
    fn gradient_fill_varies_across_the_text() {
        let p = TextParams { fill: Some(Paint::Gradient { gradient: Gradient::from_colors(&[[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]], 0.0) }), ..base() };
        let img = render(&p);
        let b = bbox(&img).unwrap();
        let col_at = |x0: u32, x1: u32| -> [f32; 3] {
            let mut acc = [0.0; 3];
            let mut n = 0.0;
            for y in b[1]..=b[3] {
                for x in x0..=x1 {
                    let q = img.get(x, y);
                    if q[3] > 0.99 {
                        let u = unpremul(q);
                        acc = [acc[0] + u[0], acc[1] + u[1], acc[2] + u[2]];
                        n += 1.0;
                    }
                }
            }
            acc.map(|v| v / f32::max(n, 1.0))
        };
        let left = col_at(b[0], b[0] + 12);
        let right = col_at(b[2] - 12, b[2]);
        assert!(left[0] > 0.7 && left[2] < 0.3, "esquerda vermelha: {left:?}");
        assert!(right[2] > 0.7 && right[0] < 0.3, "direita azul: {right:?}");
    }

    #[test]
    fn outer_glow_extends_beyond_the_glyphs_and_inner_effects_stay_inside() {
        let plain = bbox(&render(&base())).unwrap();
        let glow = render(&with_fx(TextEffectKind::OuterGlow(FxGlow { size: 20.0, ..FxGlow::default() })));
        let gb = bbox(&glow).unwrap();
        assert!(gb[0] + 12 < plain[0] && gb[2] > plain[2] + 12 && gb[1] + 12 < plain[1], "o brilho passa da borda: {plain:?} -> {gb:?}");
        let sil = silhouette();
        for kind in [
            TextEffectKind::InnerShadow(FxShadow { offset: [6.0, 6.0], blur: 3.0, color: green(), ..FxShadow::default() }),
            TextEffectKind::InnerGlow(FxGlow { size: 10.0, paint: Paint::solid([0.0, 1.0, 0.0, 1.0]), ..FxGlow::default() }),
            TextEffectKind::Satin(FxSatin { color: green(), ..FxSatin::default() }),
            TextEffectKind::ColorOverlay(FxColorOverlay { color: green() }),
        ] {
            let name = kind.key();
            let img = render(&with_fx(kind));
            let w = img.width as usize;
            let mut green_inside = 0;
            for (i, q) in img.data.iter().enumerate() {
                let u = unpremul(*q);
                if q[3] > 0.05 && u[1] > 0.3 {
                    let (x, y) = (i % w, i / w);
                    let near = (-1i64..=1).any(|dy| {
                        (-1i64..=1).any(|dx| {
                            let (xx, yy) = (x as i64 + dx, y as i64 + dy);
                            xx >= 0 && yy >= 0 && sil[yy as usize * w + xx as usize]
                        })
                    });
                    assert!(near, "{name}: tinta fora das letras em ({x}, {y})");
                    green_inside += 1;
                }
            }
            assert!(green_inside > 50, "{name}: pintou dentro ({green_inside} px)");
            assert_eq!(bbox(&img), Some(plain), "{name}: não muda a caixa");
        }
    }

    #[test]
    fn strokes_sit_outside_inside_or_centered_and_stack_double() {
        let plain = bbox(&render(&base())).unwrap();
        let red = Paint::solid([1.0, 0.0, 0.0, 1.0]);
        let mk = |pos, w| with_fx(TextEffectKind::Stroke(FxStroke { width: w, position: pos, paint: red.clone() }));
        let out = bbox(&render(&mk(StrokePosition::Outside, 8.0))).unwrap();
        assert!(out[0] + 7 <= plain[0] && out[2] >= plain[2] + 7, "fora: cresce 8 px {plain:?} -> {out:?}");
        let cen = bbox(&render(&mk(StrokePosition::Center, 8.0))).unwrap();
        assert!(cen[0] + 3 <= plain[0] && cen[0] + 5 >= plain[0], "centro: cresce 4 px {plain:?} -> {cen:?}");
        let ins = render(&mk(StrokePosition::Inside, 6.0));
        assert_eq!(bbox(&ins).unwrap(), plain, "dentro: a caixa é a mesma");
        assert!(ins.data.iter().any(|q| q[3] > 0.99 && unpremul(*q)[0] > 0.9), "dentro: tem vermelho");
        // contorno duplo: o primeiro (fino, branco) por cima do segundo (grosso, azul)
        let double = TextParams {
            effects: vec![
                TextEffect::new(TextEffectKind::Stroke(FxStroke { width: 5.0, position: StrokePosition::Outside, paint: Paint::solid([1.0, 1.0, 1.0, 1.0]) })),
                TextEffect::new(TextEffectKind::Stroke(FxStroke { width: 14.0, position: StrokePosition::Outside, paint: Paint::solid([0.0, 0.0, 1.0, 1.0]) })),
            ],
            ..base()
        };
        let img = render(&double);
        let db = bbox(&img).unwrap();
        assert!(db[0] + 13 <= plain[0], "o azul vai até 14 px: {db:?}");
        let y = (plain[1] + plain[3]) / 2;
        let row: Vec<[f32; 3]> = (db[0]..plain[0] + 2).map(|x| unpremul(img.get(x, y))).collect();
        let white = row.iter().any(|c| c[0] > 0.9 && c[1] > 0.9 && c[2] > 0.9);
        let blue = row.iter().any(|c| c[2] > 0.9 && c[0] < 0.1);
        assert!(white && blue, "branco e azul na borda esquerda: {row:?}");
    }

    #[test]
    fn drop_shadow_spread_noise_long_shadow_and_bevel_show_up() {
        let plain = bbox(&render(&base())).unwrap();
        let ds = bbox(&render(&with_fx(TextEffectKind::DropShadow(FxShadow { offset: [15.0, 10.0], blur: 0.0, ..FxShadow::default() })))).unwrap();
        assert!(ds[2] >= plain[2] + 14 && ds[3] >= plain[3] + 9 && ds[0] == plain[0], "sombra deslocada: {plain:?} -> {ds:?}");
        let sp = bbox(&render(&with_fx(TextEffectKind::DropShadow(FxShadow { offset: [0.0, 0.0], blur: 0.0, spread: 6.0, ..FxShadow::default() })))).unwrap();
        assert!(sp[0] + 5 <= plain[0], "espalhar engrossa: {sp:?}");
        let noisy = render(&with_fx(TextEffectKind::DropShadow(FxShadow { offset: [0.0, 30.0], blur: 0.0, noise: 1.0, ..FxShadow::default() })));
        let below: Vec<f32> = (plain[0]..plain[2]).map(|x| noisy.get(x, plain[3] + 20)[3]).filter(|a| *a > 0.0).collect();
        let (mn, mx) = (below.iter().cloned().fold(1.0, f32::min), below.iter().cloned().fold(0.0, f32::max));
        assert!(mx - mn > 0.2, "ruído varia o alfa: {mn}..{mx}");
        let ls = bbox(&render(&with_fx(TextEffectKind::LongShadow(FxLongShadow { angle: 45.0, length: 30.0, ..FxLongShadow::default() })))).unwrap();
        assert!(ls[2] >= plain[2] + 18 && ls[3] >= plain[3] + 18 && ls[0] == plain[0] && ls[1] == plain[1], "sombra longa para baixo-direita: {plain:?} -> {ls:?}");
        // chanfro: realce e sombra dentro das letras
        let grey = TextParams { fill: Some(Paint::solid([0.5, 0.5, 0.5, 1.0])), ..with_fx(TextEffectKind::BevelEmboss(FxBevel { size: 8.0, depth: 2.0, ..FxBevel::default() })) };
        let img = render(&grey);
        let vals: Vec<f32> = img.data.iter().filter(|q| q[3] > 0.99).map(|q| q[0]).collect();
        assert!(vals.iter().any(|v| *v > 0.65) && vals.iter().any(|v| *v < 0.35), "realce e sombra");
        // modo de mistura do efeito: multiplicar uma sobreposição escurece em vez de trocar a cor
        let mut mul = with_fx(TextEffectKind::ColorOverlay(FxColorOverlay { color: aiai_core::ColorRef::Direct { rgba: aiai_core::Rgba([0.5, 0.5, 0.5, 1.0]) } }));
        mul.fill = Some(Paint::solid([1.0, 0.0, 0.0, 1.0]));
        mul.effects[0].blend = BlendMode::Multiply;
        let m = render(&mul);
        assert!(m.data.iter().any(|q| q[3] > 0.99 && (unpremul(*q)[0] - 0.5).abs() < 0.02 && unpremul(*q)[1] < 0.02), "vermelho x cinza = vermelho escuro");
    }

    #[test]
    fn warped_text_renders_where_the_layout_says() {
        let p = TextParams { content: "ANIME".into(), warp: Some(TextWarp { style: WarpStyle::Arc, bend: 0.6, ..Default::default() }), ..base() };
        let img = render_text_layer(&p, &layout(&p), &colors(), Affine::IDENTITY, 900, 500).unwrap();
        let b = bbox(&img).unwrap();
        let cb = aiai_paint::text_layer::canvas_bounds(&p, &layout(&p));
        assert!(b[1] as f32 >= cb[1] - 2.0 && b[3] as f32 <= cb[3] + 2.0, "pixels {b:?} dentro de {cb:?}");
        let flat = TextParams { warp: None, ..p.clone() };
        let fb = bbox(&render_text_layer(&flat, &layout(&flat), &colors(), Affine::IDENTITY, 900, 500).unwrap()).unwrap();
        assert!(b[3] - b[1] > fb[3] - fb[1] + 10, "arco: mais alto {fb:?} -> {b:?}");
    }

    struct Checker;
    impl crate::text_fx::PaintResolver for Checker {
        fn color(&mut self, c: &aiai_core::ColorRef) -> [f32; 4] {
            match c {
                aiai_core::ColorRef::Direct { rgba } => rgba.0,
                _ => [0.0, 1.0, 0.0, 1.0],
            }
        }
        fn image(&mut self, _h: &aiai_core::BlobHash) -> Option<std::sync::Arc<Image>> {
            let mut img = Image::new(8, 8);
            for y in 0..8 {
                for x in 0..8 {
                    let i = img.idx(x, y);
                    img.data[i] = if (x / 4 + y / 4) % 2 == 0 { [1.0, 1.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] };
                }
            }
            Some(std::sync::Arc::new(img))
        }
    }

    #[test]
    fn pattern_fill_and_palette_stops_use_the_resolver() {
        let p = TextParams { fill: Some(Paint::Pattern { image: aiai_core::BlobHash("x".into()), scale: 1.0, offset: [0.0, 0.0], angle: 0.0 }), ..base() };
        let img = render_text_layer_with(&p, &layout(&p), &colors(), &mut Checker, Affine::IDENTITY, 420, 260).unwrap();
        let yellow = img.data.iter().filter(|q| q[3] > 0.99 && q[0] > 0.9 && q[2] < 0.1).count();
        let blue = img.data.iter().filter(|q| q[3] > 0.99 && q[2] > 0.9 && q[0] < 0.1).count();
        assert!(yellow > 100 && blue > 100, "xadrez dentro das letras: {yellow} amarelos, {blue} azuis");
        let pal = aiai_core::ColorRef::Palette { palette: aiai_core::Id::from("p"), color: aiai_core::Id::from("c") };
        let stops = vec![aiai_core::GradientStop { pos: 0.0, color: pal.clone(), opacity: 1.0 }, aiai_core::GradientStop { pos: 1.0, color: pal, opacity: 1.0 }];
        let g = TextParams { fill: Some(Paint::Gradient { gradient: Gradient { stops, ..Gradient::default() } }), ..base() };
        let gi = render_text_layer_with(&g, &layout(&g), &colors(), &mut Checker, Affine::IDENTITY, 420, 260).unwrap();
        assert!(gi.data.iter().any(|q| q[3] > 0.99 && q[1] > 0.99 && q[0] < 0.01), "paleta resolvida (verde)");
    }

    #[test]
    fn strokes_leave_the_gaps_between_letters_clean() {
        // regressão: o ruído da varredura entre as letras virava listras horizontais no contorno
        let p = TextParams { content: "AKAI HANA".into(), size: 150.0, bold: true, pos: [960.0, 180.0], align: aiai_paint::text_layer::TextAlign::Center, effects: vec![TextEffect::new(TextEffectKind::Stroke(FxStroke { width: 4.0, ..FxStroke::default() }))], ..Default::default() };
        let lay = layout(&p);
        let img = render_text_layer(&p, &lay, &colors(), Affine::IDENTITY, 1920, 1080).unwrap();
        let plain = TextParams { effects: vec![], ..p.clone() };
        let pimg = render_text_layer(&plain, &layout(&plain), &colors(), Affine::IDENTITY, 1920, 1080).unwrap();
        let w = img.width as usize;
        // todo pixel pintado pelo contorno está a até ~5 px de um pixel do texto liso
        let near = |x: usize, y: usize| (y.saturating_sub(6)..(y + 7).min(img.height as usize)).any(|yy| (x.saturating_sub(6)..(x + 7).min(w)).any(|xx| pimg.data[yy * w + xx][3] > 0.0));
        let mut stray = 0;
        for y in 0..img.height as usize {
            for x in 0..w {
                if img.data[y * w + x][3] > 0.05 && !near(x, y) {
                    stray += 1;
                }
            }
        }
        assert_eq!(stray, 0, "pixels de contorno longe das letras");
    }
}
