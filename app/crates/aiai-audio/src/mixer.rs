//! Mixer do corte: posiciona clipes (corte, fades, ganho, pan de potência constante), soma por trilha, aplica EQ,
//! compressor e reverb por trilha, ducking da BGM/ambiente pela fala e a cadeia master. Saída estéreo f32 na taxa do projeto.

use crate::buf::AudioBuf;
use crate::dsp::*;
use crate::resample::resample;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MixTrack {
    Dialogue,
    Sfx,
    Bgm,
    Ambience,
}

impl MixTrack {
    pub const ALL: [MixTrack; 4] = [MixTrack::Dialogue, MixTrack::Sfx, MixTrack::Bgm, MixTrack::Ambience];

    fn index(self) -> usize {
        match self {
            MixTrack::Dialogue => 0,
            MixTrack::Sfx => 1,
            MixTrack::Bgm => 2,
            MixTrack::Ambience => 3,
        }
    }
}

/// Clipe posicionado no tempo do corte.
#[derive(Debug, Clone)]
pub struct Clip {
    pub audio: Arc<AudioBuf>,
    pub track: MixTrack,
    /// Início no corte (s).
    pub start_s: f64,
    /// Corte do início do clipe (s).
    pub trim_start: f32,
    /// Duração usada (s); `None` = até o fim.
    pub duration: Option<f32>,
    pub gain_db: f32,
    /// -1 (esq.) .. 1 (dir.).
    pub pan: f32,
    pub fade_in: f32,
    pub fade_out: f32,
}

