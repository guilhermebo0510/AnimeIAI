//! Loudness ITU-R BS.1770-4 / EBU R128 (Tech 3341/3342): ponderação K, momentâneo (400 ms), curto prazo (3 s),
//! integrado com porta absoluta (-70 LUFS) e relativa (-10 LU), faixa de loudness (LRA, porta -20 LU),
//! true peak com oversampling 4x e normalização com limitador de true peak.
//!
//! Canais: 1/2/3 canais pesam 1,0; em 6 canais (5.1) o LFE (índice 3) fica de fora e os surround (4, 5) pesam 1,41.

use crate::buf::AudioBuf;
use crate::dsp::{Biquad, BiquadCoeffs, LimiterParams, limit};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Estágios da ponderação K (BS.1770-4) para a taxa dada. Em 48 kHz reproduzem os coeficientes da norma
/// (`k_weighting_48k_matches_spec` testa isso); em outras taxas são recalculados pela mesma topologia analógica.
pub fn k_weight_coeffs(fs: f64) -> [BiquadCoeffs; 2] {
    // Estágio 1: prateleira de agudos (cabeça).
    let f0 = 1681.974450955533;
    let g = 3.999843853973347;
    let q = 0.7071752369554196;
    let k = (PI * f0 / fs).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = BiquadCoeffs {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    };
    // Estágio 2: passa-altas (RLB).
    let f0 = 38.13547087602444;
    let q = 0.5003270373238773;
    let k = (PI * f0 / fs).tan();
    let a0 = 1.0 + k / q + k * k;
    let hp = BiquadCoeffs { b0: 1.0, b1: -2.0, b2: 1.0, a1: 2.0 * (k * k - 1.0) / a0, a2: (1.0 - k / q + k * k) / a0 };
    [shelf, hp]
}

fn channel_weight(nch: usize, c: usize) -> f64 {
    match (nch, c) {
        (6, 3) => 0.0,
        (6, 4) | (6, 5) => 1.41,
        _ => 1.0,
    }
}

/// Potência ponderada por amostra: `Σ_c G_c · y_c[n]²` (y = sinal filtrado por K).
fn weighted_power(buf: &AudioBuf) -> Vec<f64> {
    let n = buf.frames();
    let nch = buf.channels();
    let co = k_weight_coeffs(buf.rate as f64);
    let mut z = vec![0.0f64; n];
    for (c, x) in buf.ch.iter().enumerate() {
        let g = channel_weight(nch, c);
        if g == 0.0 {
            continue;
        }
        let mut s1 = Biquad::new(co[0]);
        let mut s2 = Biquad::new(co[1]);
        for (i, &v) in x.iter().enumerate() {
            let y = s2.process(s1.process(v as f64));
            z[i] += g * y * y;
        }
    }
    z
}

fn to_lufs(mean_square: f64) -> f64 {
    if mean_square <= 0.0 { f64::NEG_INFINITY } else { -0.691 + 10.0 * mean_square.log10() }
}

/// Energias médias de janelas de `win` amostras a cada `step`.
fn windows(prefix: &[f64], win: usize, step: usize) -> Vec<f64> {
    let n = prefix.len() - 1;
    if win == 0 || n < win {
        return Vec::new();
    }
    let count = (n - win) / step + 1;
    (0..count).map(|j| (prefix[j * step + win] - prefix[j * step]) / win as f64).collect()
}

fn prefix_sums(z: &[f64]) -> Vec<f64> {
    let mut p = Vec::with_capacity(z.len() + 1);
    p.push(0.0);
    let mut acc = 0.0;
    for v in z {
        acc += v;
        p.push(acc);
    }
    p
}

/// Loudness integrado (LUFS) com portas; `None` se não houver bloco acima de -70 LUFS.
fn gated_integrated(blocks: &[f64]) -> Option<f64> {
    let abs_thr = 10f64.powf((-70.0 + 0.691) / 10.0);
    let above: Vec<f64> = blocks.iter().copied().filter(|&e| e > abs_thr).collect();
    if above.is_empty() {
        return None;
    }
    let mean = above.iter().sum::<f64>() / above.len() as f64;
    let rel_thr = mean * 10f64.powf(-10.0 / 10.0);
    let kept: Vec<f64> = above.into_iter().filter(|&e| e > rel_thr).collect();
    if kept.is_empty() {
        return None;
    }
    Some(to_lufs(kept.iter().sum::<f64>() / kept.len() as f64))
}

