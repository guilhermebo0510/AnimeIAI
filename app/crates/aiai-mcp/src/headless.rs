//! Modo headless: as ações de documento rodam em processo sobre uma `CoreSession` aberta da pasta `.aiai`
//! (sem app, sem janela), usando exatamente o mesmo `core_ops` que o app usa.
//!
//! * o projeto é salvo (`persist::save`) depois de cada ação que muda o documento — o estado sobrevive entre processos;
//! * o histórico de desfazer/refazer NÃO sobrevive entre processos (cada invocação da CLI abre o projeto do disco);
//!   dentro de um servidor MCP de vida longa a sessão fica em memória e o undo funciona;
//! * se `project.json` mudou no disco por fora (outro processo), a sessão é reaberta (perde o histórico em memória);
//! * toda pasta de projeto passa pelo sandbox de gravação.

use crate::sandbox::Sandbox;
use aiai_bridge::core_ops::{self, handle_core_action_with_path};
use aiai_bridge::{BridgeError, code};
use aiai_core::{CoreSession, persist};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct Headless {
    sandbox: Sandbox,
    project_dir: Option<PathBuf>,
    session: Option<CoreSession>,
    /// mtime do `project.json` logo depois do último abrir/salvar feito por nós.
    stamp: Option<SystemTime>,
}

fn stamp_of(dir: &Path) -> Option<SystemTime> {
    std::fs::metadata(dir.join(persist::PROJECT_FILE)).and_then(|m| m.modified()).ok()
}

impl Headless {
    pub fn new(sandbox: Sandbox, project_dir: Option<PathBuf>) -> Self {
        Headless { sandbox, project_dir, session: None, stamp: None }
    }

    /// Ações que o headless sabe executar (as de documento + um GET_STATUS próprio).
    pub fn handles(action: &str) -> bool {
        core_ops::is_core_action(action) || action == "GET_STATUS"
    }

    pub fn project_dir(&self) -> Option<&Path> {
        self.project_dir.as_deref()
    }

    fn no_project() -> BridgeError {
        BridgeError::new(
            code::ACTION_FAILED,
            "sem app aberto e sem projeto headless: use aiai_project {\"op\":\"open\",\"path\":\"<pasta .aiai>\"} (ou new), \
             ou rode com --project <pasta> / AIAI_HEADLESS_PROJECT",
        )
    }

    fn checked(&self, path: &str) -> Result<PathBuf, BridgeError> {
        self.sandbox.check_write(path).map_err(|e| BridgeError::invalid_params(format!("{e}")))
    }

    /// Garante a sessão carregada da pasta atual (reabre se o disco mudou por fora).
    fn ensure_loaded(&mut self) -> Result<(), BridgeError> {
        let dir = self.project_dir.clone().ok_or_else(Self::no_project)?;
        let dir_s = dir.display().to_string();
        let checked = self.checked(&dir_s)?;
        let fresh = self.session.is_some() && self.stamp.is_some() && stamp_of(&checked) == self.stamp;
        if fresh {
            return Ok(());
        }
        if !core_ops::is_project_dir(&checked) {
            return Err(BridgeError::failed(format!("{} não é um projeto AnimeIAI (falta project.json); crie com aiai_project {{\"op\":\"new\"}}", checked.display())));
        }
        let mut session = core_ops::empty_session();
        let mut path = None;
        handle_core_action_with_path(&mut session, &mut path, "OPEN_PROJECT", &json!({ "path": checked.display().to_string() })).expect("OPEN_PROJECT é do núcleo")?;
        self.stamp = stamp_of(&checked);
        self.project_dir = Some(checked);
        self.session = Some(session);
        Ok(())
    }

    fn save(&mut self) -> Result<(), BridgeError> {
        let dir = self.project_dir.clone().ok_or_else(Self::no_project)?;
        let session = self.session.as_mut().ok_or_else(Self::no_project)?;
        let mut path = Some(dir.clone());
        handle_core_action_with_path(session, &mut path, "SAVE_PROJECT", &Value::Null).expect("SAVE_PROJECT é do núcleo")?;
        self.stamp = stamp_of(&dir);
        Ok(())
    }

