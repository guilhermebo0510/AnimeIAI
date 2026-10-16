//! Pré-produção (S16) sobre o documento: `SCRIPT`, `BEATS`, `STORYBOARD`, `ANIMATIC` e `CUTS_FROM_STORYBOARD`.
//!
//! Mesmo padrão de [`crate::production_tools`]: cada operação de `aiai_tools::preprod` devolve um `Command`, aplicado pela
//! `CoreSession` (autor `agent:claude`, `author`/`if_revision` opcionais), então entra no undo/redo e no log. As ações de
//! documento recebem `op` (ex.: `SCRIPT {op:"edit", doc, ops:[...]}`); as leituras (`read`, `timing`, `to_fountain`,
//! `panel_drawing`, `sheet`) não mudam o documento.
//!
//! Caminhos de gravação (`out` do animatic, da folha e do Fountain) devem ser absolutos e sem `..` (o `aiai-mcp` ainda
//! os confina às pastas permitidas).

use crate::core_ops::obj;
use crate::core_tools::{apply, request, tool_error};
use crate::protocol::BridgeError;
use aiai_core::{CoreSession, Id};
use aiai_tools::preprod::{
    self, AnimaticOptions, BeatBoard, BeatOp, CutsFromStoryboardRequest, PanelAddRequest, PanelPatch, SceneSpec, ScriptCreateRequest, ScriptOp, SheetOptions,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const PREPROD_ACTIONS: &[&str] = &["SCRIPT", "BEATS", "STORYBOARD", "ANIMATIC", "CUTS_FROM_STORYBOARD"];

/// Ações que podem mudar o documento (o headless salva o projeto depois delas; salvar sem mudança é inofensivo).
pub fn is_mutating(action: &str) -> bool {
    matches!(action, "SCRIPT" | "BEATS" | "STORYBOARD" | "CUTS_FROM_STORYBOARD")
}

// ------------------------------------------------------------------------------------------------- requisições

/// `SCRIPT`: roteiro (fala original + tradução, cenas e elementos, estimativa de tempo, Fountain).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ScriptRequest {
    /// Cria o documento de roteiro (raiz "Documentos").
    Create {
        name: String,
        #[serde(default)]
        title: String,
        /// Idioma original; omitido = `dialogue_language` do projeto.
        #[serde(default)]
        language: Option<String>,
        /// Idioma da tradução (legenda); omitido = `subtitle_language` do projeto.
        #[serde(default)]
        translation_language: Option<String>,
        #[serde(default)]
        scenes: Vec<SceneSpec>,
    },
    /// Devolve o roteiro inteiro (ids de cenas e elementos).
    Read { doc: Id },
    /// Edita por id (um passo de undo): set_meta, insert_scene, update_scene, delete_scene, move_scene, insert_element,
    /// update_element, delete_element, move_element.
    Edit { doc: Id, ops: Vec<ScriptOp> },
    /// Estimativa de tempo (moras/segundo por idioma) por elemento e cena, sem gravar.
    Timing { doc: Id },
    /// Grava a estimativa em `est_seconds` dos elementos de ação/fala que ainda não têm.
    ApplyTiming { doc: Id },
    /// Exporta em Fountain (texto; com `out`, grava o arquivo).
    ToFountain {
        doc: Id,
        #[serde(default)]
        out: Option<String>,
    },
    /// Cria um roteiro novo a partir de texto Fountain.
    FromFountain { name: String, fountain: String },
}

/// `BEATS`: quadro de beats (atos com cartões, segundos-alvo).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BeatsRequest {
    Create {
        name: String,
        #[serde(default)]
        board: Option<BeatBoard>,
    },
    Read { doc: Id },
    /// add_act, add_beat, update_beat, delete_beat, move_beat (um passo de undo).
    Edit { doc: Id, ops: Vec<BeatOp> },
}

/// `STORYBOARD`: storyboard/e-conte. Cada painel ganha um corte pequeno (canvas do storyboard) com a camada vetorial
/// "rascunho": desenhe nele com aiai_draw_curves/aiai_paint (cut/layer devolvidos por `panel_add` ou `panel_drawing`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StoryboardRequest {
    Create {
        name: String,
        #[serde(default)]
        title: String,
        /// Canvas dos painéis [w, h]; padrão [480, 270].
        #[serde(default)]
        canvas: Option<[u32; 2]>,
    },
    Read { storyboard: Id },
    PanelAdd(PanelAddRequest),
    PanelUpdate { storyboard: Id, panel: Id, patch: PanelPatch },
    PanelDelete { storyboard: Id, panel: Id },
    PanelMove { storyboard: Id, panel: Id, index: usize },
    /// Renumera os cortes na ordem atual (`step` 1 → 1,2,3; 5 → 5,10,15).
    Renumber {
        storyboard: Id,
        #[serde(default = "one")]
        step: u32,
    },
    /// Corte, camada "rascunho" e cel onde o painel é desenhado.
    PanelDrawing { storyboard: Id, panel: Id },
    /// Folha de e-conte em PNG (Nº | painel com quadro/seta de câmera | ação | fala/SE | segundos).
    Sheet {
        storyboard: Id,
        out: String,
        #[serde(default)]
        options: Option<SheetOptions>,
    },
}

