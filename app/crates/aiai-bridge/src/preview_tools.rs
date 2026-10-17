//! Prévia e entrega (S18): `RENDER_SHEET` (folha de contato de um corte), `EDIT_PREVIEW` (quadros da edição com legenda,
//! transição e créditos, sem exportar o vídeo), `EDIT_MIX` (mixagem da edição inteira em WAV, com loudness) e
//! `TTS_REGENERATE` (refaz o áudio de uma fala do VOICEVOX no mesmo asset — o clipe continua no lugar — com ajustes
//! opcionais na consulta: velocidade, tom, entonação, volume, pausas).

use crate::core_tools::{apply, request, tool_error};
use crate::production_tools::{check_abs, ensure_parent, loudness_json, voicevox_url};
use crate::protocol::BridgeError;
use aiai_audio::AudioBuf;
use aiai_audio::loudness::LoudnessPreset;
use aiai_audio::voicevox::{AudioQuery, VoicevoxClient};
use aiai_core::{Command, CoreSession, Id, NodeKind};
use aiai_tools::audio::{self, WavBits};
use aiai_tools::edit::{load_edit, mix_edit};
use aiai_tools::preview;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const PREVIEW_ACTIONS: &[&str] = &["RENDER_SHEET", "EDIT_PREVIEW", "EDIT_MIX", "TTS_REGENERATE"];

pub fn is_mutating(action: &str) -> bool {
    action == "TTS_REGENERATE"
}

/// Quadros pedidos: lista explícita ou faixa `start..=end` a cada `step`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct FrameSelection {
    /// Quadros 0-based explícitos (no máximo 48).
    #[serde(default)]
    pub frames: Vec<u32>,
    #[serde(default)]
    pub start: Option<u32>,
    /// Inclusivo; padrão = último quadro.
    #[serde(default)]
    pub end: Option<u32>,
    /// Padrão: o necessário para caber em 12 quadros.
    #[serde(default)]
    pub step: Option<u32>,
}

impl FrameSelection {
    fn resolve(&self, total: u32) -> Result<Vec<u32>, BridgeError> {
        if !self.frames.is_empty() {
            if self.frames.len() > preview::MAX_SHEET_FRAMES {
                return Err(BridgeError::invalid_params(format!("no máximo {} quadros", preview::MAX_SHEET_FRAMES)));
            }
            return Ok(self.frames.clone());
        }
        let last = total.saturating_sub(1);
        let (s, e) = (self.start.unwrap_or(0), self.end.unwrap_or(last).min(last));
        let step = self.step.unwrap_or_else(|| ((e.saturating_sub(s)) / 11).max(1));
        preview::frame_range(s, e, step).map_err(tool_error)
    }
}

/// `RENDER_SHEET`: vários quadros de um corte numa imagem (número do quadro e tempo embaixo de cada um).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenderSheetRequest {
    pub cut: Id,
    #[serde(flatten)]
    pub select: FrameSelection,
    /// Colunas (padrão 4).
    #[serde(default)]
    pub columns: Option<u32>,
    /// Largura de cada miniatura em px (padrão 480).
    #[serde(default)]
    pub thumb_width: Option<u32>,
    /// `false` = canvas inteiro do corte, sem câmera (padrão true).
    #[serde(default)]
    pub camera: Option<bool>,
    /// PNG de saída.
    pub out: String,
}

/// `EDIT_PREVIEW`: quadros da edição (globais) como o MP4 vai mostrar: legendas queimadas, transições, créditos.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EditPreviewRequest {
    pub edit: Id,
    #[serde(flatten)]
    pub select: FrameSelection,
    /// Instantes em segundos (alternativa a `frames`).
    #[serde(default)]
    pub seconds: Vec<f64>,
    #[serde(default)]
    pub columns: Option<u32>,
    #[serde(default)]
    pub thumb_width: Option<u32>,
    /// Padrão true.
    #[serde(default)]
    pub burn_subtitles: Option<bool>,
    pub out: String,
}

/// `EDIT_MIX`: mixagem da edição inteira (falas, SFX, BGM de todos os cortes na ordem) num WAV 48 kHz.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EditMixRequest {
    pub edit: Id,
    pub out: String,
    /// Normaliza: arib (-24 LKFS) | ebu (-23) | streaming (-14).
    #[serde(default)]
    pub preset: Option<LoudnessPreset>,
}

/// `TTS_REGENERATE`: refaz o WAV de uma fala do VOICEVOX no mesmo asset.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TtsRegenerateRequest {
    /// Asset da fala (devolvido por aiai_tts_line).
    pub asset: Id,
    /// Campos da consulta do VOICEVOX a mudar (merge): speedScale, pitchScale, intonationScale, volumeScale,
    /// prePhonemeLength, postPhonemeLength, pauseLengthScale, accent_phrases...
    #[serde(default)]
    #[schemars(with = "Option<serde_json::Map<String, serde_json::Value>>")]
    pub audio_query_patch: Option<Map<String, Value>>,
    #[serde(default)]
    pub voicevox_url: Option<String>,
}

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "RENDER_SHEET" => render_sheet(session, params),
        "EDIT_PREVIEW" => edit_preview(session, params),
        "EDIT_MIX" => edit_mix(session, params),
        "TTS_REGENERATE" => tts_regenerate(session, params),
        _ => Err(BridgeError::unknown_action(action)),
    }
}

