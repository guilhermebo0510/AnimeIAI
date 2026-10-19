//! Roteamento de ações: app aberto (bridge) ou headless (em processo), mais o tratamento de arquivos (sandbox, PNG).

use crate::bridge_client::{BridgeClient, CallError};
use crate::headless::Headless;
use crate::sandbox::Sandbox;
use base64::Engine as _;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

/// Mensagem padrão quando uma ação precisa da UI e o app não está aberto.
pub const STUDIO_CLOSED: &str = "AnimeIAI Studio não está aberto com --bridge";

#[derive(Debug, Clone)]
pub struct ToolError {
    pub code: i64,
    pub message: String,
}

impl ToolError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        ToolError { code, message: message.into() }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        ToolError::new(-32602, message)
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl From<aiai_bridge::BridgeError> for ToolError {
    fn from(e: aiai_bridge::BridgeError) -> Self {
        ToolError::new(e.code as i64, e.message)
    }
}

pub struct Ctx {
    pub client: BridgeClient,
    pub headless: Headless,
    /// `--project`/`AIAI_HEADLESS_PROJECT` dado: as ações de documento rodam em processo mesmo com o app aberto.
    pub force_headless: bool,
    pub sandbox: Sandbox,
}

impl Ctx {
    /// `project`: pasta de `--project`; senão `AIAI_HEADLESS_PROJECT`.
    pub fn from_env(project: Option<PathBuf>) -> Ctx {
        let project = project.or_else(|| std::env::var_os("AIAI_HEADLESS_PROJECT").filter(|v| !v.is_empty()).map(PathBuf::from));
        Ctx::new(BridgeClient::from_env(), Sandbox::from_env(), project)
    }

    pub fn new(client: BridgeClient, sandbox: Sandbox, project: Option<PathBuf>) -> Ctx {
        Ctx { client, force_headless: project.is_some(), headless: Headless::new(sandbox.clone(), project), sandbox }
    }

    /// Valida os caminhos de gravação e de leitura dos parâmetros ANTES de sair do processo e os troca pela forma absoluta.
    fn sanitize(&self, action: &str, params: &mut Value) -> Result<(), ToolError> {
        let write_keys: &[&str] = match action {
            "NEW_PROJECT" | "OPEN_PROJECT" | "SAVE_PROJECT" => &["path"],
            "RENDER" | "SCREENSHOT" | "MIX_CUT" | "EXPORT_VIDEO" | "ANIMATIC" | "STORYBOARD" | "SCRIPT" | "PRODUCTION" | "RETAKE" | "RENDER_SHEET" | "EDIT_PREVIEW" | "EDIT_MIX" => &["out"],
            "COMPARE" => &["out_dir", "out_diff"],
            _ => &[],
        };
        for key in write_keys {
            if let Some(s) = params.get(*key).and_then(Value::as_str) {
                let p = self.sandbox.check_write(s).map_err(|e| ToolError::invalid(format!("'{key}': {e}")))?;
                params[*key] = json!(p.display().to_string());
            }
        }
        // leitura: referências, fontes, SoundFonts, legendas e WAVs (sob C:\AnimeIAI)
        let read_keys: &[&str] = match action {
            "COMPARE" | "PROBE_COLOR" | "MEASURE_LINE" | "MEASURE_EDGE" => &["reference"],
            "TEXT" => &["font"],
            "BGM" => &["soundfont"],
            "EXPORT_VIDEO" => &["srt"],
            "LOUDNESS" | "IMPORT_AUDIO" => &["path"],
            _ => &[],
        };
        for key in read_keys {
            if let Some(s) = params.get(*key).and_then(Value::as_str) {
                if *key == "font" && (s == "regular" || s == "black") {
                    continue;
                }
                let p = self.sandbox.check_read(s).map_err(|e| ToolError::invalid(format!("'{key}': {e}")))?;
                params[*key] = json!(p.display().to_string());
            }
        }
        Ok(())
    }

    /// Executa uma ação. Documento: app (se aberto e sem `--project`) ou headless. UI: só app.
    pub fn invoke(&mut self, action: &str, mut params: Value, timeout: Option<Duration>) -> Result<Value, ToolError> {
        if params.is_null() {
            params = json!({});
        }
        self.sanitize(action, &mut params)?;
        if self.force_headless && Headless::handles(action) {
            return self.run_headless(action, &params, None);
        }
        match self.client.call(action, params.clone(), timeout) {
            Ok(v) => Ok(v),
            Err(CallError::Unreachable(why)) => {
                if Headless::handles(action) {
                    self.run_headless(action, &params, Some(why))
                } else {
                    Err(ToolError::new(-32000, format!("{STUDIO_CLOSED} ({why}). A ação {action} precisa da janela do app.")))
                }
            }
            Err(CallError::Remote { code, message }) => Err(ToolError::new(code.unwrap_or(-32000), message)),
            Err(CallError::Protocol(m)) => Err(ToolError::new(-32000, m)),
        }
    }

    fn run_headless(&mut self, action: &str, params: &Value, bridge_down: Option<String>) -> Result<Value, ToolError> {
        let mut v = self.headless.call(action, params)?;
        if let Some(o) = v.as_object_mut() {
            o.insert("_via".into(), json!("headless"));
            if let (Some(why), "GET_STATUS") = (bridge_down, action) {
                o.insert("_bridge".into(), json!(format!("app fechado: {why}")));
            }
        }
        Ok(v)
    }
}

/// Imagem encontrada em `data.png_base64`.
pub struct PngPayload {
    pub bytes: Vec<u8>,
}

/// Se `data` traz `png_base64`: decodifica, grava em `save_to` (já validado pelo sandbox) e troca o campo por um resumo.
pub fn take_png(data: &mut Value, save_to: Option<&PathBuf>) -> Result<Option<PngPayload>, ToolError> {
    let Some(obj) = data.as_object_mut() else { return Ok(None) };
    let Some(b64) = obj.get("png_base64").and_then(Value::as_str) else { return Ok(None) };
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| ToolError::new(-32000, format!("png_base64 inválido: {e}")))?;
    let mut summary = format!("<{} bytes>", bytes.len());
    if let Some(path) = save_to {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| ToolError::new(-32000, format!("criando {}: {e}", dir.display())))?;
        }
        std::fs::write(path, &bytes).map_err(|e| ToolError::new(-32000, format!("gravando {}: {e}", path.display())))?;
        summary = format!("<{} bytes salvos em {}>", bytes.len(), path.display());
        obj.insert("saved_to".into(), json!(path.display().to_string()));
    }
    obj.insert("png_base64".into(), json!(summary));
    Ok(Some(PngPayload { bytes }))
}

/// PNG reduzido para o conteúdo de imagem MCP (lado maior <= `max_side`, padrão 1280 ou `AIAI_MCP_IMAGE_MAX`).
pub fn mcp_image(png: &[u8]) -> Value {
    let max_side: u32 = std::env::var("AIAI_MCP_IMAGE_MAX").ok().and_then(|v| v.parse().ok()).filter(|v| (64..=8192).contains(v)).unwrap_or(1280);
    let out = shrink_png(png, max_side).unwrap_or_else(|| png.to_vec());
    json!({ "type": "image", "data": base64::engine::general_purpose::STANDARD.encode(out), "mimeType": "image/png" })
}

fn shrink_png(png: &[u8], max_side: u32) -> Option<Vec<u8>> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    if img.width().max(img.height()) <= max_side {
        return None;
    }
    let small = img.resize(max_side, max_side, image::imageops::FilterType::Triangle);
    let mut buf = std::io::Cursor::new(Vec::new());
    small.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    Some(buf.into_inner())
}
