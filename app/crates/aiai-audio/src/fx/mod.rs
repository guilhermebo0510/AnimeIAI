//! Efeitos estilo Audacity/Audition e o rack (cadeia ordenada). Cada efeito é uma struct com parâmetros serde e estado
//! interno (fora do serde) e implementa [`Effect`]: `process` trabalha em áudio entrelaçado (`L R L R ...`), em blocos de
//! qualquer tamanho, mantendo o estado entre chamadas (`reset()` zera). Isso serve igual ao render offline do clipe inteiro
//! e a um host em tempo real que chame o rack bloco a bloco.
//!
//! * [`basic`]: Amplificar, Normalizar, remover DC, passa-altas/passa-baixas;
//! * [`eq`]: equalizador paramétrico e gráfico (biquads);
//! * [`dynamics`]: compressor, limitador com lookahead e true peak, noise gate;
//! * [`spectral`]: redução de ruído (subtração espectral por STFT);
//! * [`reverb`]: reverb Freeverb;
//! * [`pitch`]: mudar altura e esticar o tempo (WSOLA).
//!
//! Efeitos que mudam a duração (`TimeStretch`) ou precisam do clipe inteiro (`Normalize`, `PitchShift`, `DcOffset` em modo
//! média, `Amplify` sem clipping) são marcados com `realtime() == false`; o rack os ignora em `process_block`, e o `render`
//! offline os trata normalmente.

pub mod basic;
pub mod dynamics;
pub mod eq;
pub mod pitch;
pub mod reverb;
pub mod spectral;

pub use basic::{Amplify, DcMode, DcOffset, Filter, FilterKind, Normalize, NormalizeMode};
pub use dynamics::{Compressor, Detector, Limiter, NoiseGate};
pub use eq::{BandKind, Equalizer, EqBand, GRAPHIC_10_HZ};
pub use pitch::{PitchShift, TimeStretch};
pub use reverb::Reverb;
pub use spectral::{NoiseProfile, NoiseReduction};

use crate::buf::AudioBuf;
use crate::edit::{deinterleave, interleave};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Estado interno de um efeito: opaco, fora do serde e do esquema JSON. Existe como campo público só para que os
/// parâmetros possam ser escritos com `Struct { campo: x, ..Default::default() }` de fora do crate.
#[derive(Clone, Default)]
pub struct Rt<T>(pub(crate) T);

impl<T> std::fmt::Debug for Rt<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Rt(..)")
    }
}

impl<T> std::ops::Deref for Rt<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Rt<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

/// Um processador de áudio com estado.
pub trait Effect {
    /// Processa `buf` (entrelaçado, `channels` canais) no lugar, continuando de onde a chamada anterior parou.
    fn process(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32);

    /// Esquece todo o estado (caudas, envelopes, filtros): a próxima chamada começa do zero.
    fn reset(&mut self);

    /// Atraso (em quadros) entre entrada e saída, para o host compensar.
    fn latency_frames(&self, _sample_rate: u32) -> usize {
        0
    }

    /// Quadros de cauda que o efeito produz depois do fim da entrada (reverb); só o render offline os acrescenta.
    fn tail_frames(&self, _sample_rate: u32) -> usize {
        0
    }

    /// Pode rodar bloco a bloco em tempo real?
    fn realtime(&self) -> bool {
        true
    }

    /// Render offline de um clipe: zera o estado, compensa a latência, acrescenta a cauda. Efeitos que mudam a duração
    /// sobrescrevem este método.
    fn render(&mut self, input: &AudioBuf) -> AudioBuf {
        let nch = input.channels();
        if nch == 0 || input.frames() == 0 {
            return input.clone();
        }
        self.reset();
        let rate = input.rate;
        let (lat, tail) = (self.latency_frames(rate), self.tail_frames(rate));
        let mut data = interleave(input);
        data.extend(std::iter::repeat_n(0.0, (lat + tail) * nch));
        self.process(&mut data, nch, rate);
        let out = deinterleave(&data[lat * nch..], nch, rate);
        self.reset();
        out
    }
}

