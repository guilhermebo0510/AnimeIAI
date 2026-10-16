//! Áudio (S17): fala por TTS local (VOICEVOX), lip-sync para o timesheet, SFX e BGM procedurais, mixagem do corte,
//! loudness e exportação. Cada operação que mexe no documento devolve **comandos** do núcleo (undo, log, WAL como as demais);
//! o WAV entra no `BlobStore` passado (inserir blob é idempotente e não é comando).

use crate::context::*;
use aiai_audio::bgm::{self, BgmEngine, Song};
use aiai_audio::lipsync::{self, LipsyncOptions, MouthMap, MouthShape, ShapeSet};
use aiai_audio::loudness::{self, LoudnessPreset, LoudnessReport, NormalizeReport};
use aiai_audio::mixer::{self, Clip, MixSettings, MixTrack};
use aiai_audio::sfx::{self, SfxSpec};
use aiai_audio::timing::Timings;
use aiai_audio::voicevox::{VoiceParams, VoicevoxClient};
use aiai_audio::wav::{self, WavFormat};
use aiai_audio::{AudioBuf, AudioError};
use aiai_core::command::CellEdit;
use aiai_core::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use aiai_audio::voicevox::DEFAULT_URL as VOICEVOX_URL;

impl From<AudioError> for ToolError {
    fn from(e: AudioError) -> Self {
        ToolError::Invalid(format!("áudio: {e}"))
    }
}

// ---------------------------------------------------------------- utilidades

fn frames_for(seconds: f64, fps: f64) -> u32 {
    (seconds * fps - 1e-6).ceil().max(1.0) as u32
}

fn mix_track(t: AudioTrack) -> MixTrack {
    match t {
        AudioTrack::Dialogue => MixTrack::Dialogue,
        AudioTrack::Sfx => MixTrack::Sfx,
        AudioTrack::Bgm => MixTrack::Bgm,
        AudioTrack::Ambience => MixTrack::Ambience,
    }
}

fn default_track_dialogue() -> AudioTrack {
    AudioTrack::Dialogue
}
fn default_track_sfx() -> AudioTrack {
    AudioTrack::Sfx
}
fn default_track_bgm() -> AudioTrack {
    AudioTrack::Bgm
}

/// Parâmetros de posicionamento no corte, comuns a fala/SFX/BGM.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct PlacementOpts {
    /// Corte onde colocar o clipe; omitido = só cria o asset.
    #[serde(default)]
    pub cut: Option<Id>,
    /// Quadro (0-based) de início no corte.
    #[serde(default)]
    pub start_frame: Option<u32>,
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub fade_in: f32,
    #[serde(default)]
    pub fade_out: f32,
}

/// Resultado comum de operações que criam um clipe de áudio.
#[derive(Debug, Clone)]
pub struct ClipOutput {
    pub commands: Vec<Command>,
    pub asset: Id,
    pub placement: Option<Id>,
    pub duration_s: f64,
    /// Duração arredondada para cima em quadros do projeto.
    pub duration_frames: u32,
}

impl ClipOutput {
    /// Os comandos como um passo único de undo.
    pub fn batch(&self, label: &str) -> Command {
        Command::Batch { commands: self.commands.clone(), label: Some(label.to_string()) }
    }
}

fn create_asset_command(idgen: &mut IdGen, name: &str, blob: BlobHash, params: Value, source: &str) -> (Id, Command) {
    let id = idgen.next();
    let cmd = Command::CreateNode {
        id: Some(id.clone()),
        parent: None,
        index: None,
        name: name.to_string(),
        data: NodeKind::Asset(AssetData { asset: AssetType::AudioClip, blob: Some(blob), params, source: source.to_string() }),
    };
    (id, cmd)
}

/// Comando que acrescenta um clipe à lista de áudio do corte (merge patch substitui a lista inteira).
fn append_placement(p: &Project, cut: &Id, placement: AudioPlacement) -> ToolResult<Command> {
    let cd = cut_data(p, cut)?;
    if let Some(start) = Some(placement.start_frame) {
        if start >= cd.duration {
            return Err(ToolError::Invalid(format!("start_frame {start} fora do corte (duração {})", cd.duration)));
        }
    }
    let mut list = cd.audio.clone();
    list.push(placement);
    Ok(Command::SetProps { id: cut.clone(), name: None, data: json!({ "audio": list }) })
}

