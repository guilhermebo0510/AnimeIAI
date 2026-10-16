//! Mapa de bordas para linha de anime (Canny implementado aqui), F1 de bordas com tolerancia,
//! e mascara de regioes chapadas ("cel flat").
//!
//! Canny: desfoque gaussiano sigma = 1 sobre a luma (0..255), gradiente de Sobel dividido por 8
//! (magnitude ~ niveis de cinza por pixel), supressao de nao-maximos em 4 direcoes e histerese.
//! Limiares relativos aos percentis do gradiente: `alto = max(P95(magnitude), 10)` e `baixo = 0.4 * alto`.
//! O piso absoluto (10 niveis/pixel, equivale a um degrau de ~25 niveis de luma depois do desfoque) impede
//! que ruido de compressao/grao em quadros quase chapados vire "borda". Na comparacao, os limiares sao
//! calculados na REFERENCIA e aplicados aos dois quadros (`EdgeParams::auto(ref)`), para que as duas
//! imagens sejam medidas com a mesma regua.

use crate::filt::{box_mean, dilate, distance_transform, gaussian_blur};
use crate::rgb::AsRgb8;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeParams {
    pub sigma: f32,
    pub high: f32,
    pub low: f32,
}

pub const EDGE_HIGH_FLOOR: f32 = 10.0;
pub const EDGE_HIGH_PERCENTILE: f64 = 0.95;
pub const EDGE_LOW_RATIO: f32 = 0.4;

struct Gradient {
    mag: Vec<f32>,
    gx: Vec<f32>,
    gy: Vec<f32>,
}

fn gradient(luma: &[f32], w: usize, h: usize, sigma: f32) -> Gradient {
    let b = gaussian_blur(luma, w, h, sigma);
    let at = |x: isize, y: isize| -> f32 { b[y.clamp(0, h as isize - 1) as usize * w + x.clamp(0, w as isize - 1) as usize] };
    let mut gx = vec![0f32; w * h];
    let mut gy = vec![0f32; w * h];
    let mut mag = vec![0f32; w * h];
    gx.par_chunks_mut(w).zip(gy.par_chunks_mut(w)).zip(mag.par_chunks_mut(w)).enumerate().for_each(|(y, ((rx, ry), rm))| {
        let y = y as isize;
        for x in 0..w as isize {
            let sx = (at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1)) - (at(x - 1, y - 1) + 2.0 * at(x - 1, y) + at(x - 1, y + 1));
            let sy = (at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1)) - (at(x - 1, y - 1) + 2.0 * at(x, y - 1) + at(x + 1, y - 1));
            let (sx, sy) = (sx / 8.0, sy / 8.0);
            rx[x as usize] = sx;
            ry[x as usize] = sy;
            rm[x as usize] = sx.hypot(sy);
        }
    });
    Gradient { mag, gx, gy }
}

impl EdgeParams {
    /// Limiares a partir dos percentis do gradiente de `img` (ver o topo do modulo).
    pub fn auto(img: &impl AsRgb8) -> Self {
        let rgb = img.as_rgb8();
        let (w, h) = (rgb.width as usize, rgb.height as usize);
        let g = gradient(&rgb.luma(), w, h, 1.0);
        let mut m = g.mag.clone();
        let k = ((m.len() - 1) as f64 * EDGE_HIGH_PERCENTILE) as usize;
        let (_, p, _) = m.select_nth_unstable_by(k, |a, b| a.total_cmp(b));
        let high = p.max(EDGE_HIGH_FLOOR);
        EdgeParams { sigma: 1.0, high, low: high * EDGE_LOW_RATIO }
    }
}

/// Canny com limiares automaticos desta imagem.
pub fn edge_map(img: &impl AsRgb8) -> Vec<bool> {
    let p = EdgeParams::auto(img);
    edge_map_with(img, &p)
}