/// Um efeito de qualquer tipo, serializável (`{"type": "eq", ...parâmetros}`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioFx {
    Amplify(Amplify),
    Normalize(Normalize),
    #[serde(rename = "eq")]
    Equalizer(Equalizer),
    Compressor(Compressor),
    Limiter(Limiter),
    NoiseGate(NoiseGate),
    NoiseReduction(NoiseReduction),
    Reverb(Reverb),
    PitchShift(PitchShift),
    TimeStretch(TimeStretch),
    DcOffset(DcOffset),
    Filter(Filter),
}

impl AudioFx {
    fn inner(&mut self) -> &mut dyn Effect {
        match self {
            AudioFx::Amplify(e) => e,
            AudioFx::Normalize(e) => e,
            AudioFx::Equalizer(e) => e,
            AudioFx::Compressor(e) => e,
            AudioFx::Limiter(e) => e,
            AudioFx::NoiseGate(e) => e,
            AudioFx::NoiseReduction(e) => e,
            AudioFx::Reverb(e) => e,
            AudioFx::PitchShift(e) => e,
            AudioFx::TimeStretch(e) => e,
            AudioFx::DcOffset(e) => e,
            AudioFx::Filter(e) => e,
        }
    }

    fn inner_ref(&self) -> &dyn Effect {
        match self {
            AudioFx::Amplify(e) => e,
            AudioFx::Normalize(e) => e,
            AudioFx::Equalizer(e) => e,
            AudioFx::Compressor(e) => e,
            AudioFx::Limiter(e) => e,
            AudioFx::NoiseGate(e) => e,
            AudioFx::NoiseReduction(e) => e,
            AudioFx::Reverb(e) => e,
            AudioFx::PitchShift(e) => e,
            AudioFx::TimeStretch(e) => e,
            AudioFx::DcOffset(e) => e,
            AudioFx::Filter(e) => e,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            AudioFx::Amplify(_) => "amplify",
            AudioFx::Normalize(_) => "normalize",
            AudioFx::Equalizer(_) => "eq",
            AudioFx::Compressor(_) => "compressor",
            AudioFx::Limiter(_) => "limiter",
            AudioFx::NoiseGate(_) => "noise_gate",
            AudioFx::NoiseReduction(_) => "noise_reduction",
            AudioFx::Reverb(_) => "reverb",
            AudioFx::PitchShift(_) => "pitch_shift",
            AudioFx::TimeStretch(_) => "time_stretch",
            AudioFx::DcOffset(_) => "dc_offset",
            AudioFx::Filter(_) => "filter",
        }
    }

