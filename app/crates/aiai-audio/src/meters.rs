//! Medidores para a UI: relatório completo de um buffer (LUFS momentâneo/curto/integrado, LRA, true peak, pico, RMS, DC,
//! clipping, por canal) sobre o módulo [`crate::loudness`], e um medidor de loudness em fluxo (`LoudnessMeter`) que
//! recebe blocos entrelaçados e dá os mesmos números, para o painel de medidores em tempo real.

use crate::buf::AudioBuf;
use crate::dsp::Biquad;
use crate::loudness::{self, TP_PHASES, TP_TAPS_PER_PHASE, channel_weight, gated_integrated, lra_from_short_term, opt, to_lufs};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct MeterReport {
    pub sample_rate: u32,
    pub channels: usize,
    pub duration_s: f64,
    pub integrated_lufs: Option<f64>,
    pub momentary_max_lufs: Option<f64>,
    pub short_term_max_lufs: Option<f64>,
    pub lra_lu: Option<f64>,
    pub true_peak_dbtp: Option<f64>,
    pub sample_peak_dbfs: Option<f64>,
    /// RMS de todas as amostras (dBFS); `null` em silêncio digital.
    pub rms_dbfs: Option<f64>,
    pub channel_peak_dbfs: Vec<Option<f64>>,
    pub channel_rms_dbfs: Vec<Option<f64>>,
    /// Média por canal (desvio DC) em escala linear.
    pub dc_offset: Vec<f32>,
    /// Amostras com |x| >= 1.0.
    pub clipped_samples: usize,
    /// Passo da série em segundos (0,1 s no máximo de pontos; maior se o áudio for longo).
    pub series_hop_s: f64,
    pub momentary_series: Vec<Option<f64>>,
    pub short_term_series: Vec<Option<f64>>,
}

/// Mede o buffer. As séries são reduzidas a no máximo `max_series_points` pontos (o maior valor de cada grupo).
pub fn analyze(buf: &AudioBuf, max_series_points: usize) -> MeterReport {
    let l = loudness::measure(buf);
    let group = l.momentary_series.len().div_ceil(max_series_points.max(1)).max(1);
    let reduce = |v: &[Option<f64>]| -> Vec<Option<f64>> {
        v.chunks(group)
            .map(|c| {
                let m = c.iter().flatten().copied().fold(f64::NEG_INFINITY, f64::max);
                opt(m)
            })
            .collect()
    };
    let n = buf.frames().max(1) as f64;
    let mut total_sq = 0.0f64;
    let (mut ch_peak, mut ch_rms, mut dc) = (Vec::new(), Vec::new(), Vec::new());
    let mut clipped = 0usize;
    for x in &buf.ch {
        let (mut pk, mut sq, mut sum) = (0.0f32, 0.0f64, 0.0f64);
        for &s in x {
            pk = pk.max(s.abs());
            sq += (s as f64) * (s as f64);
            sum += s as f64;
            if s.abs() >= 1.0 {
                clipped += 1;
            }
        }
        total_sq += sq;
        ch_peak.push(opt(crate::lin_to_db(pk as f64)));
        ch_rms.push(opt(crate::lin_to_db((sq / n).sqrt())));
        dc.push((sum / n) as f32);
    }
    let total_n = (buf.frames() * buf.channels()).max(1) as f64;
    MeterReport {
        sample_rate: l.sample_rate,
        channels: l.channels,
        duration_s: l.duration_s,
        integrated_lufs: l.integrated_lufs,
        momentary_max_lufs: l.momentary_max_lufs,
        short_term_max_lufs: l.short_term_max_lufs,
        lra_lu: l.lra_lu,
        true_peak_dbtp: l.true_peak_dbtp,
        sample_peak_dbfs: l.sample_peak_dbfs,
        rms_dbfs: opt(crate::lin_to_db((total_sq / total_n).sqrt())),
        channel_peak_dbfs: ch_peak,
        channel_rms_dbfs: ch_rms,
        dc_offset: dc,
        clipped_samples: clipped,
        series_hop_s: 0.1 * group as f64,
        momentary_series: reduce(&l.momentary_series),
        short_term_series: reduce(&l.short_term_series),
    }
}

// ---------------------------------------------------------------- medidor em fluxo

/// Medidor BS.1770/R128 em fluxo. Alimente com blocos entrelaçados (`push`); a cada 100 ms fecha um passo e as
/// leituras (momentâneo 400 ms, curto prazo 3 s, integrado com portas, LRA, true peak) ficam disponíveis. Para o mesmo
/// áudio, bate com `loudness::measure` (a diferença é só o true peak das últimas amostras, até `finish`).
pub struct LoudnessMeter {
    rate: u32,
    channels: usize,
    filters: Vec<[Biquad; 2]>,
    weights: Vec<f64>,
    hop: usize,
    in_hop: usize,
    hop_energy: f64,
    /// Energia de cada passo de 100 ms fechado.
    hops: Vec<f64>,
    /// Energia média das janelas de 400 ms e de 3 s, a cada passo.
    momentary: Vec<f64>,
    short_term: Vec<f64>,
    // true peak
    tp_hist: Vec<Vec<f32>>, // [canal] últimas TP_TAPS_PER_PHASE amostras (circular)
    tp_pos: usize,
    seen: usize,
    peak_lin: f64,
}

