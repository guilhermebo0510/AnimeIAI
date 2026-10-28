//! Linha central subpixel (detector de cristas de Steger) e largura ao longo do traço `w(s)` (auditoria 4.9h).
//!
//! No lugar de comparar bordas Canny com tolerância de pixels, a bancada de calibração acha o centro de cada linha com
//! precisão de décimos de pixel e a largura dela em cada ponto, e compara referência e teste ponto a ponto:
//!
//! 1. "Escuridão" `d = 255 - luma`, suavizada por uma gaussiana de `sigma` (Steger pede `sigma >= W / (2 * sqrt 3)` para
//!    uma linha de largura `W`; o automático usa a mediana da largura medida pela máscara de linha).
//! 2. Hessiana por diferenças centrais. A direção normal `n` é o autovetor do autovalor mais negativo `l1` (a escuridão
//!    cai mais depressa atravessando a linha). O centro está em `t = -(g . n) / (n' H n)` ao longo de `n`; o pixel vale
//!    se o centro cai dentro dele (`|t nx|, |t ny| <= 0,5`) e se `-l1` passa o limiar de contraste.
//! 3. Junções em V/T/L e pontas: as duas curvaturas fortes (`l2 / l1 > 0,5`). O ponto fica marcado e não entra na
//!    comparação de largura (a largura de uma junção não é a de nenhum dos traços).
//! 4. Largura: a partir do centro, anda por `±n` na escuridão original (bilinear, passo 0,05 px) até cair à metade entre
//!    o pico e o fundo de cada lado (largura a meia altura). Uma borda de degrau (sem queda de um dos lados) é descartada.
//! 5. Os pontos se encadeiam em polilinhas (vizinhos de 8 com normais alinhadas), com o comprimento de arco `s`.

use crate::filt::gaussian_blur;
use crate::lines::{dark_line_mask, line_width_stats_from_mask};
use crate::rgb::AsRgb8;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StegerParams {
    /// Suavização (px). `None`: a partir da largura mediana da máscara de linha.
    pub sigma: Option<f32>,
    /// Contraste mínimo da linha contra o fundo, em níveis de luma (0..255).
    pub min_contrast: f32,
    /// Maior meia-largura procurada de cada lado (px).
    pub max_half_width: f32,
}

