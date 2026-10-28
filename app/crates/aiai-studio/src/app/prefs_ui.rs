//! Preferências (Editar > Preferências): lista de categorias à esquerda, como no Clip Studio (Geral, Ferramentas, Tablet,
//! Cursor, Camada e seleção, Tela de pintura, Atalhos, Arquivo e pastas, Desempenho). Só opções que agem de verdade no
//! programa (o que cada uma liga está em `sprints/registro/S25-clip-studio/04-preferencias.md`); o que vale só ao
//! reabrir diz isso ao lado.

use super::StudioApp;
use crate::behavior_prefs::{self as bp, BehaviorPrefs, CopyName, CursorShape, SmallCursor, ToolCursor, UiDensity};
use crate::i18n::{tr, Lang};
use crate::keymap::{Action, Chord, Keymap, ACTIONS};
use crate::prefs::{GpuPref, MAX_ZOOM, MIN_ZOOM};
use crate::theme::ThemeKind;
use egui::{Align2, Event, Key, RichText};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PrefsTab {
    #[default]
    General,
    Tools,
    Tablet,
    Cursor,
    Layer,
    Canvas,
    Keys,
    Folders,
    Performance,
}

impl PrefsTab {
    pub(crate) const ALL: [PrefsTab; 9] = [PrefsTab::General, PrefsTab::Tools, PrefsTab::Tablet, PrefsTab::Cursor, PrefsTab::Layer, PrefsTab::Canvas, PrefsTab::Keys, PrefsTab::Folders, PrefsTab::Performance];
    pub(crate) fn key(self) -> &'static str {
        match self {
            PrefsTab::General => "general",
            PrefsTab::Tools => "tools",
            PrefsTab::Tablet => "tablet",
            PrefsTab::Cursor => "cursor",
            PrefsTab::Layer => "layer",
            PrefsTab::Canvas => "canvas",
            PrefsTab::Keys => "keys",
            PrefsTab::Folders => "folders",
            PrefsTab::Performance => "performance",
        }
    }
    fn title_key(self) -> &'static str {
        match self {
            PrefsTab::General => "prefs.tab.general",
            PrefsTab::Tools => "prefs.tab.tools",
            PrefsTab::Tablet => "prefs.tab.tablet",
            PrefsTab::Cursor => "prefs.tab.cursor",
            PrefsTab::Layer => "prefs.tab.layer",
            PrefsTab::Canvas => "prefs.tab.canvas",
            PrefsTab::Keys => "prefs.tab.keys",
            PrefsTab::Folders => "prefs.tab.files",
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

    /// Copia as preferências de comportamento para o editor e para quem desenha só com o `egui::Context`, e aplica as que
    /// agem no núcleo e no estilo (níveis de desfazer, intervalos de cópia, densidade). Roda a cada quadro e só escreve
    /// quando algo mudou.
    pub(crate) fn sync_behavior(&mut self, ctx: &egui::Context) {
        let b = &self.prefs.behavior;
        if self.ed.beh.prefs != *b {
            self.ed.beh.prefs = b.clone();
        }
        bp::publish(ctx, b);
        if self.core.undo_limit != b.undo_levels as usize {
            self.core.undo_limit = b.undo_levels as usize;
        }
        self.autosave.set_intervals(b.recovery_minutes, b.backup_minutes);
        crate::theme::apply_density(ctx, b.ui_density);
    }

    /// Grava `b` nas preferências se mudou.
    fn set_behavior(&mut self, b: BehaviorPrefs) {
        if b != self.prefs.behavior {
            self.prefs.behavior = b;
            self.mark_dirty();
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
            .fixed_size([760.0, 520.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    // lista de categorias à esquerda (preferência do usuário registrada no ANIGO)
                    ui.vertical(|ui| {
                        ui.set_width(150.0);
                        for tab in PrefsTab::ALL {
                            let r = ui.add_sized([150.0, 28.0], egui::Button::selectable(self.prefs_tab == tab, tr(tab.title_key())));
                            self.rects.note(format!("prefs:tab:{}", tab.key()), r.rect);
                            if r.clicked() {
                                self.prefs_tab = tab;
                                self.key_capture = None;
                            }
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_height(490.0);
                        egui::ScrollArea::vertical().max_height(466.0).auto_shrink([false, false]).id_salt("prefs_scroll").show(ui, |ui| {
                            ui.set_min_width(540.0);
                            match self.prefs_tab {
                                PrefsTab::General => self.prefs_general(ui, ctx),
                                PrefsTab::Tools => self.prefs_tools(ui, t),
                                PrefsTab::Tablet => self.prefs_tablet(ui, t),
                                PrefsTab::Cursor => self.prefs_cursor(ui, t),
                                PrefsTab::Layer => self.prefs_layer(ui, t),
                                PrefsTab::Canvas => self.prefs_canvas(ui, t),
                                PrefsTab::Keys => self.prefs_keys(ui, t),
                                PrefsTab::Folders => self.prefs_folders(ui, t),
                                PrefsTab::Performance => self.prefs_performance(ui, t),
                            }
                        });
                        ui.add_space(4.0);
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
        // o que mudou já vale no desenho deste mesmo quadro (formigas, grade, xadrez)
        bp::publish(ctx, &self.prefs.behavior);
        self.prefs_open = open;
    }

    fn prefs_general(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = self.tokens();
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

            ui.label(tr("prefs.density"));
            let mut b = self.prefs.behavior.clone();
            ui.vertical(|ui| {
                let r = egui::ComboBox::from_id_salt("prefs_density").selected_text(tr(b.ui_density.key())).show_ui(ui, |ui| {
                    for d in UiDensity::ALL {
                        ui.selectable_value(&mut b.ui_density, d, tr(d.key()));
                    }
                });
                self.rects.note("prefs:density", r.response.rect);
                ui.label(RichText::new(tr("prefs.density.help")).size(11.0).color(t.text_dim));
            });
            self.set_behavior(b);
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
        });
    }

    /// Ferramentas: troca temporária segurando a tecla, arrasto mínimo e tamanho da ponta arrastando na tela.
    fn prefs_tools(&mut self, ui: &mut egui::Ui, t: crate::theme::Tokens) {
        let mut b = self.prefs.behavior.clone();
        section(ui, t, "prefs.sec.tool_switch");
        let r = ui.checkbox(&mut b.tool_hold, tr("prefs.tool_hold"));
        self.rects.note("prefs:tool_hold", r.rect);
        ui.add_enabled_ui(b.tool_hold, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr("prefs.tool_hold_ms"));
                let r = ui.add(egui::DragValue::new(&mut b.tool_hold_ms).range(bp::HOLD_MS_RANGE.0..=bp::HOLD_MS_RANGE.1).speed(10.0).suffix(" ms"));
                self.rects.note("prefs:tool_hold_ms", r.rect);
            });
        });
        hint(ui, t, "prefs.tool_hold.help");

        section(ui, t, "prefs.sec.drag");
        ui.horizontal(|ui| {
            ui.label(tr("prefs.min_drag"));
            let r = ui.add(egui::DragValue::new(&mut b.min_drag_px).range(0.0..=bp::MIN_DRAG_MAX).speed(0.2).suffix(" px"));
            self.rects.note("prefs:min_drag", r.rect);
        });
        hint(ui, t, "prefs.min_drag.help");

        section(ui, t, "prefs.sec.size_drag");
        let r = ui.checkbox(&mut b.size_drag, tr("prefs.size_drag"));
        self.rects.note("prefs:size_drag", r.rect);
        ui.add_enabled_ui(b.size_drag, |ui| {
            let r = ui.checkbox(&mut b.size_drag_preview, tr("prefs.size_drag_preview"));
            self.rects.note("prefs:size_drag_preview", r.rect);
        });
        hint(ui, t, "prefs.size_drag.help");
        self.set_behavior(b);
    }

    /// Tablet: o que o app sabe da caneta (Windows Ink), a pressão, a curva e uma área de teste. Sem driver próprio, sem
    /// Wintab: a pressão chega pelo sistema.
    fn prefs_tablet(&mut self, ui: &mut egui::Ui, t: crate::theme::Tokens) {
        let mut b = self.prefs.behavior.clone();
        section(ui, t, "prefs.sec.tablet_service");
        ui.label(tr("prefs.tablet.ink"));
        let (txt, col) = if self.ed.pen_seen { (tr("prefs.pen_seen"), t.accent) } else { (tr("prefs.pen_none"), t.text_dim) };
        let r = ui.add(egui::Label::new(RichText::new(txt).size(12.0).color(col)).wrap());
        self.rects.note("prefs:pen_state", r.rect);
        hint(ui, t, "prefs.tablet.later");

        section(ui, t, "prefs.sec.pressure");
        let r = ui.checkbox(&mut b.pen_pressure, tr("prefs.pen_pressure"));
        self.rects.note("prefs:pen_pressure", r.rect);
        let (mut min, mut gamma) = (self.prefs.pen_pressure_min, self.prefs.pen_pressure_gamma);
        let on = b.pen_pressure;
        ui.add_enabled_ui(on, |ui| {
            // sliders à esquerda, a curva (força na horizontal, pressão do traço na vertical) à direita
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    let a = ui.add(egui::Slider::new(&mut min, 0.0..=0.6).text(tr("prefs.pen_min")));
                    let c = ui.add(egui::Slider::new(&mut gamma, 0.3..=3.0).logarithmic(true).text(tr("prefs.pen_gamma")));
                    self.rects.note("prefs:pen_min", a.rect);
                    self.rects.note("prefs:pen_gamma", c.rect);
                    if a.changed() || c.changed() {
                        self.prefs.pen_pressure_min = min;
                        self.prefs.pen_pressure_gamma = gamma;
                        self.mark_dirty();
                    }
                });
                let (r, painter) = ui.allocate_painter(egui::vec2(96.0, 60.0), egui::Sense::hover());
                painter.rect_stroke(r.rect, 2.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                let pts: Vec<egui::Pos2> = (0..=32)
                    .map(|i| {
                        let f = i as f32 / 32.0;
                        let p = crate::prefs::pressure_curve(f, min, gamma);
                        egui::pos2(r.rect.left() + f * r.rect.width(), r.rect.bottom() - p * r.rect.height())
                    })
                    .collect();
                painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, t.accent)));
            });
        });

        // inclinação e botões da caneta (leitura nativa do Windows Ink, auditoria 4.1d–e)
        let r = ui.checkbox(&mut b.pen_tilt, tr("prefs.pen_tilt"));
        self.rects.note("prefs:pen_tilt", r.rect);
        let r = ui.checkbox(&mut b.pen_barrel_picks, tr("prefs.pen_barrel"));
        self.rects.note("prefs:pen_barrel", r.rect);
        hint(ui, t, "prefs.pen_eraser_end");

        section(ui, t, "prefs.sec.pressure_test");
        self.pressure_pad(ui, t, b.pen_pressure);
        self.set_behavior(b);
    }

    /// Área de teste: o que se desenha aqui engrossa com a pressão pela curva acima (o app lê a força do toque do
    /// Windows Ink; com o mouse a linha fica uniforme).
    fn pressure_pad(&mut self, ui: &mut egui::Ui, t: crate::theme::Tokens, enabled: bool) {
        let ctx = ui.ctx().clone();
        let id = egui::Id::new("prefs_pressure_pad");
        let (resp, painter) = ui.allocate_painter(egui::vec2(380.0, 130.0), egui::Sense::click_and_drag());
        self.rects.note("prefs:pressure_pad", resp.rect);
        painter.rect(resp.rect, 4.0, t.bg_header, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        // força bruta do toque mais recente (o Windows Ink manda `Touch` com `force` junto do ponteiro)
        let last_force = ctx.input(|i| {
            i.events.iter().rev().find_map(|e| match e {
                Event::Touch { force: Some(f), phase, .. } if *phase != egui::TouchPhase::End && *phase != egui::TouchPhase::Cancel => Some(*f),
                _ => None,
            })
        });
        let mut trace: Vec<(egui::Pos2, f32)> = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
        let (min, gamma) = (self.prefs.pen_pressure_min, self.prefs.pen_pressure_gamma);
        let mut live: Option<(f32, f32)> = None;
        if resp.is_pointer_button_down_on() {
            if let Some(p) = resp.interact_pointer_pos().filter(|p| resp.rect.contains(*p)) {
                let raw = last_force.unwrap_or(1.0);
                let pr = if enabled { crate::prefs::pressure_curve(raw, min, gamma) } else { 1.0 };
                live = Some((raw, pr));
                if trace.last().is_none_or(|(q, _)| !q.x.is_finite() || (*q - p).length() >= 1.0) {
                    trace.push((p, pr));
                    if trace.len() > 4000 {
                        trace.remove(0);
                    }
                }
            }
        } else if trace.last().is_some_and(|(p, _)| p.x.is_finite()) {
            // levantou: o próximo traço começa solto do anterior
            trace.push((egui::pos2(f32::NAN, f32::NAN), 0.0));
        }
        let ink = t.text;
        let mut prev: Option<(egui::Pos2, f32)> = None;
        for &(p, w) in &trace {
            if !p.x.is_finite() {
                prev = None;
                continue;
            }
            if let Some((q, qw)) = prev {
                painter.line_segment([q, p], egui::Stroke::new(1.2 + 10.8 * (w + qw) / 2.0, ink));
            }
            painter.circle_filled(p, 0.6 + 5.4 * w, ink);
            prev = Some((p, w));
        }
        if trace.is_empty() {
            painter.text(resp.rect.center(), Align2::CENTER_CENTER, tr("prefs.pressure_pad.hint"), egui::FontId::proportional(12.0), t.text_dim);
        }
        ctx.data_mut(|d| d.insert_temp(id, trace));
        ui.horizontal(|ui| {
            let txt = match live {
                Some((raw, pr)) => tr("prefs.pressure_pad.live").replace("{raw}", &format!("{:.0}", raw * 100.0)).replace("{out}", &format!("{:.0}", pr * 100.0)),
                None => tr("prefs.pressure_pad.idle").to_owned(),
            };
            ui.label(RichText::new(txt).size(11.5).color(t.text_dim));
            let r = ui.small_button(tr("prefs.pressure_pad.clear"));
            self.rects.note("prefs:pressure_pad_clear", r.rect);
            if r.clicked() {
                ctx.data_mut(|d| d.insert_temp(id, Vec::<(egui::Pos2, f32)>::new()));
            }
        });
        if live.is_some() {
            ctx.request_repaint();
        }
    }

    /// Cursor: a forma por tipo de ferramenta e o que mostrar quando a ponta é pequena demais.
    fn prefs_cursor(&mut self, ui: &mut egui::Ui, t: crate::theme::Tokens) {
        let mut b = self.prefs.behavior.clone();
        let mut precise = self.prefs.cursor_precise;
        section(ui, t, "prefs.sec.cursor_shape");
        egui::Grid::new("prefs_cursor_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            for (label, key, v) in [("prefs.cursor.brush", "brush", &mut b.cursor_brush), ("prefs.cursor.pen", "pen", &mut b.cursor_pen), ("prefs.cursor.eraser", "eraser", &mut b.cursor_eraser)] {
                ui.label(tr(label));
                let r = egui::ComboBox::from_id_salt(("prefs_cursor", key)).width(200.0).selected_text(tr(shape_key(*v))).show_ui(ui, |ui| {
                    for s in [CursorShape::Tip, CursorShape::Cross, CursorShape::Dot] {
                        ui.selectable_value(v, s, tr(shape_key(s)));
                    }
                });
                self.rects.note(format!("prefs:cursor:{key}"), r.response.rect);
                ui.end_row();
            }
            ui.label(tr("prefs.cursor.other"));
            let r = egui::ComboBox::from_id_salt("prefs_cursor_other").width(200.0).selected_text(tr(tool_cursor_key(b.cursor_other))).show_ui(ui, |ui| {
                for s in [ToolCursor::Tool, ToolCursor::Cross] {
                    ui.selectable_value(&mut b.cursor_other, s, tr(tool_cursor_key(s)));
                }
            });
            self.rects.note("prefs:cursor:other", r.response.rect);
            ui.end_row();
        });
        section(ui, t, "prefs.sec.cursor_small");
        ui.horizontal(|ui| {
            for s in [SmallCursor::None, SmallCursor::Dot, SmallCursor::Cross] {
                let r = ui.selectable_value(&mut b.cursor_small, s, tr(small_key(s)));
                self.rects.note(format!("prefs:cursor_small:{}", small_key(s)), r.rect);
            }
        });
        hint(ui, t, "prefs.cursor_small.help");
        ui.add_space(8.0);
        let c = ui.checkbox(&mut precise, tr("prefs.cursor_precise"));
        self.rects.note("prefs:cursor_precise", c.rect);
        if precise != self.prefs.cursor_precise {
            self.prefs.cursor_precise = precise;
            self.mark_dirty();
        }
        self.set_behavior(b);
    }

    /// Camada e seleção: nome das cópias e cor das formigas da seleção.
    fn prefs_layer(&mut self, ui: &mut egui::Ui, t: crate::theme::Tokens) {
        let mut b = self.prefs.behavior.clone();
        section(ui, t, "prefs.sec.copy_name");
        ui.horizontal(|ui| {
            ui.label(tr("prefs.copy_name"));
            let sample = |s: CopyName| bp::copy_name_tr(tr("prefs.copy.sample"), s);
            let r = egui::ComboBox::from_id_salt("prefs_copy_name").width(220.0).selected_text(sample(b.copy_name)).show_ui(ui, |ui| {
                for s in [CopyName::Paren, CopyName::Suffix, CopyName::Prefix, CopyName::Number] {
                    ui.selectable_value(&mut b.copy_name, s, sample(s));
                }
            });
            self.rects.note("prefs:copy_name", r.response.rect);
        });
        hint(ui, t, "prefs.copy_name.help");

        section(ui, t, "prefs.sec.selection");
        ui.horizontal(|ui| {
            let mut custom = b.sel_color.is_some();
            let r = ui.checkbox(&mut custom, tr("prefs.sel_color"));
            self.rects.note("prefs:sel_color", r.rect);
            if custom && b.sel_color.is_none() {
                b.sel_color = Some([255, 64, 160]);
            } else if !custom {
                b.sel_color = None;
            }
            if let Some(c) = b.sel_color.as_mut() {
                let r = egui::color_picker::color_edit_button_srgb(ui, c);
                self.rects.note("prefs:sel_color_btn", r.rect);
            }
        });
        // prévia das formigas com a cor escolhida
        let (r, painter) = ui.allocate_painter(egui::vec2(180.0, 56.0), egui::Sense::hover());
        painter.rect_filled(r.rect, 3.0, t.bg_header);
        let q = r.rect.shrink(10.0);
        let phase = (ui.ctx().input(|i| i.time) * 12.0 % 8.0) as f32;
        bp::publish(ui.ctx(), &b);
        super::selection::ants(&painter, &[q.left_top(), q.right_top(), q.right_bottom(), q.left_bottom()], true, phase);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(90));
        hint(ui, t, "prefs.sel_color.help");
        self.set_behavior(b);
    }

    /// Tela de pintura: xadrez da transparência, grade de pixels e passo de giro da visão.
    fn prefs_canvas(&mut self, ui: &mut egui::Ui, t: crate::theme::Tokens) {
        let mut b = self.prefs.behavior.clone();
        let dark = ui.ctx().theme() == egui::Theme::Dark;
        section(ui, t, "prefs.sec.checker");
        ui.horizontal(|ui| {
            let mut custom = b.checker.is_some();
            let r = ui.checkbox(&mut custom, tr("prefs.checker_custom"));
            self.rects.note("prefs:checker_custom", r.rect);
            if custom && b.checker.is_none() {
                let [x, y] = super::canvas::checker_colors(dark);
                b.checker = Some([[x.r(), x.g(), x.b()], [y.r(), y.g(), y.b()]]);
            } else if !custom {
                b.checker = None;
            }
            if let Some([a, c]) = b.checker.as_mut() {
                let r1 = egui::color_picker::color_edit_button_srgb(ui, a);
                let r2 = egui::color_picker::color_edit_button_srgb(ui, c);
                self.rects.note("prefs:checker_a", r1.rect);
                self.rects.note("prefs:checker_b", r2.rect);
            }
        });
        hint(ui, t, "prefs.checker.help");

        section(ui, t, "prefs.sec.grid");
        ui.horizontal(|ui| {
            ui.label(tr("prefs.grid_color"));
            let r = egui::color_picker::color_edit_button_srgb(ui, &mut b.grid_color);
            self.rects.note("prefs:grid_color", r.rect);
            ui.label(tr("prefs.grid_alpha"));
            let r = ui.add(egui::Slider::new(&mut b.grid_alpha, 4..=255));
            self.rects.note("prefs:grid_alpha", r.rect);
            if ui.small_button(tr("keys.default")).clicked() {
                b.grid_color = BehaviorPrefs::default().grid_color;
                b.grid_alpha = BehaviorPrefs::default().grid_alpha;
            }
        });
        hint(ui, t, "prefs.grid.help");

        section(ui, t, "prefs.sec.rotate");
        ui.horizontal(|ui| {
            ui.label(tr("prefs.rotate_step"));
            let r = ui.add(egui::DragValue::new(&mut b.rotate_step_deg).range(1.0..=90.0).speed(0.5).suffix("°"));
            self.rects.note("prefs:rotate_step", r.rect);
        });
        self.set_behavior(b);
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
        // cópias automáticas (recuperação do projeto sem pasta; backup do documento do projeto com pasta)
        let mut b = self.prefs.behavior.clone();
        section(ui, t, "prefs.sec.autosave");
        egui::Grid::new("prefs_autosave_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label(tr("prefs.recovery_minutes"));
            let r = ui.add(egui::DragValue::new(&mut b.recovery_minutes).range(bp::AUTOSAVE_RANGE.0..=bp::AUTOSAVE_RANGE.1).suffix(format!(" {}", tr("prefs.minutes"))));
            self.rects.note("prefs:recovery_minutes", r.rect);
            ui.end_row();
            ui.label(tr("prefs.backup_minutes"));
            let r = ui.add(egui::DragValue::new(&mut b.backup_minutes).range(bp::AUTOSAVE_RANGE.0..=bp::AUTOSAVE_RANGE.1).suffix(format!(" {}", tr("prefs.minutes"))));
            self.rects.note("prefs:backup_minutes", r.rect);
            ui.end_row();
        });
        hint(ui, t, "prefs.autosave.help");
        self.set_behavior(b);
    }

    fn prefs_performance(&mut self, ui: &mut egui::Ui, t: crate::theme::Tokens) {
        let mut b = self.prefs.behavior.clone();
        section(ui, t, "prefs.sec.undo");
        ui.horizontal(|ui| {
            ui.label(tr("prefs.undo_levels"));
            let r = ui.add(egui::DragValue::new(&mut b.undo_levels).range(bp::UNDO_RANGE.0..=bp::UNDO_RANGE.1).speed(5.0));
            self.rects.note("prefs:undo_levels", r.rect);
        });
        hint(ui, t, "prefs.undo_levels.help");
        self.set_behavior(b);
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

/// Título de seção dentro de uma categoria.
fn section(ui: &mut egui::Ui, t: crate::theme::Tokens, key: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(tr(key)).strong().color(t.text));
    ui.separator();
}

/// Texto de ajuda pequeno e apagado.
fn hint(ui: &mut egui::Ui, t: crate::theme::Tokens, key: &str) {
    ui.add(egui::Label::new(RichText::new(tr(key)).size(11.5).color(t.text_dim)).wrap());
}

fn shape_key(s: CursorShape) -> &'static str {
    match s {
        CursorShape::Tip => "prefs.cursor.shape.tip",
        CursorShape::Cross => "prefs.cursor.shape.cross",
        CursorShape::Dot => "prefs.cursor.shape.dot",
    }
}

fn tool_cursor_key(s: ToolCursor) -> &'static str {
    match s {
        ToolCursor::Tool => "prefs.cursor.other.tool",
        ToolCursor::Cross => "prefs.cursor.shape.cross",
    }
}

fn small_key(s: SmallCursor) -> &'static str {
    match s {
        SmallCursor::None => "prefs.cursor.small.none",
        SmallCursor::Dot => "prefs.cursor.shape.dot",
        SmallCursor::Cross => "prefs.cursor.shape.cross",
    }
}
