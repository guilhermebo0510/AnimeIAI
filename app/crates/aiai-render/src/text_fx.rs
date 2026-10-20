//! Estilos de camada do texto (como os do Photoshop), pintados ao vivo no render a partir da cobertura das letras e do
//! campo de distância com sinal dela (distância euclidiana exata, Felzenszwalb), já no espaço de saída (câmera/zoom):
//!
//! de baixo para cima: sombra legada → sombras projetadas → sombras longas/extrusão → brilhos externos → contornos de
//! fora (o primeiro da lista por cima) → contorno legado → preenchimento (cor, degradê ou padrão) → sobreposições de
//! cor/degradê/padrão → brilhos internos → sombras internas → cetim → chanfro e relevo → contornos de dentro e do centro.
//!
//! Cada efeito tem liga/desliga, opacidade e modo de mistura (a mistura é com o que já está pintado NA camada de texto;
//! a camada inteira depois se mistura com as de baixo pelo modo dela). O chanfro usa tela no realce e multiplicar na
//! sombra (como o padrão do Photoshop), com a opacidade do efeito.
//!
//! Degradês e padrões ficam presos ao texto: o degradê é avaliado no espaço local do texto (gira com ele) sobre a
//! caixa das letras (com a distorção); o padrão, no canvas.

use aiai_core::{BlendMode, BlobHash, ColorRef, Paint, ResolvedGradient};
use aiai_paint::blend::blend_pixel;
use aiai_paint::poly::fill_polygons;
use aiai_paint::text_layer::*;
use aiai_paint::{Affine, Image, StrokeSample, rasterize_stroke};
use std::sync::Arc;

/// Quem resolve cores de paleta e imagens de padrão (o compositor, com o projeto e os blobs).
pub trait PaintResolver {
    fn color(&mut self, c: &ColorRef) -> [f32; 4];
    fn image(&mut self, h: &BlobHash) -> Option<Arc<Image>>;
}

/// Resolver sem projeto: cores diretas; paleta = magenta; sem imagens.
pub struct DirectResolver;

impl PaintResolver for DirectResolver {
    fn color(&mut self, c: &ColorRef) -> [f32; 4] {
        match c {
            ColorRef::Direct { rgba } => rgba.0,
            ColorRef::Palette { .. } => [1.0, 0.0, 1.0, 1.0],
        }
    }
    fn image(&mut self, _h: &BlobHash) -> Option<Arc<Image>> {
        None
    }
}

/// Janela de trabalho: `w`×`h` px de saída a partir de (`x0`, `y0`).
struct Win {
    x0: i32,
    y0: i32,
    w: usize,
    h: usize,
}

impl Win {
    fn n(&self) -> usize {
        self.w * self.h
    }
}

/// Tinta pronta para amostrar por pixel da janela.
enum Src {
    Solid([f32; 4]),
    Grad(ResolvedGradient, aiai_core::paint::GradientMapper),
    Pattern(Arc<Image>, f32, [f32; 2], f32),
}

/// Pixel da janela → espaço local do texto (para degradês) e → canvas (para padrões).
struct Frame {
    out_to_canvas: Option<Affine>,
    p: TextParams,
    win_x0: f32,
    win_y0: f32,
}

impl Frame {
    fn canvas(&self, x: usize, y: usize) -> [f32; 2] {
        let (ox, oy) = (self.win_x0 + x as f32 + 0.5, self.win_y0 + y as f32 + 0.5);
        match &self.out_to_canvas {
            Some(a) => {
                let (cx, cy) = a.apply(ox, oy);
                [cx, cy]
            }
            None => [ox, oy],
        }
    }
    fn local(&self, x: usize, y: usize) -> [f32; 2] {
        self.p.canvas_to_local(self.canvas(x, y))
    }
}

