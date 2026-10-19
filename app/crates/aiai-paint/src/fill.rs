//! Balde de tinta-e-pinta com fechamento de buracos.
//!
//! Algoritmo (rotulagem por competição, robusto a vazamento):
//! 1. Fronteira B = pixels de linha (alfa da camada de linha ≥ limiar, ou cor diferente da semente além da tolerância).
//! 2. Transformada de distância D até B (Felzenszwalb–Huttenlocher, exata).
//! 3. Núcleos = componentes conexos (4-viz.) de {p ∉ B : D(p) > gap/2}: um buraco de largura < gap não conecta núcleos.
//! 4. Propagação simultânea de todos os núcleos (BFS em ondas) para os pixels restantes fora de B: cada pixel fica com o
//!    núcleo que chega primeiro — o buraco é dividido ao meio entre as duas regiões, sem vazamento.
//! 5. Opcional: avança `expand` px para dentro da linha (sem halo entre a linha antisserrilhada e a cor).
//!
//! Referências lidas: "Close Gaps" do Krita (comportamento), LazyBrush (Sýkora et al. 2009) para a ideia de
//! competição entre regiões; implementação própria.

use crate::image::Image;
use crate::mask::Mask;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillParams {
    /// Largura máxima (px) de buraco na linha que ainda conta como fechado. 0 = sem fechamento.
    pub gap: f32,
    /// Quantos px o preenchimento avança por baixo da linha.
    pub expand: u32,
    /// Alfa mínimo para um pixel ser linha.
    pub line_threshold: f32,
}

impl Default for FillParams {
    fn default() -> Self {
        FillParams { gap: 3.0, expand: 2, line_threshold: 0.35 }
    }
}

/// Fronteira a partir do alfa de uma imagem de linhas.
pub fn boundary_from_alpha(img: &Image, threshold: f32) -> Vec<bool> {
    img.data.iter().map(|p| p[3] >= threshold).collect()
}

/// Fronteira por diferença de cor em relação à cor no ponto semente (balde raster "por cor").
pub fn boundary_from_color(img: &Image, seed: (u32, u32), tolerance: f32) -> Vec<bool> {
    let s = img.get(seed.0, seed.1);
    img.data
        .iter()
        .map(|p| {
            let d = (0..4).map(|c| (p[c] - s[c]).abs()).fold(0.0f32, f32::max);
            d > tolerance
        })
        .collect()
}

/// Distância euclidiana de cada pixel até o pixel de fronteira mais próximo (0 na fronteira; INF se não há fronteira).
pub fn distance_to_boundary(boundary: &[bool], w: usize, h: usize) -> Vec<f32> {
    use rayon::prelude::*;
    const INF: f32 = 1e20;
    let f: Vec<f32> = boundary.iter().map(|&b| if b { 0.0 } else { INF }).collect();
    // colunas (em paralelo, cada coluna numa lista contígua), depois linhas (em paralelo)
    let cols: Vec<Vec<f32>> = (0..w)
        .into_par_iter()
        .map(|x| {
            let tmp_in: Vec<f32> = (0..h).map(|y| f[y * w + x]).collect();
            let mut out = vec![0.0f32; h];
            edt_1d(&tmp_in, &mut out);
            out
        })
        .collect();
    let mut g = vec![0.0f32; w * h];
    g.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let tmp_in: Vec<f32> = (0..w).map(|x| cols[x][y]).collect();
        edt_1d(&tmp_in, row);
        for v in row.iter_mut() {
            *v = if *v >= INF * 0.5 { f32::INFINITY } else { v.sqrt() };
        }
    });
    g
}

