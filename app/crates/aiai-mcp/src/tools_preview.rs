//! Ferramentas curadas de prévia e entrega (S18): folha de contato de um corte, quadros da edição (legenda, transição,
//! créditos) sem exportar, mixagem da edição em WAV e regeneração de fala do VOICEVOX. Açúcar sobre
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
        "aiai_tts_regenerate" => with_req(&["asset"], "TTS_REGENERATE"),
        _ => return None,
    })
}
