//! Tempo e altura: `TimeStretch` (muda a duração sem mudar a altura) e `PitchShift` (muda a altura sem mudar a duração),
//! ambos por WSOLA (sobreposição e soma com sincronização por correlação) com janelas de ~30 ms, adequados à voz.
//! A mudança de altura é WSOLA seguido de reamostragem sinc. São efeitos offline (olham o clipe inteiro).

use super::Effect;
use crate::buf::AudioBuf;
use crate::edit::{deinterleave, interleave, rational};
use crate::resample::resample;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Estica (`ratio > 1`) ou encurta (`ratio < 1`) a duração por WSOLA. Os canais compartilham os mesmos deslocamentos
/// (a imagem estéreo fica coerente); a busca usa a soma dos canais.
pub fn wsola_stretch(input: &AudioBuf, ratio: f64, window_ms: f32, search_ms: f32) -> AudioBuf {
    let n = input.frames();
    let ratio = ratio.clamp(0.1, 10.0);
    if n == 0 || (ratio - 1.0).abs() < 1e-9 {
        return input.clone();
    }
    let fs = input.rate as f64;
    let nw = ((window_ms.max(5.0) as f64 * 0.001 * fs) as usize).max(64) & !1; // par
    let hs = nw / 2; // passo de síntese
    let ha = hs as f64 / ratio; // passo de análise nominal
    let delta = ((search_ms.max(1.0) as f64 * 0.001 * fs) as usize).min(nw);
    let out_len = (n as f64 * ratio).round() as usize;
    let nch = input.channels();
    // sinal de busca (soma dos canais)
    let m: Vec<f32> = (0..n).map(|i| input.ch.iter().map(|c| c[i]).sum::<f32>()).collect();
    let get = |x: &[f32], i: isize| -> f32 { if i < 0 || i as usize >= x.len() { 0.0 } else { x[i as usize] } };
    let win: Vec<f32> = (0..nw).map(|i| (0.5 - 0.5 * (2.0 * PI * i as f64 / nw as f64).cos()) as f32).collect();
    let mut out = vec![vec![0.0f32; out_len + 2 * nw]; nch];
    let mut wsum = vec![0.0f32; out_len + 2 * nw];
    // correlação normalizada de m[s..s+nw] com o alvo, com passo de amostragem `step`
    let corr = |target: &[f32], s: isize, step: usize| -> f32 {
        let (mut dot, mut e) = (0.0f32, 1e-9f32);
        let mut k = 0;
        while k < nw {
            let v = get(&m, s + k as isize);
            dot += v * target[k];
            e += v * v;
            k += step;
        }
        dot / e.sqrt()
    };
    let mut target = vec![0.0f32; nw];
    let mut prev: isize = 0;
    let mut k = 0usize;
    loop {
        let opos = k * hs; // início da janela na saída
        if opos >= out_len + hs {
            break;
        }
        let start: isize = if k == 0 {
            0
        } else {
            let nominal = (k as f64 * ha).round() as isize;
            if nominal as usize > n + delta + nw {
                break;
            }
            // continuação natural do segmento anterior
            let nat = prev + hs as isize;
            for (i, t) in target.iter_mut().enumerate() {
                *t = get(&m, nat + i as isize);
            }
            let lo = nominal - delta as isize;
            let hi = nominal + delta as isize;
            // busca grossa (passo 4 nos candidatos e na correlação) e refinamento fino
            let mut best = (f32::NEG_INFINITY, nominal);
            let mut s = lo;
            while s <= hi {
                let c = corr(&target, s, 4);
                if c > best.0 {
                    best = (c, s);
                }
                s += 4;
            }
            let (mut fb, mut fs_) = (f32::NEG_INFINITY, best.1);
            for s in (best.1 - 4)..=(best.1 + 4) {
                let c = corr(&target, s, 2);
                if c > fb {
                    fb = c;
                    fs_ = s;
                }
            }
            fs_
        };
        prev = start;
        for (c, o) in out.iter_mut().enumerate() {
            let x = &input.ch[c];
            for i in 0..nw {
                o[opos + i] += get(x, start + i as isize) * win[i];
            }
        }
        for i in 0..nw {
            wsum[opos + i] += win[i];
        }
        k += 1;
    }
    for o in out.iter_mut() {
        for i in 0..o.len() {
            // a soma de janelas de Hann a 50% vale 1; só as bordas pedem normalização
            let w = wsum[i];
            if w > 1e-3 {
                o[i] /= w;
            }
        }
        o.truncate(out_len);
    }
    AudioBuf { rate: input.rate, ch: out }
}

