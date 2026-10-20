//! Entrada do binário e subcomandos:
//!
//! ```text
//! aiai-mcp                                   servidor MCP (stdio)
//! aiai-mcp call <AÇÃO> ['<json>'|@arq.json] [--png out.png] [--project <pasta>] [--timeout <s>]
//! aiai-mcp info                              destino da bridge e se o app responde
//! ```
//!
//! O resultado (`data`) sai em JSON numa linha no stdout; erros vão para o stderr com saída 1 (2 = uso incorreto).
//! `--png` grava `data.png_base64` no caminho dado (sandbox de gravação).

use crate::context::{Ctx, take_png};
use crate::mcp;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, PartialEq)]
pub enum Cmd {
    Serve,
    Info,
    Call { action: String, params: Value, png: Option<String>, project: Option<String>, timeout: Option<u64> },
    Help,
}

const MAX_PARAMS_BYTES: usize = 1 << 20;

pub const USAGE: &str = "uso:
  aiai-mcp                         servidor MCP (stdio)
  aiai-mcp call <AÇÃO> ['<json>'|@arquivo.json] [--png out.png] [--project <pasta .aiai>] [--timeout <s>]
  aiai-mcp info                    destino da bridge e se o app responde
Ações: PING GET_STATUS APPLY_COMMAND QUERY UNDO REDO GET_LOG NEW_PROJECT OPEN_PROJECT SAVE_PROJECT e as de ferramenta
RENDER DRAW_CURVES FILL LINT PAINT GRADIENT TRANSFORM REPEAT BRUSH_PRESETS READ_POINTS EDIT_POINTS, SKY PATTERN_FILL TEXT LINES GAG_BG PARTICLES GUIDE3D
PERSPECTIVE MODEL_SHEET CONSISTENCY INBETWEEN PROPAGATE_FILLS TTS_LINE TTS_KOKORO VOICE_PROFILE LIPSYNC SFX BGM MIX_CUT LOUDNESS
COMPARE PROBE_COLOR MEASURE_LINE MEASURE_EDGE FILL_SHAPE EDIT_SET EXPORT_VIDEO, SCRIPT BEATS STORYBOARD ANIMATIC
CUTS_FROM_STORYBOARD PRODUCTION CORRECTION RETAKE EFFECTS GRADE CAMERA LAYER_MOTION
RENDER_SHEET EDIT_PREVIEW EDIT_MIX TTS_REGENERATE AUDIO_EDIT AUDIO_ANALYZE (headless ou app) e SCREENSHOT INPUT GET_UI_STATE UI_ACTION GET_METRICS (só com o app aberto --bridge).
Headless: sem app (ou com --project / AIAI_HEADLESS_PROJECT) as ações de documento rodam em processo e o projeto é salvo após cada mudança.";

pub fn parse(args: &[String]) -> Result<Cmd, String> {
    match args.first().map(String::as_str) {
        None => Ok(Cmd::Serve),
        Some("--help" | "-h" | "help") => Ok(Cmd::Help),
        Some("info") => Ok(Cmd::Info),
        Some("call") => {
            let mut it = args[1..].iter();
            let action = it.next().ok_or("call exige a ação (ex.: PING)")?.clone();
            if action.is_empty() || action.len() > 32 || !action.starts_with(|c: char| c.is_ascii_uppercase())
                || !action.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            {
                return Err(format!("ação inválida {action:?} (esperado MAIUSCULAS_COM_SUBLINHADO; dígitos depois da 1ª letra, ex.: GUIDE3D)"));
            }
            let mut params = json!({});
            let (mut png, mut project, mut timeout) = (None, None, None);
            let mut positional = false;
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--png" => png = Some(it.next().ok_or("--png exige um caminho")?.clone()),
                    "--project" => project = Some(it.next().ok_or("--project exige uma pasta")?.clone()),
                    "--timeout" => {
                        let t: u64 = it.next().ok_or("--timeout exige segundos")?.parse().map_err(|_| "--timeout precisa ser inteiro".to_string())?;
                        if !(1..=1800).contains(&t) {
                            return Err("--timeout fora de 1..1800".into());
                        }
                        timeout = Some(t);
                    }
                    other if other.starts_with("--") => return Err(format!("opção desconhecida: {other}")),
                    json_or_file => {
                        if positional {
                            return Err("params informado duas vezes".into());
                        }
                        positional = true;
                        params = parse_params(json_or_file)?;
                    }
                }
            }
            Ok(Cmd::Call { action, params, png, project, timeout })
        }
        Some(other) => Err(format!("argumento desconhecido: {other}")),
    }
}

