//! `eframe::App` do AnimeIAI: barras (menu, workspaces, status), prateleira de ferramentas, dock por workspace,
//! diálogos (Preferências, Sobre), o documento (`CoreSession`: Arquivo/Editar), a bridge de automação (`--bridge`) e o
//! mecanismo de screenshot (`--screenshot`).
//!
//! Submódulos: `doc` (documento, arquivo, desfazer, atalhos), `bridge_host` (ações da bridge), `input_queue` (entrada
//! sintética), `ui_state` (retângulos, GET_UI_STATE, UI_ACTION), e o editor: `editor` (estado), `commands` (comandos do
//! documento), `render_service` (render em segundo plano), `canvas`, `tools` (ferramentas) e `panels` (Projeto, Camadas,
//! Cores, Linha do tempo, Propriedades).

mod actions;
mod bridge_host;
mod canvas;
mod commands;
pub(crate) mod doc;
mod editor;
mod input_queue;
mod panels;
mod render_service;
mod tools;
mod ui_kit;
mod ui_state;

use crate::brand;
use crate::cli::Cli;
use crate::i18n::{self, tr, Lang};
use crate::icons::{IconId, Icons};
use crate::layout::{self, PanelKind};
use crate::prefs::{self, Prefs, MAX_ZOOM, MIN_ZOOM};
use crate::sysmem;
use crate::theme::{self, ThemeKind, Tokens, CARD_GAP, CARD_RADIUS, HEADER_H, PANEL_MIN};
use crate::workspace::{ToolId, WorkspaceId};
use aiai_core::CoreSession;
use egui::{Align, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind};
use egui_tiles::{Behavior, TileId, Tiles, Tree, UiResponse};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Zooms oferecidos no menu Exibir.
const ZOOM_STEPS: [f32; 8] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];


pub struct StudioApp {
    cli: Cli,
    prefs: Prefs,
    /// `None` com `--no-session` (nada é lido nem gravado).
    prefs_path: Option<PathBuf>,
    prefs_dirty: bool,
    ws: WorkspaceId,
    trees: HashMap<WorkspaceId, Tree<PanelKind>>,
    icons: Icons,
    tool: ToolId,
    about_open: bool,
    prefs_open: bool,
    /// O documento: a mesma `CoreSession` que a bridge usa (autor "human" para as ações do usuário).
    core: CoreSession,
    /// Tamanho do log da sessão no último novo/abrir/salvar: o "sujo" é calculado do que aconteceu depois (ver `doc_dirty`).
    saved_log_len: usize,
    bridge: Option<bridge_host::BridgeHost>,
    input: input_queue::InputQueue,
    rects: ui_state::UiRects,
    ed: editor::Editor,
    dialog: Option<doc::DialogJob>,
    confirm: Option<doc::Pending>,
    notice: Option<(String, Instant)>,
    title: String,
    quit_confirmed: bool,
    ctx_close: bool,
    gpu_name: Option<String>,
    // medidas da barra de status
    frame_count: u64,
    last_frame: Option<Instant>,
    frame_ms: f32,
    last_active: Option<Instant>,
    mem_bytes: Option<u64>,
    mem_sampled: Option<Instant>,
    gpu_logged: bool,
    screenshot_requested: bool,
}

impl StudioApp {
    pub fn new(cc: &eframe::CreationContext<'_>, cli: Cli) -> Self {
        Self::build(&cc.egui_ctx, cli)
    }

