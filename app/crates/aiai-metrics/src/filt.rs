//! Filtros e transformada de distancia compartilhados (deterministicos).

use rayon::prelude::*;

pub fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let r = (sigma * 3.0).ceil().max(1.0) as i32;
    let mut k: Vec<f32> = (-r..=r).map(|i| (-(i as f32).powi(2) / (2.0 * sigma * sigma)).exp()).collect();
    let s: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= s);
    k
}

/// Desfoque gaussiano separavel com extremidades replicadas.
pub fn gaussian_blur(src: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    let k = gaussian_kernel(sigma);
    let r = (k.len() / 2) as isize;
    let mut tmp = vec![0f32; w * h];
    tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let line = &src[y * w..(y + 1) * w];
        for x in 0..w {
            let mut acc = 0.0;
            for (i, kv) in k.iter().enumerate() {
                let xx = (x as isize + i as isize - r).clamp(0, w as isize - 1) as usize;
                acc += kv * line[xx];
            }
            row[x] = acc;
        }
    });
    let mut out = vec![0f32; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (i, kv) in k.iter().enumerate() {
            let yy = (y as isize + i as isize - r).clamp(0, h as isize - 1) as usize;
            let line = &tmp[yy * w..(yy + 1) * w];
            for x in 0..w {
                row[x] += kv * line[x];
            }
        }
    });
    out
}

/// Transformada de distancia euclidiana exata (Felzenszwalb e Huttenlocher): para cada pixel,
/// distancia ao pixel `true` mais proximo (0 nos proprios `true`). Sem nenhum `true`: valor muito grande.
pub fn distance_transform(fg: &[bool], w: usize, h: usize) -> Vec<f32> {
    const INF: f32 = 1e9;
    let mut d: Vec<f32> = fg.iter().map(|&b| if b { 0.0 } else { INF }).collect();
    let cols: Vec<Vec<f32>> = (0..w)
        .into_par_iter()
        .map(|x| {
            let f: Vec<f32> = (0..h).map(|y| d[y * w + x]).collect();
            edt_1d(&f)
        })
        .collect();
    for (x, col) in cols.iter().enumerate() {
        for y in 0..h {
            d[y * w + x] = col[y];
        }
    }
    d.par_chunks_mut(w).for_each(|row| {
        let r = edt_1d(&row.to_vec());
        row.copy_from_slice(&r);
    });
    d.par_iter_mut().for_each(|v| *v = v.sqrt());
    d
}

/// DT 1-D de quadrados (envelope inferior de parabolas). `f` em distancia ao quadrado.
fn edt_1d(f: &[f32]) -> Vec<f32> {
    let n = f.len();
    let mut v = vec![0usize; n];
    let mut z = vec![0f32; n + 1];
    let mut k = 0usize;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    for q in 1..n {
        let mut s;
        loop {
            let p = v[k];
            s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                break;
            }
        }
        if s <= z[k] {
            // k == 0: a nova parabola domina tudo
            v[0] = q;
            z[0] = f32::NEG_INFINITY;
            z[1] = f32::INFINITY;
        } else {
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f32::INFINITY;
        }
    }
    let mut out = vec![0f32; n];
    k = 0;
    for q in 0..n {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f32 - p as f32;
        out[q] = dq * dq + f[p];
    }
    out
}

/// Media em janela (2r+1)^2 via imagem integral (f64); nas bordas a janela e recortada.
pub fn box_mean(src: &[f64], w: usize, h: usize, r: usize) -> Vec<f64> {
    let mut integ = vec![0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0.0;
        for x in 0..w {
            row += src[y * w + x];
            integ[(y + 1) * (w + 1) + x + 1] = integ[y * (w + 1) + x + 1] + row;
        }
    }
    let mut out = vec![0f64; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let y0 = y.saturating_sub(r);
        let y1 = (y + r + 1).min(h);
        for x in 0..w {
            let x0 = x.saturating_sub(r);
            let x1 = (x + r + 1).min(w);
            let s = integ[y1 * (w + 1) + x1] - integ[y0 * (w + 1) + x1] - integ[y1 * (w + 1) + x0] + integ[y0 * (w + 1) + x0];
            row[x] = s / ((x1 - x0) * (y1 - y0)) as f64;
        }
    });
    out
}

/// Dilatacao por disco de raio `r` pixels (usa a transformada de distancia).
pub fn dilate(mask: &[bool], w: usize, h: usize, r: f32) -> Vec<bool> {
    let d = distance_transform(mask, w, h);
    d.iter().map(|&v| v <= r + 1e-4).collect()
}
