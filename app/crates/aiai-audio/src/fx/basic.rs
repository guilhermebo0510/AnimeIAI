//! Efeitos simples: Amplificar, Normalizar, remover DC e filtros passa-altas/passa-baixas.

use super::{Effect, Rt};
use crate::dsp::{Biquad, BiquadCoeffs};
use crate::edit::{deinterleave, interleave};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- Amplificar

/// Amplificar (Audacity): ganho fixo em dB. Com `allow_clipping = false` o ganho é limitado para o pico não passar de 0 dBFS
/// (isso exige olhar o trecho inteiro, então só funciona em modo offline).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Amplify {
    pub gain_db: f32,
    pub allow_clipping: bool,
}

impl Default for Amplify {
    fn default() -> Self {
        Amplify { gain_db: 0.0, allow_clipping: true }
    }
}

impl Effect for Amplify {
    fn process(&mut self, buf: &mut [f32], _channels: usize, _sample_rate: u32) {
        let mut g = crate::db_to_lin(self.gain_db as f64) as f32;
        if !self.allow_clipping {
            let peak = buf.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            if peak > 0.0 && peak * g > 1.0 {
                g = 1.0 / peak;
            }
        }
        for s in buf.iter_mut() {
            *s *= g;
        }
    }

    fn reset(&mut self) {}

    fn realtime(&self) -> bool {
        self.allow_clipping
    }
}

// ---------------------------------------------------------------- Normalizar

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NormalizeMode {
    /// Leva o maior pico ao alvo `target_db` (dBFS).
    #[default]
    Peak,
    /// Leva o loudness integrado (BS.1770) ao alvo `target_lufs`, respeitando `ceiling_dbtp` de true peak.
    Loudness,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Normalize {
    pub mode: NormalizeMode,
    /// Alvo de pico em dBFS (modo `peak`).
    pub target_db: f32,
    /// Alvo de loudness integrado (modo `loudness`): -23 EBU R128, -24 ARIB, -14 streaming.
    pub target_lufs: f32,
    /// Teto de true peak (modo `loudness`).
    pub ceiling_dbtp: f32,
    /// Tira o desvio DC (média) antes de normalizar.
    pub remove_dc: bool,
    /// Modo `peak`: cada canal vai ao alvo sozinho (desfaz o balanço estéreo). Desligado: ganho comum.
    pub independent_channels: bool,
}

impl Default for Normalize {
    fn default() -> Self {
        Normalize { mode: NormalizeMode::Peak, target_db: -1.0, target_lufs: -23.0, ceiling_dbtp: -1.0, remove_dc: true, independent_channels: false }
    }
}

impl Effect for Normalize {
    fn process(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32) {
        let nch = channels.max(1);
        if buf.len() < nch {
            return;
        }
        if self.remove_dc {
            remove_mean(buf, nch);
        }
        match self.mode {
            NormalizeMode::Peak => {
                let target = crate::db_to_lin(self.target_db as f64) as f32;
                let mut peaks = vec![0.0f32; nch];
                for fr in buf.chunks_exact(nch) {
                    for (c, s) in fr.iter().enumerate() {
                        peaks[c] = peaks[c].max(s.abs());
                    }
                }
                let all = peaks.iter().copied().fold(0.0f32, f32::max);
                for fr in buf.chunks_exact_mut(nch) {
                    for (c, s) in fr.iter_mut().enumerate() {
                        let p = if self.independent_channels { peaks[c] } else { all };
                        if p > 1e-9 {
                            *s *= target / p;
                        }
                    }
                }
            }
            NormalizeMode::Loudness => {
                let mut ab = deinterleave(buf, nch, sample_rate);
                let rep = crate::loudness::normalize(&mut ab, self.target_lufs as f64, self.ceiling_dbtp as f64);
                if rep.before_lufs.is_some() {
                    buf.copy_from_slice(&interleave(&ab));
                }
            }
        }
    }

    fn reset(&mut self) {}

    fn realtime(&self) -> bool {
        false
    }
}

fn remove_mean(buf: &mut [f32], nch: usize) {
    let n = (buf.len() / nch).max(1) as f64;
    let mut mean = vec![0.0f64; nch];
    for fr in buf.chunks_exact(nch) {
        for (c, s) in fr.iter().enumerate() {
            mean[c] += *s as f64;
        }
    }
    for fr in buf.chunks_exact_mut(nch) {
        for (c, s) in fr.iter_mut().enumerate() {
            *s -= (mean[c] / n) as f32;
        }
    }
}

// ---------------------------------------------------------------- DC

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DcMode {
    /// Subtrai a média do trecho (exato, offline).
    #[default]
    Mean,
    /// Bloqueador de DC de 1 polo em `cutoff_hz` (contínuo, serve em tempo real).
    HighPass,
}

/// Remover desvio DC.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DcOffset {
    pub mode: DcMode,
    pub cutoff_hz: f32,
    #[serde(skip)]
    #[schemars(skip)]
    pub st: Rt<Vec<(f64, f64)>>,
}