#[allow(clippy::too_many_arguments)]
fn finish_clip(
    p: &Project,
    idgen: &mut IdGen,
    blobs: &mut BlobStore,
    wav_bytes: Vec<u8>,
    duration_s: f64,
    name: &str,
    params: Value,
    source: &str,
    track: AudioTrack,
    opts: &PlacementOpts,
    character: Option<Id>,
) -> ToolResult<ClipOutput> {
    let fps = p.settings.fps();
    let blob = blobs.put(wav_bytes);
    let (asset, create) = create_asset_command(idgen, name, blob, params, source);
    let mut commands = vec![create];
    let mut placement_id = None;
    if let Some(cut) = &opts.cut {
        let pid = idgen.next();
        let pl = AudioPlacement {
            id: pid.clone(),
            asset: asset.clone(),
            track,
            start_frame: opts.start_frame.unwrap_or(0),
            trim_start: 0.0,
            duration: None,
            gain_db: opts.gain_db,
            pan: opts.pan,
            fade_in: opts.fade_in,
            fade_out: opts.fade_out,
            character,
        };
        commands.push(append_placement(p, cut, pl)?);
        placement_id = Some(pid);
    }
    Ok(ClipOutput { commands, asset, placement: placement_id, duration_s, duration_frames: frames_for(duration_s, fps) })
}

/// Decodifica o WAV de um asset de áudio.
pub fn asset_audio(p: &Project, blobs: &BlobStore, asset: &Id) -> ToolResult<AudioBuf> {
    match &node(p, asset)?.data {
        NodeKind::Asset(a) if a.asset == AssetType::AudioClip => {
            let h = a.blob.as_ref().ok_or_else(|| ToolError::Invalid(format!("asset {asset} sem blob")))?;
            let bytes = blobs.get(h).ok_or_else(|| ToolError::NotFound(format!("blob {h}")))?;
            Ok(wav::read_wav(&bytes)?)
        }
        other => Err(ToolError::Invalid(format!("{asset} é {}, não clipe de áudio", other.type_name()))),
    }
}

fn asset_params(p: &Project, asset: &Id) -> ToolResult<Value> {
    match &node(p, asset)?.data {
        NodeKind::Asset(a) => Ok(a.params.clone()),
        other => Err(ToolError::Invalid(format!("{asset} é {}, não asset", other.type_name()))),
    }
}

// ---------------------------------------------------------------- perfil de voz

/// Perfil de voz de um personagem (guardado em `CharacterData.voice`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct VoiceProfile {
    /// Motor: hoje só "voicevox".
    #[serde(default = "default_engine")]
    pub engine: String,
    /// Id do estilo no VOICEVOX (ex.: 8 = 春日部つむぎ ノーマル).
    pub speaker: u32,
    /// Nome do personagem de voz (crédito: "VOICEVOX:<nome>").
    #[serde(default)]
    pub speaker_name: String,
    #[serde(default)]
    pub params: VoiceParams,
}

fn default_engine() -> String {
    "voicevox".into()
}

/// Grava o perfil de voz no personagem.
pub fn voice_profile_command(p: &Project, character: &Id, profile: &VoiceProfile) -> ToolResult<Command> {
    match &node(p, character)?.data {
        NodeKind::Character(_) => Ok(Command::SetProps { id: character.clone(), name: None, data: json!({ "voice": profile }) }),
        other => Err(ToolError::Invalid(format!("{character} é {}, não personagem", other.type_name()))),
    }
}

fn profile_of(p: &Project, character: &Id) -> ToolResult<Option<VoiceProfile>> {
    match &node(p, character)?.data {
        NodeKind::Character(c) if !c.voice.is_null() => Ok(Some(serde_json::from_value(c.voice.clone()).map_err(|e| ToolError::Invalid(format!("perfil de voz inválido: {e}")))?)),
        NodeKind::Character(_) => Ok(None),
        other => Err(ToolError::Invalid(format!("{character} é {}, não personagem", other.type_name()))),
    }
}