/// Transformada de distância quadrática 1D (envelope inferior de parábolas; Felzenszwalb & Huttenlocher 2012), em f64
/// para não perder precisão com o "infinito" finito.
fn edt_1d(f: &[f32], d: &mut [f32]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let fv = |i: usize| f[i] as f64;
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let mut k = 0usize;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        let qf = q as f64;
        let mut s;
        loop {
            let p = v[k] as f64;
            s = ((fv(q) + qf * qf) - (fv(v[k]) + p * p)) / (2.0 * qf - 2.0 * p);
            if s <= z[k] {
                if k == 0 {
                    break;
                }
                k -= 1;
            } else {
                break;
            }
        }
        if s <= z[k] {
            // k == 0 e a nova parábola domina tudo
            v[0] = q;
            z[0] = f64::NEG_INFINITY;
            z[1] = f64::INFINITY;
            continue;
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f64 - p as f64;
        *out = (dq * dq + fv(p)) as f32;
    }
}

/// Resultado da rotulagem: rótulo 0 = fronteira não atribuída; 1..=count = regiões.
pub struct Regions {
    pub width: usize,
    pub height: usize,
    pub labels: Vec<u32>,
    pub count: u32,
}

impl Regions {
    pub fn label_at(&self, x: u32, y: u32) -> u32 {
        self.labels[y as usize * self.width + x as usize]
    }

    /// Rótulo da região na semente; se a semente cair na linha, a região mais próxima andando só pela linha
    /// (BFS por pixels sem rótulo, até 6 px): nunca pula por cima de uma região para pegar a do outro lado.
    pub fn label_near(&self, x: u32, y: u32) -> u32 {
        let l = self.label_at(x, y);
        if l != 0 {
            return l;
        }
        let (w, h) = (self.width, self.height);
        let start = y as usize * w + x as usize;
        let mut seen = std::collections::HashSet::from([start]);
        let mut ring = vec![start];
        for _ in 0..6 {
            let mut next = Vec::new();
            let mut found = 0u32;
            for &i in &ring {
                let (cx, cy) = (i % w, i / w);
                let neigh = [(cx > 0).then(|| i - 1), (cx + 1 < w).then(|| i + 1), (cy > 0).then(|| i - w), (cy + 1 < h).then(|| i + w)];
                for j in neigh.into_iter().flatten() {
                    if !seen.insert(j) {
                        continue;
                    }
                    let lj = self.labels[j];
                    if lj != 0 {
                        // empate na mesma distância: o menor rótulo (determinístico)
                        found = if found == 0 { lj } else { found.min(lj) };
                    } else {
                        next.push(j);
                    }
                }
            }
            if found != 0 {
                return found;
            }
            ring = next;
        }
        0
    }