impl Default for DcOffset {
    fn default() -> Self {
        DcOffset { mode: DcMode::Mean, cutoff_hz: 10.0, st: Rt::default() }
    }
}

impl Effect for DcOffset {
    fn process(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32) {
        let nch = channels.max(1);
        match self.mode {
            DcMode::Mean => remove_mean(buf, nch),
            DcMode::HighPass => {
                if self.st.len() != nch {
                    self.st.0 = vec![(0.0, 0.0); nch];
                }
                let r = (-2.0 * std::f64::consts::PI * self.cutoff_hz.max(0.1) as f64 / sample_rate.max(1) as f64).exp();
                for fr in buf.chunks_exact_mut(nch) {
                    for (c, s) in fr.iter_mut().enumerate() {
                        let (x1, y1) = self.st[c];
                        let x = *s as f64;
                        let y = x - x1 + r * y1;
                        self.st[c] = (x, y);
                        *s = y as f32;
                    }
                }
            }
        }
    }

    fn reset(&mut self) {
        self.st.clear();
    }

    fn realtime(&self) -> bool {
        self.mode == DcMode::HighPass
    }
}

// ---------------------------------------------------------------- filtro passa-altas / passa-baixas

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    HighPass,
    #[default]
    LowPass,
}

/// Filtro Butterworth em cascata de biquads: 12, 24, 36 ou 48 dB/oitava.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Filter {
    pub kind: FilterKind,
    pub freq_hz: f32,
    pub slope_db_oct: u32,
    #[serde(skip)]
    #[schemars(skip)]
    pub st: Rt<Option<FilterState>>,
}

#[derive(Debug, Clone)]
#[doc(hidden)]
pub struct FilterState {
    key: (usize, u32, u32, u32, FilterKind),
    sections: Vec<Vec<Biquad>>, // [canal][seção]
}

impl Default for Filter {
    fn default() -> Self {
        Filter { kind: FilterKind::LowPass, freq_hz: 1000.0, slope_db_oct: 12, st: Rt::default() }
    }
}

impl Filter {
    pub fn low_pass(freq_hz: f32, slope_db_oct: u32) -> Self {
        Filter { kind: FilterKind::LowPass, freq_hz, slope_db_oct, st: Rt::default() }
    }

    pub fn high_pass(freq_hz: f32, slope_db_oct: u32) -> Self {
        Filter { kind: FilterKind::HighPass, freq_hz, slope_db_oct, st: Rt::default() }
    }

    fn coeffs(&self, fs: f64) -> Vec<BiquadCoeffs> {
        let k = (self.slope_db_oct / 12).clamp(1, 4) as usize; // nº de seções (ordem 2k)
        let order = 2 * k;
        (0..k)
            .map(|i| {
                let q = 1.0 / (2.0 * (std::f64::consts::PI * (2 * i + 1) as f64 / (2 * order) as f64).sin());
                match self.kind {
                    FilterKind::LowPass => BiquadCoeffs::lowpass(fs, self.freq_hz as f64, q),
                    FilterKind::HighPass => BiquadCoeffs::highpass(fs, self.freq_hz as f64, q),
                }
            })
            .collect()
    }

    /// Resposta em magnitude (dB) na frequência `f`.
    pub fn response_db(&self, f: f64, fs: f64) -> f64 {
        self.coeffs(fs).iter().map(|c| c.magnitude_db(fs, f)).sum()
    }
}

