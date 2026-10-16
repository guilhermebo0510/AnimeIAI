//! BGM: sequenciador de notas (tempo, compasso, trilhas com programa General MIDI, bateria no canal 10) renderizado com
//! `rustysynth` + SoundFont livre (FluidR3_GM, MIT). Sem SoundFont, usa um sintetizador próprio simples (osciladores +
//! ADSR + filtro). O render pode fechar em loop: a cauda (release/reverb) é dobrada sobre o início.

use crate::buf::{AudioBuf, AudioError, AudioResult};
use crate::rng::Rng;
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Note {
    /// Número MIDI (C4 = 60).
    pub pitch: u8,
    /// Início em batidas desde o começo da música.
    pub start_beat: f64,
    pub length_beats: f64,
    #[serde(default = "d_vel")]
    pub velocity: u8,
}

fn d_vel() -> u8 {
    90
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BgmTrack {
    #[serde(default)]
    pub name: String,
    /// Programa General MIDI 0..127 (0 = piano de cauda, 48 = cordas, 73 = flauta...). Ignorado em bateria.
    #[serde(default)]
    pub program: u8,
    /// Bateria (canal 10 GM): `pitch` é a peça (36 bumbo, 38 caixa, 42 chimbal fechado...).
    #[serde(default)]
    pub drums: bool,
    /// Volume do canal 0..127.
    #[serde(default = "d_vol")]
    pub volume: u8,
    /// 0 (esq.) .. 64 (centro) .. 127 (dir.).
    #[serde(default = "d_pan")]
    pub pan: u8,
    /// Envio de reverb do SoundFont 0..127.
    #[serde(default = "d_rev")]
    pub reverb: u8,
    #[serde(default)]
    pub notes: Vec<Note>,
}

fn d_vol() -> u8 {
    100
}
fn d_pan() -> u8 {
    64
}
fn d_rev() -> u8 {
    40
}

impl BgmTrack {
    pub fn new(name: &str, program: u8) -> Self {
        BgmTrack { name: name.into(), program, drums: false, volume: 100, pan: 64, reverb: 40, notes: Vec::new() }
    }

    /// Acrescenta notas escritas em texto a partir de `start_beat` (ver [`parse_notes`]); devolve a batida final.
    pub fn add_text(&mut self, text: &str, start_beat: f64) -> AudioResult<f64> {
        let (notes, end) = parse_notes(text, start_beat)?;
        self.notes.extend(notes);
        Ok(end)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Song {
    #[serde(default = "d_tempo")]
    pub tempo_bpm: f32,
    #[serde(default = "d_bpb")]
    pub beats_per_bar: u32,
    /// Compasso(s) da música; 0 = calculado pela última nota.
    #[serde(default)]
    pub bars: u32,
    pub tracks: Vec<BgmTrack>,
    /// Cauda extra renderizada além do fim (s).
    #[serde(default = "d_tail")]
    pub tail_s: f32,
    /// Dobra a cauda sobre o início: o clipe tem exatamente `bars` compassos e emenda sem corte.
    #[serde(default)]
    pub looped: bool,
    #[serde(default = "d_rate")]
    pub rate: u32,
    #[serde(default)]
    pub gain_db: f32,
}

fn d_tempo() -> f32 {
    100.0
}
fn d_bpb() -> u32 {
    4
}
fn d_tail() -> f32 {
    2.0
}
fn d_rate() -> u32 {
    crate::PROJECT_RATE
}

impl Song {
    pub fn new(tempo_bpm: f32) -> Self {
        Song { tempo_bpm, beats_per_bar: 4, bars: 0, tracks: Vec::new(), tail_s: 2.0, looped: false, rate: crate::PROJECT_RATE, gain_db: 0.0 }
    }

    pub fn beat_s(&self) -> f64 {
        60.0 / self.tempo_bpm.max(1.0) as f64
    }

    /// Duração musical (sem cauda) em batidas.
    pub fn length_beats(&self) -> f64 {
        if self.bars > 0 {
            self.bars as f64 * self.beats_per_bar.max(1) as f64
        } else {
            let last = self.tracks.iter().flat_map(|t| t.notes.iter()).map(|n| n.start_beat + n.length_beats).fold(0.0f64, f64::max);
            let bpb = self.beats_per_bar.max(1) as f64;
            (last / bpb).ceil().max(1.0) * bpb
        }
    }
}

/// Nome de nota → MIDI ("C4" = 60, "F#3", "Bb5").
pub fn note_name_to_midi(name: &str) -> AudioResult<u8> {
    let b = name.as_bytes();
    if b.is_empty() {
        return Err(AudioError::Invalid("nota vazia".into()));
    }
    let base = match b[0].to_ascii_uppercase() {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return Err(AudioError::Invalid(format!("nota inválida: {name}"))),
    };
    let mut i = 1;
    let mut semis = base as i32;
    match b.get(i) {
        Some(b'#') => {
            semis += 1;
            i += 1;
        }
        Some(b'b') => {
            semis -= 1;
            i += 1;
        }
        _ => {}
    }
    let oct: i32 = name[i..].parse().map_err(|_| AudioError::Invalid(format!("oitava inválida em {name}")))?;
    let m = (oct + 1) * 12 + semis;
    if !(0..=127).contains(&m) {
        return Err(AudioError::Invalid(format!("nota fora do MIDI: {name}")));
    }
    Ok(m as u8)
}

/// Notação de texto: fichas separadas por espaço `<alturas>:<batidas>[@velocidade]`. Alturas: `C4`, `F#3`, `Bb5`, acordes com `+`
/// (`C4+E4+G4:2`), número MIDI com `m` (`m38` para a caixa) e pausa `R`. Ex.: `"C4:1 E4:1 G4:2 R:1 C5:0.5@100"`.
/// Devolve as notas e a batida final.
pub fn parse_notes(text: &str, start_beat: f64) -> AudioResult<(Vec<Note>, f64)> {
    let mut beat = start_beat;
    let mut out = Vec::new();
    for tok in text.split_whitespace() {
        let (head, vel) = match tok.split_once('@') {
            Some((h, v)) => (h, v.parse::<u8>().map_err(|_| AudioError::Invalid(format!("velocidade inválida: {tok}")))?.min(127)),
            None => (tok, 90),
        };
        let (pitches, len) = head.split_once(':').ok_or_else(|| AudioError::Invalid(format!("ficha sem ':' — {tok}")))?;
        let len: f64 = len.parse().map_err(|_| AudioError::Invalid(format!("duração inválida: {tok}")))?;
        if len <= 0.0 {
            return Err(AudioError::Invalid(format!("duração deve ser > 0: {tok}")));
        }
        if !pitches.eq_ignore_ascii_case("r") {
            for p in pitches.split('+') {
                let pitch = if let Some(n) = p.strip_prefix('m') { n.parse::<u8>().map_err(|_| AudioError::Invalid(format!("MIDI inválido: {p}")))? } else { note_name_to_midi(p)? };
                out.push(Note { pitch, start_beat: beat, length_beats: len, velocity: vel });
            }
        }
        beat += len;
    }
    Ok((out, beat))
}

/// Carrega um SoundFont (.sf2).
pub fn load_soundfont(path: &Path) -> AudioResult<Arc<SoundFont>> {
    let mut f = std::fs::File::open(path).map_err(|e| AudioError::SoundFont(format!("{}: {e}", path.display())))?;
    let sf = SoundFont::new(&mut f).map_err(|e| AudioError::SoundFont(format!("{}: {e:?}", path.display())))?;
    Ok(Arc::new(sf))
}

static SF_CACHE: std::sync::Mutex<Option<(PathBuf, Arc<SoundFont>)>> = std::sync::Mutex::new(None);

/// Como [`load_soundfont`], mas guarda o último carregado (o FluidR3 tem ~140 MB: carregar a cada trilha custa segundos).
pub fn load_soundfont_cached(path: &Path) -> AudioResult<Arc<SoundFont>> {
    let mut g = SF_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, sf)) = g.as_ref() {
        if p == path {
            return Ok(sf.clone());
        }
    }
    let sf = load_soundfont(path)?;
    *g = Some((path.to_path_buf(), sf.clone()));
    Ok(sf)
}

/// Procura o SoundFont padrão: `AIAI_SOUNDFONT`, ou `sidecars/soundfonts/FluidR3_GM.sf2` subindo a partir do diretório atual
/// e do executável.
pub fn default_soundfont_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("AIAI_SOUNDFONT") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::current_dir() {
        starts.push(d);
    }
    if let Ok(e) = std::env::current_exe() {
        if let Some(d) = e.parent() {
            starts.push(d.to_path_buf());
        }
    }
    for s in starts {
        for dir in s.ancestors() {
            let cand = dir.join("sidecars").join("soundfonts").join("FluidR3_GM.sf2");
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BgmEngine {
    SoundFont,
    Builtin,
}

pub struct RenderedBgm {
    pub audio: AudioBuf,
    pub engine: BgmEngine,
    /// Notas descartadas por começarem depois do fim da música.
    pub dropped_notes: usize,
}

/// Renderiza com SoundFont se fornecido, senão com o sintetizador interno.
pub fn render_song(song: &Song, sf: Option<&Arc<SoundFont>>) -> AudioResult<RenderedBgm> {
    if song.tracks.iter().filter(|t| !t.drums).count() > 15 {
        return Err(AudioError::Invalid("no máximo 15 trilhas melódicas (+ bateria)".into()));
    }
    let rate = song.rate;
    let beat_s = song.beat_s();
    let len_beats = song.length_beats();
    let music_frames = (len_beats * beat_s * rate as f64).round() as usize;
    let total = music_frames + (song.tail_s.max(0.0) as f64 * rate as f64) as usize;
    let mut dropped = 0usize;
    let (mut audio, engine) = match sf {
        Some(sf) => (render_soundfont(song, sf, total, beat_s, len_beats, &mut dropped)?, BgmEngine::SoundFont),
        None => (render_builtin(song, total, beat_s, len_beats, &mut dropped), BgmEngine::Builtin),
    };
    if song.looped {
        // Dobra a cauda sobre o início.
        for c in &mut audio.ch {
            for i in music_frames..total {
                let j = i - music_frames;
                if j < music_frames {
                    c[j] += c[i];
                }
            }
            c.truncate(music_frames);
        }
    }
    if song.gain_db != 0.0 {
        audio.apply_gain_db(song.gain_db as f64);
    }
    audio.sanitize();
    Ok(RenderedBgm { audio, engine, dropped_notes: dropped })
}

struct Event {
    frame: usize,
    /// off antes de on no mesmo quadro.
    order: u8,
    channel: i32,
    key: i32,
    vel: i32,
    on: bool,
}

fn channel_of(song: &Song) -> Vec<i32> {
    let mut next = 0;
    song.tracks
        .iter()
        .map(|t| {
            if t.drums {
                9
            } else {
                if next == 9 {
                    next += 1;
                }
                let c = next;
                next += 1;
                c
            }
        })
        .collect()
}

fn collect_events(song: &Song, total: usize, beat_s: f64, len_beats: f64, dropped: &mut usize) -> Vec<Event> {
    let rate = song.rate as f64;
    let chans = channel_of(song);
    let mut ev = Vec::new();
    for (t, &ch) in song.tracks.iter().zip(&chans) {
        for n in &t.notes {
            if n.start_beat >= len_beats || n.start_beat < 0.0 || n.pitch > 127 {
                *dropped += 1;
                continue;
            }
            let on = (n.start_beat * beat_s * rate).round() as usize;
            let off = (((n.start_beat + n.length_beats.max(0.01)) * beat_s * rate).round() as usize).min(total.saturating_sub(1)).max(on + 1);
            ev.push(Event { frame: on, order: 1, channel: ch, key: n.pitch as i32, vel: n.velocity.clamp(1, 127) as i32, on: true });
            ev.push(Event { frame: off, order: 0, channel: ch, key: n.pitch as i32, vel: 0, on: false });
        }
    }
    ev.sort_by_key(|e| (e.frame, e.order, e.channel, e.key));
    ev
}

fn render_soundfont(song: &Song, sf: &Arc<SoundFont>, total: usize, beat_s: f64, len_beats: f64, dropped: &mut usize) -> AudioResult<AudioBuf> {
    let settings = SynthesizerSettings::new(song.rate as i32);
    let mut synth = Synthesizer::new(sf, &settings).map_err(|e| AudioError::SoundFont(format!("{e:?}")))?;
    let chans = channel_of(song);
    for (t, &ch) in song.tracks.iter().zip(&chans) {
        if !t.drums {
            synth.process_midi_message(ch, 0xC0, t.program.min(127) as i32, 0);
        }
        synth.process_midi_message(ch, 0xB0, 0x07, t.volume.min(127) as i32);
        synth.process_midi_message(ch, 0xB0, 0x0A, t.pan.min(127) as i32);
        synth.process_midi_message(ch, 0xB0, 0x5B, t.reverb.min(127) as i32);
    }
    let events = collect_events(song, total, beat_s, len_beats, dropped);
    let mut left = vec![0.0f32; total];
    let mut right = vec![0.0f32; total];
    let mut pos = 0usize;
    let mut i = 0usize;
    while pos < total {
        while i < events.len() && events[i].frame <= pos {
            let e = &events[i];
            if e.on {
                synth.note_on(e.channel, e.key, e.vel);
            } else {
                synth.note_off(e.channel, e.key);
            }
            i += 1;
        }
        let next = events.get(i).map_or(total, |e| e.frame).min(total);
        if next > pos {
            synth.render(&mut left[pos..next], &mut right[pos..next]);
            pos = next;
        }
    }
    Ok(AudioBuf::stereo(song.rate, left, right))
}

// ---------------------------------------------------------------- sintetizador interno de reserva

#[derive(Clone, Copy)]
enum Timbre {
    Pluck,
    Pad,
    Bass,
    Lead,
    Soft,
}

fn timbre_of(program: u8) -> Timbre {
    match program {
        0..=15 | 24..=31 => Timbre::Pluck,
        32..=39 => Timbre::Bass,
        40..=55 | 88..=95 => Timbre::Pad,
        80..=87 => Timbre::Lead,
        _ => Timbre::Soft,
    }
}

fn midi_hz(k: i32) -> f32 {
    440.0 * 2f32.powf((k as f32 - 69.0) / 12.0)
}

fn render_builtin(song: &Song, total: usize, beat_s: f64, len_beats: f64, dropped: &mut usize) -> AudioBuf {
    let fs = song.rate as f32;
    let mut l = vec![0.0f32; total];
    let mut r = vec![0.0f32; total];
    let mut rng = Rng::new(0xB6A_u64);
    for t in &song.tracks {
        let gain = (t.volume as f32 / 127.0) * 0.35;
        let th = (t.pan as f32 / 127.0) * std::f32::consts::FRAC_PI_2;
        let (gl, gr) = (th.cos() * gain, th.sin() * gain);
        let tim = timbre_of(t.program);
        for n in &t.notes {
            if n.start_beat >= len_beats || n.start_beat < 0.0 {
                *dropped += 1;
                continue;
            }
            let start = (n.start_beat * beat_s * song.rate as f64).round() as usize;
            let dur_s = (n.length_beats.max(0.01) * beat_s) as f32;
            let vel = n.velocity as f32 / 127.0;
            let (att, dec, sus, rel) = if t.drums {
                (0.001, 0.12, 0.0, 0.05)
            } else {
                match tim {
                    Timbre::Pluck => (0.004, 0.35, 0.05, 0.25),
                    Timbre::Pad => (0.12, 0.3, 0.7, 0.5),
                    Timbre::Bass => (0.006, 0.15, 0.6, 0.12),
                    Timbre::Lead => (0.01, 0.1, 0.7, 0.12),
                    Timbre::Soft => (0.02, 0.2, 0.5, 0.25),
                }
            };
            let total_len = ((dur_s + rel) * fs) as usize;
            let f = midi_hz(n.pitch as i32);
            let mut ph = rng.f32();
            let mut lp = 0.0f32;
            let mut ph2 = rng.f32();
            for i in 0..total_len {
                let idx = start + i;
                if idx >= total {
                    break;
                }
                let tt = i as f32 / fs;
                let env = if tt < att {
                    tt / att
                } else if tt < att + dec {
                    1.0 - (1.0 - sus) * (tt - att) / dec
                } else {
                    sus
                } * if tt > dur_s { (1.0 - (tt - dur_s) / rel).max(0.0) } else { 1.0 };
                let v = if t.drums {
                    drum_sample(n.pitch, tt, &mut rng, &mut ph, &mut lp, fs)
                } else {
                    ph = (ph + f / fs).fract();
                    ph2 = (ph2 + f * 1.004 / fs).fract();
                    let raw = match tim {
                        Timbre::Pluck => (2.0 * ph - 1.0) * 0.5 + (2.0 * std::f32::consts::PI * ph).sin() * 0.5,
                        Timbre::Pad => ((2.0 * ph - 1.0) + (2.0 * ph2 - 1.0)) * 0.4,
                        Timbre::Bass => (if ph < 0.5 { 1.0 } else { -1.0 }) * 0.6 + (2.0 * std::f32::consts::PI * ph).sin() * 0.4,
                        Timbre::Lead => if ph < 0.35 { 0.8 } else { -0.8 },
                        Timbre::Soft => ((2.0 * std::f32::consts::PI * ph).sin() + 0.3 * (4.0 * std::f32::consts::PI * ph).sin()) * 0.7,
                    };
                    let cutoff = match tim {
                        Timbre::Pluck => 800.0 + 4000.0 * (-tt / 0.2).exp(),
                        Timbre::Bass => 600.0,
                        _ => 3500.0,
                    };
                    let c = 1.0 - (-2.0 * std::f32::consts::PI * cutoff / fs).exp();
                    lp += c * (raw - lp);
                    lp
                };
                let s = v * env * vel;
                l[idx] += s * gl;
                r[idx] += s * gr;
            }
        }
    }
    AudioBuf::stereo(song.rate, l, r)
}

fn drum_sample(key: u8, t: f32, rng: &mut Rng, ph: &mut f32, lp: &mut f32, fs: f32) -> f32 {
    let two_pi = 2.0 * std::f32::consts::PI;
    match key {
        35 | 36 => {
            *ph += two_pi * (45.0 + 110.0 * (-t / 0.03).exp()) / fs;
            ph.sin() * (-t / 0.12).exp() * 1.6
        }
        37..=40 => {
            *ph += two_pi * 190.0 / fs;
            ph.sin() * (-t / 0.05).exp() * 0.5 + rng.bipolar() * (-t / 0.09).exp() * 0.8
        }
        42 | 44 | 46 | 49 | 51 | 53 | 57 => {
            let nz = rng.bipolar();
            *lp += 0.6 * (nz - *lp);
            (nz - *lp) * (-t / if key == 46 { 0.18 } else { 0.035 }).exp() * 0.6
        }
        _ => {
            *ph += two_pi * (90.0 + 60.0 * (key as f32 - 41.0).max(0.0) * 0.2) / fs;
            ph.sin() * (-t / 0.15).exp()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_text_parsing() {
        assert_eq!(note_name_to_midi("C4").unwrap(), 60);
        assert_eq!(note_name_to_midi("A4").unwrap(), 69);
        assert_eq!(note_name_to_midi("F#3").unwrap(), 54);
        assert_eq!(note_name_to_midi("Bb5").unwrap(), 82);
        let (n, end) = parse_notes("C4:1 E4+G4:2 R:1 m38:0.5@100", 4.0).unwrap();
        assert_eq!(n.len(), 4);
        assert_eq!((n[0].start_beat, n[1].start_beat, n[2].start_beat, n[3].start_beat), (4.0, 5.0, 5.0, 8.0));
        assert_eq!((n[3].pitch, n[3].velocity, end), (38, 100, 8.5));
        assert!(parse_notes("C4", 0.0).is_err());
        assert!(parse_notes("H4:1", 0.0).is_err());
    }

    fn demo_song() -> Song {
        let mut s = Song::new(120.0);
        s.bars = 2;
        let mut lead = BgmTrack::new("lead", 73);
        lead.add_text("C5:1 E5:1 G5:1 E5:1 D5:2 R:2", 0.0).unwrap();
        let mut bass = BgmTrack::new("bass", 33);
        bass.add_text("C2:2 G2:2 C2:2 G2:2", 0.0).unwrap();
        let mut drums = BgmTrack::new("drums", 0);
        drums.drums = true;
        drums.add_text("m36:1 m42:1 m38:1 m42:1 m36:1 m42:1 m38:1 m42:1", 0.0).unwrap();
        s.tracks = vec![lead, bass, drums];
        s.looped = true;
        s
    }

    #[test]
    fn builtin_renders_loop_with_exact_length() {
        let r = render_song(&demo_song(), None).unwrap();
        assert_eq!(r.engine, BgmEngine::Builtin);
        // 2 compassos de 4 batidas a 120 bpm = 4 s exatos.
        assert_eq!(r.audio.frames(), 4 * 48_000);
        assert!(r.audio.rms() > 0.01 && r.audio.peak() < 4.0);
        // Determinístico.
        assert_eq!(r.audio, render_song(&demo_song(), None).unwrap().audio);
    }

    #[test]
    fn soundfont_render_if_available() {
        let Some(p) = default_soundfont_path() else {
            eprintln!("SoundFont ausente: teste do rustysynth ignorado");
            return;
        };
        let sf = load_soundfont(&p).unwrap();
        let r = render_song(&demo_song(), Some(&sf)).unwrap();
        assert_eq!(r.engine, BgmEngine::SoundFont);
        assert_eq!(r.audio.frames(), 4 * 48_000);
        assert!(r.audio.rms() > 0.005, "silêncio: {}", r.audio.rms());
        // Emenda do loop sem salto: último e primeiro bloco com níveis de mesma ordem (sem descontinuidade gigante).
        let (a, b) = (r.audio.ch[0][r.audio.frames() - 1], r.audio.ch[0][0]);
        assert!((a - b).abs() < 0.2, "salto na emenda: {a} → {b}");
    }
}
