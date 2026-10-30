//! Máscaras desenhadas na camada (fase 3, C2.2): caminho fechado com alças → cobertura antisserrilhada na imagem de
//! saída, expansão (campo de distância), borda suave (desfoque) e a combinação em ordem (somar, subtrair, interseção,
//! diferença), como as máscaras do After Effects.

use aiai_core::{ShapeMask, ShapeMaskMode};
use aiai_paint::Affine;

/// Cobertura 0..1 de todas as máscaras, `w × h`, com os pontos (espaço da camada) levados por `xf` à saída. `None` sem
/// máscara.
pub fn coverage(masks: &[ShapeMask], xf: &Affine, w: u32, h: u32) -> Option<Vec<f32>> {
    if masks.is_empty() || w == 0 || h == 0 {
        return None;
    }
    let [a, b, _, d, e, _] = xf.0;
    let k = (a * e - b * d).abs().sqrt();
    let k = if k.is_finite() && k > 0.0 { k } else { 1.0 };
    let n = (w * h) as usize;
    // a primeira que subtrai ou corta parte do "tudo visível"
    let mut acc = match masks[0].mode {
        ShapeMaskMode::Subtract | ShapeMaskMode::Intersect => vec![1.0f32; n],
        _ => vec![0.0f32; n],
    };
    for m in masks {
        let mut c = one_mask(m, xf, k, w, h);
        let op = m.opacity.clamp(0.0, 1.0);
        for v in c.iter_mut() {
            let x = if m.inverted { 1.0 - *v } else { *v };
            *v = x * op;
        }
        for (o, &v) in acc.iter_mut().zip(&c) {
            *o = match m.mode {
                ShapeMaskMode::Add => *o + v - *o * v,
                ShapeMaskMode::Subtract => *o * (1.0 - v),
                ShapeMaskMode::Intersect => *o * v,
                ShapeMaskMode::Difference => (*o - v).abs(),
            };
        }
    }
    Some(acc)
}

/// Pontos do caminho achatado (segmentos cúbicos entre pontos vizinhos, fechado), já na saída.
pub fn flatten(m: &ShapeMask, xf: &Affine) -> Vec<[f32; 2]> {
    let p = &m.points;
    let n = p.len();
    let mut out = Vec::new();
    if n < 2 {
        return out;
    }
    for i in 0..n {
        let (q0, q1) = (p[i], p[(i + 1) % n]);
        let b0 = q0.pos;
        let b1 = [q0.pos[0] + q0.h_out[0], q0.pos[1] + q0.h_out[1]];
        let b2 = [q1.pos[0] + q1.h_in[0], q1.pos[1] + q1.h_in[1]];
        let b3 = q1.pos;
        let straight = q0.h_out == [0.0, 0.0] && q1.h_in == [0.0, 0.0];
        let steps = if straight { 1 } else { 24 };
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let u = 1.0 - t;
            let x = u * u * u * b0[0] + 3.0 * u * u * t * b1[0] + 3.0 * u * t * t * b2[0] + t * t * t * b3[0];
            let y = u * u * u * b0[1] + 3.0 * u * u * t * b1[1] + 3.0 * u * t * t * b2[1] + t * t * t * b3[1];
            let (ox, oy) = xf.apply(x, y);
            out.push([ox, oy]);
        }
    }
    out
}

fn one_mask(m: &ShapeMask, xf: &Affine, k: f32, w: u32, h: u32) -> Vec<f32> {
    let n = (w * h) as usize;
    let mut c = vec![0.0f32; n];
    let poly = flatten(m, xf);
    if poly.len() >= 3 {
        if let Some(pc) = aiai_paint::poly::fill_polygons(&[poly], false, w, h) {
            for y in 0..pc.height as i32 {
                for x in 0..pc.width as i32 {
                    let (cx, cy) = (pc.x0 + x, pc.y0 + y);
                    if cx >= 0 && cy >= 0 && (cx as u32) < w && (cy as u32) < h {
                        c[cy as usize * w as usize + cx as usize] = pc.cov[(y as u32 * pc.width + x as u32) as usize].clamp(0.0, 1.0);
                    }
                }
            }
        }
    }
    let exp = m.expansion * k;
    if exp.abs() >= 0.25 {
        c = expand(&c, w as usize, h as usize, exp);
    }
    let fe = m.feather * k;
    if fe >= 0.5 {
        // desvio ~ metade da borda: a rampa vai de ~0 a ~1 numa largura de `feather` px de cada lado da borda
        c = blur(&c, w as usize, h as usize, fe / 2.0);
    }
    c
}

