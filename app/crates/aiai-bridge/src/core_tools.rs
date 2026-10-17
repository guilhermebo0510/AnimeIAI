//! Ações de ferramenta que só dependem do documento (sem UI): `RENDER`, `DRAW_CURVES`, `FILL`, `LINT`, `PAINT`, `GRADIENT`,
//! `TRANSFORM`, `REPEAT` e `BRUSH_PRESETS`. Cada operação de `aiai-tools` devolve um `Command`; aqui ele é aplicado pela
//! `CoreSession` (autor `agent:claude` por padrão, `author`/`if_revision` opcionais), então entra no undo/redo e no log e
//! funciona igual no app, na bridge e no modo headless. As mutações devolvem o `Outcome` + `can_undo`/`can_redo`.
//!
//! `RENDER {cut, frame?, layers?, background?: "#rrggbb", include_guides?, include_reference?, camera? (padrão true; false = canvas inteiro do corte, sem câmera), size?: [w,h], out?, inline?}`
//! devolve `{width, height, warnings, png_base64 (com inline ou sem out), path (com out)}`.
//!
//! `PAINT`: `preset` pode ser um objeto completo, o NOME de um preset de `BRUSH_PRESETS` (sem diferenciar maiúsculas) ou um
//! objeto parcial com `name` de um preset conhecido (os campos dados sobrescrevem o preset); `preset_overrides` (objeto) é
//! mesclado por cima em qualquer caso.

use crate::core_ops::{DEFAULT_AUTHOR, core_error, obj, opt_bool, opt_str, opt_u64, outcome_json};
use crate::protocol::{BridgeError, code};
use aiai_core::{Command, CoreSession, Id, Rgba};
use aiai_paint::brush::BrushPreset;
use aiai_render::{RenderOptions, render_frame};
use aiai_tools::ToolError;
use aiai_tools::draw::{DrawRequest, draw_command};
use aiai_tools::fill::{FillRequest, fill_command, lint_with};
use aiai_tools::paint::{GradientRequest, PaintRequest, gradient_command, paint_command};
use aiai_tools::transform::{RepeatRequest, TransformRequest, repeat_command, transform_command};
use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use std::path::Path;

pub const TOOL_ACTIONS: &[&str] = &["RENDER", "DRAW_CURVES", "FILL", "LINT", "PAINT", "GRADIENT", "TRANSFORM", "REPEAT", "BRUSH_PRESETS"];

/// Todas as ações de ferramenta de documento: as de aqui + as de [`crate::production_tools`].
pub fn is_tool_action(action: &str) -> bool {
    TOOL_ACTIONS.contains(&action) || crate::production_tools::PRODUCTION_ACTIONS.contains(&action) || crate::preprod_tools::PREPROD_ACTIONS.contains(&action)
        || crate::review_tools::REVIEW_ACTIONS.contains(&action)
        || crate::satsuei_tools::SATSUEI_ACTIONS.contains(&action)
        || crate::preview_tools::PREVIEW_ACTIONS.contains(&action)
}

/// Ações de ferramenta que mudam o documento.
pub fn is_mutating_tool(action: &str) -> bool {
    matches!(action, "DRAW_CURVES" | "FILL" | "PAINT" | "GRADIENT" | "TRANSFORM" | "REPEAT") || crate::production_tools::is_mutating(action) || crate::preprod_tools::is_mutating(action) || crate::review_tools::is_mutating(action)
        || crate::satsuei_tools::is_mutating(action)
        || crate::preview_tools::is_mutating(action)
}

/// Erros das ferramentas → códigos da bridge: referência/valor inválido é `-32602`; falha do motor é `-32000`.
pub fn tool_error(e: ToolError) -> BridgeError {
    match e {
        ToolError::Core(c) => core_error(c),
        ToolError::NotFound(m) => BridgeError::invalid_params(format!("não encontrado: {m}")),
        ToolError::Invalid(m) => BridgeError::invalid_params(m),
        ToolError::Render(m) => BridgeError::failed(format!("render: {m}")),
    }
}

