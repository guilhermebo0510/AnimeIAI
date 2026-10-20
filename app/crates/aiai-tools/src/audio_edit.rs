//! Edição e análise de áudio (S24): `AUDIO_EDIT` aplica passos (`aiai_audio::steps`: cortar, fade, efeitos, rack...) a um
//! asset de áudio e guarda o resultado como **novo** asset (a fonte não é tocada; `params.edited_from` e `params.steps`
//! registram a origem), e `AUDIO_ANALYZE` mede (LUFS, true peak, LRA...) e resume forma de onda e espectrograma na
//! resolução pedida. Como as demais ferramentas, a edição devolve comandos do núcleo (um passo de undo).

use crate::audio::{ClipOutput, PlacementOpts, WavBits, asset_audio, finish_clip, wav_bytes};
use crate::context::*;
use aiai_audio::analysis::{Spectrogram, SpectrogramParams, WaveformCache};
use aiai_audio::edit::secs_to_frames;
use aiai_audio::fft::WindowKind;
use aiai_audio::meters;
use aiai_audio::steps::{EditStep, apply_steps};
use aiai_audio::{AudioBuf, AudioError};
use aiai_core::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};

/// Teto de passos por chamada.
pub const MAX_STEPS: usize = 64;

fn default_track() -> AudioTrack {
    AudioTrack::Sfx
}

fn default_bits() -> WavBits {
    WavBits::Pcm24
}

/// Aplica `steps` a um clipe de áudio do projeto e guarda o resultado como novo asset (e, com `cut`, posiciona-o).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AudioEditRequest {
    /// Asset de áudio de origem (não é alterado).
    pub asset: Id,
    /// Passos em ordem (`op`: trim, delete, silence, duplicate, insert_silence, fade, reverse, invert, gain, resample, mono,
    /// stereo, remix, speed, fx, rack). `fx` aplica um efeito (`type`: amplify, normalize, eq, compressor, limiter,
    /// noise_gate, noise_reduction, reverb, pitch_shift, time_stretch, dc_offset, filter) ao clipe ou a `[start_s, end_s)`.
    pub steps: Vec<EditStep>,
    /// Nome do novo asset; padrão = nome da fonte + " (editado)".
    #[serde(default)]
    pub name: Option<String>,
    /// Trilha do clipe quando `cut` vem; padrão sfx.
    #[serde(default = "default_track")]
    pub track: AudioTrack,
    /// Formato do WAV guardado: pcm24 (padrão), pcm16 ou float32.
    #[serde(default = "default_bits")]
    pub bits: WavBits,
    #[serde(flatten)]
    pub place: PlacementOpts,
}

pub struct AudioEditOutput {
    pub clip: ClipOutput,
    pub source: Id,
    pub source_duration_s: f64,
    pub rate: u32,
    pub channels: usize,
    pub steps_applied: usize,
    pub source_name: String,
}

fn truncate_name(s: &str) -> String {
    s.chars().take(60).collect()
}

pub fn audio_edit(p: &Project, idgen: &mut IdGen, blobs: &mut BlobStore, req: &AudioEditRequest) -> ToolResult<AudioEditOutput> {
    if req.steps.is_empty() {
        return Err(ToolError::Invalid("'steps' está vazio: informe ao menos um passo".into()));
    }
    if req.steps.len() > MAX_STEPS {
        return Err(ToolError::Invalid(format!("no máximo {MAX_STEPS} passos por chamada (recebi {})", req.steps.len())));
    }
    let src = asset_audio(p, blobs, &req.asset)?;
    let (src_node, src_params, src_source) = match &node(p, &req.asset)?.data {
        NodeKind::Asset(a) => (node(p, &req.asset)?.name.clone(), a.params.clone(), a.source.clone()),
        _ => unreachable!("asset_audio já confirmou o tipo"),
    };
    let out = apply_steps(&src, &req.steps).map_err(|e: AudioError| ToolError::Invalid(e.to_string()))?;
    if out.frames() == 0 {
        return Err(ToolError::Invalid("o resultado ficou vazio (0 quadros): confira os passos".into()));
    }
    let bytes = wav_bytes(&out, req.bits)?;
    // a origem da voz (crédito obrigatório do VOICEVOX) acompanha o asset derivado; o resto do TTS não, para que
    // `TTS_REGENERATE` não refaça a fala por cima da edição
    let mut params = json!({
        "edited_from": req.asset,
        "steps": serde_json::to_value(&req.steps).map_err(|e| ToolError::Invalid(e.to_string()))?,
        "sample_rate": out.rate,
        "channels": out.channels(),
    });
    for k in ["credit", "engine", "speaker_name", "style", "text"] {
        if let Some(v) = src_params.get(k) {
            params[k] = v.clone();
        }
    }
    let source = if src_params.get("credit").is_some() && !src_source.is_empty() { src_source } else { "audio_edit".to_string() };
    let name = req.name.clone().unwrap_or_else(|| format!("{src_node} (editado)"));
    let clip = finish_clip(p, idgen, blobs, bytes, out.duration_s(), &truncate_name(&name), params, &source, req.track, &req.place, None)?;
    Ok(AudioEditOutput { clip, source: req.asset.clone(), source_duration_s: src.duration_s(), rate: out.rate, channels: out.channels(), steps_applied: req.steps.len(), source_name: src_node })
}