    /// Confere os parâmetros (faixas e dependências) antes de aplicar.
    pub fn validate(&self) -> Result<(), String> {
        let finite = |name: &str, v: f32| if v.is_finite() { Ok(()) } else { Err(format!("{}: '{name}' precisa ser um número finito", self.name())) };
        match self {
            AudioFx::Amplify(e) => finite("gain_db", e.gain_db),
            AudioFx::Normalize(e) => {
                finite("target_db", e.target_db)?;
                finite("target_lufs", e.target_lufs)?;
                finite("ceiling_dbtp", e.ceiling_dbtp)
            }
            AudioFx::Equalizer(e) => {
                if e.bands.len() > 32 {
                    return Err("eq: no máximo 32 bandas".into());
                }
                for b in &e.bands {
                    if !(b.freq_hz.is_finite() && b.freq_hz > 0.0) || !b.gain_db.is_finite() || !(b.q.is_finite() && b.q > 0.0) {
                        return Err(format!("eq: banda inválida (freq_hz {} > 0, gain_db finito, q > 0)", b.freq_hz));
                    }
                }
                Ok(())
            }
            AudioFx::Compressor(e) => {
                if !(e.ratio.is_finite() && e.ratio >= 1.0) {
                    return Err("compressor: ratio precisa ser >= 1".into());
                }
                finite("threshold_db", e.threshold_db)
            }
            AudioFx::Limiter(e) => {
                if e.ceiling_db > 0.0 {
                    return Err("limiter: ceiling_db precisa ser <= 0".into());
                }
                finite("ceiling_db", e.ceiling_db)
            }
            AudioFx::NoiseGate(e) => {
                if e.range_db > 0.0 {
                    return Err("noise_gate: range_db precisa ser <= 0".into());
                }
                finite("threshold_db", e.threshold_db)
            }
            AudioFx::NoiseReduction(e) => {
                if e.profile.is_none() {
                    return Err("noise_reduction exige um perfil de ruído (profile) ou um trecho de ruído (profile_start_s/profile_end_s)".into());
                }
                if e.reduction_db < 0.0 {
                    return Err("noise_reduction: reduction_db precisa ser >= 0".into());
                }
                Ok(())
            }
            AudioFx::Reverb(e) => {
                if !(0.0..=1.0).contains(&e.room_size) || !(0.0..=1.0).contains(&e.damping) || !(0.0..=1.0).contains(&e.width) {
                    return Err("reverb: room_size, damping e width ficam entre 0 e 1".into());
                }
                if e.tail_s < 0.0 || e.tail_s > 30.0 {
                    return Err("reverb: tail_s fica entre 0 e 30".into());
                }
                Ok(())
            }
            AudioFx::PitchShift(e) => {
                if e.semitones.abs() > 36.0 {
                    return Err("pitch_shift: semitones fica entre -36 e 36".into());
                }
                finite("cents", e.cents)
            }
            AudioFx::TimeStretch(e) => {
                if !(0.1..=10.0).contains(&e.ratio) {
                    return Err("time_stretch: ratio fica entre 0.1 e 10".into());
                }
                Ok(())
            }
            AudioFx::DcOffset(_) => Ok(()),
            AudioFx::Filter(e) => {
                if !(e.freq_hz.is_finite() && e.freq_hz > 0.0) {
                    return Err("filter: freq_hz precisa ser > 0".into());
                }
                if ![12, 24, 36, 48].contains(&e.slope_db_oct) {
                    return Err("filter: slope_db_oct é 12, 24, 36 ou 48".into());
                }
                Ok(())
            }
        }
    }
}

impl Effect for AudioFx {
    fn process(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32) {
        self.inner().process(buf, channels, sample_rate)
    }

    fn reset(&mut self) {
        self.inner().reset()
    }

    fn latency_frames(&self, sample_rate: u32) -> usize {
        self.inner_ref().latency_frames(sample_rate)
    }

    fn tail_frames(&self, sample_rate: u32) -> usize {
        self.inner_ref().tail_frames(sample_rate)
    }

    fn realtime(&self) -> bool {
        self.inner_ref().realtime()
    }

    fn render(&mut self, input: &AudioBuf) -> AudioBuf {
        self.inner().render(input)
    }
}

// ---------------------------------------------------------------- rack

/// Máximo de posições do rack (como o Effects Rack do Audition).
pub const RACK_MAX_SLOTS: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RackSlot {
    pub fx: AudioFx,
    /// Posição desligada: o áudio passa sem processar.
    #[serde(default)]
    pub bypass: bool,
}

impl RackSlot {
    pub fn new(fx: AudioFx) -> Self {
        RackSlot { fx, bypass: false }
    }
}

/// Cadeia ordenada de efeitos.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Rack {
    pub slots: Vec<RackSlot>,
    /// Desliga o rack inteiro.
    pub bypass: bool,
}

impl Rack {
    pub fn new() -> Self {
        Rack::default()
    }

    pub fn push(&mut self, fx: AudioFx) -> &mut Self {
        self.slots.push(RackSlot::new(fx));
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.slots.len() > RACK_MAX_SLOTS {
            return Err(format!("rack: no máximo {RACK_MAX_SLOTS} posições (recebi {})", self.slots.len()));
        }
        for (i, s) in self.slots.iter().enumerate() {
            s.fx.validate().map_err(|e| format!("rack[{i}]: {e}"))?;
        }
        Ok(())
    }

    fn active(&self) -> impl Iterator<Item = &RackSlot> {
        self.slots.iter().filter(move |s| !s.bypass && !self.bypass)
    }

    pub fn reset(&mut self) {
        for s in &mut self.slots {
            s.fx.reset();
        }
    }

    /// Todos os efeitos ativos rodam em bloco? (se não, `process_block` pula os offline)
    pub fn realtime_ok(&self) -> bool {
        self.active().all(|s| s.fx.realtime())
    }

