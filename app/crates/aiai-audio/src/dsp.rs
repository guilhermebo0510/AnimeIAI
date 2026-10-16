//! DSP offline sobre buffers inteiros: biquads (RBJ), EQ de 3 bandas, compressor RMS, limitador com lookahead,
//! reverb Freeverb e ducking por sidechain. Tudo determinístico e sem alocação por amostra.

use crate::buf::AudioBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::f64::consts::PI;

// ---------------------------------------------------------------- biquad

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadCoeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl BiquadCoeffs {
    pub const IDENTITY: BiquadCoeffs = BiquadCoeffs { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn norm(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        BiquadCoeffs { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    fn base(fs: f64, f: f64, q: f64) -> (f64, f64, f64) {
        let w0 = 2.0 * PI * (f / fs).clamp(1e-5, 0.499);
        (w0.cos(), w0.sin(), w0.sin() / (2.0 * q.max(1e-3)))
    }

    pub fn lowpass(fs: f64, f: f64, q: f64) -> Self {
        let (c, _s, al) = Self::base(fs, f, q);
        Self::norm((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }

    pub fn highpass(fs: f64, f: f64, q: f64) -> Self {
        let (c, _s, al) = Self::base(fs, f, q);
        Self::norm((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }

    /// Passa-faixa com ganho de pico unitário.
    pub fn bandpass(fs: f64, f: f64, q: f64) -> Self {
        let (c, s, al) = Self::base(fs, f, q);
        let _ = s;
        Self::norm(al, 0.0, -al, 1.0 + al, -2.0 * c, 1.0 - al)
    }

    pub fn peaking(fs: f64, f: f64, q: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let (c, _s, al) = Self::base(fs, f, q);
        Self::norm(1.0 + al * a, -2.0 * c, 1.0 - al * a, 1.0 + al / a, -2.0 * c, 1.0 - al / a)
    }

    pub fn low_shelf(fs: f64, f: f64, q: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let (c, _s, al) = Self::base(fs, f, q);
        let t = 2.0 * a.sqrt() * al;
        Self::norm(
            a * ((a + 1.0) - (a - 1.0) * c + t),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - t),
            (a + 1.0) + (a - 1.0) * c + t,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - t,
        )
    }

    pub fn high_shelf(fs: f64, f: f64, q: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let (c, _s, al) = Self::base(fs, f, q);
        let t = 2.0 * a.sqrt() * al;
        Self::norm(
            a * ((a + 1.0) + (a - 1.0) * c + t),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - t),
            (a + 1.0) - (a - 1.0) * c + t,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - t,
        )
    }

    /// Resposta em magnitude (dB) na frequência `f`.
    pub fn magnitude_db(&self, fs: f64, f: f64) -> f64 {
        let w = 2.0 * PI * f / fs;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
    }
}

/// Biquad em forma transposta II com estado em f64.
#[derive(Debug, Clone)]
pub struct Biquad {
    pub c: BiquadCoeffs,
    z1: f64,
    z2: f64,
}

impl Biquad {
    pub fn new(c: BiquadCoeffs) -> Self {
        Biquad { c, z1: 0.0, z2: 0.0 }
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.c.b0 * x + self.z1;
        self.z1 = self.c.b1 * x - self.c.a1 * y + self.z2;
        self.z2 = self.c.b2 * x - self.c.a2 * y;
        y
    }
}

pub fn filter_in_place(x: &mut [f32], c: BiquadCoeffs) {
    let mut f = Biquad::new(c);
    for s in x.iter_mut() {
        *s = f.process(*s as f64) as f32;
    }
}

// ---------------------------------------------------------------- EQ de 3 bandas

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Eq3Params {
    #[serde(default)]
    pub low_db: f32,
    #[serde(default)]
    pub mid_db: f32,
    #[serde(default)]
    pub high_db: f32,
    #[serde(default = "d_low_hz")]
    pub low_hz: f32,
    #[serde(default = "d_mid_hz")]
    pub mid_hz: f32,
    #[serde(default = "d_mid_q")]
    pub mid_q: f32,
    #[serde(default = "d_high_hz")]
    pub high_hz: f32,
}

fn d_low_hz() -> f32 {
    200.0
}
fn d_mid_hz() -> f32 {
    1000.0
}
fn d_mid_q() -> f32 {
    0.9
}
fn d_high_hz() -> f32 {
    4000.0
}

impl Default for Eq3Params {
    fn default() -> Self {
        Eq3Params { low_db: 0.0, mid_db: 0.0, high_db: 0.0, low_hz: 200.0, mid_hz: 1000.0, mid_q: 0.9, high_hz: 4000.0 }
    }
}

pub fn eq3(buf: &mut AudioBuf, p: &Eq3Params) {
    let fs = buf.rate as f64;
    let mut stages = Vec::new();
    if p.low_db != 0.0 {
        stages.push(BiquadCoeffs::low_shelf(fs, p.low_hz as f64, std::f64::consts::FRAC_1_SQRT_2, p.low_db as f64));
    }
    if p.mid_db != 0.0 {
        stages.push(BiquadCoeffs::peaking(fs, p.mid_hz as f64, p.mid_q as f64, p.mid_db as f64));
    }
    if p.high_db != 0.0 {
        stages.push(BiquadCoeffs::high_shelf(fs, p.high_hz as f64, std::f64::consts::FRAC_1_SQRT_2, p.high_db as f64));
    }
    for c in &mut buf.ch {
        for st in &stages {
            filter_in_place(c, *st);
        }
    }
}

// ---------------------------------------------------------------- compressor

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct CompressorParams {
    #[serde(default = "d_thr")]
    pub threshold_db: f32,
    #[serde(default = "d_ratio")]
    pub ratio: f32,
    #[serde(default = "d_att")]
    pub attack_ms: f32,
    #[serde(default = "d_rel")]
    pub release_ms: f32,
    #[serde(default = "d_knee")]
    pub knee_db: f32,
    #[serde(default)]
    pub makeup_db: f32,
    /// Janela do detector RMS.
    #[serde(default = "d_rms")]
    pub rms_ms: f32,
}

fn d_thr() -> f32 {
    -18.0
}
fn d_ratio() -> f32 {
    3.0
}
fn d_att() -> f32 {
    10.0
}
fn d_rel() -> f32 {
    120.0
}
fn d_knee() -> f32 {
    6.0
}
fn d_rms() -> f32 {
    30.0
}

impl Default for CompressorParams {
    fn default() -> Self {
        CompressorParams { threshold_db: -18.0, ratio: 3.0, attack_ms: 10.0, release_ms: 120.0, knee_db: 6.0, makeup_db: 0.0, rms_ms: 30.0 }
    }
}

fn one_pole(ms: f32, fs: f32) -> f32 {
    if ms <= 0.0 { 0.0 } else { (-1.0 / (ms * 0.001 * fs)).exp() }
}

/// Curva estática do compressor com joelho suave: redução de ganho (dB, ≤ 0) para um nível de entrada em dB.
pub fn compressor_gain_db(level_db: f32, p: &CompressorParams) -> f32 {
    let over = level_db - p.threshold_db;
    let slope = 1.0 / p.ratio.max(1.0) - 1.0;
    let knee = p.knee_db.max(0.0);
    if 2.0 * over < -knee {
        0.0
    } else if 2.0 * over.abs() <= knee {
        slope * (over + knee / 2.0).powi(2) / (2.0 * knee.max(1e-6))
    } else {
        slope * over
    }
}

/// Compressor com detector RMS ligado entre canais e suavização attack/release do ganho em dB.
pub fn compress(buf: &mut AudioBuf, p: &CompressorParams) {
    let n = buf.frames();
    let nch = buf.channels();
    if n == 0 || nch == 0 {
        return;
    }
    let fs = buf.rate as f32;
    let rms_c = one_pole(p.rms_ms, fs);
    let att = one_pole(p.attack_ms, fs);
    let rel = one_pole(p.release_ms, fs);
    let makeup = crate::db_to_lin(p.makeup_db as f64) as f32;
    let mut ms = 0.0f32;
    let mut g_db = 0.0f32;
    for i in 0..n {
        let mut e = 0.0f32;
        for c in &buf.ch {
            e += c[i] * c[i];
        }
        e /= nch as f32;
        ms = rms_c * ms + (1.0 - rms_c) * e;
        let level = 10.0 * (ms.max(1e-12)).log10();
        let target = compressor_gain_db(level, p);
        let coef = if target < g_db { att } else { rel };
        g_db = coef * g_db + (1.0 - coef) * target;
        let g = 10f32.powf(g_db / 20.0) * makeup;
        for c in &mut buf.ch {
            c[i] *= g;
        }
    }
}

// ---------------------------------------------------------------- limitador com lookahead

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct LimiterParams {
    /// Teto de pico de amostra em dBFS.
    #[serde(default = "d_ceiling")]
    pub ceiling_db: f32,
    #[serde(default = "d_look")]
    pub lookahead_ms: f32,
    #[serde(default = "d_lrel")]
    pub release_ms: f32,
}

fn d_ceiling() -> f32 {
    -1.0
}
fn d_look() -> f32 {
    5.0
}
fn d_lrel() -> f32 {
    80.0
}

impl Default for LimiterParams {
    fn default() -> Self {
        LimiterParams { ceiling_db: -1.0, lookahead_ms: 5.0, release_ms: 80.0 }
    }
}

/// Limitador de pico de amostra offline: o ganho desce *antes* do pico (rampa de média móvel sobre o mínimo deslizante)
/// e volta com `release_ms`. Garante `|y| <= teto` em todas as amostras.
pub fn limit(buf: &mut AudioBuf, p: &LimiterParams) {
    let n = buf.frames();
    if n == 0 {
        return;
    }
    let ceil = crate::db_to_lin(p.ceiling_db as f64) as f32;
    let look = ((p.lookahead_ms * 0.001 * buf.rate as f32) as usize).max(1);
    // Ganho necessário por amostra.
    let mut g = vec![1.0f32; n];
    for i in 0..n {
        let mut m = 0.0f32;
        for c in &buf.ch {
            m = m.max(c[i].abs());
        }
        if m > ceil {
            g[i] = ceil / m;
        }
    }
    if g.iter().all(|&v| v >= 1.0) {
        return;
    }
    // Mínimo deslizante sobre [i, i+look-1].
    let mut gm = vec![1.0f32; n];
    let mut dq: VecDeque<usize> = VecDeque::new();
    let mut next = 0usize; // próximo índice a entrar na janela
    for i in 0..n {
        let hi = (i + look - 1).min(n - 1);
        while next <= hi {
            while let Some(&b) = dq.back() {
                if g[b] >= g[next] {
                    dq.pop_back();
                } else {
                    break;
                }
            }
            dq.push_back(next);
            next += 1;
        }
        while let Some(&f) = dq.front() {
            if f < i {
                dq.pop_front();
            } else {
                break;
            }
        }
        gm[i] = g[*dq.front().unwrap()];
    }
    // Release: sobe devagar; nunca passa de gm.
    let rel = 1.0 - one_pole(p.release_ms, buf.rate as f32);
    let mut r = vec![1.0f32; n];
    let mut prev = 1.0f32;
    for i in 0..n {
        let up = prev + (1.0 - prev) * rel;
        prev = gm[i].min(up);
        r[i] = prev;
    }
    // Média móvel de `look` amostras (r antes do início = 1).
    let mut pre = vec![0.0f64; n + 1];
    for i in 0..n {
        pre[i + 1] = pre[i] + r[i] as f64;
    }
    for i in 0..n {
        let a = i + 1 - look.min(i + 1);
        let sum = pre[i + 1] - pre[a];
        let missing = look - (i + 1 - a);
        let gs = ((sum + missing as f64) / look as f64) as f32;
        for c in &mut buf.ch {
            let y = c[i] * gs;
            c[i] = y.clamp(-ceil, ceil);
        }
    }
}

// ---------------------------------------------------------------- reverb (Freeverb)

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ReverbParams {
    /// 0..1 — tamanho da sala.
    #[serde(default = "d_room")]
    pub room: f32,
    /// 0..1 — absorção de agudos.
    #[serde(default = "d_damp")]
    pub damp: f32,
    /// 0..1 — abertura estéreo.
    #[serde(default = "d_width")]
    pub width: f32,
}

fn d_room() -> f32 {
    0.6
}
fn d_damp() -> f32 {
    0.5
}
fn d_width() -> f32 {
    1.0
}

impl Default for ReverbParams {
    fn default() -> Self {
        ReverbParams { room: 0.6, damp: 0.5, width: 1.0 }
    }
}

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

/// Cauda do reverb somente (100% molhado), estéreo, com o mesmo comprimento da entrada.
pub fn reverb_wet(input: &AudioBuf, p: &ReverbParams) -> AudioBuf {
    let n = input.frames();
    let st = input.to_stereo();
    let scale = input.rate as f32 / 44_100.0;
    let feedback = p.room.clamp(0.0, 1.0) * 0.28 + 0.7;
    let damp1 = p.damp.clamp(0.0, 1.0) * 0.4;
    let damp2 = 1.0 - damp1;
    let make = |spread: usize| -> (Vec<Comb>, Vec<Allpass>) {
        (
            COMB_TUNING.iter().map(|&t| Comb { buf: vec![0.0; (((t + spread) as f32 * scale) as usize).max(1)], idx: 0, store: 0.0 }).collect(),
            ALLPASS_TUNING.iter().map(|&t| Allpass { buf: vec![0.0; (((t + spread) as f32 * scale) as usize).max(1)], idx: 0 }).collect(),
        )
    };
    let (mut cl, mut al) = make(0);
    let (mut cr, mut ar) = make(STEREO_SPREAD);
    let mut outl = vec![0.0f32; n];
    let mut outr = vec![0.0f32; n];
    let fixed_gain = 0.015f32;
    let run = |combs: &mut Vec<Comb>, aps: &mut Vec<Allpass>, x: f32| -> f32 {
        let mut o = 0.0f32;
        for c in combs.iter_mut() {
            let y = c.buf[c.idx];
            c.store = y * damp2 + c.store * damp1;
            c.buf[c.idx] = x + c.store * feedback;
            c.idx += 1;
            if c.idx >= c.buf.len() {
                c.idx = 0;
            }
            o += y;
        }
        for a in aps.iter_mut() {
            let b = a.buf[a.idx];
            let y = -o + b;
            a.buf[a.idx] = o + b * 0.5;
            a.idx += 1;
            if a.idx >= a.buf.len() {
                a.idx = 0;
            }
            o = y;
        }
        o
    };
    for i in 0..n {
        let x = (st.ch[0][i] + st.ch[1][i]) * fixed_gain;
        outl[i] = run(&mut cl, &mut al, x);
        outr[i] = run(&mut cr, &mut ar, x);
    }
    // Largura: mistura L/R.
    let w = p.width.clamp(0.0, 1.0);
    let w1 = 0.5 * (1.0 + w);
    let w2 = 0.5 * (1.0 - w);
    for i in 0..n {
        let (l, r) = (outl[i], outr[i]);
        outl[i] = l * w1 + r * w2;
        outr[i] = r * w1 + l * w2;
    }
    AudioBuf::stereo(input.rate, outl, outr)
}

// ---------------------------------------------------------------- ducking

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct DuckingParams {
    /// Nível (dBFS, pico suavizado) acima do qual a fala conta como ativa.
    #[serde(default = "d_dthr")]
    pub threshold_db: f32,
    /// Redução da trilha duckada com a fala ativa.
    #[serde(default = "d_depth")]
    pub depth_db: f32,
    #[serde(default = "d_datt")]
    pub attack_ms: f32,
    #[serde(default = "d_drel")]
    pub release_ms: f32,
    /// Segura a redução entre sílabas/palavras.
    #[serde(default = "d_hold")]
    pub hold_ms: f32,
    /// Antecipa a redução (o offline enxerga o futuro).
    #[serde(default = "d_dlook")]
    pub lookahead_ms: f32,
}

fn d_dthr() -> f32 {
    -45.0
}
fn d_depth() -> f32 {
    12.0
}
fn d_datt() -> f32 {
    60.0
}
fn d_drel() -> f32 {
    450.0
}
fn d_hold() -> f32 {
    180.0
}
fn d_dlook() -> f32 {
    40.0
}

impl Default for DuckingParams {
    fn default() -> Self {
        DuckingParams { threshold_db: -45.0, depth_db: 12.0, attack_ms: 60.0, release_ms: 450.0, hold_ms: 180.0, lookahead_ms: 40.0 }
    }
}

/// Curva de ganho (linear, por amostra) que reduz a trilha duckada quando `sidechain` tem fala.
pub fn ducking_gain(sidechain: &AudioBuf, p: &DuckingParams) -> Vec<f32> {
    let n = sidechain.frames();
    let fs = sidechain.rate as f32;
    // Envelope de pico com ataque rápido e release médio.
    let a = one_pole(3.0, fs);
    let r = one_pole(60.0, fs);
    let mut env = vec![0.0f32; n];
    let mut e = 0.0f32;
    for i in 0..n {
        let mut m = 0.0f32;
        for c in &sidechain.ch {
            m = m.max(c[i].abs());
        }
        let coef = if m > e { a } else { r };
        e = coef * e + (1.0 - coef) * m;
        env[i] = e;
    }
    let thr = crate::db_to_lin(p.threshold_db as f64) as f32;
    let active: Vec<f32> = env.iter().map(|&v| if v >= thr { 1.0 } else { 0.0 }).collect();
    // Máximo deslizante em [i - hold, i + look].
    let hold = (p.hold_ms * 0.001 * fs) as usize;
    let look = (p.lookahead_ms * 0.001 * fs) as usize;
    let mut held = vec![0.0f32; n];
    let mut pre = vec![0u32; n + 1];
    for i in 0..n {
        pre[i + 1] = pre[i] + (active[i] > 0.5) as u32;
    }
    for i in 0..n {
        let lo = i.saturating_sub(hold);
        let hi = (i + look + 1).min(n);
        held[i] = if pre[hi] - pre[lo] > 0 { 1.0 } else { 0.0 };
    }
    // Suaviza em dB: ataque ao reduzir, release ao recuperar.
    let att = one_pole(p.attack_ms, fs);
    let rel = one_pole(p.release_ms, fs);
    let mut g_db = 0.0f32;
    let mut out = vec![1.0f32; n];
    for i in 0..n {
        let target = -p.depth_db.abs() * held[i];
        let coef = if target < g_db { att } else { rel };
        g_db = coef * g_db + (1.0 - coef) * target;
        out[i] = 10f32.powf(g_db / 20.0);
    }
    out
}

pub fn apply_gain_curve(buf: &mut AudioBuf, curve: &[f32]) {
    for c in &mut buf.ch {
        for (s, g) in c.iter_mut().zip(curve) {
            *s *= g;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_and_peak_gains() {
        let fs = 48_000.0;
        let ls = BiquadCoeffs::low_shelf(fs, 200.0, 0.7071, 6.0);
        assert!((ls.magnitude_db(fs, 20.0) - 6.0).abs() < 0.2);
        assert!(ls.magnitude_db(fs, 10_000.0).abs() < 0.2);
        let pk = BiquadCoeffs::peaking(fs, 1000.0, 1.0, -9.0);
        assert!((pk.magnitude_db(fs, 1000.0) + 9.0).abs() < 0.05);
        let hs = BiquadCoeffs::high_shelf(fs, 4000.0, 0.7071, 4.0);
        assert!((hs.magnitude_db(fs, 18_000.0) - 4.0).abs() < 0.3);
    }

    #[test]
    fn limiter_never_exceeds_ceiling() {
        let n = 48_000;
        let x: Vec<f32> = (0..n).map(|i| 1.8 * (i as f32 * 0.05).sin() * if (i / 4000) % 2 == 0 { 1.0 } else { 0.1 }).collect();
        let mut b = AudioBuf::mono(48_000, x);
        limit(&mut b, &LimiterParams { ceiling_db: -3.0, ..Default::default() });
        let ceil = crate::db_to_lin(-3.0) as f32;
        assert!(b.peak() <= ceil + 1e-6, "pico {}", b.peak());
        assert!(b.peak() > ceil * 0.9, "deve usar o teto");
    }

    #[test]
    fn compressor_curve() {
        let p = CompressorParams { threshold_db: -20.0, ratio: 4.0, knee_db: 0.0, ..Default::default() };
        assert_eq!(compressor_gain_db(-30.0, &p), 0.0);
        // 8 dB acima do limiar com 4:1 → reduz 6 dB.
        assert!((compressor_gain_db(-12.0, &p) + 6.0).abs() < 1e-4);
    }

    #[test]
    fn reverb_has_tail_and_is_deterministic() {
        let mut x = vec![0.0f32; 48_000];
        x[0] = 1.0;
        let b = AudioBuf::mono(48_000, x);
        let w1 = reverb_wet(&b, &ReverbParams::default());
        let w2 = reverb_wet(&b, &ReverbParams::default());
        assert_eq!(w1, w2);
        let tail: f32 = w1.ch[0][20_000..40_000].iter().map(|s| s.abs()).sum();
        assert!(tail > 0.01, "cauda audível");
    }

    #[test]
    fn ducking_lowers_under_speech() {
        let n = 144_000;
        let speech: Vec<f32> = (0..n).map(|i| if (12_000..24_000).contains(&i) { 0.3 * (i as f32 * 0.1).sin() } else { 0.0 }).collect();
        let g = ducking_gain(&AudioBuf::mono(48_000, speech), &DuckingParams::default());
        assert!(g[2000] > 0.99);
        assert!(g[23_000] < 0.3, "deve estar ~ -12 dB: {}", g[23_000]);
        assert!(g[143_000] > 0.9, "recupera");
    }
}
