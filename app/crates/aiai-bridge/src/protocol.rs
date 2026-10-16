//! Envelope do protocolo da bridge. É o MESMO do bridge Tauri legado (`bridge_client.rs` do `aiai-mcp`):
//!
//! ```text
//! -> {"id":"mcp-1","action":"APPLY_COMMAND","params":{...},"token":"<hex>"}\n
//! <- {"id":"mcp-1","success":true,"data":{...}}\n
//! <- {"id":"mcp-1","success":false,"error":"mensagem","code":-32602}\n
//! ```
//!
//! `code` é ADITIVO (clientes antigos o ignoram). Mensagens de auth contêm "Unauthorized"/"token" para o
//! cliente legado continuar produzindo a mensagem acionável.

use serde::Deserialize;
use serde_json::{json, Value};

/// Versão do protocolo (sobe só em mudança incompatível do envelope).
pub const PROTOCOL_VERSION: u32 = 1;

pub const MAX_REQUEST_BYTES: usize = 1 << 20;
pub const MAX_RESPONSE_BYTES: usize = 32 << 20;

/// Códigos de erro (estilo JSON-RPC).
pub mod code {
    pub const UNAUTHORIZED: i32 = -32001;
    pub const UI_TIMEOUT: i32 = -32002;
    pub const BUSY: i32 = -32003;
    pub const RATE_LIMITED: i32 = -32004;
    pub const TOO_LARGE: i32 = -32005;
    pub const SHUTTING_DOWN: i32 = -32006;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const UNKNOWN_ACTION: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const ACTION_FAILED: i32 = -32000;
    /// `if_revision` não bate com a revisão atual do documento.
    pub const STALE_REVISION: i32 = -32010;
}

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub id: Value,
    pub action: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub token: Option<String>,
}

impl Request {
    /// Id em texto (o cliente legado usa string; aceitamos número por tolerância).
    pub fn id_string(&self) -> String {
        match &self.id {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeError {
    pub code: i32,
    pub message: String,
}

impl BridgeError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(code::INVALID_PARAMS, message)
    }
    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(code::ACTION_FAILED, message)
    }
    pub fn unknown_action(action: &str) -> Self {
        Self::new(code::UNKNOWN_ACTION, format!("ação desconhecida: {action}"))
    }
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for BridgeError {}

pub type BridgeResult = Result<Value, BridgeError>;

/// Resposta serializada (uma linha NDJSON / corpo HTTP).
pub fn response_json(id: &str, result: &BridgeResult) -> Value {
    match result {
        Ok(data) => json!({ "id": id, "success": true, "data": data }),
        Err(e) => json!({ "id": id, "success": false, "error": e.message, "code": e.code }),
    }
}

/// Comparação em tempo constante (o tamanho do token não é segredo).
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Token aleatório de 256 bits em hexadecimal (64 caracteres). SEM fallback fraco: sem entropia do SO, erro.
pub fn generate_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("sem entropia do sistema operacional: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

