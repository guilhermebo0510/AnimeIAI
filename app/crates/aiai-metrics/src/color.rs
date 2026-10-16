//! sRGB -> Lab (D65) e ΔE2000 exato (Sharma, Wu, Dalal 2005).

use crate::rgb::{same_size, AsRgb8, Rgb8};
use rayon::prelude::*;

const XN: f64 = 0.950_47;
const YN: f64 = 1.0;
const ZN: f64 = 1.088_83;

pub type Lab = [f64; 3];

#[inline]
pub fn srgb_to_linear(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

#[inline]
pub fn linear_to_srgb(v: f64) -> f64 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn lab_f(t: f64) -> f64 {
    const D: f64 = 6.0 / 29.0;
    if t > D * D * D { t.cbrt() } else { t / (3.0 * D * D) + 4.0 / 29.0 }
}

fn lab_f_inv(t: f64) -> f64 {
    const D: f64 = 6.0 / 29.0;
    if t > D { t * t * t } else { 3.0 * D * D * (t - 4.0 / 29.0) }
}

/// sRGB (0..1, codificado) -> Lab, iluminante D65.
pub fn srgb_to_lab(rgb: [f64; 3]) -> Lab {
    let r = srgb_to_linear(rgb[0]);
    let g = srgb_to_linear(rgb[1]);
    let b = srgb_to_linear(rgb[2]);
    let x = 0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175_0 * b;
    let z = 0.019_333_9 * r + 0.119_192_0 * g + 0.950_304_1 * b;
    let fx = lab_f(x / XN);
    let fy = lab_f(y / YN);
    let fz = lab_f(z / ZN);
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Lab (D65) -> sRGB (0..1, codificado, recortado).
pub fn lab_to_srgb(lab: Lab) -> [f64; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;
    let x = XN * lab_f_inv(fx);
    let y = YN * lab_f_inv(fy);
    let z = ZN * lab_f_inv(fz);
    let r = 3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z;
    let g = -0.969_266_0 * x + 1.876_010_8 * y + 0.041_556_0 * z;
    let b = 0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z;
    [linear_to_srgb(r).clamp(0.0, 1.0), linear_to_srgb(g).clamp(0.0, 1.0), linear_to_srgb(b).clamp(0.0, 1.0)]
}

pub fn rgb8_to_lab(p: [u8; 3]) -> Lab {
    srgb_to_lab([p[0] as f64 / 255.0, p[1] as f64 / 255.0, p[2] as f64 / 255.0])
}

/// Lab de todos os pixels (rayon). Usa tabela de linearização de 256 entradas.
pub fn image_to_lab(img: &Rgb8) -> Vec<[f32; 3]> {
    let lut: Vec<f64> = (0..256).map(|i| srgb_to_linear(i as f64 / 255.0)).collect();
    img.data
        .par_chunks_exact(3)
        .map(|p| {
            let (r, g, b) = (lut[p[0] as usize], lut[p[1] as usize], lut[p[2] as usize]);
            let x = 0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b;
            let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175_0 * b;
            let z = 0.019_333_9 * r + 0.119_192_0 * g + 0.950_304_1 * b;
            let (fx, fy, fz) = (lab_f(x / XN), lab_f(y / YN), lab_f(z / ZN));
            [(116.0 * fy - 16.0) as f32, (500.0 * (fx - fy)) as f32, (200.0 * (fy - fz)) as f32]
        })
        .collect()
}

/// ΔE2000 conforme Sharma, Wu e Dalal (2005), kL = kC = kH = 1.
pub fn delta_e2000(lab1: Lab, lab2: Lab) -> f64 {
    let (l1, a1, b1) = (lab1[0], lab1[1], lab1[2]);
    let (l2, a2, b2) = (lab2[0], lab2[1], lab2[2]);
    let c1 = a1.hypot(b1);
    let c2 = a2.hypot(b2);
    let cbar = (c1 + c2) / 2.0;
    let cbar7 = cbar.powi(7);
    let g = 0.5 * (1.0 - (cbar7 / (cbar7 + 25f64.powi(7))).sqrt());
    let a1p = (1.0 + g) * a1;
    let a2p = (1.0 + g) * a2;
    let c1p = a1p.hypot(b1);
    let c2p = a2p.hypot(b2);
    let hp = |b: f64, ap: f64| -> f64 {
        if b == 0.0 && ap == 0.0 {
            0.0
        } else {
            let h = b.atan2(ap).to_degrees();
            if h < 0.0 { h + 360.0 } else { h }
        }
    };
    let h1p = hp(b1, a1p);
    let h2p = hp(b2, a2p);

    let dlp = l2 - l1;
    let dcp = c2p - c1p;
    let dhp_deg = if c1p * c2p == 0.0 {
        0.0
    } else {
        let d = h2p - h1p;
        if d.abs() <= 180.0 {
            d
        } else if d > 180.0 {
            d - 360.0
        } else {
            d + 360.0
        }
    };
    let dhp = 2.0 * (c1p * c2p).sqrt() * (dhp_deg.to_radians() / 2.0).sin();

    let lbp = (l1 + l2) / 2.0;
    let cbp = (c1p + c2p) / 2.0;
    let hbp = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let rad = |d: f64| d.to_radians();
    let t = 1.0 - 0.17 * rad(hbp - 30.0).cos() + 0.24 * rad(2.0 * hbp).cos() + 0.32 * rad(3.0 * hbp + 6.0).cos()
        - 0.20 * rad(4.0 * hbp - 63.0).cos();
    let dtheta = 30.0 * (-((hbp - 275.0) / 25.0).powi(2)).exp();
    let cbp7 = cbp.powi(7);
    let rc = 2.0 * (cbp7 / (cbp7 + 25f64.powi(7))).sqrt();
    let sl = 1.0 + 0.015 * (lbp - 50.0).powi(2) / (20.0 + (lbp - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cbp;
    let sh = 1.0 + 0.015 * cbp * t;
    let rt = -(2.0 * rad(dtheta)).sin() * rc;
    let (x, y, z) = (dlp / sl, dcp / sc, dhp / sh);
    (x * x + y * y + z * z + rt * y * z).sqrt()
}

/// ΔE2000 por pixel (mesmo tamanho). `mask` seleciona os pixels que entram na média.
pub fn delta_e_map(reference: &impl AsRgb8, test: &impl AsRgb8) -> Result<Vec<f32>, String> {
    let (r, t) = (reference.as_rgb8(), test.as_rgb8());
    same_size(&r, &t)?;
    let (lr, lt) = (image_to_lab(&r), image_to_lab(&t));
    Ok(lr
        .par_iter()
        .zip(lt.par_iter())
        .map(|(a, b)| delta_e2000([a[0] as f64, a[1] as f64, a[2] as f64], [b[0] as f64, b[1] as f64, b[2] as f64]) as f32)
        .collect())
}

/// ΔE2000 médio (opcionalmente restrito à máscara). Máscara vazia -> 0.
pub fn mean_delta_e(reference: &impl AsRgb8, test: &impl AsRgb8, mask: Option<&[bool]>) -> Result<f64, String> {
    let map = delta_e_map(reference, test)?;
    if let Some(m) = mask {
        if m.len() != map.len() {
            return Err("máscara com tamanho diferente da imagem".into());
        }
    }
    Ok(mean_masked(&map, mask))
}

/// Soma determinística (blocos fixos somados em ordem; independe do escalonamento do rayon).
pub(crate) fn det_sum_f32(v: &[f32]) -> f64 {
    let parts: Vec<f64> = v.par_chunks(8192).map(|c| c.iter().map(|x| *x as f64).sum::<f64>()).collect();
    parts.iter().sum()
}

pub(crate) fn mean_masked(map: &[f32], mask: Option<&[bool]>) -> f64 {
    match mask {
        Some(m) => {
            let parts: Vec<(f64, u64)> = map
                .par_chunks(8192)
                .zip(m.par_chunks(8192))
                .map(|(c, k)| {
                    let mut s = 0.0;
                    let mut n = 0u64;
                    for (v, on) in c.iter().zip(k) {
                        if *on {
                            s += *v as f64;
                            n += 1;
                        }
                    }
                    (s, n)
                })
                .collect();
            let (s, n) = parts.iter().fold((0.0, 0u64), |a, b| (a.0 + b.0, a.1 + b.1));
            if n == 0 { 0.0 } else { s / n as f64 }
        }
        None => det_sum_f32(map) / map.len().max(1) as f64,
    }
}
