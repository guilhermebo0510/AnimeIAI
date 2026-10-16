//! SSIM (Wang et al. 2004) sobre luma BT.709 de valores sRGB codificados.
//! Janela gaussiana 11x11, sigma 1.5, K1 = 0.01, K2 = 0.03, faixa dinamica 255, modo "valid".

use crate::rgb::{same_size, AsRgb8};
use rayon::prelude::*;

pub const SSIM_WINDOW: usize = 11;
pub const SSIM_SIGMA: f64 = 1.5;

#[derive(Clone, Debug)]
pub struct SsimResult {
    /// Media do mapa SSIM (pontuacao unica).
    pub mean: f64,
    /// Mapa SSIM na regiao "valid": `(width-10) x (height-10)` (1x1 se a imagem for menor que a janela).
    pub map: Vec<f32>,
    pub map_width: u32,
    pub map_height: u32,
}

fn conv_valid(src: &[f64], w: usize, h: usize, k: &[f64]) -> (Vec<f64>, usize, usize) {
    let n = k.len();
    let (ow, oh) = (w + 1 - n, h + 1 - n);
    let mut tmp = vec![0f64; ow * h];
    tmp.par_chunks_mut(ow).enumerate().for_each(|(y, row)| {
        let line = &src[y * w..(y + 1) * w];
        for x in 0..ow {
            let mut acc = 0.0;
            for i in 0..n {
                acc += k[i] * line[x + i];
            }
            row[x] = acc;
        }
    });
    let mut out = vec![0f64; ow * oh];
    out.par_chunks_mut(ow).enumerate().for_each(|(y, row)| {
        for i in 0..n {
            let line = &tmp[(y + i) * ow..(y + i + 1) * ow];
            for x in 0..ow {
                row[x] += k[i] * line[x];
            }
        }
    });
    (out, ow, oh)
}

fn window() -> Vec<f64> {
    let c = (SSIM_WINDOW / 2) as f64;
    let mut k: Vec<f64> = (0..SSIM_WINDOW).map(|i| (-((i as f64 - c).powi(2)) / (2.0 * SSIM_SIGMA * SSIM_SIGMA)).exp()).collect();
    let s: f64 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= s);
    k
}

pub fn ssim_full(reference: &impl AsRgb8, test: &impl AsRgb8) -> Result<SsimResult, String> {
    let (r, t) = (reference.as_rgb8(), test.as_rgb8());
    same_size(&r, &t)?;
    let (w, h) = (r.width as usize, r.height as usize);
    let x: Vec<f64> = r.luma().iter().map(|v| *v as f64).collect();
    let y: Vec<f64> = t.luma().iter().map(|v| *v as f64).collect();
    let c1 = (0.01f64 * 255.0).powi(2);
    let c2 = (0.03f64 * 255.0).powi(2);
    if w < SSIM_WINDOW || h < SSIM_WINDOW {
        // imagem menor que a janela: uma unica janela uniforme sobre tudo
        let n = (w * h) as f64;
        let (mx, my) = (x.iter().sum::<f64>() / n, y.iter().sum::<f64>() / n);
        let vx = x.iter().map(|v| (v - mx).powi(2)).sum::<f64>() / n;
        let vy = y.iter().map(|v| (v - my).powi(2)).sum::<f64>() / n;
        let cxy = x.iter().zip(&y).map(|(a, b)| (a - mx) * (b - my)).sum::<f64>() / n;
        let s = ((2.0 * mx * my + c1) * (2.0 * cxy + c2)) / ((mx * mx + my * my + c1) * (vx + vy + c2));
        return Ok(SsimResult { mean: s, map: vec![s as f32], map_width: 1, map_height: 1 });
    }
    let k = window();
    let xx: Vec<f64> = x.iter().map(|v| v * v).collect();
    let yy: Vec<f64> = y.iter().map(|v| v * v).collect();
    let xy: Vec<f64> = x.iter().zip(&y).map(|(a, b)| a * b).collect();
    let (mx, ow, oh) = conv_valid(&x, w, h, &k);
    let (my, _, _) = conv_valid(&y, w, h, &k);
    let (sxx, _, _) = conv_valid(&xx, w, h, &k);
    let (syy, _, _) = conv_valid(&yy, w, h, &k);
    let (sxy, _, _) = conv_valid(&xy, w, h, &k);
    let map: Vec<f64> = (0..ow * oh)
        .into_par_iter()
        .map(|i| {
            let vx = sxx[i] - mx[i] * mx[i];
            let vy = syy[i] - my[i] * my[i];
            let cxy = sxy[i] - mx[i] * my[i];
            ((2.0 * mx[i] * my[i] + c1) * (2.0 * cxy + c2)) / ((mx[i] * mx[i] + my[i] * my[i] + c1) * (vx + vy + c2))
        })
        .collect();
    let parts: Vec<f64> = map.par_chunks(8192).map(|c| c.iter().sum::<f64>()).collect();
    let mean = parts.iter().sum::<f64>() / map.len() as f64;
    Ok(SsimResult { mean, map: map.iter().map(|v| *v as f32).collect(), map_width: ow as u32, map_height: oh as u32 })
}

/// SSIM medio (luma, gaussiana 11x11).
pub fn ssim(reference: &impl AsRgb8, test: &impl AsRgb8) -> Result<f64, String> {
    Ok(ssim_full(reference, test)?.mean)
}
