//! Paleta por k-means em Lab com semeadura deterministica (k-means++, RNG de semente fixa).

use crate::color::{delta_e2000, image_to_lab, lab_to_srgb, rgb8_to_lab};
use crate::rgb::AsRgb8;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

pub type Rgb = [u8; 3];

pub const PALETTE_SEED: u64 = 0x5EED_A11E_2024;
const MAX_FIT_SAMPLES: usize = 60_000;
const MAX_ITERS: usize = 60;

struct SplitMix(u64);
impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn d2(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let (x, y, z) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    x * x + y * y + z * z
}

fn fit_centers(pts: &[[f32; 3]], k: usize) -> Vec<[f32; 3]> {
    let k = k.min(pts.len());
    // subamostra deterministica por passo fixo
    let step = pts.len().div_ceil(MAX_FIT_SAMPLES).max(1);
    let fit: Vec<[f32; 3]> = pts.iter().step_by(step).copied().collect();

    let mut rng = SplitMix(PALETTE_SEED);
    let mut centers: Vec<[f32; 3]> = vec![fit[(rng.next() % fit.len() as u64) as usize]];
    let mut mind: Vec<f32> = fit.iter().map(|p| d2(p, &centers[0])).collect();
    while centers.len() < k {
        let total: f64 = mind.iter().map(|v| *v as f64).sum();
        let idx = if total <= 0.0 {
            (rng.next() % fit.len() as u64) as usize
        } else {
            let mut t = rng.unit() * total;
            let mut pick = fit.len() - 1;
            for (i, v) in mind.iter().enumerate() {
                t -= *v as f64;
                if t <= 0.0 {
                    pick = i;
                    break;
                }
            }
            pick
        };
        let c = fit[idx];
        for (i, p) in fit.iter().enumerate() {
            mind[i] = mind[i].min(d2(p, &c));
        }
        centers.push(c);
    }

    let mut assign = vec![0usize; fit.len()];
    for _ in 0..MAX_ITERS {
        let mut changed = false;
        for (i, p) in fit.iter().enumerate() {
            let mut best = (f32::INFINITY, 0);
            for (c, ctr) in centers.iter().enumerate() {
                let d = d2(p, ctr);
                if d < best.0 {
                    best = (d, c);
                }
            }
            if assign[i] != best.1 {
                assign[i] = best.1;
                changed = true;
            }
        }
        let mut sums = vec![[0f64; 3]; k];
        let mut cnt = vec![0usize; k];
        for (p, a) in fit.iter().zip(&assign) {
            for c in 0..3 {
                sums[*a][c] += p[c] as f64;
            }
            cnt[*a] += 1;
        }
        for c in 0..k {
            if cnt[c] > 0 {
                centers[c] = [(sums[c][0] / cnt[c] as f64) as f32, (sums[c][1] / cnt[c] as f64) as f32, (sums[c][2] / cnt[c] as f64) as f32];
            }
        }
        if !changed {
            break;
        }
    }

    centers
}

/// Cores dominantes (RGB, fracao dos pixels da mascara), da mais para a menos frequente.
/// `mask = None` usa todos os pixels. O agrupamento e feito em Lab (distancia euclidiana, ~CIE76).
pub fn palette(img: &impl AsRgb8, k: usize, mask: Option<&[bool]>) -> Vec<(Rgb, f32)> {
    let rgb = img.as_rgb8();
    let lab = image_to_lab(&rgb);
    let pts: Vec<[f32; 3]> = match mask {
        Some(m) => lab.iter().zip(m).filter(|(_, on)| **on).map(|(p, _)| *p).collect(),
        None => lab,
    };
    if pts.is_empty() || k == 0 {
        return vec![];
    }
    let k = k.min(pts.len());
    let centers = fit_centers(&pts, k);
    // fracoes sobre todos os pixels da mascara
    let counts = pts
        .par_chunks(16384)
        .map(|chunk| {
            let mut cnt = vec![0u64; k];
            for p in chunk {
                let mut best = (f32::INFINITY, 0);
                for (c, ctr) in centers.iter().enumerate() {
                    let d = d2(p, ctr);
                    if d < best.0 {
                        best = (d, c);
                    }
                }
                cnt[best.1] += 1;
            }
            cnt
        })
        .reduce(|| vec![0u64; k], |mut a, b| {
            for i in 0..k {
                a[i] += b[i];
            }
            a
        });
    let total = pts.len() as f32;
    let mut out: Vec<(Rgb, f32, f32)> = centers
        .iter()
        .zip(&counts)
        .filter(|(_, n)| **n > 0)
        .map(|(c, n)| {
            let s = lab_to_srgb([c[0] as f64, c[1] as f64, c[2] as f64]);
            ([(s[0] * 255.0 + 0.5) as u8, (s[1] * 255.0 + 0.5) as u8, (s[2] * 255.0 + 0.5) as u8], *n as f32 / total, c[0])
        })
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.total_cmp(&b.2)));
    out.into_iter().map(|(c, f, _)| (c, f)).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaletteMatch {
    pub a: Rgb,
    pub a_fraction: f32,
    pub b: Rgb,
    pub b_fraction: f32,
    pub delta_e: f64,
}

