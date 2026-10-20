//! Redução de ruído por subtração espectral: o perfil do ruído vem de um trecho só com ruído (média de potência por bin
//! de uma STFT) e o efeito aplica um ganho por bin e por quadro (STFT com janela raiz-de-Hann e sobreposição de 75%,
//! soma e sobreposição). Processa em fluxo; a latência é o tamanho da FFT.

use super::{Effect, Rt};
use crate::buf::AudioBuf;
use crate::fft::Fft;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

pub const DEFAULT_FFT: usize = 2048;

fn sqrt_hann(n: usize) -> Vec<f32> {
    (0..n).map(|i| (0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()).sqrt() as f32).collect()
}

/// Potência média por bin (`fft_size/2 + 1` valores) de um trecho de ruído.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct NoiseProfile {
    pub fft_size: usize,
    pub sample_rate: u32,
    pub frames_used: usize,
    pub mean_power: Vec<f32>,
}

impl NoiseProfile {
    /// Mede o perfil em `buf` inteiro (passe o trecho já recortado). `None` se for mais curto que a FFT.
    pub fn from_audio(buf: &AudioBuf, fft_size: usize) -> Option<NoiseProfile> {
        let n = fft_size.next_power_of_two().max(256);
        if buf.frames() < n || buf.channels() == 0 {
            return None;
        }
        let hop = n / 4;
        let fft = Fft::new(n);
        let win = sqrt_hann(n);
        let bins = n / 2 + 1;
        let mut acc = vec![0.0f64; bins];
        let mut frames = 0usize;
        let (mut re, mut im) = (vec![0.0f32; n], vec![0.0f32; n]);
        for x in &buf.ch {
            let mut start = 0;
            while start + n <= x.len() {
                for k in 0..n {
                    re[k] = x[start + k] * win[k];
                    im[k] = 0.0;
                }
                fft.forward(&mut re, &mut im);
                for k in 0..bins {
                    acc[k] += (re[k] * re[k] + im[k] * im[k]) as f64;
                }
                frames += 1;
                start += hop;
            }
        }
        if frames == 0 {
            return None;
        }
        Some(NoiseProfile { fft_size: n, sample_rate: buf.rate, frames_used: frames, mean_power: acc.iter().map(|v| (v / frames as f64) as f32).collect() })
    }

