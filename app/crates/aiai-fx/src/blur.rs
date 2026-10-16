//! Desfoques. Todos trabalham em alfa pré-multiplicado (sem franja escura) e com borda repetida.
//!
//! - Gaussiano separável exato: σ = raio/3, núcleo truncado em 3σ (raio inteiro = ceil(raio)).
//!   Para raio > 30 (σ > 10) usa-se a aproximação por 3 desfoques de caixa (método "boxes for gauss": larguras
//!   mistas wl/wu escolhidas para igualar a variância), O(1) por pixel e conserva a massa exatamente; o erro máximo
//!   contra a gaussiana exata fica em poucos por cento do pico.
//! - `blur_fast` (usado por glow, difusão, halação...): reduz a imagem por 2 até que σ ≤ 8 no nível reduzido, borra e
//!   amplia com bilinear — para brilhos suaves, não para desfoque de imagem final.
//! - Direcional e radial: integral de linha com taps bilineares a ~1 px de passo; acima de 24 amostras o pente de N
//!   amostras é fatorado em dois passes n1×n2 (custo ≈ 2√N em vez de N).
//! - Consciente de cor: bilateral conjunto separável (passe horizontal + vertical), peso = gaussiana espacial ×
//!   [diferença de cor ≤ tolerância]; regiões chapadas de cel não escorrem uma para a outra.

use crate::util::*;
use aiai_paint::Image;
use rayon::prelude::*;

/// Raio máximo (px) a partir do qual se usa a aproximação por caixas.
pub const EXACT_MAX_RADIUS: f32 = 30.0;

fn half_kernel(sigma: f32) -> Vec<f32> {
    let r = ((3.0 * sigma).ceil() as usize).max(1);
    let mut k: Vec<f64> = (0..=r).map(|i| (-(i as f64 * i as f64) / (2.0 * sigma as f64 * sigma as f64)).exp()).collect();
    let sum: f64 = k[0] + 2.0 * k[1..].iter().sum::<f64>();
    for v in &mut k {
        *v /= sum;
    }
    k.into_iter().map(|v| v as f32).collect()
}

fn conv_h(src: &[Px], w: usize, h: usize, half: &[f32]) -> Vec<Px> {
    let mut out = vec![ZERO; w * h];
    let r = half.len() - 1;
    out.par_chunks_mut(w).zip(src.par_chunks(w)).for_each_init(Vec::<Px>::new, |pad, (orow, srow)| {
        pad.clear();
        pad.extend(std::iter::repeat_n(srow[0], r));
        pad.extend_from_slice(srow);
        pad.extend(std::iter::repeat_n(srow[w - 1], r));
        for x in 0..w {
            orow[x] = scale(pad[x + r], half[0]);
        }
        for j in 1..=r {
            let k = half[j];
            for x in 0..w {
                let a = pad[x + r - j];
                let b = pad[x + r + j];
                let o = &mut orow[x];
                o[0] += k * (a[0] + b[0]);
                o[1] += k * (a[1] + b[1]);
                o[2] += k * (a[2] + b[2]);
                o[3] += k * (a[3] + b[3]);
            }
        }
    });
    out
}

fn conv_v(src: &[Px], w: usize, h: usize, half: &[f32]) -> Vec<Px> {
    let mut out = vec![ZERO; w * h];
    let r = half.len() - 1;
    out.par_chunks_mut(w).enumerate().for_each(|(y, orow)| {
        let c = &src[y * w..(y + 1) * w];
        for x in 0..w {
            orow[x] = scale(c[x], half[0]);
        }
        for j in 1..=r {
            let k = half[j];
            let ya = y.saturating_sub(j);
            let yb = (y + j).min(h - 1);
            let a = &src[ya * w..(ya + 1) * w];
            let b = &src[yb * w..(yb + 1) * w];
            for x in 0..w {
                let o = &mut orow[x];
                o[0] += k * (a[x][0] + b[x][0]);
                o[1] += k * (a[x][1] + b[x][1]);
                o[2] += k * (a[x][2] + b[x][2]);
                o[3] += k * (a[x][3] + b[x][3]);
            }
        }
    });
    out
}

fn transpose(src: &[Px], w: usize, h: usize) -> Vec<Px> {
    let mut out = vec![ZERO; w * h];
    out.par_chunks_mut(h).enumerate().for_each(|(x, orow)| {
        for y in 0..h {
            orow[y] = src[y * w + x];
        }
    });
    out
}

