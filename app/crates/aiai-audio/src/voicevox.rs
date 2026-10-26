//! Cliente do VOICEVOX Engine (HTTP local, porta 50021 por padrão): `/speakers`, `/audio_query` (moras com tempos e altura),
//! edição (velocidade, altura, entonação, volume, silêncios, tom e duração por mora), `/synthesis` → WAV e os tempos finais
//! de cada fonema. Modelo de tempo calibrado contra o engine 0.25.2: cada fonema vale `round(len / speedScale · 93,75) / 93,75` s
//! (quadros de 256 amostras a 24 kHz), e os silêncios inicial/final e a pausa seguem a mesma conta (`pauseLength` substitui a pausa).

use crate::buf::{AudioError, AudioResult};
use crate::http::{request, url_encode};
use crate::timing::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::time::Duration;

pub const DEFAULT_URL: &str = "http://127.0.0.1:50021";
/// Taxa de quadros interna do modelo acústico do VOICEVOX (24000 / 256).
pub const FRAME_RATE: f64 = 93.75;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Style {
    pub name: String,
    pub id: u32,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Speaker {
    pub name: String,
    pub speaker_uuid: String,
    pub styles: Vec<Style>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Mora {
    pub text: String,
    #[serde(default)]
    pub consonant: Option<String>,
    #[serde(default)]
    pub consonant_length: Option<f64>,
    pub vowel: String,
    pub vowel_length: f64,
    pub pitch: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccentPhrase {
    pub moras: Vec<Mora>,
    pub accent: u32,
    #[serde(default)]
    pub pause_mora: Option<Mora>,
    #[serde(default)]
    pub is_interrogative: bool,
}

/// Consulta de áudio do engine. Campos desconhecidos de versões futuras passam intactos em `extra`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioQuery {
    pub accent_phrases: Vec<AccentPhrase>,
    #[serde(rename = "speedScale")]
    pub speed_scale: f64,
    #[serde(rename = "pitchScale")]
    pub pitch_scale: f64,
    #[serde(rename = "intonationScale")]
    pub intonation_scale: f64,
    #[serde(rename = "volumeScale")]
    pub volume_scale: f64,
    #[serde(rename = "prePhonemeLength")]
    pub pre_phoneme_length: f64,
    #[serde(rename = "postPhonemeLength")]
    pub post_phoneme_length: f64,
    #[serde(default, rename = "pauseLength")]
    pub pause_length: Option<f64>,
    #[serde(default = "one", rename = "pauseLengthScale")]
    pub pause_length_scale: f64,
    #[serde(default = "rate24k", rename = "outputSamplingRate")]
    pub output_sampling_rate: u32,
    #[serde(default, rename = "outputStereo")]
    pub output_stereo: bool,
    #[serde(default)]
    pub kana: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn one() -> f64 {
    1.0
}
fn rate24k() -> u32 {
    24_000
}

/// Edição de uma mora (altura em log F0; comprimentos em segundos, antes da divisão por `speed_scale`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct MoraEdit {
    /// Índice do sintagma (frase acentual) em `audio_query.accent_phrases`.
    pub phrase: usize,
    /// Índice da mora dentro do sintagma.
    pub mora: usize,
    /// Altura absoluta (log F0, ~5,0–6,5 na fala).
    #[serde(default)]
    pub pitch: Option<f64>,
    /// Soma à altura atual.
    #[serde(default)]
    pub pitch_delta: Option<f64>,
    #[serde(default)]
    pub vowel_length: Option<f64>,
    #[serde(default)]
    pub consonant_length: Option<f64>,
}

/// Ajustes de voz (todos opcionais; omitido = valor do engine).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct VoiceParams {
    #[serde(default)]
    pub speed_scale: Option<f64>,
    #[serde(default)]
    pub pitch_scale: Option<f64>,
    #[serde(default)]
    pub intonation_scale: Option<f64>,
    #[serde(default)]
    pub volume_scale: Option<f64>,
    #[serde(default)]
    pub pre_phoneme_length: Option<f64>,
    #[serde(default)]
    pub post_phoneme_length: Option<f64>,
    /// Duração fixa (s) das pausas de pontuação; omitido = a do engine × `pause_length_scale`.
    #[serde(default)]
    pub pause_length: Option<f64>,
    #[serde(default)]
    pub pause_length_scale: Option<f64>,
    #[serde(default)]
    pub mora_edits: Vec<MoraEdit>,
}

/// Aplica os ajustes (e as edições de mora) a uma consulta.
pub fn apply_params(q: &mut AudioQuery, p: &VoiceParams) -> AudioResult<()> {
    if let Some(v) = p.speed_scale {
        q.speed_scale = v;
    }
    if let Some(v) = p.pitch_scale {
        q.pitch_scale = v;
    }
    if let Some(v) = p.intonation_scale {
        q.intonation_scale = v;
    }
    if let Some(v) = p.volume_scale {
        q.volume_scale = v;
    }
    if let Some(v) = p.pre_phoneme_length {
        q.pre_phoneme_length = v;
    }
    if let Some(v) = p.post_phoneme_length {
        q.post_phoneme_length = v;
    }
    if let Some(v) = p.pause_length {
        q.pause_length = Some(v);
    }
    if let Some(v) = p.pause_length_scale {
        q.pause_length_scale = v;
    }
    if q.speed_scale <= 0.0 || !q.speed_scale.is_finite() {
        return Err(AudioError::Invalid("speed_scale deve ser > 0".into()));
    }
    for e in &p.mora_edits {
        let ph = q.accent_phrases.get_mut(e.phrase).ok_or_else(|| AudioError::Invalid(format!("sintagma {} inexistente", e.phrase)))?;
        let m = ph.moras.get_mut(e.mora).ok_or_else(|| AudioError::Invalid(format!("mora {} inexistente no sintagma {}", e.mora, e.phrase)))?;
        if let Some(v) = e.pitch {
            m.pitch = v;
        }
        if let Some(v) = e.pitch_delta {
            m.pitch += v;
        }
        if let Some(v) = e.vowel_length {
            m.vowel_length = v.max(0.0);
        }
        if let Some(v) = e.consonant_length {
            if m.consonant.is_some() {
                m.consonant_length = Some(v.max(0.0));
            }
        }
    }
    Ok(())
}

fn q_frames(seconds: f64) -> f64 {
    (seconds.max(0.0) * FRAME_RATE + 0.5).floor() / FRAME_RATE
}

/// Tempos de cada fonema/mora que o engine produzirá para esta consulta (mesma quantização do `/synthesis`).
pub fn timings_of(q: &AudioQuery) -> Timings {
    let sp = q.speed_scale.max(1e-6);
    let mut t = 0.0f64;
    let mut moras = Vec::new();
    let mut phonemes = Vec::new();
    let pre = q_frames(q.pre_phoneme_length / sp);
    if pre > 0.0 {
        phonemes.push(PhonemeSeg { phoneme: "pau".into(), kind: SegKind::Silence, start: 0.0, end: pre });
    }
    t += pre;
    let speech_start = t;
    for (pi, ph) in q.accent_phrases.iter().enumerate() {
        for m in &ph.moras {
            let start = t;
            let mut vowel_start = t;
            if let (Some(c), Some(cl)) = (&m.consonant, m.consonant_length) {
                let d = q_frames(cl / sp);
                phonemes.push(PhonemeSeg { phoneme: c.clone(), kind: SegKind::Consonant, start: t, end: t + d });
                t += d;
                vowel_start = t;
            }
            let d = q_frames(m.vowel_length / sp);
            phonemes.push(PhonemeSeg { phoneme: m.vowel.clone(), kind: SegKind::Vowel, start: t, end: t + d });
            t += d;
            let (class, devoiced) = VowelClass::from_phoneme(&m.vowel).unwrap_or((VowelClass::Pau, false));
            moras.push(MoraTiming { text: m.text.clone(), consonant: m.consonant.clone(), vowel: m.vowel.clone(), class, devoiced, start, vowel_start, end: t, phrase: pi, pitch: m.pitch });
        }
        if let Some(p) = &ph.pause_mora {
            let raw = match q.pause_length {
                Some(v) => v,
                None => p.vowel_length * q.pause_length_scale,
            };
            let d = q_frames(raw / sp);
            phonemes.push(PhonemeSeg { phoneme: "pau".into(), kind: SegKind::Pause, start: t, end: t + d });
            moras.push(MoraTiming { text: p.text.clone(), consonant: None, vowel: "pau".into(), class: VowelClass::Pau, devoiced: false, start: t, vowel_start: t, end: t + d, phrase: pi, pitch: 0.0 });
            t += d;
        }
    }
    let speech_end = t;
    let post = q_frames(q.post_phoneme_length / sp);
    if post > 0.0 {
        phonemes.push(PhonemeSeg { phoneme: "pau".into(), kind: SegKind::Silence, start: t, end: t + post });
    }
    t += post;
    Timings { moras, phonemes, duration: t, speech_start, speech_end }
}

pub struct VoicevoxClient {
    pub base: String,
    pub timeout: Duration,
}

/// Resultado de uma fala sintetizada.
pub struct VoiceLine {
    /// WAV PCM16 mono 24 kHz como o engine devolveu.
    pub wav: Vec<u8>,
    pub query: AudioQuery,
    pub timings: Timings,
}

impl Default for VoicevoxClient {
    fn default() -> Self {
        VoicevoxClient { base: DEFAULT_URL.into(), timeout: Duration::from_secs(120) }
    }
}

impl VoicevoxClient {
    pub fn new(base: &str) -> Self {
        VoicevoxClient { base: base.trim_end_matches('/').into(), timeout: Duration::from_secs(120) }
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> AudioResult<Vec<u8>> {
        let (ct, bytes) = match body {
            Some(v) => (Some("application/json"), serde_json::to_vec(v).map_err(|e| AudioError::Voicevox(e.to_string()))?),
            None => (None, Vec::new()),
        };
        let r = request(&self.base, method, path, ct, &bytes, self.timeout)?;
        if r.status / 100 != 2 {
            return Err(AudioError::Voicevox(format!("{method} {path} → HTTP {}: {}", r.status, String::from_utf8_lossy(&r.body).chars().take(300).collect::<String>())));
        }
        Ok(r.body)
    }

    /// O mesmo cliente com outro limite de espera (a verificação de saúde não espera 2 minutos).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn available(&self) -> bool {
        self.version().is_ok()
    }

    pub fn version(&self) -> AudioResult<String> {
        let b = self.call("GET", "/version", None)?;
        serde_json::from_slice::<String>(&b).map_err(|e| AudioError::Voicevox(e.to_string()))
    }

    pub fn speakers(&self) -> AudioResult<Vec<Speaker>> {
        let b = self.call("GET", "/speakers", None)?;
        serde_json::from_slice(&b).map_err(|e| AudioError::Voicevox(format!("speakers: {e}")))
    }

    /// Texto dos termos de uso do personagem (`policy` de `/speaker_info`) — o crédito exigido sai daqui.
    pub fn speaker_policy(&self, speaker_uuid: &str) -> AudioResult<String> {
        let b = self.call("GET", &format!("/speaker_info?speaker_uuid={}&resource_format=url", url_encode(speaker_uuid)), None)?;
        let v: Value = serde_json::from_slice(&b).map_err(|e| AudioError::Voicevox(e.to_string()))?;
        Ok(v.get("policy").and_then(|p| p.as_str()).unwrap_or_default().to_string())
    }

    pub fn audio_query(&self, text: &str, speaker: u32) -> AudioResult<AudioQuery> {
        let b = self.call("POST", &format!("/audio_query?speaker={speaker}&text={}", url_encode(text)), None)?;
        serde_json::from_slice(&b).map_err(|e| AudioError::Voicevox(format!("audio_query: {e}")))
    }

    /// Sintagmas a partir de leitura em kana com acento (AquesTalk-like: `オハヨオ'、キョ'オモ`).
    pub fn accent_phrases(&self, text: &str, speaker: u32, is_kana: bool) -> AudioResult<Vec<AccentPhrase>> {
        let b = self.call("POST", &format!("/accent_phrases?speaker={speaker}&is_kana={is_kana}&text={}", url_encode(text)), None)?;
        serde_json::from_slice(&b).map_err(|e| AudioError::Voicevox(format!("accent_phrases: {e}")))
    }

    /// Consulta cuja leitura vem de kana explícito (resolve leituras erradas de kanji).
    pub fn audio_query_from_kana(&self, kana: &str, speaker: u32) -> AudioResult<AudioQuery> {
        let mut q = self.audio_query("あ", speaker)?;
        q.accent_phrases = self.accent_phrases(kana, speaker, true)?;
        q.kana = Some(kana.to_string());
        Ok(q)
    }

    /// Recalcula altura e duração das moras depois de editar o acento.
    pub fn mora_data(&self, phrases: &[AccentPhrase], speaker: u32) -> AudioResult<Vec<AccentPhrase>> {
        let v = serde_json::to_value(phrases).map_err(|e| AudioError::Voicevox(e.to_string()))?;
        let b = self.call("POST", &format!("/mora_data?speaker={speaker}"), Some(&v))?;
        serde_json::from_slice(&b).map_err(|e| AudioError::Voicevox(format!("mora_data: {e}")))
    }

    pub fn synthesis(&self, q: &AudioQuery, speaker: u32) -> AudioResult<Vec<u8>> {
        let v = serde_json::to_value(q).map_err(|e| AudioError::Voicevox(e.to_string()))?;
        self.call("POST", &format!("/synthesis?speaker={speaker}"), Some(&v))
    }

    /// Texto (ou kana) → consulta → ajustes/edições → síntese → WAV + tempos.
    pub fn synthesize_line(&self, text: &str, kana: Option<&str>, speaker: u32, params: &VoiceParams) -> AudioResult<VoiceLine> {
        let mut q = match kana {
            Some(k) => self.audio_query_from_kana(k, speaker)?,
            None => self.audio_query(text, speaker)?,
        };
        apply_params(&mut q, params)?;
        let wav = self.synthesis(&q, speaker)?;
        let timings = timings_of(&q);
        Ok(VoiceLine { wav, query: q, timings })
    }

    /// Ressintetiza uma consulta já editada (regerável a partir dos parâmetros guardados no clipe).
    pub fn synthesize_query(&self, q: &AudioQuery, speaker: u32) -> AudioResult<VoiceLine> {
        let wav = self.synthesis(q, speaker)?;
        Ok(VoiceLine { wav, query: q.clone(), timings: timings_of(q) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mora(text: &str, c: Option<(&str, f64)>, v: &str, vl: f64) -> Mora {
        Mora { text: text.into(), consonant: c.map(|x| x.0.to_string()), consonant_length: c.map(|x| x.1), vowel: v.into(), vowel_length: vl, pitch: 5.5 }
    }

    fn query() -> AudioQuery {
        AudioQuery {
            accent_phrases: vec![
                AccentPhrase { moras: vec![mora("オ", None, "o", 0.075), mora("ハ", Some(("h", 0.082)), "a", 0.08)], accent: 1, pause_mora: Some(mora("、", None, "pau", 0.426)), is_interrogative: false },
                AccentPhrase { moras: vec![mora("ン", None, "N", 0.09)], accent: 1, pause_mora: None, is_interrogative: false },
            ],
            speed_scale: 1.0,
            pitch_scale: 0.0,
            intonation_scale: 1.0,
            volume_scale: 1.0,
            pre_phoneme_length: 0.1,
            post_phoneme_length: 0.1,
            pause_length: None,
            pause_length_scale: 1.0,
            output_sampling_rate: 24_000,
            output_stereo: false,
            kana: None,
            extra: Map::new(),
        }
    }

    #[test]
    fn timing_model_quantizes_to_engine_frames() {
        let t = timings_of(&query());
        // pre/post: round(0.1*93.75)=9 quadros = 0,096 s; consoante 0,082 → 8 quadros.
        assert!((t.speech_start - 9.0 / 93.75).abs() < 1e-9);
        assert_eq!(t.moras.len(), 4);
        assert_eq!(t.moras[1].consonant.as_deref(), Some("h"));
        assert!((t.moras[1].vowel_start - t.moras[1].start - 8.0 / 93.75).abs() < 1e-9);
        assert_eq!(t.moras[2].class, VowelClass::Pau);
        // Contíguo: o fim de uma mora é o início da seguinte.
        for w in t.moras.windows(2) {
            assert!((w[0].end - w[1].start).abs() < 1e-9);
        }
        let last = t.phonemes.last().unwrap();
        assert!((last.end - t.duration).abs() < 1e-9);
        // Velocidade 2x: tudo (inclusive pre/post) cai à metade, com arredondamento por fonema.
        let mut q = query();
        q.speed_scale = 2.0;
        let t2 = timings_of(&q);
        assert!(t2.duration < t.duration * 0.55 && t2.duration > t.duration * 0.45, "{} {}", t2.duration, t.duration);
        // pauseLength substitui a pausa.
        let mut q = query();
        q.pause_length = Some(0.2);
        let p = timings_of(&q).phonemes.into_iter().find(|p| p.kind == SegKind::Pause).unwrap();
        assert!((p.end - p.start - 19.0 / 93.75).abs() < 1e-9);
    }

    #[test]
    fn edits_apply_and_validate() {
        let mut q = query();
        let p = VoiceParams { speed_scale: Some(1.2), pitch_scale: Some(0.05), mora_edits: vec![MoraEdit { phrase: 0, mora: 1, pitch_delta: Some(0.3), vowel_length: Some(0.2), consonant_length: Some(0.05), ..Default::default() }], ..Default::default() };
        apply_params(&mut q, &p).unwrap();
        assert_eq!(q.speed_scale, 1.2);
        assert!((q.accent_phrases[0].moras[1].pitch - 5.8).abs() < 1e-9);
        assert_eq!(q.accent_phrases[0].moras[1].vowel_length, 0.2);
        assert!(apply_params(&mut q, &VoiceParams { mora_edits: vec![MoraEdit { phrase: 9, mora: 0, ..Default::default() }], ..Default::default() }).is_err());
        assert!(apply_params(&mut q, &VoiceParams { speed_scale: Some(0.0), ..Default::default() }).is_err());
    }

    #[test]
    fn query_json_roundtrip_keeps_unknown_fields() {
        let j = r#"{"accent_phrases":[],"speedScale":1.0,"pitchScale":0.0,"intonationScale":1.0,"volumeScale":1.0,"prePhonemeLength":0.1,"postPhonemeLength":0.1,"pauseLength":null,"pauseLengthScale":1.0,"outputSamplingRate":24000,"outputStereo":false,"kana":"x","futureField":42}"#;
        let q: AudioQuery = serde_json::from_str(j).unwrap();
        let back: Value = serde_json::to_value(&q).unwrap();
        assert_eq!(back["futureField"], 42);
        assert_eq!(back["speedScale"], 1.0);
    }
}
