//! Ferramentas curadas de prévia e entrega (S18): folha de contato de um corte, quadros da edição (legenda, transição,
//! créditos) sem exportar, mixagem da edição em WAV, regeneração de fala do VOICEVOX e, na edição v2 (S24), operações
//! de montagem (`aiai_edit_ops`) e legendas (`aiai_edit_captions`). Açúcar sobre
//! `aiai_bridge::preview_tools`; rodam no app e no headless.

use crate::context::ToolError;
use crate::tools::Plan;
use crate::tools_preprod::{object_schema, required};
use serde_json::{Map, Value, json};

const RENDER_SHEET_DESC: &str = r##"Folha de contato de um corte: vários quadros renderizados numa imagem só (número do quadro e tempo embaixo), para conferir animação, câmera, efeitos e timing de uma vez. Quadros: `frames` [lista] OU `start`/`end` (inclusivo)/`step` (padrão: ~12 quadros espalhados pelo corte); máx. 48. `columns` (4), `thumb_width` (480 px), `camera` (true; false = canvas inteiro). Grava o PNG em `out` — olhe a imagem.
Exemplo: {"cut":"<corte>","start":0,"end":71,"step":6,"out":"C:\AnimeIAI\producoes\curta\c01_folha.png"}"##;

const EDIT_PREVIEW_DESC: &str = r##"Prévia da EDIÇÃO sem exportar o vídeo: quadros globais da edição exatamente como o MP4 vai mostrar (legendas queimadas, transições, créditos). `frames`/`start`/`end`/`step` (quadros globais) ou `seconds` [instantes em s]; `burn_subtitles` (true). Use para conferir posição e quebra das legendas e as transições. Grava a folha em `out`.
Exemplo: {"edit":"<documento de edição>","seconds":[1.0,4.5,9.2],"out":"C:\AnimeIAI\producoes\curta\legendas.png"}"##;

const EDIT_MIX_DESC: &str = r##"Mixagem da edição inteira (falas, SFX e BGM de todos os cortes na ordem da edição) num WAV estéreo, com relatório de loudness (integrado, true peak, LRA). `preset` normaliza: arib (-24 LKFS, entrega de TV japonesa) | ebu (-23) | streaming (-14). Confira o som sem exportar o vídeo.
Exemplo: {"edit":"<documento de edição>","out":"C:\AnimeIAI\producoes\curta\mix.wav","preset":"arib"}"##;

const TTS_REGENERATE_DESC: &str = r##"Refaz o áudio de uma fala do VOICEVOX NO MESMO asset (o clipe continua no lugar da trilha), opcionalmente mudando a consulta: `audio_query_patch` com speedScale, pitchScale, intonationScale, volumeScale, prePhonemeLength, postPhonemeLength, pauseLengthScale ou accent_phrases (acento/moras). Devolve a nova duração — se mudou, refaça o lip-sync. Precisa do VOICEVOX local.
Exemplo: {"asset":"<asset da fala>","audio_query_patch":{"speedScale":0.92,"intonationScale":1.25}}"##;

const EDIT_OPS_DESC: &str = r##"Operações de montagem num documento de EDIÇÃO existente (aiai_edit), em ordem, tudo ou nada, um passo de desfazer. `ops`: [{op: razor, frame, items?} (navalha; sem items = tudo nas trilhas destravadas) | {op: delete, items, ripple?, linked? (true)} | {op: place, source: {type: cut, cut}|{type: audio, asset}|{type: image, asset}|{type: color, rgba}|{type: caption, text}, at, src_in?, dur, mode: insert|overwrite, video_track?, audio_track?} (corte = imagem + som vinculado) | {op: move, items, delta, track_shift?} (sobrescreve no destino; vinculados e legendas presas andam junto) | {op: trim, items, edge: in|out, delta, ripple?} | {op: roll, item, delta} (ponto de corte depois do item) | {op: slip, items, delta} | {op: slide, item, delta} | {op: transition, item, edge: in|out, transition: {kind, frames, align: center|start|end}|null} | {op: link|unlink, items} | {op: set_item, item, patch: {speed, opacity, volume, pan, filters, audio_fx, keys, enabled, source...}} | {op: add_track, kind: video|audio|subtitle, name?, lang?} | {op: set_track, track, patch: {locked, hidden, muted, solo, volume, lang, burn, style}} | {op: remove_track, track}]. Ids dos itens e trilhas vêm de `timeline` (resposta de aiai_edit/aiai_edit_ops). Devolve `created` (ids por op) e `timeline`.
Exemplo: {"edit":"<edição>","ops":[{"op":"razor","frame":48},{"op":"transition","item":3,"edge":"in","transition":{"kind":"dip_black","frames":12}},{"op":"delete","items":[5],"ripple":true}]}"##;

