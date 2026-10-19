//! Chaves desconhecidas nos parâmetros das ações viram erro (`-32602`) em vez de serem ignoradas em silêncio: um
//! `RENDER {only_layers: [...]}` (a chave certa é `layers`) renderizava todas as camadas sem aviso. A mensagem nomeia a
//! chave, sugere a mais parecida e lista as válidas.
//!
//! * Ações que decodificam numa struct ([`crate::core_tools::request`]) tiram as chaves válidas do JSON Schema da struct
//!   (o mesmo que o `aiai-mcp` publica como `inputSchema`), incluindo campos achatados (`flatten`) e, em enums com
//!   `tag = "op"`, só as chaves das variantes compatíveis com o `op` dado; mais as chaves lidas à mão por fora da struct
//!   ([`extra_keys`]).
//! * Ações que leem os parâmetros à mão (aqui e as de UI do app) chamam [`check_keys`] com a lista delas.

use crate::protocol::BridgeError;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// Chaves lidas fora da struct de requisição: `author`/`if_revision` (lidas por `apply` nas ações que mudam o documento),
/// `voicevox_url` (TTS) e `preset_overrides` (PAINT).
pub(crate) fn extra_keys(action: &str) -> Vec<&'static str> {
    let mut keys = Vec::new();
    if crate::core_tools::is_mutating_tool(action) {
        keys.extend(["author", "if_revision"]);
    }
    match action {
        "TTS_LINE" | "TTS_REGENERATE" => keys.push("voicevox_url"),
        "PAINT" => keys.push("preset_overrides"),
        _ => {}
    }
    keys
}

/// Recusa chave fora de `allowed` (os parâmetros já devem ser um objeto ou `null`). Público para as ações de UI do app.
pub fn check_keys(params: &Value, action: &str, allowed: &[&str]) -> Result<(), BridgeError> {
    let Some(p) = params.as_object() else { return Ok(()) };
    check_set(p, action, &allowed.iter().map(|s| s.to_string()).collect())
}

/// Como [`check_keys`], com as chaves tiradas do JSON Schema de `T` + [`extra_keys`]. Schema aberto
/// (`additionalProperties` diferente de `false`) aceita qualquer chave.
pub(crate) fn check_schema_keys<T: schemars::JsonSchema>(params: &Value, action: &str) -> Result<(), BridgeError> {
    let Some(p) = params.as_object() else { return Ok(()) };
    let root = serde_json::to_value(schemars::schema_for!(T)).map_err(|e| BridgeError::failed(format!("{action}: schema: {e}")))?;
    let defs = root.get("definitions").cloned().unwrap_or(Value::Null);
    let mut allowed = BTreeSet::new();
    if collect(&root, &defs, p, &mut allowed, 0) {
        return Ok(());
    }
    allowed.extend(extra_keys(action).into_iter().map(String::from));
    check_set(p, action, &allowed)
}

fn check_set(p: &Map<String, Value>, action: &str, allowed: &BTreeSet<String>) -> Result<(), BridgeError> {
    let unknown: Vec<&String> = p.keys().filter(|k| !allowed.contains(*k)).collect();
    if unknown.is_empty() {
        return Ok(());
    }
    let names = unknown.iter().map(|k| format!("'{k}'")).collect::<Vec<_>>().join(", ");
    let hints: Vec<String> = unknown.iter().filter_map(|k| closest(k, allowed).map(|c| format!("'{k}' → '{c}'?"))).collect();
    let valid = if allowed.is_empty() { "nenhum (a ação não recebe parâmetros)".to_string() } else { allowed.iter().cloned().collect::<Vec<_>>().join(", ") };
    let plural = if unknown.len() > 1 { "parâmetros desconhecidos" } else { "parâmetro desconhecido" };
    let hint = if hints.is_empty() { String::new() } else { format!(" (quis dizer {})", hints.join(", ")) };
    Err(BridgeError::invalid_params(format!("{action}: {plural} {names}{hint}; válidos: {valid}")))
}

/// Junta em `out` as chaves de `schema`; devolve `true` se o schema aceita chaves livres.
fn collect(schema: &Value, defs: &Value, params: &Map<String, Value>, out: &mut BTreeSet<String>, depth: usize) -> bool {
    if depth > 16 {
        return true;
    }
    let schema = resolve(schema, defs);
    if schema.get("additionalProperties").is_some_and(|a| a != &Value::Bool(false)) {
        return true;
    }
    if let Some(props) = schema.get("properties").and_then(Value::as_object) {
        out.extend(props.keys().cloned());
    }
    let mut open = false;
    for sub in schema.get("allOf").and_then(Value::as_array).into_iter().flatten() {
        open |= collect(sub, defs, params, out, depth + 1);
    }
    for key in ["anyOf", "oneOf"] {
        let Some(variants) = schema.get(key).and_then(Value::as_array) else { continue };
        let matching: Vec<&Value> = variants.iter().filter(|v| compatible(v, defs, params, depth + 1)).collect();
        // nenhuma variante bate (ex.: `op` errado): junta todas e deixa o serde explicar o valor errado
        for v in if matching.is_empty() { variants.iter().collect() } else { matching } {
            open |= collect(v, defs, params, out, depth + 1);
        }
    }
    open
}

/// A variante não contradiz uma chave de valor único (`enum` de um item ou `const`, ex.: `op` de enum com tag)?
fn compatible(variant: &Value, defs: &Value, params: &Map<String, Value>, depth: usize) -> bool {
    if depth > 16 {
        return true;
    }
    let v = resolve(variant, defs);
    if let Some(props) = v.get("properties").and_then(Value::as_object) {
        let required: Vec<&str> = v.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        for (k, s) in props {
            let s = resolve(s, defs);
            let fixed = s.get("const").or_else(|| s.get("enum").and_then(Value::as_array).filter(|e| e.len() == 1).map(|e| &e[0]));
            if let Some(fixed) = fixed {
                match params.get(k) {
                    Some(given) if given != fixed => return false,
                    None if required.contains(&k.as_str()) => return false,
                    _ => {}
                }
            }
        }
    }
    v.get("allOf").and_then(Value::as_array).into_iter().flatten().all(|s| compatible(s, defs, params, depth + 1))
}

fn resolve<'a>(schema: &'a Value, defs: &'a Value) -> &'a Value {
    let mut s = schema;
    for _ in 0..8 {
        match s.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix("#/definitions/")).and_then(|name| defs.get(name)) {
            Some(d) => s = d,
            None => break,
        }
    }
    s
}

/// Chave válida mais parecida (contém a outra ou está a até 2 edições).
fn closest<'a>(key: &str, allowed: &'a BTreeSet<String>) -> Option<&'a String> {
    let k = key.to_lowercase();
    allowed
        .iter()
        .map(|a| {
            let al = a.to_lowercase();
            let d = levenshtein(&k, &al);
            let score = if d <= 2 { d } else if al.len() >= 3 && (k.contains(&al) || al.contains(&k)) { 3 } else { usize::MAX };
            (score, a)
        })
        .filter(|(s, _)| *s != usize::MAX)
        .min_by_key(|(s, _)| *s)
        .map(|(_, a)| a)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + usize::from(ca != *cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

