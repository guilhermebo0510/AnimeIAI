//! Ferramentas de produção sobre o documento (S02 B3): cenário (`SKY`, `PATTERN_FILL`), efeitos 2D (`TEXT`, `LINES`,
//! `GAG_BG`, `PARTICLES`), guias (`GUIDE3D`, `PERSPECTIVE`), personagem (`MODEL_SHEET`, `CONSISTENCY`), animação
//! (`INBETWEEN`, `PROPAGATE_FILLS`), áudio (`TTS_LINE`, `TTS_KOKORO`, `VOICE_PROFILE`, `LIPSYNC`, `SFX`, `BGM`, `MIX_CUT`,
//! `LOUDNESS`), calibração N1 (`COMPARE`, `PROBE_COLOR`, `MEASURE_LINE`) e `EXPORT_VIDEO`.
//!
//! Mesmo padrão de [`crate::core_tools`]: cada ferramenta de `aiai-tools` devolve um `Command`, aplicado pela `CoreSession`
//! (autor `agent:claude`, `author`/`if_revision` opcionais), então entra no undo/redo e no log e roda igual no app, na
//! bridge e no headless. As mutações devolvem o `Outcome` + `can_undo`/`can_redo` (+ campos próprios da ferramenta).
//! `COMPARE`, `CONSISTENCY`, `PROBE_COLOR`, `MEASURE_LINE`, `LOUDNESS`, `MIX_CUT` e `EXPORT_VIDEO` não mudam o documento.
//!
//! Caminhos de gravação (`out`, `out_dir`) devem ser absolutos e sem `..` (o `aiai-mcp` ainda os confina às pastas
//! permitidas); caminhos de leitura (`reference`, `font`, `soundfont`, `srt`) são lidos como estão (o MCP restringe a
//! leitura a C:\AnimeIAI e às pastas de gravação).

use crate::core_ops::{obj, opt_str};
use crate::core_tools::{apply, request, tool_error};
use crate::protocol::{BridgeError, code};
use aiai_audio::AudioBuf;
use aiai_audio::loudness::{LoudnessPreset, LoudnessReport};
use aiai_audio::mixer::MixSettings;
use aiai_audio::voicevox::VoicevoxClient;
use aiai_core::{Command, CoreSession, Id, LayerType, NodeKind};
use aiai_media::{EncodeOptions, Encoder, Subtitle, VideoCodec};
use aiai_render::{RenderOptions, render_frame};
use aiai_tools::ToolError;
use aiai_tools::anim::{InbetweenRequest, PropagateFillsRequest, inbetween_command, propagate_fills};
use aiai_tools::audio::{self, BgmRequest, KokoroLineRequest, LipsyncRequest, SfxRequest, TtsLineRequest, VoiceProfile, WavBits};
use aiai_tools::background::{PatternFillRequest, SkyRequest, pattern_fill_command, sky_command};
use aiai_tools::calib::{load_reference, measure_band, measure_edge, measure_line_color, probe_colors};
use aiai_tools::edit::{EditDoc, edit_command, load_edit, mix_edit, render_edit_frame};
use aiai_tools::paint::{FillShapeRequest, fill_shape_command};
use aiai_tools::char_material::{self, CharMaterialRequest};
use aiai_tools::character::{ConsistencyRequest, ModelSheetRequest, consistency, model_sheet};
use aiai_tools::compare::{CompareRequest, compare_frame};
use aiai_tools::context::{canvas_size, cut_data, layer_data};
use aiai_tools::fx2d::{GagBgRequest, LinesRequest, TextRequest, gag_bg_command, lines_command, text_command};
use aiai_tools::layer_merge::{LayerMergeRequest, layer_merge_command};
use aiai_tools::text_layer::{TextLayerRequest, text_layer_command};
use aiai_tools::gradient_paint::{GradientPaintRequest, gradient_paint};
use aiai_tools::guide::{Guide3dRequest, PerspectiveRequest, guide3d_command, perspective_command, perspective_info};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const PRODUCTION_ACTIONS: &[&str] = &[
    "SKY", "PATTERN_FILL", "TEXT", "TEXT_LAYER", "LAYER_MERGE", "LINES", "GAG_BG", "PARTICLES", "GUIDE3D", "PERSPECTIVE", "MODEL_SHEET", "CHARACTER_MATERIAL", "CONSISTENCY", "INBETWEEN", "PROPAGATE_FILLS",
    "TTS_LINE", "TTS_KOKORO", "VOICE_PROFILE", "LIPSYNC", "SFX", "BGM", "IMPORT_AUDIO", "IMPORT_IMAGE", "IMPORT_LAYERS", "EXPORT_PSD", "EXPORT_XDTS", "EXPORT_SVG", "EXPORT_OTIO", "MIX_CUT", "LOUDNESS", "COMPARE", "PROBE_COLOR", "MEASURE_LINE", "MEASURE_EDGE", "FILL_SHAPE", "EDIT_SET", "EXPORT_VIDEO", "GRADIENT_PAINT",
];

/// Ações que mudam o documento (o headless salva o projeto depois delas).
pub fn is_mutating(action: &str) -> bool {
    matches!(
        action,
        "SKY" | "PATTERN_FILL" | "TEXT" | "TEXT_LAYER" | "LAYER_MERGE" | "LINES" | "GAG_BG" | "PARTICLES" | "GUIDE3D" | "PERSPECTIVE" | "MODEL_SHEET" | "CHARACTER_MATERIAL" | "INBETWEEN" | "PROPAGATE_FILLS" | "TTS_LINE" | "TTS_KOKORO" | "VOICE_PROFILE" | "LIPSYNC" | "SFX" | "BGM" | "IMPORT_AUDIO" | "IMPORT_IMAGE" | "IMPORT_LAYERS" | "FILL_SHAPE" | "EDIT_SET" | "GRADIENT_PAINT"
    )
}

/// Ações que não dependem de um projeto aberto (o headless as roda mesmo sem projeto).
pub fn is_stateless(action: &str) -> bool {
    matches!(action, "PROBE_COLOR" | "MEASURE_LINE" | "MEASURE_EDGE")
}