impl Default for StegerParams {
    fn default() -> Self {
        Self { sigma: None, min_contrast: 30.0, max_half_width: 30.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CenterPoint {
    pub x: f32,
    pub y: f32,
    /// Normal unitária (atravessa a linha).
    pub nx: f32,
    pub ny: f32,
    /// Largura a meia altura (px).
    pub width: f32,
    /// Contraste do pico contra o fundo mais claro dos dois lados (níveis de luma).
    pub contrast: f32,
    /// Junção (V/T/L), canto ou ponta solta: fora da comparação de largura e de posição.
    pub junction: bool,
}

/// Uma linha encadeada: índices em `points` e o comprimento de arco acumulado de cada um.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CenterLine {
    pub idx: Vec<usize>,
    pub s: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StegerResult {
    pub width: u32,
    pub height: u32,
    pub sigma: f32,
    pub points: Vec<CenterPoint>,
    pub lines: Vec<CenterLine>,
}

impl StegerResult {
    /// `w(s)` de uma linha: (comprimento de arco, largura) dos pontos que não são junção.
    pub fn width_profile(&self, line: usize) -> Vec<(f32, f32)> {
        let l = &self.lines[line];
        l.idx.iter().zip(&l.s).filter(|(i, _)| !self.points[**i].junction).map(|(i, s)| (*s, self.points[*i].width)).collect()
    }

    pub fn junctions(&self) -> usize {
        self.points.iter().filter(|p| p.junction).count()
    }

    /// Mediana, média, p10 e p90 da largura (pontos fora de junção).
    pub fn width_stats(&self) -> WidthSummary {
        let mut w: Vec<f32> = self.points.iter().filter(|p| !p.junction).map(|p| p.width).collect();
        w.sort_by(|a, b| a.total_cmp(b));
        if w.is_empty() {
            return WidthSummary::default();
        }
        let q = |p: f64| w[((w.len() - 1) as f64 * p).round() as usize];
        let mean = (w.iter().map(|v| *v as f64).sum::<f64>() / w.len() as f64) as f32;
        WidthSummary { samples: w.len(), mean, median: q(0.5), p10: q(0.1), p90: q(0.9), junctions: self.junctions(), lines: self.lines.len() }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WidthSummary {
    pub samples: usize,
    pub mean: f32,
    pub median: f32,
    pub p10: f32,
    pub p90: f32,
    pub junctions: usize,
    pub lines: usize,
}

/// `sigma` automático: Steger com folga para a largura mediana da máscara de linha (mínimo 1 px).
pub fn auto_sigma(img: &impl AsRgb8) -> f32 {
    let rgb = img.as_rgb8();
    let (w, h) = (rgb.width as usize, rgb.height as usize);
    let st = line_width_stats_from_mask(&dark_line_mask(&*rgb), w, h);
    (0.36 * st.median.max(1.0)).max(1.0)
}

pub fn steger(img: &impl AsRgb8, params: &StegerParams) -> StegerResult {
    let rgb = img.as_rgb8();
    let (w, h) = (rgb.width as usize, rgb.height as usize);
    let dark: Vec<f32> = rgb.luma().iter().map(|l| 255.0 - l).collect();
    let sigma = params.sigma.unwrap_or_else(|| auto_sigma(&*rgb)).max(0.5);
    steger_on(&dark, w, h, sigma, params)
}

/// O detector sobre um mapa de escuridão (0..255, maior = mais tinta), `w * h`.
pub fn steger_on(dark: &[f32], w: usize, h: usize, sigma: f32, params: &StegerParams) -> StegerResult {
    let g = gaussian_blur(dark, w, h, sigma);
    // curvatura de uma linha fina de contraste C no centro: C * W / (sqrt(2 pi) sigma^3); metade disso para W = 1
    let curv_min = 0.5 * params.min_contrast / ((2.0 * std::f32::consts::PI).sqrt() * sigma.powi(3));
    let at = |x: usize, y: usize| g[y * w + x];
    let rows: Vec<Vec<CenterPoint>> = (1..h.saturating_sub(1))
        .into_par_iter()
        .map(|y| {
            let mut out = Vec::new();
            for x in 1..w - 1 {
                let c = at(x, y);
                let rx = 0.5 * (at(x + 1, y) - at(x - 1, y));
                let ry = 0.5 * (at(x, y + 1) - at(x, y - 1));
                let rxx = at(x + 1, y) - 2.0 * c + at(x - 1, y);
                let ryy = at(x, y + 1) - 2.0 * c + at(x, y - 1);
                let rxy = 0.25 * (at(x + 1, y + 1) - at(x - 1, y + 1) - at(x + 1, y - 1) + at(x - 1, y - 1));
                // autovalores da hessiana simétrica 2x2
                let tr = 0.5 * (rxx + ryy);
                let disc = (0.25 * (rxx - ryy) * (rxx - ryy) + rxy * rxy).sqrt();
                let (l1, l2) = (tr - disc, tr + disc);
                if -l1 < curv_min {
                    continue;
                }
                // autovetor de l1
                // das duas formas do autovetor, a de maior norma (a outra some quando a hessiana é quase diagonal)
                let (ax, ay) = (l1 - ryy, rxy);
                let (bx, by) = (rxy, l1 - rxx);
                let (mut nx, mut ny) = if ax * ax + ay * ay >= bx * bx + by * by { (ax, ay) } else { (bx, by) };
                let nl = (nx * nx + ny * ny).sqrt();
                if nl < 1e-12 {
                    continue;
                }
                nx /= nl;
                ny /= nl;
                // o centro tem de cair dentro deste pixel; intervalo meio aberto (com folga para o arredondamento) para
                // que um centro exatamente entre dois pixels fique em um só, e não em nenhum ou nos dois
                let inside = |v: f32| (-0.501..0.499).contains(&v);
                // junção (V/T/L) ou ponta: as duas curvaturas fortes; o centro é o máximo 2D da escuridão (H d = -g)
                if l2 < 0.0 && l2 / l1 > JUNCTION_RATIO {
                    let det = rxx * ryy - rxy * rxy;
                    if det.abs() > 1e-9 {
                        let dx = -(ryy * rx - rxy * ry) / det;
                        let dy = -(rxx * ry - rxy * rx) / det;
                        if inside(dx) && inside(dy) {
                            out.push(CenterPoint { x: x as f32 + 0.5 + dx, y: y as f32 + 0.5 + dy, nx, ny, width: 0.0, contrast: c, junction: true });
                        }
                    }
                    continue;
                }
                let t = -(rx * nx + ry * ny) / l1;
                if !inside(t * nx) || !inside(t * ny) {
                    continue;
                }
                let (px, py) = (x as f32 + 0.5 + t * nx, y as f32 + 0.5 + t * ny);
                let Some((width, contrast)) = fwhm(dark, w, h, px, py, nx, ny, params.max_half_width) else { continue };
                if contrast < params.min_contrast {
                    continue;
                }
                out.push(CenterPoint { x: px, y: py, nx, ny, width, contrast, junction: false });
            }
            out
        })
        .collect();
    let mut points: Vec<CenterPoint> = rows.into_iter().flatten().collect();
    let lines = link(&points, w, h);
    // junções de linhas de mesma largura (T, L, V largo) quase não têm curvatura dupla: o detector deixa um buraco ali e as
    // linhas terminam. Uma ponta de linha a menos de `largura + 2 sigma` de outra linha é junção, no meio das duas.
    let mut owner = vec![usize::MAX; points.len()];
    for (li, l) in lines.iter().enumerate() {
        for &i in &l.idx {
            owner[i] = li;
        }
    }
    const CELL: usize = 8;
    let (gw, gh) = (w.div_ceil(CELL).max(1), h.div_ceil(CELL).max(1));
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); gw * gh];
    for (j, q) in points.iter().enumerate() {
        if owner[j] != usize::MAX && !q.junction {
            grid[(q.y as usize / CELL).min(gh - 1) * gw + (q.x as usize / CELL).min(gw - 1)].push(j);
        }
    }
    let mut extra = Vec::new();
    for (li, l) in lines.iter().enumerate() {
        for &e in [l.idx[0], l.idx[l.idx.len() - 1]].iter() {
            let pe = points[e];
            let r = pe.width + 2.0 * sigma;
            let rc = (r.ceil() as usize).div_ceil(CELL) as i64;
            let (cx, cy) = ((pe.x as usize / CELL) as i64, (pe.y as usize / CELL) as i64);
            let mut near: Option<(f32, &CenterPoint)> = None;
            for gy in (cy - rc).max(0)..=(cy + rc).min(gh as i64 - 1) {
                for gx in (cx - rc).max(0)..=(cx + rc).min(gw as i64 - 1) {
                    for &j in &grid[gy as usize * gw + gx as usize] {
                        if owner[j] == li {
                            continue;
                        }
                        let q = &points[j];
                        let d = (q.x - pe.x).powi(2) + (q.y - pe.y).powi(2);
                        if d <= r * r && near.is_none_or(|(bd, _)| d < bd) {
                            near = Some((d, q));
                        }
                    }
                }
            }
            if let Some((_, q)) = near {
                extra.push(CenterPoint { x: 0.5 * (pe.x + q.x), y: 0.5 * (pe.y + q.y), nx: pe.nx, ny: pe.ny, width: 0.0, contrast: pe.contrast, junction: true });
            }
        }
    }
    // cantos (L, e o V quando as linhas se encadeiam pela ponta): a normal gira mais de 35 graus em 4 pontos
    for l in &lines {
        for k in 2..l.idx.len().saturating_sub(2) {
            let (a, b) = (&points[l.idx[k - 2]], &points[l.idx[k + 2]]);
            if (a.nx * b.nx + a.ny * b.ny).abs() < 0.82 {
                let p = points[l.idx[k]];
                extra.push(CenterPoint { width: 0.0, junction: true, ..p });
            }
        }
    }
    points.extend(extra);
    // perto de uma junção o centro se curva para ela e a largura engorda: esses pontos também ficam de fora da comparação
    let centers: Vec<(f32, f32)> = points.iter().filter(|p| p.junction).map(|p| (p.x, p.y)).collect();
    if !centers.is_empty() {
        let mut cg: Vec<Vec<(f32, f32)>> = vec![Vec::new(); gw * gh];
        for c in &centers {
            cg[(c.1.max(0.0) as usize / CELL).min(gh - 1) * gw + (c.0.max(0.0) as usize / CELL).min(gw - 1)].push(*c);
        }
        for p in points.iter_mut().filter(|p| !p.junction) {
            let r = 2.0 * sigma + 0.5 * p.width;
            let rc = (r.ceil() as usize).div_ceil(CELL) as i64;
            let (cx, cy) = ((p.x as usize / CELL) as i64, (p.y as usize / CELL) as i64);
            let hit = ((cy - rc).max(0)..=(cy + rc).min(gh as i64 - 1)).any(|gy| {
                ((cx - rc).max(0)..=(cx + rc).min(gw as i64 - 1))
                    .any(|gx| cg[gy as usize * gw + gx as usize].iter().any(|c| (c.0 - p.x).powi(2) + (c.1 - p.y).powi(2) <= r * r))
            });
            if hit {
                p.junction = true;
            }
        }
    }
    // pontos que não entraram em linha nenhuma (soltos, de 1 ou 2): sem vizinhos para confirmar, ficam de fora
    for (j, p) in points.iter_mut().enumerate() {
        if j < owner.len() && owner[j] == usize::MAX {
            p.junction = true;
        }
    }
    // pontas soltas: a meia largura mais 2 sigma do fim, a gaussiana já mistura o fundo além da ponta e a largura cai
    for l in &lines {
        let total = *l.s.last().unwrap_or(&0.0);
        let mut ws: Vec<f32> = l.idx.iter().map(|&i| points[i].width).collect();
        ws.sort_by(|a, b| a.total_cmp(b));
        let cut = 0.5 * ws[ws.len() / 2] + 2.0 * sigma;
        for (&i, &sv) in l.idx.iter().zip(&l.s) {
            if sv < cut || total - sv < cut {
                points[i].junction = true;
            }
        }
    }
    StegerResult { width: w as u32, height: h as u32, sigma, points, lines }
}

fn bilinear(img: &[f32], w: usize, h: usize, x: f32, y: f32) -> Option<f32> {
    // coordenadas com o centro do pixel em +0,5
    let (fx, fy) = (x - 0.5, y - 0.5);
    if fx < 0.0 || fy < 0.0 || fx > (w - 1) as f32 || fy > (h - 1) as f32 {
        return None;
    }
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
    let v = |x: usize, y: usize| img[y * w + x];
    Some((v(x0, y0) * (1.0 - ax) + v(x1, y0) * ax) * (1.0 - ay) + (v(x0, y1) * (1.0 - ax) + v(x1, y1) * ax) * ay)
}

/// Largura a meia altura atravessando a linha em (x, y) pela normal; e o contraste contra o lado de fundo mais claro.
#[allow(clippy::too_many_arguments)]
fn fwhm(dark: &[f32], w: usize, h: usize, x: f32, y: f32, nx: f32, ny: f32, max_half: f32) -> Option<(f32, f32)> {
    const STEP: f32 = 0.05;
    // pico: o maior valor a até 1 px do centro (a linha antisserrilhada pode ter o máximo meio pixel ao lado)
    let mut peak = f32::MIN;
    let mut k = -20;
    while k <= 20 {
        let d = k as f32 * STEP;
        if let Some(v) = bilinear(dark, w, h, x + nx * d, y + ny * d) {
            peak = peak.max(v);
        }
        k += 1;
    }
    let mut half = [0f32; 2];
    let mut bg = [0f32; 2];
    for (side, sgn) in [(0, 1.0f32), (1, -1.0f32)] {
        // fundo deste lado: o mínimo da escuridão até a distância máxima
        let mut lo = peak;
        let mut d = 0.0;
        let mut prof = Vec::new();
        while d <= max_half {
            // a borda da imagem encerra a busca deste lado
            let Some(v) = bilinear(dark, w, h, x + sgn * nx * d, y + sgn * ny * d) else { break };
            prof.push(v);
            lo = lo.min(v);
            // achou um fundo bem claro e já voltou a subir: outra linha; para aqui
            if v > lo + 0.5 * (peak - lo) && lo < peak - 0.75 * (peak - lo).max(1.0) && d > 1.0 && prof.len() > 2 {
                break;
            }
            d += STEP;
        }
        if peak - lo < 1.0 {
            return None;
        }
        let level = lo + 0.5 * (peak - lo);
        // primeira queda abaixo da metade, com interpolação linear entre as duas amostras
        let mut found = None;
        for i in 1..prof.len() {
            if prof[i] <= level && prof[i - 1] > level {
                let f = (prof[i - 1] - level) / (prof[i - 1] - prof[i]).max(1e-6);
                found = Some((i as f32 - 1.0 + f) * STEP);
                break;
            }
        }
        half[side] = found?;
        bg[side] = lo;
    }
    let contrast = peak - bg[0].min(bg[1]);
    // contraste contra o lado mais escuro também tem de existir (um degrau só cai de um lado)
    if peak - bg[0].max(bg[1]) < 0.35 * contrast {
        return None;
    }
    Some((half[0] + half[1], contrast))
}

/// Encadeia os pontos em linhas: vizinho de 8 (na grade dos pixels) à frente na tangente, com a normal alinhada.
fn link(points: &[CenterPoint], w: usize, h: usize) -> Vec<CenterLine> {
    let mut grid = vec![usize::MAX; w * h];
    for (i, p) in points.iter().enumerate() {
        let (x, y) = (p.x.floor() as usize, p.y.floor() as usize);
        if x < w && y < h {
            grid[y * w + x] = i;
        }
    }
    let mut used = vec![false; points.len()];
    let next = |cur: usize, dir: (f32, f32), used: &[bool]| -> Option<(usize, (f32, f32))> {
        let p = &points[cur];
        let (cx, cy) = (p.x.floor() as i64, p.y.floor() as i64);
        let mut best: Option<(f32, usize)> = None;
        for dy in -1..=1i64 {
            for dx in -1..=1i64 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (x, y) = (cx + dx, cy + dy);
                if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                    continue;
                }
                let j = grid[y as usize * w + x as usize];
                if j == usize::MAX || used[j] {
                    continue;
                }
                let q = &points[j];
                if (p.nx * q.nx + p.ny * q.ny).abs() < 0.7 {
                    continue;
                }
                let (vx, vy) = (q.x - p.x, q.y - p.y);
                let dist = (vx * vx + vy * vy).sqrt();
                if dist < 1e-4 || dist > 2.0 {
                    continue;
                }
                let along = (vx * dir.0 + vy * dir.1) / dist;
                if along < 0.5 {
                    continue;
                }
                // prefere o que segue a tangente e fica perto da reta
                let across = ((vx * p.nx + vy * p.ny) / dist).abs();
                let cost = dist * (1.0 + 2.0 * across);
                if best.is_none_or(|(c, _)| cost < c) {
                    best = Some((cost, j));
                }
            }
        }
        let (_, j) = best?;
        let q = &points[j];
        let mut t = (-q.ny, q.nx);
        if t.0 * dir.0 + t.1 * dir.1 < 0.0 {
            t = (-t.0, -t.1);
        }
        Some((j, t))
    };
    let mut lines = Vec::new();
    for start in 0..points.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let p = &points[start];
        let mut fwd = vec![start];
        let mut dir = (-p.ny, p.nx);
        let mut cur = start;
        while let Some((j, d)) = next(cur, dir, &used) {
            used[j] = true;
            fwd.push(j);
            cur = j;
            dir = d;
        }
        let mut back = Vec::new();
        let mut dir = (p.ny, -p.nx);
        let mut cur = start;
        while let Some((j, d)) = next(cur, dir, &used) {
            used[j] = true;
            back.push(j);
            cur = j;
            dir = d;
        }
        back.reverse();
        back.extend(fwd);
        if back.len() < 3 {
            continue;
        }
        let mut s = Vec::with_capacity(back.len());
        let mut acc = 0.0;
        for (k, &i) in back.iter().enumerate() {
            if k > 0 {
                let (a, b) = (&points[back[k - 1]], &points[i]);
                acc += ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
            }
            s.push(acc);
        }
        lines.push(CenterLine { idx: back, s });
    }
    lines
}