fn parse_params(arg: &str) -> Result<Value, String> {
    let raw = if let Some(path) = arg.strip_prefix('@') {
        let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
        if meta.len() as usize > MAX_PARAMS_BYTES {
            return Err("arquivo de params grande demais (> 1 MiB)".into());
        }
        // BOM UTF-8 (PowerShell 5.1 grava com BOM) não pode quebrar o JSON
        std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?.trim_start_matches('\u{feff}').to_string()
    } else {
        arg.to_string()
    };
    if raw.len() > MAX_PARAMS_BYTES {
        return Err("params grande demais (> 1 MiB)".into());
    }
    let v: Value = serde_json::from_str(&raw).map_err(|e| format!("params não é JSON válido: {e}"))?;
    if !v.is_object() {
        return Err("params deve ser um objeto JSON".into());
    }
    Ok(v)
}

/// Ponto de entrada do binário; devolve o código de saída.
pub fn main_entry(args: Vec<String>) -> i32 {
    let cmd = match parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("aiai-mcp: {e}\n{USAGE}");
            return 2;
        }
    };
    match cmd {
        Cmd::Help => {
            println!("{USAGE}");
            0
        }
        Cmd::Serve => {
            init_logging();
            let mut ctx = Ctx::from_env(None);
            tracing::info!(sandbox = ?ctx.sandbox.dirs(), bridge = %ctx.client.describe(), "aiai-mcp pronto (stdio)");
            let stdin = std::io::stdin();
            match mcp::serve(&mut ctx, stdin.lock(), std::io::stdout().lock()) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("aiai-mcp: {e}");
                    1
                }
            }
        }
        Cmd::Info => {
            let ctx = Ctx::from_env(None);
            let live = ctx.client.is_live();
            println!("{}", json!({ "target": ctx.client.describe(), "live": live, "sandbox": ctx.sandbox.dirs() }));
            0
        }
        Cmd::Call { action, params, png, project, timeout } => match run_call(action, params, png, project, timeout) {
            Ok(out) => {
                println!("{out}");
                0
            }
            Err(e) => {
                eprintln!("aiai-mcp: {e}");
                1
            }
        },
    }
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("aiai_mcp=info"));
    // stdout é do protocolo MCP: log só no stderr
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).try_init();
}

fn run_call(action: String, params: Value, png: Option<String>, project: Option<String>, timeout: Option<u64>) -> Result<String, String> {
    let mut ctx = Ctx::from_env(project.map(PathBuf::from));
    // valida o destino do PNG antes de gastar a chamada
    let save_to = match &png {
        Some(p) => Some(ctx.sandbox.check_write(p).map_err(|e| format!("--png: {e}"))?),
        None => None,
    };
    let mut params = params;
    if action == "SCREENSHOT" && png.is_some() {
        params["inline"] = json!(true);
    }
    let mut data = ctx.invoke(&action, params, timeout.map(Duration::from_secs)).map_err(|e| e.to_string())?;
    if save_to.is_some() {
        let got = take_png(&mut data, save_to.as_ref()).map_err(|e| e.to_string())?;
        if got.is_none() {
            return Err("a resposta não tem png_base64 (--png serve para SCREENSHOT/RENDER com inline)".into());
        }
    }
    serde_json::to_string(&data).map_err(|e| e.to_string())
}