    /// Monta o app a partir de um contexto egui (o eframe passa o dele; os testes, um contexto solto).
    pub fn build(ctx: &egui::Context, cli: Cli) -> Self {
        theme::install_fonts(ctx);
        let prefs_path = if cli.no_session { None } else { cli.session_file.clone().map(PathBuf::from).or_else(prefs::default_path) };
        let mut prefs = prefs_path.as_deref().map(prefs::load).unwrap_or_default();
        // flags da linha de comando valem para esta execução (só viram preferência se o usuário mexer depois)
        if let Some(l) = cli.lang {
            prefs.lang = l;
        }
        if let Some(t) = cli.theme {
            prefs.theme = t;
        }
        if let Some(w) = cli.workspace {
            prefs.workspace = w;
        }
        i18n::set_lang(prefs.lang);
        theme::apply(ctx, prefs.theme);
        ctx.set_zoom_factor(prefs.ui_zoom);
        let mut core = CoreSession::new(aiai_core::Project::new(tr("doc.untitled")));
        core.author = doc::AUTHOR.into();
        let bridge = bridge_host::BridgeHost::start(&cli, ctx);
        let mut app = StudioApp {
            ws: prefs.workspace,
            cli,
            prefs,
            prefs_path,
            prefs_dirty: false,
            trees: HashMap::new(),
            icons: Icons::default(),
            tool: ToolId::default(),
            about_open: false,
            prefs_open: false,
            core,
            saved_log_len: 0,
            bridge,
            input: input_queue::InputQueue::default(),
            rects: ui_state::UiRects::default(),
            ed: editor::Editor::default(),
            dialog: None,
            confirm: None,
            notice: None,
            title: String::new(),
            quit_confirmed: false,
            ctx_close: false,
            gpu_name: None,
            frame_count: 0,
            last_frame: None,
            frame_ms: 16.7,
            last_active: None,
            mem_bytes: None,
            mem_sampled: None,
            gpu_logged: false,
            screenshot_requested: false,
        };
        if let Some(dir) = app.cli.project.clone() {
            app.doc_open_path(std::path::Path::new(&dir));
        }
        app
    }

    fn tokens(&self) -> Tokens {
        self.prefs.theme.tokens()
    }

    // ------------------------------------------------------------------------------------------ estado e preferências

    fn mark_dirty(&mut self) {
        self.prefs_dirty = true;
    }

    /// Grava as preferências se algo mudou (nunca com `--no-session`).
    fn persist(&mut self) {
        if !self.prefs_dirty {
            return;
        }
        self.prefs_dirty = false;
        if let Some(p) = &self.prefs_path {
            if let Err(e) = prefs::save(p, &self.prefs) {
                tracing::warn!("falha ao salvar preferências em {}: {e}", p.display());
            }
        }
    }

    fn set_theme(&mut self, ctx: &egui::Context, kind: ThemeKind) {
        if self.prefs.theme != kind {
            self.prefs.theme = kind;
            theme::apply(ctx, kind);
            self.mark_dirty();
        }
    }

    fn set_lang(&mut self, lang: Lang) {
        if self.prefs.lang != lang {
            self.prefs.lang = lang;
            i18n::set_lang(lang);
            self.mark_dirty();
        }
    }

    fn set_zoom(&mut self, ctx: &egui::Context, z: f32) {
        let z = z.clamp(MIN_ZOOM, MAX_ZOOM);
        if (self.prefs.ui_zoom - z).abs() > f32::EPSILON {
            self.prefs.ui_zoom = z;
            ctx.set_zoom_factor(z);
            self.mark_dirty();
        }
    }

    fn switch_workspace(&mut self, w: WorkspaceId) {
        if self.ws != w {
            self.ws = w;
            self.prefs.workspace = w;
            self.mark_dirty();
        }
    }

    fn reset_layout(&mut self, ws: WorkspaceId) {
        self.trees.remove(&ws);
    }

    // ------------------------------------------------------------------------------------------ medidas

