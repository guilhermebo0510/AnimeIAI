//! Ações da bridge que só dependem da `CoreSession` (sem UI): são o MESMO caminho de `apply`/`undo`/`redo` que o app
//! usa para as ações do usuário. O studio chama [`handle_core_action`] da thread da UI; o `aiai-mcp` em modo headless e
//! os testes de integração chamam de outras threads/processos.
//!
//! Ações: `PING`, `APPLY_COMMAND`, `QUERY`, `UNDO`, `REDO`, `GET_LOG`, `NEW_PROJECT`, `OPEN_PROJECT`, `SAVE_PROJECT` e as
//! ferramentas de [`crate::core_tools`] (`RENDER`, `DRAW_CURVES`, `FILL`, `LINT`, `PAINT`, `GRADIENT`, `TRANSFORM`, `REPEAT`,
//! `BRUSH_PRESETS`), que só precisam do documento e portanto também rodam no modo headless.
//! `NEW_PROJECT {name, settings?, path?, overwrite?}` com `path` já salva a pasta e recusa sobrescrever projeto existente.
//! Ações de UI (SCREENSHOT, INPUT, GET_UI_STATE, UI_ACTION, RENDER, COMPARE, GET_STATUS) devolvem `None`: são do app.

use crate::params_check::check_keys;
use crate::protocol::{BridgeError, code};
use aiai_core::{Command, CoreError, CoreSession, Project, Query, persist};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// Autor registrado no log para comandos vindos da bridge.
pub const DEFAULT_AUTHOR: &str = "agent:claude";

pub const CORE_ACTIONS: &[&str] = &["PING", "APPLY_COMMAND", "QUERY", "UNDO", "REDO", "GET_LOG", "NEW_PROJECT", "OPEN_PROJECT", "SAVE_PROJECT"];

pub fn is_core_action(action: &str) -> bool {
    CORE_ACTIONS.contains(&action) || crate::core_tools::is_tool_action(action)
}

/// Ações que (quando bem-sucedidas) mudam o documento: o headless salva o projeto depois delas.
pub fn is_mutating(action: &str) -> bool {
    matches!(action, "APPLY_COMMAND" | "UNDO" | "REDO") || crate::core_tools::is_mutating_tool(action)
}

/// Traduz erros do núcleo para códigos da bridge.
pub fn core_error(e: CoreError) -> BridgeError {
    match e {
        CoreError::StaleRevision { .. } => BridgeError::new(code::STALE_REVISION, e.to_string()),
        CoreError::Json(_) | CoreError::Invalid { .. } => BridgeError::invalid_params(e.to_string()),
        other => BridgeError::failed(other.to_string()),
    }
}

pub(crate) fn obj(params: &Value) -> Result<Map<String, Value>, BridgeError> {
    match params {
        Value::Null => Ok(Map::new()),
        Value::Object(m) => Ok(m.clone()),
        _ => Err(BridgeError::invalid_params("params deve ser um objeto JSON")),
    }
}

pub(crate) fn opt_u64(p: &Map<String, Value>, key: &str) -> Result<Option<u64>, BridgeError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| BridgeError::invalid_params(format!("'{key}' deve ser um inteiro não negativo"))),
    }
}

pub(crate) fn opt_bool(p: &Map<String, Value>, key: &str) -> Result<Option<bool>, BridgeError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| BridgeError::invalid_params(format!("'{key}' deve ser booleano"))),
    }
}

pub(crate) fn opt_str<'a>(p: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, BridgeError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.as_str())),
        Some(_) => Err(BridgeError::invalid_params(format!("'{key}' deve ser um texto não vazio"))),
    }
}

/// Decodifica e valida a forma de um `Command` (objeto com `kind`).
pub fn parse_command(v: &Value) -> Result<Command, BridgeError> {
    let o = v.as_object().ok_or_else(|| BridgeError::invalid_params("'command' deve ser um objeto {\"kind\": ...}; veja aiai_schema {\"which\":\"commands\"}"))?;
    if !o.get("kind").map(Value::is_string).unwrap_or(false) {
        return Err(BridgeError::invalid_params("'command.kind' é obrigatório (texto)"));
    }
    serde_json::from_value::<Command>(v.clone()).map_err(|e| BridgeError::invalid_params(format!("command inválido: {e}")))
}

pub(crate) fn outcome_json(session: &CoreSession, o: &aiai_core::Outcome) -> Result<Value, BridgeError> {
    let mut v = serde_json::to_value(o).map_err(|e| BridgeError::failed(e.to_string()))?;
    if let Some(m) = v.as_object_mut() {
        m.insert("can_undo".into(), json!(session.can_undo()));
        m.insert("can_redo".into(), json!(session.can_redo()));
    }
    Ok(v)
}

/// Versão sem caminho de projeto: `SAVE_PROJECT` usa `session.path` (definido por abrir/salvar) e exige `path` se não houver.
pub fn handle_core_action(session: &mut CoreSession, action: &str, params: &Value) -> Option<Result<Value, BridgeError>> {
    dispatch(session, action, params)
}