// ---------------------------------------------------------------- análise

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpectrogramSummary {
    /// Colunas (tempo) da grade devolvida; padrão 128, máximo 1024.
    #[serde(default)]
    pub width: Option<usize>,
    /// Linhas (frequência, agudos primeiro); padrão 64, máximo 512.
    #[serde(default)]
    pub height: Option<usize>,
    /// FFT (potência de 2, 256 a 16384); padrão 2048.
    #[serde(default)]
    pub fft_size: Option<usize>,
    #[serde(default)]
    pub window: Option<WindowKind>,
    /// Canal (padrão = média dos canais).
    #[serde(default)]
    pub channel: Option<usize>,
    /// Piso em dB (padrão -90); 0 dB = seno de escala cheia.
    #[serde(default)]
    pub min_db: Option<f32>,
    /// Eixo de frequência logarítmico (padrão true).
    #[serde(default)]
    pub log_freq: Option<bool>,
    /// Faixa de frequência em Hz (padrão 20 a Nyquist).
    #[serde(default)]
    pub f_lo: Option<f64>,
    #[serde(default)]
    pub f_hi: Option<f64>,
}

/// Mede um áudio e resume forma de onda e espectrograma. Exatamente um entre `asset`, `cut` (mixagem) e `path` (WAV).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AudioAnalyzeRequest {
    /// Clipe de áudio do projeto.
    #[serde(default)]
    pub asset: Option<Id>,
    /// Corte: analisa a mixagem.
    #[serde(default)]
    pub cut: Option<Id>,
    /// WAV sob C:\AnimeIAI.
    #[serde(default)]
    pub path: Option<String>,
    /// Intervalo analisado em segundos (padrão = tudo).
    #[serde(default)]
    pub start_s: Option<f64>,
    #[serde(default)]
    pub end_s: Option<f64>,
    /// Inclui os medidores (LUFS momentâneo/curto/integrado, LRA, true peak, pico, RMS, DC, clipping). Padrão true.
    #[serde(default)]
    pub meters: Option<bool>,
    /// Pontos das séries de loudness (0 = sem séries; máximo 1000).
    #[serde(default)]
    pub series_points: Option<usize>,
    /// Colunas do resumo da forma de onda (mín/máx/RMS por coluna e por canal); omitido = sem forma de onda. Máximo 4096.
    #[serde(default)]
    pub waveform_pixels: Option<usize>,
    /// Resumo do espectrograma; omitido = sem espectrograma.
    #[serde(default)]
    pub spectrogram: Option<SpectrogramSummary>,
}

fn pyramid_cache() -> &'static WaveformCache {
    static C: OnceLock<WaveformCache> = OnceLock::new();
    C.get_or_init(|| WaveformCache::new(16))
}

/// Hash do blob de um asset de áudio (chave do cache de forma de onda).
pub fn asset_blob_key(p: &Project, asset: &Id) -> Option<String> {
    match &p.node(asset)?.data {
        NodeKind::Asset(a) => a.blob.as_ref().map(|h| h.to_string()),
        _ => None,
    }
}

fn round4(x: f32) -> f64 {
    (x as f64 * 1e4).round() / 1e4
}