    fn sample_frame(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if let Some(prev) = self.last_frame {
            let dt = now.duration_since(prev);
            // intervalos longos são ociosidade (o egui só redesenha quando algo muda), não custo de quadro
            if dt < Duration::from_millis(250) {
                let ms = dt.as_secs_f32() * 1000.0;
                self.frame_ms = if self.last_active.is_some() { self.frame_ms * 0.9 + ms * 0.1 } else { ms };
                self.last_active = Some(now);
            }
        }
        self.last_frame = Some(now);
        if self.mem_sampled.is_none_or(|t| now.duration_since(t) > Duration::from_secs(1)) {
            self.mem_bytes = sysmem::process_memory_bytes();
            self.mem_sampled = Some(now);
        }
        // mantém a memória atualizada com a janela parada
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn fps_text(&self) -> String {
        match self.last_active {
            Some(t) if t.elapsed() < Duration::from_millis(600) && self.frame_ms > 0.0 => format!("{:.0} fps", 1000.0 / self.frame_ms),
            _ => "— fps".to_string(),
        }
    }

    // ------------------------------------------------------------------------------------------ barras

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens();
        let r = egui::Panel::top("menubar")
            .exact_size(28.0)
            .frame(egui::Frame::NONE.fill(t.bg_header).inner_margin(egui::Margin::symmetric(8, 0)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::hover());
                    self.icons.paint(ui.painter(), IconId::Logo, rect, Color32::WHITE);
                    ui.add_space(4.0);
                    egui::MenuBar::new().ui(ui, |ui| {
                        self.menu_file(ui);
                        self.menu_edit(ui);
                        self.menu_view(ui);
                        self.menu_window(ui);
                        self.menu_help(ui);
                    });
                });
            });
        self.rects.note("menubar", r.response.rect);
    }

    /// Item de menu com atalho e retângulo registrado (o harness de mouse real clica pelo `GET_UI_STATE`).
    fn menu_item(&mut self, ui: &mut egui::Ui, name: &str, label_key: &str, shortcut: &str, enabled: bool) -> bool {
        let mut b = egui::Button::new(tr(label_key));
        if !shortcut.is_empty() {
            b = b.shortcut_text(shortcut);
        }
        let r = ui.add_enabled(enabled, b);
        self.rects.note(format!("menu_item:{name}"), r.rect);
        r.clicked()
    }

    fn soon_item(ui: &mut egui::Ui, label_key: &str) {
        ui.add_enabled(false, egui::Button::new(tr(label_key))).on_disabled_hover_text(tr("menu.soon"));
    }

    fn menu_file(&mut self, ui: &mut egui::Ui) {
        let r = ui.menu_button(tr("menu.file"), |ui| {
            if self.menu_item(ui, "file.new", "menu.file.new", "Ctrl+N", true) {
                self.request_new();
                ui.close();
            }
            if self.menu_item(ui, "file.open", "menu.file.open", "Ctrl+O", true) {
                self.request_open();
                ui.close();
            }
            if self.menu_item(ui, "file.save", "menu.file.save", "Ctrl+S", true) {
                self.request_save(None);
                ui.close();
            }
            if self.menu_item(ui, "file.save_as", "menu.file.save_as", "Ctrl+Shift+S", true) {
                self.request_save_as();
                ui.close();
            }
            Self::soon_item(ui, "menu.file.export");
            ui.separator();
            if self.menu_item(ui, "file.quit", "menu.file.quit", "", true) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                ui.close();
            }
        });
        self.rects.note("menu:file", r.response.rect);
    }

    fn menu_edit(&mut self, ui: &mut egui::Ui) {
        let r = ui.menu_button(tr("menu.edit"), |ui| {
            let (can_undo, can_redo) = (self.core.can_undo(), self.core.can_redo());
            if self.menu_item(ui, "edit.undo", "menu.edit.undo", "Ctrl+Z", can_undo) {
                self.doc_undo();
                ui.close();
            }
            if self.menu_item(ui, "edit.redo", "menu.edit.redo", "Ctrl+Shift+Z", can_redo) {
                self.doc_redo();
                ui.close();
            }
            ui.separator();
            if self.menu_item(ui, "edit.prefs", "menu.edit.prefs", "", true) {
                self.prefs_open = true;
                ui.close();
            }
        });
        self.rects.note("menu:edit", r.response.rect);
    }

    fn menu_view(&mut self, ui: &mut egui::Ui) {
        let r = ui.menu_button(tr("menu.view"), |ui| {
            ui.menu_button(tr("menu.view.theme"), |ui| {
                for (k, key) in [(ThemeKind::Light, "menu.view.theme.light"), (ThemeKind::Dark, "menu.view.theme.dark")] {
                    if ui.radio(self.prefs.theme == k, tr(key)).clicked() {
                        let ctx = ui.ctx().clone();
                        self.set_theme(&ctx, k);
                        ui.close();
                    }
                }
            });
            ui.menu_button(tr("menu.view.language"), |ui| {
                for (l, key) in [(Lang::Pt, "lang.pt"), (Lang::En, "lang.en")] {
                    if ui.radio(self.prefs.lang == l, tr(key)).clicked() {
                        self.set_lang(l);
                        ui.close();
                    }
                }
            });
            ui.menu_button(tr("menu.view.zoom"), |ui| {
                for z in ZOOM_STEPS {
                    let on = (self.prefs.ui_zoom - z).abs() < 0.005;
                    if ui.radio(on, format!("{:.0}%", z * 100.0)).clicked() {
                        let ctx = ui.ctx().clone();
                        self.set_zoom(&ctx, z);
                        ui.close();
                    }
                }
            });
        });
        self.rects.note("menu:view", r.response.rect);
    }

    fn menu_window(&mut self, ui: &mut egui::Ui) {
        let r = ui.menu_button(tr("menu.window"), |ui| {
            let t = self.tokens();
            for w in WorkspaceId::ALL {
                let tex = self.icons.texture(ui.ctx(), w.icon(), 16.0);
                let selected = self.ws == w;
                let img = egui::Image::new((tex, egui::vec2(16.0, 16.0))).tint(if selected { t.accent } else { t.text_dim });
                let item = ui.add(egui::Button::image_and_text(img, tr(w.title_key())).selected(selected));
                self.rects.note(format!("menu_item:window.{}", w.key()), item.rect);
                if item.clicked() {
                    self.switch_workspace(w);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(tr("menu.window.reset")).clicked() {
                self.reset_layout(self.ws);
                ui.close();
            }
            if ui.button(tr("menu.window.reset_all")).clicked() {
                self.trees.clear();
                ui.close();
            }
        });
        self.rects.note("menu:window", r.response.rect);
    }

    fn menu_help(&mut self, ui: &mut egui::Ui) {
        let r = ui.menu_button(tr("menu.help"), |ui| {
            if ui.button(tr("menu.help.about")).clicked() {
                self.about_open = true;
                ui.close();
            }
        });
        self.rects.note("menu:help", r.response.rect);
    }

    /// Abas de workspace (como no Blender). Se não couberem com rótulo, só o ativo mostra o nome.
    fn workspace_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens();
        let bar = egui::Panel::top("wsbar")
            .exact_size(34.0)
            .frame(egui::Frame::NONE.fill(t.bg_frame).inner_margin(egui::Margin { left: 6, right: 6, top: 4, bottom: 0 }))
            .show(ui, |ui| {
                let font = FontId::proportional(13.0);
                let pad = 9.0;
                let icon = 16.0;
                let gap = 6.0;
                let widths: Vec<f32> = WorkspaceId::ALL
                    .iter()
                    .map(|w| pad * 2.0 + icon + gap + ui.painter().layout_no_wrap(tr(w.title_key()).to_owned(), font.clone(), Color32::WHITE).size().x)
                    .collect();
                let spacing = 2.0;
                let total: f32 = widths.iter().sum::<f32>() + spacing * (widths.len() as f32 - 1.0);
                let compact = total > ui.available_width();
                ui.spacing_mut().item_spacing.x = spacing;
                ui.horizontal(|ui| {
                    for (w, full) in WorkspaceId::ALL.into_iter().zip(widths) {
                        let selected = self.ws == w;
                        let show_label = !compact || selected;
                        let width = if show_label { full } else { pad * 2.0 + icon };
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 30.0), Sense::click());
                        self.rects.note(format!("ws_tab:{}", w.key()), rect);
                        let hovered = resp.hovered();
                        let p = ui.painter();
                        let top_round = CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 };
                        if selected {
                            p.rect_filled(rect, top_round, t.bg_panel);
                            p.rect_filled(Rect::from_min_size(rect.left_top(), egui::vec2(rect.width(), 2.0)), top_round, t.accent);
                        } else if hovered {
                            p.rect_filled(rect, top_round, t.bg_header);
                        }
                        let ink = if selected { t.text } else if hovered { t.text } else { t.text_dim };
                        let icon_rect = Rect::from_center_size(egui::pos2(rect.left() + pad + icon / 2.0, rect.center().y + 1.0), egui::vec2(icon, icon));
                        self.icons.paint(p, w.icon(), icon_rect, if selected { t.accent } else { ink });
                        if show_label {
                            p.text(egui::pos2(rect.left() + pad + icon + gap, rect.center().y + 1.0), Align2::LEFT_CENTER, tr(w.title_key()), font.clone(), ink);
                        }
                        let resp = if show_label { resp } else { resp.on_hover_text(tr(w.title_key())) };
                        if resp.clicked() {
                            self.switch_workspace(w);
                        }
                    }
                });
            });
        self.rects.note("wsbar", bar.response.rect);
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens();
        let bar = egui::Panel::bottom("statusbar")
            .exact_size(24.0)
            .frame(egui::Frame::NONE.fill(t.bg_header).inner_margin(egui::Margin::symmetric(10, 0)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
                    self.icons.paint(ui.painter(), self.ws.icon(), r, t.text_dim);
                    ui.label(RichText::new(tr(self.ws.title_key())).size(12.0).color(t.text));
                    ui.separator();
                    let project = format!("{}{}", self.doc_name(), if self.doc_dirty() { "*" } else { "" });
                    ui.label(RichText::new(format!("{}: {}  ({} {})", tr("status.project"), project, tr("status.revision"), self.core.project.revision)).size(12.0).color(t.text_dim));
                    if let Some((msg, at)) = &self.notice {
                        if at.elapsed() < Duration::from_secs(6) {
                            ui.separator();
                            ui.label(RichText::new(msg.as_str()).size(12.0).color(t.text));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(self.fps_text()).size(12.0).color(t.text_dim).monospace());
                        ui.separator();
                        let mem = self.mem_bytes.map(sysmem::format_bytes).unwrap_or_else(|| "—".to_owned());
                        ui.label(RichText::new(format!("{}: {}", tr("status.memory"), mem)).size(12.0).color(t.text_dim));
                        if let Some(b) = &self.bridge {
                            ui.separator();
                            ui.label(RichText::new(format!("{} {}", tr("status.bridge"), b.addr())).size(12.0).color(t.accent))
                                .on_hover_text(format!("{} ({})", tr("status.bridge.tip"), b.requests_total()));
                        }
                    });
                });
            });
        self.rects.note("statusbar", bar.response.rect);
    }

    /// Prateleira de ferramentas dos workspaces de canvas (só seleção por enquanto).
    fn tool_shelf(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens();
        let shelf = egui::Panel::left("toolshelf")
            .exact_size(CARD_GAP + 46.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(t.bg_frame).inner_margin(egui::Margin { left: CARD_GAP as i8, right: 0, top: CARD_GAP as i8, bottom: CARD_GAP as i8 }))
            .show(ui, |ui| {
                egui::Frame::NONE.fill(t.bg_panel).corner_radius(CARD_RADIUS).inner_margin(egui::Margin::symmetric(4, 6)).show(ui, |ui| {
                    ui.set_width(38.0);
                    ui.set_min_height(ui.available_height());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.vertical_centered(|ui| {
                        for tool in ToolId::ALL {
                            let selected = self.tool == tool;
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), Sense::click());
                            self.rects.note(format!("tool:{}", tool.key()), rect);
                            let p = ui.painter();
                            if selected {
                                p.rect(rect, 5.0, t.accent_dim, Stroke::new(1.0, t.accent), StrokeKind::Inside);
                            } else if resp.hovered() {
                                p.rect_filled(rect, 5.0, t.bg_hover);
                            }
                            let ink = if selected || resp.hovered() { t.text } else { t.text_dim };
                            self.icons.paint(p, tool.icon(), rect.shrink(7.0), ink);
                            if resp.on_hover_text(tr(tool.title_key())).clicked() {
                                self.tool = tool;
                            }
                        }
                    });
                });
            });
        self.rects.note("toolshelf", shelf.response.rect);
    }

    fn workspace_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens();
        let ws = self.ws;
        let tree = self.trees.entry(ws).or_insert_with(|| layout::default_tree(ws));
        let mut notes: Vec<(String, Rect)> = Vec::new();
        let env = ui_kit::Env { t, core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut notes, tool: &mut self.tool, ws };
        let mut behavior = DockBehavior { t, in_tabs: layout::panes_in_tabs(tree), env };
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.bg_frame).inner_margin(egui::Margin::same(CARD_GAP as i8))).show(ui, |ui| {
            tree.ui(&mut behavior, ui);
        });
        for (name, rect) in notes {
            self.rects.note(name, rect);
        }
        if let Some(msg) = self.ed.toast.take() {
            self.notice(msg);
        }
        if let Some(w) = self.ed.want_workspace.take() {
            self.switch_workspace(w);
        }
    }

    /// Atalhos do editor (ferramentas, tamanho do pincel, Enter/Esc do mover, quadros, ajustar visão).
    fn editor_shortcuts(&mut self, ctx: &egui::Context) {
        if !(self.ws.has_canvas() || matches!(self.ws, WorkspaceId::Storyboard | WorkspaceId::Audio | WorkspaceId::Edicao)) {
            return;
        }
        let t = self.tokens();
        let ws = self.ws;
        let mut notes = Vec::new();
        let mut env = ui_kit::Env { t, core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut notes, tool: &mut self.tool, ws };
        tools::shortcuts(ctx, &mut env);
    }

    // ------------------------------------------------------------------------------------------ diálogos

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let t = self.tokens();
        let mut open = true;
        let win = egui::Window::new(tr("about.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([340.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    let tex = self.icons.texture(ctx, IconId::Logo, 88.0);
                    ui.add(egui::Image::new((tex, egui::vec2(88.0, 88.0))));
                    ui.add_space(8.0);
                    ui.label(RichText::new(brand::APP_NAME).heading().strong());
                    ui.label(RichText::new(format!("{} {}", tr("about.version"), brand::VERSION)).color(t.text_dim));
                    ui.add_space(8.0);
                    ui.label(tr("about.tagline"));
                    ui.add_space(8.0);
                    ui.label(RichText::new(tr("about.credits")).size(11.5).color(t.text_dim));
                    ui.add_space(6.0);
                });
            });
        if let Some(w) = win {
            self.rects.note("dialog:about", w.response.rect);
        }
        self.about_open = open;
    }

    fn prefs_window(&mut self, ctx: &egui::Context) {
        if !self.prefs_open {
            return;
        }
        let t = self.tokens();
        let mut open = true;
        let win = egui::Window::new(tr("prefs.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
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
                });
                ui.add_space(8.0);
                let where_ = match &self.prefs_path {
                    Some(p) => format!("{}: {}", tr("prefs.file"), p.display()),
                    None => tr("prefs.no_session").to_owned(),
                };
                ui.label(RichText::new(where_).size(11.5).color(t.text_dim));
            });
        if let Some(w) = win {
            self.rects.note("dialog:prefs", w.response.rect);
        }
        self.prefs_open = open;
    }

    // ------------------------------------------------------------------------------------------ screenshot

    /// `--screenshot <png> --frames <n>`: espera `n` quadros, pede o screenshot da janela ao egui, salva o PNG e fecha.
    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.cli.screenshot.clone() else { return };
        // em modo de captura a UI desenha continuamente (senão o egui para quando nada muda)
        ctx.request_repaint();
        if !self.screenshot_requested && self.frame_count >= self.cli.frames as u64 {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let shots: Vec<_> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    // capturas pedidas pela bridge (user_data presente) não são do `--screenshot`
                    egui::Event::Screenshot { image, user_data, .. } if user_data.data.is_none() => Some(image.clone()),
                    _ => None,
                })
                .collect()
        });
        for img in shots {
            let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(dir) = std::path::Path::new(&path).parent().filter(|d| !d.as_os_str().is_empty()) {
                let _ = std::fs::create_dir_all(dir);
            }
            match image::save_buffer(&path, &rgba, img.size[0] as u32, img.size[1] as u32, image::ColorType::Rgba8) {
                Ok(()) => eprintln!("screenshot salvo: {path} ({}x{})", img.size[0], img.size[1]),
                Err(e) => eprintln!("falha ao salvar screenshot: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl StudioApp {
    /// Um quadro inteiro da interface (o `eframe::App::ui` só acrescenta o que depende do `Frame`).
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        self.frame_count += 1;
        let ctx = ui.ctx().clone();
        self.sample_frame(&ctx);
        self.bridge_tick(&ctx);
        panels::timeline::tick(&self.core, &mut self.ed, &ctx);
        self.editor_shortcuts(&ctx);
        self.poll_dialog(&ctx);
        self.doc_shortcuts(&ctx);
        self.handle_close_request(&ctx);

        self.menu_bar(ui);
        self.workspace_bar(ui);
        self.status_bar(ui);
        if self.ws.has_canvas() {
            self.tool_shelf(ui);
        }
        self.workspace_ui(ui);
        self.about_window(&ctx);
        self.prefs_window(&ctx);
        self.confirm_window(&ctx);
        self.persist();
        self.handle_screenshot(&ctx);
        let title = self.window_title();
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        if self.notice.as_ref().is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(6)) {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        self.rects.end_frame();
    }
}

impl eframe::App for StudioApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.input.next_events(raw_input);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if !self.gpu_logged {
            self.gpu_logged = true;
            if let Some(rs) = frame.wgpu_render_state() {
                let info = rs.adapter.get_info();
                self.gpu_name = Some(info.name.clone());
                tracing::info!("GPU: {} ({:?}, {:?})", info.name, info.device_type, info.backend);
            }
        }
        self.frame(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.tokens().bg_frame.to_normalized_gamma_f32()
    }

    // O estado de UI do egui (posição de janelas etc.) não é persistido: só `prefs.json`, e com `--no-session` nada.
    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn on_exit(&mut self) {
        self.persist();
    }
}

