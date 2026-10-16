//! Espessura de linha: mascara de linha escura adaptativa, esqueleto (Zhang-Suen) e transformada de distancia.
//!
//! Mascara: um pixel e "linha" se sua luma esta abaixo da media local gaussiana (sigma 8) por mais de
//! `max(12, 12% da media local)` niveis (linha = mais escura que a vizinhanca), e a componente conexa tem
//! pelo menos 12 pixels (descarta pontos de ruido). Largura em cada pixel do esqueleto = `2*d - 1`, onde `d` e a
//! distancia euclidiana (ate o centro do pixel de fundo mais proximo); isso devolve exatamente a largura de uma
//! linha reta de largura impar e subestima em ~1 px linhas de largura par (quantizacao de pixel). Medidas
//! acima de `MAX_LINE_WIDTH_PX` (manchas escuras grandes, nao linhas) sao descartadas.
//! Comparacoes devem usar a mesma funcao nos dois quadros: o vies de quantizacao se cancela na razao.

use crate::filt::{distance_transform, gaussian_blur};
use crate::rgb::AsRgb8;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

pub const MAX_LINE_WIDTH_PX: f32 = 30.0;
pub const MIN_LINE_COMPONENT_PX: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineWidthStats {
    pub median: f32,
    pub p10: f32,
    pub p90: f32,
    pub samples: usize,
    /// Fracao da imagem coberta pela mascara de linha.
    pub line_fraction: f32,
    /// Media das larguras amostradas (nao e discreta como a mediana; util para ver variacoes menores que 1 px).
    pub mean: f32,
}

/// Mascara de linha escura adaptativa (ver o topo do modulo).
pub fn dark_line_mask(img: &impl AsRgb8) -> Vec<bool> {
    let rgb = img.as_rgb8();
    let (w, h) = (rgb.width as usize, rgb.height as usize);
    let l = rgb.luma();
    let local = gaussian_blur(&l, w, h, 8.0);
    mask_from(&l, &local, w, h)
}

fn mask_from(l: &[f32], local: &[f32], w: usize, h: usize) -> Vec<bool> {
    let mut mask: Vec<bool> = (0..w * h).into_par_iter().map(|i| l[i] < local[i] - (12.0f32).max(0.12 * local[i])).collect();
    remove_small_components(&mut mask, w, h, MIN_LINE_COMPONENT_PX);
    mask
}

fn remove_small_components(mask: &mut [bool], w: usize, h: usize, min_px: usize) {
    let mut seen = vec![false; w * h];
    let mut stack = Vec::new();
    let mut comp = Vec::new();
    for s in 0..w * h {
        if !mask[s] || seen[s] {
            continue;
        }
        comp.clear();
        stack.push(s);
        seen[s] = true;
        while let Some(i) = stack.pop() {
            comp.push(i);
            let (x, y) = ((i % w) as isize, (i / w) as isize);
            for dy in -1..=1isize {
                for dx in -1..=1isize {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if mask[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        if comp.len() < min_px {
            for &i in &comp {
                mask[i] = false;
            }
        }
    }
}

/// Zhang-Suen. Devolve o esqueleto (largura 1) da mascara.
pub fn skeletonize(mask: &[bool], w: usize, h: usize) -> Vec<bool> {
    // grade com margem de 1 pixel para evitar testes de borda
    let (pw, ph) = (w + 2, h + 2);
    let mut g = vec![0u8; pw * ph];
    for y in 0..h {
        for x in 0..w {
            g[(y + 1) * pw + x + 1] = mask[y * w + x] as u8;
        }
    }
    let mut changed = true;
    let mut del: Vec<usize> = Vec::new();
    while changed {
        changed = false;
        for step in 0..2 {
            del.clear();
            for y in 1..=h {
                for x in 1..=w {
                    let i = y * pw + x;
                    if g[i] == 0 {
                        continue;
                    }
                    let p2 = g[i - pw];
                    let p3 = g[i - pw + 1];
                    let p4 = g[i + 1];
                    let p5 = g[i + pw + 1];
                    let p6 = g[i + pw];
                    let p7 = g[i + pw - 1];
                    let p8 = g[i - 1];
                    let p9 = g[i - pw - 1];
                    let b = (p2 + p3 + p4 + p5 + p6 + p7 + p8 + p9) as i32;
                    if !(2..=6).contains(&b) {
                        continue;
                    }
                    let seq = [p2, p3, p4, p5, p6, p7, p8, p9, p2];
                    let a = seq.windows(2).filter(|s| s[0] == 0 && s[1] == 1).count();
                    if a != 1 {
                        continue;
                    }
                    let ok = if step == 0 { p2 * p4 * p6 == 0 && p4 * p6 * p8 == 0 } else { p2 * p4 * p8 == 0 && p2 * p6 * p8 == 0 };
                    if ok {
                        del.push(i);
                    }
                }
            }
            if !del.is_empty() {
                changed = true;
                for &i in &del {
                    g[i] = 0;
                }
            }
        }
    }
    let mut out = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = g[(y + 1) * pw + x + 1] != 0;
        }
    }
    out
}

/// Larguras (px) medidas em cada pixel do esqueleto da mascara.
pub fn line_width_samples(mask: &[bool], w: usize, h: usize) -> Vec<f32> {
    let skel = skeletonize(mask, w, h);
    samples_from(mask, &skel, w, h)
}

fn samples_from(mask: &[bool], skel: &[bool], w: usize, h: usize) -> Vec<f32> {
    let bg: Vec<bool> = mask.iter().map(|m| !*m).collect();
    // distancia ate o pixel de fundo mais proximo (borda da imagem nao conta como fundo)
    let d = distance_transform(&bg, w, h);
    let mut out: Vec<f32> = (0..w * h).filter(|&i| skel[i]).map(|i| 2.0 * d[i] - 1.0).filter(|v| *v <= MAX_LINE_WIDTH_PX && *v >= 1.0).collect();
    out.sort_by(|a, b| a.total_cmp(b));
    out
}

pub fn stats_from_samples(sorted: &[f32], line_fraction: f32) -> LineWidthStats {
    if sorted.is_empty() {
        return LineWidthStats { median: 0.0, p10: 0.0, p90: 0.0, samples: 0, line_fraction, mean: 0.0 };
    }
    let q = |p: f64| sorted[((sorted.len() - 1) as f64 * p).round() as usize];
    let mean = (sorted.iter().map(|v| *v as f64).sum::<f64>() / sorted.len() as f64) as f32;
    LineWidthStats { median: q(0.5), p10: q(0.1), p90: q(0.9), samples: sorted.len(), line_fraction, mean }
}

pub fn line_width_stats(img: &impl AsRgb8) -> LineWidthStats {
    let rgb = img.as_rgb8();
    let (w, h) = (rgb.width as usize, rgb.height as usize);
    let mask = dark_line_mask(&*rgb);
    line_width_stats_from_mask(&mask, w, h)
}

pub fn line_width_stats_from_mask(mask: &[bool], w: usize, h: usize) -> LineWidthStats {
    let frac = mask.iter().filter(|b| **b).count() as f32 / mask.len().max(1) as f32;
    let s = line_width_samples(mask, w, h);
    stats_from_samples(&s, frac)
}
