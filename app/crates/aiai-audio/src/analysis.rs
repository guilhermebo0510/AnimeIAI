//! Análise para a UI: pirâmide de forma de onda (mín/máx/RMS por balde em vários níveis de zoom, com cache) e
//! espectrograma (STFT em dB). Devolvem dados puros; quem desenha é a UI.

use crate::buf::{AudioBuf, AudioError, AudioResult};
use crate::fft::{Fft, WindowKind, window};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------- pirâmide de forma de onda

/// Menor tamanho de balde (quadros por balde no nível 0); cada nível acima dobra.
pub const WAVE_BASE_BLOCK: usize = 64;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct MinMax {
    pub min: f32,
    pub max: f32,
    pub rms: f32,
}

#[derive(Debug, Clone)]
struct Level {
    block: usize,
    min: Vec<Vec<f32>>,   // [canal][balde]
    max: Vec<Vec<f32>>,
    sumsq: Vec<Vec<f32>>,
}

/// Pirâmide de resumos de uma forma de onda: o nível `k` guarda mín/máx/soma de quadrados de baldes de
/// `64·2^k` quadros. Consulta em qualquer zoom custa O(pixels), sem reler as amostras (exceto abaixo de 64 quadros por pixel).
#[derive(Debug, Clone)]
pub struct WaveformPyramid {
    pub sample_rate: u32,
    pub frames: usize,
    pub channels: usize,
    levels: Vec<Level>,
}

impl WaveformPyramid {
    pub fn build(buf: &AudioBuf) -> WaveformPyramid {
        let (frames, channels) = (buf.frames(), buf.channels());
        let nb = frames.div_ceil(WAVE_BASE_BLOCK);
        let mut l0 = Level { block: WAVE_BASE_BLOCK, min: vec![Vec::with_capacity(nb); channels], max: vec![Vec::with_capacity(nb); channels], sumsq: vec![Vec::with_capacity(nb); channels] };
        for (c, x) in buf.ch.iter().enumerate() {
            for chunk in x.chunks(WAVE_BASE_BLOCK) {
                let (mut lo, mut hi, mut ss) = (f32::MAX, f32::MIN, 0.0f32);
                for &s in chunk {
                    lo = lo.min(s);
                    hi = hi.max(s);
                    ss += s * s;
                }
                l0.min[c].push(lo);
                l0.max[c].push(hi);
                l0.sumsq[c].push(ss);
            }
        }
        let mut levels = vec![l0];
        while levels.last().is_some_and(|l| l.min.first().is_some_and(|v| v.len() > 1)) {
            let prev = levels.last().unwrap();
            let mut next = Level { block: prev.block * 2, min: Vec::new(), max: Vec::new(), sumsq: Vec::new() };
            for c in 0..channels {
                let (mut mn, mut mx, mut sq) = (Vec::new(), Vec::new(), Vec::new());
                for i in (0..prev.min[c].len()).step_by(2) {
                    let j = (i + 1).min(prev.min[c].len() - 1);
                    mn.push(prev.min[c][i].min(prev.min[c][j]));
                    mx.push(prev.max[c][i].max(prev.max[c][j]));
                    sq.push(prev.sumsq[c][i] + if j != i { prev.sumsq[c][j] } else { 0.0 });
                }
                next.min.push(mn);
                next.max.push(mx);
                next.sumsq.push(sq);
            }
            levels.push(next);
        }
        WaveformPyramid { sample_rate: buf.rate, frames, channels, levels }
    }

    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    /// Quadros por balde de cada nível.
    pub fn block_sizes(&self) -> Vec<usize> {
        self.levels.iter().map(|l| l.block).collect()
    }