    /// Atraso total (quadros) dos efeitos ativos que rodam em bloco.
    pub fn latency_frames(&self, sample_rate: u32) -> usize {
        self.active().filter(|s| s.fx.realtime()).map(|s| s.fx.latency_frames(sample_rate)).sum()
    }

    /// Processa um bloco entrelaçado pela cadeia. Devolve quantas posições ativas foram puladas por serem só offline.
    pub fn process_block(&mut self, buf: &mut [f32], channels: usize, sample_rate: u32) -> usize {
        if self.bypass {
            return 0;
        }
        let mut skipped = 0;
        for s in self.slots.iter_mut().filter(|s| !s.bypass) {
            if s.fx.realtime() {
                s.fx.process(buf, channels, sample_rate);
            } else {
                skipped += 1;
            }
        }
        skipped
    }

    /// Aplica a cadeia a um clipe inteiro, na ordem, compensando latências e acrescentando caudas; efeitos que mudam a
    /// duração (esticar o tempo) são aplicados normalmente. Não altera `input`.
    pub fn render(&mut self, input: &AudioBuf) -> AudioBuf {
        let mut cur = input.clone();
        if self.bypass {
            return cur;
        }
        for s in self.slots.iter_mut().filter(|s| !s.bypass) {
            cur = s.fx.render(&cur);
        }
        cur
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use crate::fft::{Fft, WindowKind, window};
    use crate::rng::Rng;
    use std::f64::consts::PI;

    /// Seno entrelaçado (o mesmo em todos os canais).
    pub fn sine_il(fs: u32, f: f64, secs: f64, amp: f32, ch: usize) -> Vec<f32> {
        let n = (fs as f64 * secs) as usize;
        let mut out = Vec::with_capacity(n * ch);
        for i in 0..n {
            let v = amp * (2.0 * PI * f * i as f64 / fs as f64).sin() as f32;
            for _ in 0..ch {
                out.push(v);
            }
        }
        out
    }

    /// Ruído branco uniforme em [-amp, amp], canais independentes.
    pub fn noise_il(seed: u64, secs: f64, amp: f32, ch: usize) -> Vec<f32> {
        let mut rng = Rng::new(seed);
        (0..(44_100.0 * secs) as usize * ch).map(|_| rng.bipolar() * amp).collect()
    }

    pub fn rms(x: &[f32]) -> f32 {
        ((x.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()) as f32
    }

    pub fn rms_ch(x: &[f32], ch: usize, c: usize) -> f32 {
        let v: Vec<f32> = x.chunks_exact(ch).map(|f| f[c]).collect();
        rms(&v)
    }

    pub fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, v| m.max(v.abs()))
    }

    pub fn peak_ch(x: &[f32], ch: usize, c: usize) -> f32 {
        x.chunks_exact(ch).fold(0.0, |m, f| m.max(f[c].abs()))
    }

    pub fn mean(x: &[f32]) -> f32 {
        (x.iter().map(|&v| v as f64).sum::<f64>() / x.len().max(1) as f64) as f32
    }

    pub fn db(ratio: f32) -> f64 {
        20.0 * (ratio as f64).max(1e-12).log10()
    }

    /// Amplitude do componente em `f` Hz (correlação com seno e cosseno).
    pub fn tone_amp(x: &[f32], fs: u32, f: f64) -> f32 {
        let (mut s, mut c) = (0.0f64, 0.0f64);
        for (i, &v) in x.iter().enumerate() {
            let ph = 2.0 * PI * f * i as f64 / fs as f64;
            s += v as f64 * ph.sin();
            c += v as f64 * ph.cos();
        }
        (2.0 / x.len() as f64 * (s * s + c * c).sqrt()) as f32
    }

