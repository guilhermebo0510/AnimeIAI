//! Edição e análise de áudio (S24): `AUDIO_EDIT` aplica passos de edição e efeitos (aparar, apagar, fade, EQ, compressor,
//! limitador, redução de ruído, reverb, mudar altura/tempo, rack...) a um asset de áudio e guarda o resultado como NOVO
//! asset (a fonte nunca é sobrescrita; `params.edited_from` e `params.steps` registram a origem), num passo de undo;
//! `AUDIO_ANALYZE` devolve medidores (LUFS, LRA, true peak...) e resumos de forma de onda e espectrograma, sem mudar o
//! documento. O motor está em `aiai_audio::{edit, fx, steps, analysis, meters}`; a lógica de documento em
//! `aiai_tools::audio_edit`.

use crate::core_tools::{apply, request, tool_error};
use crate::production_tools::check_abs;
use crate::protocol::BridgeError;
use aiai_audio::AudioBuf;
use aiai_core::CoreSession;
use aiai_tools::audio::{self};
use aiai_tools::audio_edit::{self, AudioAnalyzeRequest, AudioEditRequest};
use serde_json::{Value, json};

pub const AUDIO_ACTIONS: &[&str] = &["AUDIO_EDIT", "AUDIO_ANALYZE"];

pub fn is_mutating(action: &str) -> bool {
    action == "AUDIO_EDIT"
}

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "AUDIO_EDIT" => audio_edit_action(session, params),
        "AUDIO_ANALYZE" => audio_analyze_action(session, params),
        _ => Err(BridgeError::unknown_action(action)),
    }
}

fn audio_edit_action(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: AudioEditRequest = request(params, "AUDIO_EDIT")?;
    let out = audio_edit::audio_edit(&session.project, &mut session.ids, &mut session.blobs, &r).map_err(tool_error)?;
    let cmd = out.clip.batch("Editar áudio");
    let mut v = apply(session, params, cmd)?;
    if let Some(o) = v.as_object_mut() {
        o.insert("asset".into(), json!(out.clip.asset));
        o.insert("placement".into(), json!(out.clip.placement));
        o.insert("duration_s".into(), json!(out.clip.duration_s));
        o.insert("duration_frames".into(), json!(out.clip.duration_frames));
        o.insert("source".into(), json!(out.source));
        o.insert("source_name".into(), json!(out.source_name));
        o.insert("source_duration_s".into(), json!(out.source_duration_s));
        o.insert("sample_rate".into(), json!(out.rate));
        o.insert("channels".into(), json!(out.channels));
        o.insert("steps_applied".into(), json!(out.steps_applied));
    }
    Ok(v)
}

fn audio_analyze_action(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: AudioAnalyzeRequest = request(params, "AUDIO_ANALYZE")?;
    let (buf, key): (AudioBuf, Option<String>) = match (&r.asset, &r.cut, &r.path) {
        (Some(a), None, None) => {
            let buf = audio::asset_audio(&session.project, &session.blobs, a).map_err(tool_error)?;
            (buf, audio_edit::asset_blob_key(&session.project, a))
        }
        (None, Some(c), None) => (audio::mix_cut(&session.project, &session.blobs, c, None).map_err(tool_error)?, None),
        (None, None, Some(path)) => {
            let p = check_abs(path, "path")?;
            let bytes = std::fs::read(&p).map_err(|e| BridgeError::invalid_params(format!("lendo {path}: {e}")))?;
            (aiai_audio::wav::read_wav(&bytes).map_err(|e| BridgeError::invalid_params(format!("{path}: {e}")))?, None)
        }
        _ => return Err(BridgeError::invalid_params("AUDIO_ANALYZE exige exatamente um de: 'asset' (clipe de áudio), 'cut' (mixagem do corte) ou 'path' (WAV)")),
    };
    audio_edit::audio_analyze(&buf, key.as_deref(), &r).map_err(tool_error)
}
