//! Instrumentos de medição para a calibração N1 (decisão 22): o agente mede a referência — cor mediana numa região,
//! centro e largura da linha perto do traço que ELE estimou — e usa os números para emitir os próprios traços.
//! Nada aqui cria traços nem extrai contornos da referência.

use crate::context::{ToolError, ToolResult};
use aiai_paint::Image;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Carrega um quadro de referência (PNG).
pub fn load_reference(path: &str) -> ToolResult<Image> {
    Image::load_png(std::path::Path::new(path)).map_err(|e| ToolError::Invalid(format!("referência {path}: {e}")))
}

#[inline]
fn lum(p: [f32; 4]) -> f32 {
    if p[3] <= 0.0 { 1.0 } else { (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]) / p[3] }
}

fn sample(img: &Image, x: f32, y: f32) -> [f32; 4] {
    aiai_paint::warp::sample_bilinear(img, x, y)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProbeResult {
    pub point: [f32; 2],
    /// Cor mediana sRGB "#rrggbb".
    pub hex: String,
    pub rgb: [f32; 3],
}

/// Cor mediana (por canal) num quadrado de lado 2·radius+1 em volta de cada ponto.
pub fn probe_colors(img: &Image, points: &[[f32; 2]], radius: u32) -> Vec<ProbeResult> {
    points
        .iter()
        .map(|&[x, y]| {
            let mut ch: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
            let r = radius as i32;
            for dy in -r..=r {
                for dx in -r..=r {
                    let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                    if xx < 0 || yy < 0 || xx >= img.width as i32 || yy >= img.height as i32 {
                        continue;
                    }
                    let p = img.get(xx as u32, yy as u32);
                    if p[3] > 0.0 {
                        for c in 0..3 {
                            ch[c].push(p[c] / p[3]);
                        }
                    }
                }
            }
            let med = |v: &mut Vec<f32>| {
                if v.is_empty() {
                    return 0.0;
                }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                v[v.len() / 2]
            };
            let rgb = [med(&mut ch[0]), med(&mut ch[1]), med(&mut ch[2])];
            let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            ProbeResult { point: [x, y], hex: format!("#{:02x}{:02x}{:02x}", c(rgb[0]), c(rgb[1]), c(rgb[2])), rgb }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LineMeasure {
    /// Ponto informado pelo agente.
    pub given: [f32; 2],
    /// Centro da linha medido na referência (ou o próprio ponto, se não achou linha).
    pub center: [f32; 2],
    /// Deslocamento aplicado (px).
    pub moved: f32,
    /// Largura medida da linha (px, meia-altura do contraste).
    pub width: f32,
    /// Escuridão relativa da linha (0 = não achou).
    pub contrast: f32,
}

/// Para cada ponto do traço estimado, busca ao longo da normal (±`search` px) o vale de luminância mais forte
/// (centro da linha) e mede sua largura. A normal vem dos vizinhos do próprio traço.
pub fn measure_line(img: &Image, points: &[[f32; 2]], search: f32, min_contrast: f32) -> Vec<LineMeasure> {
    measure_line_color(img, points, search, min_contrast, None)
}

/// Como [`measure_line`], mas com `line_color` (sRGB 0..1) o vale procurado é o da distância a essa cor de tinta em
/// vez da luminância — para linhas escuras sobre massas também escuras (ex.: linha marrom sobre cabelo carmim).
pub fn measure_line_color(img: &Image, points: &[[f32; 2]], search: f32, min_contrast: f32, line_color: Option<[f32; 3]>) -> Vec<LineMeasure> {
    let value = |p: [f32; 4]| match line_color {
        None => lum(p),
        Some(c) if p[3] > 0.0 => {
            let d: f32 = (0..3).map(|k| (p[k] / p[3] - c[k]).powi(2)).sum();
            (d / 3.0).sqrt()
        }
        Some(_) => 1.0,
    };
    let n = points.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p = points[i];
        let (a, b) = (points[i.saturating_sub(1)], points[(i + 1).min(n - 1)]);
        let (tx, ty) = (b[0] - a[0], b[1] - a[1]);
        let tl = (tx * tx + ty * ty).sqrt();
        if tl < 1e-4 {
            out.push(LineMeasure { given: p, center: p, moved: 0.0, width: 0.0, contrast: 0.0 });
            continue;
        }
        let (nx, ny) = (-ty / tl, tx / tl);
        let step = 0.25f32;
        let k = (search / step).ceil() as i32;
        let prof: Vec<(f32, f32)> = (-k..=k)
            .map(|j| {
                let t = j as f32 * step;
                // média de 3 amostras ao longo da tangente para reduzir ruído de compressão
                let mut l = 0.0;
                for s in [-1.0f32, 0.0, 1.0] {
                    l += value(sample(img, p[0] + nx * t + tx / tl * s, p[1] + ny * t + ty / tl * s));
                }
                (t, l / 3.0)
            })
            .collect();
        let bg = prof.iter().map(|q| q.1).fold(f32::MIN, f32::max);
        let (tmin, lmin) = prof.iter().cloned().fold((0.0, f32::MAX), |acc, q| if q.1 < acc.1 { q } else { acc });
        let contrast = bg - lmin;
        if contrast < min_contrast {
            out.push(LineMeasure { given: p, center: p, moved: 0.0, width: 0.0, contrast: 0.0 });
            continue;
        }
        // meia altura medida de CADA lado contra o fundo daquele lado (uma massa escura colada à linha — cabelo, sombra —
        // não conta como tinta); centro = centróide da escuridão dentro dessa região; largura = extensão dela
        let imin = prof.iter().position(|q| q.0 == tmin).unwrap_or(0);
        let bg_lo = prof[..=imin].iter().map(|q| q.1).fold(f32::MIN, f32::max);
        let bg_hi = prof[imin..].iter().map(|q| q.1).fold(f32::MIN, f32::max);
        let (half_lo, half_hi) = (lmin + (bg_lo - lmin) * 0.5, lmin + (bg_hi - lmin) * 0.5);
        let (mut lo, mut hi) = (imin, imin);
        while lo > 0 && prof[lo - 1].1 < half_lo {
            lo -= 1;
        }
        while hi + 1 < prof.len() && prof[hi + 1].1 < half_hi {
            hi += 1;
        }
        let half = half_lo.min(half_hi).max(lmin + 1e-4);
        let (mut wsum, mut tsum) = (0.0f32, 0.0f32);
        for q in &prof[lo..=hi] {
            let w = (half - q.1).max(0.0) + 1e-6;
            wsum += w;
            tsum += w * q.0;
        }
        let tc = if wsum > 0.0 { tsum / wsum } else { tmin };
        let width = (hi - lo + 1) as f32 * step;
        out.push(LineMeasure { given: p, center: [p[0] + nx * tc, p[1] + ny * tc], moved: tc.abs(), width, contrast });
    }
    out
}

/// Faixa de cor (paquímetro): ao longo da normal (±`search`), os pixels a até `tolerance` (distância RGB 0..1, como em
/// [`measure_line_color`]) de `color` formam a faixa; centro = meio do trecho contínuo que contém o ponto (ou o mais
/// próximo dele) e largura = comprimento desse trecho. Serve para faixas cuja cor varia por dentro (realces, reflexos).
pub fn measure_band(img: &Image, points: &[[f32; 2]], search: f32, color: [f32; 3], tolerance: f32) -> Vec<LineMeasure> {
    let n = points.len();
    let mut out = Vec::with_capacity(n);
    let step = 0.25f32;
    let k = (search / step).ceil() as i32;
    for i in 0..n {
        let p = points[i];
        let (a, b) = (points[i.saturating_sub(1)], points[(i + 1).min(n - 1)]);
        let (tx, ty) = (b[0] - a[0], b[1] - a[1]);
        let tl = (tx * tx + ty * ty).sqrt();
        let none = LineMeasure { given: p, center: p, moved: 0.0, width: 0.0, contrast: 0.0 };
        if tl < 1e-4 {
            out.push(none);
            continue;
        }
        let (nx, ny) = (-ty / tl, tx / tl);
        let inside: Vec<bool> = (-k..=k)
            .map(|j| {
                let t = j as f32 * step;
                let q = sample(img, p[0] + nx * t, p[1] + ny * t);
                if q[3] <= 0.0 {
                    return false;
                }
                let d: f32 = (0..3).map(|c| (q[c] / q[3] - color[c]).powi(2)).sum();
                (d / 3.0).sqrt() <= tolerance
            })
            .collect();
        // trechos contínuos [ini, fim] em índices
        let mut runs: Vec<(usize, usize)> = Vec::new();
        let mut start = None;
        for (idx, &v) in inside.iter().enumerate() {
            match (v, start) {
                (true, None) => start = Some(idx),
                (false, Some(s0)) => {
                    runs.push((s0, idx - 1));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s0) = start {
            runs.push((s0, inside.len() - 1));
        }
        let mid = k as usize;
        let dist = |r: &(usize, usize)| if r.0 <= mid && mid <= r.1 { 0 } else { (r.0 as i64 - mid as i64).abs().min((r.1 as i64 - mid as i64).abs()) };
        let Some(best) = runs.iter().filter(|r| r.1 > r.0).min_by_key(|r| dist(r)) else {
            out.push(none);
            continue;
        };
        let t0 = best.0 as f32 * step - search;
        let t1 = best.1 as f32 * step - search;
        let tc = (t0 + t1) / 2.0;
        out.push(LineMeasure { given: p, center: [p[0] + nx * tc, p[1] + ny * tc], moved: tc.abs(), width: t1 - t0 + step, contrast: 1.0 });
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EdgeMeasure {
    pub given: [f32; 2],
    /// Posição da borda (maior variação de cor ao longo da normal), subpixel.
    pub edge: [f32; 2],
    pub moved: f32,
    /// Intensidade do degrau (0 = não achou).
    pub strength: f32,
}

/// Borda de uma massa de cor (sem linha): para cada ponto do contorno estimado, procura ao longo da normal a maior
/// variação de cor (RGB) e refina por parábola. O contorno é tratado como fechado.
pub fn measure_edge(img: &Image, points: &[[f32; 2]], search: f32, min_strength: f32) -> Vec<EdgeMeasure> {
    let n = points.len();
    let mut out = Vec::with_capacity(n);
    let col = |x: f32, y: f32| {
        let p = sample(img, x, y);
        if p[3] > 0.0 { [p[0] / p[3], p[1] / p[3], p[2] / p[3]] } else { [1.0; 3] }
    };
    for i in 0..n {
        let p = points[i];
        let (a, b) = (points[(i + n - 1) % n], points[(i + 1) % n]);
        let (tx, ty) = (b[0] - a[0], b[1] - a[1]);
        let tl = (tx * tx + ty * ty).sqrt();
        if tl < 1e-4 {
            out.push(EdgeMeasure { given: p, edge: p, moved: 0.0, strength: 0.0 });
            continue;
        }
        let (nx, ny) = (-ty / tl, tx / tl);
        let step = 0.25f32;
        let k = (search / step).ceil() as i32;
        let grads: Vec<(f32, f32)> = (-k..k)
            .map(|j| {
                let t = j as f32 * step;
                let (c0, c1) = (col(p[0] + nx * t, p[1] + ny * t), col(p[0] + nx * (t + step), p[1] + ny * (t + step)));
                let g = ((c1[0] - c0[0]).powi(2) + (c1[1] - c0[1]).powi(2) + (c1[2] - c0[2]).powi(2)).sqrt() / step;
                (t + step / 2.0, g)
            })
            .collect();
        let best = grads.iter().enumerate().max_by(|x, y| x.1.1.partial_cmp(&y.1.1).unwrap_or(std::cmp::Ordering::Equal));
        let Some((im, &(tm, gm))) = best else {
            out.push(EdgeMeasure { given: p, edge: p, moved: 0.0, strength: 0.0 });
            continue;
        };
        if gm * step < min_strength {
            out.push(EdgeMeasure { given: p, edge: p, moved: 0.0, strength: 0.0 });
            continue;
        }
        let mut t = tm;
        if im > 0 && im + 1 < grads.len() {
            let (g0, g1, g2) = (grads[im - 1].1, gm, grads[im + 1].1);
            let den = g0 - 2.0 * g1 + g2;
            if den.abs() > 1e-6 {
                t += 0.5 * (g0 - g2) / den * step;
            }
        }
        out.push(EdgeMeasure { given: p, edge: [p[0] + nx * t, p[1] + ny * t], moved: t.abs(), strength: gm * step });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_line_center_and_width() {
        // linha escura vertical de 3 px centrada em x = 50,5 sobre fundo claro
        let mut img = Image::filled(100, 40, [0.95, 0.9, 0.85, 1.0]);
        for y in 0..40 {
            for x in 49..52 {
                let i = img.idx(x, y);
                img.data[i] = [0.2, 0.15, 0.15, 1.0];
            }
        }
        let est = vec![[46.0, 10.0], [46.5, 20.0], [47.0, 30.0]];
        let m = measure_line(&img, &est, 8.0, 0.1);
        for r in &m {
            assert!((r.center[0] - 50.5).abs() < 0.35, "centro {:?}", r.center);
            assert!((r.width - 3.0).abs() < 0.8, "largura {}", r.width);
        }
        // borda de massa: retângulo vermelho de x=60 em diante
        let mut m = Image::filled(100, 40, [0.95, 0.9, 0.85, 1.0]);
        for y in 0..40 {
            for x in 60..100 {
                let i = m.idx(x, y);
                m.data[i] = [0.7, 0.1, 0.1, 1.0];
            }
        }
        let e = measure_edge(&m, &[[57.0, 5.0], [57.5, 20.0], [58.0, 35.0]], 6.0, 0.1);
        for r in &e {
            assert!((r.edge[0] - 60.0).abs() < 0.35, "borda {:?}", r.edge);
        }
        // linha marrom sobre massa carmim escura: por luminância quase não há vale; pela cor da tinta, sim
        let mut h = Image::filled(100, 40, [0.63, 0.11, 0.23, 1.0]);
        for y in 0..40 {
            for x in 49..53 {
                let i = h.idx(x, y);
                h.data[i] = [0.36, 0.18, 0.18, 1.0];
            }
        }
        assert!(measure_line(&h, &est, 8.0, 0.06).iter().all(|r| r.contrast == 0.0));
        for r in measure_line_color(&h, &est, 8.0, 0.06, Some([0.36, 0.18, 0.18])) {
            assert!((r.center[0] - 51.0).abs() < 0.5 && (r.width - 4.0).abs() < 1.0, "{r:?}");
        }
        // linha de 4 px colada à direita numa massa carmim (x >= 53) sobre fundo claro: a massa não entra na largura
        let mut k = Image::filled(100, 40, [0.95, 0.9, 0.85, 1.0]);
        for y in 0..40 {
            for x in 49..100 {
                let i = k.idx(x, y);
                k.data[i] = if x < 53 { [0.36, 0.18, 0.18, 1.0] } else { [0.63, 0.11, 0.23, 1.0] };
            }
        }
        for r in measure_line_color(&k, &est, 8.0, 0.06, Some([0.36, 0.18, 0.18])) {
            assert!((r.width - 4.0).abs() < 1.0 && (r.center[0] - 51.0).abs() < 0.8, "{r:?}");
        }
        // faixa clara de 10 px (x 60..70) com cor variando por dentro: a faixa inteira é medida
        let mut f = Image::filled(100, 40, [0.63, 0.11, 0.23, 1.0]);
        for y in 0..40 {
            for x in 60..70 {
                let i = f.idx(x, y);
                let v = (x - 60) as f32 / 10.0;
                f.data[i] = [0.95, 0.60 + 0.1 * v, 0.57 + 0.1 * v, 1.0];
            }
        }
        for r in measure_band(&f, &[[58.0, 10.0], [58.0, 20.0], [58.0, 30.0]], 12.0, [0.95, 0.63, 0.6], 0.2) {
            assert!((r.center[0] - 65.0).abs() < 0.6 && (r.width - 10.0).abs() < 1.0, "{r:?}");
        }
        let c = probe_colors(&img, &[[10.0, 10.0]], 2);
        assert_eq!(c[0].hex, "#f2e6d9");
    }
}
