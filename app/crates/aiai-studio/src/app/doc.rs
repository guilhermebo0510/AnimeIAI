//! Documento do app: a `CoreSession` (a MESMA que a bridge usa), "sujo"/título da janela, Arquivo (Novo/Abrir/Salvar/Salvar
//! como) e Editar (Desfazer/Refazer). Tudo passa por `aiai_bridge::core_ops`, o mesmo caminho do MCP e do headless, então
//! o app e o agente nunca divergem. Os diálogos nativos (`rfd`) rodam em outra thread: a UI nunca trava esperando o usuário.

use super::StudioApp;
use crate::i18n::tr;
use aiai_bridge::core_ops::handle_core_action;
use egui::{Align2, Key, Modifiers};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Ação que espera o usuário decidir sobre as alterações não salvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pending {
    New,
    Open,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogKind {
    Open,
    SaveAs,
}

pub(crate) struct DialogJob {
    kind: DialogKind,
    /// Ação a executar depois de salvar com sucesso (Salvar do aviso de alterações não salvas).
    then: Option<Pending>,
    rx: crossbeam_channel::Receiver<Option<PathBuf>>,
}

pub(crate) const AUTHOR: &str = "human";

/// Nome padrão da pasta ao salvar: `<nome>.aiai`, sem caracteres inválidos no Windows.
pub(crate) fn suggested_folder_name(project_name: &str) -> String {
    let clean: String = project_name.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '-' } else { c }).collect();
    let clean = clean.trim().trim_end_matches('.');
    format!("{}.aiai", if clean.is_empty() { "projeto" } else { clean })
}

impl StudioApp {
    // ------------------------------------------------------------------------------------------ estado

    /// Há alterações não salvas? Percorre o log desde o último salvamento: desfazer tudo de volta ao estado salvo volta a
    /// ficar "limpo"; um comando novo depois de desfazer abaixo do ponto salvo o torna inalcançável (sujo para sempre).
    pub(crate) fn doc_dirty(&self) -> bool {
        let log = self.core.log();
        if log.len() < self.saved_log_len {
            return true;
        }
        let mut depth = 0i64;
        for e in &log[self.saved_log_len..] {
            match e.op.as_str() {
                "apply" => {
                    if depth < 0 {
                        return true;
                    }
                    depth += 1;
                }
                "undo" => depth -= 1,
                "redo" => depth += 1,
                _ => return true,
            }
        }
        depth != 0
    }

    pub(crate) fn doc_name(&self) -> &str {
        &self.core.project.name
    }

    /// Título da janela: `AnimeIAI - <projeto>[*]` (o harness encontra a janela pelo prefixo "AnimeIAI").
    pub(crate) fn window_title(&self) -> String {
        format!("{} - {}{}", crate::brand::APP_NAME, self.doc_name(), if self.doc_dirty() { "*" } else { "" })
    }