    /// Máscara (recortada na caixa da região) e caixa [x, y, w, h].
    pub fn mask_of(&self, label: u32) -> Option<([i32; 4], Mask)> {
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        for y in 0..self.height {
            for x in 0..self.width {
                if self.labels[y * self.width + x] == label {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        if x0 == usize::MAX {
            return None;
        }
        let (mw, mh) = (x1 - x0 + 1, y1 - y0 + 1);
        let mut m = Mask::new(mw as u32, mh as u32);
        for y in 0..mh {
            for x in 0..mw {
                if self.labels[(y0 + y) * self.width + x0 + x] == label {
                    m.data[y * mw + x] = 255;
                }
            }
        }
        Some(([x0 as i32, y0 as i32, mw as i32, mh as i32], m))
    }

    /// Área (px) de cada região (índice = rótulo).
    pub fn areas(&self) -> Vec<usize> {
        let mut a = vec![0usize; self.count as usize + 1];
        for &l in &self.labels {
            a[l as usize] += 1;
        }
        a
    }

    /// Regiões que tocam a borda do canvas (normalmente o "fora" do desenho).
    pub fn touches_border(&self) -> Vec<bool> {
        let mut t = vec![false; self.count as usize + 1];
        let (w, h) = (self.width, self.height);
        for x in 0..w {
            t[self.labels[x] as usize] = true;
            t[self.labels[(h - 1) * w + x] as usize] = true;
        }
        for y in 0..h {
            t[self.labels[y * w] as usize] = true;
            t[self.labels[y * w + w - 1] as usize] = true;
        }
        t
    }
}

pub fn label_regions(boundary: &[bool], w: usize, h: usize, params: &FillParams) -> Regions {
    let n = w * h;
    let dist = distance_to_boundary(boundary, w, h);
    let core_r = params.gap.max(0.0) / 2.0;
    let mut labels = vec![0u32; n];
    let mut count = 0u32;
    let mut queue = VecDeque::new();
    // 1) núcleos
    for start in 0..n {
        if labels[start] != 0 || boundary[start] || dist[start] <= core_r {
            continue;
        }
        count += 1;
        labels[start] = count;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if labels[j] == 0 && !boundary[j] && dist[j] > core_r {
                    labels[j] = count;
                    queue.push_back(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
    }
    // 2) propagação em ondas: primeiro fora da linha (sem limite), depois `expand` ondas para dentro da linha.
    let mut frontier: Vec<usize> = (0..n).filter(|&i| labels[i] != 0).collect();
    let wave = |frontier: Vec<usize>, labels: &mut Vec<u32>, allow_boundary: bool| -> Vec<usize> {
        let mut next = Vec::new();
        for &i in &frontier {
            let l = labels[i];
            let (x, y) = (i % w, i / w);
            let neigh = [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ];
            for j in neigh.into_iter().flatten() {
                if labels[j] == 0 && (allow_boundary || !boundary[j]) {
                    labels[j] = l;
                    next.push(j);
                }
            }
        }
        next
    };
    // ondas fora da linha até esgotar (empate dentro da mesma onda: ordem estável = determinístico)
    let mut f = std::mem::take(&mut frontier);
    while !f.is_empty() {
        f = wave(f, &mut labels, false);
    }
    // regiões fechadas mais estreitas que o `gap` (espinho, ponta de mecha, canto de olho) não têm núcleo e nenhuma
    // onda chega nelas (estão cercadas de linha): cada componente que sobrou vira uma região própria.
    for start in 0..n {
        if labels[start] != 0 || boundary[start] {
            continue;
        }
        count += 1;
        labels[start] = count;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i % w, i / w);
            let neigh = [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)];
            for j in neigh.into_iter().flatten() {
                if labels[j] == 0 && !boundary[j] {
                    labels[j] = count;
                    queue.push_back(j);
                }
            }
        }
    }
    // ondas para dentro da linha
    let mut f: Vec<usize> = (0..n)
        .filter(|&i| labels[i] != 0)
        .filter(|&i| {
            let (x, y) = (i % w, i / w);
            (x > 0 && boundary[i - 1]) || (x + 1 < w && boundary[i + 1]) || (y > 0 && boundary[i - w]) || (y + 1 < h && boundary[i + w])
        })
        .collect();
    for _ in 0..params.expand {
        if f.is_empty() {
            break;
        }
        f = wave(f, &mut labels, true);
    }
    Regions { width: w, height: h, labels, count }
}

/// Balde: máscara da região que contém `seed`. `None` se a semente não cai em nenhuma região.
pub fn fill_at(boundary: &[bool], w: usize, h: usize, seed: (u32, u32), params: &FillParams) -> Option<([i32; 4], Mask)> {
    let regions = label_regions(boundary, w, h, params);
    let l = regions.label_near(seed.0, seed.1);
    if l == 0 {
        return None;
    }
    regions.mask_of(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Anel (círculo de linha) com um buraco de `gap_px` na direita.
    fn ring_with_gap(w: usize, h: usize, gap_px: f32) -> Vec<bool> {
        let (cx, cy, r) = (w as f32 / 2.0, h as f32 / 2.0, 30.0);
        let mut b = vec![false; w * h];
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let d = (dx * dx + dy * dy).sqrt();
                let in_gap = dx > 0.0 && dy.abs() < gap_px / 2.0;
                if (d - r).abs() <= 1.5 && !in_gap {
                    b[y * w + x] = true;
                }
            }
        }
        b
    }

    #[test]
    fn distance_transform_is_exact_on_simple_case() {
        let (w, h) = (9, 9);
        let mut b = vec![false; w * h];
        b[4 * w + 4] = true;
        let d = distance_to_boundary(&b, w, h);
        assert_eq!(d[4 * w + 4], 0.0);
        assert!((d[4 * w + 7] - 3.0).abs() < 1e-5);
        assert!((d[0] - (32.0f32).sqrt()).abs() < 1e-4);
    }

    #[test]
    fn closed_ring_fills_inside_only() {
        let (w, h) = (100, 100);
        let b = ring_with_gap(w, h, 0.0);
        let (bounds, m) = fill_at(&b, w, h, (50, 50), &FillParams { gap: 0.0, expand: 0, line_threshold: 0.5 }).unwrap();
        let area: usize = m.data.iter().filter(|&&v| v > 0).count();
        assert!(bounds[2] < 70 && area > 2200 && area < 2900, "interior do círculo r≈28: área {area}, caixa {bounds:?}");
    }

    #[test]
    fn gap_leaks_without_closing_and_holds_with_closing() {
        let (w, h) = (100, 100);
        let b = ring_with_gap(w, h, 4.0);
        let leak = fill_at(&b, w, h, (50, 50), &FillParams { gap: 0.0, expand: 0, line_threshold: 0.5 }).unwrap();
        let leak_area = leak.1.data.iter().filter(|&&v| v > 0).count();
        assert!(leak_area > 5000, "sem fechamento vaza para fora: {leak_area}");
        let held = fill_at(&b, w, h, (50, 50), &FillParams { gap: 6.0, expand: 0, line_threshold: 0.5 }).unwrap();
        let held_area = held.1.data.iter().filter(|&&v| v > 0).count();
        assert!(held_area > 2200 && held_area < 2950, "com fechamento fica dentro: {held_area}");
        assert!(held.0[2] < 70, "caixa do interior: {:?}", held.0);
    }

    #[test]
    fn expand_goes_under_the_line() {
        let (w, h) = (100, 100);
        let b = ring_with_gap(w, h, 0.0);
        let a0 = fill_at(&b, w, h, (50, 50), &FillParams { gap: 0.0, expand: 0, line_threshold: 0.5 }).unwrap().1.data.iter().filter(|&&v| v > 0).count();
        let a2 = fill_at(&b, w, h, (50, 50), &FillParams { gap: 0.0, expand: 2, line_threshold: 0.5 }).unwrap().1.data.iter().filter(|&&v| v > 0).count();
        assert!(a2 > a0 + 200, "expandir 2 px cresce a área: {a0} → {a2}");
    }

    #[test]
    fn narrow_closed_region_gets_its_own_label_and_never_leaks() {
        // "espinho": retângulo fechado de 3 px de vão interno (mais estreito que gap = 4) no meio de um fundo aberto
        let (w, h) = (60, 40);
        let mut b = vec![false; w * h];
        for x in 20..=40 {
            for y in [15usize, 16, 20, 21] {
                b[y * w + x] = true;
            }
        }
        for y in 15..=21 {
            for x in [20usize, 21, 39, 40] {
                b[y * w + x] = true;
            }
        }
        let params = FillParams { gap: 4.0, expand: 0, line_threshold: 0.5 };
        let r = label_regions(&b, w, h, &params);
        let inside = r.label_at(30, 18);
        let outside = r.label_at(5, 5);
        assert!(inside != 0 && outside != 0 && inside != outside, "o vão estreito tem rótulo próprio: dentro {inside}, fora {outside}");
        let (bounds, m) = fill_at(&b, w, h, (30, 18), &params).unwrap();
        assert!(bounds[2] <= 17 && bounds[3] <= 3, "a máscara fica no espinho: {bounds:?}");
        assert!(m.data.iter().filter(|&&v| v > 0).count() < 60);
        // semente em cima da linha de baixo do espinho: anda pela linha e pega uma das regiões vizinhas, não pula longe
        let l = r.label_near(30, 20);
        assert!(l == inside || l == outside);
    }
}