// -------------------------------------------------------------------------------------------------------- dock

struct DockBehavior<'a> {
    t: Tokens,
    in_tabs: HashSet<TileId>,
    /// Documento, estado do editor, ícones e os retângulos desenhados neste quadro (vão para o `GET_UI_STATE`).
    env: ui_kit::Env<'a>,
}

impl DockBehavior<'_> {
    /// Cabeçalho de um painel sem barra de abas: título + alça de arrastar (todo o cabeçalho).
    fn header(&mut self, ui: &mut egui::Ui, rect: Rect, kind: PanelKind, tile_id: TileId) -> UiResponse {
        let hdr = Rect::from_min_size(rect.min, egui::vec2(rect.width(), HEADER_H));
        let r = CARD_RADIUS;
        ui.painter().rect_filled(hdr, CornerRadius { nw: r, ne: r, sw: 0, se: 0 }, self.t.bg_header);
        ui.painter().hline(hdr.x_range(), hdr.bottom(), Stroke::new(1.0, self.t.border));
        let resp = ui.interact(hdr, ui.id().with(("dock_hdr", tile_id)), Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::Grab);
        ui.painter().text(egui::pos2(hdr.left() + 10.0, hdr.center().y), Align2::LEFT_CENTER, tr(kind.title_key()), FontId::proportional(12.0), self.t.text_dim);
        if resp.drag_started() {
            UiResponse::DragStarted
        } else {
            UiResponse::None
        }
    }

    /// Texto de estado vazio centralizado no corpo do painel (com quebra de linha).
    fn empty_state(&self, ui: &egui::Ui, body: Rect, text: &str) {
        if body.width() < 40.0 || body.height() < 20.0 {
            return;
        }
        let mut job = egui::text::LayoutJob::simple(text.to_owned(), FontId::proportional(13.0), self.t.text_dim, (body.width() - 32.0).max(40.0));
        job.halign = Align::Center;
        let galley = ui.painter().layout_job(job);
        let pos = body.center() - galley.rect.center().to_vec2();
        ui.painter().with_clip_rect(body).galley(pos, galley, self.t.text_dim);
    }
}