    /// Mensagem curta na barra de status (some sozinha).
    pub(crate) fn notice(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    fn doc_error(&mut self, what: &str, e: impl std::fmt::Display) {
        tracing::warn!("{what}: {e}");
        self.notice(format!("{}: {e}", tr("msg.error")));
    }

    /// Marca o estado atual como salvo (depois de novo/abrir/salvar com sucesso).
    pub(crate) fn mark_saved(&mut self) {
        self.saved_log_len = self.core.log().len();
    }

    // ------------------------------------------------------------------------------------------ arquivo

    /// Projeto novo e vazio (sem pasta até o primeiro Salvar).
    pub(crate) fn doc_new(&mut self) {
        let name = tr("doc.untitled").to_owned();
        match handle_core_action(&mut self.core, "NEW_PROJECT", &json!({ "name": name })) {
            Some(Ok(_)) => {
                self.mark_saved();
                self.notice(tr("msg.new").to_owned());
            }
            Some(Err(e)) => self.doc_error("novo projeto", e),
            None => {}
        }
    }

    pub(crate) fn doc_open_path(&mut self, path: &Path) {
        match handle_core_action(&mut self.core, "OPEN_PROJECT", &json!({ "path": path.display().to_string() })) {
            Some(Ok(_)) => {
                self.mark_saved();
                self.notice(format!("{}: {}", tr("msg.opened"), self.core.project.name));
            }
            Some(Err(e)) => self.doc_error("abrir projeto", e),
            None => {}
        }
    }

    fn doc_save_to(&mut self, path: Option<&Path>) -> bool {
        let params = match path {
            Some(p) => json!({ "path": p.display().to_string() }),
            None => json!({}),
        };
        match handle_core_action(&mut self.core, "SAVE_PROJECT", &params) {
            Some(Ok(_)) => {
                self.mark_saved();
                self.notice(tr("msg.saved").to_owned());
                true
            }
            Some(Err(e)) => {
                self.doc_error("salvar projeto", e);
                false
            }
            None => false,
        }
    }

    /// Salvar: na pasta atual; sem pasta ainda, pede uma (Salvar como).
    pub(crate) fn request_save(&mut self, then: Option<Pending>) {
        if self.core.path.is_some() {
            if self.doc_save_to(None) {
                if let Some(p) = then {
                    self.run_pending(p);
                }
            }
        } else {
            self.spawn_dialog(DialogKind::SaveAs, then);
        }
    }

    pub(crate) fn request_save_as(&mut self) {
        self.spawn_dialog(DialogKind::SaveAs, None);
    }

    pub(crate) fn request_new(&mut self) {
        if self.doc_dirty() {
            self.confirm = Some(Pending::New);
        } else {
            self.doc_new();
        }
    }

    pub(crate) fn request_open(&mut self) {
        if self.doc_dirty() {
            self.confirm = Some(Pending::Open);
        } else {
            self.spawn_dialog(DialogKind::Open, None);
        }
    }

    /// Executa a ação que estava esperando a decisão sobre as alterações.
    fn run_pending(&mut self, p: Pending) {
        match p {
            Pending::New => self.doc_new(),
            Pending::Open => self.spawn_dialog(DialogKind::Open, None),
            Pending::Quit => {
                self.quit_confirmed = true;
                self.ctx_close = true;
            }
        }
    }

    fn spawn_dialog(&mut self, kind: DialogKind, then: Option<Pending>) {
        if self.dialog.is_some() {
            return; // um diálogo por vez
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        let title = match kind {
            DialogKind::Open => tr("dialog.open_title"),
            DialogKind::SaveAs => tr("dialog.save_title"),
        }
        .to_owned();
        let start = self.core.path.as_deref().and_then(Path::parent).map(Path::to_path_buf).or_else(default_projects_dir);
        let file_name = suggested_folder_name(self.doc_name());
        let spawned = std::thread::Builder::new().name("aiai-file-dialog".into()).spawn(move || {
            let mut d = rfd::FileDialog::new().set_title(title);
            if let Some(dir) = start.filter(|d| d.is_dir()) {
                d = d.set_directory(dir);
            }
            let picked = match kind {
                DialogKind::Open => d.pick_folder(),
                DialogKind::SaveAs => d.set_file_name(file_name).save_file(),
            };
            let _ = tx.send(picked);
        });
        match spawned {
            Ok(_) => self.dialog = Some(DialogJob { kind, then, rx }),
            Err(e) => self.doc_error("diálogo de arquivo", e),
        }
    }

    /// Resultado do diálogo nativo, se já veio.
    pub(crate) fn poll_dialog(&mut self, ctx: &egui::Context) {
        let Some(job) = self.dialog.as_ref() else { return };
        match job.rx.try_recv() {
            Ok(picked) => {
                let (kind, then) = (job.kind, job.then);
                self.dialog = None;
                if let Some(path) = picked {
                    match kind {
                        DialogKind::Open => self.doc_open_path(&path),
                        DialogKind::SaveAs => {
                            if self.doc_save_to(Some(&path)) {
                                if let Some(p) = then {
                                    self.run_pending(p);
                                }
                            }
                        }
                    }
                }
                ctx.request_repaint();
            }
            Err(crossbeam_channel::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            Err(crossbeam_channel::TryRecvError::Disconnected) => self.dialog = None,
        }
    }

    pub(crate) fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    // ------------------------------------------------------------------------------------------ editar

    pub(crate) fn doc_undo(&mut self) {
        let prev = std::mem::replace(&mut self.core.author, AUTHOR.into());
        let r = self.core.undo();
        self.core.author = prev;
        match r {
            Ok(o) => self.notice(o.description),
            Err(e) => self.doc_error("desfazer", e),
        }
    }

    pub(crate) fn doc_redo(&mut self) {
        let prev = std::mem::replace(&mut self.core.author, AUTHOR.into());
        let r = self.core.redo();
        self.core.author = prev;
        match r {
            Ok(o) => self.notice(o.description),
            Err(e) => self.doc_error("refazer", e),
        }
    }

    /// Ctrl+N/O/S/Shift+S, Ctrl+Z, Ctrl+Shift+Z e Ctrl+Y. Desfazer/refazer ficam com o campo de texto quando ele tem o foco.
    pub(crate) fn doc_shortcuts(&mut self, ctx: &egui::Context) {
        let ctrl = Modifiers::COMMAND;
        let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        // com um campo de texto focado, Ctrl+Z/Y são dele (o evento não pode ser consumido aqui)
        let text_focus = ctx.egui_wants_keyboard_input();
        let (new, open, save_as, save, redo_shift, redo_y, undo) = ctx.input_mut(|i| {
            let (rs, ry, u) = if text_focus {
                (false, false, false)
            } else {
                (i.consume_key(ctrl_shift, Key::Z), i.consume_key(ctrl, Key::Y), i.consume_key(ctrl, Key::Z))
            };
            (i.consume_key(ctrl, Key::N), i.consume_key(ctrl, Key::O), i.consume_key(ctrl_shift, Key::S), i.consume_key(ctrl, Key::S), rs, ry, u)
        });
        if new {
            self.request_new();
        }
        if open {
            self.request_open();
        }
        if save_as {
            self.request_save_as();
        } else if save {
            self.request_save(None);
        }
        if redo_shift || redo_y {
            self.doc_redo();
        } else if undo {
            self.doc_undo();
        }
    }

    // ------------------------------------------------------------------------------------------ aviso de alterações

    /// Janela "Alterações não salvas" (Salvar / Descartar / Cancelar) para Novo, Abrir e Sair.
    pub(crate) fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.confirm else { return };
        let mut choice: Option<u8> = None;
        let text = tr("confirm.text").replace("{name}", self.doc_name());
        let r = egui::Window::new(tr("confirm.title"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(text);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let save = ui.button(tr("confirm.save"));
                    let discard = ui.button(tr("confirm.discard"));
                    let cancel = ui.button(tr("confirm.cancel"));
                    self.rects.note("confirm:save", save.rect);
                    self.rects.note("confirm:discard", discard.rect);
                    self.rects.note("confirm:cancel", cancel.rect);
                    if save.clicked() {
                        choice = Some(0);
                    } else if discard.clicked() {
                        choice = Some(1);
                    } else if cancel.clicked() {
                        choice = Some(2);
                    }
                });
            });
        if let Some(r) = r {
            self.rects.note("dialog:confirm", r.response.rect);
        }
        match choice {
            Some(0) => {
                self.confirm = None;
                self.request_save(Some(pending));
            }
            Some(1) => {
                self.confirm = None;
                self.run_pending(pending);
            }
            Some(_) => self.confirm = None,
            None => {}
        }
    }

    /// Fechar a janela com alterações pendentes pergunta antes (a janela só fecha de verdade com `quit_confirmed`).
    pub(crate) fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.doc_dirty() && !self.quit_confirmed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Quit);
        }
        if self.ctx_close {
            self.ctx_close = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Pasta inicial dos diálogos: `producoes/` ao lado do app, se existir.
fn default_projects_dir() -> Option<PathBuf> {
    let p = PathBuf::from(r"C:\AnimeIAI\producoes");
    p.is_dir().then_some(p)
}

/// Ações de arquivo da bridge: depois de uma delas bem-sucedida o estado atual conta como salvo.
pub(crate) fn is_file_action(action: &str) -> bool {
    matches!(action, "NEW_PROJECT" | "OPEN_PROJECT" | "SAVE_PROJECT")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_valid_on_windows() {
        assert_eq!(suggested_folder_name("Sem título"), "Sem título.aiai");
        assert_eq!(suggested_folder_name("a/b:c*d?"), "a-b-c-d-.aiai");
        assert_eq!(suggested_folder_name("  "), "projeto.aiai");
        assert_eq!(suggested_folder_name("fim."), "fim.aiai");
    }
}