impl LoudnessMeter {
    pub fn new(rate: u32, channels: usize) -> Self {
        let channels = channels.max(1);
        let co = loudness::k_weight_coeffs(rate as f64);
        LoudnessMeter {
            rate,
            channels,
            filters: (0..channels).map(|_| [Biquad::new(co[0]), Biquad::new(co[1])]).collect(),
            weights: (0..channels).map(|c| channel_weight(channels, c)).collect(),
            hop: (rate as usize / 10).max(1),
            in_hop: 0,
            hop_energy: 0.0,
            hops: Vec::new(),
            momentary: Vec::new(),
            short_term: Vec::new(),
            tp_hist: vec![vec![0.0; TP_TAPS_PER_PHASE]; channels],
            tp_pos: 0,
            seen: 0,
            peak_lin: 0.0,
        }
    }

    pub fn reset(&mut self) {
        *self = LoudnessMeter::new(self.rate, self.channels);
    }

    /// Recebe um bloco entrelaçado.
    pub fn push(&mut self, data: &[f32]) {
        let nch = self.channels;
        let f = loudness::tp_filter();
        for fr in data.chunks_exact(nch) {
            // loudness
            let mut e = 0.0f64;
            for c in 0..nch {
                let w = self.weights[c];
                let x = fr[c];
                self.peak_lin = self.peak_lin.max((x as f64).abs());
                self.tp_hist[c][self.tp_pos] = x;
                if w == 0.0 {
                    continue;
                }
                let [s1, s2] = &mut self.filters[c];
                let y = s2.process(s1.process(x as f64));
                e += w * y * y;
            }
            self.hop_energy += e;
            self.in_hop += 1;
            // true peak: com a amostra `seen` chegando, interpola entre i = seen - h e i + 1 (contexto completo à frente)
            self.tp_pos = (self.tp_pos + 1) % TP_TAPS_PER_PHASE;
            self.seen += 1;
            if self.seen >= TP_TAPS_PER_PHASE {
                for c in 0..nch {
                    for p in 1..TP_PHASES {
                        let mut acc = 0.0f64;
                        for m in 0..TP_TAPS_PER_PHASE {
                            // a janela do histórico circular começa na amostra mais antiga (tp_pos)
                            acc += self.tp_hist[c][(self.tp_pos + m) % TP_TAPS_PER_PHASE] as f64 * f[p][m];
                        }
                        self.peak_lin = self.peak_lin.max(acc.abs());
                    }
                }
            }
            if self.in_hop == self.hop {
                self.hops.push(self.hop_energy);
                self.hop_energy = 0.0;
                self.in_hop = 0;
                let k = self.hops.len();
                if k >= 4 {
                    self.momentary.push(self.hops[k - 4..].iter().sum::<f64>() / (4 * self.hop) as f64);
                }
                if k >= 30 {
                    self.short_term.push(self.hops[k - 30..].iter().sum::<f64>() / (30 * self.hop) as f64);
                }
            }
        }
    }

    pub fn momentary_lufs(&self) -> Option<f64> {
        self.momentary.last().map(|&e| to_lufs(e)).filter(|v| v.is_finite())
    }

    pub fn short_term_lufs(&self) -> Option<f64> {
        self.short_term.last().map(|&e| to_lufs(e)).filter(|v| v.is_finite())
    }

    pub fn integrated_lufs(&self) -> Option<f64> {
        gated_integrated(&self.momentary)
    }

    pub fn lra_lu(&self) -> Option<f64> {
        lra_from_short_term(&self.short_term)
    }

    pub fn true_peak_dbtp(&self) -> Option<f64> {
        opt(crate::lin_to_db(self.peak_lin))
    }

    pub fn momentary_max_lufs(&self) -> Option<f64> {
        opt(self.momentary.iter().map(|&e| to_lufs(e)).fold(f64::NEG_INFINITY, f64::max))
    }

    pub fn short_term_max_lufs(&self) -> Option<f64> {
        opt(self.short_term.iter().map(|&e| to_lufs(e)).fold(f64::NEG_INFINITY, f64::max))
    }