fn save(img: &aiai_paint::Image, out: &str) -> Result<String, BridgeError> {
    let path = check_abs(out, "out")?;
    ensure_parent(&path)?;
    img.save_png(&path).map_err(|e| BridgeError::failed(format!("gravar {}: {e}", path.display())))?;
    Ok(path.display().to_string())
}

fn render_sheet(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: RenderSheetRequest = request(params, "RENDER_SHEET")?;
    let duration = match session.project.node(&r.cut).map(|n| &n.data) {
        Some(NodeKind::Cut(c)) => c.duration,
        _ => return Err(BridgeError::invalid_params(format!("{} não é corte", r.cut))),
    };
    let frames = r.select.resolve(duration)?;
    let img = preview::cut_contact_sheet(&session.project, &session.blobs, &r.cut, &frames, r.columns.unwrap_or(4), r.thumb_width.unwrap_or(480).clamp(64, 1920), r.camera.unwrap_or(true))
        .map_err(tool_error)?;
    let path = save(&img, &r.out)?;
    Ok(json!({ "path": path, "frames": frames, "width": img.width, "height": img.height }))
}

fn edit_preview(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: EditPreviewRequest = request(params, "EDIT_PREVIEW")?;
    let doc = load_edit(&session.project, &r.edit).map_err(tool_error)?;
    let fps = session.project.settings.fps();
    let total = doc.timeline(&session.project, fps as f32).map_err(tool_error)?.total;
    let frames = if r.seconds.is_empty() {
        r.select.resolve(total)?
    } else {
        if r.seconds.len() > preview::MAX_SHEET_FRAMES {
            return Err(BridgeError::invalid_params(format!("no máximo {} instantes", preview::MAX_SHEET_FRAMES)));
        }
        r.seconds.iter().map(|s| ((s.max(0.0) * fps).round() as u32).min(total.saturating_sub(1))).collect()
    };
    let img = preview::edit_contact_sheet(&session.project, &session.blobs, &doc, &frames, r.columns.unwrap_or(3), r.thumb_width.unwrap_or(640).clamp(64, 1920), r.burn_subtitles.unwrap_or(true))
        .map_err(tool_error)?;
    let path = save(&img, &r.out)?;
    Ok(json!({ "path": path, "frames": frames, "total_frames": total, "seconds": total as f64 / fps }))
}

fn edit_mix(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: EditMixRequest = request(params, "EDIT_MIX")?;
    let path = check_abs(&r.out, "out")?;
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
        return Err(BridgeError::invalid_params("'out' deve terminar em .wav"));
    }
    let doc = load_edit(&session.project, &r.edit).map_err(tool_error)?;
    let inter = mix_edit(&session.project, &session.blobs, &doc).map_err(tool_error)?;
    let (mut l, mut rr) = (Vec::with_capacity(inter.len() / 2), Vec::with_capacity(inter.len() / 2));
    for fr in inter.chunks_exact(2) {
        l.push(fr[0]);
        rr.push(fr[1]);
    }
    let mut buf = AudioBuf::stereo(session.project.settings.audio_sample_rate.max(8000), l, rr);
    let mut v = json!({ "duration_s": buf.duration_s() });
    if let Some(preset) = r.preset {
        v["normalize"] = serde_json::to_value(audio::normalize_mix(&mut buf, preset)).map_err(|e| BridgeError::failed(e.to_string()))?;
    }
    v["loudness"] = loudness_json(&audio::loudness_report(&buf), false)?;
    ensure_parent(&path)?;
    audio::export_wav(&buf, &path, WavBits::Pcm16).map_err(tool_error)?;
    v["path"] = json!(path.display().to_string());
    Ok(v)
}

fn tts_regenerate(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: TtsRegenerateRequest = request(params, "TTS_REGENERATE")?;
    let url = voicevox_url(params)?;
    let aparams = match session.project.node(&r.asset).map(|n| &n.data) {
        Some(NodeKind::Asset(a)) => a.params.clone(),
        _ => return Err(BridgeError::invalid_params(format!("{} não é asset", r.asset))),
    };
    let speaker = aparams.get("speaker").and_then(Value::as_u64).ok_or_else(|| BridgeError::invalid_params("asset sem `speaker`: não é fala do VOICEVOX"))? as u32;
    let mut qv = aparams.get("audio_query").cloned().ok_or_else(|| BridgeError::invalid_params("asset sem `audio_query`"))?;
    if let (Some(q), Some(patch)) = (qv.as_object_mut(), &r.audio_query_patch) {
        for (k, v) in patch {
            q.insert(k.clone(), v.clone());
        }
    }
    let q: AudioQuery = serde_json::from_value(qv).map_err(|e| BridgeError::invalid_params(format!("audio_query inválida depois do patch: {e}")))?;
    let line = VoicevoxClient::new(&url).synthesize_query(&q, speaker).map_err(|e| BridgeError::failed(format!("VOICEVOX: {e}")))?;
    let duration = aiai_audio::wav::read_wav(&line.wav).map(|b| b.duration_s()).ok();
    let h = session.blobs.put(line.wav);
    let cmd = Command::SetProps { id: r.asset.clone(), name: None, data: json!({ "blob": h, "params": { "audio_query": line.query, "timings": line.timings } }) };
    let mut v = apply(session, params, cmd)?;
    v["asset"] = json!(r.asset);
    v["duration_s"] = json!(duration);
    v["hint"] = json!("se a duração mudou, confira o lip-sync (aiai_lipsync) e a posição dos clipes seguintes");
    Ok(v)
}
