//! Equalizador: paramétrico (qualquer número de bandas biquad RBJ) e gráfico (10 bandas de oitava).

use super::{Effect, Rt};
use crate::dsp::{Biquad, BiquadCoeffs};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BandKind {
    #[default]
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
    Notch,
    BandPass,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct EqBand {
    pub kind: BandKind,
    pub freq_hz: f32,
    /// Ganho em dB (só `peak`, `low_shelf` e `high_shelf`).
    pub gain_db: f32,
    pub q: f32,
    pub enabled: bool,
}

impl Default for EqBand {
    fn default() -> Self {
        EqBand { kind: BandKind::Peak, freq_hz: 1000.0, gain_db: 0.0, q: 1.0, enabled: true }
    }
}

impl EqBand {
    pub fn peak(freq_hz: f32, gain_db: f32, q: f32) -> Self {
        EqBand { kind: BandKind::Peak, freq_hz, gain_db, q, enabled: true }
    }

    pub fn coeffs(&self, fs: f64) -> BiquadCoeffs {
        let (f, q, g) = (self.freq_hz as f64, self.q as f64, self.gain_db as f64);
        match self.kind {
            BandKind::Peak => BiquadCoeffs::peaking(fs, f, q, g),
            BandKind::LowShelf => BiquadCoeffs::low_shelf(fs, f, q, g),
            BandKind::HighShelf => BiquadCoeffs::high_shelf(fs, f, q, g),
            BandKind::LowPass => BiquadCoeffs::lowpass(fs, f, q),
            BandKind::HighPass => BiquadCoeffs::highpass(fs, f, q),
            BandKind::BandPass => BiquadCoeffs::bandpass(fs, f, q),
            BandKind::Notch => {
                let w0 = 2.0 * std::f64::consts::PI * (f / fs).clamp(1e-5, 0.499);
                let al = w0.sin() / (2.0 * q.max(1e-3));
                let a0 = 1.0 + al;
                BiquadCoeffs { b0: 1.0 / a0, b1: -2.0 * w0.cos() / a0, b2: 1.0 / a0, a1: -2.0 * w0.cos() / a0, a2: (1.0 - al) / a0 }
            }
        }
    }
}

/// Frequências centrais do equalizador gráfico de 10 bandas (oitavas ISO).
pub const GRAPHIC_10_HZ: [f32; 10] = [31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Equalizer {
    pub bands: Vec<EqBand>,
    /// Ganho de saída em dB depois das bandas.
    pub output_gain_db: f32,
    #[serde(skip)]
    #[schemars(skip)]
    pub st: Rt<Option<EqState>>,
}

#[derive(Debug, Clone)]
#[doc(hidden)]
pub struct EqState {
    channels: usize,
    rate: u32,
    snapshot: Vec<EqBand>,
    filters: Vec<Vec<Biquad>>, // [canal][banda ativa]
}

impl Default for Equalizer {
    fn default() -> Self {
        Equalizer { bands: Vec::new(), output_gain_db: 0.0, st: Rt::default() }
    }
}

impl Equalizer {
    pub fn parametric(bands: Vec<EqBand>) -> Self {
        Equalizer { bands, output_gain_db: 0.0, st: Rt::default() }
    }

    /// Gráfico: um ganho (dB) por banda de `GRAPHIC_10_HZ` (Q de oitava, 1,41). Ganhos a menos ficam em 0.
    pub fn graphic10(gains_db: &[f32]) -> Self {
        let bands = GRAPHIC_10_HZ
            .iter()
            .enumerate()
            .map(|(i, &f)| EqBand::peak(f, gains_db.get(i).copied().unwrap_or(0.0), 1.41))
            .collect();
        Equalizer { bands, output_gain_db: 0.0, st: Rt::default() }
    }

    /// Resposta em magnitude (dB) na frequência `f` (para desenhar a curva na UI).
    pub fn response_db(&self, f: f64, fs: f64) -> f64 {
        self.output_gain_db as f64 + self.bands.iter().filter(|b| b.enabled).map(|b| b.coeffs(fs).magnitude_db(fs, f)).sum::<f64>()
    }

    /// A curva em `points` frequências espaçadas logaritmicamente de `f_lo` a `f_hi`.
    pub fn response_curve(&self, fs: f64, f_lo: f64, f_hi: f64, points: usize) -> Vec<(f64, f64)> {
        let n = points.max(2);
        (0..n)
            .map(|i| {
                let f = f_lo * (f_hi / f_lo).powf(i as f64 / (n - 1) as f64);
                (f, self.response_db(f, fs))
            })
            .collect()
    }
}

impl Effect for Equalizer {
    fn process(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32) {
        let nch = channels.max(1);
        let fs = sample_rate as f64;
        let active: Vec<EqBand> = self.bands.iter().copied().filter(|b| b.enabled).collect();
        let fresh = match &self.st.0 {
            Some(s) => s.channels != nch || s.rate != sample_rate || s.snapshot.len() != active.len(),
            None => true,
        };
        if fresh {
            self.st.0 = Some(EqState {
                channels: nch,
                rate: sample_rate,
                snapshot: active.clone(),
                filters: (0..nch).map(|_| active.iter().map(|b| Biquad::new(b.coeffs(fs))).collect()).collect(),
            });
        } else if let Some(st) = &mut self.st.0 {
            // mesmas bandas: se algum parâmetro mudou, troca só os coeficientes e mantém o estado (sem estalo)
            if st.snapshot != active {
                for (i, b) in active.iter().enumerate() {
                    if st.snapshot[i] != *b {
                        let c = b.coeffs(fs);
                        for ch in &mut st.filters {
                            ch[i].c = c;
                        }
                    }
                }
                st.snapshot = active;
            }
        }
        let out_gain = crate::db_to_lin(self.output_gain_db as f64);
        let st = self.st.as_mut().unwrap();
        for fr in buf.chunks_exact_mut(nch) {
            for (c, s) in fr.iter_mut().enumerate() {
                let mut v = *s as f64;
                for b in &mut st.filters[c] {
                    v = b.process(v);
                }
                *s = (v * out_gain) as f32;
            }
        }
    }

    fn reset(&mut self) {
        self.st.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::testutil::*;

    /// Ganho medido (dB) de um seno em `f` depois do efeito.
    fn measured_gain(e: &mut Equalizer, f: f64) -> f64 {
        let fs = 48_000;
        let mut x = sine_il(fs, f, 0.6, 0.3, 1);
        let r0 = rms(&x[12_000..]);
        e.reset();
        e.process(&mut x, 1, fs);
        db(rms(&x[12_000..]) / r0)
    }

    #[test]
    fn parametric_peak_gain_at_center() {
        let mut e = Equalizer::parametric(vec![EqBand::peak(1000.0, 9.0, 2.0)]);
        assert!((measured_gain(&mut e, 1000.0) - 9.0).abs() < 0.1);
        // longe do centro: quase nada
        assert!(measured_gain(&mut e, 100.0).abs() < 0.3);
        assert!(measured_gain(&mut e, 10_000.0).abs() < 0.5);
        // a curva calculada concorda com a medida
        assert!((e.response_db(1000.0, 48_000.0) - 9.0).abs() < 1e-6);
    }

    #[test]
    fn shelves_and_filters() {
        let mut lo = Equalizer::parametric(vec![EqBand { kind: BandKind::LowShelf, freq_hz: 200.0, gain_db: 6.0, q: 0.707, enabled: true }]);
        assert!((measured_gain(&mut lo, 30.0) - 6.0).abs() < 0.3);
        assert!(measured_gain(&mut lo, 5000.0).abs() < 0.2);
        let mut hi = Equalizer::parametric(vec![EqBand { kind: BandKind::HighShelf, freq_hz: 4000.0, gain_db: -8.0, q: 0.707, enabled: true }]);
        assert!((measured_gain(&mut hi, 15_000.0) + 8.0).abs() < 0.4);
        let mut notch = Equalizer::parametric(vec![EqBand { kind: BandKind::Notch, freq_hz: 1000.0, gain_db: 0.0, q: 5.0, enabled: true }]);
        assert!(measured_gain(&mut notch, 1000.0) < -40.0);
        assert!(measured_gain(&mut notch, 2000.0).abs() < 0.5);
    }

    #[test]
    fn graphic10_boosts_only_that_band() {
        let mut g = [0.0f32; 10];
        g[5] = 12.0; // 1 kHz
        let mut e = Equalizer::graphic10(&g);
        assert!((measured_gain(&mut e, 1000.0) - 12.0).abs() < 0.2);
        assert!(measured_gain(&mut e, 62.5).abs() < 0.5);
        assert!(measured_gain(&mut e, 16_000.0).abs() < 0.5);
        // oitava vizinha recebe ganho parcial, entre 0 e 12
        let n = measured_gain(&mut e, 2000.0);
        assert!(n > 2.0 && n < 11.0, "{n}");
        // desligar a banda zera o efeito
        e.bands[5].enabled = false;
        assert!(measured_gain(&mut e, 1000.0).abs() < 1e-3);
    }

    #[test]
    fn block_processing_and_live_param_change() {
        let fs = 48_000;
        let x = sine_il(fs, 700.0, 0.3, 0.5, 2);
        let mut whole = x.clone();
        let mut e = Equalizer::graphic10(&[3.0, 0.0, -4.0, 0.0, 5.0, 0.0, 2.0, 0.0, -3.0, 0.0]);
        e.process(&mut whole, 2, fs);
        let mut parts = x.clone();
        let mut e2 = Equalizer::graphic10(&[3.0, 0.0, -4.0, 0.0, 5.0, 0.0, 2.0, 0.0, -3.0, 0.0]);
        for blk in parts.chunks_mut(2 * 512) {
            e2.process(blk, 2, fs);
        }
        assert!(whole.iter().zip(&parts).all(|(a, b)| (a - b).abs() < 1e-6));
        // mudar ganho no meio do fluxo não zera o estado nem gera saltos grandes
        let mut y = sine_il(fs, 700.0, 0.2, 0.5, 1);
        let mut e3 = Equalizer::parametric(vec![EqBand::peak(700.0, 0.0, 1.0)]);
        let (a, b) = y.split_at_mut(4800);
        e3.process(a, 1, fs);
        e3.bands[0].gain_db = 6.0;
        e3.process(b, 1, fs);
        let jump = (b[0] - a[a.len() - 1]).abs();
        assert!(jump < 0.2, "{jump}");
        assert!(db(rms(&b[2000..]) / 0.3535) > 5.0);
    }
}
