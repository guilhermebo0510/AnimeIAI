//! Preferências (Editar > Preferências): abas verticais Geral, Atalhos, Pastas e Desempenho. Só opções que agem de
//! verdade no programa; o que vale só ao reabrir diz isso ao lado.

use super::StudioApp;
use crate::i18n::{tr, Lang};
use crate::keymap::{Action, Chord, Keymap, ACTIONS};
use crate::prefs::{GpuPref, MAX_ZOOM, MIN_ZOOM};
use crate::theme::ThemeKind;
use egui::{Align2, Event, Key, RichText};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PrefsTab {
    #[default]
    General,
    Keys,
    Folders,
    Performance,
}

impl PrefsTab {
    pub(crate) const ALL: [PrefsTab; 4] = [PrefsTab::General, PrefsTab::Keys, PrefsTab::Folders, PrefsTab::Performance];
    pub(crate) fn key(self) -> &'static str {
        match self {
            PrefsTab::General => "general",
            PrefsTab::Keys => "keys",
            PrefsTab::Folders => "folders",
            PrefsTab::Performance => "performance",
        }
    }
    fn title_key(self) -> &'static str {
        match self {
            PrefsTab::General => "prefs.tab.general",
            PrefsTab::Keys => "prefs.tab.keys",
            PrefsTab::Folders => "prefs.tab.folders",
            PrefsTab::Performance => "prefs.tab.performance",
        }
    }
    pub(crate) fn from_key(k: &str) -> Option<PrefsTab> {
        PrefsTab::ALL.into_iter().find(|t| t.key() == k)
    }
}

/// Teclas fixas (Enter confirma o mover, Tab navega, espaço é a mão temporária): sozinhas não viram atalho.
/// (Ctrl/Shift/Alt sozinhos não geram evento de tecla no egui: a captura espera a tecla de verdade.)
fn is_reserved(k: Key) -> bool {
    matches!(k, Key::Escape | Key::Enter | Key::Tab | Key::Space)
}

impl StudioApp {
    /// Troca o atalho de uma ação (ou limpa com `None`), grava nas preferências e avisa se outra ação perdeu o atalho.
    pub(crate) fn set_shortcut(&mut self, a: Action, chord: Option<Chord>) {
        let lost = self.ed.keys.set(a, chord);
        self.prefs.shortcuts = self.ed.keys.to_overrides();
        self.prefs.shortcuts.extend(self.ed.modeling.keys.to_overrides());
        self.mark_dirty();
        if let Some(o) = lost {
            let c = chord.map(|c| c.label()).unwrap_or_default();
            self.notice(tr("keys.moved").replace("{key}", &c).replace("{from}", tr(o.label_key())).replace("{to}", tr(a.label_key())));
        }
    }

    pub(crate) fn reset_shortcuts(&mut self) {
        self.ed.keys = Keymap::default();
        self.prefs.shortcuts = self.ed.modeling.keys.to_overrides();
        self.mark_dirty();
    }

    /// Captura de atalho em curso: a próxima tecla vira o atalho (Esc cancela, Backspace/Delete limpa).
    fn capture_key(&mut self, ctx: &egui::Context) {
        let Some(a) = self.key_capture else { return };
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        let Some((key, m)) = pressed else { return };
        let plain = !(m.ctrl || m.command || m.shift || m.alt);
        if key == Key::Escape && plain {
            self.key_capture = None;
        } else if matches!(key, Key::Backspace | Key::Delete) && plain {
            self.key_capture = None;
            self.set_shortcut(a, None);
        } else if !(is_reserved(key) && plain) {
            self.key_capture = None;
            self.set_shortcut(a, Some(Chord { ctrl: m.ctrl || m.command, shift: m.shift, alt: m.alt, key }));
        }
    }