fn lra_from_short_term(st: &[f64]) -> Option<f64> {
    let abs_thr = 10f64.powf((-70.0 + 0.691) / 10.0);
    let above: Vec<f64> = st.iter().copied().filter(|&e| e > abs_thr).collect();
    if above.is_empty() {
        return None;
    }
    let mean = above.iter().sum::<f64>() / above.len() as f64;
    let rel_thr = mean * 10f64.powf(-20.0 / 10.0);
    let mut l: Vec<f64> = above.into_iter().filter(|&e| e > rel_thr).map(to_lufs).collect();
    if l.len() < 2 {
        return Some(0.0);
    }
    l.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |p: f64| -> f64 {
        let pos = p * (l.len() - 1) as f64;
        let (i, f) = (pos.floor() as usize, pos.fract());
        if i + 1 < l.len() { l[i] * (1.0 - f) + l[i + 1] * f } else { l[i] }
    };
    Some(pct(0.95) - pct(0.10))
}

// ---------------------------------------------------------------- true peak

const TP_PHASES: usize = 4;
const TP_TAPS_PER_PHASE: usize = 24;

fn tp_filter() -> &'static [[f64; TP_TAPS_PER_PHASE]; TP_PHASES] {
    use std::sync::OnceLock;
    static F: OnceLock<[[f64; TP_TAPS_PER_PHASE]; TP_PHASES]> = OnceLock::new();
    F.get_or_init(|| {
        // Interpolador sinc janelado (Kaiser β = 8) de 4 fases; cada fase com ganho DC unitário.
        let mut out = [[0.0f64; TP_TAPS_PER_PHASE]; TP_PHASES];
        let beta = 8.0;
        let i0 = |x: f64| -> f64 {
            let (mut s, mut t) = (1.0, 1.0);
            for k in 1..50 {
                t *= (x / 2.0) / k as f64;
                s += t * t;
            }
            s
        };
        let denom = i0(beta);
        for p in 0..TP_PHASES {
            let mut acc = 0.0;
            for m in 0..TP_TAPS_PER_PHASE {
                // posição (em amostras de entrada) relativa ao ponto interpolado; centro entre as duas amostras do meio
                let t = (m as f64 - (TP_TAPS_PER_PHASE as f64 / 2.0 - 1.0)) - p as f64 / TP_PHASES as f64;
                let r = t / (TP_TAPS_PER_PHASE as f64 / 2.0);
                let w = i0(beta * (1.0 - r * r).max(0.0).sqrt()) / denom;
                let s = if t.abs() < 1e-12 { 1.0 } else { (PI * t).sin() / (PI * t) };
                out[p][m] = s * w;
                acc += s * w;
            }
            for m in 0..TP_TAPS_PER_PHASE {
                out[p][m] /= acc;
            }
        }
        out
    })
}

/// True peak (linear) por canal com interpolação 4x. Para taxas ≥ 96 kHz a amostragem já cobre o intersample e o
/// oversampling 4x continua válido (apenas redundante).
pub fn true_peak_linear(buf: &AudioBuf) -> f64 {
    let f = tp_filter();
    let mut peak = 0.0f64;
    let h = TP_TAPS_PER_PHASE / 2;
    for x in &buf.ch {
        let n = x.len();
        for &s in x {
            peak = peak.max((s as f64).abs());
        }
        for i in 0..n {
            // Fase 0 reproduz a amostra; fases 1..3 interpolam entre x[i] e x[i+1].
            for p in 1..TP_PHASES {
                let mut acc = 0.0f64;
                for m in 0..TP_TAPS_PER_PHASE {
                    let idx = i as isize + m as isize - (h as isize - 1);
                    if idx >= 0 && (idx as usize) < n {
                        acc += x[idx as usize] as f64 * f[p][m];
                    }
                }
                peak = peak.max(acc.abs());
            }
        }
    }
    peak
}

pub fn true_peak_db(buf: &AudioBuf) -> f64 {
    crate::lin_to_db(true_peak_linear(buf))
}

