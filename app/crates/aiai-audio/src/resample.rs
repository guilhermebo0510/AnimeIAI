//! Reamostragem sinc polifásica com janela Kaiser (β = 9, ~100 dB de rejeição). Racional exata via mdc das taxas.

use crate::buf::AudioBuf;

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let q = x * x / 4.0;
    for k in 1..60 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < 1e-14 * sum {
            break;
        }
    }
    sum
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        let a = std::f64::consts::PI * x;
        a.sin() / a
    }
}

/// Meia largura do filtro em cruzamentos por zero (do sinal de menor banda).
const HALF_ZC: f64 = 24.0;
const KAISER_BETA: f64 = 9.0;

pub fn resample(input: &AudioBuf, to_rate: u32) -> AudioBuf {
    if input.rate == to_rate || input.frames() == 0 {
        return AudioBuf { rate: to_rate, ch: input.ch.clone() };
    }
    let g = gcd(input.rate as u64, to_rate as u64);
    let up = (to_rate as u64 / g) as usize;
    let down = (input.rate as u64 / g) as usize;
    // Corte normalizado à taxa de entrada: 1 = Nyquist da entrada. Margem de 2% abaixo do Nyquist mais baixo.
    let fc = (up as f64 / down as f64).min(1.0) * 0.98;
    let half = (HALF_ZC / fc).ceil() as isize;
    let taps = (2 * half) as usize;
    let i0 = bessel_i0(KAISER_BETA);
    // Tabela de coeficientes por fase (cada fase soma 1: ganho DC unitário).
    let mut table = vec![0.0f32; up * taps];
    for p in 0..up {
        let frac = p as f64 / up as f64;
        let mut acc = 0.0f64;
        let mut row = vec![0.0f64; taps];
        for (k, slot) in row.iter_mut().enumerate() {
            let j = k as isize - half + 1; // -half+1 ..= half
            let t = j as f64 - frac;
            let w = {
                let r = t / half as f64;
                if r.abs() >= 1.0 { 0.0 } else { bessel_i0(KAISER_BETA * (1.0 - r * r).sqrt()) / i0 }
            };
            let h = fc * sinc(fc * t) * w;
            *slot = h;
            acc += h;
        }
        for (k, v) in row.iter().enumerate() {
            table[p * taps + k] = (v / acc) as f32;
        }
    }
    let n_in = input.frames();
    let n_out = ((n_in as u128 * up as u128) / down as u128) as usize;
    let ch = input
        .ch
        .iter()
        .map(|x| {
            let mut out = vec![0.0f32; n_out];
            for (n, o) in out.iter_mut().enumerate() {
                let pos = n as u128 * down as u128;
                let base = (pos / up as u128) as isize;
                let p = (pos % up as u128) as usize;
                let row = &table[p * taps..(p + 1) * taps];
                let start = base - half + 1;
                let mut acc = 0.0f32;
                if start >= 0 && (start as usize + taps) <= n_in {
                    let s = &x[start as usize..start as usize + taps];
                    for (a, b) in s.iter().zip(row) {
                        acc += a * b;
                    }
                } else {
                    for (k, c) in row.iter().enumerate() {
                        let idx = start + k as isize;
                        if idx >= 0 && (idx as usize) < n_in {
                            acc += x[idx as usize] * c;
                        }
                    }
                }
                *o = acc;
            }
            out
        })
        .collect();
    AudioBuf { rate: to_rate, ch }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, f: f64, secs: f64) -> AudioBuf {
        let n = (rate as f64 * secs) as usize;
        AudioBuf::mono(rate, (0..n).map(|i| (0.5 * (2.0 * std::f64::consts::PI * f * i as f64 / rate as f64).sin()) as f32).collect())
    }

    fn rms_mid(b: &AudioBuf) -> f64 {
        let n = b.frames();
        let s = &b.ch[0][n / 4..3 * n / 4];
        (s.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / s.len() as f64).sqrt()
    }

    #[test]
    fn preserves_level_and_length() {
        for (from, to) in [(44_100u32, 48_000u32), (24_000, 48_000), (48_000, 24_000), (22_050, 48_000)] {
            let o = resample(&sine(from, 1000.0, 1.0), to);
            assert_eq!(o.rate, to);
            assert!((o.frames() as i64 - to as i64).abs() <= 1, "{from}->{to}: {}", o.frames());
            assert!((rms_mid(&o) - 0.5 / 2f64.sqrt()).abs() < 0.005, "{from}->{to}: {}", rms_mid(&o));
        }
    }

    #[test]
    fn rejects_aliases_when_downsampling() {
        // 18 kHz a 48 kHz → 24 kHz: acima do novo Nyquist (12 kHz) → deve sumir (> 60 dB).
        let o = resample(&sine(48_000, 18_000.0, 1.0), 24_000);
        assert!(rms_mid(&o) < 0.5 / 2f64.sqrt() * 1e-3, "{}", rms_mid(&o));
    }
}