/// Referência contra teste pelas linhas centrais (critério da auditoria 4.9.6, teste 4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CenterlineCompare {
    /// Pontos de linha (fora de junção) da referência e do teste.
    pub ref_points: usize,
    pub test_points: usize,
    /// Fração dos pontos da referência com par no teste a até `max_dist`, e o inverso.
    pub recall: f64,
    pub precision: f64,
    /// Desvio da linha central (distância pela normal da referência), média e p90 (px).
    pub deviation_mean: f64,
    pub deviation_p90: f64,
    /// Larguras pareadas: média da referência e do teste, erro absoluto médio e R² de w_teste contra w_ref.
    pub width_ref_mean: f64,
    pub width_test_mean: f64,
    pub width_abs_err: f64,
    pub width_r2: f64,
    /// Desvio-padrão da largura da referência (com pouca variação, o R² não diz nada e vale o erro absoluto).
    pub width_ref_std: f64,
    pub junctions_ref: usize,
    pub junctions_test: usize,
    pub deviation_ok: bool,
    pub width_profile_ok: bool,
}

/// Razão `l2 / l1` acima da qual o ponto é junção (ou ponta), e não linha.
pub const JUNCTION_RATIO: f32 = 0.3;

pub const CENTERLINE_DEVIATION_MAX: f64 = 0.75;
pub const WIDTH_R2_MIN: f64 = 0.90;