// ---------------------------------------------------------------- fala (TTS)

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TtsLineRequest {
    pub text: String,
    /// Leitura em kana com acento (`オハヨオ'`), quando o texto tem kanji mal lido.
    #[serde(default)]
    pub kana: Option<String>,
    /// Personagem que fala; sem `speaker`, usa o perfil de voz dele.
    #[serde(default)]
    pub character: Option<Id>,
    /// Estilo do VOICEVOX; omitido = perfil do personagem.
    #[serde(default)]
    pub speaker: Option<u32>,
    /// Sobrepõe os ajustes do perfil.
    #[serde(default)]
    pub params: Option<VoiceParams>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_track_dialogue")]
    pub track: AudioTrack,
    #[serde(flatten)]
    pub place: PlacementOpts,
}

#[derive(Debug, Clone)]
pub struct TtsLineOutput {
    pub clip: ClipOutput,
    pub timings: Timings,
    /// Crédito exigido pela licença da voz (vai nos créditos do vídeo).
    pub credit: String,
}

fn speaker_label(client: &VoicevoxClient, speaker: u32) -> Result<(String, String), ToolError> {
    let list = client.speakers()?;
    for s in list {
        if let Some(st) = s.styles.iter().find(|st| st.id == speaker) {
            return Ok((s.name.clone(), st.name.clone()));
        }
    }
    Err(ToolError::NotFound(format!("estilo VOICEVOX {speaker}")))
}

fn truncate_name(s: &str) -> String {
    let t: String = s.chars().take(24).collect();
    if s.chars().count() > 24 { format!("{t}…") } else { t }
}

/// Sintetiza a fala, guarda o WAV, cria o asset (texto, ajustes, consulta de áudio e tempos em `params`) e, se houver corte,
/// acrescenta o clipe à trilha.
pub fn tts_line(p: &Project, idgen: &mut IdGen, blobs: &mut BlobStore, voicevox_url: &str, req: &TtsLineRequest) -> ToolResult<TtsLineOutput> {
    let client = VoicevoxClient::new(voicevox_url);
    let profile = match &req.character {
        Some(c) => profile_of(p, c)?,
        None => None,
    };
    let speaker = req.speaker.or(profile.as_ref().map(|v| v.speaker)).ok_or_else(|| ToolError::Invalid("informe `speaker` ou um personagem com perfil de voz".into()))?;
    let mut params = profile.as_ref().map(|v| v.params.clone()).unwrap_or_default();
    if let Some(o) = &req.params {
        merge_params(&mut params, o);
    }
    let (voice_name, style_name) = speaker_label(&client, speaker)?;
    let line = client.synthesize_line(&req.text, req.kana.as_deref(), speaker, &params)?;
    let audio = wav::read_wav(&line.wav)?;
    let credit = format!("VOICEVOX:{voice_name}");
    let pparams = json!({
        "engine": "voicevox",
        "engine_version": client.version().ok(),
        "speaker": speaker,
        "speaker_name": voice_name,
        "style": style_name,
        "text": req.text,
        "kana": req.kana,
        "voice_params": params,
        "audio_query": line.query,
        "timings": line.timings,
        "sample_rate": audio.rate,
        "credit": credit,
    });
    let name = req.name.clone().unwrap_or_else(|| format!("Fala: {}", truncate_name(&req.text)));
    let clip = finish_clip(p, idgen, blobs, line.wav, audio.duration_s(), &name, pparams, &credit, req.track, &req.place, req.character.clone())?;
    Ok(TtsLineOutput { clip, timings: line.timings, credit })
}

fn merge_params(base: &mut VoiceParams, over: &VoiceParams) {
    macro_rules! m {
        ($f:ident) => {
            if over.$f.is_some() {
                base.$f = over.$f;
            }
        };
    }
    m!(speed_scale);
    m!(pitch_scale);
    m!(intonation_scale);
    m!(volume_scale);
    m!(pre_phoneme_length);
    m!(post_phoneme_length);
    m!(pause_length);
    m!(pause_length_scale);
    if !over.mora_edits.is_empty() {
        base.mora_edits = over.mora_edits.clone();
    }
}