    pub fn call(&mut self, action: &str, params: &Value) -> Result<Value, BridgeError> {
        match action {
            "GET_STATUS" => Ok(self.status()),
            "NEW_PROJECT" => self.new_or_open(action, params),
            "OPEN_PROJECT" => self.new_or_open(action, params),
            "SAVE_PROJECT" => {
                self.ensure_loaded()?;
                let mut p = params.clone();
                if let Some(path) = p.get("path").and_then(Value::as_str) {
                    let checked = self.checked(path)?;
                    p["path"] = json!(checked.display().to_string());
                }
                let session = self.session.as_mut().expect("carregada");
                let mut path = self.project_dir.clone();
                let r = handle_core_action_with_path(session, &mut path, action, &p).ok_or_else(|| BridgeError::unknown_action(action))??;
                self.project_dir = path;
                self.stamp = self.project_dir.as_deref().and_then(stamp_of);
                Ok(r)
            }
            other => {
                self.ensure_loaded()?;
                let session = self.session.as_mut().expect("carregada");
                let mut path = self.project_dir.clone();
                let r = handle_core_action_with_path(session, &mut path, other, params).ok_or_else(|| BridgeError::unknown_action(other))??;
                if core_ops::is_mutating(other) {
                    self.save().map_err(|e| BridgeError::failed(format!("aplicado em memória, mas falhou ao salvar o projeto: {}", e.message)))?;
                }
                Ok(r)
            }
        }
    }

    /// NEW_PROJECT / OPEN_PROJECT: `path` (ou a pasta `--project`) é a pasta do projeto.
    fn new_or_open(&mut self, action: &str, params: &Value) -> Result<Value, BridgeError> {
        let mut p = match params {
            Value::Object(_) => params.clone(),
            Value::Null => json!({}),
            _ => return Err(BridgeError::invalid_params("params deve ser um objeto JSON")),
        };
        let raw = match p.get("path").and_then(Value::as_str) {
            Some(s) => s.to_string(),
            None => self.project_dir.as_ref().map(|d| d.display().to_string()).ok_or_else(|| BridgeError::invalid_params("informe 'path' (pasta .aiai) ou rode com --project <pasta>"))?,
        };
        let checked = self.checked(&raw)?;
        p["path"] = json!(checked.display().to_string());
        let mut session = self.session.take().unwrap_or_else(core_ops::empty_session);
        let mut path = self.project_dir.clone();
        let r = handle_core_action_with_path(&mut session, &mut path, action, &p).ok_or_else(|| BridgeError::unknown_action(action))?;
        match r {
            Ok(v) => {
                self.project_dir = path;
                self.stamp = self.project_dir.as_deref().and_then(stamp_of);
                self.session = Some(session);
                Ok(v)
            }
            Err(e) => {
                // falhou: o que estava carregado continua valendo
                if self.project_dir.is_some() {
                    self.session = Some(session);
                }
                Err(e)
            }
        }
    }

    fn status(&mut self) -> Value {
        if self.session.is_none() && self.project_dir.is_some() {
            let _ = self.ensure_loaded();
        }
        match (&self.session, &self.project_dir) {
            (Some(s), Some(dir)) => json!({
                "mode": "headless", "project": dir.display().to_string(), "name": s.project.name, "revision": s.project.revision,
                "can_undo": s.can_undo(), "can_redo": s.can_redo(), "nodes": s.project.nodes.len(),
                "note": "headless: o projeto é salvo após cada mudança; o histórico de undo não sobrevive entre processos",
            }),
            (_, dir) => json!({
                "mode": "headless", "project": dir.as_ref().map(|d| d.display().to_string()), "loaded": false,
                "note": "nenhum projeto carregado: use aiai_project {op:'open'|'new', path}",
            }),
        }
    }
}