/// Expansão (`e` > 0 cresce, < 0 encolhe) pela distância à borda, com 1 px de rampa (antisserrilhado).
fn expand(c: &[f32], w: usize, h: usize, e: f32) -> Vec<f32> {
    let inside: Vec<bool> = c.iter().map(|&v| v >= 0.5).collect();
    if e > 0.0 {
        // distância de cada pixel de fora até o de dentro mais perto
        let d = edt(&inside, w, h);
        d.iter().zip(&inside).map(|(&dd, &i)| if i { 1.0 } else { (e - dd.sqrt() + 0.5).clamp(0.0, 1.0) }).collect()
    } else {
        let outside: Vec<bool> = inside.iter().map(|&i| !i).collect();
        let d = edt(&outside, w, h);
        d.iter().zip(&inside).map(|(&dd, &i)| if !i { 0.0 } else { (dd.sqrt() - (-e) - 0.5 + 1.0).clamp(0.0, 1.0) }).collect()
    }
}

/// Distância euclidiana ao quadrado até o pixel `true` mais perto (Felzenszwalb e Huttenlocher, separável).
fn edt(fg: &[bool], w: usize, h: usize) -> Vec<f32> {
    const INF: f32 = 1e20;
    let mut g: Vec<f32> = fg.iter().map(|&b| if b { 0.0 } else { INF }).collect();
    let mut f = vec![0.0f32; w.max(h)];
    let mut out = vec![0.0f32; w.max(h)];
    for x in 0..w {
        for y in 0..h {
            f[y] = g[y * w + x];
        }
        dt1(&f[..h], &mut out[..h]);
        for y in 0..h {
            g[y * w + x] = out[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&g[y * w..y * w + w]);
        dt1(&f[..w], &mut out[..w]);
        g[y * w..y * w + w].copy_from_slice(&out[..w]);
    }
    g
}

fn dt1(f: &[f32], d: &mut [f32]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f32; n + 1];
    let sect = |q: usize, p: usize| ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
    let mut k = 0usize;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    for q in 1..n {
        let mut s = sect(q, v[k]);
        while s <= z[k] {
            k -= 1;
            s = sect(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f32::INFINITY;
    }
    k = 0;
    for q in 0..n {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        d[q] = (q as f32 - p as f32).powi(2) + f[p];
    }
}

/// Desfoque gaussiano aproximado (três caixas por eixo).
fn blur(c: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    // raio das três caixas para o desvio pedido
    let r = (((12.0 * sigma * sigma / 3.0) + 1.0).sqrt() / 2.0).floor().max(1.0) as usize;
    let mut a = c.to_vec();
    let mut b = vec![0.0f32; c.len()];
    for _ in 0..3 {
        box_h(&a, &mut b, w, h, r);
        box_v(&b, &mut a, w, h, r);
    }
    a
}

fn box_h(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for y in 0..h {
        let row = &src[y * w..y * w + w];
        let at = |x: isize| row[x.clamp(0, w as isize - 1) as usize];
        let mut s: f32 = (-(r as isize)..=r as isize).map(at).sum();
        for x in 0..w {
            dst[y * w + x] = s * norm;
            s += at(x as isize + r as isize + 1) - at(x as isize - r as isize);
        }
    }
}

fn box_v(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for x in 0..w {
        let at = |y: isize| src[y.clamp(0, h as isize - 1) as usize * w + x];
        let mut s: f32 = (-(r as isize)..=r as isize).map(at).sum();
        for y in 0..h {
            dst[y * w + x] = s * norm;
            s += at(y as isize + r as isize + 1) - at(y as isize - r as isize);
        }
    }
}