    pub(crate) fn prefs_window(&mut self, ctx: &egui::Context) {
        if !self.prefs_open {
            self.key_capture = None;
            return;
        }
        self.capture_key(ctx);
        let t = self.tokens();
        let mut open = true;
        let win = egui::Window::new(tr("prefs.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([620.0, 440.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    // abas verticais (preferência do usuário registrada no ANIGO)
                    ui.vertical(|ui| {
                        ui.set_width(130.0);
                        for tab in PrefsTab::ALL {
                            let r = ui.add_sized([130.0, 28.0], egui::Button::selectable(self.prefs_tab == tab, tr(tab.title_key())));
                            self.rects.note(format!("prefs:tab:{}", tab.key()), r.rect);
                            if r.clicked() {
                                self.prefs_tab = tab;
                                self.key_capture = None;
                            }
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_height(420.0);
                        match self.prefs_tab {
                            PrefsTab::General => self.prefs_general(ui, ctx),
                            PrefsTab::Keys => self.prefs_keys(ui, t),
                            PrefsTab::Folders => self.prefs_folders(ui, t),
                            PrefsTab::Performance => self.prefs_performance(ui, t),
                        }
                        ui.add_space(8.0);
                        let where_ = match &self.prefs_path {
                            Some(p) => format!("{}: {}", tr("prefs.file"), p.display()),
                            None => tr("prefs.no_session").to_owned(),
                        };
                        ui.add(egui::Label::new(RichText::new(where_).size(11.5).color(t.text_dim)).wrap());
                    });
                });
            });
        if let Some(w) = win {
            self.rects.note("dialog:prefs", w.response.rect);
        }
        self.prefs_open = open;
    }

    fn prefs_general(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Grid::new("prefs_grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(tr("prefs.theme"));
            let mut theme_sel = self.prefs.theme;
            egui::ComboBox::from_id_salt("prefs_theme")
                .selected_text(tr(if theme_sel == ThemeKind::Dark { "menu.view.theme.dark" } else { "menu.view.theme.light" }))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut theme_sel, ThemeKind::Dark, tr("menu.view.theme.dark"));
                    ui.selectable_value(&mut theme_sel, ThemeKind::Light, tr("menu.view.theme.light"));
                });
            self.set_theme(ctx, theme_sel);
            ui.end_row();

            ui.label(tr("prefs.language"));
            let mut lang_sel = self.prefs.lang;
            egui::ComboBox::from_id_salt("prefs_lang")
                .selected_text(tr(if lang_sel == Lang::Pt { "lang.pt" } else { "lang.en" }))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut lang_sel, Lang::Pt, tr("lang.pt"));
                    ui.selectable_value(&mut lang_sel, Lang::En, tr("lang.en"));
                });
            self.set_lang(lang_sel);
            ui.end_row();

            ui.label(tr("prefs.zoom"));
            let mut pct = self.prefs.ui_zoom * 100.0;
            let r = ui.add(egui::Slider::new(&mut pct, MIN_ZOOM * 100.0..=MAX_ZOOM * 100.0).suffix("%").step_by(5.0));
            // aplica ao soltar: mudar o zoom enquanto arrasta redimensiona o próprio slider
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                self.set_zoom(ctx, pct / 100.0);
            }
            ui.end_row();

            ui.label(tr("prefs.start"));
            let mut show = self.prefs.show_start;
            if ui.checkbox(&mut show, tr("start.show_on_launch")).changed() {
                self.prefs.show_start = show;
                self.mark_dirty();
            }
            ui.end_row();

            // caneta: curva de pressão (Windows Ink / WM_POINTER chega pelo winit como toque com força)
            ui.label(tr("prefs.pen"));
            ui.vertical(|ui| {
                let (mut min, mut gamma) = (self.prefs.pen_pressure_min, self.prefs.pen_pressure_gamma);
                let a = ui.add(egui::Slider::new(&mut min, 0.0..=0.6).text(tr("prefs.pen_min")));
                let b = ui.add(egui::Slider::new(&mut gamma, 0.3..=3.0).logarithmic(true).text(tr("prefs.pen_gamma")));
                self.rects.note("prefs:pen_min", a.rect);
                self.rects.note("prefs:pen_gamma", b.rect);
                if a.changed() || b.changed() {
                    self.prefs.pen_pressure_min = min;
                    self.prefs.pen_pressure_gamma = gamma;
                    self.mark_dirty();
                }
                // desenho da curva (força na horizontal, pressão do traço na vertical)
                let (r, painter) = ui.allocate_painter(egui::vec2(140.0, 70.0), egui::Sense::hover());
                let t = self.tokens();
                painter.rect_stroke(r.rect, 2.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                let pts: Vec<egui::Pos2> = (0..=32)
                    .map(|i| {
                        let f = i as f32 / 32.0;
                        let p = crate::prefs::pressure_curve(f, min, gamma);
                        egui::pos2(r.rect.left() + f * r.rect.width(), r.rect.bottom() - p * r.rect.height())
                    })
                    .collect();
                painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, t.accent)));
                ui.label(egui::RichText::new(tr(if self.ed.pen_seen { "prefs.pen_seen" } else { "prefs.pen_none" })).size(11.0).color(t.text_dim));
            });
            ui.end_row();

            ui.label(tr("prefs.cursor"));
            let mut precise = self.prefs.cursor_precise;
            let c = ui.checkbox(&mut precise, tr("prefs.cursor_precise"));
            self.rects.note("prefs:cursor_precise", c.rect);
            if c.changed() {
                self.prefs.cursor_precise = precise;
                self.mark_dirty();
            }
            ui.end_row();
        });
    }

    fn prefs_keys(&mut self, ui: &mut egui::Ui, t: crate::theme::Tokens) {
        ui.label(RichText::new(tr("keys.help")).size(11.5).color(t.text_dim));
        ui.add_space(4.0);
        let mut set: Option<(Action, Option<Chord>)> = None;
        let mut capture: Option<Action> = None;
        egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("keys_grid").num_columns(3).spacing([12.0, 4.0]).striped(true).show(ui, |ui| {
                for (a, id, label_key, default) in ACTIONS {
                    ui.label(tr(label_key));
                    let capturing = self.key_capture == Some(a);
                    let text = if capturing {
                        RichText::new(tr("keys.press")).color(t.accent)
                    } else {
                        match self.ed.keys.get(a) {
                            Some(c) => RichText::new(c.label()).monospace(),
                            None => RichText::new(tr("keys.none")).color(t.text_dim),
                        }
                    };
                    let r = ui.add_sized([150.0, 22.0], egui::Button::selectable(capturing, text)).on_hover_text(tr("keys.click_to_change"));
                    self.rects.note(format!("keys:chord:{id}"), r.rect);
                    if r.clicked() {
                        capture = Some(a);
                    }
                    let differs = self.ed.keys.get(a) != Some(default);
                    let rr = ui.add_enabled(differs, egui::Button::new(tr("keys.default")).small()).on_hover_text(default.label());
                    self.rects.note(format!("keys:reset:{id}"), rr.rect);
                    if rr.clicked() {
                        set = Some((a, Some(default)));
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(a) = capture {
            self.key_capture = if self.key_capture == Some(a) { None } else { Some(a) };
        }
        if let Some((a, c)) = set {
            self.set_shortcut(a, c);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let r = ui.button(tr("keys.reset_all"));
            self.rects.note("keys:reset_all", r.rect);
            if r.clicked() {
                self.reset_shortcuts();
            }
            let n = self.ed.keys.conflicts().len();
            let (txt, col) = if n == 0 { (tr("keys.no_conflicts").to_owned(), t.text_dim) } else { (tr("keys.conflicts").replace("{n}", &n.to_string()), t.st_retake) };
            ui.label(RichText::new(txt).color(col));
        });
        // contexto Modelagem (perfil AnimeIAI ou Blender/ANIGO)
        let mut msg = None;
        let rects = &mut self.rects;
        if super::modeling::keys::prefs_section(ui, &t, &mut self.ed.modeling.keys, &mut |n, r| rects.note(n, r), &mut msg) {
            self.prefs.shortcuts = self.ed.keys.to_overrides();
            self.prefs.shortcuts.extend(self.ed.modeling.keys.to_overrides());
            self.mark_dirty();
        }
        if let Some(m) = msg {
            self.notice(m);
        }
    }

    fn prefs_folders(&mut self, ui: &mut egui::Ui, t: crate::theme::Tokens) {
        ui.label(tr("prefs.projects_dir"));
        let shown = match &self.prefs.projects_dir {
            Some(p) => p.display().to_string(),
            None => format!("{} ({})", super::doc::default_projects_dir().map(|p| p.display().to_string()).unwrap_or_default(), tr("prefs.default")),
        };
        ui.add(egui::Label::new(RichText::new(shown).monospace()).truncate());
        ui.horizontal(|ui| {
            let r = ui.add_enabled(!self.dialog_open(), egui::Button::new(tr("prefs.choose")));
            self.rects.note("prefs:projects_dir", r.rect);
            if r.clicked() {
                self.request_projects_dir();
            }
            let rr = ui.add_enabled(self.prefs.projects_dir.is_some(), egui::Button::new(tr("keys.default")));
            if rr.clicked() {
                self.prefs.projects_dir = None;
                self.mark_dirty();
            }
        });
        ui.label(RichText::new(tr("prefs.projects_dir_help")).size(11.5).color(t.text_dim));
    }

    fn prefs_performance(&mut self, ui: &mut egui::Ui, t: crate::theme::Tokens) {
        ui.label(tr("prefs.gpu"));
        let mut g = self.prefs.gpu;
        egui::ComboBox::from_id_salt("prefs_gpu")
            .width(260.0)
            .selected_text(tr(if g == GpuPref::Dedicated { "prefs.gpu.dedicated" } else { "prefs.gpu.integrated" }))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut g, GpuPref::Dedicated, tr("prefs.gpu.dedicated"));
                ui.selectable_value(&mut g, GpuPref::Integrated, tr("prefs.gpu.integrated"));
            });
        if g != self.prefs.gpu {
            self.prefs.gpu = g;
            self.mark_dirty();
        }
        let now = self.gpu_name.clone().unwrap_or_else(|| "—".into());
        ui.label(RichText::new(format!("{}: {now}", tr("prefs.gpu.current"))).size(11.5).color(t.text_dim));
        ui.label(RichText::new(tr("prefs.restart")).size(11.5).color(t.text_dim));
        self.prefs_vram(ui, t);
    }
}
