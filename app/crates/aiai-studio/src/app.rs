//! `eframe::App` do AnimeIAI: barras (menu, workspaces, status), prateleira de ferramentas, dock por workspace,
//! diálogos (Preferências, Sobre) e o mecanismo de screenshot (`--screenshot`).

use crate::brand;
use crate::cli::Cli;
use crate::i18n::{self, tr, Lang};
use crate::icons::{IconId, Icons};
use crate::layout::{self, PanelKind};
use crate::prefs::{self, Prefs, MAX_ZOOM, MIN_ZOOM};
use crate::sysmem;
use crate::theme::{self, ThemeKind, Tokens, CARD_GAP, CARD_RADIUS, HEADER_H, PANEL_MIN};
use crate::workspace::{ToolId, WorkspaceId};
use egui::{Align, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind};
use egui_tiles::{Behavior, TileId, Tiles, Tree, UiResponse};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Zooms oferecidos no menu Exibir.
const ZOOM_STEPS: [f32; 8] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

/// Quadro de câmera do projeto (a resolução real virá do projeto na S01).
const CAMERA_W: f32 = 1920.0;
const CAMERA_H: f32 = 1080.0;

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
    script: String,
    project_name: Option<String>,
    about_open: bool,
    prefs_open: bool,
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
        let ctx = &cc.egui_ctx;
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
        let project_name = cli.project.as_ref().and_then(|p| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()));
        StudioApp {
            ws: prefs.workspace,
            cli,
            prefs,
            prefs_path,
            prefs_dirty: false,
            trees: HashMap::new(),
            icons: Icons::default(),
            tool: ToolId::default(),
            script: String::new(),
            project_name,
            about_open: false,
            prefs_open: false,
            frame_count: 0,
            last_frame: None,
            frame_ms: 16.7,
            last_active: None,
            mem_bytes: None,
            mem_sampled: None,
            gpu_logged: false,
            screenshot_requested: false,
        }
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
        egui::Panel::top("menubar")
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
    }

    fn soon_item(ui: &mut egui::Ui, label_key: &str) {
        ui.add_enabled(false, egui::Button::new(tr(label_key))).on_disabled_hover_text(tr("menu.soon"));
    }

    fn menu_file(&mut self, ui: &mut egui::Ui) {
        ui.menu_button(tr("menu.file"), |ui| {
            for key in ["menu.file.new", "menu.file.open", "menu.file.save", "menu.file.save_as", "menu.file.export"] {
                Self::soon_item(ui, key);
            }
            ui.separator();
            if ui.button(tr("menu.file.quit")).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                ui.close();
            }
        });
    }

    fn menu_edit(&mut self, ui: &mut egui::Ui) {
        ui.menu_button(tr("menu.edit"), |ui| {
            Self::soon_item(ui, "menu.edit.undo");
            Self::soon_item(ui, "menu.edit.redo");
            ui.separator();
            if ui.button(tr("menu.edit.prefs")).clicked() {
                self.prefs_open = true;
                ui.close();
            }
        });
    }

    fn menu_view(&mut self, ui: &mut egui::Ui) {
        ui.menu_button(tr("menu.view"), |ui| {
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
    }

    fn menu_window(&mut self, ui: &mut egui::Ui) {
        ui.menu_button(tr("menu.window"), |ui| {
            let t = self.tokens();
            for w in WorkspaceId::ALL {
                let tex = self.icons.texture(ui.ctx(), w.icon(), 16.0);
                let selected = self.ws == w;
                let img = egui::Image::new((tex, egui::vec2(16.0, 16.0))).tint(if selected { t.accent } else { t.text_dim });
                if ui.add(egui::Button::image_and_text(img, tr(w.title_key())).selected(selected)).clicked() {
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
    }

    fn menu_help(&mut self, ui: &mut egui::Ui) {
        ui.menu_button(tr("menu.help"), |ui| {
            if ui.button(tr("menu.help.about")).clicked() {
                self.about_open = true;
                ui.close();
            }
        });
    }

    /// Abas de workspace (como no Blender). Se não couberem com rótulo, só o ativo mostra o nome.
    fn workspace_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens();
        egui::Panel::top("wsbar")
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
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens();
        egui::Panel::bottom("statusbar")
            .exact_size(24.0)
            .frame(egui::Frame::NONE.fill(t.bg_header).inner_margin(egui::Margin::symmetric(10, 0)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
                    self.icons.paint(ui.painter(), self.ws.icon(), r, t.text_dim);
                    ui.label(RichText::new(tr(self.ws.title_key())).size(12.0).color(t.text));
                    ui.separator();
                    let project = self.project_name.clone().unwrap_or_else(|| tr("status.no_project").to_owned());
                    ui.label(RichText::new(format!("{}: {}", tr("status.project"), project)).size(12.0).color(t.text_dim));
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(self.fps_text()).size(12.0).color(t.text_dim).monospace());
                        ui.separator();
                        let mem = self.mem_bytes.map(sysmem::format_bytes).unwrap_or_else(|| "—".to_owned());
                        ui.label(RichText::new(format!("{}: {}", tr("status.memory"), mem)).size(12.0).color(t.text_dim));
                    });
                });
            });
    }

    /// Prateleira de ferramentas dos workspaces de canvas (só seleção por enquanto).
    fn tool_shelf(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens();
        egui::Panel::left("toolshelf")
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
    }

    fn workspace_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens();
        let ws = self.ws;
        let tree = self.trees.entry(ws).or_insert_with(|| layout::default_tree(ws));
        let mut behavior = DockBehavior { t, script: &mut self.script, in_tabs: layout::panes_in_tabs(tree) };
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.bg_frame).inner_margin(egui::Margin::same(CARD_GAP as i8))).show(ui, |ui| {
            tree.ui(&mut behavior, ui);
        });
    }

    // ------------------------------------------------------------------------------------------ diálogos

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let t = self.tokens();
        let mut open = true;
        egui::Window::new(tr("about.title"))
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
        self.about_open = open;
    }

    fn prefs_window(&mut self, ctx: &egui::Context) {
        if !self.prefs_open {
            return;
        }
        let t = self.tokens();
        let mut open = true;
        egui::Window::new(tr("prefs.title"))
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
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
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

impl eframe::App for StudioApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.frame_count += 1;
        if !self.gpu_logged {
            self.gpu_logged = true;
            if let Some(rs) = frame.wgpu_render_state() {
                let info = rs.adapter.get_info();
                tracing::info!("GPU: {} ({:?}, {:?})", info.name, info.device_type, info.backend);
            }
        }
        let ctx = ui.ctx().clone();
        self.sample_frame(&ctx);

        self.menu_bar(ui);
        self.workspace_bar(ui);
        self.status_bar(ui);
        if self.ws.has_canvas() {
            self.tool_shelf(ui);
        }
        self.workspace_ui(ui);
        self.about_window(&ctx);
        self.prefs_window(&ctx);
        self.persist();
        self.handle_screenshot(&ctx);
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
    script: &'a mut String,
    in_tabs: HashSet<TileId>,
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

    /// Placeholder do canvas: mesa neutra e o contorno do quadro de câmera 1920x1080, centralizado e escalado.
    fn canvas(&self, ui: &egui::Ui, body: Rect) {
        let p = ui.painter().with_clip_rect(body);
        p.rect_filled(body, CornerRadius { nw: 0, ne: 0, sw: CARD_RADIUS, se: CARD_RADIUS }, self.t.canvas_bg);
        let avail = body.shrink(28.0);
        if avail.width() < 60.0 || avail.height() < 40.0 {
            return;
        }
        let scale = (avail.width() / CAMERA_W).min(avail.height() / CAMERA_H);
        let frame = Rect::from_center_size(avail.center(), egui::vec2(CAMERA_W * scale, CAMERA_H * scale));
        p.rect_filled(frame, 0.0, self.t.canvas_frame_fill);
        p.rect_stroke(frame, 0.0, Stroke::new(1.5, self.t.canvas_frame_stroke), StrokeKind::Outside);
        // marcas de canto do quadro
        let tick = 10.0;
        let s = Stroke::new(2.0, self.t.accent);
        for (c, dx, dy) in [(frame.left_top(), 1.0, 1.0), (frame.right_top(), -1.0, 1.0), (frame.left_bottom(), 1.0, -1.0), (frame.right_bottom(), -1.0, -1.0)] {
            p.line_segment([c, c + egui::vec2(tick * dx, 0.0)], s);
            p.line_segment([c, c + egui::vec2(0.0, tick * dy)], s);
        }
        let label = format!("{}  {:.0} × {:.0}", tr("panel.canvas.camera"), CAMERA_W, CAMERA_H);
        p.text(frame.left_top() + egui::vec2(0.0, -6.0), Align2::LEFT_BOTTOM, label, FontId::proportional(11.5), self.t.text_dim);
        if frame.width() > 260.0 {
            self.empty_state(ui, frame, tr(PanelKind::Canvas.empty_key()));
        }
    }

    /// Área de texto central do Roteiro (placeholder editável; a persistência vem com o projeto, S01).
    fn script(&mut self, ui: &mut egui::Ui, body: Rect) {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body.shrink2(egui::vec2(18.0, 12.0))));
        child.set_clip_rect(body);
        let row_h = child.text_style_height(&egui::TextStyle::Monospace);
        let rows = ((body.height() - 24.0) / row_h).floor().max(3.0) as usize;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
            ui.add(
                egui::TextEdit::multiline(self.script)
                    .frame(egui::Frame::NONE)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows)
                    .hint_text(RichText::new(tr("panel.script.empty")).color(self.t.text_dim)),
            );
        });
    }
}

impl Behavior<PanelKind> for DockBehavior<'_> {
    fn pane_ui(&mut self, ui: &mut egui::Ui, tile_id: TileId, pane: &mut PanelKind) -> UiResponse {
        let rect = ui.max_rect();
        let kind = *pane;
        ui.painter().rect_filled(rect, CARD_RADIUS, self.t.bg_panel);
        let mut resp = UiResponse::None;
        let mut body = rect;
        if !self.in_tabs.contains(&tile_id) {
            resp = self.header(ui, rect, kind, tile_id);
            body.min.y += HEADER_H + 1.0;
        }
        match kind {
            PanelKind::Canvas => self.canvas(ui, body),
            PanelKind::Script => self.script(ui, body),
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