/// Para cada cor de `a` (referencia) a cor mais proxima de `b` por ΔE2000 e o ΔE correspondente.
pub fn match_palette(a: &[(Rgb, f32)], b: &[(Rgb, f32)]) -> Vec<PaletteMatch> {
    a.iter()
        .map(|(ca, fa)| {
            let la = rgb8_to_lab(*ca);
            let mut best: Option<PaletteMatch> = None;
            for (cb, fb) in b {
                let de = delta_e2000(la, rgb8_to_lab(*cb));
                if best.map(|m| de < m.delta_e).unwrap_or(true) {
                    best = Some(PaletteMatch { a: *ca, a_fraction: *fa, b: *cb, b_fraction: *fb, delta_e: de });
                }
            }
            best.unwrap_or(PaletteMatch { a: *ca, a_fraction: *fa, b: [0, 0, 0], b_fraction: 0.0, delta_e: f64::INFINITY })
        })
        .collect()
}

fn nearest(p: &[f32; 3], centers: &[[f32; 3]]) -> usize {
    let mut best = (f32::INFINITY, 0);
    for (c, ctr) in centers.iter().enumerate() {
        let d = d2(p, ctr);
        if d < best.0 {
            best = (d, c);
        }
    }
    best.1
}

/// Deslocamento de cor por grupo de cor da REFERENCIA: os grupos (k-means em Lab) sao ajustados nos pixels da
/// referencia (dentro de `mask`); para cada grupo compara-se a cor media da referencia com a cor media do teste
/// nos MESMOS pixels (ΔE2000). E o que o protocolo pede ("cada cor do settei casa com a referencia") sem o
/// ruido de dois k-means independentes caírem em particoes diferentes. Ordenado por fracao decrescente.
pub fn palette_shift(reference: &impl AsRgb8, test: &impl AsRgb8, k: usize, mask: Option<&[bool]>) -> Result<Vec<PaletteMatch>, String> {
    let (r, t) = (reference.as_rgb8(), test.as_rgb8());
    crate::rgb::same_size(&r, &t)?;
    let (lr, lt) = (image_to_lab(&r), image_to_lab(&t));
    let idx: Vec<usize> = match mask {
        Some(m) => (0..lr.len()).filter(|&i| m[i]).collect(),
        None => (0..lr.len()).collect(),
    };
    if idx.is_empty() || k == 0 {
        return Ok(vec![]);
    }
    let pts: Vec<[f32; 3]> = idx.iter().map(|&i| lr[i]).collect();
    let step = pts.len().div_ceil(MAX_FIT_SAMPLES).max(1);
    let fit: Vec<[f32; 3]> = pts.iter().step_by(step).copied().collect();
    let centers = fit_centers(&fit, k.min(fit.len()));
    let kk = centers.len();
    let mut sr = vec![[0f64; 3]; kk];
    let mut st = vec![[0f64; 3]; kk];
    let mut cnt = vec![0u64; kk];
    for &i in &idx {
        let c = nearest(&lr[i], &centers);
        for ch in 0..3 {
            sr[c][ch] += lr[i][ch] as f64;
            st[c][ch] += lt[i][ch] as f64;
        }
        cnt[c] += 1;
    }
    let to_rgb = |lab: [f64; 3]| -> Rgb {
        let s = lab_to_srgb(lab);
        [(s[0] * 255.0 + 0.5) as u8, (s[1] * 255.0 + 0.5) as u8, (s[2] * 255.0 + 0.5) as u8]
    };
    let total = idx.len() as f32;
    let mut out: Vec<(PaletteMatch, f32)> = (0..kk)
        .filter(|&c| cnt[c] > 0)
        .map(|c| {
            let n = cnt[c] as f64;
            let la = [sr[c][0] / n, sr[c][1] / n, sr[c][2] / n];
            let lb = [st[c][0] / n, st[c][1] / n, st[c][2] / n];
            let f = cnt[c] as f32 / total;
            (PaletteMatch { a: to_rgb(la), a_fraction: f, b: to_rgb(lb), b_fraction: f, delta_e: delta_e2000(la, lb) }, la[0] as f32)
        })
        .collect();
    out.sort_by(|x, y| y.0.a_fraction.total_cmp(&x.0.a_fraction).then(x.1.total_cmp(&y.1)));
    Ok(out.into_iter().map(|(m, _)| m).collect())
}
