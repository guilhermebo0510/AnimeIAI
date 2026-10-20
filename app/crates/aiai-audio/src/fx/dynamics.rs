//! Dinâmica: compressor (joelho suave, detector de pico ou RMS), limitador com lookahead e detecção de true peak,
//! e noise gate (histerese, hold e piso). Todos têm estado e processam em blocos de qualquer tamanho.

use super::{Effect, Rt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::f64::consts::PI;

/// Coeficiente de um polo para uma constante de tempo em ms (0 = instantâneo).
fn one_pole(ms: f32, fs: f32) -> f32 {
    if ms <= 0.0 { 0.0 } else { (-1.0 / (ms * 0.001 * fs)).exp() }
}

// ---------------------------------------------------------------- compressor

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Detector {
    Peak,
    #[default]
    Rms,
}

/// Curva estática: redução de ganho (dB, <= 0) para um nível de entrada em dB, com joelho suave.
pub fn gain_reduction_db(level_db: f32, threshold_db: f32, ratio: f32, knee_db: f32) -> f32 {
    let over = level_db - threshold_db;
    let slope = 1.0 / ratio.max(1.0) - 1.0;
    let knee = knee_db.max(0.0);
    if 2.0 * over < -knee {
        0.0
    } else if 2.0 * over.abs() <= knee {
        slope * (over + knee / 2.0).powi(2) / (2.0 * knee.max(1e-6))
    } else {
        slope * over
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Compressor {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub knee_db: f32,
    pub makeup_db: f32,
    /// Soma ao ganho de compensação a redução que a curva daria a um sinal de 0 dBFS.
    pub auto_makeup: bool,
    pub detector: Detector,
    /// Janela do detector RMS.
    pub rms_ms: f32,
    /// Detecção ligada entre canais (a imagem estéreo não se desloca). Desligado: cada canal comprime sozinho.
    pub link: bool,
    #[serde(skip)]
    #[schemars(skip)]
    pub st: Rt<Vec<CompState>>,
}

#[derive(Debug, Clone, Copy, Default)]
#[doc(hidden)]
pub struct CompState {
    ms: f32,
    gr_db: f32,
}

impl Default for Compressor {
    fn default() -> Self {
        Compressor {
            threshold_db: -18.0,
            ratio: 3.0,
            attack_ms: 10.0,
            release_ms: 120.0,
            knee_db: 6.0,
            makeup_db: 0.0,
            auto_makeup: false,
            detector: Detector::Rms,
            rms_ms: 30.0,
            link: true,
            st: Rt::default(),
        }
    }
}

impl Effect for Compressor {
    fn process(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32) {
        let nch = channels.max(1);
        let fs = sample_rate as f32;
        let groups = if self.link { 1 } else { nch };
        if self.st.len() != groups {
            self.st.0 = vec![CompState::default(); groups];
        }
        let rms_c = one_pole(self.rms_ms, fs);
        let att = one_pole(self.attack_ms, fs);
        let rel = one_pole(self.release_ms, fs);
        let makeup_db = self.makeup_db + if self.auto_makeup { -gain_reduction_db(0.0, self.threshold_db, self.ratio, self.knee_db) } else { 0.0 };
        let makeup = crate::db_to_lin(makeup_db as f64) as f32;
        for fr in buf.chunks_exact_mut(nch) {
            for g in 0..groups {
                let (a, b) = if self.link { (0, nch) } else { (g, g + 1) };
                let st = &mut self.st[g];
                let level_db = match self.detector {
                    Detector::Rms => {
                        let e: f32 = fr[a..b].iter().map(|s| s * s).sum::<f32>() / (b - a) as f32;
                        st.ms = rms_c * st.ms + (1.0 - rms_c) * e;
                        10.0 * st.ms.max(1e-12).log10()
                    }
                    Detector::Peak => {
                        let p = fr[a..b].iter().fold(0.0f32, |m, s| m.max(s.abs()));
                        20.0 * p.max(1e-6).log10()
                    }
                };
                let target = gain_reduction_db(level_db, self.threshold_db, self.ratio, self.knee_db);
                let coef = if target < st.gr_db { att } else { rel };
                st.gr_db = coef * st.gr_db + (1.0 - coef) * target;
                let gain = 10f32.powf(st.gr_db / 20.0) * makeup;
                for s in &mut fr[a..b] {
                    *s *= gain;
                }
            }
        }
    }

    fn reset(&mut self) {
        self.st.clear();
    }
}

impl Compressor {
    /// Redução de ganho atual (dB, <= 0) do primeiro detector; para o medidor de redução da UI.
    pub fn current_reduction_db(&self) -> f32 {
        self.st.first().map_or(0.0, |s| s.gr_db)
    }
}

// ---------------------------------------------------------------- limitador

/// Metade do comprimento do interpolador de true peak (quadros de contexto de cada lado).
const TP_HALF: usize = 8;
const TP_PHASES: usize = 4;

fn tp_weights() -> &'static [[f32; 2 * TP_HALF]; TP_PHASES - 1] {
    use std::sync::OnceLock;
    static W: OnceLock<[[f32; 2 * TP_HALF]; TP_PHASES - 1]> = OnceLock::new();
    W.get_or_init(|| {
        // Interpolador sinc janelado (Kaiser, beta 8) para as fases 1/4, 2/4 e 3/4 entre as amostras c e c+1.
        // Os pesos valem para x[c-TP_HALF+1 ..= c+TP_HALF].
        let beta = 8.0f64;
        let i0 = |x: f64| -> f64 {
            let (mut s, mut t) = (1.0, 1.0);
            for k in 1..50 {
                t *= (x / 2.0) / k as f64;
                s += t * t;
            }
            s
        };
        let denom = i0(beta);
        let mut out = [[0.0f32; 2 * TP_HALF]; TP_PHASES - 1];
        for p in 1..TP_PHASES {
            let frac = p as f64 / TP_PHASES as f64;
            let mut acc = 0.0;
            let mut row = [0.0f64; 2 * TP_HALF];
            for (m, slot) in row.iter_mut().enumerate() {
                let t = (m as f64 - (TP_HALF as f64 - 1.0)) - frac; // posição da amostra m em relação ao ponto interpolado
                let r = t / TP_HALF as f64;
                let w = i0(beta * (1.0 - r * r).max(0.0).sqrt()) / denom;
                let s = if t.abs() < 1e-12 { 1.0 } else { (PI * t).sin() / (PI * t) };
                *slot = s * w;
                acc += s * w;
            }
            for (m, v) in row.iter().enumerate() {
                out[p - 1][m] = (v / acc) as f32;
            }
        }
        out
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Limiter {
    /// Teto em dBFS (de true peak se `true_peak`, senão de amostra).
    pub ceiling_db: f32,
    pub lookahead_ms: f32,
    pub release_ms: f32,
    /// Detecta picos entre amostras (interpolação 4x): o teto vale para o sinal reconstruído, não só para as amostras.
    pub true_peak: bool,
    #[serde(skip)]
    #[schemars(skip)]
    pub st: Rt<Option<LimState>>,
}

#[derive(Debug, Clone)]
#[doc(hidden)]
pub struct LimState {
    channels: usize,
    rate: u32,
    look: usize,
    /// Atraso total em quadros: lookahead + contexto do interpolador.
    delay: usize,
    ring: Vec<Vec<f32>>, // [canal][posição], comprimento `ring_len`
    ring_len: usize,
    n: usize, // contador de quadros recebidos
    min_q: VecDeque<(usize, f32)>,
    gmin: Vec<f32>,
    gmin_sum: f64,
    gs: f32,
}

impl Default for Limiter {
    fn default() -> Self {
        Limiter { ceiling_db: -1.0, lookahead_ms: 5.0, release_ms: 80.0, true_peak: true, st: Rt::default() }
    }
}

impl Limiter {
    fn init(&self, channels: usize, rate: u32) -> LimState {
        let look = ((self.lookahead_ms.max(0.1) * 0.001 * rate as f32) as usize).max(1);
        let delay = look + TP_HALF;
        let ring_len = delay + 2 * TP_HALF + 2;
        LimState {
            channels,
            rate,
            look,
            delay,
            ring: vec![vec![0.0; ring_len]; channels],
            ring_len,
            n: 0,
            min_q: VecDeque::new(),
            gmin: vec![1.0; look + 1],
            gmin_sum: (look + 1) as f64,
            gs: 1.0,
        }
    }
}

impl Effect for Limiter {
    fn process(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32) {
        let nch = channels.max(1);
        let look_now = ((self.lookahead_ms.max(0.1) * 0.001 * sample_rate as f32) as usize).max(1);
        if self.st.as_ref().is_none_or(|s| s.channels != nch || s.rate != sample_rate || s.look != look_now) {
            self.st.0 = Some(self.init(nch, sample_rate));
        }
        // pequena folga: a rampa de ganho não passa exatamente por r[c] entre amostras vizinhas
        let ceil = crate::db_to_lin(self.ceiling_db as f64 - 0.03) as f32;
        let rel = one_pole(self.release_ms, sample_rate as f32);
        let w = tp_weights();
        let tp = self.true_peak;
        let st = self.st.as_mut().unwrap();
        let (rl, look, delay) = (st.ring_len, st.look, st.delay);
        for fr in buf.chunks_exact_mut(nch) {
            let n = st.n;
            let pos = n % rl;
            for c in 0..nch {
                st.ring[c][pos] = fr[c];
            }
            // pico da posição `cpos` = n - TP_HALF (a amostra e os pontos interpolados entre ela e a seguinte)
            let cidx = n as isize - TP_HALF as isize;
            let mut peak = 0.0f32;
            if cidx >= 0 {
                let c0 = cidx as usize;
                for c in 0..nch {
                    let at = |i: isize| -> f32 { if i < 0 { 0.0 } else { st.ring[c][(i as usize) % rl] } };
                    peak = peak.max(at(cidx).abs());
                    if tp {
                        for p in 0..TP_PHASES - 1 {
                            let mut acc = 0.0f32;
                            for m in 0..2 * TP_HALF {
                                acc += at(c0 as isize - (TP_HALF as isize - 1) + m as isize) * w[p][m];
                            }
                            peak = peak.max(acc.abs());
                        }
                    }
                }
            }
            let r = if peak > ceil { ceil / peak } else { 1.0 };
            // mínimo deslizante das últimas `look + 1` necessidades
            while st.min_q.back().is_some_and(|&(_, v)| v >= r) {
                st.min_q.pop_back();
            }
            st.min_q.push_back((n, r));
            while st.min_q.front().is_some_and(|&(i, _)| i + look < n) {
                st.min_q.pop_front();
            }
            let gmin = st.min_q.front().map_or(1.0, |&(_, v)| v);
            // média móvel do ganho mínimo (rampa de descida antes do pico)
            let slot = n % (look + 1);
            st.gmin_sum += gmin as f64 - st.gmin[slot] as f64;
            st.gmin[slot] = gmin;
            let avg = (st.gmin_sum / (look + 1) as f64) as f32;
            // liberação lenta: o ganho só sobe devagar, nunca passa da média (o teto continua garantido)
            st.gs = if avg < st.gs { avg } else { st.gs + (1.0 - rel) * (avg - st.gs) };
            let gain = st.gs.min(1.0);
            // saída atrasada por `delay` quadros
            let out_idx = n as isize - delay as isize;
            for c in 0..nch {
                fr[c] = if out_idx >= 0 { st.ring[c][(out_idx as usize) % rl] * gain } else { 0.0 };
            }
            st.n += 1;
        }
    }

    fn reset(&mut self) {
        self.st.0 = None;
    }

    fn latency_frames(&self, sample_rate: u32) -> usize {
        ((self.lookahead_ms.max(0.1) * 0.001 * sample_rate as f32) as usize).max(1) + TP_HALF
    }
}

// ---------------------------------------------------------------- noise gate

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct NoiseGate {
    pub threshold_db: f32,
    pub attack_ms: f32,
    /// Tempo que a porta fica aberta depois que o nível cai abaixo do limiar de fechamento.
    pub hold_ms: f32,
    pub release_ms: f32,
    /// Atenuação máxima com a porta fechada (dB, <= 0). -80 = quase mudo.
    pub range_db: f32,
    /// A porta fecha `hysteresis_db` abaixo do limiar de abertura (evita tremor).
    pub hysteresis_db: f32,
    #[serde(skip)]
    #[schemars(skip)]
    pub st: Rt<Option<GateState>>,
}

#[derive(Debug, Clone, Copy)]
#[doc(hidden)]
pub struct GateState {
    env: f32,
    open: bool,
    hold_left: usize,
    gain: f32,
}

impl Default for NoiseGate {
    fn default() -> Self {
        NoiseGate { threshold_db: -50.0, attack_ms: 2.0, hold_ms: 20.0, release_ms: 100.0, range_db: -80.0, hysteresis_db: 3.0, st: Rt::default() }
    }
}

impl Effect for NoiseGate {
    fn process(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32) {
        let nch = channels.max(1);
        let fs = sample_rate as f32;
        let floor = crate::db_to_lin(self.range_db.min(0.0) as f64) as f32;
        let thr_open = crate::db_to_lin(self.threshold_db as f64) as f32;
        let thr_close = crate::db_to_lin((self.threshold_db - self.hysteresis_db.max(0.0)) as f64) as f32;
        let hold = (self.hold_ms.max(0.0) * 0.001 * fs) as usize;
        let att = one_pole(self.attack_ms, fs);
        let rel = one_pole(self.release_ms, fs);
        let det_rel = one_pole(5.0, fs); // queda do seguidor de envelope
        let st = self.st.get_or_insert(GateState { env: 0.0, open: false, hold_left: 0, gain: floor });
        for fr in buf.chunks_exact_mut(nch) {
            let p = fr.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            st.env = if p > st.env { p } else { det_rel * st.env + (1.0 - det_rel) * p };
            if st.env >= thr_open {
                st.open = true;
                st.hold_left = hold;
            } else if st.open && st.env < thr_close {
                if st.hold_left > 0 {
                    st.hold_left -= 1;
                } else {
                    st.open = false;
                }
            }
            let target = if st.open { 1.0 } else { floor };
            let coef = if target > st.gain { att } else { rel };
            st.gain = coef * st.gain + (1.0 - coef) * target;
            for s in fr.iter_mut() {
                *s *= st.gain;
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
    fn compressor_static_curve_and_steady_state() {
        // curva: abaixo do limiar nada; 10 dB acima com razão 4:1 sem joelho → -7,5 dB
        assert_eq!(gain_reduction_db(-30.0, -20.0, 4.0, 0.0), 0.0);
        assert!((gain_reduction_db(-10.0, -20.0, 4.0, 0.0) + 7.5).abs() < 1e-5);
        // joelho: no limiar a redução é pequena e contínua
        let k = gain_reduction_db(-20.0, -20.0, 4.0, 6.0);
        assert!((k + 0.5625).abs() < 1e-5, "{k}");
        // seno de pico -10 dBFS (RMS -13,01) com limiar -20 e 4:1: redução de 6,99 * 0,75 = 5,24 dB
        let fs = 48_000;
        let mut x = sine_il(fs, 1000.0, 1.5, db_to_lin(-10.0), 1);
        let r0 = rms(&x[48_000..]);
        let mut c = Compressor { threshold_db: -20.0, ratio: 4.0, knee_db: 0.0, attack_ms: 5.0, release_ms: 50.0, ..Default::default() };
        c.process(&mut x, 1, fs);
        let g = db(rms(&x[48_000..]) / r0);
        assert!((g + 5.24).abs() < 0.3, "{g}");
        // abaixo do limiar: transparente
        let mut y = sine_il(fs, 1000.0, 1.0, db_to_lin(-40.0), 1);
        let y0 = y.clone();
        Compressor { threshold_db: -20.0, knee_db: 0.0, ..Default::default() }.process(&mut y, 1, fs);
        assert!(y.iter().zip(&y0).all(|(a, b)| (a - b).abs() < 1e-6));
    }

    #[test]
    fn compressor_attack_release_and_makeup() {
        let fs = 48_000;
        // rajada alta no meio de um sinal baixo: durante a rajada o ganho cai, depois volta
        let mut x = sine_il(fs, 500.0, 2.0, db_to_lin(-40.0), 1);
        for (i, s) in x.iter_mut().enumerate().skip(fs as usize / 2).take(fs as usize / 2) {
            *s = 0.5 * (2.0 * std::f32::consts::PI * 500.0 * i as f32 / fs as f32).sin();
        }
        let mut c = Compressor { threshold_db: -20.0, ratio: 10.0, attack_ms: 2.0, release_ms: 100.0, knee_db: 0.0, ..Default::default() };
        let mut y = x.clone();
        c.process(&mut y, 1, fs);
        let during = db(rms(&y[fs as usize * 3 / 4..fs as usize]) / rms(&x[fs as usize * 3 / 4..fs as usize]));
        assert!((during + 9.9).abs() < 0.5, "{during}");
        let after = db(rms(&y[fs as usize * 3 / 2..]) / rms(&x[fs as usize * 3 / 2..]));
        assert!(after > -0.5, "{after}");
        // auto makeup compensa a redução de um sinal de 0 dB de nível; um seno de pico 0,99 (RMS -3,1 dB) sobra +2,3 dB
        let mut z = sine_il(fs, 500.0, 1.0, 0.99, 1);
        Compressor { threshold_db: -20.0, ratio: 4.0, knee_db: 0.0, auto_makeup: true, ..Default::default() }.process(&mut z, 1, fs);
        let pk = peak(&z[24_000..]);
        assert!((pk - 1.293).abs() < 0.03, "{pk}");
        // estéreo ligado: o canal fraco recebe a mesma redução que o forte
        let mut s = vec![0.0f32; 2 * 48_000];
        for i in 0..48_000 {
            let v = (i as f32 * 0.07).sin();
            s[2 * i] = 0.8 * v;
            s[2 * i + 1] = 0.08 * v;
        }
        let mut linked = s.clone();
        Compressor { threshold_db: -30.0, ratio: 8.0, knee_db: 0.0, link: true, ..Default::default() }.process(&mut linked, 2, 48_000);
        let (rl, rr) = (rms_ch(&linked[48_000..], 2, 0) / rms_ch(&s[48_000..], 2, 0), rms_ch(&linked[48_000..], 2, 1) / rms_ch(&s[48_000..], 2, 1));
        assert!((rl - rr).abs() < 1e-3, "{rl} {rr}");
        assert!(rl < 0.9);
    }

    #[test]
    fn limiter_respects_ceiling_including_true_peak() {
        let fs = 44_100u32;
        // fs/4 com fase 45°: amostras ±0,707·A mas o pico real é A. A = 1,0 → true peak 0 dBTP
        let n = 44_100;
        let mut x: Vec<f32> = (0..n).map(|i| (2.0 * PI32 * 0.25 * i as f32 + PI32 / 4.0).sin()).collect();
        // com um trecho mais alto (ganho 2x no meio) para o limitador agir de verdade
        for v in x[10_000..30_000].iter_mut() {
            *v *= 2.0;
        }
        let mut lim = Limiter { ceiling_db: -3.0, ..Default::default() };
        let lat = lim.latency_frames(fs);
        let mut padded = x.clone();
        padded.extend(std::iter::repeat_n(0.0, lat));
        lim.process(&mut padded, 1, fs);
        let y = crate::buf::AudioBuf::mono(fs, padded[lat..].to_vec());
        let tp = crate::loudness::true_peak_db(&y);
        assert!(tp <= -3.0 + 0.1, "true peak {tp}");
        assert!(tp > -3.6, "não deve limitar demais: {tp}");
        // sem true peak: o teto vale só para amostras (e as intersample passam)
        let mut lim2 = Limiter { ceiling_db: -3.0, true_peak: false, ..Default::default() };
        let mut p2 = x.clone();
        p2.extend(std::iter::repeat_n(0.0, lat));
        lim2.process(&mut p2, 1, fs);
        let y2 = crate::buf::AudioBuf::mono(fs, p2[lat..].to_vec());
        assert!(crate::loudness::sample_peak_db(&y2) <= -3.0 + 0.05);
        assert!(crate::loudness::true_peak_db(&y2) > tp + 1.0);
    }

    #[test]
    fn limiter_transparent_below_ceiling_and_block_invariant() {
        let fs = 48_000;
        let x = sine_il(fs, 440.0, 0.5, 0.3, 2);
        let mut a = x.clone();
        let mut l = Limiter::default();
        let lat = l.latency_frames(fs);
        a.extend(std::iter::repeat_n(0.0, lat * 2));
        l.process(&mut a, 2, fs);
        // abaixo do teto só atrasa
        for i in 0..(x.len() / 2 - lat) {
            for c in 0..2 {
                assert!((a[(i + lat) * 2 + c] - x[i * 2 + c]).abs() < 1e-6, "{i}");
            }
        }
        // blocos de tamanhos variados dão o mesmo resultado de uma só chamada
        let mut loud = sine_il(fs, 300.0, 0.4, 1.5, 2);
        let mut whole = loud.clone();
        Limiter::default().process(&mut whole, 2, fs);
        let mut lb = Limiter::default();
        let mut off = 0;
        for (k, size) in [100usize, 777, 5, 4000, 123].iter().cycle().enumerate() {
            if off >= loud.len() || k > 1000 {
                break;
            }
            let end = (off + size * 2).min(loud.len());
            lb.process(&mut loud[off..end], 2, fs);
            off = end;
        }
        assert!(whole.iter().zip(&loud).all(|(a, b)| (a - b).abs() < 1e-6));
        // reset zera a memória
        lb.reset();
    }

    #[test]
    fn gate_attenuates_noise_and_passes_signal() {
        let fs = 48_000;
        // 1 s de tom a -10 dBFS, depois 1 s de ruído a -65 dBFS
        let mut x = sine_il(fs, 1000.0, 1.0, db_to_lin(-10.0), 1);
        let noise = noise_il(3, 1.0, db_to_lin(-65.0), 1);
        x.extend_from_slice(&noise);
        let (tone_rms, noise_rms) = (rms(&x[..48_000]), rms(&x[48_000..]));
        let mut g = NoiseGate { threshold_db: -50.0, range_db: -60.0, release_ms: 30.0, ..Default::default() };
        g.process(&mut x, 1, fs);
        let tone_gain = db(rms(&x[2000..46_000]) / rms_slice(&sine_il(fs, 1000.0, 1.0, db_to_lin(-10.0), 1)[2000..46_000]));
        assert!(tone_gain.abs() < 0.3, "{tone_gain}");
        let red = db(rms(&x[60_000..]) / noise_rms);
        assert!((red + 60.0).abs() < 6.0, "{red}");
        let _ = tone_rms;
    }

    #[test]
    fn gate_hold_and_hysteresis() {
        let fs = 48_000;
        // tom que fica exatamente entre o limiar de fechamento e o de abertura depois de aberto: continua aberto
        let mut x = sine_il(fs, 1000.0, 0.5, db_to_lin(-30.0), 1);
        let lo = sine_il(fs, 1000.0, 0.5, db_to_lin(-52.0), 1); // abaixo de -50 (abre) e acima de -53 (fecha)
        x.extend_from_slice(&lo);
        let mut g = NoiseGate { threshold_db: -50.0, hysteresis_db: 3.0, hold_ms: 10.0, release_ms: 20.0, range_db: -60.0, ..Default::default() };
        g.process(&mut x, 1, fs);
        let tail = &x[24_000 + 2400..];
        let want = db_to_lin(-52.0) / 2f32.sqrt();
        assert!((rms(tail) / want - 1.0).abs() < 0.1, "{}", rms(tail) / want);
        // sem histerese a mesma situação fecha
        let mut y = sine_il(fs, 1000.0, 0.5, db_to_lin(-30.0), 1);
        y.extend_from_slice(&lo);
        let mut g2 = NoiseGate { threshold_db: -50.0, hysteresis_db: 0.0, hold_ms: 10.0, release_ms: 20.0, range_db: -60.0, ..Default::default() };
        g2.process(&mut y, 1, fs);
        assert!(rms(&y[24_000 + 4800..]) < want * 0.1);
    }

    const PI32: f32 = std::f32::consts::PI;

    fn rms_slice(x: &[f32]) -> f32 {
        rms(x)
    }

    fn db_to_lin(d: f32) -> f32 {
        10f32.powf(d / 20.0)
    }
}