pub fn compare_centerlines(reference: &StegerResult, test: &StegerResult, max_dist: f32) -> CenterlineCompare {
    let (w, h) = (reference.width as usize, reference.height as usize);
    let cell = 4usize;
    let (gw, gh) = (w.div_ceil(cell).max(1), h.div_ceil(cell).max(1));
    let bucket = |pts: &[CenterPoint]| {
        let mut b: Vec<Vec<usize>> = vec![Vec::new(); gw * gh];
        for (i, p) in pts.iter().enumerate() {
            if p.junction {
                continue;
            }
            let (cx, cy) = ((p.x.max(0.0) as usize / cell).min(gw - 1), (p.y.max(0.0) as usize / cell).min(gh - 1));
            b[cy * gw + cx].push(i);
        }
        b
    };
    let nearest = |p: &CenterPoint, pts: &[CenterPoint], b: &[Vec<usize>]| -> Option<usize> {
        let r = (max_dist.ceil() as usize).div_ceil(cell) as i64;
        let (cx, cy) = ((p.x.max(0.0) as usize / cell) as i64, (p.y.max(0.0) as usize / cell) as i64);
        let mut best: Option<(f32, usize)> = None;
        for gy in cy - r..=cy + r {
            for gx in cx - r..=cx + r {
                if gx < 0 || gy < 0 || gx >= gw as i64 || gy >= gh as i64 {
                    continue;
                }
                for &j in &b[gy as usize * gw + gx as usize] {
                    let q = &pts[j];
                    // só pares do mesmo traço: normais alinhadas
                    if (p.nx * q.nx + p.ny * q.ny).abs() < 0.7 {
                        continue;
                    }
                    let d = ((q.x - p.x).powi(2) + (q.y - p.y).powi(2)).sqrt();
                    if d <= max_dist && best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, j));
                    }
                }
            }
        }
        best.map(|(_, j)| j)
    };
    let (rp, tp) = (&reference.points, &test.points);
    let (rb, tb) = (bucket(rp), bucket(tp));
    let mut dev = Vec::new();
    let mut pairs: Vec<(f64, f64)> = Vec::new();
    let mut nref = 0usize;
    for p in rp.iter().filter(|p| !p.junction) {
        nref += 1;
        if let Some(j) = nearest(p, tp, &tb) {
            let q = &tp[j];
            dev.push((((q.x - p.x) * p.nx + (q.y - p.y) * p.ny).abs()) as f64);
            pairs.push((p.width as f64, q.width as f64));
        }
    }
    let ntest = tp.iter().filter(|p| !p.junction).count();
    let matched_test = tp.iter().filter(|p| !p.junction && nearest(p, rp, &rb).is_some()).count();
    dev.sort_by(|a, b| a.total_cmp(b));
    let mean = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
    let deviation_mean = mean(&dev);
    let deviation_p90 = if dev.is_empty() { 0.0 } else { dev[((dev.len() - 1) as f64 * 0.9).round() as usize] };
    let wr: Vec<f64> = pairs.iter().map(|p| p.0).collect();
    let wt: Vec<f64> = pairs.iter().map(|p| p.1).collect();
    let (mr, mt) = (mean(&wr), mean(&wt));
    let ss_tot: f64 = wr.iter().map(|v| (v - mr).powi(2)).sum();
    let ss_res: f64 = pairs.iter().map(|(a, b)| (b - a).powi(2)).sum();
    let width_r2 = if ss_tot > 1e-9 { 1.0 - ss_res / ss_tot } else { 0.0 };
    let width_ref_std = if wr.is_empty() { 0.0 } else { (ss_tot / wr.len() as f64).sqrt() };
    let width_abs_err = mean(&pairs.iter().map(|(a, b)| (b - a).abs()).collect::<Vec<_>>());
    let recall = if nref == 0 { 0.0 } else { pairs.len() as f64 / nref as f64 };
    let precision = if ntest == 0 { 0.0 } else { matched_test as f64 / ntest as f64 };
    CenterlineCompare {
        ref_points: nref,
        test_points: ntest,
        recall,
        precision,
        deviation_mean,
        deviation_p90,
        width_ref_mean: mr,
        width_test_mean: mt,
        width_abs_err,
        width_r2,
        width_ref_std,
        junctions_ref: reference.junctions(),
        junctions_test: test.junctions(),
        deviation_ok: !pairs.is_empty() && deviation_mean <= CENTERLINE_DEVIATION_MAX,
        // R² >= 0,90; com a largura da referência quase constante (desvio < 0,5 px), erro absoluto médio <= 0,3 px
        width_profile_ok: !pairs.is_empty() && (width_r2 >= WIDTH_R2_MIN || (width_ref_std < 0.5 && width_abs_err <= 0.3)),
    }
}