pub fn handle_tool_action(session: &mut CoreSession, action: &str, params: &Value) -> Option<Result<Value, BridgeError>> {
    Some(match action {
        "RENDER" => render(session, params),
        "BRUSH_PRESETS" => Ok(brush_presets()),
        a if crate::production_tools::PRODUCTION_ACTIONS.contains(&a) => crate::production_tools::handle(session, a, params),
        a if crate::preprod_tools::PREPROD_ACTIONS.contains(&a) => crate::preprod_tools::handle(session, a, params),
        a if crate::review_tools::REVIEW_ACTIONS.contains(&a) => crate::review_tools::handle(session, a, params),
        a if crate::satsuei_tools::SATSUEI_ACTIONS.contains(&a) => crate::satsuei_tools::handle(session, a, params),
        a if crate::preview_tools::PREVIEW_ACTIONS.contains(&a) => crate::preview_tools::handle(session, a, params),
        "LINT" => lint_action(session, params),
        "DRAW_CURVES" => request::<DrawRequest>(params, "DRAW_CURVES").and_then(|r| {
            let cmd = draw_command(&session.project, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }),
        "FILL" => request::<FillRequest>(params, "FILL").and_then(|r| {
            let cmd = fill_command(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }),
        "PAINT" => paint(session, params),
        "GRADIENT" => request::<GradientRequest>(params, "GRADIENT").and_then(|r| {
            let cmd = gradient_command(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }),
        "TRANSFORM" => request::<TransformRequest>(params, "TRANSFORM").and_then(|r| {
            let cmd = transform_command(&session.project, &mut session.blobs, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }),
        "REPEAT" => request::<RepeatRequest>(params, "REPEAT").and_then(|r| {
            let cmd = repeat_command(&session.project, &r).map_err(tool_error)?;
            apply(session, params, cmd)
        }),
        _ => return None,
    })
}

/// Decodifica os parâmetros no tipo de requisição da ferramenta (campos extras como `author` são ignorados).
pub(crate) fn request<T: DeserializeOwned>(params: &Value, action: &str) -> Result<T, BridgeError> {
    if !params.is_object() {
        return Err(BridgeError::invalid_params(format!("{action}: params deve ser um objeto JSON")));
    }
    serde_json::from_value::<T>(params.clone()).map_err(|e| BridgeError::invalid_params(format!("{action}: parâmetros inválidos: {e}")))
}

/// Aplica o comando devolvido pela ferramenta (autor e `if_revision` dos params).
pub(crate) fn apply(session: &mut CoreSession, params: &Value, cmd: Command) -> Result<Value, BridgeError> {
    let p = obj(params)?;
    let author = opt_str(&p, "author")?.unwrap_or(DEFAULT_AUTHOR).to_string();
    let if_revision = opt_u64(&p, "if_revision")?;
    let outcome = session.apply_as(cmd, &author, if_revision).map_err(core_error)?;
    outcome_json(session, &outcome)
}

// ------------------------------------------------------------------------------------------------------- pincéis

/// Presets iniciais (`BrushPreset::defaults()`) como JSON, com os nomes que `PAINT` aceita em `preset`.
pub fn brush_presets() -> Value {
    let presets = BrushPreset::defaults();
    json!({ "names": presets.iter().map(|p| p.name.clone()).collect::<Vec<_>>(), "presets": presets })
}

fn norm(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' => 'u',
            'ç' => 'c',
            other => other,
        })
        .filter(|c| !c.is_whitespace())
        .collect()
}

fn find_default(name: &str) -> Option<BrushPreset> {
    let wanted = norm(name);
    BrushPreset::defaults().into_iter().find(|p| norm(&p.name) == wanted)
}

fn merge(base: &mut Value, over: &Map<String, Value>) {
    if let Some(b) = base.as_object_mut() {
        for (k, v) in over {
            b.insert(k.clone(), v.clone());
        }
    }
}

/// `preset` (nome | objeto completo | objeto parcial com `name` conhecido) + `preset_overrides` → `BrushPreset`.
pub fn resolve_preset(p: &Map<String, Value>) -> Result<BrushPreset, BridgeError> {
    let unknown = |name: &str| {
        let names: Vec<String> = BrushPreset::defaults().into_iter().map(|p| p.name).collect();
        BridgeError::invalid_params(format!("preset desconhecido {name:?}; presets: {} (ou envie o objeto completo)", names.join(", ")))
    };
    let raw = p.get("preset").filter(|v| !v.is_null()).ok_or_else(|| BridgeError::invalid_params("PAINT exige 'preset' (nome de BRUSH_PRESETS ou objeto)"))?;
    let mut value = match raw {
        Value::String(name) => serde_json::to_value(find_default(name).ok_or_else(|| unknown(name))?).map_err(|e| BridgeError::failed(e.to_string()))?,
        Value::Object(o) => {
            let base = o.get("name").and_then(Value::as_str).and_then(find_default);
            match base {
                Some(b) => {
                    let mut v = serde_json::to_value(b).map_err(|e| BridgeError::failed(e.to_string()))?;
                    merge(&mut v, o);
                    v
                }
                None => raw.clone(),
            }
        }
        _ => return Err(BridgeError::invalid_params("'preset' deve ser um nome (texto) ou um objeto")),
    };
    if let Some(ov) = p.get("preset_overrides").filter(|v| !v.is_null()) {
        let ov = ov.as_object().ok_or_else(|| BridgeError::invalid_params("'preset_overrides' deve ser um objeto"))?;
        merge(&mut value, ov);
    }
    let preset: BrushPreset = serde_json::from_value(value).map_err(|e| BridgeError::invalid_params(format!("preset inválido: {e}")))?;
    preset.validate().map_err(|e| BridgeError::invalid_params(format!("preset inválido: {e}")))?;
    Ok(preset)
}

fn paint(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let p = obj(params)?;
    let preset = resolve_preset(&p)?;
    let mut with_preset = p.clone();
    with_preset.insert("preset".into(), serde_json::to_value(&preset).map_err(|e| BridgeError::failed(e.to_string()))?);
    let req: PaintRequest = request(&Value::Object(with_preset), "PAINT")?;
    let cmd = paint_command(&session.project, &mut session.blobs, &req).map_err(tool_error)?;
    apply(session, params, cmd)
}

// -------------------------------------------------------------------------------------------------------- lint

fn lint_action(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let p = obj(params)?;
    let id = |key: &str| -> Result<Id, BridgeError> { opt_str(&p, key)?.map(Id::from).ok_or_else(|| BridgeError::invalid_params(format!("LINT exige '{key}'"))) };
    let (cut, layer) = (id("cut")?, id("layer")?);
    let frame = opt_u64(&p, "frame")?.unwrap_or(0) as u32;
    let gap = match p.get("gap").filter(|v| !v.is_null()) {
        None => 3.0,
        Some(v) => v.as_f64().filter(|g| g.is_finite() && *g >= 0.0).ok_or_else(|| BridgeError::invalid_params("'gap' deve ser um número >= 0"))? as f32,
    };
    let min_area = opt_u64(&p, "min_area")?.unwrap_or(30) as usize;
    let include_border = p.get("include_border").and_then(Value::as_bool).unwrap_or(false);
    let report = lint_with(&session.project, &session.blobs, &cut, frame, &layer, gap, min_area, include_border).map_err(tool_error)?;
    serde_json::to_value(report).map_err(|e| BridgeError::failed(e.to_string()))
}

// ------------------------------------------------------------------------------------------------------ render

fn parse_background(s: &str) -> Result<[f32; 3], BridgeError> {
    let c = Rgba::from_hex(s).ok_or_else(|| BridgeError::invalid_params(format!("'background' deve ser #rrggbb, veio {s:?}")))?;
    Ok([c.0[0], c.0[1], c.0[2]])
}

fn render(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let p = obj(params)?;
    let cut = opt_str(&p, "cut")?.map(Id::from).ok_or_else(|| BridgeError::invalid_params("RENDER exige 'cut' (id do corte)"))?;
    let frame = opt_u64(&p, "frame")?.unwrap_or(0) as u32;
    if p.get("range").is_some_and(|v| !v.is_null()) {
        return Err(BridgeError::failed("RENDER: 'range' não é suportado; para vários quadros numa imagem use RENDER_SHEET (aiai_render_sheet)"));
    }
    if matches!(opt_str(&p, "backend")?, Some(b) if b != "cpu") {
        return Err(BridgeError::failed("RENDER: só o backend 'cpu' está implementado"));
    }
    let only_layers = match p.get("layers").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => {
            let arr = v.as_array().ok_or_else(|| BridgeError::invalid_params("'layers' deve ser uma lista de ids"))?;
            let mut ids = Vec::new();
            for x in arr {
                ids.push(Id::from(x.as_str().ok_or_else(|| BridgeError::invalid_params("'layers' deve conter textos (ids)"))?));
            }
            Some(ids)
        }
    };
    let opts = RenderOptions {
        include_guides: opt_bool(&p, "include_guides")?.unwrap_or(false),
        include_reference: opt_bool(&p, "include_reference")?.unwrap_or(false),
        only_layers,
        background: opt_str(&p, "background")?.map(parse_background).transpose()?,
        camera: opt_bool(&p, "camera")?.unwrap_or(true),
    };
    let out = render_frame(&session.project, &session.blobs, &cut, frame, &opts).map_err(|e| BridgeError::invalid_params(e.to_string()))?;
    let (mut width, mut height) = (out.image.width, out.image.height);
    let mut png = out.image.encode_png().map_err(|e| BridgeError::failed(format!("PNG: {e}")))?;
    if let Some(size) = p.get("size").filter(|v| !v.is_null()) {
        let a = size.as_array().filter(|a| a.len() == 2).ok_or_else(|| BridgeError::invalid_params("'size' deve ser [largura, altura]"))?;
        let dim = |i: usize| a[i].as_u64().filter(|v| (1..=16384).contains(v)).ok_or_else(|| BridgeError::invalid_params("'size' fora de 1..16384"));
        let (nw, nh) = (dim(0)? as u32, dim(1)? as u32);
        if (nw, nh) != (width, height) {
            let rgba = image::RgbaImage::from_raw(width, height, out.image.to_rgba8()).ok_or_else(|| BridgeError::failed("buffer de imagem inconsistente"))?;
            let resized = image::imageops::resize(&rgba, nw, nh, image::imageops::FilterType::Triangle);
            let mut buf = std::io::Cursor::new(Vec::new());
            resized.write_to(&mut buf, image::ImageFormat::Png).map_err(|e| BridgeError::failed(format!("PNG: {e}")))?;
            png = buf.into_inner();
            (width, height) = (nw, nh);
        }
    }
    let inline = opt_bool(&p, "inline")?.unwrap_or(false);
    let out_path = opt_str(&p, "out")?;
    let mut result = json!({ "width": width, "height": height, "warnings": out.warnings, "cut": cut, "frame": frame, "bytes": png.len() });
    if let Some(path) = out_path {
        let path = Path::new(path);
        if !path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(BridgeError::invalid_params("'out' deve ser um caminho absoluto sem '..'"));
        }
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
            return Err(BridgeError::invalid_params("'out' deve terminar em .png"));
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| BridgeError::new(code::ACTION_FAILED, format!("criando {}: {e}", dir.display())))?;
        }
        std::fs::write(path, &png).map_err(|e| BridgeError::new(code::ACTION_FAILED, format!("gravando {}: {e}", path.display())))?;
        result["path"] = json!(path.display().to_string());
    }
    if inline || out_path.is_none() {
        result["png_base64"] = json!(base64::engine::general_purpose::STANDARD.encode(&png));
    }
    Ok(result)
}