    /// RMS equivalente da banda `lo..hi` Hz (por uma FFT com janela Hann do começo do trecho).
    pub fn band_rms(x: &[f32], fs: u32, lo: f64, hi: f64) -> f32 {
        let n = (x.len().next_power_of_two() / if x.len().is_power_of_two() { 1 } else { 2 }).max(256);
        let fft = Fft::new(n);
        let w = window(WindowKind::Hann, n);
        let mut re: Vec<f32> = (0..n).map(|i| x[i] * w[i]).collect();
        let mut im = vec![0.0f32; n];
        fft.forward(&mut re, &mut im);
        let mut e = 0.0f64;
        for k in 1..n / 2 {
            let f = k as f64 * fs as f64 / n as f64;
            if f >= lo && f <= hi {
                e += (re[k] as f64).powi(2) + (im[k] as f64).powi(2);
            }
        }
        (e / (n as f64 * n as f64 / 4.0)).sqrt() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    #[test]
    fn fx_json_roundtrip_and_strict_keys() {
        let j = r#"{"slots":[{"fx":{"type":"eq","bands":[{"freq_hz":1000,"gain_db":6,"q":1.0}]}},{"fx":{"type":"compressor","threshold_db":-20}},{"fx":{"type":"limiter"},"bypass":true}]}"#;
        let rack: Rack = serde_json::from_str(j).unwrap();
        assert_eq!(rack.slots.len(), 3);
        assert!(rack.slots[2].bypass);
        rack.validate().unwrap();
        let back: Rack = serde_json::from_str(&serde_json::to_string(&rack).unwrap()).unwrap();
        assert_eq!(back.slots.len(), 3);
        // chave desconhecida em um efeito e em uma posição: erro
        assert!(serde_json::from_str::<AudioFx>(r#"{"type":"amplify","gain_dbs":3}"#).is_err());
        assert!(serde_json::from_str::<RackSlot>(r#"{"fx":{"type":"limiter"},"bypas":true}"#).is_err());
        assert!(serde_json::from_str::<AudioFx>(r#"{"type":"nao_existe"}"#).is_err());
        // o estado interno não vai para o JSON
        let mut e = AudioFx::Reverb(Reverb::default());
        let mut buf = vec![0.5f32; 200];
        e.process(&mut buf, 2, 44_100);
        assert!(!serde_json::to_string(&e).unwrap().contains("\"st\""));
        // esquema JSON gerável
        let _ = serde_json::to_value(schemars::schema_for!(Rack)).unwrap();
    }

    #[test]
    fn validation_catches_bad_params() {
        assert!(AudioFx::Compressor(Compressor { ratio: 0.5, ..Default::default() }).validate().is_err());
        assert!(AudioFx::Reverb(Reverb { room_size: 2.0, ..Default::default() }).validate().is_err());
        assert!(AudioFx::NoiseReduction(NoiseReduction::default()).validate().is_err());
        assert!(AudioFx::Filter(Filter { slope_db_oct: 18, ..Default::default() }).validate().is_err());
        assert!(AudioFx::TimeStretch(TimeStretch { ratio: 0.0, ..Default::default() }).validate().is_err());
        let mut r = Rack::new();
        for _ in 0..17 {
            r.push(AudioFx::Amplify(Amplify::default()));
        }
        assert!(r.validate().is_err());
    }

    #[test]
    fn rack_render_chain_order_bypass_and_length() {
        let fs = 44_100;
        let src = AudioBuf::mono(fs, sine_il(fs, 1000.0, 1.0, 0.1, 1));
        // +6 dB depois -6 dB = identidade; bypass de uma posição = só a outra
        let mut r = Rack::new();
        r.push(AudioFx::Amplify(Amplify { gain_db: 6.0, ..Default::default() })).push(AudioFx::Amplify(Amplify { gain_db: -6.0, ..Default::default() }));
        let y = r.render(&src);
        assert!(y.ch[0].iter().zip(&src.ch[0]).all(|(a, b)| (a - b).abs() < 1e-3));
        r.slots[1].bypass = true;
        assert!((db(rms(&r.render(&src).ch[0]) / rms(&src.ch[0])) - 6.0).abs() < 0.01);
        r.bypass = true;
        assert_eq!(r.render(&src), src);
        r.bypass = false;
        // ordem importa: limitador antes ou depois do ganho
        let mut a = Rack::new();
        a.push(AudioFx::Limiter(Limiter { ceiling_db: -6.0, ..Default::default() })).push(AudioFx::Amplify(Amplify { gain_db: 12.0, ..Default::default() }));
        let mut b = Rack::new();
        b.push(AudioFx::Amplify(Amplify { gain_db: 12.0, ..Default::default() })).push(AudioFx::Limiter(Limiter { ceiling_db: -6.0, ..Default::default() }));
        let loud = AudioBuf::mono(fs, sine_il(fs, 300.0, 1.0, 0.5, 1));
        let (pa, pb) = (a.render(&loud).peak(), b.render(&loud).peak());
        assert!(pb < 0.51 && pa > 1.9, "{pa} {pb}"); // limita depois = 0,5; ganho depois = 0,5·4
        // latência compensada e cauda: duração = original + cauda do reverb; com time stretch a duração escala
        let mut c = Rack::new();
        c.push(AudioFx::Limiter(Limiter::default())).push(AudioFx::Reverb(Reverb { tail_s: 0.5, ..Default::default() }));
        assert_eq!(c.render(&src).frames(), src.frames() + 22_050);
        let mut d = Rack::new();
        d.push(AudioFx::TimeStretch(TimeStretch { ratio: 1.5, ..Default::default() }));
        let l = d.render(&src).frames() as i64;
        assert!((l - (src.frames() as f64 * 1.5) as i64).abs() <= 2, "{l}");
    }

    #[test]
    fn rack_blocks_equal_offline_and_skip_offline_effects() {
        let fs = 44_100;
        let x = noise_il(3, 0.5, 0.3, 2);
        let build = || {
            let mut r = Rack::new();
            r.push(AudioFx::Filter(Filter::high_pass(80.0, 12)))
                .push(AudioFx::Equalizer(Equalizer::graphic10(&[0.0, 0.0, 2.0, 0.0, -3.0, 0.0, 0.0, 4.0, 0.0, 0.0])))
                .push(AudioFx::Compressor(Compressor::default()))
                .push(AudioFx::Reverb(Reverb::default()));
            r
        };
        let mut whole = x.clone();
        let mut r1 = build();
        assert!(r1.realtime_ok());
        assert_eq!(r1.process_block(&mut whole, 2, fs), 0);
        let mut parts = x.clone();
        let mut r2 = build();
        for blk in parts.chunks_mut(2 * 477) {
            r2.process_block(blk, 2, fs);
        }
        assert!(whole.iter().zip(&parts).all(|(a, b)| (a - b).abs() < 1e-6));
        // reset reproduz o mesmo
        r2.reset();
        let mut again = x.clone();
        r2.process_block(&mut again, 2, fs);
        assert_eq!(again, whole);
        // efeito offline é pulado e contado
        let mut r3 = Rack::new();
        r3.push(AudioFx::Normalize(Normalize::default())).push(AudioFx::Amplify(Amplify::default()));
        assert!(!r3.realtime_ok());
        let mut z = x.clone();
        assert_eq!(r3.process_block(&mut z, 2, fs), 1);
        assert_eq!(z, x);
    }

    #[test]
    fn effect_defaults_roundtrip_through_render() {
        // todo efeito com parâmetros padrão processa sem NaN e mantém o formato (menos os que mudam a duração)
        let fs = 44_100;
        let src = AudioBuf { rate: fs, ch: vec![noise_il(1, 0.4, 0.2, 1), noise_il(2, 0.4, 0.2, 1)] };
        let fxs = vec![
            AudioFx::Amplify(Amplify::default()),
            AudioFx::Normalize(Normalize::default()),
            AudioFx::Equalizer(Equalizer::graphic10(&[1.0; 10])),
            AudioFx::Compressor(Compressor::default()),
            AudioFx::Limiter(Limiter::default()),
            AudioFx::NoiseGate(NoiseGate::default()),
            AudioFx::Reverb(Reverb::default()),
            AudioFx::PitchShift(PitchShift { semitones: 2.0, ..Default::default() }),
            AudioFx::DcOffset(DcOffset::default()),
            AudioFx::Filter(Filter::default()),
        ];
        for mut fx in fxs {
            let y = fx.render(&src);
            assert_eq!((y.channels(), y.frames(), y.rate), (2, src.frames(), fs), "{}", fx.name());
            assert!(y.ch.iter().flatten().all(|v| v.is_finite()), "{}", fx.name());
        }
    }
}
