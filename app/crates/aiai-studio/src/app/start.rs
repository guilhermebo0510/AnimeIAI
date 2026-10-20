//! Janela de projeto (Arquivo > Novo / Ctrl+N; abre sozinha ao iniciar sem `--project`): projeto novo com nome, pasta,
//! formato e taxa de quadros (já nasce salvo na pasta), projetos recentes e Abrir. O projeto novo passa pelo mesmo
//! `NEW_PROJECT` da bridge (`doc_new`), com `settings`, e é salvo na pasta escolhida.

use super::StudioApp;
use crate::brand;
use crate::i18n::tr;
use egui::{Align2, RichText};
use std::path::PathBuf;

/// Formatos de quadro (chave i18n, largura, altura).
pub(crate) const FORMATS: [(&str, u32, u32); 5] = [
    ("start.fmt.hd", 1920, 1080),
    ("start.fmt.uhd", 3840, 2160),
    ("start.fmt.hd720", 1280, 720),
    ("start.fmt.vertical", 1080, 1920),
    ("start.fmt.square", 1080, 1080),
];

/// Taxas de quadros (rótulo, numerador, denominador). 23,976 é o padrão de TV/anime.
pub(crate) const RATES: [(&str, u32, u32); 5] = [("23,976", 24_000, 1001), ("24", 24, 1), ("25", 25, 1), ("29,97", 30_000, 1001), ("30", 30, 1)];

/// Pedido de projeto novo vindo da tela inicial: `doc_new` usa e consome.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NewRequest {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    /// Pasta do projeto (`<pasta escolhida>/<nome>.aiai`): o projeto já nasce salvo nela.
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub(crate) struct StartState {
    pub open: bool,
    pub name: String,
    pub format: usize,
    pub rate: usize,
    pub request: Option<NewRequest>,
    /// Pasta onde a pasta do projeto novo é criada (padrão: a pasta de projetos das Preferências).
    pub parent: Option<PathBuf>,
    /// Recente escolhido esperando a decisão sobre alterações não salvas.
    pub open_path: Option<PathBuf>,
    /// Avisos da última abertura (tiles danificados, diário, trava de outro processo, pincéis) até o usuário fechar.
    pub warn: Option<super::doc::OpenWarnings>,
}

impl StartState {
    pub(crate) fn request(&self) -> NewRequest {
        let (_, width, height) = FORMATS[self.format.min(FORMATS.len() - 1)];
        let (_, fps_num, fps_den) = RATES[self.rate.min(RATES.len() - 1)];
        let name = self.name.trim();
        let name = if name.is_empty() { tr("doc.untitled").to_owned() } else { name.to_owned() };
        let dir = self.parent.as_ref().map(|p| p.join(super::doc::suggested_folder_name(&name)));
        NewRequest { name, width, height, fps_num, fps_den, dir }
    }
}

enum Act {
    Create,
    ChooseParent,
    Open,
    Recent(PathBuf),
    Recover(PathBuf),
    DiscardRecovery(PathBuf),
}

/// Um recente vale se a pasta ainda tem `project.json`.
fn recent_exists(p: &std::path::Path) -> bool {
    p.join("project.json").is_file()
}