/// Regera o WAV de um clipe de fala a partir dos parâmetros guardados (`audio_query` + `speaker`): mesma consulta, mesmo áudio.
pub fn tts_regenerate(p: &Project, blobs: &mut BlobStore, voicevox_url: &str, asset: &Id) -> ToolResult<Command> {
    let params = asset_params(p, asset)?;
    let speaker = params.get("speaker").and_then(|v| v.as_u64()).ok_or_else(|| ToolError::Invalid("asset sem `speaker`: não é fala do VOICEVOX".into()))? as u32;
    let q: aiai_audio::voicevox::AudioQuery = serde_json::from_value(params.get("audio_query").cloned().ok_or_else(|| ToolError::Invalid("asset sem `audio_query`".into()))?).map_err(|e| ToolError::Invalid(e.to_string()))?;
    let line = VoicevoxClient::new(voicevox_url).synthesize_query(&q, speaker)?;
    let h = blobs.put(line.wav);
    Ok(Command::SetProps { id: asset.clone(), name: None, data: json!({ "blob": h, "params": { "timings": line.timings } }) })
}

// ---------------------------------------------------------------- fala Kokoro (PT-BR/EN)

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KokoroLineRequest {
    pub text: String,
    #[serde(default)]
    pub kokoro: aiai_audio::kokoro::KokoroParams,
    #[serde(default)]
    pub character: Option<Id>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_track_dialogue")]
    pub track: AudioTrack,
    #[serde(flatten)]
    pub place: PlacementOpts,
}

/// Fala em PT-BR/EN pelo Kokoro-82M (processo `uv run --script tools/kokoro/kokoro_tts.py`). Sem moras: o lip-sync usa energia.
/// Crédito: "Kokoro-82M (Apache-2.0)" — a licença não exige crédito em vídeo, só preservar o aviso nas distribuições do modelo.
pub fn tts_kokoro_line(p: &Project, idgen: &mut IdGen, blobs: &mut BlobStore, req: &KokoroLineRequest) -> ToolResult<ClipOutput> {
    let script = aiai_audio::kokoro::find_script().ok_or_else(|| ToolError::NotFound("tools/kokoro/kokoro_tts.py".into()))?;
    if !aiai_audio::kokoro::models_present(&script) {
        return Err(ToolError::NotFound("modelos do Kokoro em sidecars/kokoro (kokoro-v1.0.onnx e voices-v1.0.bin)".into()));
    }
    let bytes = aiai_audio::kokoro::synthesize(&script, &req.text, &req.kokoro)?;
    let audio = wav::read_wav(&bytes)?;
    let params = json!({
        "engine": "kokoro",
        "model": "Kokoro-82M v1.0 (ONNX)",
        "text": req.text,
        "kokoro": req.kokoro,
        "deterministic": false,
        "sample_rate": audio.rate,
    });
    let name = req.name.clone().unwrap_or_else(|| format!("Fala: {}", truncate_name(&req.text)));
    finish_clip(p, idgen, blobs, bytes, audio.duration_s(), &name, params, "Kokoro-82M (Apache-2.0)", req.track, &req.place, req.character.clone())
}

