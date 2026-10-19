//! Documento do app: a `CoreSession` (a MESMA que a bridge usa), "sujo"/título da janela, Arquivo (Novo/Abrir/Salvar/Salvar
//! como) e Editar (Desfazer/Refazer). Tudo passa por `aiai_bridge::core_ops`, o mesmo caminho do MCP e do headless, então
//! o app e o agente nunca divergem. Os diálogos nativos (`rfd`) rodam em outra thread: a UI nunca trava esperando o usuário.

use super::StudioApp;
use crate::i18n::tr;
use aiai_bridge::core_ops::handle_core_action;
use egui::{Align2, Key};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Ação que espera o usuário decidir sobre as alterações não salvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pending {
    New,
    Open,
    /// Abrir um recente da tela inicial (o caminho fica em `start.open_path`).
    OpenPath,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogKind {
    Open,
    SaveAs,
    /// Pasta de projetos das Preferências.
    ProjectsDir,
    /// Arquivo > Importar imagem (vira camada de referência no corte ativo).
    ImportImage,
    /// Arquivo > Exportar quadro (PNG do quadro atual, como o RENDER).
    ExportFrame,
    /// Arquivo > Exportar PSD em camadas (quadro atual).
    ExportPsd,
    ExportSvg,
    ExportOtio,
    /// Arquivo > Exportar timesheet XDTS (corte ativo).
    ExportXdts,
}

const IMAGE_EXTS: [&str; 8] = ["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "gif"];

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

    /// Projeto novo e vazio (sem pasta até o primeiro Salvar). Vindo da tela inicial, com o nome, formato e taxa escolhidos.
    pub(crate) fn doc_new(&mut self) {
        let params = match self.start.request.take() {
            Some(r) => json!({ "name": r.name, "settings": { "width": r.width, "height": r.height, "fps_num": r.fps_num, "fps_den": r.fps_den } }),
            None => json!({ "name": tr("doc.untitled") }),
        };
        match handle_core_action(&mut self.core, "NEW_PROJECT", &params) {
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
            Some(Ok(v)) => {
                self.mark_saved();
                self.note_recent();
                let replayed = v["wal_replayed"].as_u64().unwrap_or(0);
                if super::autosave::is_recovery(path) {
                    // cópia de recuperação: o documento volta sem pasta (Salvar pergunta onde) e conta como não salvo
                    self.core.detach_wal();
                    self.core.path = None;
                    self.saved_log_len = usize::MAX;
                    self.recovered_from = Some(path.to_path_buf());
                    self.recoveries.retain(|p| p != path);
                    self.notice(tr("msg.recovered_copy").to_owned());
                } else if replayed > 0 {
                    // recuperado do diário: o disco ainda não tem esses comandos no project.json, então não está salvo
                    self.saved_log_len = usize::MAX;
                    self.notice(format!("{}: {} ({replayed} {})", tr("msg.opened"), self.core.project.name, tr("msg.recovered")));
                } else {
                    self.notice(format!("{}: {}", tr("msg.opened"), self.core.project.name));
                }
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
                self.note_recent();
                self.notice(tr("msg.saved").to_owned());
                if let Some(r) = self.recovered_from.take() {
                    let _ = std::fs::remove_dir_all(r);
                }
                self.autosave.discard();
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

    /// Abrir uma pasta conhecida (recente): pergunta antes se houver alterações não salvas.
    pub(crate) fn request_open_path(&mut self, path: PathBuf) {
        self.start.open_path = Some(path);
        if self.doc_dirty() {
            self.confirm = Some(Pending::OpenPath);
        } else {
            self.run_pending(Pending::OpenPath);
        }
    }

    /// Autosave do projeto sem pasta (a cada quadro; a cópia roda noutra thread).
    pub(crate) fn autosave_tick(&mut self) {
        let needed = self.core.path.is_none() && self.doc_dirty();
        if self.core.path.is_some() && self.autosave.wrote {
            self.autosave.discard();
        }
        if let Some(e) = self.autosave.tick(needed, &self.core.project, &self.core.blobs) {
            self.doc_error("autosave", e);
        }
    }

    /// O projeto atual (com pasta) vai para o topo dos recentes.
    fn note_recent(&mut self) {
        if let Some(p) = self.core.path.clone() {
            self.prefs.push_recent(&p);
            self.mark_dirty();
        }
    }

    /// Executa a ação que estava esperando a decisão sobre as alterações.
    fn run_pending(&mut self, p: Pending) {
        match p {
            Pending::New => self.doc_new(),
            Pending::Open => self.spawn_dialog(DialogKind::Open, None),
            Pending::OpenPath => {
                if let Some(p) = self.start.open_path.take() {
                    self.doc_open_path(&p);
                }
            }
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
            DialogKind::ProjectsDir => tr("prefs.projects_dir"),
            DialogKind::ImportImage => tr("menu.file.import_image"),
            DialogKind::ExportFrame => tr("menu.file.export_frame"),
            DialogKind::ExportPsd => tr("menu.file.export_psd"),
            DialogKind::ExportSvg => tr("menu.file.export_svg"),
            DialogKind::ExportOtio => tr("menu.file.export_otio"),
            DialogKind::ExportXdts => tr("menu.file.export_xdts"),
        }
        .to_owned();
        let cut_name = self.ed.cut.as_ref().and_then(|c| self.core.project.node(c)).map(|n| n.name.clone()).unwrap_or_else(|| "corte".into());
        let (psd_name, xdts_name) = (format!("{cut_name}_q{:03}.psd", self.ed.frame + 1), format!("{cut_name}.xdts"));
        let svg_name = format!("{cut_name}_q{:03}.svg", self.ed.frame + 1);
        let otio_name = format!("{}.otio", self.active_edit().and_then(|e| self.core.project.node(&e)).map(|n| n.name.clone()).unwrap_or_else(|| "edicao".into()));
        let frame_name = format!("{}_q{:03}.png", self.ed.cut.as_ref().and_then(|c| self.core.project.node(c)).map(|n| n.name.clone()).unwrap_or_else(|| "quadro".into()), self.ed.frame + 1);
        let prefs_dir = self.prefs.projects_dir.clone().filter(|d| d.is_dir());
        let start = match kind {
            DialogKind::ProjectsDir => prefs_dir.or_else(default_projects_dir),
            _ => self.core.path.as_deref().and_then(Path::parent).map(Path::to_path_buf).or(prefs_dir).or_else(default_projects_dir),
        };
        let file_name = suggested_folder_name(self.doc_name());
        let spawned = std::thread::Builder::new().name("aiai-file-dialog".into()).spawn(move || {
            let mut d = rfd::FileDialog::new().set_title(title);
            if let Some(dir) = start.filter(|d| d.is_dir()) {
                d = d.set_directory(dir);
            }
            let picked = match kind {
                DialogKind::Open | DialogKind::ProjectsDir => d.pick_folder(),
                DialogKind::SaveAs => d.set_file_name(file_name).save_file(),
                DialogKind::ImportImage => d.add_filter("Imagem", &IMAGE_EXTS).pick_file(),
                DialogKind::ExportFrame => d.add_filter("PNG", &["png"]).set_file_name(frame_name).save_file(),
                DialogKind::ExportPsd => d.add_filter("PSD", &["psd"]).set_file_name(psd_name).save_file(),
                DialogKind::ExportSvg => d.add_filter("SVG", &["svg"]).set_file_name(svg_name).save_file(),
                DialogKind::ExportOtio => d.add_filter("OpenTimelineIO", &["otio"]).set_file_name(otio_name).save_file(),
                DialogKind::ExportXdts => d.add_filter("XDTS", &["xdts"]).set_file_name(xdts_name).save_file(),
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
                        DialogKind::ProjectsDir => {
                            self.prefs.projects_dir = Some(path);
                            self.mark_dirty();
                        }
                        DialogKind::SaveAs => {
                            if self.doc_save_to(Some(&path)) {
                                if let Some(p) = then {
                                    self.run_pending(p);
                                }
                            }
                        }
                        DialogKind::ImportImage => self.import_file(&path),
                        DialogKind::ExportFrame => self.export_frame(&path),
                        DialogKind::ExportPsd => self.export_with("EXPORT_PSD", &path),
                        DialogKind::ExportSvg => self.export_with("EXPORT_SVG", &path),
                        DialogKind::ExportOtio => self.export_otio(&path),
                        DialogKind::ExportXdts => self.export_with("EXPORT_XDTS", &path),
                    }
                }
                ctx.request_repaint();
            }
            Err(crossbeam_channel::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            Err(crossbeam_channel::TryRecvError::Disconnected) => self.dialog = None,
        }
    }

    pub(crate) fn request_import_image(&mut self) {
        self.spawn_dialog(DialogKind::ImportImage, None);
    }

    pub(crate) fn request_export_frame(&mut self) {
        self.spawn_dialog(DialogKind::ExportFrame, None);
    }

    /// Arquivo solto na janela ou escolhido em Importar: imagem vira camada de referência no corte ativo; WAV vira
    /// clipe no cabeçote (trilha SE).
    pub(crate) fn import_file(&mut self, path: &Path) {
        let Some(cut) = self.ed.active_cut(&self.core) else {
            self.notice(tr("err.no_cut").to_owned());
            return;
        };
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let r: Result<String, String> = if ext == "wav" {
            super::actions::run(&mut self.core, "IMPORT_AUDIO", json!({ "path": path.display().to_string(), "cut": cut, "start_frame": self.ed.frame, "track": "sfx" }))
                .map(|_| format!("{}: {stem}", tr("msg.imported")))
        } else if IMAGE_EXTS.contains(&ext.as_str()) {
            // camada de referência nova no topo do corte, com um cel, e a imagem ajustada ao canvas
            let (layer, cel) = (self.core.ids.next(), self.core.ids.next());
            let mut ld = aiai_core::LayerData::new(aiai_core::LayerType::Reference);
            ld.opacity = 0.6;
            let batch = aiai_core::Command::Batch {
                label: Some(format!("{}: {stem}", tr("menu.file.import_image"))),
                commands: vec![
                    aiai_core::Command::CreateNode { id: Some(layer.clone()), parent: Some(cut.clone()), index: None, name: format!("ref: {stem}"), data: aiai_core::NodeKind::Layer(ld) },
                    aiai_core::Command::CreateNode { id: Some(cel), parent: Some(layer.clone()), index: None, name: "1".into(), data: aiai_core::NodeKind::Cel(aiai_core::CelData::new(1)) },
                ],
            };
            super::commands::run(&mut self.core, batch)
                .and_then(|_| super::actions::run(&mut self.core, "IMPORT_IMAGE", json!({ "cut": cut, "layer": layer, "frame": 0, "path": path.display().to_string(), "fit": "contain" })))
                .map(|_| {
                    self.ed.layer = Some(layer.clone());
                    self.ed.show_reference = true;
                    format!("{}: {stem}", tr("msg.imported"))
                })
        } else {
            Err(format!("{}: .{ext}", tr("err.import_type")))
        };
        match r {
            Ok(m) => self.notice(m),
            Err(e) => self.doc_error("importar", e),
        }
    }

    pub(crate) fn request_export(&mut self, kind: DialogKind) {
        self.spawn_dialog(kind, None);
    }

    /// PSD em camadas do quadro atual / XDTS do corte ativo.
    fn export_with(&mut self, action: &str, path: &Path) {
        let Some(cut) = self.ed.active_cut(&self.core) else {
            self.notice(tr("err.no_cut").to_owned());
            return;
        };
        let r = super::actions::run(&mut self.core, action, json!({ "cut": cut, "frame": self.ed.frame, "out": path.display().to_string() }));
        match r {
            Ok(_) => self.notice(format!("{}: {}", tr("msg.exported"), path.display())),
            Err(e) => self.doc_error("exportar", e),
        }
    }

    /// Edição aberta no painel de edição, ou a primeira do projeto.
    pub(crate) fn active_edit(&self) -> Option<super::commands::NodeId> {
        let docs = super::actions::docs_of(&self.core.project, super::actions::DocKind::Edit);
        self.ed.edit.sel_edit.clone().filter(|e| docs.iter().any(|(i, _)| i == e)).or_else(|| docs.first().map(|(i, _)| i.clone()))
    }

    /// OTIO da edição com os WAV numa pasta "<nome>_midia" ao lado (vídeo sem render: o MCP tem `render_video`).
    fn export_otio(&mut self, path: &Path) {
        let Some(edit) = self.active_edit() else {
            self.notice(tr("editing.no_edit").to_owned());
            return;
        };
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "edicao".into());
        let media = path.with_file_name(format!("{stem}_midia"));
        let r = super::actions::run(&mut self.core, "EXPORT_OTIO", json!({ "edit": edit, "out": path.display().to_string(), "media_dir": media.display().to_string() }));
        match r {
            Ok(_) => self.notice(format!("{}: {}", tr("msg.exported"), path.display())),
            Err(e) => self.doc_error("exportar OTIO", e),
        }
    }

    fn export_frame(&mut self, path: &Path) {
        let Some(cut) = self.ed.active_cut(&self.core) else {
            self.notice(tr("err.no_cut").to_owned());
            return;
        };
        let r = super::actions::run(&mut self.core, "RENDER", json!({ "cut": cut, "frame": self.ed.frame, "out": path.display().to_string(), "camera": true }));
        match r {
            Ok(_) => self.notice(format!("{}: {}", tr("msg.exported"), path.display())),
            Err(e) => self.doc_error("exportar quadro", e),
        }
    }

    pub(crate) fn request_projects_dir(&mut self) {
        self.spawn_dialog(DialogKind::ProjectsDir, None);
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

    /// Novo/Abrir/Salvar/Salvar como/Desfazer/Refazer pelo mapa de atalhos (padrão Ctrl+N/O/S/Shift+S/Z/Shift+Z) e Ctrl+Y
    /// fixo para refazer. Desfazer/refazer ficam com o campo de texto quando ele tem o foco; nada durante a captura de
    /// atalho nas Preferências.
    pub(crate) fn doc_shortcuts(&mut self, ctx: &egui::Context) {
        use crate::keymap::Action as A;
        if self.key_capture.is_some() {
            return;
        }
        let keys = self.ed.keys.clone();
        // com um campo de texto focado, Ctrl+Z/Y são dele (o evento não pode ser consumido aqui)
        let text_focus = ctx.egui_wants_keyboard_input();
        let (new, open, save_as, save, redo_shift, redo_y, undo) = ctx.input_mut(|i| {
            let mut hit = |a: A| keys.get(a).is_some_and(|c| c.take(i));
            let (save_as, save, new, open) = (hit(A::FileSaveAs), hit(A::FileSave), hit(A::FileNew), hit(A::FileOpen));
            let (rs, u) = if text_focus { (false, false) } else { (hit(A::EditRedo), hit(A::EditUndo)) };
            let ry = !text_focus && crate::keymap::Chord::ctrl(Key::Y).take(i);
            (new, open, save_as, save, rs, ry, u)
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
            Some(_) => {
                // Cancelar desiste também do pedido da tela inicial (o próximo Novo volta ao padrão)
                self.confirm = None;
                self.start.request = None;
                self.start.open_path = None;
            }
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
pub(crate) fn default_projects_dir() -> Option<PathBuf> {
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