fn one() -> u32 {
    1
}

/// `ANIMATIC`: MP4 (H.264, sem áudio) com cada painel pela duração dele.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AnimaticRequest {
    pub storyboard: Id,
    /// Destino .mp4 (pasta permitida).
    pub out: String,
    /// Padrão: tamanho do projeto.
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// fps (omitido = do projeto), captions (fala do painel como legenda), cut_numbers (Nº do corte no canto).
    #[serde(flatten)]
    pub options: AnimaticOptions,
}

// --------------------------------------------------------------------------------------------------- despacho

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "SCRIPT" => script(session, params),
        "BEATS" => beats(session, params),
        "STORYBOARD" => storyboard(session, params),
        "ANIMATIC" => animatic(session, params),
        "CUTS_FROM_STORYBOARD" => {
            let r: CutsFromStoryboardRequest = request(params, action)?;
            let out = preprod::cuts_from_storyboard(&session.project, &mut session.ids, &mut session.blobs, &r).map_err(tool_error)?;
            let cuts: Vec<Value> = out.cuts.iter().map(|(panel, cut)| json!({ "panel": panel, "cut": cut })).collect();
            apply_with(session, params, out.command, json!({ "scenes": out.scenes, "cuts": cuts }))
        }
        _ => Err(BridgeError::unknown_action(action)),
    }
}

fn script(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    match request::<ScriptRequest>(params, "SCRIPT")? {
        ScriptRequest::Create { name, title, language, translation_language, scenes } => {
            let req = ScriptCreateRequest { name, title, language, translation_language, scenes };
            let c = preprod::script_create(&session.project, &mut session.ids, req).map_err(tool_error)?;
            apply_with(session, params, c.command, json!({ "doc": c.id, "script": c.script }))
        }
        ScriptRequest::Read { doc } => {
            let s = preprod::script_read(&session.project, &doc).map_err(tool_error)?;
            let t = preprod::estimate_timing(&s);
            Ok(json!({ "doc": doc, "script": s, "estimated_seconds": t.total_seconds }))
        }
        ScriptRequest::Edit { doc, ops } => {
            let e = preprod::script_edit(&session.project, &mut session.ids, &doc, ops).map_err(tool_error)?;
            apply_with(session, params, e.command, json!({ "doc": doc, "created": e.created, "script": e.script }))
        }
        ScriptRequest::Timing { doc } => {
            let s = preprod::script_read(&session.project, &doc).map_err(tool_error)?;
            to_json(&preprod::estimate_timing(&s))
        }
        ScriptRequest::ApplyTiming { doc } => {
            let cmd = preprod::script_apply_timing(&session.project, &doc).map_err(tool_error)?;
            let mut v = apply(session, params, cmd)?;
            let s = preprod::script_read(&session.project, &doc).map_err(tool_error)?;
            v["timing"] = to_json(&preprod::estimate_timing(&s))?;
            Ok(v)
        }
        ScriptRequest::ToFountain { doc, out } => {
            let s = preprod::script_read(&session.project, &doc).map_err(tool_error)?;
            let text = preprod::script_to_fountain(&session.project, &s);
            match out {
                Some(path) => {
                    let p = check_abs(&path, "out")?;
                    write_file(&p, text.as_bytes())?;
                    Ok(json!({ "path": p.display().to_string(), "bytes": text.len() }))
                }
                None => Ok(json!({ "fountain": text })),
            }
        }
        ScriptRequest::FromFountain { name, fountain } => {
            let s = preprod::fountain_to_script(&session.project, &mut session.ids, &fountain);
            let c = preprod::script_create_from_doc(&mut session.ids, &name, s).map_err(tool_error)?;
            apply_with(session, params, c.command, json!({ "doc": c.id, "script": c.script }))
        }
    }
}

fn beats(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    match request::<BeatsRequest>(params, "BEATS")? {
        BeatsRequest::Create { name, board } => {
            let c = preprod::beats_create(&mut session.ids, &name, board.unwrap_or_default()).map_err(tool_error)?;
            apply_with(session, params, c.command, json!({ "doc": c.id }))
        }
        BeatsRequest::Read { doc } => {
            let b = preprod::beats_read(&session.project, &doc).map_err(tool_error)?;
            Ok(json!({ "doc": doc, "board": b, "total_target_seconds": b.total_target_seconds() }))
        }
        BeatsRequest::Edit { doc, ops } => {
            let (cmd, b) = preprod::beats_edit(&session.project, &mut session.ids, &doc, ops).map_err(tool_error)?;
            apply_with(session, params, cmd, json!({ "doc": doc, "board": b }))
        }
    }
}