fn src_of(paint: &Paint, res: &mut dyn PaintResolver, local_box: [f32; 4]) -> Src {
    match paint {
        Paint::Solid { color } => Src::Solid(res.color(color)),
        Paint::Gradient { gradient } => {
            let g = gradient.resolve(|c| res.color(c));
            let m = g.mapper(local_box);
            Src::Grad(g, m)
        }
        Paint::Pattern { image, scale, offset, angle } => match res.image(image) {
            Some(img) if img.width > 0 && img.height > 0 => Src::Pattern(img, scale.max(0.01), *offset, *angle),
            _ => Src::Solid([1.0, 0.0, 1.0, 1.0]),
        },
    }
}

/// Cor não pré-multiplicada da tinta no pixel (x, y) da janela.
fn sample(src: &Src, fr: &Frame, x: usize, y: usize) -> [f32; 4] {
    match src {
        Src::Solid(c) => *c,
        Src::Grad(g, m) => g.sample(m, fr.local(x, y)),
        Src::Pattern(img, scale, off, angle) => {
            let c = fr.canvas(x, y);
            let (s, co) = (-angle.to_radians()).sin_cos();
            let (dx, dy) = ((c[0] - off[0]) / scale, (c[1] - off[1]) / scale);
            let (u, v) = (dx * co - dy * s, dx * s + dy * co);
            let (iw, ih) = (img.width as i64, img.height as i64);
            let (ix, iy) = ((u.floor() as i64).rem_euclid(iw), (v.floor() as i64).rem_euclid(ih));
            let px = img.get(ix as u32, iy as u32);
            if px[3] <= 0.0 { [0.0; 4] } else { [px[0] / px[3], px[1] / px[3], px[2] / px[3], px[3]] }
        }
    }
}

/// A tinta como cor por pixel da janela (não pré-multiplicada); tinta chapada não aloca.
fn paint_field(src: &Src, fr: &Frame, win: &Win) -> Option<Vec<[f32; 4]>> {
    match src {
        Src::Solid(_) => None,
        _ => {
            let mut v = Vec::with_capacity(win.n());
            for y in 0..win.h {
                for x in 0..win.w {
                    v.push(sample(src, fr, x, y));
                }
            }
            Some(v)
        }
    }
}

/// Compõe `cov` × tinta na imagem da janela com o modo e a opacidade do efeito.
fn composite(dst: &mut Image, cov: &[f32], src: &Src, fr: &Frame, win: &Win, mode: BlendMode, opacity: f32) {
    let field = paint_field(src, fr, win);
    for i in 0..win.n() {
        let a = cov[i];
        if a <= 0.0 {
            continue;
        }
        let c = match (&field, src) {
            (Some(f), _) => f[i],
            (None, Src::Solid(c)) => *c,
            _ => [0.0; 4],
        };
        let al = (c[3] * a).clamp(0.0, 1.0);
        if al <= 0.0 {
            continue;
        }
        blend_pixel(&mut dst.data[i], [c[0] * al, c[1] * al, c[2] * al, al], mode, opacity);
    }
}

/// Compõe com cor por pixel já calculada (pré-multiplicada).
fn composite_premul(dst: &mut Image, layer: &[[f32; 4]], mode: BlendMode, opacity: f32) {
    for (d, s) in dst.data.iter_mut().zip(layer) {
        if s[3] > 0.0 {
            blend_pixel(d, *s, mode, opacity);
        }
    }
}

// ------------------------------------------------------------------------------------------------ campo de distância

const INF: f32 = 1e20;

/// Distância euclidiana ao quadrado 1D (Felzenszwalb & Huttenlocher).
fn edt_1d(f: &[f32], d: &mut [f32], v: &mut [usize], z: &mut [f32]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut k = 0usize;
    v[0] = 0;
    z[0] = -INF;
    z[1] = INF;
    for q in 1..n {
        loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }
            if s <= z[k] {
                // k == 0 e a parábola nova domina desde o começo
                v[0] = q;
                z[0] = -INF;
                z[1] = INF;
                break;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = INF;
            break;
        }
    }
    k = 0;
    for (q, dq) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        *dq = (q as f32 - p as f32).powi(2) + f[p];
    }
}

