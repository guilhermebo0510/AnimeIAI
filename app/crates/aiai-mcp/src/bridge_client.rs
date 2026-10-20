//! Cliente síncrono (NDJSON sobre TCP) da bridge do AnimeIAI Studio.
//!
//! Porta e token vêm de `AIAI_BRIDGE_ADDR` + `AIAI_BRIDGE_TOKEN` (se definidos) ou do arquivo de descoberta, RELIDO a cada
//! chamada: o app escolhe porta e token novos a cada execução, e o MCP precisa sobreviver a reinício/recompilação.

use aiai_bridge::discovery;
use aiai_bridge::server::parse_loopback_port;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Teto da resposta (screenshots em base64 passam de 1 MiB; o servidor limita em 32 MiB).
const MAX_RESPONSE: u64 = 32 << 20;
const MAX_REQUEST: usize = 1 << 20;

#[derive(Debug)]
pub enum CallError {
    /// Não deu para falar com o app (sem descoberta, recusou conexão, sem resposta ao conectar). O headless pode assumir.
    Unreachable(String),
    /// O app respondeu com erro (`code` do protocolo da bridge).
    Remote { code: Option<i64>, message: String },
    /// Falha no meio da conversa (timeout de leitura, resposta inválida/grande demais).
    Protocol(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Unreachable(m) | CallError::Protocol(m) => f.write_str(m),
            CallError::Remote { code: Some(c), message } => write!(f, "[{c}] {message}"),
            CallError::Remote { code: None, message } => f.write_str(message),
        }
    }
}

impl std::error::Error for CallError {}

enum Target {
    Fixed { addr: String, token: String },
    Discover { path: PathBuf },
}

pub struct BridgeClient {
    target: Target,
    next_id: AtomicU64,
}

/// Prazo de leitura por ação, um pouco acima do prazo que o servidor dá à UI (o erro do app chega antes do nosso).
pub fn read_timeout_for(action: &str) -> Duration {
    match action {
        "SCREENSHOT" | "RENDER" | "COMPARE" => Duration::from_secs(35),
        "NEW_PROJECT" | "OPEN_PROJECT" | "SAVE_PROJECT" => Duration::from_secs(25),
        "EXPORT_VIDEO" | "ANIMATIC" => Duration::from_secs(1200),
        "STORYBOARD" | "CUTS_FROM_STORYBOARD" | "PRODUCTION" | "RETAKE" => Duration::from_secs(65),
        "RENDER_SHEET" | "EDIT_PREVIEW" | "EDIT_MIX" | "TTS_REGENERATE" => Duration::from_secs(130),
        "BGM" | "TTS_LINE" | "TTS_KOKORO" | "MIX_CUT" | "LOUDNESS" | "AUDIO_EDIT" | "AUDIO_ANALYZE" => Duration::from_secs(130),
        "SKY" | "PATTERN_FILL" | "FILL_SHAPE" | "EDIT_SET" | "TEXT" | "GAG_BG" | "GUIDE3D" | "MODEL_SHEET" | "INBETWEEN" | "PROPAGATE_FILLS" | "LIPSYNC" => Duration::from_secs(65),
        _ => Duration::from_secs(15),
    }
}

impl BridgeClient {
    pub fn from_env() -> Self {
        let token = std::env::var("AIAI_BRIDGE_TOKEN").unwrap_or_default();
        if let Some(addr) = std::env::var("AIAI_BRIDGE_ADDR").ok().filter(|a| !a.trim().is_empty()) {
            return Self::fixed(addr, token);
        }
        Self::discovering(discovery::default_path())
    }

    pub fn fixed(addr: impl Into<String>, token: impl Into<String>) -> Self {
        BridgeClient { target: Target::Fixed { addr: addr.into(), token: token.into() }, next_id: AtomicU64::new(1) }
    }

    pub fn discovering(path: PathBuf) -> Self {
        BridgeClient { target: Target::Discover { path }, next_id: AtomicU64::new(1) }
    }