fn box_h(src: &[Px], w: usize, h: usize, r: usize) -> Vec<Px> {
    if r == 0 {
        return src.to_vec();
    }
    let mut out = vec![ZERO; w * h];
    let inv = 1.0 / (2 * r + 1) as f64;
    out.par_chunks_mut(w).zip(src.par_chunks(w)).for_each_init(Vec::<Px>::new, |pad, (orow, srow)| {
        pad.clear();
        pad.extend(std::iter::repeat_n(srow[0], r));
        pad.extend_from_slice(srow);
        pad.extend(std::iter::repeat_n(srow[w - 1], r));
        let mut s = [0.0f64; 4];
        for p in &pad[..2 * r + 1] {
            for c in 0..4 {
                s[c] += p[c] as f64;
            }
        }
        for x in 0..w {
            orow[x] = [(s[0] * inv) as f32, (s[1] * inv) as f32, (s[2] * inv) as f32, (s[3] * inv) as f32];
            if x + 1 < w {
                let add = pad[x + 2 * r + 1];
                let sub = pad[x];
                for c in 0..4 {
                    s[c] += add[c] as f64 - sub[c] as f64;
                }
            }
        }
    });
    out
}

/// Larguras de caixa (raios inteiros) para 3 passes que aproximam uma gaussiana de desvio `sigma`.
fn box_radii(sigma: f32) -> [usize; 3] {
    let n = 3.0f32;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n) / (-4.0 * wl as f32 - 4.0)).round() as i32;
    let mut out = [0usize; 3];
    for (i, o) in out.iter_mut().enumerate() {
        let wd = if (i as i32) < m { wl } else { wu };
        *o = ((wd - 1) / 2) as usize;
    }
    out
}

fn gaussian_box(img: &Image, sigma: f32) -> Image {
    let (w, h) = (img.width as usize, img.height as usize);
    let radii = box_radii(sigma);
    let mut d = img.data.clone();
    for &r in &radii {
        d = box_h(&d, w, h, r);
    }
    let mut t = transpose(&d, w, h);
    for &r in &radii {
        t = box_h(&t, h, w, r);
    }
    Image { width: img.width, height: img.height, data: transpose(&t, h, w) }
}

fn gaussian_exact(img: &Image, sigma: f32) -> Image {
    let (w, h) = (img.width as usize, img.height as usize);
    let k = half_kernel(sigma);
    let d = conv_h(&img.data, w, h, &k);
    let d = conv_v(&d, w, h, &k);
    Image { width: img.width, height: img.height, data: d }
}

/// Gaussiana com σ dado; exata até σ = 10 (raio 30), caixa×3 acima.
pub fn gaussian(img: &Image, sigma: f32) -> Image {
    if is_empty(img) || sigma < 0.1 {
        return img.clone();
    }
    if sigma <= EXACT_MAX_RADIUS / 3.0 { gaussian_exact(img, sigma) } else { gaussian_box(img, sigma) }
}

/// Efeito `GaussianBlur { radius }`.
pub fn gaussian_blur(img: &mut Image, radius: f32) {
    if is_empty(img) {
        return;
    }
    let radius = radius.max(0.0).min(img.width.max(img.height) as f32);
    *img = gaussian(img, radius / 3.0);
}

/// Redução por 2 (caixa 2×2, borda repetida).
pub fn downsample2(img: &Image) -> Image {
    let (w, h) = (img.width as usize, img.height as usize);
    let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
    let mut out = Image::new(nw as u32, nh as u32);
    out.data.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
        let y0 = (2 * y).min(h - 1);
        let y1 = (2 * y + 1).min(h - 1);
        for (x, o) in row.iter_mut().enumerate() {
            let x0 = (2 * x).min(w - 1);
            let x1 = (2 * x + 1).min(w - 1);
            let (a, b, c, d) = (img.data[y0 * w + x0], img.data[y0 * w + x1], img.data[y1 * w + x0], img.data[y1 * w + x1]);
            for k in 0..4 {
                o[k] = 0.25 * (a[k] + b[k] + c[k] + d[k]);
            }
        }
    });
    out
}

/// Amplia por bilinear (centros de pixel alinhados) para `w`×`h`.
pub fn upsample_to(img: &Image, w: u32, h: u32) -> Image {
    let sx = img.width as f32 / w as f32;
    let sy = img.height as f32 / h as f32;
    generate(w, h, |x, y| sample(img, (x as f32 + 0.5) * sx, (y as f32 + 0.5) * sy))
}