impl Behavior<PanelKind> for DockBehavior<'_> {
    fn pane_ui(&mut self, ui: &mut egui::Ui, tile_id: TileId, pane: &mut PanelKind) -> UiResponse {
        let rect = ui.max_rect();
        let kind = *pane;
        self.env.note(format!("panel:{}", kind.key()), rect);
        ui.painter().rect_filled(rect, CARD_RADIUS, self.t.bg_panel);
        let mut resp = UiResponse::None;
        let mut body = rect;
        if !self.in_tabs.contains(&tile_id) {
            resp = self.header(ui, rect, kind, tile_id);
            body.min.y += HEADER_H + 1.0;
        }
        match kind {
            PanelKind::Canvas => canvas::show(&mut self.env, ui, body),
            PanelKind::Layers => panels::layers::show(&mut self.env, ui, body),
            PanelKind::Palette => panels::colors::show(&mut self.env, ui, body),
            PanelKind::Timeline => panels::timeline::show(&mut self.env, ui, body),
            PanelKind::Properties => panels::properties::show(&mut self.env, ui, body),
            PanelKind::Project => panels::project::show(&mut self.env, ui, body),
            PanelKind::Outline => panels::outline::show(&mut self.env, ui, body),
            PanelKind::Script => panels::script::show(&mut self.env, ui, body),
            PanelKind::Shots => panels::shots::show(&mut self.env, ui, body),
            PanelKind::Board => panels::board::show(&mut self.env, ui, body),
            _ => self.empty_state(ui, body, tr(kind.empty_key())),
        }
        ui.painter().rect_stroke(rect, CARD_RADIUS, Stroke::new(1.0, self.t.border), StrokeKind::Inside);
        resp
    }

    fn tab_title_for_pane(&mut self, pane: &PanelKind) -> egui::WidgetText {
        tr(pane.title_key()).into()
    }

    fn is_tab_closable(&self, _tiles: &Tiles<PanelKind>, _tile_id: TileId) -> bool {
        false
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        CARD_GAP
    }

    fn min_size(&self) -> f32 {
        PANEL_MIN
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        HEADER_H + 1.0
    }

    fn tab_bar_color(&self, _visuals: &egui::Visuals) -> Color32 {
        self.t.bg_header
    }

    fn tab_bg_color(&self, _visuals: &egui::Visuals, _tiles: &Tiles<PanelKind>, _tile_id: TileId, state: &egui_tiles::TabState) -> Color32 {
        if state.active {
            self.t.bg_panel
        } else {
            Color32::TRANSPARENT
        }
    }

    fn tab_text_color(&self, _visuals: &egui::Visuals, _tiles: &Tiles<PanelKind>, _tile_id: TileId, state: &egui_tiles::TabState) -> Color32 {
        if state.active {
            self.t.text
        } else {
            self.t.text_dim
        }
    }

    fn tab_outline_stroke(&self, _visuals: &egui::Visuals, _tiles: &Tiles<PanelKind>, _tile_id: TileId, state: &egui_tiles::TabState) -> Stroke {
        if state.active {
            Stroke::new(1.0, self.t.border)
        } else {
            Stroke::NONE
        }
    }

    fn resize_stroke(&self, _style: &egui::Style, state: egui_tiles::ResizeState) -> Stroke {
        match state {
            // o espaço entre painéis já é a divisória; só ganha cor quando o ponteiro está nele
            egui_tiles::ResizeState::Idle => Stroke::NONE,
            egui_tiles::ResizeState::Hovering => Stroke::new(2.0, self.t.accent_dim),
            egui_tiles::ResizeState::Dragging => Stroke::new(2.0, self.t.accent),
        }
    }
}