impl Clip {
    pub fn new(audio: Arc<AudioBuf>, track: MixTrack, start_s: f64) -> Self {
        Clip { audio, track, start_s, trim_start: 0.0, duration: None, gain_db: 0.0, pan: 0.0, fade_in: 0.0, fade_out: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ReverbSend {
    /// Nível do sinal molhado somado à trilha (dB).
    #[serde(default = "d_send")]
    pub level_db: f32,
    #[serde(default)]
    pub params: ReverbParams,
}

fn d_send() -> f32 {
    -12.0
}

impl Default for ReverbSend {
    fn default() -> Self {
        ReverbSend { level_db: -12.0, params: ReverbParams::default() }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TrackFx {
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub eq: Option<Eq3Params>,
    #[serde(default)]
    pub compressor: Option<CompressorParams>,
    #[serde(default)]
    pub reverb: Option<ReverbSend>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct MixSettings {
    #[serde(default = "d_rate")]
    pub rate: u32,
    #[serde(default)]
    pub dialogue: TrackFx,
    #[serde(default)]
    pub sfx: TrackFx,
    #[serde(default)]
    pub bgm: TrackFx,
    #[serde(default)]
    pub ambience: TrackFx,
    /// Reduz BGM e ambiente quando há fala; `None` desliga.
    #[serde(default = "d_duck")]
    pub ducking: Option<DuckingParams>,
    #[serde(default)]
    pub master_eq: Option<Eq3Params>,
    #[serde(default)]
    pub master_gain_db: f32,
    /// Limitador de pico na saída do master; `None` deixa sem limitar (a normalização final cuida do true peak).
    #[serde(default)]
    pub master_limiter: Option<LimiterParams>,
}

fn d_rate() -> u32 {
    crate::PROJECT_RATE
}

fn d_duck() -> Option<DuckingParams> {
    Some(DuckingParams::default())
}

impl Default for MixSettings {
    fn default() -> Self {
        MixSettings {
            rate: crate::PROJECT_RATE,
            dialogue: TrackFx::default(),
            sfx: TrackFx::default(),
            bgm: TrackFx::default(),
            ambience: TrackFx::default(),
            ducking: Some(DuckingParams::default()),
            master_eq: None,
            master_gain_db: 0.0,
            master_limiter: None,
        }
    }
}

impl MixSettings {
    fn fx(&self, t: MixTrack) -> &TrackFx {
        match t {
            MixTrack::Dialogue => &self.dialogue,
            MixTrack::Sfx => &self.sfx,
            MixTrack::Bgm => &self.bgm,
            MixTrack::Ambience => &self.ambience,
        }
    }
}

/// Pan de potência constante: devolve (ganho L, ganho R) para fonte mono (centro = -3 dB em cada lado).
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let th = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (th.cos(), th.sin())
}

/// Envelope de fade com cosseno elevado (suave, sem clique).
fn fade_gain(i: usize, n: usize, fade_in: usize, fade_out: usize) -> f32 {
    let mut g = 1.0f32;
    if fade_in > 0 && i < fade_in {
        g *= 0.5 - 0.5 * (std::f32::consts::PI * (i as f32 + 0.5) / fade_in as f32).cos();
    }
    if fade_out > 0 && i + fade_out > n {
        let k = n - i; // restantes, 1..=fade_out
        g *= 0.5 - 0.5 * (std::f32::consts::PI * (k as f32 - 0.5) / fade_out as f32).cos();
    }
    g
}

/// Aplica um clipe sobre a trilha estéreo `bus`.
fn place_clip(bus: &mut AudioBuf, clip: &Clip, cache: &mut HashMap<(usize, u32), Arc<AudioBuf>>) {
    let rate = bus.rate;
    let src: Arc<AudioBuf> = if clip.audio.rate == rate {
        clip.audio.clone()
    } else {
        let key = (Arc::as_ptr(&clip.audio) as usize, rate);
        cache.entry(key).or_insert_with(|| Arc::new(resample(&clip.audio, rate))).clone()
    };
    let total = src.frames();
    let trim = ((clip.trim_start.max(0.0) as f64) * rate as f64).round() as usize;
    if trim >= total {
        return;
    }
    let mut len = total - trim;
    if let Some(d) = clip.duration {
        len = len.min(((d.max(0.0) as f64) * rate as f64).round() as usize);
    }
    let start = (clip.start_s.max(0.0) * rate as f64).round() as usize;
    if start >= bus.frames() || len == 0 {
        return;
    }
    len = len.min(bus.frames() - start);
    let fi = ((clip.fade_in.max(0.0) as f64) * rate as f64).round() as usize;
    let fo = ((clip.fade_out.max(0.0) as f64) * rate as f64).round() as usize;
    let g = crate::db_to_lin(clip.gain_db as f64) as f32;
    let (pl, pr) = pan_gains(clip.pan);
    let stereo_src = src.channels() >= 2;
    let sq2 = std::f32::consts::SQRT_2;
    for i in 0..len {
        let env = g * fade_gain(i, len, fi, fo);
        let (l, r) = if stereo_src {
            (src.ch[0][trim + i] * pl * sq2, src.ch[1][trim + i] * pr * sq2)
        } else {
            let m = src.ch[0][trim + i];
            (m * pl, m * pr)
        };
        bus.ch[0][start + i] += l * env;
        bus.ch[1][start + i] += r * env;
    }
}

/// Renderiza `frames` amostras (na taxa `settings.rate`) a partir dos clipes.
pub fn render(clips: &[Clip], frames: usize, settings: &MixSettings) -> AudioBuf {
    let rate = settings.rate;
    let mut buses: Vec<AudioBuf> = (0..4).map(|_| AudioBuf::zeros(rate, 2, frames)).collect();
    let mut cache = HashMap::new();
    for c in clips {
        place_clip(&mut buses[c.track.index()], c, &mut cache);
    }
    // Efeitos por trilha.
    for t in MixTrack::ALL {
        let fx = settings.fx(t);
        let bus = &mut buses[t.index()];
        if let Some(eq) = &fx.eq {
            eq3(bus, eq);
        }
        if let Some(cp) = &fx.compressor {
            compress(bus, cp);
        }
        if let Some(rv) = &fx.reverb {
            let wet = reverb_wet(bus, &rv.params);
            let g = crate::db_to_lin(rv.level_db as f64) as f32;
            bus.mix_in(&wet, 0, g);
        }
        if fx.gain_db != 0.0 {
            bus.apply_gain_db(fx.gain_db as f64);
        }
    }
    // Ducking pela fala.
    if let Some(dp) = &settings.ducking {
        let curve = ducking_gain(&buses[MixTrack::Dialogue.index()], dp);
        apply_gain_curve(&mut buses[MixTrack::Bgm.index()], &curve);
        apply_gain_curve(&mut buses[MixTrack::Ambience.index()], &curve);
    }
    let mut master = AudioBuf::zeros(rate, 2, frames);
    for b in &buses {
        master.mix_in(b, 0, 1.0);
    }
    if let Some(eq) = &settings.master_eq {
        eq3(&mut master, eq);
    }
    if settings.master_gain_db != 0.0 {
        master.apply_gain_db(settings.master_gain_db as f64);
    }
    if let Some(l) = &settings.master_limiter {
        limit(&mut master, l);
    }
    master.sanitize();
    master
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dc(rate: u32, secs: f32, v: f32) -> Arc<AudioBuf> {
        Arc::new(AudioBuf::mono(rate, vec![v; (rate as f32 * secs) as usize]))
    }

    #[test]
    fn pan_is_constant_power() {
        for p in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            let (l, r) = pan_gains(p);
            assert!((l * l + r * r - 1.0).abs() < 1e-5);
        }
        assert_eq!(pan_gains(-1.0).1.abs() < 1e-6, true);
    }

    #[test]
    fn placement_trim_fades_and_gain() {
        let mut s = MixSettings::default();
        s.ducking = None;
        let mut c = Clip::new(dc(48_000, 1.0, 1.0), MixTrack::Sfx, 0.5);
        c.trim_start = 0.25;
        c.duration = Some(0.5);
        c.gain_db = -6.0;
        c.fade_in = 0.1;
        c.fade_out = 0.1;
        let out = render(&[c], 48_000 * 2, &s);
        let g = crate::db_to_lin(-6.0) as f32 * std::f32::consts::FRAC_1_SQRT_2;
        // Antes do clipe e depois dele: silêncio.
        assert_eq!(out.ch[0][100], 0.0);
        assert_eq!(out.ch[0][(1.0 * 48_000.0) as usize + 100], 0.0);
        // Meio: ganho pleno de -6 dB com pan central (-3 dB).
        assert!((out.ch[0][(0.75 * 48_000.0) as usize] - g).abs() < 1e-4);
        // Fade-in começa quase em zero e fade-out termina quase em zero.
        assert!(out.ch[0][(0.5 * 48_000.0) as usize].abs() < 0.01 * g.max(0.1));
        assert!(out.ch[0][(1.0 * 48_000.0) as usize - 2].abs() < 0.01);
        // Duração do clipe = 0,5 s exatos.
        let nz = out.ch[0].iter().filter(|v| v.abs() > 0.0).count();
        assert!((nz as i64 - 24_000).abs() <= 2, "{nz}");
    }

    #[test]
    fn resamples_foreign_rate_and_ducks_bgm() {
        let speech = Arc::new(AudioBuf::mono(24_000, (0..24_000).map(|i| 0.01 * (i as f32 * 0.2).sin()).collect()));
        let bgm = dc(48_000, 4.0, 0.2);
        let mut d = Clip::new(speech, MixTrack::Dialogue, 1.0);
        d.pan = 0.0;
        let b = Clip::new(bgm, MixTrack::Bgm, 0.0);
        let solo = render(&[d.clone()], 48_000 * 4, &MixSettings::default());
        // A fala de 24 kHz foi reamostrada: ocupa 1 s a 48 kHz (de 48000 a 96000), não 0,5 s.
        let nz: Vec<usize> = (0..solo.frames()).filter(|&i| solo.ch[0][i].abs() > 1e-5).collect();
        assert!(nz.first().unwrap() >= &47_900 && nz.last().unwrap() >= &95_000 && nz.last().unwrap() <= &96_100, "{:?} {:?}", nz.first(), nz.last());
        let out = render(&[d, b], 48_000 * 4, &MixSettings::default());
        // Antes da fala: BGM cheia; durante: reduzida (~12 dB).
        let before = out.ch[0][20_000].abs();
        let during: f32 = out.ch[0][70_000..90_000].iter().map(|s| s.abs()).sum::<f32>() / 20_000.0;
        assert!(before > 0.13, "{before}");
        assert!(during < before * 0.6, "{during} vs {before}");
    }
}