/// Gaussiana rápida por pirâmide (ver documentação do módulo). Devolve imagem do mesmo tamanho.
pub fn blur_fast(src: &Image, sigma: f32) -> Image {
    if is_empty(src) || sigma < 0.1 {
        return src.clone();
    }
    let sigma = sigma.min(src.width.max(src.height) as f32);
    let mut s = sigma;
    let mut cur: Option<Image> = None;
    let mut levels = 0;
    while s > 8.0 && levels < 8 {
        let base = cur.as_ref().unwrap_or(src);
        if base.width <= 2 || base.height <= 2 {
            break;
        }
        cur = Some(downsample2(base));
        s *= 0.5;
        levels += 1;
    }
    match cur {
        None => gaussian_exact(src, s),
        Some(small) => {
            let blurred = gaussian_exact(&small, s);
            upsample_to(&blurred, src.width, src.height)
        }
    }
}

/// Média de `src` em posições deslocadas `(dx, dy) · t` para cada `t` de `offs` (taps bilineares).
fn gather_line(src: &Image, dx: f32, dy: f32, offs: &[f32]) -> Image {
    let inv = 1.0 / offs.len() as f32;
    generate(src.width, src.height, |x, y| {
        let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
        let mut acc = ZERO;
        for &t in offs {
            let s = sample(src, cx + dx * t, cy + dy * t);
            for k in 0..4 {
                acc[k] += s[k];
            }
        }
        scale(acc, inv)
    })
}

/// Fatora N amostras uniformes em dois passes (n1 densas × n2 espaçadas de n1): o núcleo resultante é o mesmo pente
/// de N amostras com custo n1 + n2 ≈ 2√N.
fn split_samples(n: usize) -> (usize, usize) {
    let n1 = (n as f32).sqrt().ceil() as usize;
    (n1, n.div_ceil(n1))
}

/// Efeito `DirectionalBlur { angle, length }`: média ao longo de um segmento de `length` px centrado no pixel,
/// amostras a ~1 px com taps bilineares. Acima de 24 amostras usa dois passes (n1 × n2, ver `split_samples`).
pub fn directional_blur(img: &mut Image, angle_deg: f32, length: f32) {
    if is_empty(img) || length < 0.5 {
        return;
    }
    let length = length.min((img.width + img.height) as f32);
    let n = length.ceil() as usize + 1;
    let a = angle_deg.to_radians();
    let (dx, dy) = (a.cos(), a.sin());
    if n <= 24 {
        let offs: Vec<f32> = (0..n).map(|i| (i as f32 / (n - 1) as f32 - 0.5) * length).collect();
        *img = gather_line(img, dx, dy, &offs);
        return;
    }
    let (n1, n2) = split_samples(n);
    let step = length / (n1 * n2 - 1) as f32;
    let offs1: Vec<f32> = (0..n1).map(|i| (i as f32 - (n1 - 1) as f32 * 0.5) * step).collect();
    let offs2: Vec<f32> = (0..n2).map(|j| (j as f32 - (n2 - 1) as f32 * 0.5) * step * n1 as f32).collect();
    let tmp = gather_line(img, dx, dy, &offs1);
    *img = gather_line(&tmp, dx, dy, &offs2);
}

/// Média das amostras de `src` nos pontos `c + (p − c)·(1 − amount·k·δ)` para cada fator `k` de `ks`.
fn gather_zoom(src: &Image, center: [f32; 2], amount_step: f32, ks: &[f32], min_span: f32, amount: f32) -> Image {
    let inv = 1.0 / ks.len() as f32;
    generate(src.width, src.height, |x, y| {
        let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
        let (vx, vy) = (cx - center[0], cy - center[1]);
        if (vx * vx + vy * vy).sqrt() * amount < min_span {
            return src.data[y as usize * src.width as usize + x as usize];
        }
        let mut acc = ZERO;
        for &k in ks {
            let s = 1.0 - amount_step * k;
            let q = sample(src, center[0] + vx * s, center[1] + vy * s);
            for c in 0..4 {
                acc[c] += q[c];
            }
        }
        scale(acc, inv)
    })
}

/// Efeito `RadialBlur { center, amount }`: média das amostras ao longo do raio até o centro.
/// `amount` 0..1 é a fração da distância ao centro percorrida pela amostra mais distante do pixel. O número de
/// amostras N cobre o raio máximo da imagem a ≤ ~1 px de passo; acima de 24 vira dois passes (n1 × n2).
pub fn radial_blur(img: &mut Image, center: [f32; 2], amount: f32) {
    if is_empty(img) {
        return;
    }
    let amount = amount.max(0.0).min(1.0);
    if amount <= 0.0 {
        return;
    }
    let (w, h) = (img.width as f32, img.height as f32);
    let dmax = [center[0].abs().max((w - center[0]).abs()), center[1].abs().max((h - center[1]).abs())];
    let maxspan = (dmax[0] * dmax[0] + dmax[1] * dmax[1]).sqrt() * amount;
    let n = ((maxspan.ceil() as usize).min(1024) + 1).max(2);
    if n <= 24 {
        let ks: Vec<f32> = (0..n).map(|i| i as f32 / (n - 1) as f32).collect();
        *img = gather_zoom(img, center, amount, &ks, 0.5, amount);
        return;
    }
    let (n1, n2) = split_samples(n);
    let delta = amount / (n1 * n2 - 1) as f32;
    let ks1: Vec<f32> = (0..n1).map(|i| i as f32).collect();
    let ks2: Vec<f32> = (0..n2).map(|j| (j * n1) as f32).collect();
    let tmp = gather_zoom(img, center, delta, &ks1, 0.5, amount);
    *img = gather_zoom(&tmp, center, delta, &ks2, 0.5, amount);
}

