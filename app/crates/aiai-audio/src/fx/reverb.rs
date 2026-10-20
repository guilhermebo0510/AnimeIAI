//! Reverb no estilo Freeverb (Jezar): 8 filtros comb em paralelo com amortecimento + 4 all-pass em série por canal,
//! com a abertura estéreo clássica de 23 amostras (em 44,1 kHz) e pré-atraso opcional.

use super::{Effect, Rt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const COMB_44K: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_44K: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD_44K: usize = 23;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Reverb {
    /// Tamanho da sala (0 a 1): quanto maior, mais longa a cauda.
    pub room_size: f32,
    /// Amortecimento dos agudos na cauda (0 a 1).
    pub damping: f32,
    /// Nível do som processado (dB).
    pub wet_db: f32,
    /// Nível do som original (dB). `-120` ou menos = só reverb.
    pub dry_db: f32,
    /// Abertura estéreo da reverb (0 = mono, 1 = máxima).
    pub width: f32,
    pub pre_delay_ms: f32,
    /// Segundos de cauda acrescentados no fim em modo offline (a duração do clipe cresce).
    pub tail_s: f32,
    #[serde(skip)]
    #[schemars(skip)]
    pub st: Rt<Option<RevState>>,
}

#[derive(Debug, Clone)]
struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

impl Comb {
    fn new(n: usize) -> Self {
        Comb { buf: vec![0.0; n.max(1)], idx: 0, store: 0.0 }
    }

    #[inline]
    fn process(&mut self, x: f32, feedback: f32, damp1: f32, damp2: f32) -> f32 {
        let out = self.buf[self.idx];
        self.store = out * damp2 + self.store * damp1;
        self.buf[self.idx] = x + self.store * feedback;
        self.idx += 1;
        if self.idx == self.buf.len() {
            self.idx = 0;
        }
        out
    }
}

#[derive(Debug, Clone)]
struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(n: usize) -> Self {
        Allpass { buf: vec![0.0; n.max(1)], idx: 0 }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.idx];
        let out = -x + b;
        self.buf[self.idx] = x + b * 0.5;
        self.idx += 1;
        if self.idx == self.buf.len() {
            self.idx = 0;
        }
        out
    }
}

#[derive(Debug, Clone)]
#[doc(hidden)]
pub struct RevState {
    channels: usize,
    rate: u32,
    combs: Vec<Vec<Comb>>,     // [canal][8]
    allpass: Vec<Vec<Allpass>>, // [canal][4]
    pre: Vec<f32>,
    pre_idx: usize,
}

impl Default for Reverb {
    fn default() -> Self {
        Reverb { room_size: 0.5, damping: 0.5, wet_db: -10.0, dry_db: 0.0, width: 1.0, pre_delay_ms: 0.0, tail_s: 0.0, st: Rt::default() }
    }
}

impl Reverb {
    fn build(&self, channels: usize, rate: u32) -> RevState {
        let k = rate as f64 / 44_100.0;
        let sc = |n: usize| ((n as f64 * k).round() as usize).max(1);
        let spread = sc(STEREO_SPREAD_44K);
        let combs = (0..channels).map(|c| COMB_44K.iter().map(|&n| Comb::new(sc(n) + if c % 2 == 1 { spread } else { 0 })).collect()).collect();
        let allpass = (0..channels).map(|c| ALLPASS_44K.iter().map(|&n| Allpass::new(sc(n) + if c % 2 == 1 { spread } else { 0 })).collect()).collect();
        let pre_n = (self.pre_delay_ms.max(0.0) * 0.001 * rate as f32) as usize + 1;
        RevState { channels, rate, combs, allpass, pre: vec![0.0; pre_n * channels], pre_idx: 0 }
    }
}

impl Effect for Reverb {
    fn process(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32) {
        let nch = channels.max(1);
        let pre_n = (self.pre_delay_ms.max(0.0) * 0.001 * sample_rate as f32) as usize + 1;
        if self.st.as_ref().is_none_or(|s| s.channels != nch || s.rate != sample_rate || s.pre.len() != pre_n * nch) {
            self.st.0 = Some(self.build(nch, sample_rate));
        }
        let feedback = 0.7 + self.room_size.clamp(0.0, 1.0) * 0.28;
        let damp1 = self.damping.clamp(0.0, 1.0) * 0.4;
        let damp2 = 1.0 - damp1;
        let wet = crate::db_to_lin(self.wet_db as f64) as f32 * 3.0;
        let dry = if self.dry_db <= -120.0 { 0.0 } else { crate::db_to_lin(self.dry_db as f64) as f32 };
        let width = self.width.clamp(0.0, 1.0);
        let (wet1, wet2) = (wet * (width / 2.0 + 0.5), wet * ((1.0 - width) / 2.0));
        let gain_in = 0.015f32;
        let st = self.st.as_mut().unwrap();
        for fr in buf.chunks_exact_mut(nch) {
            // entrada comum (mono) com pré-atraso
            let mono = fr.iter().sum::<f32>() / nch as f32;
            let delayed = if pre_n > 1 {
                let slot = st.pre_idx % pre_n;
                let d = st.pre[slot];
                st.pre[slot] = mono;
                st.pre_idx = (st.pre_idx + 1) % pre_n;
                d
            } else {
                mono
            };
            let input = delayed * gain_in;
            // cada canal tem a sua cadeia (canais ímpares usam o atraso espalhado)
            let mut outs = [0.0f32; 16];
            for c in 0..nch.min(16) {
                let mut acc = 0.0;
                for cb in &mut st.combs[c] {
                    acc += cb.process(input, feedback, damp1, damp2);
                }
                for ap in &mut st.allpass[c] {
                    acc = ap.process(acc);
                }
                outs[c] = acc;
            }
            if nch == 2 {
                let (l, r) = (outs[0], outs[1]);
                fr[0] = fr[0] * dry + l * wet1 + r * wet2;
                fr[1] = fr[1] * dry + r * wet1 + l * wet2;
            } else {
                for c in 0..nch.min(16) {
                    fr[c] = fr[c] * dry + outs[c] * wet;
                }
            }
        }
    }