/// Como [`handle_core_action`], para quem acompanha o caminho do projeto por fora (o headless guarda a pasta `--project`
/// antes mesmo de o projeto existir). `project_path` com valor vira `session.path` antes da ação e recebe `session.path` depois.
/// O app não precisa disto: usa [`handle_core_action`] e lê `session.path`.
pub fn handle_core_action_with_path(session: &mut CoreSession, project_path: &mut Option<PathBuf>, action: &str, params: &Value) -> Option<Result<Value, BridgeError>> {
    if project_path.is_some() {
        session.path = project_path.clone();
    }
    let r = dispatch(session, action, params);
    *project_path = session.path.clone();
    r
}

fn dispatch(session: &mut CoreSession, action: &str, params: &Value) -> Option<Result<Value, BridgeError>> {
    if let Some(r) = crate::core_tools::handle_tool_action(session, action, params) {
        return Some(r);
    }
    Some(match action {
        "PING" => check_keys(params, action, &[]).map(|_| json!({ "pong": true, "name": session.project.name, "revision": session.project.revision })),
        "APPLY_COMMAND" => apply_command(session, params),
        "QUERY" => query(session, params),
        "UNDO" => undo_redo(session, params, true),
        "REDO" => undo_redo(session, params, false),
        "GET_LOG" => get_log(session, params),
        "NEW_PROJECT" => new_project(session, params),
        "OPEN_PROJECT" => open_project(session, params),
        "SAVE_PROJECT" => save_project(session, params),
        _ => return None,
    })
}

fn apply_command(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let p = obj(params)?;
    check_keys(params, "APPLY_COMMAND", &["command", "if_revision", "author"])?;
    let cmd = parse_command(p.get("command").unwrap_or(&Value::Null))?;
    let if_revision = opt_u64(&p, "if_revision")?;
    let author = opt_str(&p, "author")?.unwrap_or(DEFAULT_AUTHOR).to_string();
    let outcome = session.apply_as(cmd, &author, if_revision).map_err(core_error)?;
    outcome_json(session, &outcome)
}

fn undo_redo(session: &mut CoreSession, params: &Value, undo: bool) -> Result<Value, BridgeError> {
    let p = obj(params)?;
    check_keys(params, if undo { "UNDO" } else { "REDO" }, &["author"])?;
    let author = opt_str(&p, "author")?.unwrap_or(DEFAULT_AUTHOR).to_string();
    // `undo`/`redo` registram o autor da sessão: troca só durante a chamada (o autor humano do app não pode mudar).
    let previous = std::mem::replace(&mut session.author, author);
    let r = if undo { session.undo() } else { session.redo() };
    session.author = previous;
    let outcome = r.map_err(core_error)?;
    outcome_json(session, &outcome)
}

fn query(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let p = obj(params)?;
    check_keys(params, "QUERY", &["query"])?;
    let q = p.get("query").ok_or_else(|| BridgeError::invalid_params("'query' é obrigatório: objeto {\"kind\": ...}; veja aiai_schema {\"which\":\"queries\"}"))?;
    if !q.get("kind").map(Value::is_string).unwrap_or(false) {
        return Err(BridgeError::invalid_params("'query.kind' é obrigatório (texto)"));
    }
    let q: Query = serde_json::from_value(q.clone()).map_err(|e| BridgeError::invalid_params(format!("query inválida: {e}")))?;
    q.run(session).map_err(core_error)
}

fn get_log(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let p = obj(params)?;
    check_keys(params, "GET_LOG", &["from", "limit", "include_commands"])?;
    let from = opt_u64(&p, "from")?.unwrap_or(0);
    let limit = opt_u64(&p, "limit")?.unwrap_or(100);
    let include_commands = opt_bool(&p, "include_commands")?.unwrap_or(false);
    Query::Log { from, limit, include_commands }.run(session).map_err(core_error)
}

fn path_param(p: &Map<String, Value>) -> Result<Option<PathBuf>, BridgeError> {
    Ok(opt_str(p, "path")?.map(PathBuf::from))
}