/// Distância ao quadrado de cada pixel ao pixel `true` mais próximo.
fn edt(mask: &[bool], w: usize, h: usize) -> Vec<f32> {
    let mut g: Vec<f32> = mask.iter().map(|&m| if m { 0.0 } else { INF }).collect();
    let m = w.max(h);
    let (mut f, mut d, mut v, mut z) = (vec![0.0f32; m], vec![0.0f32; m], vec![0usize; m], vec![0.0f32; m + 1]);
    for x in 0..w {
        for y in 0..h {
            f[y] = g[y * w + x];
        }
        edt_1d(&f[..h], &mut d[..h], &mut v[..h], &mut z[..h + 1]);
        for y in 0..h {
            g[y * w + x] = d[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&g[y * w..y * w + w]);
        edt_1d(&f[..w], &mut d[..w], &mut v[..w], &mut z[..w + 1]);
        g[y * w..y * w + w].copy_from_slice(&d[..w]);
    }
    g
}

/// Distância com sinal à borda (px; > 0 fora, < 0 dentro), refinada pela cobertura nos pixels da borda.
pub(crate) fn signed_distance(a: &[f32], w: usize, h: usize) -> Vec<f32> {
    let inside: Vec<bool> = a.iter().map(|&v| v >= 0.5).collect();
    let outside: Vec<bool> = inside.iter().map(|&b| !b).collect();
    let d_in = edt(&inside, w, h);
    let d_out = edt(&outside, w, h);
    (0..a.len())
        .map(|i| {
            if a[i] > 0.0 && a[i] < 1.0 {
                0.5 - a[i]
            } else if inside[i] {
                -(d_out[i].sqrt() - 0.5)
            } else {
                d_in[i].sqrt() - 0.5
            }
        })
        .collect()
}

// ------------------------------------------------------------------------------------------------ utilidades de máscara

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return if x < e0 { 0.0 } else { 1.0 };
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Amostra bilinear da máscara (fora = `outside`).
fn bil(m: &[f32], w: usize, h: usize, x: f32, y: f32, outside: f32) -> f32 {
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let get = |xi: i64, yi: i64| if xi < 0 || yi < 0 || xi >= w as i64 || yi >= h as i64 { outside } else { m[yi as usize * w + xi as usize] };
    let (xi, yi) = (x0 as i64, y0 as i64);
    let a = get(xi, yi) * (1.0 - tx) + get(xi + 1, yi) * tx;
    let b = get(xi, yi + 1) * (1.0 - tx) + get(xi + 1, yi + 1) * tx;
    a * (1.0 - ty) + b * ty
}

/// Máscara deslocada por (dx, dy).
fn shifted(m: &[f32], w: usize, h: usize, dx: f32, dy: f32, outside: f32) -> Vec<f32> {
    let mut out = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = bil(m, w, h, x as f32 + 0.5 - dx, y as f32 + 0.5 - dy, outside);
        }
    }
    out
}

/// Desfoque gaussiano de uma máscara (sigma em px).
fn blur_mask(m: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    if sigma < 0.1 {
        return m.to_vec();
    }
    let mut img = Image::new(w as u32, h as u32);
    for (d, &v) in img.data.iter_mut().zip(m) {
        *d = [v, v, v, v];
    }
    let b = aiai_fx::blur::gaussian(&img, sigma);
    b.data.iter().map(|p| p[3]).collect()
}

