//! Correlacao de fase (translacao global) via FFT 2-D, para medir erro de posicao de camera.
//!
//! Convencao: `(dx, dy)` e o deslocamento de `b` em relacao a `a`, isto e `b(x, y) = a(x - dx, y - dy)`
//! (dx > 0: o conteudo de `b` esta mais a direita; dy > 0: mais abaixo). `peak` e a altura do pico
//! normalizado (1.0 = translacao pura perfeita; ruido/mudanca de conteudo reduz).
//! Pre-processamento: luma, remocao da media, janela de Hann 2-D (reduz vazamento de borda); refinamento
//! subpixel por parabola nos 3 pontos do pico.

use crate::rgb::{same_size, AsRgb8};
use rayon::prelude::*;
use rustfft::{num_complex::Complex, FftPlanner};

fn hann(n: usize) -> Vec<f64> {
    if n < 2 {
        return vec![1.0; n];
    }
    (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos()).collect()
}

fn fft2(buf: &mut [Complex<f64>], w: usize, h: usize, inverse: bool) {
    let mut planner = FftPlanner::<f64>::new();
    let (fw, fh) = if inverse { (planner.plan_fft_inverse(w), planner.plan_fft_inverse(h)) } else { (planner.plan_fft_forward(w), planner.plan_fft_forward(h)) };
    buf.par_chunks_mut(w).for_each(|row| fw.process(row));
    // colunas: transpoe, FFT, transpoe de volta
    let mut t = vec![Complex::new(0.0, 0.0); w * h];
    for y in 0..h {
        for x in 0..w {
            t[x * h + y] = buf[y * w + x];
        }
    }
    t.par_chunks_mut(h).for_each(|col| fh.process(col));
    for y in 0..h {
        for x in 0..w {
            buf[y * w + x] = t[x * h + y];
        }
    }
}

fn prep(luma: &[f32], w: usize, h: usize) -> Vec<Complex<f64>> {
    let mean = luma.iter().map(|v| *v as f64).sum::<f64>() / luma.len() as f64;
    let (hx, hy) = (hann(w), hann(h));
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            out.push(Complex::new((luma[y * w + x] as f64 - mean) * hx[x] * hy[y], 0.0));
        }
    }
    out
}

/// Translacao global de `b` em relacao a `a`: `(dx, dy, peak)` (ver topo do modulo).
pub fn phase_correlation(a: &impl AsRgb8, b: &impl AsRgb8) -> Result<(f64, f64, f64), String> {
    let (ra, rb) = (a.as_rgb8(), b.as_rgb8());
    same_size(&ra, &rb)?;
    let (w, h) = (ra.width as usize, ra.height as usize);
    let mut fa = prep(&ra.luma(), w, h);
    let mut fb = prep(&rb.luma(), w, h);
    fft2(&mut fa, w, h, false);
    fft2(&mut fb, w, h, false);
    let mut r: Vec<Complex<f64>> = fa
        .par_iter()
        .zip(fb.par_iter())
        .map(|(x, y)| {
            let c = y * x.conj();
            let m = c.norm();
            if m < 1e-9 { Complex::new(0.0, 0.0) } else { c / m }
        })
        .collect();
    fft2(&mut r, w, h, true);
    let scale = 1.0 / (w * h) as f64;
    let re: Vec<f64> = r.iter().map(|c| c.re * scale).collect();
    let (mut bi, mut bv) = (0usize, f64::NEG_INFINITY);
    for (i, v) in re.iter().enumerate() {
        if *v > bv {
            bv = *v;
            bi = i;
        }
    }
    let (px, py) = (bi % w, bi / w);
    let at = |x: isize, y: isize| re[(y.rem_euclid(h as isize)) as usize * w + (x.rem_euclid(w as isize)) as usize];
    let sub = |m1: f64, c: f64, p1: f64| -> f64 {
        let den = m1 - 2.0 * c + p1;
        if den.abs() < 1e-12 { 0.0 } else { (0.5 * (m1 - p1) / den).clamp(-0.5, 0.5) }
    };
    let (xi, yi) = (px as isize, py as isize);
    let sx = sub(at(xi - 1, yi), bv, at(xi + 1, yi));
    let sy = sub(at(xi, yi - 1), bv, at(xi, yi + 1));
    let wrap = |p: usize, n: usize| -> f64 { if p > n / 2 { p as f64 - n as f64 } else { p as f64 } };
    Ok((wrap(px, w) + sx, wrap(py, h) + sy, bv))
}