pub fn sample_peak_db(buf: &AudioBuf) -> f64 {
    crate::lin_to_db(buf.peak() as f64)
}

// ---------------------------------------------------------------- relatório

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct LoudnessReport {
    pub sample_rate: u32,
    pub channels: usize,
    pub duration_s: f64,
    /// LUFS integrado com portas (`null` se silêncio/curto demais).
    pub integrated_lufs: Option<f64>,
    pub momentary_max_lufs: Option<f64>,
    pub short_term_max_lufs: Option<f64>,
    /// Faixa de loudness (LU); `null` se < 3 s.
    pub lra_lu: Option<f64>,
    pub true_peak_dbtp: Option<f64>,
    pub sample_peak_dbfs: Option<f64>,
    /// Momentâneo a cada 100 ms (LUFS; `null` = -inf) — para gráfico.
    pub momentary_series: Vec<Option<f64>>,
    pub short_term_series: Vec<Option<f64>>,
}

fn opt(v: f64) -> Option<f64> {
    if v.is_finite() { Some(v) } else { None }
}

pub fn measure(buf: &AudioBuf) -> LoudnessReport {
    let fs = buf.rate as usize;
    let z = weighted_power(buf);
    let pre = prefix_sums(&z);
    let step = fs / 10;
    let momentary = windows(&pre, fs * 4 / 10, step.max(1));
    let short = windows(&pre, fs * 3, step.max(1));
    let m_series: Vec<f64> = momentary.iter().map(|&e| to_lufs(e)).collect();
    let s_series: Vec<f64> = short.iter().map(|&e| to_lufs(e)).collect();
    let max_of = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let tp = true_peak_db(buf);
    let sp = sample_peak_db(buf);
    LoudnessReport {
        sample_rate: buf.rate,
        channels: buf.channels(),
        duration_s: buf.duration_s(),
        integrated_lufs: gated_integrated(&momentary),
        momentary_max_lufs: opt(max_of(&m_series)),
        short_term_max_lufs: opt(max_of(&s_series)),
        lra_lu: lra_from_short_term(&short),
        true_peak_dbtp: opt(tp),
        sample_peak_dbfs: opt(sp),
        momentary_series: m_series.iter().map(|&v| opt(v)).collect(),
        short_term_series: s_series.iter().map(|&v| opt(v)).collect(),
    }
}

pub fn integrated_lufs(buf: &AudioBuf) -> Option<f64> {
    let fs = buf.rate as usize;
    let pre = prefix_sums(&weighted_power(buf));
    gated_integrated(&windows(&pre, fs * 4 / 10, (fs / 10).max(1)))
}

// ---------------------------------------------------------------- normalização

/// Presets de entrega.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LoudnessPreset {
    /// ARIB TR-B32 (TV japonesa): -24 LKFS, true peak ≤ -1 dBTP.
    Arib,
    /// EBU R128: -23 LUFS, ≤ -1 dBTP.
    Ebu,
    /// Streaming (referência): -14 LUFS, ≤ -1 dBTP.
    Streaming,
}

impl LoudnessPreset {
    pub fn target_lufs(self) -> f64 {
        match self {
            LoudnessPreset::Arib => -24.0,
            LoudnessPreset::Ebu => -23.0,
            LoudnessPreset::Streaming => -14.0,
        }
    }