/// Ruído determinístico 0..1 por pixel do canvas (o grão não "anda" com a janela).
fn hash01(x: i64, y: i64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

fn apply_noise(m: &mut [f32], win: &Win, noise: f32) {
    if noise <= 0.0 {
        return;
    }
    for y in 0..win.h {
        for x in 0..win.w {
            let r = hash01(win.x0 as i64 + x as i64, win.y0 as i64 + y as i64);
            m[y * win.w + x] *= 1.0 - noise * r;
        }
    }
}

// ------------------------------------------------------------------------------------------------ render

/// Margem (px de saída) que os estilos pedem além das letras.
pub(crate) fn styles_reach(p: &TextParams, k: f32) -> f32 {
    p.effects.iter().map(|e| e.reach()).fold(0.0, f32::max) * k
}

/// Pinta o texto com estilos na janela. `contours` já estão em px da janela; `glyph_box` = caixa das letras no espaço
/// local do texto (com a distorção), para mapear degradês. `legacy_stroke_r` / `bold_r` em px de saída.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_styles(
    p: &TextParams,
    contours: &[Vec<[f32; 2]>],
    colors: &crate::text::TextColors,
    res: &mut dyn PaintResolver,
    xf: Affine,
    k: f32,
    win_origin: [i32; 2],
    lw: u32,
    lh: u32,
    legacy_stroke_r: Option<f32>,
    bold_r: Option<f32>,
    glyph_box: [f32; 4],
) -> Image {
    let win = Win { x0: win_origin[0], y0: win_origin[1], w: lw as usize, h: lh as usize };
    let fr = Frame { out_to_canvas: if xf.is_identity() { None } else { xf.inverse() }, p: p.clone(), win_x0: win.x0 as f32, win_y0: win.y0 as f32 };
    let (w, h) = (win.w, win.h);
    let n = win.n();
    // ---- cobertura das letras (com o negrito sintético)
    let mut a = vec![0.0f32; n];
    let stroke_cov = |r: f32, out: &mut Vec<f32>| {
        for c in contours {
            if c.len() < 2 {
                continue;
            }
            let mut smp: Vec<StrokeSample> = c.iter().map(|q| StrokeSample { x: q[0], y: q[1], r }).collect();
            smp.push(smp[0]);
            if let Some(cov) = rasterize_stroke(&smp, false, lw, lh) {
                for yy in 0..cov.height as usize {
                    for xx in 0..cov.width as usize {
                        let (x, y) = (cov.x0 as usize + xx, cov.y0 as usize + yy);
                        let v = cov.cov[yy * cov.width as usize + xx];
                        let i = y * w + x;
                        out[i] = out[i].max(v);
                    }
                }
            }
        }
    };
    if let Some(cov) = fill_polygons(contours, false, lw, lh) {
        for yy in 0..cov.height as usize {
            for xx in 0..cov.width as usize {
                let (x, y) = (cov.x0 as usize + xx, cov.y0 as usize + yy);
                if x < w && y < h {
                    a[y * w + x] = cov.cov[yy * cov.width as usize + xx];
                }
            }
        }
    }
    if let Some(r) = bold_r {
        stroke_cov(r, &mut a);
    }
    // a varredura acumula ruído (1e-7) nas linhas entre as letras: sem limpar, o campo de distância lia esses pixels
    // como borda e os contornos viravam listras horizontais
    for v in a.iter_mut() {
        if *v < 1.0 / 512.0 {
            *v = 0.0;
        } else if *v > 1.0 - 1.0 / 512.0 {
            *v = 1.0;
        }
    }
    let needs_sd = legacy_stroke_r.is_some()
        || p.effects.iter().any(|e| {
            e.enabled
                && matches!(
                    &e.kind,
                    TextEffectKind::Stroke(_) | TextEffectKind::OuterGlow(_) | TextEffectKind::InnerGlow(_) | TextEffectKind::BevelEmboss(_) | TextEffectKind::InnerShadow(_)
                )
                || matches!(&e.kind, TextEffectKind::DropShadow(s) if e.enabled && s.spread > 0.0)
        });
    let sd = if needs_sd { signed_distance(&a, w, h) } else { Vec::new() };
    let mut img = Image::new(lw, lh);
    let fx_on = |e: &&TextEffect| e.enabled && e.opacity > 0.0;

    // ---- sombra legada (do contorno legado + letras)
    if let Some(s) = p.shadow.as_ref().filter(|_| colors.shadow[3] > 0.0) {
        let mut sil = a.clone();
        if let Some(r) = legacy_stroke_r {
            for i in 0..n {
                sil[i] = sil[i].max((r + 0.5 - sd[i]).clamp(0.0, 1.0));
            }
        }
        let (dx, dy) = ((s.offset[0] * k).round(), (s.offset[1] * k).round());
        let mut m = shifted(&sil, w, h, dx, dy, 0.0);
        m = blur_mask(&m, w, h, s.blur * k / 2.0);
        composite(&mut img, &m, &Src::Solid(colors.shadow), &fr, &win, BlendMode::Normal, 1.0);
    }
    // ---- sombras projetadas
    for e in p.effects.iter().filter(fx_on) {
        if let TextEffectKind::DropShadow(s) = &e.kind {
            let base: Vec<f32> = if s.spread > 0.0 { (0..n).map(|i| (s.spread * k + 0.5 - sd[i]).clamp(0.0, 1.0)).collect() } else { a.clone() };
            let mut m = shifted(&base, w, h, s.offset[0] * k, s.offset[1] * k, 0.0);
            m = blur_mask(&m, w, h, s.blur * k / 2.0);
            apply_noise(&mut m, &win, s.noise);
            composite(&mut img, &m, &Src::Solid(res.color(&s.color)), &fr, &win, e.blend, e.opacity);
        }
    }
    // ---- sombra longa / extrusão
    for e in p.effects.iter().filter(fx_on) {
        if let TextEffectKind::LongShadow(l) = &e.kind {
            let len = l.length * k;
            if len < 0.5 {
                continue;
            }
            let steps = (len.ceil() as usize).clamp(1, 400);
            let (sn, cs) = l.angle.to_radians().sin_cos();
            let src = src_of(&l.paint, res, [0.0, 0.0, 1.0, 1.0]);
            let mut layer = vec![[0.0f32; 4]; n];
            // de longe para perto: as cópias mais próximas cobrem as distantes
            for st in (1..=steps).rev() {
                let d = len * st as f32 / steps as f32;
                let t = st as f32 / steps as f32;
                let c = match &src {
                    Src::Solid(c) => *c,
                    Src::Grad(g, _) => g.color_at(t),
                    Src::Pattern(..) => [0.5, 0.5, 0.5, 1.0],
                };
                let fade = if l.fade { 1.0 - 0.95 * t } else { 1.0 };
                let (dx, dy) = (cs * d, sn * d);
                for y in 0..h {
                    for x in 0..w {
                        let m = bil(&a, w, h, x as f32 + 0.5 - dx, y as f32 + 0.5 - dy, 0.0);
                        if m <= 0.0 {
                            continue;
                        }
                        let al = (m * c[3] * fade).clamp(0.0, 1.0);
                        Image::over(&mut layer[y * w + x], [c[0] * al, c[1] * al, c[2] * al, al], 1.0);
                    }
                }
            }
            composite_premul(&mut img, &layer, e.blend, e.opacity);
        }
    }
    // ---- brilhos externos
    for e in p.effects.iter().filter(fx_on) {
        if let TextEffectKind::OuterGlow(g) = &e.kind {
            let size = (g.size * k).max(0.5);
            let solid_part = g.spread * size;
            let src = src_of(&g.paint, res, [0.0, 0.0, 1.0, 1.0]);
            let mut layer = vec![[0.0f32; 4]; n];
            for i in 0..n {
                let d = sd[i];
                let m = if d <= solid_part { 1.0 } else { 1.0 - smoothstep(solid_part, size, d) };
                if m <= 0.0 {
                    continue;
                }
                let c = match &src {
                    Src::Grad(gr, _) => gr.color_at((d.max(0.0) / size).clamp(0.0, 1.0)),
                    Src::Solid(c) => *c,
                    Src::Pattern(..) => sample(&src, &fr, i % w, i / w),
                };
                let mut al = m * c[3];
                if g.noise > 0.0 {
                    al *= 1.0 - g.noise * hash01(win.x0 as i64 + (i % w) as i64, win.y0 as i64 + (i / w) as i64);
                }
                layer[i] = [c[0] * al, c[1] * al, c[2] * al, al];
            }
            composite_premul(&mut img, &layer, e.blend, e.opacity);
        }
    }
    // ---- contornos de fora (o primeiro da lista por cima: pinta do último para o primeiro)
    let strokes: Vec<&TextEffect> = p.effects.iter().filter(fx_on).filter(|e| matches!(&e.kind, TextEffectKind::Stroke(_))).collect();
    for e in strokes.iter().rev() {
        let TextEffectKind::Stroke(s) = &e.kind else { continue };
        if s.position != StrokePosition::Outside {
            continue;
        }
        let r = s.width * k;
        let cov: Vec<f32> = (0..n).map(|i| (r + 0.5 - sd[i]).clamp(0.0, 1.0)).collect();
        let src = src_of(&s.paint, res, glyph_box);
        composite(&mut img, &cov, &src, &fr, &win, e.blend, e.opacity);
    }
    // ---- contorno legado (por baixo do preenchimento)
    if let (Some(r), Some(st)) = (legacy_stroke_r, p.stroke.as_ref()) {
        let cov: Vec<f32> = (0..n).map(|i| (r + 0.5 - sd[i]).clamp(0.0, 1.0)).collect();
        let src = match &st.paint {
            Some(pt) => src_of(pt, res, glyph_box),
            None => Src::Solid(colors.stroke),
        };
        composite(&mut img, &cov, &src, &fr, &win, BlendMode::Normal, 1.0);
    }
    // ---- preenchimento
    let fill_src = match &p.fill {
        Some(pt) => src_of(pt, res, glyph_box),
        None => Src::Solid(colors.fill),
    };
    composite(&mut img, &a, &fill_src, &fr, &win, BlendMode::Normal, 1.0);
    // ---- sobreposições (dentro das letras)
    for e in p.effects.iter().filter(fx_on) {
        let paint = match &e.kind {
            TextEffectKind::ColorOverlay(c) => Paint::Solid { color: c.color.clone() },
            TextEffectKind::GradientOverlay(g) => Paint::Gradient { gradient: g.gradient.clone() },
            TextEffectKind::PatternOverlay(pt) => pt.paint.clone(),
            _ => continue,
        };
        let src = src_of(&paint, res, glyph_box);
        composite(&mut img, &a, &src, &fr, &win, e.blend, e.opacity);
    }
    // ---- brilhos internos
    for e in p.effects.iter().filter(fx_on) {
        if let TextEffectKind::InnerGlow(g) = &e.kind {
            let size = (g.size * k).max(0.5);
            let solid_part = g.spread * size;
            let src = src_of(&g.paint, res, [0.0, 0.0, 1.0, 1.0]);
            let mut layer = vec![[0.0f32; 4]; n];
            for i in 0..n {
                if a[i] <= 0.0 {
                    continue;
                }
                let di = (-sd[i]).max(0.0);
                let (m, t) = if g.from_center {
                    let v = smoothstep(0.0, size, di);
                    (v, 1.0 - v)
                } else {
                    let v = if di <= solid_part { 1.0 } else { 1.0 - smoothstep(solid_part, size, di) };
                    (v, (di / size).clamp(0.0, 1.0))
                };
                if m <= 0.0 {
                    continue;
                }
                let c = match &src {
                    Src::Grad(gr, _) => gr.color_at(t),
                    Src::Solid(c) => *c,
                    Src::Pattern(..) => sample(&src, &fr, i % w, i / w),
                };
                let mut al = m * c[3] * a[i];
                if g.noise > 0.0 {
                    al *= 1.0 - g.noise * hash01(win.x0 as i64 + (i % w) as i64, win.y0 as i64 + (i / w) as i64);
                }
                layer[i] = [c[0] * al, c[1] * al, c[2] * al, al];
            }
            composite_premul(&mut img, &layer, e.blend, e.opacity);
        }
    }
    // ---- sombras internas: o avesso da forma, deslocado e desfocado, só dentro das letras
    for e in p.effects.iter().filter(fx_on) {
        if let TextEffectKind::InnerShadow(s) = &e.kind {
            let choke = s.spread * k;
            let inv: Vec<f32> = (0..n).map(|i| (sd[i] + 0.5 + choke).clamp(0.0, 1.0)).collect();
            let mut m = shifted(&inv, w, h, s.offset[0] * k, s.offset[1] * k, 1.0);
            m = blur_mask(&m, w, h, s.blur * k / 2.0);
            apply_noise(&mut m, &win, s.noise);
            for i in 0..n {
                m[i] *= a[i];
            }
            composite(&mut img, &m, &Src::Solid(res.color(&s.color)), &fr, &win, e.blend, e.opacity);
        }
    }
    // ---- cetim
    for e in p.effects.iter().filter(fx_on) {
        if let TextEffectKind::Satin(s) = &e.kind {
            let (sn, cs) = s.angle.to_radians().sin_cos();
            let d = s.distance * k;
            let m1 = blur_mask(&shifted(&a, w, h, cs * d, sn * d, 0.0), w, h, s.size * k / 2.0);
            let m2 = blur_mask(&shifted(&a, w, h, -cs * d, -sn * d, 0.0), w, h, s.size * k / 2.0);
            let m: Vec<f32> = (0..n)
                .map(|i| {
                    let v = (m1[i] - m2[i]).abs().clamp(0.0, 1.0);
                    (if s.invert { 1.0 - v } else { v }) * a[i]
                })
                .collect();
            composite(&mut img, &m, &Src::Solid(res.color(&s.color)), &fr, &win, e.blend, e.opacity);
        }
    }
    // ---- chanfro e relevo
    for e in p.effects.iter().filter(fx_on) {
        if let TextEffectKind::BevelEmboss(b) = &e.kind {
            bevel(&mut img, b, &a, &sd, &win, k, res, e.opacity);
        }
    }
    // ---- contornos de dentro e do centro (por cima)
    for e in strokes.iter().rev() {
        let TextEffectKind::Stroke(s) = &e.kind else { continue };
        let r = s.width * k;
        let cov: Vec<f32> = match s.position {
            StrokePosition::Outside => continue,
            StrokePosition::Inside => (0..n).map(|i| a[i] * (r + 0.5 + sd[i]).clamp(0.0, 1.0)).collect(),
            StrokePosition::Center => (0..n).map(|i| (r / 2.0 + 0.5 - sd[i].abs()).clamp(0.0, 1.0)).collect(),
        };
        let src = src_of(&s.paint, res, glyph_box);
        composite(&mut img, &cov, &src, &fr, &win, e.blend, e.opacity);
    }
    img
}