    /// Potência por bin para uma taxa de amostragem diferente (reamostra o eixo de frequência por interpolação linear).
    fn for_rate(&self, rate: u32) -> Vec<f32> {
        if rate == self.sample_rate {
            return self.mean_power.clone();
        }
        let ratio = rate as f64 / self.sample_rate.max(1) as f64;
        let last = self.mean_power.len() - 1;
        (0..=last)
            .map(|k| {
                let pos = k as f64 * ratio;
                if pos >= last as f64 {
                    return *self.mean_power.last().unwrap_or(&0.0);
                }
                let (i, f) = (pos.floor() as usize, pos.fract() as f32);
                self.mean_power[i] * (1.0 - f) + self.mean_power[(i + 1).min(last)] * f
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct NoiseReduction {
    /// Perfil do ruído; sem ele o efeito não faz nada (use `NoiseProfile::from_audio` num trecho só com ruído).
    pub profile: Option<NoiseProfile>,
    /// Atenuação máxima por bin (dB).
    pub reduction_db: f32,
    /// Fator de sobre-subtração do ruído estimado (1 = exato; maior remove mais e arrisca artefatos).
    pub sensitivity: f32,
    /// Suavização do ganho entre bins vizinhos (largura da média, em bins; 1 = nenhuma).
    pub freq_smoothing: usize,
    /// Tempo de queda do ganho (ms): evita o ruído musical que nasce de ganhos que piscam.
    pub decay_ms: f32,
    #[serde(skip)]
    #[schemars(skip)]
    pub st: Rt<Option<NrState>>,
}

#[derive(Debug, Clone)]
struct NrChan {
    inbuf: Vec<f32>,
    acc: Vec<f32>,
    out_block: Vec<f32>,
    prev_gain: Vec<f32>,
}

#[derive(Debug, Clone)]
#[doc(hidden)]
pub struct NrState {
    channels: usize,
    rate: u32,
    n: usize,
    hop: usize,
    fft: Fft,
    win: Vec<f32>,
    noise: Vec<f32>,
    chans: Vec<NrChan>,
    counter: usize,
    written: usize,
}

impl Default for NoiseReduction {
    fn default() -> Self {
        NoiseReduction { profile: None, reduction_db: 12.0, sensitivity: 2.5, freq_smoothing: 5, decay_ms: 40.0, st: Rt::default() }
    }
}

impl NoiseReduction {
    pub fn with_profile(profile: NoiseProfile) -> Self {
        NoiseReduction { profile: Some(profile), ..Default::default() }
    }

    pub fn is_ready(&self) -> bool {
        self.profile.is_some()
    }

    fn build(&self, channels: usize, rate: u32) -> Option<NrState> {
        let p = self.profile.as_ref()?;
        let n = p.fft_size;
        let hop = n / 4;
        Some(NrState {
            channels,
            rate,
            n,
            hop,
            fft: Fft::new(n),
            win: sqrt_hann(n),
            noise: p.for_rate(rate),
            chans: (0..channels).map(|_| NrChan { inbuf: vec![0.0; n], acc: vec![0.0; n], out_block: vec![0.0; hop], prev_gain: vec![1.0; n / 2 + 1] }).collect(),
            counter: 0,
            written: 0,
        })
    }

    fn run_frame(&self, st: &mut NrState, c: usize) {
        let (n, hop) = (st.n, st.hop);
        let bins = n / 2 + 1;
        let (mut re, mut im) = (vec![0.0f32; n], vec![0.0f32; n]);
        let wp = st.written % n; // posição da amostra mais antiga
        {
            let ch = &st.chans[c];
            for k in 0..n {
                re[k] = ch.inbuf[(wp + k) % n] * st.win[k];
            }
        }
        st.fft.forward(&mut re, &mut im);
        // ganho alvo por bin
        let floor = crate::db_to_lin(-(self.reduction_db.max(0.0) as f64)) as f32;
        let alpha = self.sensitivity.max(0.0);
        let mut g = vec![1.0f32; bins];
        for k in 0..bins {
            let p = re[k] * re[k] + im[k] * im[k] + 1e-20;
            g[k] = (1.0 - alpha * st.noise[k] / p).max(0.0).sqrt().max(floor);
        }
        // suavização em frequência (média móvel)
        let half = self.freq_smoothing.max(1) / 2;
        let gs: Vec<f32> = if half == 0 {
            g.clone()
        } else {
            (0..bins)
                .map(|k| {
                    let (a, b) = (k.saturating_sub(half), (k + half).min(bins - 1));
                    g[a..=b].iter().sum::<f32>() / (b - a + 1) as f32
                })
                .collect()
        };
        // queda lenta no tempo
        let decay = (-(hop as f32) / (self.decay_ms.max(0.1) * 0.001 * st.rate as f32)).exp();
        let ch = &mut st.chans[c];
        for k in 0..bins {
            let v = gs[k].max(ch.prev_gain[k] * decay);
            ch.prev_gain[k] = v;
            re[k] *= v;
            im[k] *= v;
            if k > 0 && k < n / 2 {
                re[n - k] *= v;
                im[n - k] *= v;
            }
        }
        st.fft.inverse(&mut re, &mut im);
        for k in 0..n {
            ch.acc[k] += re[k] * st.win[k] * 0.5;
        }
        ch.out_block.copy_from_slice(&ch.acc[..hop]);
        ch.acc.copy_within(hop.., 0);
        let tail = n - hop;
        ch.acc[tail..].fill(0.0);
    }
}

impl Effect for NoiseReduction {
    fn process(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32) {
        let nch = channels.max(1);
        let stale = self.st.as_ref().is_none_or(|s| s.channels != nch || s.rate != sample_rate || Some(s.n) != self.profile.as_ref().map(|p| p.fft_size));
        if stale {
            self.st.0 = self.build(nch, sample_rate);
        }
        let Some(mut st) = self.st.take() else {
            return; // sem perfil: passa direto
        };
        for fr in buf.chunks_exact_mut(nch) {
            let wp = st.written % st.n;
            for c in 0..nch {
                let ch = &mut st.chans[c];
                ch.inbuf[wp] = fr[c];
                fr[c] = ch.out_block[st.counter];
            }
            st.written += 1;
            st.counter += 1;
            if st.counter == st.hop {
                st.counter = 0;
                for c in 0..nch {
                    self.run_frame(&mut st, c);
                }
            }
        }
        self.st.0 = Some(st);
    }

    fn reset(&mut self) {
        self.st.0 = None;
    }

    fn latency_frames(&self, _sample_rate: u32) -> usize {
        self.profile.as_ref().map_or(0, |p| p.fft_size)
    }

    fn realtime(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::testutil::*;

    #[test]
    fn profile_matches_noise_power() {
        // ruído branco uniforme [-a, a]: variância a²/3; potência por bin de uma janela raiz-de-Hann = variância · soma(w²) = σ²·N/2
        let fs = 44_100;
        let a = 0.1f32;
        let buf = AudioBuf::mono(fs, noise_il(1, 3.0, a, 1));
        let p = NoiseProfile::from_audio(&buf, 2048).unwrap();
        assert_eq!(p.mean_power.len(), 1025);
        let expect = (a as f64).powi(2) / 3.0 * 1024.0;
        let mid = p.mean_power[100..900].iter().map(|&v| v as f64).sum::<f64>() / 800.0;
        assert!((mid / expect - 1.0).abs() < 0.1, "{mid} vs {expect}");
        assert!(NoiseProfile::from_audio(&AudioBuf::mono(fs, vec![0.0; 100]), 2048).is_none());
    }

    #[test]
    fn reduces_noise_floor_and_keeps_tone() {
        let fs = 44_100;
        let noise_amp = 0.02f32;
        let noise1 = noise_il(11, 4.0, noise_amp, 1);
        // perfil de um trecho só com ruído (outra realização)
        let prof_src = AudioBuf::mono(fs, noise_il(12, 1.0, noise_amp, 1));
        let profile = NoiseProfile::from_audio(&prof_src, 2048).unwrap();
        // sinal: 2 s de tom 1 kHz + ruído, depois 2 s só de ruído
        let mut x = noise1.clone();
        for i in 0..(2 * fs as usize) {
            x[i] += 0.3 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / fs as f32).sin();
        }
        let original = x.clone();
        let mut nr = NoiseReduction { reduction_db: 30.0, sensitivity: 3.0, ..NoiseReduction::with_profile(profile) };
        let lat = nr.latency_frames(fs);
        x.extend(std::iter::repeat_n(0.0, lat));
        nr.process(&mut x, 1, fs);
        let y = &x[lat..];
        // trecho só com ruído (depois do segundo 2,5): o piso cai bem
        let r = |v: &[f32]| rms(&v[(2.5 * fs as f32) as usize..(3.9 * fs as f32) as usize]);
        let red = db(r(y) / r(&original));
        assert!(red < -15.0, "redução {red} dB");
        // o tom passa quase intacto: amplitude do componente de 1 kHz
        let seg = |v: &[f32]| v[(0.5 * fs as f32) as usize..(1.5 * fs as f32) as usize].to_vec();
        let (a_in, a_out) = (tone_amp(&seg(&original), fs, 1000.0), tone_amp(&seg(y), fs, 1000.0));
        assert!((db(a_out / a_in)).abs() < 1.0, "tom {}", db(a_out / a_in));
        // e o ruído ao redor do tom cai também (banda 3-6 kHz com o tom tocando)
        let band = |v: &[f32]| band_rms(&seg(v), fs, 3000.0, 6000.0);
        assert!(db(band(y) / band(&original)) < -12.0, "{}", db(band(y) / band(&original)));
    }

    #[test]
    fn passthrough_without_profile_and_block_invariant() {
        let fs = 44_100;
        let x = sine_il(fs, 300.0, 0.2, 0.3, 1);
        let mut a = x.clone();
        NoiseReduction::default().process(&mut a, 1, fs);
        assert_eq!(a, x);
        let profile = NoiseProfile::from_audio(&AudioBuf::mono(fs, noise_il(5, 1.0, 0.05, 1)), 1024).unwrap();
        let src = noise_il(6, 0.5, 0.05, 2);
        let mut whole = src.clone();
        NoiseReduction::with_profile(profile.clone()).process(&mut whole, 2, fs);
        let mut parts = src.clone();
        let mut nr = NoiseReduction::with_profile(profile.clone());
        for blk in parts.chunks_mut(2 * 300) {
            nr.process(blk, 2, fs);
        }
        assert!(whole.iter().zip(&parts).all(|(p, q)| (p - q).abs() < 1e-6));
        // outra taxa de amostragem aceita o perfil (reescala) sem falhar
        let mut o = noise_il(7, 0.3, 0.05, 1);
        NoiseReduction::with_profile(profile).process(&mut o, 1, 48_000);
        assert!(o.iter().all(|v| v.is_finite()));
    }
}