pub fn edge_map_with(img: &impl AsRgb8, p: &EdgeParams) -> Vec<bool> {
    let rgb = img.as_rgb8();
    let (w, h) = (rgb.width as usize, rgb.height as usize);
    let g = gradient(&rgb.luma(), w, h, p.sigma);
    // supressao de nao-maximos
    let mut nms = vec![0u8; w * h]; // 0 nada, 1 fraco, 2 forte
    nms.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let i = y * w + x;
            let m = g.mag[i];
            if m < p.low {
                continue;
            }
            // direcao quantizada do gradiente
            let (gx, gy) = (g.gx[i], g.gy[i]);
            let ang = gy.atan2(gx).to_degrees().rem_euclid(180.0);
            let (dx, dy): (isize, isize) = if !(22.5..157.5).contains(&ang) {
                (1, 0)
            } else if ang < 67.5 {
                (1, 1)
            } else if ang < 112.5 {
                (0, 1)
            } else {
                (-1, 1)
            };
            let sample = |sx: isize, sy: isize| -> f32 {
                if sx < 0 || sy < 0 || sx >= w as isize || sy >= h as isize { 0.0 } else { g.mag[sy as usize * w + sx as usize] }
            };
            let (xi, yi) = (x as isize, y as isize);
            let n1 = sample(xi + dx, yi + dy);
            let n2 = sample(xi - dx, yi - dy);
            if m > n1 && m >= n2 {
                row[x] = if m >= p.high { 2 } else { 1 };
            }
        }
    });
    // histerese (BFS a partir dos fortes, vizinhanca 8)
    let mut out = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();
    for i in 0..w * h {
        if nms[i] == 2 {
            out[i] = true;
            stack.push(i);
        }
    }
    while let Some(i) = stack.pop() {
        let (x, y) = ((i % w) as isize, (i / w) as isize);
        for dy in -1..=1isize {
            for dx in -1..=1isize {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if !out[j] && nms[j] == 1 {
                    out[j] = true;
                    stack.push(j);
                }
            }
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeScore {
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    pub ref_pixels: u64,
    pub test_pixels: u64,
}

/// F1 de bordas com tolerancia: um pixel de borda do teste esta certo se ha borda da referencia a
/// distancia <= `tolerance_px` (precisao); e vice-versa (revocacao). Ambos vazios -> 1.0; so um vazio -> 0.
pub fn edge_f1(ref_edges: &[bool], test_edges: &[bool], width: u32, height: u32, tolerance_px: f32) -> EdgeScore {
    let (w, h) = (width as usize, height as usize);
    assert_eq!(ref_edges.len(), w * h);
    assert_eq!(test_edges.len(), w * h);
    let nr = ref_edges.iter().filter(|b| **b).count() as u64;
    let nt = test_edges.iter().filter(|b| **b).count() as u64;
    if nr == 0 && nt == 0 {
        return EdgeScore { precision: 1.0, recall: 1.0, f1: 1.0, ref_pixels: 0, test_pixels: 0 };
    }
    if nr == 0 || nt == 0 {
        return EdgeScore { precision: 0.0, recall: 0.0, f1: 0.0, ref_pixels: nr, test_pixels: nt };
    }
    let dist_to_ref = distance_transform(ref_edges, w, h);
    let dist_to_test = distance_transform(test_edges, w, h);
    let tol = tolerance_px + 1e-4;
    let tp_p = (0..w * h).filter(|&i| test_edges[i] && dist_to_ref[i] <= tol).count() as f64;
    let tp_r = (0..w * h).filter(|&i| ref_edges[i] && dist_to_test[i] <= tol).count() as f64;
    let precision = tp_p / nt as f64;
    let recall = tp_r / nr as f64;
    let f1 = if precision + recall > 0.0 { 2.0 * precision * recall / (precision + recall) } else { 0.0 };
    EdgeScore { precision, recall, f1, ref_pixels: nr, test_pixels: nt }
}

pub const FLAT_VAR_MAX: f32 = 6.0;
pub const FLAT_EDGE_MARGIN_PX: f32 = 2.0;

/// Pixels "chapados" (cor lisa de cel): variancia da luma na janela 5x5 <= `FLAT_VAR_MAX` (desvio ~2.4 niveis,
/// acima do grao/ruido do HEVC) E a mais de `FLAT_EDGE_MARGIN_PX` pixels de qualquer borda Canny da imagem.
pub fn flat_region_mask(img: &impl AsRgb8) -> Vec<bool> {
    flat_region_mask_with(img, FLAT_VAR_MAX)
}

pub fn flat_region_mask_with(img: &impl AsRgb8, var_max: f32) -> Vec<bool> {
    let rgb = img.as_rgb8();
    let (w, h) = (rgb.width as usize, rgb.height as usize);
    let l: Vec<f64> = rgb.luma().iter().map(|v| *v as f64).collect();
    let l2: Vec<f64> = l.iter().map(|v| v * v).collect();
    let m1 = box_mean(&l, w, h, 2);
    let m2 = box_mean(&l2, w, h, 2);
    let edges = edge_map(&*rgb);
    let near_edge = dilate(&edges, w, h, FLAT_EDGE_MARGIN_PX);
    (0..w * h).into_par_iter().map(|i| (m2[i] - m1[i] * m1[i]) <= var_max as f64 && !near_edge[i]).collect()
}