const EDIT_CAPTIONS_DESC: &str = r##"Legendas de uma trilha S da EDIÇÃO (`track`: id; padrão a primeira). `action`: export (`out` .srt ou .vtt) | import (`path` .srt/.vtt; `replace` apaga as antigas) | generate (das falas: cada clipe de fala dos cortes da edição vira legenda no tempo real do áudio, texto do roteiro — tradução quando o idioma `lang`/da trilha não é o original — presa ao clipe de imagem; `replace`) | check (cps, caracteres por linha, linhas e duração com `limits` {max_cps 17, max_chars_per_line 42, max_lines 2, min_seconds 0.8, max_seconds 7}). Import/generate entram no undo; para queimar ou não use aiai_edit_ops set_track {burn}. Exportação de vídeo com `sidecar_srt` grava um .srt por trilha.
Exemplo: {"edit":"<edição>","action":"generate","replace":true}"##;

pub fn definitions() -> Vec<Value> {
    use aiai_bridge::preview_tools as pv;
    let voicevox = json!({ "type": "string", "description": "URL do VOICEVOX Engine (só loopback). Padrão: env AIAI_VOICEVOX_URL ou http://127.0.0.1:50021." });
    let mut tts = object_schema::<pv::TtsRegenerateRequest>();
    if let Some(p) = tts.get_mut("properties").and_then(Value::as_object_mut) {
        p.insert("voicevox_url".into(), voicevox);
    }
    vec![
        json!({ "name": "aiai_render_sheet", "description": RENDER_SHEET_DESC, "inputSchema": object_schema::<pv::RenderSheetRequest>() }),
        json!({ "name": "aiai_edit_preview", "description": EDIT_PREVIEW_DESC, "inputSchema": object_schema::<pv::EditPreviewRequest>() }),
        json!({ "name": "aiai_edit_mix", "description": EDIT_MIX_DESC, "inputSchema": object_schema::<pv::EditMixRequest>() }),
        json!({ "name": "aiai_edit_ops", "description": EDIT_OPS_DESC, "inputSchema": object_schema::<pv::EditOpsRequest>() }),
        json!({ "name": "aiai_edit_captions", "description": EDIT_CAPTIONS_DESC, "inputSchema": object_schema::<pv::EditCaptionsRequest>() }),
        json!({ "name": "aiai_tts_regenerate", "description": TTS_REGENERATE_DESC, "inputSchema": tts }),
    ]
}

/// Plano de uma ferramenta de prévia/entrega; `None` se o nome não é daqui.
pub fn plan(name: &str, m: &Map<String, Value>) -> Option<Result<Plan, ToolError>> {
    let with_req = |keys: &[&str], action: &'static str| -> Result<Plan, ToolError> {
        required(m, keys)?;
        Ok(Plan { action, params: Value::Object(m.clone()), save_png: None })
    };
    Some(match name {
        "aiai_render_sheet" => with_req(&["cut", "out"], "RENDER_SHEET"),
        "aiai_edit_preview" => with_req(&["edit", "out"], "EDIT_PREVIEW"),
        "aiai_edit_mix" => with_req(&["edit", "out"], "EDIT_MIX"),
        "aiai_edit_ops" => with_req(&["edit", "ops"], "EDIT_OPS"),
        "aiai_edit_captions" => with_req(&["edit", "action"], "EDIT_CAPTIONS"),
        "aiai_tts_regenerate" => with_req(&["asset"], "TTS_REGENERATE"),
        _ => return None,
    })
}