fn storyboard(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let p = &session.project;
    match request::<StoryboardRequest>(params, "STORYBOARD")? {
        StoryboardRequest::Create { name, title, canvas } => {
            let c = preprod::storyboard_create(&mut session.ids, &name, &title, canvas).map_err(tool_error)?;
            apply_with(session, params, c.command, json!({ "storyboard": c.id }))
        }
        StoryboardRequest::Read { storyboard } => {
            let sb = preprod::storyboard_read(p, &storyboard).map_err(tool_error)?;
            Ok(json!({ "storyboard": storyboard, "total_seconds": sb.total_seconds(), "doc": sb }))
        }
        StoryboardRequest::PanelAdd(req) => {
            let a = preprod::storyboard_panel_add(p, &mut session.ids, &req).map_err(tool_error)?;
            let d = &a.drawing;
            let extra = json!({ "panel": a.panel, "drawing": { "cut": d.cut, "layer": d.layer, "cel": d.cel } });
            apply_with(session, params, a.command, extra)
        }
        StoryboardRequest::PanelUpdate { storyboard, panel, patch } => {
            let cmd = preprod::storyboard_panel_update(p, &storyboard, &panel, &patch).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        StoryboardRequest::PanelDelete { storyboard, panel } => {
            let cmd = preprod::storyboard_panel_delete(p, &storyboard, &panel).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        StoryboardRequest::PanelMove { storyboard, panel, index } => {
            let cmd = preprod::storyboard_panel_move(p, &storyboard, &panel, index).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        StoryboardRequest::Renumber { storyboard, step } => {
            let cmd = preprod::storyboard_renumber(p, &storyboard, step).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        StoryboardRequest::PanelDrawing { storyboard, panel } => {
            let d = preprod::panel_drawing(p, &storyboard, &panel).map_err(tool_error)?;
            Ok(json!({ "cut": d.cut, "layer": d.layer, "cel": d.cel }))
        }
        StoryboardRequest::Sheet { storyboard, out, options } => {
            let path = check_abs(&out, "out")?;
            let img = preprod::storyboard_export_sheet(p, &session.blobs, &storyboard, &options.unwrap_or_default()).map_err(tool_error)?;
            ensure_parent(&path)?;
            img.save_png(&path).map_err(|e| BridgeError::failed(format!("gravar {}: {e}", path.display())))?;
            Ok(json!({ "path": path.display().to_string(), "width": img.width, "height": img.height }))
        }
    }
}

fn animatic(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: AnimaticRequest = request(params, "ANIMATIC")?;
    let path = check_abs(&r.out, "out")?;
    ensure_parent(&path)?;
    let s = &session.project.settings;
    let (w, h) = (r.width.unwrap_or(s.width), r.height.unwrap_or(s.height));
    if w < 16 || h < 16 || w > 3840 || h > 2160 || w % 2 != 0 || h % 2 != 0 {
        return Err(BridgeError::invalid_params("ANIMATIC: width/height devem ser pares, entre 16 e 3840×2160"));
    }
    let rep = preprod::render_animatic_opts(&session.project, &session.blobs, &r.storyboard, &path, w, h, &r.options).map_err(tool_error)?;
    let panels: Vec<Value> = rep.panels.iter().map(|(id, start, dur)| json!({ "panel": id, "start": start, "frames": dur })).collect();
    Ok(json!({ "path": path.display().to_string(), "frames": rep.frames, "fps": rep.fps, "seconds": rep.seconds, "panels": panels }))
}

// --------------------------------------------------------------------------------------------------- utilidades

fn to_json<T: Serialize>(v: &T) -> Result<Value, BridgeError> {
    serde_json::to_value(v).map_err(|e| BridgeError::failed(e.to_string()))
}

fn apply_with(session: &mut CoreSession, params: &Value, cmd: aiai_core::Command, extra: Value) -> Result<Value, BridgeError> {
    obj(params)?;
    let mut v = apply(session, params, cmd)?;
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in e {
            o.insert(k.clone(), x.clone());
        }
    }
    Ok(v)
}

fn check_abs(path: &str, what: &str) -> Result<PathBuf, BridgeError> {
    let p = Path::new(path);
    if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(BridgeError::invalid_params(format!("'{what}' deve ser um caminho absoluto sem '..'")));
    }
    Ok(p.to_path_buf())
}

fn ensure_parent(p: &Path) -> Result<(), BridgeError> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| BridgeError::failed(format!("criar {}: {e}", dir.display())))?;
    }
    Ok(())
}

fn write_file(p: &Path, bytes: &[u8]) -> Result<(), BridgeError> {
    ensure_parent(p)?;
    std::fs::write(p, bytes).map_err(|e| BridgeError::failed(format!("gravar {}: {e}", p.display())))
}