    /// Resumo de `pixels` colunas cobrindo os quadros `[start, end)` do canal `channel` (`None` = pior caso entre canais:
    /// menor mín e maior máx). `src` serve só para zoom abaixo de um balde (amostras cruas).
    pub fn query(&self, src: &AudioBuf, channel: Option<usize>, start: usize, end: usize, pixels: usize) -> Vec<MinMax> {
        let end = end.min(self.frames);
        let start = start.min(end);
        let pixels = pixels.max(1);
        let span = (end - start) as f64;
        let fpp = span / pixels as f64;
        let chans: Vec<usize> = match channel {
            Some(c) if c < self.channels => vec![c],
            Some(_) => return vec![MinMax { min: 0.0, max: 0.0, rms: 0.0 }; pixels],
            None => (0..self.channels).collect(),
        };
        // maior nível cujo balde cabe em um pixel; abaixo do balde base, lê amostras
        let li = self.levels.iter().rposition(|l| (l.block as f64) <= fpp);
        (0..pixels)
            .map(|p| {
                let a = start + (p as f64 * fpp) as usize;
                let b = (start + ((p + 1) as f64 * fpp).ceil() as usize).clamp(a + 1, end.max(a + 1));
                let (mut lo, mut hi, mut ss, mut cnt) = (f32::MAX, f32::MIN, 0.0f64, 0usize);
                for &c in &chans {
                    match li {
                        Some(li) => {
                            let l = &self.levels[li];
                            let (ia, ib) = (a / l.block, b.div_ceil(l.block).min(l.min[c].len()));
                            for i in ia..ib.max(ia + 1).min(l.min[c].len()) {
                                lo = lo.min(l.min[c][i]);
                                hi = hi.max(l.max[c][i]);
                                ss += l.sumsq[c][i] as f64;
                                cnt += (l.block).min(self.frames - i * l.block);
                            }
                        }
                        None => {
                            if let Some(x) = src.ch.get(c) {
                                for &s in &x[a.min(x.len())..b.min(x.len())] {
                                    lo = lo.min(s);
                                    hi = hi.max(s);
                                    ss += (s * s) as f64;
                                    cnt += 1;
                                }
                            }
                        }
                    }
                }
                if cnt == 0 || lo > hi {
                    MinMax { min: 0.0, max: 0.0, rms: 0.0 }
                } else {
                    MinMax { min: lo, max: hi, rms: (ss / cnt as f64).sqrt() as f32 }
                }
            })
            .collect()
    }
}

/// Cache de pirâmides por chave (hash do blob, por exemplo). Descarta a mais antiga quando passa de `cap`.
pub struct WaveformCache {
    cap: usize,
    inner: Mutex<(HashMap<String, Arc<WaveformPyramid>>, VecDeque<String>)>,
}

impl WaveformCache {
    pub fn new(cap: usize) -> Self {
        WaveformCache { cap: cap.max(1), inner: Mutex::new((HashMap::new(), VecDeque::new())) }
    }