    pub fn true_peak_ceiling_db(self) -> f64 {
        -1.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct NormalizeReport {
    pub target_lufs: f64,
    pub ceiling_dbtp: f64,
    pub before_lufs: Option<f64>,
    pub gain_db: f64,
    pub after_lufs: Option<f64>,
    pub after_true_peak_dbtp: Option<f64>,
    /// O limitador atuou (o ganho sozinho passava do teto).
    pub limited: bool,
    pub iterations: u32,
}

/// Leva o áudio ao loudness integrado `target_lufs` sem passar de `ceiling_dbtp` de true peak: ganho, limitador de pico
/// com lookahead e verificação por true peak (reduz o teto interno até o true peak caber). Se o limitador custar
/// loudness, o ganho sobe para compensar (até convergir em ±0,05 LU ou 8 iterações).
pub fn normalize(buf: &mut AudioBuf, target_lufs: f64, ceiling_dbtp: f64) -> NormalizeReport {
    let before = integrated_lufs(buf);
    let mut rep = NormalizeReport { target_lufs, ceiling_dbtp, before_lufs: before, gain_db: 0.0, after_lufs: before, after_true_peak_dbtp: opt(true_peak_db(buf)), limited: false, iterations: 0 };
    let Some(before_l) = before else {
        return rep;
    };
    let original = buf.clone();
    let mut gain = target_lufs - before_l;
    let mut best: Option<(AudioBuf, f64, f64, f64, bool)> = None; // (buf, gain, lufs, tp, limited)
    for it in 0..8u32 {
        rep.iterations = it + 1;
        let mut trial = original.clone();
        trial.apply_gain_db(gain);
        let mut limited = false;
        let mut internal_ceiling = ceiling_dbtp;
        for _ in 0..6 {
            let tp = true_peak_db(&trial);
            if tp <= ceiling_dbtp + 1e-3 {
                break;
            }
            limited = true;
            let excess = tp - ceiling_dbtp;
            internal_ceiling -= excess.max(0.05) + 0.02;
            let mut t2 = original.clone();
            t2.apply_gain_db(gain);
            limit(&mut t2, &LimiterParams { ceiling_db: internal_ceiling as f32, ..Default::default() });
            trial = t2;
        }
        let l = integrated_lufs(&trial).unwrap_or(f64::NEG_INFINITY);
        let tp = true_peak_db(&trial);
        let err = target_lufs - l;
        let ok_peak = tp <= ceiling_dbtp + 0.05;
        let better = match &best {
            None => true,
            Some((_, _, bl, btp, _)) => {
                let bok = *btp <= ceiling_dbtp + 0.05;
                (ok_peak && !bok) || (ok_peak == bok && (target_lufs - l).abs() < (target_lufs - *bl).abs())
            }
        };
        if better {
            best = Some((trial, gain, l, tp, limited));
        }
        if err.abs() <= 0.05 || !l.is_finite() {
            break;
        }
        gain += err;
    }
    if let Some((b, g, l, tp, lim)) = best {
        *buf = b;
        rep.gain_db = g;
        rep.after_lufs = opt(l);
        rep.after_true_peak_dbtp = opt(tp);
        rep.limited = lim;
    }
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, secs: f64, freq: f64, dbfs: f64, ch: usize) -> AudioBuf {
        let n = (rate as f64 * secs) as usize;
        let a = crate::db_to_lin(dbfs);
        let x: Vec<f32> = (0..n).map(|i| (a * (2.0 * PI * freq * i as f64 / rate as f64).sin()) as f32).collect();
        AudioBuf { rate, ch: vec![x; ch] }
    }

    #[test]
    fn k_weighting_48k_matches_spec() {
        let c = k_weight_coeffs(48_000.0);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // BS.1770-4, tabelas 1 e 2.
        assert!(near(c[0].b0, 1.53512485958697) && near(c[0].b1, -2.69169618940638) && near(c[0].b2, 1.19839281085285));
        assert!(near(c[0].a1, -1.69065929318241) && near(c[0].a2, 0.73248077421585));
        assert!(near(c[1].b0, 1.0) && near(c[1].b1, -2.0) && near(c[1].b2, 1.0));
        assert!(near(c[1].a1, -1.99004745483398) && near(c[1].a2, 0.99007225036621), "{:?}", c[1]);
    }

    #[test]
    fn ebu_tech3341_sine_minus23() {
        // Caso 1 do Tech 3341: seno 1 kHz estéreo a -23 dBFS por 20 s → -23,0 LUFS (±0,1).
        let b = tone(48_000, 20.0, 1000.0, -23.0, 2);
        let r = measure(&b);
        let i = r.integrated_lufs.unwrap();
        assert!((i + 23.0).abs() < 0.1, "integrado {i}");
        let m = r.momentary_max_lufs.unwrap();
        assert!((m + 23.0).abs() < 0.1, "momentâneo {m}");
        let s = r.short_term_max_lufs.unwrap();
        assert!((s + 23.0).abs() < 0.1, "curto prazo {s}");
        // Seno puro: pico de amostra = -23 dBFS.
        assert!((r.sample_peak_dbfs.unwrap() + 23.0).abs() < 0.05);
        // Mesma medida em outras taxas (ponderação recalculada).
        for rate in [44_100u32, 96_000] {
            let i = integrated_lufs(&tone(rate, 20.0, 1000.0, -23.0, 2)).unwrap();
            assert!((i + 23.0).abs() < 0.15, "{rate}: {i}");
        }
    }

    #[test]
    fn mono_is_3db_below_dual_mono() {
        let s = integrated_lufs(&tone(48_000, 10.0, 1000.0, -20.0, 2)).unwrap();
        let m = integrated_lufs(&tone(48_000, 10.0, 1000.0, -20.0, 1)).unwrap();
        assert!((s - m - 3.01).abs() < 0.05, "{s} {m}");
    }

    #[test]
    fn gating_ignores_quiet_part() {
        // 10 s a -23 LUFS + 10 s a -60 dBFS: a parte baixa fica abaixo da porta relativa e não conta.
        let mut a = tone(48_000, 10.0, 1000.0, -23.0, 2);
        let b = tone(48_000, 10.0, 1000.0, -60.0, 2);
        for (x, y) in a.ch.iter_mut().zip(&b.ch) {
            x.extend_from_slice(y);
        }
        let i = integrated_lufs(&a).unwrap();
        assert!((i + 23.0).abs() < 0.1, "{i}");
        // Abaixo de -70 LUFS: sem medida.
        assert!(integrated_lufs(&tone(48_000, 5.0, 1000.0, -80.0, 2)).is_none());
    }

    #[test]
    fn lra_tech3342_ten_lu_step() {
        // Tech 3342, caso 1: 20 s a -20 LUFS e 20 s a -30 LUFS → LRA = 10 LU (±1).
        let mut a = tone(48_000, 20.0, 1000.0, -20.0, 2);
        let b = tone(48_000, 20.0, 1000.0, -30.0, 2);
        for (x, y) in a.ch.iter_mut().zip(&b.ch) {
            x.extend_from_slice(y);
        }
        let lra = measure(&a).lra_lu.unwrap();
        assert!((lra - 10.0).abs() < 1.0, "LRA {lra}");
        // Tom constante: LRA ≈ 0.
        assert!(measure(&tone(48_000, 10.0, 1000.0, -23.0, 2)).lra_lu.unwrap() < 0.1);
    }

    #[test]
    fn true_peak_finds_intersample_peaks() {
        // Seno em fs/4 com fase 45°: as amostras valem ±0,707 (-3,01 dBFS) mas o pico real é 1,0 (0 dBTP).
        let n = 4800;
        let x: Vec<f32> = (0..n).map(|i| (2.0 * PI * 0.25 * i as f64 + PI / 4.0).sin() as f32).collect();
        let b = AudioBuf::mono(48_000, x);
        assert!((sample_peak_db(&b) + 3.01).abs() < 0.05);
        let tp = true_peak_db(&b);
        assert!(tp > -0.3 && tp < 0.3, "true peak {tp}");
    }

    #[test]
    fn normalize_hits_target_and_ceiling() {
        // Tom a -30 dBFS → -24 LUFS exige +3 dB (estéreo): sem limitador.
        let mut b = tone(48_000, 12.0, 1000.0, -27.0, 2);
        let r = normalize(&mut b, -24.0, -1.0);
        assert!((r.after_lufs.unwrap() + 24.0).abs() < 0.1, "{r:?}");
        assert!(!r.limited);
        // Sinal de pico alto: baixa densidade (clicks) a -24 LUFS exige limitador e respeita -1 dBTP.
        let mut x = vec![0.0f32; 48_000 * 12];
        for i in (0..x.len()).step_by(2400) {
            for k in 0..120 {
                x[i + k] = 0.9 * (k as f32 * 0.4).sin();
            }
        }
        let mut b = AudioBuf { rate: 48_000, ch: vec![x.clone(), x] };
        let r = normalize(&mut b, -24.0, -1.0);
        assert!(r.after_true_peak_dbtp.unwrap() <= -0.95, "{r:?}");
        assert!((r.after_lufs.unwrap() + 24.0).abs() < 1.0, "{r:?}");
    }
}