/// Resume `buf` conforme `req`. `cache_key` identifica o conteúdo (hash do blob) para reaproveitar a pirâmide de forma de onda.
pub fn audio_analyze(buf: &AudioBuf, cache_key: Option<&str>, req: &AudioAnalyzeRequest) -> ToolResult<Value> {
    let bad = |m: String| ToolError::Invalid(m);
    for (n, v) in [("start_s", req.start_s), ("end_s", req.end_s)] {
        if let Some(v) = v {
            if !(v.is_finite() && v >= 0.0) {
                return Err(bad(format!("{n} precisa ser >= 0")));
            }
        }
    }
    let n = buf.frames();
    let a = req.start_s.map_or(0, |s| secs_to_frames(buf.rate, s)).min(n);
    let b = req.end_s.map_or(n, |s| secs_to_frames(buf.rate, s)).min(n);
    if b < a {
        return Err(bad("intervalo invertido: start_s > end_s".into()));
    }
    let whole = a == 0 && b == n;
    let sub = if whole { None } else { Some(aiai_audio::edit::copy(buf, a..b)) };
    let view: &AudioBuf = sub.as_ref().unwrap_or(buf);
    let mut out = json!({
        "sample_rate": buf.rate,
        "channels": buf.channels(),
        "duration_s": buf.duration_s(),
        "range": { "start_s": a as f64 / buf.rate as f64, "end_s": b as f64 / buf.rate as f64, "frames": b - a },
    });
    if req.meters.unwrap_or(true) {
        let pts = req.series_points.unwrap_or(0).min(1000);
        let mut m = serde_json::to_value(meters::analyze(view, pts.max(1))).map_err(|e| bad(e.to_string()))?;
        if pts == 0 {
            if let Some(o) = m.as_object_mut() {
                o.remove("momentary_series");
                o.remove("short_term_series");
                o.remove("series_hop_s");
            }
        }
        out["meters"] = m;
    }
    if let Some(px) = req.waveform_pixels {
        if !(1..=4096).contains(&px) {
            return Err(bad(format!("waveform_pixels fica entre 1 e 4096 (recebi {px})")));
        }
        let pyr: Arc<aiai_audio::analysis::WaveformPyramid> = match cache_key {
            Some(k) => pyramid_cache().get_or_build(k, || Ok(buf.clone())).map_err(|e| bad(e.to_string()))?,
            None => Arc::new(aiai_audio::analysis::WaveformPyramid::build(buf)),
        };
        let chans: Vec<Value> = (0..buf.channels())
            .map(|c| {
                let q = pyr.query(buf, Some(c), a, b, px);
                json!({
                    "channel": c,
                    "min": q.iter().map(|m| round4(m.min)).collect::<Vec<_>>(),
                    "max": q.iter().map(|m| round4(m.max)).collect::<Vec<_>>(),
                    "rms": q.iter().map(|m| round4(m.rms)).collect::<Vec<_>>(),
                })
            })
            .collect();
        out["waveform"] = json!({ "pixels": px, "frames_per_pixel": (b - a) as f64 / px as f64, "channels": chans });
    }
    if let Some(sp) = &req.spectrogram {
        let (w, h) = (sp.width.unwrap_or(128), sp.height.unwrap_or(64));
        if !(1..=1024).contains(&w) || !(1..=512).contains(&h) {
            return Err(bad("spectrogram: width fica entre 1 e 1024 e height entre 1 e 512".into()));
        }
        let fft = sp.fft_size.unwrap_or(2048);
        let hop = ((b - a) / w).max(64);
        let params = SpectrogramParams { fft_size: fft, hop, window: sp.window.unwrap_or_default(), min_db: sp.min_db.unwrap_or(-90.0), max_db: 0.0, channel: sp.channel };
        let spec = Spectrogram::compute(view, 0..view.frames(), &params).map_err(|e| bad(e.to_string()))?;
        let log = sp.log_freq.unwrap_or(true);
        let nyq = buf.rate as f64 / 2.0;
        let (f_lo, f_hi) = (sp.f_lo.unwrap_or(20.0), sp.f_hi.unwrap_or(nyq));
        if !(f_lo.is_finite() && f_hi.is_finite() && f_lo >= 0.0 && f_hi > f_lo) {
            return Err(bad("spectrogram: f_lo/f_hi inválidos (0 <= f_lo < f_hi)".into()));
        }
        let grid = spec.to_u8(w, h, f_lo, f_hi, log);
        let rows: Vec<Vec<u8>> = grid.chunks(w).map(|r| r.to_vec()).collect();
        out["spectrogram"] = json!({
            "width": w, "height": h, "fft_size": spec.fft_size, "hop": spec.hop, "window": sp.window.unwrap_or_default(),
            "min_db": spec.min_db, "max_db": spec.max_db, "f_lo": f_lo.max(if log { 20.0 } else { 0.0 }), "f_hi": f_hi.min(nyq), "log_freq": log,
            "seconds_per_column": (b - a) as f64 / buf.rate as f64 / w as f64,
            "note": "rows[0] = frequência mais alta; valores 0..255 = min_db..max_db",
            "rows": rows,
        });
    }
    Ok(out)
}