/// `frame` ausente = 0 nas ações por quadro (o MCP já faz isso; a CLI e o cliente Python também se beneficiam).
fn with_default_frame(action: &str, params: &Value) -> Option<Value> {
    if matches!(action, "SKY" | "PATTERN_FILL" | "TEXT" | "LINES" | "GAG_BG" | "GUIDE3D" | "PERSPECTIVE" | "COMPARE") {
        if let Some(m) = params.as_object().filter(|m| m.get("frame").is_none_or(Value::is_null)) {
            let mut m = m.clone();
            m.insert("frame".into(), json!(0));
            return Some(Value::Object(m));
        }
    }
    None
}

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    let patched = with_default_frame(action, params);
    let params = patched.as_ref().unwrap_or(params);
    match action {
        "SKY" => {
            let r: SkyRequest = request(params, action)?;
            let cmd = sky_command(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        "PATTERN_FILL" => {
            let r: PatternFillRequest = request(params, action)?;
            let cmd = pattern_fill_command(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        "TEXT" => {
            let r: TextRequest = request(params, action)?;
            let cmd = text_command(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        "TEXT_LAYER" => {
            let r: TextLayerRequest = request(params, action)?;
            if r.list_fonts {
                return Ok(aiai_tools::text_layer::list_fonts());
            }
            if r.list_styles {
                return Ok(aiai_tools::text_layer::list_styles());
            }
            let plan = text_layer_command(&session.project, &mut session.ids, &r).map_err(tool_error)?;
            let info = aiai_tools::text_layer::info(&plan.params);
            apply_with(session, params, plan.command, json!({ "layer": plan.layer, "created": plan.created, "text": info }))
        }
        "LAYER_MERGE" => {
            let r: LayerMergeRequest = request(params, action)?;
            let plan = layer_merge_command(&session.project, &mut session.blobs, &mut session.ids, &r).map_err(tool_error)?;
            let info = json!({ "layer": plan.layer, "merged": plan.merged, "kind": plan.kind, "cels_changed": plan.cels_changed, "cels_created": plan.cels_created });
            apply_with(session, params, plan.command, info)
        }
        "GRADIENT_PAINT" => {
            let r: GradientPaintRequest = request(params, action)?;
            let plan = gradient_paint(&session.project, &session.blobs, &mut session.ids, &r).map_err(tool_error)?;
            match plan.command {
                Some(cmd) => apply_with(session, params, cmd, plan.value),
                None => Ok(plan.value),
            }
        }
        "LINES" => {
            let r: LinesRequest = request(params, action)?;
            let cmd = lines_command(&session.project, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        "GAG_BG" => {
            let r: GagBgRequest = request(params, action)?;
            let cmd = gag_bg_command(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        "PARTICLES" => particles(session, params),
        "GUIDE3D" => {
            let r: Guide3dRequest = request(params, action)?;
            let cmd = guide3d_command(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        "PERSPECTIVE" => {
            let r: PerspectiveRequest = request(params, action)?;
            let info = perspective_info(&session.project, &r).map_err(tool_error)?;
            let cmd = perspective_command(&session.project, &r).map_err(tool_error)?;
            apply_with(session, params, cmd, json!({ "perspective": info }))
        }
        "MODEL_SHEET" => {
            let r: ModelSheetRequest = request(params, action)?;
            let plan = model_sheet(&session.project, &mut session.ids, &r).map_err(tool_error)?;
            let cmd = Command::Batch { commands: plan.commands, label: Some(format!("Model sheet {}", r.name)) };
            apply_with(session, params, cmd, json!({ "cut": plan.cut, "line_layer": plan.line_layer, "fill_layer": plan.fill_layer, "material": plan.material }))
        }
        "CHARACTER_MATERIAL" => character_material(session, params),
        "CONSISTENCY" => {
            let r: ConsistencyRequest = request(params, action)?;
            let rep = consistency(&session.project, &r).map_err(tool_error)?;
            to_json(&rep)
        }
        "INBETWEEN" => {
            let r: InbetweenRequest = request(params, action)?;
            let cmd = inbetween_command(&session.project, &mut session.ids, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        "PROPAGATE_FILLS" => {
            let r: PropagateFillsRequest = request(params, action)?;
            let cmds = propagate_fills(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
            if cmds.is_empty() {
                return Err(BridgeError::invalid_params("PROPAGATE_FILLS: 'frames' vazio (informe os quadros a pintar)"));
            }
            let n = cmds.len();
            let cmd = Command::Batch { commands: cmds, label: Some(format!("Propagar preenchimento em {n} quadro(s)")) };
            apply_with(session, params, cmd, json!({ "frames_filled": r.frames }))
        }
        "TTS_LINE" => tts_line(session, params),
        "TTS_KOKORO" => {
            let r: KokoroLineRequest = request(params, action)?;
            let out = audio::tts_kokoro_line(&session.project, &mut session.ids, &mut session.blobs, &r).map_err(|e| match e {
                ToolError::NotFound(m) => BridgeError::failed(format!("Kokoro indisponível: {m} (veja sprints/registro/licencas-voz.md e tools/kokoro)")),
                other => tool_error(other),
            })?;
            let cmd = out.batch("Fala (Kokoro)");
            apply_with(session, params, cmd, clip_info(&out))
        }
        "VOICE_PROFILE" => {
            let r: VoiceProfileRequest = request(params, action)?;
            let cmd = audio::voice_profile_command(&session.project, &r.character, &r.profile).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        "LIPSYNC" => {
            let r: LipsyncRequest = request(params, action)?;
            let out = audio::lipsync(&session.project, &mut session.ids, &session.blobs, &r).map_err(tool_error)?;
            let extra = json!({ "track": out.track, "first_frame": out.first_frame, "frames": out.frames.len(), "exposures": out.exposures, "used_moras": out.used_moras });
            apply_with(session, params, out.command, extra)
        }
        "SFX" => {
            let r: SfxRequest = request(params, action)?;
            let out = audio::sfx(&session.project, &mut session.ids, &mut session.blobs, &r).map_err(tool_error)?;
            let cmd = out.batch("SFX");
            apply_with(session, params, cmd, clip_info(&out))
        }
        "BGM" => {
            let r: BgmRequest = request(params, action)?;
            let out = audio::bgm(&session.project, &mut session.ids, &mut session.blobs, &r).map_err(tool_error)?;
            let cmd = out.clip.batch("BGM");
            let mut extra = clip_info(&out.clip);
            extra["engine"] = json!(match out.engine {
                aiai_audio::bgm::BgmEngine::SoundFont => "soundfont",
                aiai_audio::bgm::BgmEngine::Builtin => "builtin",
            });
            extra["soundfont"] = json!(out.soundfont);
            apply_with(session, params, cmd, extra)
        }
        "IMPORT_AUDIO" => {
            let r: audio::ImportAudioRequest = request(params, action)?;
            check_abs(&r.path, "path")?;
            let out = audio::import_audio(&session.project, &mut session.ids, &mut session.blobs, &r).map_err(tool_error)?;
            let cmd = out.batch("Importar áudio");
            apply_with(session, params, cmd, clip_info(&out))
        }
        "IMPORT_IMAGE" => {
            let r: aiai_tools::import::ImportImageRequest = request(params, action)?;
            check_abs(&r.path, "path")?;
            let cmd = aiai_tools::import::import_image_command(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        "IMPORT_LAYERS" => {
            let r: aiai_tools::import_layers::ImportLayersRequest = request(params, action)?;
            check_abs(&r.path, "path")?;
            let cmd = aiai_tools::import_layers::import_layers_command(&session.project, &mut session.blobs, &mut session.ids, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        "EXPORT_PSD" => {
            let r: aiai_tools::export::ExportPsdRequest = request(params, action)?;
            check_abs(&r.out, "out")?;
            ensure_parent(std::path::Path::new(&r.out))?;
            aiai_tools::export::export_psd(&session.project, &session.blobs, &r).map_err(tool_error)
        }
        "EXPORT_XDTS" => {
            let r: aiai_tools::export::ExportXdtsRequest = request(params, action)?;
            check_abs(&r.out, "out")?;
            ensure_parent(std::path::Path::new(&r.out))?;
            aiai_tools::export::export_xdts(&session.project, &r).map_err(tool_error)
        }
        "EXPORT_SVG" => {
            let r: aiai_tools::export::ExportSvgRequest = request(params, action)?;
            check_abs(&r.out, "out")?;
            ensure_parent(std::path::Path::new(&r.out))?;
            aiai_tools::export::export_svg(&session.project, &session.blobs, &r).map_err(tool_error)
        }
        "EXPORT_OTIO" => {
            let r: aiai_tools::export::ExportOtioRequest = request(params, action)?;
            check_abs(&r.out, "out")?;
            if let Some(d) = &r.media_dir {
                check_abs(d, "media_dir")?;
            }
            ensure_parent(std::path::Path::new(&r.out))?;
            aiai_tools::export::export_otio(&session.project, &session.blobs, &r).map_err(tool_error)
        }
        "MIX_CUT" => mix_cut(session, params),
        "LOUDNESS" => loudness(session, params),
        "COMPARE" => {
            let r: CompareRequest = request(params, action)?;
            if let Some(d) = &r.out_dir {
                check_abs(d, "out_dir")?;
            }
            let v = compare_frame(&session.project, &session.blobs, &r).map_err(tool_error)?;
            to_json(&v)
        }
        "PROBE_COLOR" => probe_color(params),
        "MEASURE_LINE" => measure(params),
        "MEASURE_EDGE" => measure_edge_action(params),
        "FILL_SHAPE" => {
            let r: FillShapeRequest = request(params, action)?;
            let cmd = fill_shape_command(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        "EDIT_SET" => edit_set(session, params),
        "EXPORT_VIDEO" => export_video(session, params),
        _ => Err(BridgeError::unknown_action(action)),
    }
}

// --------------------------------------------------------------------------------------------------- utilidades

fn to_json<T: Serialize>(v: &T) -> Result<Value, BridgeError> {
    serde_json::to_value(v).map_err(|e| BridgeError::failed(e.to_string()))
}

/// Aplica o comando e acrescenta campos próprios da ferramenta ao `Outcome`.
fn apply_with(session: &mut CoreSession, params: &Value, cmd: Command, extra: Value) -> Result<Value, BridgeError> {
    let mut v = apply(session, params, cmd)?;
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in e {
            o.insert(k.clone(), x.clone());
        }
    }
    Ok(v)
}

/// Pasta do personagem: `list` devolve os materiais (não altera nada); as outras ações (create, import, rename, duplicate,
/// delete, move, update) aplicam UM comando e devolvem o Outcome + `material`, `cut`, as camadas para desenhar, as células
/// do modelo (rótulo e retângulo) e a lista depois da mudança.
fn character_material(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: CharMaterialRequest = request(params, "CHARACTER_MATERIAL")?;
    let plan = char_material::plan(&session.project, &mut session.ids, &mut session.blobs, &r).map_err(tool_error)?;
    let list: Vec<Value> = plan.list.iter().map(char_material::material_json).collect();
    let cells: Vec<Value> = plan.cells.iter().map(|(rect, label)| json!({ "label": label, "rect": rect })).collect();
    let extra = json!({ "material": plan.material, "cut": plan.cut, "line_layer": plan.line_layer, "fill_layer": plan.fill_layer, "cells": cells, "canvas": plan.canvas, "materials": list });
    match plan.command {
        None => Ok(extra),
        Some(cmd) => apply_with(session, params, cmd, extra),
    }
}

fn clip_info(c: &audio::ClipOutput) -> Value {
    json!({ "asset": c.asset, "placement": c.placement, "duration_s": c.duration_s, "duration_frames": c.duration_frames })
}

/// Caminho de gravação: absoluto e sem `..` (o MCP confina às pastas permitidas).
pub(crate) fn check_abs(path: &str, what: &str) -> Result<PathBuf, BridgeError> {
    let p = Path::new(path);
    if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(BridgeError::invalid_params(format!("'{what}' deve ser um caminho absoluto sem '..'")));
    }
    Ok(p.to_path_buf())
}

pub(crate) fn ensure_parent(p: &Path) -> Result<(), BridgeError> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| BridgeError::new(code::ACTION_FAILED, format!("criando {}: {e}", dir.display())))?;
    }
    Ok(())
}

fn parse_hex_rgb(s: &str, what: &str) -> Result<[f32; 3], BridgeError> {
    let c = aiai_core::Rgba::from_hex(s).ok_or_else(|| BridgeError::invalid_params(format!("'{what}' deve ser #rrggbb, veio {s:?}")))?;
    Ok([c.0[0], c.0[1], c.0[2]])
}

/// Relatório de loudness como JSON; as séries (um ponto por 100 ms) só entram com `series`.
pub(crate) fn loudness_json(r: &LoudnessReport, series: bool) -> Result<Value, BridgeError> {
    let mut v = to_json(r)?;
    if !series {
        if let Some(o) = v.as_object_mut() {
            o.remove("momentary_series");
            o.remove("short_term_series");
        }
    }
    Ok(v)
}

// ------------------------------------------------------------------------------------------------------ partículas

/// Sistema de partículas de uma camada `particles`: `preset` (nome; veja a descrição da ferramenta) + `overrides`
/// (mesclados em profundidade) ou um `system` completo (+ `overrides`). A camada precisa existir (crie com
/// `create_node {type: layer, kind: particles}`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ParticlesRequest {
    /// Camada de partículas.
    pub layer: Id,
    /// Corte (opcional; o padrão é o corte que contém a camada). Dá o tamanho do canvas aos presets.
    #[serde(default)]
    pub cut: Option<Id>,
    /// Preset: petalas | brilhos | neve | chuva | poeira | folhas (também rose_petals, sparkles, snow, rain, dust, leaves).
    #[serde(default)]
    pub preset: Option<String>,
    /// Campos do sistema a sobrescrever (mescla em profundidade; listas e números substituem).
    #[serde(default)]
    pub overrides: Option<Value>,
    /// Sistema completo (alternativa a `preset`).
    #[serde(default)]
    pub system: Option<aiai_anim::particles::ParticleSystem>,
    /// Semente do preset (mesma semente = mesma nevasca).
    #[serde(default)]
    pub seed: u64,
}

fn deep_merge(base: &mut Value, over: &Value) {
    match (base.as_object_mut(), over.as_object()) {
        (Some(b), Some(o)) => {
            for (k, v) in o {
                if v.is_null() {
                    b.remove(k);
                } else {
                    deep_merge(b.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        _ => *base = over.clone(),
    }
}

pub const PARTICLE_PRESETS: &[&str] = &["petalas", "brilhos", "neve", "chuva", "poeira", "folhas"];

fn particles(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: ParticlesRequest = request(params, "PARTICLES")?;
    let layer = layer_data(&session.project, &r.layer).map_err(tool_error)?;
    if layer.kind != LayerType::Particles {
        return Err(BridgeError::invalid_params(format!("{} não é camada de partículas: crie com create_node {{data: {{type: layer, kind: particles}}}}", r.layer)));
    }
    let cut_id = match &r.cut {
        Some(c) => c.clone(),
        None => session.project.cut_of(&r.layer).ok_or_else(|| BridgeError::invalid_params("a camada não está dentro de um corte; informe 'cut'"))?,
    };
    let cut = cut_data(&session.project, &cut_id).map_err(tool_error)?;
    let (w, h) = canvas_size(&session.project, cut);
    let base = match (&r.system, &r.preset) {
        (Some(_), Some(_)) => return Err(BridgeError::invalid_params("PARTICLES: informe 'preset' OU 'system', não os dois")),
        (Some(s), None) => s.clone(),
        (None, Some(name)) => aiai_anim::particles::preset(&name.trim().to_lowercase(), w as f32, h as f32, r.seed)
            .ok_or_else(|| BridgeError::invalid_params(format!("preset de partículas desconhecido {name:?}; presets: {}", PARTICLE_PRESETS.join(", "))))?,
        (None, None) => return Err(BridgeError::invalid_params(format!("PARTICLES exige 'preset' ({}) ou 'system'", PARTICLE_PRESETS.join(", ")))),
    };
    let mut value = to_json(&base)?;
    if let Some(ov) = r.overrides.as_ref().filter(|v| !v.is_null()) {
        if !ov.is_object() {
            return Err(BridgeError::invalid_params("'overrides' deve ser um objeto"));
        }
        deep_merge(&mut value, ov);
    }
    let sys: aiai_anim::particles::ParticleSystem = serde_json::from_value(value).map_err(|e| BridgeError::invalid_params(format!("sistema de partículas inválido: {e}")))?;
    sys.validate().map_err(|e| BridgeError::invalid_params(format!("sistema de partículas inválido: {e}")))?;
    // dois passos num só undo: remove o sistema antigo (merge patch mesclaria campos soltos) e grava o novo
    let cmd = Command::Batch {
        commands: vec![
            Command::SetProps { id: r.layer.clone(), name: None, data: json!({ "params": { "particles": null } }) },
            Command::SetProps { id: r.layer.clone(), name: None, data: json!({ "params": { "particles": sys } }) },
        ],
        label: Some("Partículas".into()),
    };
    apply_with(session, params, cmd, json!({ "canvas": [w, h], "particles": { "rate": sys.rate, "bursts": sys.bursts, "lifetime": sys.lifetime, "seed": sys.seed } }))
}

// ------------------------------------------------------------------------------------------------------------ áudio

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct VoiceProfileRequest {
    /// Personagem que recebe o perfil de voz.
    pub character: Id,
    #[serde(flatten)]
    pub profile: VoiceProfile,
}

fn is_loopback_url(url: &str) -> bool {
    let rest = url.strip_prefix("http://").unwrap_or("");
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    let name = if let Some(v6) = host.strip_prefix('[') { v6.split(']').next().unwrap_or("") } else { host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host) };
    matches!(name, "127.0.0.1" | "localhost" | "::1")
}

/// URL do VOICEVOX: parâmetro `voicevox_url` (só loopback) > env `AIAI_VOICEVOX_URL` > padrão.
pub(crate) fn voicevox_url(params: &Value) -> Result<String, BridgeError> {
    let p = obj(params)?;
    if let Some(u) = opt_str(&p, "voicevox_url")? {
        if !is_loopback_url(u) {
            return Err(BridgeError::invalid_params("'voicevox_url' deve ser http://127.0.0.1:<porta> (ou localhost); o motor roda só na sua máquina"));
        }
        return Ok(u.to_string());
    }
    match std::env::var("AIAI_VOICEVOX_URL") {
        Ok(u) if !u.trim().is_empty() => Ok(u),
        _ => Ok(audio::VOICEVOX_URL.to_string()),
    }
}

fn tts_line(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: TtsLineRequest = request(params, "TTS_LINE")?;
    let url = voicevox_url(params)?;
    if let Err(e) = VoicevoxClient::new(&url).version() {
        return Err(BridgeError::failed(format!(
            "VOICEVOX Engine não responde em {url} ({e}). Suba o motor com: powershell -File C:\\AnimeIAI\\tools\\voicevox\\start.ps1 (ou defina AIAI_VOICEVOX_URL / 'voicevox_url')"
        )));
    }
    let out = audio::tts_line(&session.project, &mut session.ids, &mut session.blobs, &url, &r).map_err(tool_error)?;
    let cmd = out.clip.batch("Fala (VOICEVOX)");
    let mut extra = clip_info(&out.clip);
    extra["credit"] = json!(out.credit);
    extra["moras"] = json!(out.timings.moras.len());
    extra["speech_start"] = json!(out.timings.speech_start);
    extra["speech_end"] = json!(out.timings.speech_end);
    apply_with(session, params, cmd, extra)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MixRequest {
    pub cut: Id,
    /// Quadros a mixar a partir do início; omitido = duração do corte.
    #[serde(default)]
    pub frames: Option<u32>,
    /// WAV de saída (absoluto, .wav). Omitido = só mede.
    #[serde(default)]
    pub out: Option<String>,
    /// Normaliza ao alvo: arib (-24 LKFS), ebu (-23 LUFS) ou streaming (-14 LUFS), teto de true peak -1 dBTP.
    #[serde(default)]
    pub preset: Option<LoudnessPreset>,
    /// pcm16 (padrão) | pcm24 | float32.
    #[serde(default)]
    pub bits: WavBits,
    /// Ajustes do mixer (ducking, EQ, ganhos por trilha...); omitido = padrão com a taxa do projeto.
    #[serde(default)]
    pub settings: Option<MixSettings>,
    /// Inclui as séries de loudness (um ponto por 100 ms).
    #[serde(default)]
    pub series: bool,
}

fn mix_cut(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: MixRequest = request(params, "MIX_CUT")?;
    let out_path = r.out.as_deref().map(|o| check_abs(o, "out")).transpose()?;
    if let Some(p) = &out_path {
        if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
            return Err(BridgeError::invalid_params("'out' deve terminar em .wav"));
        }
    }
    let settings = r.settings.clone().unwrap_or_else(|| MixSettings { rate: session.project.settings.audio_sample_rate.max(8000), ..Default::default() });
    let mut buf = audio::mix_cut_with(&session.project, &session.blobs, &r.cut, r.frames, &settings).map_err(tool_error)?;
    let mut result = json!({ "cut": r.cut, "duration_s": buf.duration_s(), "rate": buf.rate, "channels": buf.channels(), "clips": cut_data(&session.project, &r.cut).map(|c| c.audio.len()).unwrap_or(0) });
    if let Some(preset) = r.preset {
        let rep = audio::normalize_mix(&mut buf, preset);
        result["normalize"] = to_json(&rep)?;
    }
    result["loudness"] = loudness_json(&audio::loudness_report(&buf), r.series)?;
    if let Some(p) = out_path {
        ensure_parent(&p)?;
        audio::export_wav(&buf, &p, r.bits).map_err(tool_error)?;
        result["path"] = json!(p.display().to_string());
        result["bytes"] = json!(std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0));
    }
    Ok(result)
}

fn loudness(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let p = obj(params)?;
    crate::params_check::check_keys(params, "LOUDNESS", &["asset", "cut", "path", "series"])?;
    let series = p.get("series").and_then(Value::as_bool).unwrap_or(false);
    let buf: AudioBuf = match (opt_str(&p, "asset")?, opt_str(&p, "cut")?, opt_str(&p, "path")?) {
        (Some(a), None, None) => audio::asset_audio(&session.project, &session.blobs, &Id::from(a)).map_err(tool_error)?,
        (None, Some(c), None) => audio::mix_cut(&session.project, &session.blobs, &Id::from(c), None).map_err(tool_error)?,
        (None, None, Some(path)) => {
            let bytes = std::fs::read(path).map_err(|e| BridgeError::invalid_params(format!("lendo {path}: {e}")))?;
            aiai_audio::wav::read_wav(&bytes).map_err(|e| BridgeError::invalid_params(format!("{path}: {e}")))?
        }
        _ => return Err(BridgeError::invalid_params("LOUDNESS exige exatamente um de: 'asset' (clipe de áudio), 'cut' (mixagem do corte) ou 'path' (WAV)")),
    };
    loudness_json(&audio::loudness_report(&buf), series)
}

// ------------------------------------------------------------------------------------------ calibração N1 (medição)

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ProbeColorRequest {
    /// PNG do quadro de referência (qualquer lugar sob C:\AnimeIAI).
    pub reference: String,
    /// Pontos [x, y] em pixels da referência.
    pub points: Vec<[f32; 2]>,
    /// Raio (px) do quadrado de amostragem; a cor é a mediana por canal. Padrão 2.
    #[serde(default)]
    pub radius: Option<u32>,
}

fn probe_color(params: &Value) -> Result<Value, BridgeError> {
    let r: ProbeColorRequest = request(params, "PROBE_COLOR")?;
    if r.points.is_empty() || r.points.len() > 500 {
        return Err(BridgeError::invalid_params("'points' deve ter de 1 a 500 pontos [x, y]"));
    }
    let radius = r.radius.unwrap_or(2);
    if radius > 64 {
        return Err(BridgeError::invalid_params("'radius' deve ser 0..64"));
    }
    let img = load_reference(&r.reference).map_err(tool_error)?;
    Ok(json!({ "width": img.width, "height": img.height, "radius": radius, "colors": probe_colors(&img, &r.points, radius) }))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MeasureLineRequest {
    /// PNG do quadro de referência (qualquer lugar sob C:\AnimeIAI).
    pub reference: String,
    /// Pontos [x, y] do traço que VOCÊ estimou (em ordem ao longo do traço; a normal vem dos vizinhos).
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    /// Vários traços estimados de uma vez (cada um uma lista de pontos), alternativa a `points`.
    #[serde(default)]
    pub strokes: Vec<Vec<[f32; 2]>>,
    /// Alcance da busca ao longo da normal (px de cada lado). Padrão 6.
    #[serde(default)]
    pub search: Option<f32>,
    /// Contraste mínimo (0..1) para considerar que há linha. Padrão 0,1.
    #[serde(default)]
    pub min_contrast: Option<f32>,
    /// Cor da tinta "#rrggbb" (opcional): mede o vale de distância a essa cor em vez da luminância — use quando a
    /// linha passa sobre massas escuras (cabelo, sombra) e o contraste de luminância some.
    #[serde(default)]
    pub line_color: Option<String>,
    /// Com `line_color`: modo faixa (paquímetro) — pixels a até `tolerance` (0..1) da cor formam a faixa; devolve o meio
    /// e o comprimento do trecho contínuo junto ao ponto. Para faixas de cor que varia por dentro (realces).
    #[serde(default)]
    pub tolerance: Option<f32>,
}

fn measure(params: &Value) -> Result<Value, BridgeError> {
    let r: MeasureLineRequest = request(params, "MEASURE_LINE")?;
    let search = r.search.unwrap_or(6.0);
    let min_contrast = r.min_contrast.unwrap_or(0.1);
    if !(0.5..=64.0).contains(&search) || !(0.0..=1.0).contains(&min_contrast) {
        return Err(BridgeError::invalid_params("'search' deve ser 0,5..64 px e 'min_contrast' 0..1"));
    }
    let (single, strokes): (bool, Vec<Vec<[f32; 2]>>) = match (r.points.is_empty(), r.strokes.is_empty()) {
        (false, true) => (true, vec![r.points.clone()]),
        (true, false) => (false, r.strokes.clone()),
        _ => return Err(BridgeError::invalid_params("MEASURE_LINE exige 'points' (um traço) OU 'strokes' (vários), não os dois nem nenhum")),
    };
    if strokes.len() > 200 || strokes.iter().any(|s| s.len() < 2 || s.len() > 2000) {
        return Err(BridgeError::invalid_params("cada traço precisa de 2 a 2000 pontos (no máximo 200 traços)"));
    }
    let line_color = match &r.line_color {
        None => None,
        Some(h) => Some(parse_hex_rgb(h, "line_color")?),
    };
    let img = load_reference(&r.reference).map_err(tool_error)?;
    let summarize = |pts: &[[f32; 2]]| {
        let m = match (r.tolerance, line_color) {
            (Some(tol), Some(c)) => measure_band(&img, pts, search, c, tol),
            _ => measure_line_color(&img, pts, search, min_contrast, line_color),
        };
        let found: Vec<_> = m.iter().filter(|x| x.contrast > 0.0).collect();
        let n = found.len().max(1) as f32;
        json!({
            "found": found.len(), "total": m.len(),
            "mean_width": found.iter().map(|x| x.width).sum::<f32>() / n,
            "mean_moved": found.iter().map(|x| x.moved).sum::<f32>() / n,
            "measures": m,
        })
    };
    let mut out = if single { summarize(&strokes[0]) } else { json!({ "strokes": strokes.iter().map(|s| summarize(s)).collect::<Vec<_>>() }) };
    out["reference_size"] = json!([img.width, img.height]);
    out["note"] = json!("medição da referência perto do traço que você estimou (decisão 22): desenhe os SEUS traços com aiai_draw_curves usando estes centros/larguras; nada foi criado");
    Ok(out)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MeasureEdgeRequest {
    /// PNG do quadro de referência (qualquer lugar sob C:\AnimeIAI).
    pub reference: String,
    /// Pontos [x, y] do contorno da massa de cor que VOCÊ estimou (em ordem; o contorno é tratado como fechado).
    pub points: Vec<[f32; 2]>,
    /// Alcance da busca ao longo da normal (px de cada lado). Padrão 6.
    #[serde(default)]
    pub search: Option<f32>,
    /// Intensidade mínima do degrau de cor (0..1) para aceitar a borda. Padrão 0,06.
    #[serde(default)]
    pub min_strength: Option<f32>,
}

fn measure_edge_action(params: &Value) -> Result<Value, BridgeError> {
    let r: MeasureEdgeRequest = request(params, "MEASURE_EDGE")?;
    let search = r.search.unwrap_or(6.0);
    let min_strength = r.min_strength.unwrap_or(0.06);
    if !(0.5..=64.0).contains(&search) || !(0.0..=1.0).contains(&min_strength) {
        return Err(BridgeError::invalid_params("'search' deve ser 0,5..64 px e 'min_strength' 0..1"));
    }
    if r.points.len() < 3 || r.points.len() > 4000 {
        return Err(BridgeError::invalid_params("'points' deve ter de 3 a 4000 pontos do contorno"));
    }
    let img = load_reference(&r.reference).map_err(tool_error)?;
    let m = measure_edge(&img, &r.points, search, min_strength);
    let found: Vec<_> = m.iter().filter(|x| x.strength > 0.0).collect();
    let n = found.len().max(1) as f32;
    Ok(json!({
        "found": found.len(), "total": m.len(),
        "mean_moved": found.iter().map(|x| x.moved).sum::<f32>() / n,
        "measures": m,
        "reference_size": [img.width, img.height],
        "note": "medição da borda da massa de cor perto do contorno que você estimou (decisão 22): desenhe a SUA forma com aiai_fill_shape usando estes pontos de borda; nada foi criado",
    }))
}

/// Cria (ou, com `id`, substitui) um documento de edição v2 (trilhas V/A/S, itens, transições, filtros, legendas,
/// créditos). O v1 (`clips`/`subtitles`) continua aceito e é convertido antes de gravar.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EditSetRequest {
    /// Nome do documento (padrão "Edição").
    #[serde(default)]
    pub name: Option<String>,
    /// Documento de edição existente a substituir; omitido = cria um novo.
    #[serde(default)]
    pub id: Option<Id>,
    #[serde(flatten)]
    pub doc: EditDoc,
}

fn edit_set(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: EditSetRequest = request(params, "EDIT_SET")?;
    if let Some(id) = &r.id {
        load_edit(&session.project, id).map_err(tool_error)?; // confirma que é um documento de edição
    }
    let name = r.name.clone().unwrap_or_else(|| "Edição".into());
    let cmd = edit_command(&session.project, &mut session.ids, &name, &r.doc, r.id.as_ref()).map_err(tool_error)?;
    let mut doc = r.doc.clone();
    doc.normalize(&session.project).map_err(tool_error)?;
    let mut out = apply(session, params, cmd)?;
    let id = match &r.id {
        Some(i) => i.clone(),
        None => out["created"][0].as_str().map(Id::from).unwrap_or_default(),
    };
    out["edit"] = json!(id);
    out["timeline"] = crate::preview_tools::doc_summary(&doc, session);
    Ok(out)
}

// ------------------------------------------------------------------------------------------------------ exportação

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ExportVideoRequest {
    /// Corte a exportar (ou `episode` ou `edit`).
    #[serde(default)]
    pub cut: Option<Id>,
    /// Episódio inteiro: todos os cortes de todas as cenas, em ordem.
    #[serde(default)]
    pub episode: Option<Id>,
    /// Documento de edição (aiai_edit): cortes com transições, legendas e créditos já desenhados; o áudio é a mixagem da edição.
    #[serde(default)]
    pub edit: Option<Id>,
    /// Primeiro quadro (0-based) do corte; padrão 0. Só com `cut`.
    #[serde(default)]
    pub start: Option<u32>,
    /// Fim EXCLUSIVO: exporta [start, end); padrão = duração do corte. Só com `cut`.
    #[serde(default)]
    pub end: Option<u32>,
    /// Arquivo de saída (absoluto): .mp4 (h264/h264_nvenc), .mov (prores) ou, em png_sequence, um padrão com `%05d` (ex.: C:\\...\\q_%05d.png).
    pub out: String,
    /// h264 (padrão) | h264_nvenc | prores | png_sequence.
    #[serde(default)]
    pub codec: VideoCodec,
    /// Qualidade (CRF do x264 / CQ do NVENC), 16–20 = alta. Padrão 18.
    #[serde(default)]
    pub crf: Option<u32>,
    /// Inclui a mixagem de áudio do corte (padrão true; sem clipes de áudio sai mudo).
    #[serde(default)]
    pub audio: Option<bool>,
    /// Normaliza a mixagem: arib | ebu | streaming.
    #[serde(default)]
    pub preset: Option<LoudnessPreset>,
    /// Legenda SRT existente para queimar na imagem (sob C:\AnimeIAI).
    #[serde(default)]
    pub srt: Option<String>,
    /// Ou legendas em linha {start, end (s), text}, gravadas num SRT temporário e queimadas.
    #[serde(default)]
    pub subtitles: Vec<Subtitle>,
    /// Fundo sob a transparência (#rrggbb). Padrão #ffffff.
    #[serde(default)]
    pub background: Option<String>,
    /// Grava também a legenda como arquivo `.srt` ao lado do vídeo (mesmo nome), para entrega. Com `edit`, uma por
    /// trilha de legenda visível: a primeira com o nome do vídeo, as outras com o idioma (`.en.srt`) ou o número da trilha.
    #[serde(default)]
    pub sidecar_srt: bool,
    /// Reduz o vídeo a esta altura (largura proporcional e par), depois das legendas queimadas; ignorado se o projeto já for menor. Não vale em png_sequence.
    #[serde(default)]
    pub height: Option<u32>,
    /// Velocidade do x264/x265: ultrafast, veryfast, faster, fast, medium, slow (padrão: slow no h264, medium no h265). Mais rápido = arquivo maior.
    #[serde(default)]
    pub speed: Option<String>,
}

/// Arquivo temporário apagado ao sair do escopo.
struct TempFile(PathBuf);

impl TempFile {
    fn new(ext: &str) -> TempFile {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        TempFile(std::env::temp_dir().join(format!("aiai-export-{}-{nanos}.{ext}", std::process::id())))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct RenderedFrame {
    image: aiai_paint::Image,
}

/// Cortes a exportar, na ordem, com o intervalo [início, fim) de cada um.
fn export_segments(session: &CoreSession, r: &ExportVideoRequest) -> Result<Vec<(Id, u32, u32)>, BridgeError> {
    let p = &session.project;
    match (&r.cut, &r.episode) {
        (Some(c), None) => {
            let dur = cut_data(p, c).map_err(tool_error)?.duration;
            let (s, e) = (r.start.unwrap_or(0), r.end.unwrap_or(dur));
            if s >= e || e > dur {
                return Err(BridgeError::invalid_params(format!("intervalo [{s}, {e}) inválido: o corte tem {dur} quadros")));
            }
            Ok(vec![(c.clone(), s, e)])
        }
        (None, Some(ep)) => {
            if r.start.is_some() || r.end.is_some() {
                return Err(BridgeError::invalid_params("'start'/'end' só valem com 'cut'"));
            }
            let node = p.node(ep).ok_or_else(|| BridgeError::invalid_params(format!("não encontrado: {ep}")))?;
            if !matches!(node.data, NodeKind::Episode(_)) {
                return Err(BridgeError::invalid_params(format!("{ep} é {}, não episódio", node.data.type_name())));
            }
            let mut out = Vec::new();
            for sc in &node.children {
                let Some(scene) = p.node(sc) else { continue };
                if !matches!(scene.data, NodeKind::Scene(_)) {
                    continue;
                }
                for c in &scene.children {
                    if let Some(NodeKind::Cut(cd)) = p.node(c).map(|n| &n.data) {
                        out.push((c.clone(), 0, cd.duration));
                    }
                }
            }
            if out.is_empty() {
                return Err(BridgeError::invalid_params(format!("o episódio {ep} não tem cortes")));
            }
            Ok(out)
        }
        _ => Err(BridgeError::invalid_params("EXPORT_VIDEO exige 'cut' OU 'episode'")),
    }
}

/// Mixagem dos segmentos em sequência (mesma taxa do projeto); `None` se nenhum corte tem áudio.
fn export_audio(session: &CoreSession, segs: &[(Id, u32, u32)]) -> Result<Option<AudioBuf>, BridgeError> {
    let p = &session.project;
    let any = segs.iter().any(|(c, _, _)| cut_data(p, c).map(|d| !d.audio.is_empty()).unwrap_or(false));
    if !any {
        return Ok(None);
    }
    let fps = p.settings.fps();
    let rate = p.settings.audio_sample_rate.max(8000);
    let settings = MixSettings { rate, ..Default::default() };
    let total_frames: u64 = segs.iter().map(|(_, s, e)| (e - s) as u64).sum();
    let at = |frame: u64| (frame as f64 / fps * rate as f64).round() as usize;
    let mut out = AudioBuf::zeros(rate, 2, at(total_frames));
    let mut cursor = 0u64;
    for (cut, s, e) in segs {
        let dur = cut_data(p, cut).map_err(tool_error)?.duration;
        let mix = audio::mix_cut_with(p, &session.blobs, cut, Some(dur), &settings).map_err(tool_error)?.to_stereo();
        let (a, b) = (at(*s as u64), at(*e as u64));
        out.mix_in(&mix.slice(a, b - a), at(cursor), 1.0);
        cursor += (e - s) as u64;
    }
    Ok(Some(out))
}

pub(crate) fn export_video(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let started = Instant::now();
    let r: ExportVideoRequest = request(params, "EXPORT_VIDEO")?;
    let out_path = check_abs(&r.out, "out")?;
    let name = out_path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
    match r.codec {
        VideoCodec::H264 | VideoCodec::H264Nvenc | VideoCodec::H265 => {
            if !(name.ends_with(".mp4") || name.ends_with(".mkv") || name.ends_with(".mov")) {
                return Err(BridgeError::invalid_params("'out' deve terminar em .mp4 (ou .mkv/.mov) para h264/h265"));
            }
        }
        VideoCodec::Vp9 => {
            if !(name.ends_with(".webm") || name.ends_with(".mkv")) {
                return Err(BridgeError::invalid_params("'out' deve terminar em .webm (ou .mkv) para vp9"));
            }
        }
        VideoCodec::Prores | VideoCodec::Prores4444 => {
            if !name.ends_with(".mov") {
                return Err(BridgeError::invalid_params("'out' deve terminar em .mov para prores"));
            }
        }
        VideoCodec::PngSequence => {
            if !name.ends_with(".png") || !name.contains("%0") {
                return Err(BridgeError::invalid_params("em png_sequence 'out' deve ser um padrão como C:\\...\\q_%05d.png"));
            }
        }
    }
    if r.crf.is_some_and(|c| c > 63 || (c > 51 && r.codec != VideoCodec::Vp9)) {
        return Err(BridgeError::invalid_params("'crf' deve ser 0..51 (0..63 em vp9)"));
    }
    if let Some(sp) = &r.speed {
        if !["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower"].contains(&sp.as_str()) {
            return Err(BridgeError::invalid_params(format!("'speed' desconhecido: {sp} (ultrafast, veryfast, faster, fast, medium, slow)")));
        }
    }
    // o codificador precisa existir neste ffmpeg: o erro diz qual falta (H.265/WebM dependem da build)
    r.codec.availability().map_err(|e| BridgeError::failed(format!("{:?}: {e}", r.codec)))?;
    let ctl = crate::export_ctl::current();
    let bg = parse_hex_rgb(r.background.as_deref().unwrap_or("#ffffff"), "background")?;
    let edit_doc = match (&r.edit, &r.cut, &r.episode) {
        (Some(id), None, None) => {
            if r.start.is_some() || r.end.is_some() || r.srt.is_some() || !r.subtitles.is_empty() {
                return Err(BridgeError::invalid_params("com 'edit' não use start/end/srt/subtitles: a edição já tem o trecho e as legendas"));
            }
            Some(load_edit(&session.project, id).map_err(tool_error)?)
        }
        (Some(_), _, _) => return Err(BridgeError::invalid_params("EXPORT_VIDEO: informe só um entre 'cut', 'episode' e 'edit'")),
        _ => None,
    };
    let (segs, total) = match &edit_doc {
        Some(doc) => (Vec::new(), doc.timeline(&session.project, session.project.settings.fps() as f32).map_err(tool_error)?.total as u64),
        None => {
            let segs = export_segments(session, &r)?;
            let total: u64 = segs.iter().map(|(_, s, e)| (e - s) as u64).sum();
            (segs, total)
        }
    };
    if total > 200_000 {
        return Err(BridgeError::invalid_params(format!("{total} quadros é demais para uma chamada (máx. 200000)")));
    }
    if let Some(c) = &ctl {
        c.begin(total, crate::export_ctl::Phase::Audio);
        c.checkpoint().map_err(|_| BridgeError::failed(crate::export_ctl::CANCELED))?;
    }
    let settings = session.project.settings.clone();
    let want_audio = r.audio.unwrap_or(true) && r.codec != VideoCodec::PngSequence;

    // áudio: mixa, normaliza, mede e grava um WAV temporário para o muxer
    let mut audio_report = Value::Null;
    let wav_tmp = TempFile::new("wav");
    let mut audio_path = None;
    if want_audio {
        let mixed = match &edit_doc {
            Some(doc) => {
                let inter = mix_edit(&session.project, &session.blobs, doc).map_err(tool_error)?;
                let (mut l, mut rr) = (Vec::with_capacity(inter.len() / 2), Vec::with_capacity(inter.len() / 2));
                for fr in inter.chunks_exact(2) {
                    l.push(fr[0]);
                    rr.push(fr[1]);
                }
                let buf = AudioBuf::stereo(settings.audio_sample_rate.max(8000), l, rr);
                if buf.peak() > 0.0 { Some(buf) } else { None }
            }
            None => export_audio(session, &segs)?,
        };
        if let Some(mut buf) = mixed {
            let mut rep = Map::new();
            if let Some(preset) = r.preset {
                rep.insert("normalize".into(), to_json(&audio::normalize_mix(&mut buf, preset))?);
            }
            rep.insert("loudness".into(), loudness_json(&audio::loudness_report(&buf), false)?);
            audio::export_wav(&buf, &wav_tmp.0, WavBits::Pcm16).map_err(tool_error)?;
            audio_path = Some(wav_tmp.0.clone());
            audio_report = Value::Object(rep);
        }
    }

    // legendas queimadas
    let srt_tmp = TempFile::new("srt");
    let mut srt_path = None;
    if !r.subtitles.is_empty() {
        std::fs::write(&srt_tmp.0, aiai_media::write_srt(&r.subtitles)).map_err(|e| BridgeError::failed(format!("gravando SRT temporário: {e}")))?;
        srt_path = Some(srt_tmp.0.clone());
    } else if let Some(s) = &r.srt {
        let p = PathBuf::from(s);
        if !p.is_file() {
            return Err(BridgeError::invalid_params(format!("'srt' não encontrado: {s}")));
        }
        srt_path = Some(p);
    }
    let burned = srt_path.is_some();

    ensure_parent(&out_path)?;
    let mut encoder: Option<Encoder> = None;
    let mut size = (0u32, 0u32);
    let mut warnings: BTreeMap<String, u32> = BTreeMap::new();
    // com alfa (ProRes 4444) o render vai sem fundo: o que é transparente no corte sai transparente no vídeo
    let opts = RenderOptions { camera: true, background: if r.codec.has_alpha() { None } else { Some(bg) }, ..Default::default() };
    // lista de (corte, quadro); None = quadro da edição
    let jobs: Vec<(Option<&Id>, u32)> = match &edit_doc {
        Some(_) => (0..total as u32).map(|f| (None, f)).collect(),
        None => segs.iter().flat_map(|(c, s, e)| (*s..*e).map(move |f| (Some(c), f))).collect(),
    };
    if let Some(c) = &ctl {
        c.set_phase(crate::export_ctl::Phase::Frames);
    }
    let tuning = aiai_media::EncodeTuning { scale_height: r.height.filter(|_| r.codec != VideoCodec::PngSequence), speed: r.speed.clone() };
    {
        for (cut, f) in jobs {
            // pausa e cancelamento entre os quadros (o ffmpeg é morto e o arquivo parcial apagado)
            if let Some(c) = &ctl {
                if c.checkpoint().is_err() {
                    if let Some(e) = encoder.take() {
                        e.abort();
                    }
                    return Err(BridgeError::failed(crate::export_ctl::CANCELED));
                }
            }
            let (image, frame_warnings) = match (cut, &edit_doc) {
                (Some(cut), _) => {
                    let o = render_frame(&session.project, &session.blobs, cut, f, &opts).map_err(|er| BridgeError::invalid_params(format!("render do corte {cut} quadro {f}: {er}")))?;
                    (o.image, o.warnings)
                }
                (None, Some(doc)) => (render_edit_frame(&session.project, &session.blobs, doc, f, true).map_err(tool_error)?, Vec::new()),
                (None, None) => unreachable!("sem corte nem edição"),
            };
            let rendered = RenderedFrame { image };
            for w in frame_warnings {
                *warnings.entry(w).or_insert(0) += 1;
            }
            if encoder.is_none() {
                size = (rendered.image.width, rendered.image.height);
                let eo = EncodeOptions {
                    width: size.0,
                    height: size.1,
                    fps_num: settings.fps_num,
                    fps_den: settings.fps_den,
                    codec: r.codec,
                    crf: r.crf.unwrap_or(18),
                    audio: audio_path.clone(),
                    burn_subtitles: srt_path.clone(),
                    background: bg,
                };
                encoder = Some(Encoder::start_tuned(&out_path, &eo, &tuning).map_err(BridgeError::failed)?);
            }
            encoder.as_mut().expect("iniciado").push(&rendered.image).map_err(BridgeError::failed)?;
            if let Some(c) = &ctl {
                c.frame_done();
            }
        }
    }
    if let Some(c) = &ctl {
        c.set_phase(crate::export_ctl::Phase::Finishing);
    }
    let report = encoder.ok_or_else(|| BridgeError::failed("nenhum quadro renderizado"))?.finish().map_err(BridgeError::failed)?;
    // legenda de entrega ao lado do vídeo
    let mut sidecar = Value::Null;
    let mut sidecars: Vec<Value> = Vec::new();
    if r.sidecar_srt && r.codec != VideoCodec::PngSequence {
        // (sufixo, texto): a edição grava uma por trilha de legenda (a primeira com o nome do vídeo)
        let texts: Vec<(String, String)> = match &edit_doc {
            Some(doc) => aiai_tools::edit::sidecar_srts(&session.project, doc)
                .into_iter()
                .enumerate()
                .map(|(k, (track, lang, t))| (if k == 0 { String::new() } else if lang.is_empty() { format!(".{}", doc.track_label(track)) } else { format!(".{lang}") }, t))
                .collect(),
            None if !r.subtitles.is_empty() => vec![(String::new(), aiai_media::write_srt(&r.subtitles))],
            None => r.srt.as_ref().and_then(|s| std::fs::read_to_string(s).ok()).map(|t| vec![(String::new(), t)]).unwrap_or_default(),
        };
        for (suffix, t) in texts.into_iter().filter(|(_, t)| !t.trim().is_empty()) {
            let stem = out_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let srt = out_path.with_file_name(format!("{stem}{suffix}.srt"));
            std::fs::write(&srt, t.as_bytes()).map_err(|e| BridgeError::failed(format!("gravar {}: {e}", srt.display())))?;
            if sidecar.is_null() {
                sidecar = json!(srt.display().to_string());
            }
            sidecars.push(json!(srt.display().to_string()));
        }
        if sidecar.is_null() {
            sidecar = json!("sem legendas para gravar");
        }
    }
    let probe = if r.codec == VideoCodec::PngSequence { Value::Null } else { to_json(&aiai_media::probe(&out_path).map_err(|e| BridgeError::failed(format!("ffprobe: {e}")))?)? };
    let warn_list: Vec<Value> = warnings.iter().take(20).map(|(w, n)| json!({ "warning": w, "frames": n })).collect();
    Ok(json!({
        "path": report.path.display().to_string(),
        "frames": report.frames,
        "expected_frames": total,
        "fps": [settings.fps_num, settings.fps_den],
        "size": [size.0, size.1],
        "codec": r.codec,
        "segments": match (&r.edit, &edit_doc) {
            (Some(id), Some(_)) => json!([{ "edit": id, "frames": total }]),
            _ => json!(segs.iter().map(|(c, s, e)| json!({ "cut": c, "start": s, "end": e })).collect::<Vec<_>>()),
        },
        "audio": audio_report,
        "subtitles_burned": burned,
        "sidecar_srt": sidecar,
        "sidecar_srts": sidecars,
        "probe": probe,
        "warnings": warn_list,
        "elapsed_s": started.elapsed().as_secs_f64(),
    }))
}