impl Effect for Filter {
    fn process(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32) {
        let nch = channels.max(1);
        let key = (nch, sample_rate, self.freq_hz.to_bits(), self.slope_db_oct, self.kind);
        let rebuild = self.st.as_ref().is_none_or(|s| s.key != key);
        if rebuild {
            let co = self.coeffs(sample_rate as f64);
            // troca de parâmetro mantém o estado quando só a frequência mudou (sem estalo): aqui recria por simplicidade
            self.st.0 = Some(FilterState { key, sections: (0..nch).map(|_| co.iter().map(|&c| Biquad::new(c)).collect()).collect() });
        }
        let st = self.st.as_mut().unwrap();
        for fr in buf.chunks_exact_mut(nch) {
            for (c, s) in fr.iter_mut().enumerate() {
                let mut v = *s as f64;
                for b in &mut st.sections[c] {
                    v = b.process(v);
                }
                *s = v as f32;
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

    #[test]
    fn amplify_gain_in_db() {
        let mut x = sine_il(48_000, 1000.0, 0.5, 0.25, 1);
        let before = rms(&x);
        Amplify { gain_db: 6.0, ..Default::default() }.process(&mut x, 1, 48_000);
        assert!((db(rms(&x) / before) - 6.0).abs() < 0.01);
        // sem clipping: o ganho é limitado a 0 dBFS de pico
        let mut y = sine_il(48_000, 1000.0, 0.5, 0.5, 1);
        Amplify { gain_db: 20.0, allow_clipping: false }.process(&mut y, 1, 48_000);
        assert!((peak(&y) - 1.0).abs() < 1e-5);
        let mut z = sine_il(48_000, 1000.0, 0.5, 0.5, 1);
        Amplify { gain_db: 20.0, allow_clipping: true }.process(&mut z, 1, 48_000);
        assert!(peak(&z) > 4.9);
    }

    #[test]
    fn normalize_peak_and_independent() {
        let mut x = sine_il(48_000, 440.0, 0.5, 0.1, 1);
        Normalize { target_db: -3.0, ..Default::default() }.process(&mut x, 1, 48_000);
        assert!((db(peak(&x)) + 3.0).abs() < 0.01, "{}", db(peak(&x)));
        // estéreo: ganho comum preserva o balanço, independente iguala
        let mut st = vec![0.0f32; 2 * 4800];
        for i in 0..4800 {
            let s = (i as f32 * 0.1).sin();
            st[2 * i] = 0.8 * s;
            st[2 * i + 1] = 0.2 * s;
        }
        let mut a = st.clone();
        Normalize { target_db: 0.0, remove_dc: false, ..Default::default() }.process(&mut a, 2, 48_000);
        let (pl, pr) = (peak_ch(&a, 2, 0), peak_ch(&a, 2, 1));
        assert!((pl - 1.0).abs() < 1e-4 && (pr - 0.25).abs() < 1e-3, "{pl} {pr}");
        let mut b = st;
        Normalize { target_db: 0.0, remove_dc: false, independent_channels: true, ..Default::default() }.process(&mut b, 2, 48_000);
        assert!((peak_ch(&b, 2, 1) - 1.0).abs() < 1e-4);
        // silêncio não explode
        let mut z = vec![0.0f32; 100];
        Normalize::default().process(&mut z, 1, 48_000);
        assert!(z.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn normalize_loudness_reaches_lufs_target() {
        let mut x = sine_il(48_000, 1000.0, 10.0, 0.05, 2);
        Normalize { mode: NormalizeMode::Loudness, target_lufs: -23.0, ..Default::default() }.process(&mut x, 2, 48_000);
        let l = crate::loudness::integrated_lufs(&crate::edit::deinterleave(&x, 2, 48_000)).unwrap();
        assert!((l + 23.0).abs() < 0.1, "{l}");
    }

    #[test]
    fn dc_removed_both_modes() {
        let mut x = sine_il(48_000, 440.0, 1.0, 0.3, 1);
        for s in x.iter_mut() {
            *s += 0.2;
        }
        let mut m = x.clone();
        DcOffset::default().process(&mut m, 1, 48_000);
        assert!(mean(&m).abs() < 1e-5, "{}", mean(&m));
        let mut h = x.clone();
        DcOffset { mode: DcMode::HighPass, cutoff_hz: 10.0, ..Default::default() }.process(&mut h, 1, 48_000);
        assert!(mean(&h[24_000..]).abs() < 5e-3, "{}", mean(&h[24_000..]));
        // o seno de 440 Hz não perde nível
        assert!((db(rms(&h[24_000..]) / rms(&m[24_000..]))).abs() < 0.1);
    }

    #[test]
    fn filters_follow_butterworth() {
        let fs = 48_000;
        let f = Filter::low_pass(1000.0, 24);
        assert!((f.response_db(1000.0, fs as f64) + 3.01).abs() < 0.05);
        // no sinal real: 8 kHz (3 oitavas) cai >= 60 dB com 24 dB/oitava... mede
        let mut hi = sine_il(fs, 8000.0, 0.5, 0.5, 1);
        let mut lo = sine_il(fs, 200.0, 0.5, 0.5, 1);
        let (rh, rl) = (rms(&hi[12_000..]), rms(&lo[12_000..]));
        let mut f1 = Filter::low_pass(1000.0, 24);
        f1.process(&mut hi, 1, fs);
        let mut f2 = Filter::low_pass(1000.0, 24);
        f2.process(&mut lo, 1, fs);
        assert!(db(rms(&hi[12_000..]) / rh) < -55.0, "{}", db(rms(&hi[12_000..]) / rh));
        assert!(db(rms(&lo[12_000..]) / rl).abs() < 0.1);
        // passa-altas: 50 Hz atenuado com 12 dB/oct em 200 Hz: ~-24 dB
        let mut low = sine_il(fs, 50.0, 1.0, 0.5, 2);
        let r0 = rms(&low[24_000..]);
        Filter::high_pass(200.0, 12).process(&mut low, 2, fs);
        let g = db(rms(&low[24_000..]) / r0);
        assert!((g + 24.0).abs() < 1.5, "{g}");
        // reset/blocos: processar em dois blocos dá o mesmo que de uma vez
        let x = sine_il(fs, 300.0, 0.2, 0.5, 1);
        let mut whole = x.clone();
        Filter::high_pass(500.0, 24).process(&mut whole, 1, fs);
        let mut parts = x.clone();
        let mut fb = Filter::high_pass(500.0, 24);
        let (a, b) = parts.split_at_mut(3333);
        fb.process(a, 1, fs);
        fb.process(b, 1, fs);
        assert!(whole.iter().zip(&parts).all(|(a, b)| (a - b).abs() < 1e-6));
    }
}