    pub fn duration_s(&self) -> f64 {
        self.seen as f64 / self.rate as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::interleave;
    use std::f64::consts::PI;

    fn tone(rate: u32, secs: f64, freq: f64, dbfs: f64, ch: usize) -> AudioBuf {
        let n = (rate as f64 * secs) as usize;
        let a = crate::db_to_lin(dbfs);
        let x: Vec<f32> = (0..n).map(|i| (a * (2.0 * PI * freq * i as f64 / rate as f64).sin()) as f32).collect();
        AudioBuf { rate, ch: vec![x; ch] }
    }

    #[test]
    fn reference_sine_reads_minus_23_lufs() {
        // EBU Tech 3341: seno 1 kHz a -23 dBFS estéreo = -23,0 LUFS (±0,1); mono dual = igual
        let r = analyze(&tone(48_000, 20.0, 1000.0, -23.0, 2), 200);
        assert!((r.integrated_lufs.unwrap() + 23.0).abs() < 0.1, "{:?}", r.integrated_lufs);
        assert!((r.momentary_max_lufs.unwrap() + 23.0).abs() < 0.1);
        assert!((r.short_term_max_lufs.unwrap() + 23.0).abs() < 0.1);
        assert!(r.lra_lu.unwrap() < 0.1);
        assert!((r.sample_peak_dbfs.unwrap() + 23.0).abs() < 0.05);
        assert!((r.true_peak_dbtp.unwrap() + 23.0).abs() < 0.2);
        // RMS de um seno = pico - 3,01 dB
        assert!((r.rms_dbfs.unwrap() + 26.01).abs() < 0.05, "{:?}", r.rms_dbfs);
        assert_eq!(r.channel_peak_dbfs.len(), 2);
        assert!(r.dc_offset.iter().all(|d| d.abs() < 1e-4));
        assert_eq!(r.clipped_samples, 0);
        // séries reduzidas ao limite pedido
        assert!(r.momentary_series.len() <= 200 && r.momentary_series.len() >= 100, "{}", r.momentary_series.len());
        assert!((r.series_hop_s - 0.1).abs() < 1e-9 || r.series_hop_s > 0.1);
    }

    #[test]
    fn level_step_changes_lra_and_clipping_and_dc_are_reported() {
        let mut a = tone(48_000, 20.0, 1000.0, -20.0, 2);
        let b = tone(48_000, 20.0, 1000.0, -30.0, 2);
        for (x, y) in a.ch.iter_mut().zip(&b.ch) {
            x.extend_from_slice(y);
        }
        assert!((analyze(&a, 100).lra_lu.unwrap() - 10.0).abs() < 1.0);
        let mut c = tone(48_000, 1.0, 100.0, 6.0, 1);
        for s in c.ch[0].iter_mut() {
            *s = s.clamp(-1.0, 1.0) + 0.0;
        }
        let rc = analyze(&c, 10);
        assert!(rc.clipped_samples > 1000, "{}", rc.clipped_samples);
        let mut d = tone(48_000, 1.0, 100.0, -20.0, 1);
        for s in d.ch[0].iter_mut() {
            *s += 0.1;
        }
        assert!((analyze(&d, 10).dc_offset[0] - 0.1).abs() < 1e-3);
        // silêncio: sem loudness, sem pico
        let z = analyze(&AudioBuf::zeros(48_000, 2, 48_000), 10);
        assert!(z.integrated_lufs.is_none() && z.rms_dbfs.is_none() && z.sample_peak_dbfs.is_none());
    }

    #[test]
    fn streaming_meter_matches_batch() {
        // sinal com variação: 6 s a -20 e 6 s a -28 dBFS, estéreo, com um pico intersample
        let mut a = tone(48_000, 6.0, 1000.0, -20.0, 2);
        let b = tone(48_000, 6.0, 440.0, -28.0, 2);
        for (x, y) in a.ch.iter_mut().zip(&b.ch) {
            x.extend_from_slice(y);
        }
        let batch = loudness::measure(&a);
        let il = interleave(&a);
        let mut m = LoudnessMeter::new(48_000, 2);
        for blk in il.chunks(2 * 701) {
            m.push(blk);
        }
        let near = |x: Option<f64>, y: Option<f64>, tol: f64| (x.unwrap() - y.unwrap()).abs() < tol;
        assert!(near(m.integrated_lufs(), batch.integrated_lufs, 1e-6), "{:?} {:?}", m.integrated_lufs(), batch.integrated_lufs);
        assert!(near(m.momentary_max_lufs(), batch.momentary_max_lufs, 1e-6));
        assert!(near(m.short_term_max_lufs(), batch.short_term_max_lufs, 1e-6));
        assert!(near(m.lra_lu(), batch.lra_lu, 1e-6));
        assert!(near(m.true_peak_dbtp(), batch.true_peak_dbtp, 0.05), "{:?} {:?}", m.true_peak_dbtp(), batch.true_peak_dbtp);
        // leitura atual: no fim, a janela está sobre o trecho de 440 Hz a -28 dBFS (a ponderação K em 440 Hz vale -0,7 dB)
        assert!((m.momentary_lufs().unwrap() + 28.7).abs() < 0.2, "{:?}", m.momentary_lufs());
        assert!((m.short_term_lufs().unwrap() + 28.7).abs() < 0.2);
        assert!((m.duration_s() - 12.0).abs() < 1e-9);
        m.reset();
        assert!(m.integrated_lufs().is_none() && m.true_peak_dbtp().is_none());
    }

    #[test]
    fn streaming_true_peak_finds_intersample_peak() {
        let n = 4800;
        let x: Vec<f32> = (0..n).map(|i| (2.0 * PI * 0.25 * i as f64 + PI / 4.0).sin() as f32).collect();
        let mut m = LoudnessMeter::new(48_000, 1);
        m.push(&x);
        let tp = m.true_peak_dbtp().unwrap();
        assert!(tp > -0.3 && tp < 0.3, "{tp}");
    }
}
