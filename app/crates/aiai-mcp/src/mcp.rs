//! Servidor MCP em stdio: JSON-RPC 2.0 delimitado por linhas, escrito à mão (initialize, notifications/initialized,
//! tools/list, tools/call, ping). Imagens saem como conteúdo `image` (base64 PNG).

use crate::context::Ctx;
use crate::tools;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

const DEFAULT_PROTOCOL: &str = "2024-11-05";

/// Responde a UMA mensagem JSON-RPC. `None` para notificações (sem `id`) e respostas de cliente.
pub fn handle_message(ctx: &mut Ctx, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        // sem `method` não há o que responder (lixo com id ganha erro)
        return id.map(|id| error(id, -32600, "Invalid Request: falta `method`"));
    };
    // notificação (notifications/initialized, notifications/cancelled...): sem resposta
    let id = id?;
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL);
            ok(
                id,
                json!({
                    "protocolVersion": version,
                    "serverInfo": { "name": "aiai-mcp", "version": env!("CARGO_PKG_VERSION") },
                    "capabilities": { "tools": { "listChanged": false } },
                    "instructions": "Operador do AnimeIAI Studio. Comece por aiai_status (app aberto ou headless), leia o documento com aiai_query, \
                        mude por aiai_apply ou pelas ferramentas curadas (aiai_draw_curves, aiai_fill, aiai_lint, aiai_paint, aiai_gradient, aiai_transform, aiai_repeat; tudo entra no undo) e confirme o efeito com aiai_render (olhe a imagem) ou lendo de volta. aiai_schema {\"which\":\"commands\"} lista todos os comandos."
                }),
            )
        }
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({ "tools": tools::definitions() })),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(error(id, -32602, "tools/call exige `name`"));
            };
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            let r = tools::call_tool(ctx, name, &args);
            let mut result = json!({ "content": r.content });
            if r.is_error {
                result["isError"] = json!(true);
            }
            ok(id, result)
        }
        "resources/list" => ok(id, json!({ "resources": [] })),
        "prompts/list" => ok(id, json!({ "prompts": [] })),
        "logging/setLevel" => ok(id, json!({})),
        other => error(id, -32601, &format!("Method '{other}' not found")),
    })
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Laço do servidor: uma mensagem por linha de `input`, uma resposta por linha em `output`.
pub fn serve(ctx: &mut Ctx, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(trimmed) {
            Ok(msg) if msg.is_array() => Some(error(Value::Null, -32600, "lotes JSON-RPC não são suportados")),
            Ok(msg) => handle_message(ctx, &msg),
            Err(e) => Some(error(Value::Null, -32700, &format!("Parse error: {e}"))),
        };
        if let Some(r) = reply {
            writeln!(output, "{}", serde_json::to_string(&r).unwrap_or_default())?;
            output.flush()?;
        }
    }
    Ok(())
}