/// As duas imagens pelo mesmo `sigma` (o da referência, quando automático) e a comparação.
pub fn centerline_compare(reference: &impl AsRgb8, test: &impl AsRgb8, params: &StegerParams, max_dist: f32) -> (StegerResult, StegerResult, CenterlineCompare) {
    let r = reference.as_rgb8();
    let sigma = params.sigma.unwrap_or_else(|| auto_sigma(&*r));
    let p = StegerParams { sigma: Some(sigma), ..*params };
    let a = steger(&*r, &p);
    let b = steger(test, &p);
    let c = compare_centerlines(&a, &b, max_dist);
    (a, b, c)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Escuridão de uma linha reta de largura `wd` antisserrilhada (cobertura analítica por supersampling 8x8).
    fn line_img(w: usize, h: usize, a: (f32, f32), b: (f32, f32), wd: f32, contrast: f32) -> Vec<f32> {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt();
        let (ux, uy) = (dx / len, dy / len);
        let mut out = vec![0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut cov = 0.0;
                for sy in 0..8 {
                    for sx in 0..8 {
                        let (px, py) = (x as f32 + (sx as f32 + 0.5) / 8.0 - a.0, y as f32 + (sy as f32 + 0.5) / 8.0 - a.1);
                        let along = px * ux + py * uy;
                        let across = (px * -uy + py * ux).abs();
                        if (0.0..=len).contains(&along) && across <= wd / 2.0 {
                            cov += 1.0;
                        }
                    }
                }
                out[y * w + x] = contrast * cov / 64.0;
            }
        }
        out
    }

    fn params() -> StegerParams {
        StegerParams::default()
    }

    #[test]
    fn subpixel_center_and_width_of_straight_lines() {
        for (wd, y0) in [(2.0f32, 30.37f32), (3.0, 30.81), (5.0, 31.2), (8.0, 30.5)] {
            // levemente inclinada: o centro passa por todos os décimos de pixel
            let d = line_img(120, 64, (5.0, y0), (115.0, y0 + 2.3), wd, 220.0);
            let sigma = (0.36 * wd).max(1.0);
            let r = steger_on(&d, 120, 64, sigma, &params());
            let body: Vec<&CenterPoint> = r.points.iter().filter(|p| !p.junction && p.x > 25.0 && p.x < 95.0).collect();
            assert!(body.len() > 65, "largura {wd}: {} pontos", body.len());
            let slope = 2.3 / 110.0;
            let mut err = 0.0f32;
            let mut werr = 0.0f32;
            for p in &body {
                let yc = y0 + (p.x - 5.0) * slope;
                err = err.max((p.y - yc).abs());
                werr = werr.max((p.width - wd).abs());
            }
            assert!(err < 0.12, "largura {wd}: centro errado em {err} px");
            assert!(werr < 0.3, "largura {wd}: largura errada em {werr} px");
            // uma linha só, encadeada
            let long = r.lines.iter().map(|l| l.idx.len()).max().unwrap_or(0);
            assert!(long > 90, "largura {wd}: linha encadeada com {long} pontos");
        }
    }

    #[test]
    fn width_profile_follows_a_taper() {
        // traço que afina de 6 px para 2 px: w(s) decresce
        let (w, h) = (140usize, 50usize);
        let mut d = vec![0f32; w * h];
        for x in 10..130 {
            let wd = 6.0 - 4.0 * (x as f32 - 10.0) / 120.0;
            let seg = line_img(w, h, (x as f32, 25.0), (x as f32 + 1.0, 25.0), wd, 220.0);
            for (o, s) in d.iter_mut().zip(seg) {
                *o = o.max(s);
            }
        }
        let r = steger_on(&d, w, h, 1.6, &params());
        let li = (0..r.lines.len()).max_by_key(|&i| r.lines[i].idx.len()).unwrap();
        let prof = r.width_profile(li);
        assert!(prof.len() > 90);
        let first: f32 = prof.iter().take(10).map(|p| p.1).sum::<f32>() / 10.0;
        let last: f32 = prof.iter().rev().take(10).map(|p| p.1).sum::<f32>() / 10.0;
        let (big, small) = if first > last { (first, last) } else { (last, first) };
        assert!(big > 5.0 && small < 3.0, "w(s) não acompanhou o afinamento: {first} .. {last}");
    }

    #[test]
    fn junctions_are_flagged_and_step_edges_ignored() {
        // T: horizontal e vertical se encontrando; e um degrau de cor (sem linha) à direita
        let (w, h) = (120usize, 80usize);
        let a = line_img(w, h, (10.0, 20.0), (70.0, 20.0), 3.0, 220.0);
        let b = line_img(w, h, (40.0, 20.0), (40.0, 70.0), 3.0, 220.0);
        let mut d: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x.max(*y)).collect();
        for y in 0..h {
            for x in 90..w {
                d[y * w + x] = 120.0;
            }
        }
        let r = steger_on(&d, w, h, 1.2, &params());
        assert!(r.points.iter().any(|p| p.junction && (p.x - 40.0).abs() < 4.0 && (p.y - 20.0).abs() < 4.0), "junção do T não marcada");
        assert!(!r.points.iter().any(|p| p.x > 85.0 && !p.junction), "degrau tomado por linha");
    }

    #[test]
    fn v_and_l_junctions_are_found() {
        let (w, h) = (160usize, 120usize);
        // V de 15 graus (mecha de cabelo) e L de 90 graus
        let tip = (20.0f32, 30.0f32);
        let ang = 15f32.to_radians();
        let a = line_img(w, h, tip, (tip.0 + 100.0, tip.1), 3.0, 220.0);
        let b = line_img(w, h, tip, (tip.0 + 100.0 * ang.cos(), tip.1 + 100.0 * ang.sin()), 3.0, 220.0);
        let c = line_img(w, h, (30.0, 70.0), (130.0, 70.0), 3.0, 220.0);
        let e = line_img(w, h, (30.0, 70.0), (30.0, 115.0), 3.0, 220.0);
        let d: Vec<f32> = (0..w * h).map(|i| a[i].max(b[i]).max(c[i]).max(e[i])).collect();
        let r = steger_on(&d, w, h, 1.2, &params());
        let near = |x: f32, y: f32, rad: f32| r.points.iter().any(|p| p.junction && (p.x - x).hypot(p.y - y) < rad);
        // o V se funde por um trecho (as duas linhas encostam): a junção fica perto da ponta, dentro do trecho fundido
        assert!(r.points.iter().any(|p| p.junction && p.x < tip.0 + 25.0 && (p.y - tip.1).abs() < 6.0), "V sem junção");
        assert!(near(30.0, 70.0, 5.0), "L sem junção");
        // longe das junções, larguras de 3 px
        let ws: Vec<f32> = r.points.iter().filter(|p| !p.junction && p.x > 60.0 && (p.y - 70.0).abs() < 2.0).map(|p| p.width).collect();
        assert!(!ws.is_empty() && ws.iter().all(|v| (v - 3.0).abs() < 0.3), "{ws:?}");
    }

    #[test]
    fn compare_finds_offset_and_width_change() {
        let (w, h) = (100usize, 60usize);
        let a = line_img(w, h, (5.0, 30.0), (95.0, 30.0), 3.0, 220.0);
        let same = steger_on(&a, w, h, 1.2, &params());
        let c = compare_centerlines(&same, &same, 3.0);
        assert!(c.deviation_mean < 1e-6 && c.recall > 0.99 && c.deviation_ok && c.width_profile_ok);
        // deslocada 0,4 px e mais grossa: desvio ~0,4 px; largura +1 px reprova o perfil
        let b = line_img(w, h, (5.0, 30.4), (95.0, 30.4), 4.0, 220.0);
        let t = steger_on(&b, w, h, 1.2, &params());
        let c = compare_centerlines(&same, &t, 3.0);
        assert!((c.deviation_mean - 0.4).abs() < 0.1, "desvio {}", c.deviation_mean);
        assert!(c.deviation_ok);
        assert!((c.width_test_mean - c.width_ref_mean - 1.0).abs() < 0.3, "{c:?}");
        assert!(!c.width_profile_ok);
        // deslocada 1,5 px: reprova o desvio
        let b = line_img(w, h, (5.0, 31.5), (95.0, 31.5), 3.0, 220.0);
        let t = steger_on(&b, w, h, 1.2, &params());
        assert!(!compare_centerlines(&same, &t, 3.0).deviation_ok);
    }
}