/// Peso `k` se o vizinho está dentro da tolerância de cor do centro, senão 0 (sem desvio, para vetorizar).
#[inline]
fn gate(tap: &Px, center: &Px, tol: f32, k: f32) -> f32 {
    let d0 = (tap[0] - center[0]).abs();
    let d1 = (tap[1] - center[1]).abs();
    let d2 = (tap[2] - center[2]).abs();
    let d3 = (tap[3] - center[3]).abs();
    let d = d0.max(d1).max(d2.max(d3));
    (d <= tol) as u8 as f32 * k
}

fn aware_h(src: &[Px], w: usize, h: usize, half: &[f32], tol: f32) -> Vec<Px> {
    let mut out = vec![ZERO; w * h];
    let r = half.len() - 1;
    out.par_chunks_mut(w).zip(src.par_chunks(w)).for_each_init(
        || (Vec::<Px>::new(), Vec::<f32>::new()),
        |(pad, wsum), (orow, srow)| {
            pad.clear();
            pad.extend(std::iter::repeat_n(srow[0], r));
            pad.extend_from_slice(srow);
            pad.extend(std::iter::repeat_n(srow[w - 1], r));
            wsum.clear();
            wsum.resize(w, half[0]);
            for x in 0..w {
                orow[x] = scale(srow[x], half[0]);
            }
            for j in 1..=r {
                let k = half[j];
                for x in 0..w {
                    let c = &srow[x];
                    for tap in [&pad[x + r - j], &pad[x + r + j]] {
                        let m = gate(tap, c, tol, k);
                        let o = &mut orow[x];
                        o[0] += m * tap[0];
                        o[1] += m * tap[1];
                        o[2] += m * tap[2];
                        o[3] += m * tap[3];
                        wsum[x] += m;
                    }
                }
            }
            for x in 0..w {
                orow[x] = scale(orow[x], 1.0 / wsum[x]);
            }
        },
    );
    out
}

fn aware_v(src: &[Px], w: usize, h: usize, half: &[f32], tol: f32) -> Vec<Px> {
    let mut out = vec![ZERO; w * h];
    let r = half.len() - 1;
    out.par_chunks_mut(w).enumerate().for_each_init(Vec::<f32>::new, |wsum, (y, orow)| {
        let c = &src[y * w..(y + 1) * w];
        wsum.clear();
        wsum.resize(w, half[0]);
        for x in 0..w {
            orow[x] = scale(c[x], half[0]);
        }
        for j in 1..=r {
            let k = half[j];
            let ya = y.saturating_sub(j);
            let yb = (y + j).min(h - 1);
            let a = &src[ya * w..(ya + 1) * w];
            let b = &src[yb * w..(yb + 1) * w];
            for x in 0..w {
                for tap in [&a[x], &b[x]] {
                    let m = gate(tap, &c[x], tol, k);
                    let o = &mut orow[x];
                    o[0] += m * tap[0];
                    o[1] += m * tap[1];
                    o[2] += m * tap[2];
                    o[3] += m * tap[3];
                    wsum[x] += m;
                }
            }
        }
        for x in 0..w {
            orow[x] = scale(orow[x], 1.0 / wsum[x]);
        }
    });
    out
}

/// Efeito `ColorAwareBlur { radius, tolerance }`. `tolerance` é a maior diferença por canal (pré-multiplicado,
/// 0..1) para um vizinho entrar na média. Com tolerância 0 a imagem não muda.
pub fn color_aware_blur(img: &mut Image, radius: f32, tolerance: f32) {
    if is_empty(img) {
        return;
    }
    let radius = radius.max(0.0).min(img.width.max(img.height) as f32).min(60.0);
    if radius < 0.3 {
        return;
    }
    let (w, h) = (img.width as usize, img.height as usize);
    let half = half_kernel(radius / 3.0);
    let a = aware_h(&img.data, w, h, &half, tolerance.max(0.0));
    img.data = aware_v(&a, w, h, &half, tolerance.max(0.0));
}