/// Chanfro: altura pela distância à borda, normal pelo gradiente da altura, luz direcional.
#[allow(clippy::too_many_arguments)]
fn bevel(img: &mut Image, b: &FxBevel, a: &[f32], sd: &[f32], win: &Win, k: f32, res: &mut dyn PaintResolver, opacity: f32) {
    let (w, h) = (win.w, win.h);
    let n = win.n();
    let size = (b.size * k).max(0.5);
    let mut hgt: Vec<f32> = (0..n)
        .map(|i| {
            let d = sd[i];
            let v = match b.style {
                BevelStyle::Inner => smoothstep(0.0, size, -d),
                BevelStyle::Outer => 1.0 - smoothstep(0.0, size, d),
                BevelStyle::Emboss => 1.0 - smoothstep(-size, size, d),
                BevelStyle::Pillow => 1.0 - smoothstep(0.0, size, d.abs()),
            };
            if b.down { -v } else { v }
        })
        .collect();
    if b.soften > 0.0 {
        hgt = blur_mask(&hgt.iter().map(|v| v * 0.5 + 0.5).collect::<Vec<_>>(), w, h, b.soften * k / 2.0).iter().map(|v| v * 2.0 - 1.0).collect();
    }
    let z = b.depth * size;
    let (alt_s, alt_c) = b.altitude.to_radians().sin_cos();
    let (an_s, an_c) = b.angle.to_radians().sin_cos();
    let light = [an_c * alt_c, an_s * alt_c, alt_s];
    let hi_c = res.color(&b.highlight);
    let sh_c = res.color(&b.shadow);
    let at = |x: i64, y: i64| hgt[(y.clamp(0, h as i64 - 1) as usize) * w + x.clamp(0, w as i64 - 1) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            // região: dentro (interno), fora (externo) ou perto da borda (relevo/almofada)
            let region = match b.style {
                BevelStyle::Inner => a[i],
                BevelStyle::Outer => (1.0 - a[i]) * (sd[i] < size + 1.0) as u8 as f32,
                BevelStyle::Emboss | BevelStyle::Pillow => (sd[i].abs() < size + 1.0) as u8 as f32,
            };
            if region <= 0.0 {
                continue;
            }
            let (xi, yi) = (x as i64, y as i64);
            let gx = (at(xi + 1, yi) - at(xi - 1, yi)) * 0.5;
            let gy = (at(xi, yi + 1) - at(xi, yi - 1)) * 0.5;
            // normal da superfície (y para baixo); luz vem DE `angle`
            let nv = [-gx * z, -gy * z, 1.0];
            let len = (nv[0] * nv[0] + nv[1] * nv[1] + 1.0).sqrt();
            let shade = (nv[0] * light[0] + nv[1] * light[1] + nv[2] * light[2]) / len;
            let diff = shade - alt_s;
            if diff > 0.0 {
                let al = (diff * 2.5).clamp(0.0, 1.0) * b.highlight_opacity * hi_c[3] * region;
                blend_pixel(&mut img.data[i], [hi_c[0] * al, hi_c[1] * al, hi_c[2] * al, al], BlendMode::Screen, opacity);
            } else if diff < 0.0 {
                let al = (-diff * 2.5).clamp(0.0, 1.0) * b.shadow_opacity * sh_c[3] * region;
                blend_pixel(&mut img.data[i], [sh_c[0] * al, sh_c[1] * al, sh_c[2] * al, al], BlendMode::Multiply, opacity);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_distance_is_euclidean_and_signed() {
        // quadrado 10×10 no meio de 40×40
        let (w, h) = (40usize, 40usize);
        let a: Vec<f32> = (0..w * h).map(|i| if (15..25).contains(&(i % w)) && (15..25).contains(&(i / w)) { 1.0 } else { 0.0 }).collect();
        let sd = signed_distance(&a, w, h);
        assert!(sd[20 * w + 20] < -3.0, "dentro é negativo");
        assert!((sd[20 * w + 30] - 5.5).abs() < 0.01, "6 px à direita da borda: {}", sd[20 * w + 30]);
        // na diagonal a distância é euclidiana
        let diag = sd[(24 + 4) * w + (24 + 3)];
        assert!((diag - (5.0f32 - 0.5)).abs() < 0.01, "3-4-5: {diag}");
    }

    #[test]
    fn edt_matches_brute_force_on_sparse_masks() {
        let (w, h) = (37usize, 23usize);
        let mut seed = 12345u64;
        for round in 0..40 {
            let density = [0.01, 0.05, 0.2, 0.6][round % 4];
            let mask: Vec<bool> = (0..w * h)
                .map(|_| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    ((seed >> 33) as f64 / (1u64 << 31) as f64) < density
                })
                .collect();
            if !mask.iter().any(|m| *m) {
                continue;
            }
            let d = edt(&mask, w, h);
            for y in 0..h {
                for x in 0..w {
                    let mut best = f32::MAX;
                    for yy in 0..h {
                        for xx in 0..w {
                            if mask[yy * w + xx] {
                                best = best.min(((x as f32 - xx as f32).powi(2) + (y as f32 - yy as f32).powi(2)) as f32);
                            }
                        }
                    }
                    assert!((d[y * w + x] - best).abs() < 1e-3, "round {round} ({x},{y}): {} vs {best}", d[y * w + x]);
                }
            }
        }
    }
}