/// Muda a altura em `semitones` mantendo a duração: WSOLA com razão `p` seguido de reamostragem por `1/p`.
pub fn pitch_shift(input: &AudioBuf, semitones: f64, window_ms: f32, search_ms: f32) -> AudioBuf {
    let n = input.frames();
    if n == 0 || semitones.abs() < 1e-6 {
        return input.clone();
    }
    let p = 2f64.powf(semitones / 12.0);
    let stretched = wsola_stretch(input, p, window_ms, search_ms);
    // `stretched` tem n·p quadros; tocá-lo p vezes mais rápido devolve n quadros e sobe a altura em p
    let (num, den) = rational(p, 512);
    let tmp = AudioBuf { rate: input.rate * num, ch: stretched.ch };
    let mut out = resample(&tmp, input.rate * den);
    out.rate = input.rate;
    out.resize(n);
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TimeStretch {
    /// Nova duração / duração original (2.0 = o dobro do tempo, metade da velocidade; altura igual).
    pub ratio: f32,
    pub window_ms: f32,
    pub search_ms: f32,
}

impl Default for TimeStretch {
    fn default() -> Self {
        TimeStretch { ratio: 1.0, window_ms: 30.0, search_ms: 10.0 }
    }
}

impl Effect for TimeStretch {
    /// Não muda o buffer (a duração muda; use `render`).
    fn process(&mut self, _buf: &mut [f32], _channels: usize, _sample_rate: u32) {}

    fn reset(&mut self) {}

    fn realtime(&self) -> bool {
        false
    }

    fn render(&mut self, input: &AudioBuf) -> AudioBuf {
        wsola_stretch(input, self.ratio as f64, self.window_ms, self.search_ms)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PitchShift {
    pub semitones: f32,
    /// Ajuste fino em centésimos de semitom (somado a `semitones`).
    pub cents: f32,
    pub window_ms: f32,
    pub search_ms: f32,
}

impl Default for PitchShift {
    fn default() -> Self {
        PitchShift { semitones: 0.0, cents: 0.0, window_ms: 30.0, search_ms: 10.0 }
    }
}

impl Effect for PitchShift {
    fn process(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32) {
        let nch = channels.max(1);
        let ab = deinterleave(buf, nch, sample_rate);
        let out = self.render(&ab);
        buf.copy_from_slice(&interleave(&out));
    }

    fn reset(&mut self) {}

    fn realtime(&self) -> bool {
        false
    }

    fn render(&mut self, input: &AudioBuf) -> AudioBuf {
        pitch_shift(input, self.semitones as f64 + self.cents as f64 / 100.0, self.window_ms, self.search_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::testutil::*;

    fn voiceish(fs: u32, f0: f64, secs: f64) -> AudioBuf {
        // série harmônica com envelope lento: parecido com vogal sustentada
        let n = (fs as f64 * secs) as usize;
        AudioBuf::mono(
            fs,
            (0..n)
                .map(|i| {
                    let t = i as f64 / fs as f64;
                    let env = 0.6 + 0.4 * (2.0 * PI * 3.0 * t).sin();
                    let mut s = 0.0;
                    for h in 1..=8 {
                        s += (2.0 * PI * f0 * h as f64 * t).sin() / h as f64;
                    }
                    (0.3 * env * s) as f32
                })
                .collect(),
        )
    }

    /// Frequência fundamental por autocorrelação (Hz) no trecho `a..b`.
    fn f0_of(x: &[f32], fs: u32, lo: f64, hi: f64) -> f64 {
        let (mut best, mut best_lag) = (f64::NEG_INFINITY, 0usize);
        let (l0, l1) = ((fs as f64 / hi) as usize, (fs as f64 / lo) as usize);
        for lag in l0..=l1 {
            let c: f64 = x.iter().zip(&x[lag..]).map(|(a, b)| *a as f64 * *b as f64).sum();
            if c > best {
                best = c;
                best_lag = lag;
            }
        }
        fs as f64 / best_lag as f64
    }

    #[test]
    fn time_stretch_changes_length_not_pitch() {
        let fs = 44_100;
        let x = sine_il(fs, 440.0, 2.0, 0.5, 1);
        let ab = AudioBuf::mono(fs, x.clone());
        for ratio in [0.5f32, 0.8, 1.25, 2.0] {
            let y = TimeStretch { ratio, ..Default::default() }.render(&ab);
            let want = (ab.frames() as f64 * ratio as f64).round() as i64;
            assert!((y.frames() as i64 - want).abs() <= 2, "{ratio}: {} vs {want}", y.frames());
            let mid = &y.ch[0][y.frames() / 4..3 * y.frames() / 4];
            let zc = mid.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f64;
            let f = zc / (mid.len() as f64 / fs as f64);
            assert!((f - 440.0).abs() < 440.0 * 0.02, "{ratio}: {f} Hz");
            assert!(db(rms(mid) / rms(&x[..mid.len()])).abs() < 1.0, "{ratio}: nível {}", db(rms(mid) / rms(&x[..mid.len()])));
        }
        // razão 1: idêntico
        assert_eq!(TimeStretch::default().render(&ab), ab);
    }

    #[test]
    fn time_stretch_voice_like_keeps_f0() {
        let fs = 44_100;
        let v = voiceish(fs, 150.0, 2.0);
        let y = TimeStretch { ratio: 1.4, ..Default::default() }.render(&v);
        let f = f0_of(&y.ch[0][20_000..24_000], fs, 100.0, 220.0);
        assert!((f - 150.0).abs() < 4.0, "{f}");
        // multicanal: mesma duração e canais coerentes
        let st = AudioBuf { rate: fs, ch: vec![v.ch[0].clone(), v.ch[0].iter().map(|s| -s).collect()] };
        let ys = TimeStretch { ratio: 0.7, ..Default::default() }.render(&st);
        assert_eq!(ys.channels(), 2);
        assert!(ys.ch[0].iter().zip(&ys.ch[1]).all(|(a, b)| (a + b).abs() < 1e-5));
    }

    #[test]
    fn pitch_shift_moves_frequency_keeps_length() {
        let fs = 44_100;
        let ab = AudioBuf::mono(fs, sine_il(fs, 440.0, 2.0, 0.5, 1));
        for (st, want) in [(12.0f32, 880.0f64), (-12.0, 220.0), (7.0, 440.0 * 2f64.powf(7.0 / 12.0))] {
            let y = PitchShift { semitones: st, ..Default::default() }.render(&ab);
            assert_eq!(y.frames(), ab.frames(), "{st}");
            let mid = &y.ch[0][fs as usize / 2..3 * fs as usize / 2];
            let zc = mid.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f64;
            let f = zc / (mid.len() as f64 / fs as f64);
            assert!((f - want).abs() < want * 0.015, "{st}: {f} vs {want}");
            assert!(db(rms(mid) / rms(&ab.ch[0][..mid.len()])).abs() < 1.5);
        }
        // voz: +5 semitons leva 150 Hz a ~200 Hz
        let v = voiceish(fs, 150.0, 2.0);
        let y = PitchShift { semitones: 5.0, ..Default::default() }.render(&v);
        let f = f0_of(&y.ch[0][20_000..24_000], fs, 120.0, 300.0);
        assert!((f - 150.0 * 2f64.powf(5.0 / 12.0)).abs() < 5.0, "{f}");
        // processar em fluxo (entrelaçado) dá o mesmo que render
        let mut x = sine_il(fs, 300.0, 0.5, 0.4, 2);
        let want = PitchShift { semitones: 3.0, ..Default::default() }.render(&deinterleave(&x, 2, fs));
        PitchShift { semitones: 3.0, ..Default::default() }.process(&mut x, 2, fs);
        assert_eq!(x, interleave(&want));
        assert!(!PitchShift::default().realtime());
    }
}