fn new_project(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let p = obj(params)?;
    check_keys(params, "NEW_PROJECT", &["name", "settings", "path", "overwrite"])?;
    let name = opt_str(&p, "name")?.ok_or_else(|| BridgeError::invalid_params("'name' é obrigatório"))?;
    let mut project = Project::new(name);
    if let Some(settings) = p.get("settings").filter(|v| !v.is_null()) {
        let so = settings.as_object().ok_or_else(|| BridgeError::invalid_params("'settings' deve ser um objeto"))?;
        let mut base = serde_json::to_value(&project.settings).map_err(|e| BridgeError::failed(e.to_string()))?;
        for (k, v) in so {
            base[k] = v.clone();
        }
        project.settings = serde_json::from_value(base).map_err(|e| BridgeError::invalid_params(format!("settings inválidas: {e}")))?;
    }
    project.validate().map_err(|e| BridgeError::invalid_params(format!("projeto inválido: {e}")))?;
    let dir = path_param(&p)?;
    if let Some(d) = &dir {
        // Nunca sobrescrever um projeto existente por descuido (o agente pode repetir a chamada).
        if is_project_dir(d) && !opt_bool(&p, "overwrite")?.unwrap_or(false) {
            return Err(BridgeError::failed(format!("já existe um projeto em {}: use OPEN_PROJECT (ou overwrite:true para recomeçar)", d.display())));
        }
    }
    // Grava primeiro numa sessão nova: se salvar falhar, o projeto aberto não é perdido.
    let mut fresh = CoreSession::new(project);
    fresh.ids = session.ids.clone();
    fresh.clock = session.clock.clone();
    fresh.full_validate = session.full_validate;
    if let Some(dir) = &dir {
        persist::save(&mut fresh, dir).map_err(core_error)?;
    }
    session.detach_wal();
    // tudo o que é da pasta nova vem junto — diário, trava, travas e bases dos cortes —, e o do projeto anterior sai
    // (CX18 C18-01: antes as bases dos cortes de um projeto novo dividido ficavam para trás)
    session.adopt_document(fresh);
    session.path = dir.clone();
    Ok(json!({ "name": session.project.name, "revision": session.project.revision, "path": dir.map(|d| d.display().to_string()), "settings": session.project.settings }))
}

fn open_project(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let p = obj(params)?;
    check_keys(params, "OPEN_PROJECT", &["path", "take_over"])?;
    let given = path_param(&p)?.ok_or_else(|| BridgeError::invalid_params("'path' (pasta .aiai, arquivo .aiaiproj ou project.json) é obrigatório"))?;
    let dir = persist::resolve_project_dir(&given).ok_or_else(|| BridgeError::invalid_params(format!("{} não é um projeto do AnimeIAI (pasta .aiai com project.json, ou o arquivo .aiaiproj dela)", given.display())))?;
    // take_over: assumir a trava de outro processo vivo (sem isso o projeto abre só para leitura)
    let take_over = opt_bool(&p, "take_over")?.unwrap_or(false);
    let (opened, report) = persist::open_with(&dir, take_over).map_err(core_error)?;
    if !report.read_only() {
        // projetos de antes do arquivo de abrir ganham o arquivo (melhor esforço: pasta só de leitura abre igual)
        let _ = persist::write_opener(&dir, &opened.project);
    }
    session.detach_wal();
    // mantém o histórico da recuperação (os comandos reaplicados do WAL continuam no undo)
    session.adopt_document(opened);
    session.path = Some(dir.clone());
    Ok(json!({
        "name": session.project.name, "revision": session.project.revision, "path": dir.display().to_string(),
        "wal_replayed": report.replayed, "wal_truncated_at": report.wal_truncated_at, "wal_error": report.replay_error,
        "wal_lines_skipped": report.wal_lines_skipped, "wal_tail_only": report.wal_tail_only,
        "wal_backup": report.wal_backup.as_ref().map(|p| p.display().to_string()),
        "damaged_blobs": report.damaged_blobs.iter().map(|d| json!({ "hash": d.hash.0, "problem": d.problem, "used_by": d.used_by, "kind": d.kind, "missing": d.missing })).collect::<Vec<_>>(),
        "read_only": report.read_only(),
        "locked_by": report.locked_by.as_ref().map(lock_json),
        "replaced_lock": report.replaced_lock.as_ref().map(lock_json),
        "backup": report.backup.as_ref().map(|p| p.display().to_string()),
        "warnings": report.warnings,
    }))
}

/// Trava "aberto em outro lugar" para o JSON de resposta (sem o token).
fn lock_json(i: &aiai_core::lock::LockInfo) -> Value {
    json!({ "pid": i.pid, "host": i.host, "started_ms": i.started_ms, "exe": i.exe, "description": i.describe() })
}

fn save_project(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let p = obj(params)?;
    check_keys(params, "SAVE_PROJECT", &["path"])?;
    let dir = match path_param(&p)? {
        Some(d) => d,
        None => session.path.clone().ok_or_else(|| BridgeError::invalid_params("o projeto ainda não tem pasta: informe 'path' (pasta .aiai)"))?,
    };
    persist::save(session, &dir).map_err(core_error)?;
    Ok(json!({ "path": dir.display().to_string(), "revision": session.project.revision, "name": session.project.name }))
}

/// A pasta é um projeto (tem `project.json`, ou o registro de uma gravação interrompida que abrir termina)?
pub fn is_project_dir(dir: &Path) -> bool {
    persist::is_project_dir(dir)
}

/// Sessão vazia (autor `agent:claude`) para o headless e para testes que ainda não abriram nada.
pub fn empty_session() -> CoreSession {
    let mut s = CoreSession::new(Project::new("sem-titulo"));
    s.author = DEFAULT_AUTHOR.into();
    // tudo o que o headless grava vem de chamada direta de ferramenta (R16 REQ-P03)
    s.source = aiai_core::InputSource::DirectToolCall;
    s
}