impl StudioApp {
    pub(crate) fn start_window(&mut self, ctx: &egui::Context) {
        if !self.start.open {
            return;
        }
        if self.start.name.is_empty() {
            self.start.name = tr("doc.untitled").to_owned();
        }
        // execuções sem sessão (testes, capturas) não sugerem pasta: Criar faz um projeto sem pasta (Salvar pergunta)
        if self.start.parent.is_none() && !self.cli.no_session {
            self.start.parent = self.prefs.projects_dir.clone().filter(|d| d.is_dir()).or_else(super::doc::default_projects_dir);
        }
        let t = self.tokens();
        let mut open = true;
        let mut act: Option<Act> = None;
        let win = egui::Window::new(tr("start.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([600.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let tex = self.icons.texture(ctx, brand::logo_for(self.prefs.theme), 44.0);
                    ui.add(egui::Image::new((tex, egui::vec2(44.0, 44.0))));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(brand::APP_NAME).heading().strong());
                        ui.label(RichText::new(tr("about.tagline")).color(t.text_dim));
                    });
                });
                ui.separator();
                ui.columns(2, |cols| {
                    // ---- projeto novo
                    let ui = &mut cols[0];
                    ui.label(RichText::new(tr("start.new")).strong());
                    ui.add_space(4.0);
                    ui.label(tr("start.name"));
                    let r = ui.add(egui::TextEdit::singleline(&mut self.start.name).desired_width(f32::INFINITY));
                    self.rects.note("start:name", r.rect);
                    ui.add_space(4.0);
                    ui.label(tr("start.format"));
                    let fmt = &mut self.start.format;
                    let r = egui::ComboBox::from_id_salt("start_format")
                        .width(ui.available_width())
                        .selected_text(tr(FORMATS[*fmt].0))
                        .show_ui(ui, |ui| {
                            for (i, (k, _, _)) in FORMATS.iter().enumerate() {
                                let r = ui.selectable_value(fmt, i, tr(k));
                                self.rects.note(format!("start:format:{i}"), r.rect);
                            }
                        });
                    self.rects.note("start:format", r.response.rect);
                    ui.add_space(4.0);
                    ui.label(tr("start.rate"));
                    let rate = &mut self.start.rate;
                    let r = egui::ComboBox::from_id_salt("start_rate")
                        .width(ui.available_width())
                        .selected_text(format!("{} {}", RATES[*rate].0, tr("start.fps")))
                        .show_ui(ui, |ui| {
                            for (i, (label, _, _)) in RATES.iter().enumerate() {
                                let r = ui.selectable_value(rate, i, format!("{label} {}", tr("start.fps")));
                                self.rects.note(format!("start:rate:{i}"), r.rect);
                            }
                        });
                    self.rects.note("start:rate", r.response.rect);
                    let req = self.start.request();
                    ui.label(RichText::new(format!("{} × {} px · {} {}", req.width, req.height, RATES[self.start.rate].0, tr("start.fps"))).size(11.5).color(t.text_dim));
                    ui.add_space(4.0);
                    // pasta: o projeto já nasce salvo (Salvar não pergunta de novo)
                    ui.label(tr("start.folder"));
                    ui.horizontal(|ui| {
                        let shown = self.start.parent.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| tr("start.no_folder").to_owned());
                        let b = ui.button(tr("start.choose_folder"));
                        self.rects.note("start:folder", b.rect);
                        if b.clicked() {
                            act = Some(Act::ChooseParent);
                        }
                        ui.add(egui::Label::new(RichText::new(shown).size(11.5)).truncate());
                    });
                    if let Some(d) = &req.dir {
                        let exists = d.join("project.json").is_file();
                        let (text, color) = if exists { (format!("{}: {}", tr("start.exists"), d.display()), t.danger) } else { (format!("{}: {}", tr("start.will_create"), d.display()), t.text_dim) };
                        ui.add(egui::Label::new(RichText::new(text).size(11.0).color(color)).wrap());
                    }
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        let taken = req.dir.as_ref().is_some_and(|d| d.join("project.json").is_file());
                        let c = ui.add_enabled(!taken, egui::Button::new(tr("start.create")));
                        self.rects.note("start:create", c.rect);
                        if c.clicked() {
                            act = Some(Act::Create);
                        }
                        let o = ui.button(tr("start.open"));
                        self.rects.note("start:open", o.rect);
                        if o.clicked() {
                            act = Some(Act::Open);
                        }
                    });

                    // ---- recentes (antes: cópias de recuperação de uma execução que caiu)
                    let ui = &mut cols[1];
                    if !self.recoveries.is_empty() {
                        ui.label(RichText::new(tr("start.recover")).strong().color(t.st_retake));
                        ui.label(RichText::new(tr("start.recover_tip")).size(11.0).color(t.text_dim));
                        for (i, p) in self.recoveries.iter().enumerate() {
                            let mins = std::fs::metadata(p.join("project.json")).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|d| d.as_secs() / 60).unwrap_or(0);
                            ui.horizontal(|ui| {
                                let r = ui.button(format!("{} ({mins} min)", tr("start.recover_open"))).on_hover_text(p.display().to_string());
                                self.rects.note(format!("start:recover:{i}"), r.rect);
                                if r.clicked() {
                                    act = Some(Act::Recover(p.clone()));
                                }
                                let d = ui.small_button(tr("start.recover_discard"));
                                self.rects.note(format!("start:recover_discard:{i}"), d.rect);
                                if d.clicked() {
                                    act = Some(Act::DiscardRecovery(p.clone()));
                                }
                            });
                        }
                        ui.add_space(8.0);
                    }
                    ui.label(RichText::new(tr("start.recent")).strong());
                    ui.add_space(4.0);
                    if self.prefs.recent.is_empty() {
                        ui.label(RichText::new(tr("start.no_recent")).color(t.text_dim));
                    }
                    egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                        for (i, p) in self.prefs.recent.iter().enumerate() {
                            let exists = recent_exists(p);
                            let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
                            let text = RichText::new(name).strong();
                            let r = ui
                                .add_enabled(exists, egui::Button::new(text).truncate().min_size(egui::vec2(ui.available_width(), 0.0)))
                                .on_hover_text(p.display().to_string())
                                .on_disabled_hover_text(format!("{}\n{}", tr("start.missing"), p.display()));
                            self.rects.note(format!("start:recent:{i}"), r.rect);
                            if r.clicked() {
                                act = Some(Act::Recent(p.clone()));
                            }
                            let dir = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
                            ui.add(egui::Label::new(RichText::new(dir).size(11.0).color(t.text_dim)).truncate());
                            ui.add_space(2.0);
                        }
                    });
                });
                ui.separator();
                let mut show = self.prefs.show_start;
                let r = ui.checkbox(&mut show, tr("start.show_on_launch"));
                self.rects.note("start:show_on_launch", r.rect);
                if show != self.prefs.show_start {
                    self.prefs.show_start = show;
                    self.mark_dirty();
                }
            });
        if let Some(w) = win {
            self.rects.note("dialog:start", w.response.rect);
        }
        self.start.open = open;
        match act {
            Some(Act::Create) => {
                self.start.request = Some(self.start.request());
                self.request_new();
            }
            Some(Act::ChooseParent) => self.request_new_parent(),
            Some(Act::Open) => {
                self.start.open = false;
                self.request_open();
            }
            Some(Act::Recent(p)) => {
                self.start.open = false;
                self.request_open_path(p);
            }
            Some(Act::Recover(p)) => {
                self.start.open = false;
                self.request_open_path(p);
            }
            Some(Act::DiscardRecovery(p)) => {
                if super::autosave::is_recovery(&p) {
                    let _ = std::fs::remove_dir_all(&p);
                }
                self.recoveries.retain(|q| q != &p);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_uses_chosen_format_and_rate() {
        let s = StartState { name: "  Piloto ".into(), format: 3, rate: 4, ..Default::default() };
        assert_eq!(s.request(), NewRequest { name: "Piloto".into(), width: 1080, height: 1920, fps_num: 30, fps_den: 1, dir: None });
        let with_dir = StartState { name: "Piloto".into(), parent: Some(PathBuf::from("C:/proj")), ..Default::default() };
        assert_eq!(with_dir.request().dir, Some(PathBuf::from("C:/proj").join("Piloto.aiai")));
        let d = StartState::default().request();
        assert_eq!((d.width, d.height, d.fps_num, d.fps_den), (1920, 1080, 24_000, 1001));
        assert!(!d.name.is_empty());
    }
}