    /// (endereço, token, origem em texto) para a próxima chamada, ou o motivo de não haver destino.
    fn resolve(&self) -> Result<(String, String, String), String> {
        match &self.target {
            Target::Fixed { addr, token } => Ok((addr.clone(), token.clone(), "AIAI_BRIDGE_ADDR".to_string())),
            Target::Discover { path } => match discovery::read(path) {
                Ok(d) => Ok((d.addr, d.token, format!("descoberta {}", path.display()))),
                Err(e) => Err(format!("sem arquivo de descoberta válido em {} ({e})", path.display())),
            },
        }
    }

    /// Texto curto para diagnóstico (nunca inclui o token).
    pub fn describe(&self) -> String {
        match self.resolve() {
            Ok((addr, _, source)) => format!("{addr} via {source}"),
            Err(why) => why,
        }
    }

    pub fn call(&self, action: &str, params: Value, read_timeout: Option<Duration>) -> Result<Value, CallError> {
        let (addr, token, source) = self.resolve().map_err(CallError::Unreachable)?;
        let sock = parse_addr(&addr).ok_or_else(|| CallError::Unreachable(format!("endereço da bridge inválido ou fora do loopback: {addr}")))?;
        let stream = TcpStream::connect_timeout(&sock, Duration::from_millis(800))
            .map_err(|e| CallError::Unreachable(format!("AnimeIAI Studio não responde em {addr} ({source}): {e}; abra o app com --bridge")))?;
        let _ = stream.set_nodelay(true);
        let timeout = read_timeout.unwrap_or_else(|| read_timeout_for(action));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_read_timeout(Some(timeout));

        let id = format!("mcp-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let mut line = serde_json::to_string(&json!({ "id": id, "action": action, "params": params, "token": token })).map_err(|e| CallError::Protocol(e.to_string()))?;
        line.push('\n');
        if line.len() > MAX_REQUEST {
            return Err(CallError::Protocol(format!("requisição grande demais para a bridge: {} > {MAX_REQUEST} bytes", line.len())));
        }
        let mut writer = stream.try_clone().map_err(|e| CallError::Protocol(e.to_string()))?;
        writer.write_all(line.as_bytes()).and_then(|_| writer.flush()).map_err(|e| CallError::Protocol(format!("falha ao enviar para a bridge: {e}")))?;

        let mut reader = BufReader::new(stream.take(MAX_RESPONSE + 1));
        let mut resp_line = String::new();
        reader.read_line(&mut resp_line).map_err(|e| CallError::Protocol(format!("sem resposta da bridge em {}s para {action}: {e}", timeout.as_secs())))?;
        if resp_line.len() as u64 > MAX_RESPONSE {
            return Err(CallError::Protocol(format!("resposta da bridge grande demais (> {MAX_RESPONSE} bytes)")));
        }
        if resp_line.trim().is_empty() {
            return Err(CallError::Protocol(format!("resposta vazia da bridge para {action}")));
        }
        let resp: Value = serde_json::from_str(&resp_line).map_err(|e| {
            let head: String = resp_line.chars().take(200).collect();
            CallError::Protocol(format!("resposta da bridge não é JSON ({e}): {head}"))
        })?;
        if resp.get("success").and_then(Value::as_bool).unwrap_or(false) {
            return Ok(resp.get("data").cloned().unwrap_or(Value::Null));
        }
        let message = resp.get("error").and_then(Value::as_str).unwrap_or("erro desconhecido da bridge").to_string();
        let code = resp.get("code").and_then(Value::as_i64);
        if code == Some(-32001) {
            let hint = if token.is_empty() {
                "a bridge exige token e nenhum foi encontrado: abra o app com --bridge (o token vem do arquivo de descoberta) ou defina AIAI_BRIDGE_TOKEN"
            } else {
                "a bridge recusou o token: o MCP está falando com outra instância do app? confira o arquivo de descoberta"
            };
            return Err(CallError::Remote { code, message: format!("{message}; {hint} (destino: {source})") });
        }
        Err(CallError::Remote { code, message })
    }

    /// O app está respondendo ao PING?
    pub fn is_live(&self) -> bool {
        self.call("PING", json!({}), Some(Duration::from_millis(1500))).is_ok()
    }
}

fn parse_addr(addr: &str) -> Option<SocketAddr> {
    let port = parse_loopback_port(addr)?;
    Some(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port))
}