    fn reset(&mut self) {
        self.st.0 = None;
    }

    fn tail_frames(&self, sample_rate: u32) -> usize {
        (self.tail_s.max(0.0) * sample_rate as f32) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::testutil::*;

    fn impulse(fs: u32, secs: f32, ch: usize) -> Vec<f32> {
        let mut x = vec![0.0f32; (fs as f32 * secs) as usize * ch];
        for c in 0..ch {
            x[c] = 1.0;
        }
        x
    }

    /// RMS (por canal 0) de um trecho em segundos.
    fn window_rms(x: &[f32], ch: usize, fs: u32, a: f32, b: f32) -> f32 {
        let (i, j) = ((a * fs as f32) as usize * ch, (b * fs as f32) as usize * ch);
        rms_ch(&x[i..j], ch, 0)
    }

    #[test]
    fn impulse_response_decays_and_is_stable() {
        let fs = 44_100;
        let mut x = impulse(fs, 4.0, 2);
        Reverb { dry_db: -200.0, wet_db: 0.0, room_size: 0.7, ..Default::default() }.process(&mut x, 2, fs);
        assert!(x.iter().all(|v| v.is_finite()));
        let early = window_rms(&x, 2, fs, 0.05, 0.3);
        let late = window_rms(&x, 2, fs, 1.5, 1.75);
        let later = window_rms(&x, 2, fs, 3.5, 3.75);
        assert!(early > 1e-4, "cauda existe: {early}");
        assert!(db(late / early) < -12.0, "decai: {}", db(late / early));
        assert!(later < late, "continua decaindo");
        assert!(peak(&x) < 2.0);
    }

    #[test]
    fn bigger_room_and_less_damping_ring_longer() {
        let fs = 44_100;
        let decay = |room: f32, damp: f32| {
            let mut x = impulse(fs, 3.0, 1);
            Reverb { room_size: room, damping: damp, dry_db: -200.0, wet_db: 0.0, ..Default::default() }.process(&mut x, 1, fs);
            db(window_rms(&x, 1, fs, 1.5, 1.75) / window_rms(&x, 1, fs, 0.05, 0.3))
        };
        let (small, big) = (decay(0.2, 0.5), decay(0.9, 0.5));
        assert!(big > small + 10.0, "{small} {big}");
        let (bright, dull) = (decay(0.7, 0.0), decay(0.7, 1.0));
        assert!(bright > dull, "{bright} {dull}");
    }

    #[test]
    fn dry_wet_mix_and_pre_delay() {
        let fs = 44_100;
        // só seco: passa o sinal sem mudança
        let x = sine_il(fs, 500.0, 0.2, 0.5, 2);
        let mut a = x.clone();
        Reverb { wet_db: -200.0, dry_db: 0.0, ..Default::default() }.process(&mut a, 2, fs);
        assert!(a.iter().zip(&x).all(|(p, q)| (p - q).abs() < 1e-5));
        // pré-atraso: nada de wet antes dele
        let mut imp = impulse(fs, 1.0, 1);
        Reverb { dry_db: -200.0, wet_db: 0.0, pre_delay_ms: 50.0, ..Default::default() }.process(&mut imp, 1, fs);
        let before = imp[..(0.049 * fs as f32) as usize].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(before < 1e-6, "{before}");
        // estéreo: os dois canais da cauda diferem (abertura)
        let mut st = impulse(fs, 1.0, 2);
        Reverb { dry_db: -200.0, wet_db: 0.0, width: 1.0, ..Default::default() }.process(&mut st, 2, fs);
        let diff: f32 = st.chunks_exact(2).map(|f| (f[0] - f[1]).abs()).sum();
        assert!(diff > 0.01);
        let mut mono = impulse(fs, 1.0, 2);
        Reverb { dry_db: -200.0, wet_db: 0.0, width: 0.0, ..Default::default() }.process(&mut mono, 2, fs);
        let diff0: f32 = mono.chunks_exact(2).map(|f| (f[0] - f[1]).abs()).sum();
        assert!(diff0 < diff * 0.01, "{diff0} {diff}");
    }

    #[test]
    fn tail_extends_render_and_blocks_match() {
        let fs = 44_100;
        let r = Reverb { tail_s: 1.0, ..Default::default() };
        assert_eq!(r.tail_frames(fs), 44_100);
        let x = noise_il(9, 0.3, 0.3, 1);
        let mut whole = x.clone();
        Reverb::default().process(&mut whole, 1, fs);
        let mut parts = x.clone();
        let mut rv = Reverb::default();
        for blk in parts.chunks_mut(333) {
            rv.process(blk, 1, fs);
        }
        assert!(whole.iter().zip(&parts).all(|(a, b)| (a - b).abs() < 1e-6));
    }
}
