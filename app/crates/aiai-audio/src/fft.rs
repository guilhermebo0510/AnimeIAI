//! FFT radix-2 (tamanho potência de 2) em f32 e janelas. Sem dependências: serve ao espectrograma, à redução de ruído
//! e a qualquer processamento por STFT.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Plano de FFT para um tamanho fixo (tabela de bit-reversal e de fatores de giro).
#[derive(Debug, Clone)]
pub struct Fft {
    n: usize,
    rev: Vec<u32>,
    cos: Vec<f32>,
    sin: Vec<f32>,
}

impl Fft {
    pub fn new(n: usize) -> Fft {
        assert!(n >= 2 && n.is_power_of_two(), "FFT exige potência de 2 (recebi {n})");
        let bits = n.trailing_zeros();
        let rev = (0..n as u32).map(|i| i.reverse_bits() >> (32 - bits)).collect();
        let cos = (0..n / 2).map(|k| (2.0 * PI * k as f64 / n as f64).cos() as f32).collect();
        let sin = (0..n / 2).map(|k| (2.0 * PI * k as f64 / n as f64).sin() as f32).collect();
        Fft { n, rev, cos, sin }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// Transformada direta (sem escala): `X[k] = Σ x[n]·e^(-2πi·kn/N)`.
    pub fn forward(&self, re: &mut [f32], im: &mut [f32]) {
        self.run(re, im, false);
    }

    /// Transformada inversa com escala 1/N.
    pub fn inverse(&self, re: &mut [f32], im: &mut [f32]) {
        self.run(re, im, true);
    }

    fn run(&self, re: &mut [f32], im: &mut [f32], inv: bool) {
        let n = self.n;
        assert!(re.len() == n && im.len() == n, "FFT: buffers com tamanho diferente do plano");
        for i in 0..n {
            let j = self.rev[i] as usize;
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            let mut start = 0;
            while start < n {
                for k in 0..half {
                    let wr = self.cos[k * step];
                    let wi = if inv { self.sin[k * step] } else { -self.sin[k * step] };
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
                start += len;
            }
            len <<= 1;
        }
        if inv {
            let s = 1.0 / n as f32;
            for v in re.iter_mut().chain(im.iter_mut()) {
                *v *= s;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    #[default]
    Hann,
    Hamming,
    Blackman,
    Rectangular,
}

/// Janela periódica (própria para STFT com sobreposição) de `n` amostras.
pub fn window(kind: WindowKind, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let x = 2.0 * PI * i as f64 / n as f64;
            (match kind {
                WindowKind::Hann => 0.5 - 0.5 * x.cos(),
                WindowKind::Hamming => 0.54 - 0.46 * x.cos(),
                WindowKind::Blackman => 0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos(),
                WindowKind::Rectangular => 1.0,
            }) as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_lands_in_one_bin_and_inverse_restores() {
        let n = 1024;
        let fft = Fft::new(n);
        let x: Vec<f32> = (0..n).map(|i| (2.0 * PI * 50.0 * i as f64 / n as f64).sin() as f32).collect();
        let (mut re, mut im) = (x.clone(), vec![0.0; n]);
        fft.forward(&mut re, &mut im);
        let mag = |k: usize| ((re[k] * re[k] + im[k] * im[k]) as f64).sqrt();
        assert!((mag(50) - n as f64 / 2.0).abs() < 1e-2, "{}", mag(50));
        assert!(mag(49) < 1e-2 && mag(60) < 1e-2);
        fft.inverse(&mut re, &mut im);
        let err = re.iter().zip(&x).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(err < 1e-5, "{err}");
    }

    #[test]
    fn parseval_holds() {
        let n = 512;
        let fft = Fft::new(n);
        let x: Vec<f32> = (0..n).map(|i| ((i * 7919) % 101) as f32 / 50.0 - 1.0).collect();
        let e_t: f64 = x.iter().map(|&v| (v as f64).powi(2)).sum();
        let (mut re, mut im) = (x, vec![0.0; n]);
        fft.forward(&mut re, &mut im);
        let e_f: f64 = re.iter().zip(&im).map(|(a, b)| (*a as f64).powi(2) + (*b as f64).powi(2)).sum::<f64>() / n as f64;
        assert!((e_t - e_f).abs() / e_t < 1e-5);
    }
}