    pub fn get_or_build(&self, key: &str, buf: impl FnOnce() -> AudioResult<AudioBuf>) -> AudioResult<Arc<WaveformPyramid>> {
        if let Some(p) = self.inner.lock().map_err(|_| AudioError::Invalid("cache de forma de onda envenenado".into()))?.0.get(key) {
            return Ok(p.clone());
        }
        let p = Arc::new(WaveformPyramid::build(&buf()?));
        let mut g = self.inner.lock().map_err(|_| AudioError::Invalid("cache de forma de onda envenenado".into()))?;
        if !g.0.contains_key(key) {
            g.1.push_back(key.to_string());
            while g.1.len() > self.cap {
                if let Some(old) = g.1.pop_front() {
                    g.0.remove(&old);
                }
            }
        }
        g.0.insert(key.to_string(), p.clone());
        Ok(p)
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map_or(0, |g| g.0.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------- espectrograma

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SpectrogramParams {
    /// Tamanho da FFT (potência de 2, 64 a 16384): mais bins = mais resolução em frequência, menos em tempo.
    pub fft_size: usize,
    /// Passo entre colunas em quadros (> 0).
    pub hop: usize,
    pub window: WindowKind,
    /// Piso do gráfico em dB (valores abaixo viram este).
    pub min_db: f32,
    pub max_db: f32,
    /// Canal analisado; `null` = média dos canais.
    pub channel: Option<usize>,
}

impl Default for SpectrogramParams {
    fn default() -> Self {
        SpectrogramParams { fft_size: 1024, hop: 256, window: WindowKind::Hann, min_db: -100.0, max_db: 0.0, channel: None }
    }
}

/// Magnitudes em dB (0 dB = seno de escala cheia), uma linha de `bins` valores por quadro, colunas centradas em
/// `frame_index · hop` (a coluna `j` fica sobre a amostra `start + j·hop`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Spectrogram {
    pub sample_rate: u32,
    pub fft_size: usize,
    pub hop: usize,
    pub bins: usize,
    pub frames: usize,
    pub min_db: f32,
    pub max_db: f32,
    /// Primeiro quadro da fonte coberto.
    pub start_frame: usize,
    pub data: Vec<f32>,
}

/// Limite de células (quadros × bins) para não estourar memória por engano.
pub const SPECTROGRAM_MAX_CELLS: usize = 48_000_000;

impl Spectrogram {
    pub fn compute(buf: &AudioBuf, range: std::ops::Range<usize>, p: &SpectrogramParams) -> AudioResult<Spectrogram> {
        if !p.fft_size.is_power_of_two() || !(64..=16_384).contains(&p.fft_size) {
            return Err(AudioError::Invalid(format!("fft_size precisa ser potência de 2 entre 64 e 16384 (recebi {})", p.fft_size)));
        }
        if p.hop == 0 {
            return Err(AudioError::Invalid("hop precisa ser > 0".into()));
        }
        if p.max_db <= p.min_db {
            return Err(AudioError::Invalid("max_db precisa ser maior que min_db".into()));
        }
        if let Some(c) = p.channel
            && c >= buf.channels()
        {
            return Err(AudioError::Invalid(format!("canal {c} não existe (o áudio tem {})", buf.channels())));
        }
        let n = p.fft_size;
        let bins = n / 2 + 1;
        let r = crate::edit::clamp_range(buf.frames(), range);
        let len = r.len();
        let frames = len.div_ceil(p.hop).max(1);
        if frames * bins > SPECTROGRAM_MAX_CELLS {
            return Err(AudioError::Invalid(format!("espectrograma grande demais ({frames} quadros × {bins} bins); aumente o hop ou reduza o intervalo")));
        }
        // fonte: canal escolhido ou média
        let src: Vec<f32> = match p.channel {
            Some(c) => buf.ch[c][r.clone()].to_vec(),
            None => {
                let k = buf.channels().max(1) as f32;
                (r.clone()).map(|i| buf.ch.iter().map(|c| c[i]).sum::<f32>() / k).collect()
            }
        };
        let fft = Fft::new(n);
        let win = window(p.window, n);
        let norm = 2.0 / win.iter().sum::<f32>().max(1e-9); // seno de escala cheia = 0 dB
        let mut data = Vec::with_capacity(frames * bins);
        let (mut re, mut im) = (vec![0.0f32; n], vec![0.0f32; n]);
        for j in 0..frames {
            let center = j * p.hop;
            for k in 0..n {
                let idx = center as isize + k as isize - (n / 2) as isize;
                re[k] = if idx >= 0 && (idx as usize) < len { src[idx as usize] * win[k] } else { 0.0 };
                im[k] = 0.0;
            }
            fft.forward(&mut re, &mut im);
            for k in 0..bins {
                let mag = (re[k] * re[k] + im[k] * im[k]).sqrt() * norm;
                let d = 20.0 * mag.max(1e-12).log10();
                data.push(d.clamp(p.min_db, p.max_db));
            }
        }
        Ok(Spectrogram { sample_rate: buf.rate, fft_size: n, hop: p.hop, bins, frames, min_db: p.min_db, max_db: p.max_db, start_frame: r.start, data })
    }

    pub fn at(&self, frame: usize, bin: usize) -> f32 {
        self.data[frame * self.bins + bin]
    }

    pub fn bin_freq(&self, bin: usize) -> f64 {
        bin as f64 * self.sample_rate as f64 / self.fft_size as f64
    }

    /// Reduz a uma grade `width × height` (linhas de cima para baixo: agudos primeiro). Cada célula é o máximo da região
    /// (picos não somem ao reduzir). `log_freq` espaça as linhas em escala logarítmica entre `f_lo` e `f_hi` (Hz).
    pub fn to_grid(&self, width: usize, height: usize, f_lo: f64, f_hi: f64, log_freq: bool) -> Vec<f32> {
        let (w, h) = (width.max(1), height.max(1));
        let nyq = self.sample_rate as f64 / 2.0;
        let f_hi = f_hi.clamp(1.0, nyq);
        let f_lo = f_lo.clamp(if log_freq { 20.0 } else { 0.0 }, f_hi - 1e-6);
        let bin_of = |f: f64| (f / nyq * (self.bins - 1) as f64).clamp(0.0, (self.bins - 1) as f64);
        let mut out = vec![self.min_db; w * h];
        for row in 0..h {
            let t0 = 1.0 - (row + 1) as f64 / h as f64;
            let t1 = 1.0 - row as f64 / h as f64;
            let (fa, fb) = if log_freq { (f_lo * (f_hi / f_lo).powf(t0), f_lo * (f_hi / f_lo).powf(t1)) } else { (f_lo + (f_hi - f_lo) * t0, f_lo + (f_hi - f_lo) * t1) };
            let (ba, bb) = (bin_of(fa).floor() as usize, bin_of(fb).ceil() as usize);
            for col in 0..w {
                let (ja, jb) = (col * self.frames / w, ((col + 1) * self.frames).div_ceil(w).max(col * self.frames / w + 1).min(self.frames));
                let mut m = self.min_db;
                for j in ja..jb {
                    for b in ba..=bb.min(self.bins - 1) {
                        m = m.max(self.at(j, b));
                    }
                }
                out[row * w + col] = m;
            }
        }
        out
    }

    /// Grade normalizada para 0..=255 (0 = `min_db`, 255 = `max_db`), pronta para virar textura.
    pub fn to_u8(&self, width: usize, height: usize, f_lo: f64, f_hi: f64, log_freq: bool) -> Vec<u8> {
        let span = (self.max_db - self.min_db).max(1e-6);
        self.to_grid(width, height, f_lo, f_hi, log_freq).iter().map(|&d| (((d - self.min_db) / span).clamp(0.0, 1.0) * 255.0).round() as u8).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn sine(rate: u32, f: f64, secs: f64, amp: f32) -> AudioBuf {
        let n = (rate as f64 * secs) as usize;
        AudioBuf::mono(rate, (0..n).map(|i| amp * (2.0 * PI * f * i as f64 / rate as f64).sin() as f32).collect())
    }

    #[test]
    fn pyramid_levels_and_envelope() {
        // 1 s a 48 kHz: pico de 0,25 no começo e de 1,0 no fim
        let mut b = sine(48_000, 440.0, 1.0, 0.25);
        b.ch[0][47_000] = 1.0;
        b.ch[0][100] = -0.9;
        let p = WaveformPyramid::build(&b);
        let sizes = p.block_sizes();
        assert_eq!(sizes[0], 64);
        assert!(sizes.windows(2).all(|w| w[1] == w[0] * 2));
        assert!(*sizes.last().unwrap() >= b.frames() / 2, "o último nível tem 1 balde");
        // visão geral em 100 colunas: o pico de 1.0 aparece na coluna certa, o -0.9 na primeira
        let q = p.query(&b, Some(0), 0, b.frames(), 100);
        assert_eq!(q.len(), 100);
        let hi_col = q.iter().position(|m| m.max >= 0.999).unwrap();
        assert_eq!(hi_col, 47_000 * 100 / 48_000);
        assert!(q[0].min <= -0.899);
        // fora dos picos: ~0,25 de amplitude, RMS de um seno ≈ 0,177
        assert!((q[50].max - 0.25).abs() < 0.01 && (q[50].min + 0.25).abs() < 0.01);
        assert!((q[50].rms - 0.25 / 2f32.sqrt()).abs() < 0.01, "{}", q[50].rms);
        // consulta de um trecho
        let z = p.query(&b, Some(0), 40_000, 48_000, 8);
        assert!(z.iter().any(|m| m.max >= 0.999));
        // zoom abaixo do balde base lê as amostras cruas
        let raw = p.query(&b, Some(0), 100, 108, 8);
        assert!((raw[0].min + 0.9).abs() < 1e-6 || raw.iter().any(|m| m.min <= -0.89));
        for (i, m) in raw.iter().enumerate() {
            assert_eq!(m.max, b.ch[0][100 + i].max(m.min));
        }
    }

    #[test]
    fn pyramid_multichannel_and_edges() {
        let a = sine(8000, 100.0, 0.5, 0.5);
        let b = AudioBuf { rate: 8000, ch: vec![a.ch[0].clone(), a.ch[0].iter().map(|s| s * 2.0).collect()] };
        let p = WaveformPyramid::build(&b);
        let both = p.query(&b, None, 0, b.frames(), 10);
        let c1 = p.query(&b, Some(1), 0, b.frames(), 10);
        assert!(both.iter().zip(&c1).all(|(a, b)| a.min == b.min && a.max == b.max));
        assert!(p.query(&b, Some(5), 0, 100, 4).iter().all(|m| m.max == 0.0));
        // vazio e intervalo fora
        let e = WaveformPyramid::build(&AudioBuf::zeros(8000, 1, 0));
        assert_eq!(e.query(&AudioBuf::zeros(8000, 1, 0), None, 0, 10, 4).len(), 4);
        assert_eq!(p.query(&b, None, 999_999, 2_000_000, 3).len(), 3);
        // tamanho não múltiplo de 64: o último balde parcial entra
        let odd = AudioBuf::mono(8000, vec![0.5; 1000]);
        let po = WaveformPyramid::build(&odd);
        let qo = po.query(&odd, None, 0, 1000, 3);
        assert!(qo.iter().all(|m| (m.rms - 0.5).abs() < 1e-3 && m.max == 0.5), "{qo:?}");
    }

    #[test]
    fn cache_reuses_and_evicts() {
        let cache = WaveformCache::new(2);
        let mut built = 0;
        for k in ["a", "b", "a", "c", "a"] {
            cache
                .get_or_build(k, || {
                    built += 1;
                    Ok(sine(8000, 100.0, 0.1, 0.5))
                })
                .unwrap();
        }
        // a, b construídos; a reaproveitado; c entra e expulsa a (a mais antiga); a de novo reconstrói
        assert_eq!(built, 4);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn spectrogram_peak_at_tone_frequency_and_level() {
        let fs = 48_000;
        let b = sine(fs, 3000.0, 1.0, 0.5);
        let s = Spectrogram::compute(&b, 0..b.frames(), &SpectrogramParams { fft_size: 2048, hop: 512, ..Default::default() }).unwrap();
        assert_eq!(s.bins, 1025);
        assert_eq!(s.frames, 94);
        let col = 40;
        let (mut best, mut bb) = (f32::MIN, 0);
        for k in 0..s.bins {
            if s.at(col, k) > best {
                best = s.at(col, k);
                bb = k;
            }
        }
        assert!((s.bin_freq(bb) - 3000.0).abs() < 25.0, "{}", s.bin_freq(bb));
        // amplitude 0,5 = -6,02 dB (pico entre bins perde até ~1,4 dB com Hann)
        assert!(best < -5.0 && best > -8.0, "{best}");
        // longe do tom: piso muito baixo
        assert!(s.at(col, 700) < -70.0, "{}", s.at(col, 700));
        // grade reduzida: o tom aparece na linha certa (escala linear até 24 kHz, 64 linhas)
        let g = s.to_grid(32, 64, 0.0, 24_000.0, false);
        let row = (0..64).max_by(|&a, &b| g[a * 32 + 16].partial_cmp(&g[b * 32 + 16]).unwrap()).unwrap();
        let expect = ((1.0 - 3000.0 / 24_000.0) * 64.0) as usize;
        assert!((row as i64 - expect as i64).abs() <= 1, "{row} vs {expect}");
        let u = s.to_u8(32, 64, 0.0, 24_000.0, false);
        assert_eq!(u.len(), 32 * 64);
        assert!(u[row * 32 + 16] > 200 && u[10] < 60);
        // escala logarítmica: 3 kHz fica mais acima de 20 Hz..20 kHz do que na linear
        let gl = s.to_grid(32, 64, 20.0, 20_000.0, true);
        let rl = (0..64).max_by(|&a, &b| gl[a * 32 + 16].partial_cmp(&gl[b * 32 + 16]).unwrap()).unwrap();
        let exp_l = ((1.0 - (3000f64 / 20.0).ln() / (1000f64).ln()) * 64.0) as usize;
        assert!((rl as i64 - exp_l as i64).abs() <= 1, "{rl} vs {exp_l}");
    }

    #[test]
    fn spectrogram_params_and_range() {
        let b = sine(8000, 440.0, 1.0, 0.5);
        assert!(Spectrogram::compute(&b, 0..100, &SpectrogramParams { fft_size: 1000, ..Default::default() }).is_err());
        assert!(Spectrogram::compute(&b, 0..100, &SpectrogramParams { hop: 0, ..Default::default() }).is_err());
        assert!(Spectrogram::compute(&b, 0..100, &SpectrogramParams { channel: Some(3), ..Default::default() }).is_err());
        let s = Spectrogram::compute(&b, 2000..4000, &SpectrogramParams { fft_size: 256, hop: 100, window: WindowKind::Blackman, ..Default::default() }).unwrap();
        assert_eq!((s.start_frame, s.frames), (2000, 20));
        // intervalo vazio: um quadro de silêncio no piso
        let e = Spectrogram::compute(&b, 5..5, &SpectrogramParams::default()).unwrap();
        assert_eq!(e.frames, 1);
        assert!(e.data.iter().all(|&d| d == e.min_db));
    }
}