// ---------------------------------------------------------------- lip-sync

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum LipsyncMode {
    /// Moras se o clipe as tiver (fala do VOICEVOX), senão energia.
    #[default]
    Auto,
    Moras,
    Energy,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LipsyncRequest {
    pub cut: Id,
    /// Clipe posicionado (id em `cut.audio`); alternativa: `asset` + `start_frame`.
    #[serde(default)]
    pub placement: Option<Id>,
    #[serde(default)]
    pub asset: Option<Id>,
    #[serde(default)]
    pub start_frame: Option<u32>,
    /// Camada cujos cels são as bocas (um cel por forma).
    pub mouth_layer: Id,
    /// Forma → número do cel: chaves a, i, u, e, o, n, closed (ou rest), half, open.
    pub mapping: BTreeMap<String, u32>,
    #[serde(default)]
    pub mode: LipsyncMode,
    /// `three` = fechada/meia/aberta.
    #[serde(default)]
    pub shapes: ShapeSet,
    #[serde(default)]
    pub min_hold_frames: Option<u32>,
    #[serde(default)]
    pub close_gap_frames: Option<u32>,
    /// Adianta a boca em quadros (pode ser fracionário).
    #[serde(default)]
    pub lead_frames: Option<f64>,
    /// Nome da coluna se for preciso criar a trilha de boca.
    #[serde(default)]
    pub track_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LipsyncOutput {
    pub command: Command,
    pub track: Id,
    pub first_frame: u32,
    /// Forma por quadro, a partir de `first_frame`.
    pub frames: Vec<MouthShape>,
    /// Exposições gravadas (quadro absoluto, cel).
    pub exposures: Vec<(u32, u32)>,
    pub used_moras: bool,
}

pub fn lipsync(p: &Project, idgen: &mut IdGen, blobs: &BlobStore, req: &LipsyncRequest) -> ToolResult<LipsyncOutput> {
    let cut = cut_data(p, &req.cut)?;
    let pl = cut
        .audio
        .iter()
        .find(|a| match (&req.placement, &req.asset) {
            (Some(id), _) => &a.id == id,
            (None, Some(asset)) => &a.asset == asset && req.start_frame.is_none_or(|s| s == a.start_frame),
            (None, None) => false,
        })
        .ok_or_else(|| ToolError::NotFound("clipe de áudio no corte (use `placement`, ou `asset` + `start_frame`)".into()))?;
    let layer = layer_data(p, &req.mouth_layer)?;
    let _ = layer;
    let fps = p.settings.fps();
    let params = asset_params(p, &pl.asset)?;
    let timings: Option<Timings> = params.get("timings").and_then(|t| serde_json::from_value(t.clone()).ok());
    let use_moras = match req.mode {
        LipsyncMode::Moras => {
            if timings.is_none() {
                return Err(ToolError::Invalid("o clipe não tem moras (`params.timings`); use mode=energy".into()));
            }
            true
        }
        LipsyncMode::Energy => false,
        LipsyncMode::Auto => timings.is_some(),
    };
    let mut o = LipsyncOptions::new(fps);
    o.shapes = req.shapes;
    if let Some(v) = req.min_hold_frames {
        o.min_hold_frames = v;
    }
    if let Some(v) = req.close_gap_frames {
        o.close_gap_frames = v;
    }
    o.lead_s = req.lead_frames.unwrap_or(0.0) / fps;
    let audio = if use_moras { None } else { Some(asset_audio(p, blobs, &pl.asset)?) };
    let clip_len = match (&timings, &audio) {
        (Some(t), None) => t.duration,
        (_, Some(a)) => a.duration_s(),
        _ => 0.0,
    };
    let eff = (clip_len - pl.trim_start as f64).max(0.0).min(pl.duration.map_or(f64::MAX, |d| d as f64));
    if eff <= 0.0 {
        return Err(ToolError::Invalid("clipe vazio depois do corte (trim)".into()));
    }
    let n_frames = frames_for(eff, fps) as usize;
    let shapes = if use_moras {
        let mut t = timings.unwrap();
        let shift = pl.trim_start as f64;
        for m in &mut t.moras {
            m.start -= shift;
            m.vowel_start -= shift;
            m.end -= shift;
        }
        lipsync::frames_from_moras(&t, Some(n_frames), &o)
    } else {
        let a = audio.unwrap();
        let rate = a.rate as f64;
        let from = (pl.trim_start as f64 * rate) as usize;
        let sliced = a.slice(from, (eff * rate).ceil() as usize);
        let mut f = lipsync::frames_from_energy(&sliced, &o);
        f.resize(n_frames, MouthShape::Rest);
        f
    };
    // Não passar do fim do corte.
    let first = pl.start_frame;
    let room = cut.duration.saturating_sub(first) as usize;
    let shapes: Vec<MouthShape> = shapes.into_iter().take(room).collect();
    let map = MouthMap::from_pairs(&req.mapping);
    let mut exposures = lipsync::exposures(&shapes, first, &map, false).map_err(ToolError::Invalid)?;
    // Repouso no quadro seguinte ao fim da fala (se ainda couber no corte).
    let end_frame = first + shapes.len() as u32;
    if end_frame < cut.duration {
        let rest = map.cel(MouthShape::Rest).ok_or_else(|| ToolError::Invalid("mapping precisa de closed/rest".into()))?;
        if exposures.last().map(|e| e.1) != Some(rest) {
            exposures.push((end_frame, rest));
        }
    }
    // Trilha de boca da camada (cria se não existir).
    let mut cmds = Vec::new();
    let existing = cut.xsheet.track_for_layer(&req.mouth_layer);
    let track_id = match existing {
        Some(t) => t.id.clone(),
        None => {
            let tid = idgen.next();
            let name = req.track_name.clone().or_else(|| p.node(&req.mouth_layer).map(|n| n.name.clone())).unwrap_or_else(|| "BOCA".into());
            cmds.push(Command::AddTrack { cut: req.cut.clone(), index: None, track: Track::new(tid.clone(), &name, TrackKind::Mouth, Some(req.mouth_layer.clone())) });
            tid
        }
    };
    // Limpa entradas antigas dentro do intervalo que a nova sequência não reescreve.
    let new_frames: std::collections::BTreeSet<u32> = exposures.iter().map(|e| e.0).collect();
    let mut edits: Vec<CellEdit> = Vec::new();
    if let Some(t) = existing {
        let upper = end_frame.saturating_add(1);
        for (&f, _) in t.cells.range(first..upper.min(cut.duration)) {
            if !new_frames.contains(&f) {
                edits.push(CellEdit { frame: f, cell: None });
            }
        }
    }
    edits.extend(exposures.iter().map(|&(f, c)| CellEdit { frame: f, cell: Some(CellRef::Cel { number: c }) }));
    cmds.push(Command::SetCells { cut: req.cut.clone(), track: track_id.clone(), cells: edits });
    let label = format!("Lip-sync ({} quadros, {})", shapes.len(), if use_moras { "moras" } else { "energia" });
    Ok(LipsyncOutput { command: Command::Batch { commands: cmds, label: Some(label) }, track: track_id, first_frame: first, frames: shapes, exposures, used_moras: use_moras })
}

// ---------------------------------------------------------------- SFX

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SfxRequest {
    #[serde(flatten)]
    pub spec: SfxSpec,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_track_sfx")]
    pub track: AudioTrack,
    #[serde(flatten)]
    pub place: PlacementOpts,
}

pub fn sfx(p: &Project, idgen: &mut IdGen, blobs: &mut BlobStore, req: &SfxRequest) -> ToolResult<ClipOutput> {
    let audio = sfx::synth(&req.spec);
    let bytes = wav::write_wav(&audio, WavFormat::Pcm16)?;
    let preset = serde_json::to_value(req.spec.preset).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    let name = req.name.clone().unwrap_or_else(|| format!("SFX {preset}"));
    finish_clip(p, idgen, blobs, bytes, audio.duration_s(), &name, json!({ "generator": "sfx", "spec": req.spec }), "procedural: aiai-audio/sfx", req.track, &req.place, None)
}

// ---------------------------------------------------------------- BGM

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BgmRequest {
    pub song: Song,
    /// Caminho do .sf2; omitido = SoundFont padrão do projeto (sidecars/soundfonts) ou, se não houver, síntese interna.
    #[serde(default)]
    pub soundfont: Option<String>,
    /// Força o sintetizador interno (sem SoundFont).
    #[serde(default)]
    pub builtin_synth: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_track_bgm")]
    pub track: AudioTrack,
    #[serde(flatten)]
    pub place: PlacementOpts,
}

pub struct BgmOutput {
    pub clip: ClipOutput,
    pub engine: BgmEngine,
    pub soundfont: Option<String>,
}

pub fn bgm(p: &Project, idgen: &mut IdGen, blobs: &mut BlobStore, req: &BgmRequest) -> ToolResult<BgmOutput> {
    let sf_path = match (&req.soundfont, req.builtin_synth) {
        (_, true) => None,
        (Some(s), false) => Some(PathBuf::from(s)),
        (None, false) => bgm::default_soundfont_path(),
    };
    let sf = match &sf_path {
        Some(path) => Some(bgm::load_soundfont_cached(path)?),
        None => None,
    };
    let r = bgm::render_song(&req.song, sf.as_ref())?;
    let bytes = wav::write_wav(&r.audio, WavFormat::Pcm16)?;
    let (engine_name, source) = match (&r.engine, &sf_path) {
        (BgmEngine::SoundFont, Some(path)) => ("rustysynth".to_string(), format!("procedural: aiai-audio/bgm + rustysynth + {} (MIT, Frank Wen)", path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())),
        _ => ("builtin".to_string(), "procedural: aiai-audio/bgm (síntese interna)".to_string()),
    };
    let name = req.name.clone().unwrap_or_else(|| "BGM".into());
    let params = json!({ "generator": "bgm", "engine": engine_name, "song": req.song, "dropped_notes": r.dropped_notes });
    let clip = finish_clip(p, idgen, blobs, bytes, r.audio.duration_s(), &name, params, &source, req.track, &req.place, None)?;
    Ok(BgmOutput { clip, engine: r.engine, soundfont: sf_path.map(|p| p.display().to_string()) })
}

// ---------------------------------------------------------------- mixagem

/// Clipes do corte como entrada do mixer (decodifica cada asset uma vez).
pub fn cut_clips(p: &Project, blobs: &BlobStore, cut: &Id) -> ToolResult<Vec<Clip>> {
    let cd = cut_data(p, cut)?;
    let fps = p.settings.fps();
    let mut cache: BTreeMap<Id, Arc<AudioBuf>> = BTreeMap::new();
    let mut out = Vec::new();
    for a in &cd.audio {
        let audio = match cache.get(&a.asset) {
            Some(x) => x.clone(),
            None => {
                let x = Arc::new(asset_audio(p, blobs, &a.asset)?);
                cache.insert(a.asset.clone(), x.clone());
                x
            }
        };
        out.push(Clip {
            audio,
            track: mix_track(a.track),
            start_s: a.start_frame as f64 / fps,
            trim_start: a.trim_start,
            duration: a.duration,
            gain_db: a.gain_db,
            pan: a.pan,
            fade_in: a.fade_in,
            fade_out: a.fade_out,
        });
    }
    Ok(out)
}

/// Mixagem estéreo do corte com os ajustes dados. `frames` omitido/0 = duração do corte.
pub fn mix_cut_with(p: &Project, blobs: &BlobStore, cut: &Id, frames: Option<u32>, settings: &MixSettings) -> ToolResult<AudioBuf> {
    let cd = cut_data(p, cut)?;
    let n_frames = frames.filter(|&f| f > 0).unwrap_or(cd.duration);
    let samples = (n_frames as f64 / p.settings.fps() * settings.rate as f64).round() as usize;
    let clips = cut_clips(p, blobs, cut)?;
    Ok(mixer::render(&clips, samples, settings))
}

/// Mixagem com os ajustes padrão (taxa do projeto, ducking ligado).
pub fn mix_cut(p: &Project, blobs: &BlobStore, cut: &Id, frames: Option<u32>) -> ToolResult<AudioBuf> {
    let settings = MixSettings { rate: p.settings.audio_sample_rate.max(8000), ..Default::default() };
    mix_cut_with(p, blobs, cut, frames, &settings)
}

pub fn loudness_report(buf: &AudioBuf) -> LoudnessReport {
    loudness::measure(buf)
}

/// Leva a mixagem ao preset (ARIB -24 LKFS / EBU -23 / streaming -14) com teto de true peak de -1 dBTP.
pub fn normalize_mix(buf: &mut AudioBuf, preset: LoudnessPreset) -> NormalizeReport {
    loudness::normalize(buf, preset.target_lufs(), preset.true_peak_ceiling_db())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum WavBits {
    #[default]
    Pcm16,
    Pcm24,
    Float32,
}

pub fn wav_bytes(buf: &AudioBuf, bits: WavBits) -> ToolResult<Vec<u8>> {
    let f = match bits {
        WavBits::Pcm16 => WavFormat::Pcm16,
        WavBits::Pcm24 => WavFormat::Pcm24,
        WavBits::Float32 => WavFormat::Float32,
    };
    Ok(wav::write_wav(buf, f)?)
}

/// Grava o WAV em disco.
pub fn export_wav(buf: &AudioBuf, path: &Path, bits: WavBits) -> ToolResult<()> {
    let bytes = wav_bytes(buf, bits)?;
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| ToolError::Invalid(format!("criar {}: {e}", dir.display())))?;
        }
    }
    std::fs::write(path, bytes).map_err(|e| ToolError::Invalid(format!("gravar {}: {e}", path.display())))
}
