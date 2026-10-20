//! `eframe::App` do AnimeIAI: barras (menu, workspaces, status), prateleira de ferramentas, dock por workspace,
//! diálogos (Preferências, Sobre), o documento (`CoreSession`: Arquivo/Editar), a bridge de automação (`--bridge`) e o
//! mecanismo de screenshot (`--screenshot`).
//!
//! Submódulos: `doc` (documento, arquivo, desfazer, atalhos), `bridge_host` (ações da bridge), `input_queue` (entrada
//! sintética), `ui_state` (retângulos, GET_UI_STATE, UI_ACTION), e o editor: `editor` (estado), `commands` (comandos do
//! documento), `render_service` (render em segundo plano), `canvas`, `tools` (ferramentas) e `panels` (Projeto, Camadas,
//! Cores, Linha do tempo, Propriedades).

mod actions;
mod anim;
mod audio_out;
mod autosave;
mod brushes;
mod bridge_host;
mod canvas;
mod commands;
mod comp;
mod comp_viewer;
pub(crate) mod doc;
mod prefs_ui;
mod start;
mod editor;
mod gpu_evict;
mod gpu_owners;
mod input_queue;
mod library;
mod live;
mod model3d;
mod modeling;
mod move_live;
mod nodes;
mod panels;
mod pen;
mod render_service;
mod rulers;
mod selection;
mod text_tool;
mod tools;
mod ui_kit;
mod ui_state;

use crate::brand;
use crate::cli::Cli;
use crate::i18n::{self, tr, Lang};
use crate::icons::{IconId, Icons};
use crate::layout::{self, PanelKind};
use crate::layout_store::{self, LayoutStore};
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

/// Largura reservada à direita da barra de status (bridge, memória, fps): o aviso nunca passa por cima.
const STATUS_RIGHT_W: f32 = 360.0;

pub struct StudioApp {
    cli: Cli,
    prefs: Prefs,
    /// `None` com `--no-session` (nada é lido nem gravado).
    prefs_path: Option<PathBuf>,
    prefs_dirty: bool,
    ws: WorkspaceId,
    /// Ferramenta do quadro anterior (troca de ferramenta traz o painel Pincéis).
    /// (ferramenta, área de trabalho) do quadro anterior: só troca de ferramenta na mesma área traz o painel.
    last_tool: Option<(crate::workspace::ToolId, WorkspaceId)>,
    trees: HashMap<WorkspaceId, Tree<PanelKind>>,
    /// Layouts guardados (`layouts.json`): a árvore em uso de cada workspace, a revisão de fábrica vista e os com nome.
    layouts: LayoutStore,
    /// `None` com `--no-session` (nada é lido nem gravado).
    layouts_path: Option<PathBuf>,
    /// Última mudança ainda não gravada no arquivo (grava 500 ms depois da última, e ao sair).
    layouts_dirty_at: Option<Instant>,
    /// Diálogo "Salvar layout atual como…" aberto, com o nome digitado.
    layout_name: Option<String>,
    icons: Icons,
    tool: ToolId,
    about_open: bool,
    prefs_open: bool,
    start: start::StartState,
    prefs_tab: prefs_ui::PrefsTab,
    /// Ação cujo atalho está sendo capturado nas Preferências (os atalhos do app param enquanto isso).
    key_capture: Option<crate::keymap::Action>,
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
    /// Cópia periódica do projeto ainda sem pasta (ver `autosave`).
    autosave: autosave::Autosave,
    /// Cópias de execuções que caíram (oferecidas na tela inicial).
    recoveries: Vec<PathBuf>,
    /// O documento veio de uma cópia de recuperação: some depois de salvo numa pasta de verdade.
    recovered_from: Option<PathBuf>,
    /// Memória de vídeo: contabilidade, orçamento e pressão (`gpu_mem`, `gpu_evict`).
    gpu: crate::gpu_mem::GpuMem,
    gpu_seen: gpu_owners::Seen,
    gpu_pressure_seen: crate::gpu_mem::Pressure,
}

impl StudioApp {
    pub fn new(cc: &eframe::CreationContext<'_>, cli: Cli) -> Self {
        // ícones pequeno e grande da janela, na grade de pixels e na variante do tema do Windows (brand.rs)
        crate::brand::init_window_icons(cc);
        Self::build(&cc.egui_ctx, cli)
    }

    /// Monta o app a partir de um contexto egui (o eframe passa o dele; os testes, um contexto solto).
    pub fn build(ctx: &egui::Context, cli: Cli) -> Self {
        crate::splash::status(tr("splash.fonts"));
        theme::install_fonts(ctx);
        let prefs_path = if cli.no_session { None } else { cli.session_file.clone().map(PathBuf::from).or_else(prefs::default_path) };
        let mut prefs = prefs_path.as_deref().map(prefs::load).unwrap_or_default();
        let layouts_path = layout_store::path_for(cli.no_session, cli.session_file.as_deref());
        let layouts = layouts_path.as_deref().map(layout_store::load).unwrap_or_default();
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
        let cli_no_autosave = cli.no_session || cli.screenshot.is_some();
        let mut app = StudioApp {
            ws: prefs.workspace,
            cli,
            prefs,
            prefs_path,
            prefs_dirty: false,
            trees: HashMap::new(),
            layouts,
            layouts_path,
            layouts_dirty_at: None,
            layout_name: None,
            last_tool: None,
            icons: Icons::default(),
            tool: ToolId::default(),
            about_open: false,
            prefs_open: false,
            start: start::StartState::default(),
            prefs_tab: prefs_ui::PrefsTab::default(),
            key_capture: None,
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
            autosave: autosave::Autosave::new(!cli_no_autosave),
            recoveries: Vec::new(),
            recovered_from: None,
            gpu: Default::default(),
            gpu_seen: Default::default(),
            gpu_pressure_seen: Default::default(),
            frame_count: 0,
            last_frame: None,
            frame_ms: 16.7,
            last_active: None,
            mem_bytes: None,
            mem_sampled: None,
            gpu_logged: false,
            screenshot_requested: false,
        };
        app.ed.keys = crate::keymap::Keymap::from_overrides(&app.prefs.shortcuts);
        app.fit_tool_to_workspace();
        // pincéis de fábrica + os .aiaibrush da pasta do usuário (sem sessão: só os de fábrica, nada é lido nem gravado)
        crate::splash::status(tr("splash.library"));
        app.ed.brushes = brushes::BrushLibrary::open(!app.cli.no_session, &app.prefs.brush_author);
        // biblioteca de recursos em Documentos\AnimeIAI (com --no-session nada é lido nem gravado, salvo AIAI_LIBRARY_DIR)
        aiai_assets::enable_user_folders(!app.cli.no_session);
        if let Some(dir) = app.cli.project.clone() {
            // diário (wal.ndjson) com comandos: a abertura vai reaplicá-los
            let has_wal = std::fs::metadata(std::path::Path::new(&dir).join("wal.ndjson")).is_ok_and(|m| m.len() > 0);
            crate::splash::status(tr(if has_wal { "splash.wal" } else { "splash.open_project" }));
            app.doc_open_path(std::path::Path::new(&dir));
        } else {
            // sem projeto na linha de comando: tela inicial (nunca nas execuções de teste/captura)
            app.start.open = app.prefs.show_start && !app.cli.no_session && app.cli.screenshot.is_none();
        }
        // som ao vivo mudo nas execuções de teste/captura (AIAI_AUDIO=1 liga mesmo assim)
        app.ed.live = audio_out::AudioOut::new(cli_no_autosave);
        // workspace Áudio: mesma regra de som; predefinições de rack só com sessão
        app.ed.audio_ws.player.muted = cli_no_autosave && !std::env::var("AIAI_AUDIO").is_ok_and(|v| v == "1");
        app.ed.audio_ws.user_files = !app.cli.no_session;
        app.ed.comp.user_files = !app.cli.no_session;
        app.ed.m3d.user_files = !app.cli.no_session;
        app.ed.modeling.user_files = !app.cli.no_session;
        if !cli_no_autosave {
            if let Some(root) = autosave::recovery_root() {
                app.recoveries = autosave::leftovers(&root, app.autosave.dir.as_deref());
                // trabalho de uma execução que caiu: a tela inicial abre para oferecer a recuperação
                if !app.recoveries.is_empty() && app.cli.project.is_none() {
                    app.start.open = true;
                }
            }
        }
        app.gpu_startup_markers();
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
            self.fit_tool_to_workspace();
        }
    }

    /// A ferramenta ativa que não existe na prateleira do workspace troca pela padrão dele (a Composição não fica com o
    /// balde da Pintura escolhido sem mostrar).
    fn fit_tool_to_workspace(&mut self) {
        let p = self.ws.profile();
        if p.canvas && !p.allows(self.tool) {
            self.tool = p.default_tool;
        }
    }

    // ------------------------------------------------------------------------------------------ layouts do dock

    /// Garante a árvore do workspace em `trees`: a guardada em `layouts.json` (se ainda tem painel conhecido) ou a de fábrica.
    pub(crate) fn ensure_tree(&mut self, ws: WorkspaceId) {
        if self.trees.contains_key(&ws) {
            return;
        }
        let saved = self.layouts.get(ws).and_then(|e| e.current.as_ref()).and_then(|s| layout::tree_from_saved(ws, s));
        let tree = saved.unwrap_or_else(|| layout::default_tree_v(ws, self.layouts.variant(ws)));
        self.trees.insert(ws, tree);
    }

    /// Animação: a fase do corte ativo (Genga/Douga) escolhe a variante do layout. Ao trocar, a árvore da variante que
    /// sai fica guardada (cada uma tem o seu layout personalizado) e a da outra entra.
    fn sync_layout_variant(&mut self) {
        let ws = self.ws;
        if ws != WorkspaceId::Animacao {
            return;
        }
        let want = anim::layout_variant(&self.core, &mut self.ed);
        if self.layouts.variant(ws) == want {
            return;
        }
        if let Some(tree) = self.trees.get(&ws) {
            if self.layouts.record_current(ws, layout::saved_from_tree(tree)) {
                self.layouts_dirty_at = Some(Instant::now());
            }
        }
        self.layouts.set_variant(ws, want);
        self.trees.remove(&ws);
        self.ensure_tree(ws);
    }

    /// Mudanças na árvore do workspace atual (arrastar, fechar, redimensionar) vão para o `layouts.json`; o arquivo é
    /// gravado 500 ms depois da última mudança (nunca com `--no-session`).
    fn layouts_tick(&mut self) {
        let ws = self.ws;
        if let Some(tree) = self.trees.get(&ws) {
            let saved = layout::saved_from_tree(tree);
            if self.layouts.record_current(ws, saved) {
                self.layouts_dirty_at = Some(Instant::now());
            }
        }
        if self.layouts_dirty_at.is_some_and(|t| t.elapsed() >= Duration::from_millis(500)) {
            self.save_layouts();
        }
    }

    fn save_layouts(&mut self) {
        self.layouts_dirty_at = None;
        if let Some(p) = &self.layouts_path {
            if let Err(e) = layout_store::save(p, &self.layouts) {
                tracing::warn!("falha ao salvar os layouts em {}: {e}", p.display());
                self.notice(format!("{} {}: {e}", tr("msg.layout_save_failed"), p.display()));
            }
        }
    }

    fn layouts_changed(&mut self) {
        self.layouts_dirty_at = Some(Instant::now());
    }

    /// Volta o workspace ao layout de fábrica (os layouts com nome ficam).
    fn reset_layout(&mut self, ws: WorkspaceId) {
        self.trees.insert(ws, layout::default_tree_v(ws, self.layouts.variant(ws)));
        self.layouts.reset(ws);
        self.layouts_changed();
    }

    /// Guarda a árvore em uso do workspace atual com um nome.
    pub(crate) fn layout_save_named(&mut self, name: &str) -> Result<bool, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("nome vazio".into());
        }
        let ws = self.ws;
        self.ensure_tree(ws);
        let saved = self.trees.get(&ws).and_then(layout::saved_from_tree).ok_or("árvore vazia")?;
        let replaced = self.layouts.save_named(ws, name, saved);
        self.layouts_changed();
        self.notice(format!("{} {name}", tr(if replaced { "msg.layout_replaced" } else { "msg.layout_saved" })));
        Ok(replaced)
    }

    /// Aplica um layout com nome do workspace atual.
    pub(crate) fn layout_apply(&mut self, name: &str) -> Result<(), String> {
        let ws = self.ws;
        let node = self.layouts.named(ws, name).cloned().ok_or_else(|| format!("layout desconhecido neste workspace: {name}"))?;
        let Some(tree) = layout::tree_from_saved(ws, &node) else {
            self.notice(tr("msg.layout_bad"));
            return Err(tr("msg.layout_bad").to_owned());
        };
        self.trees.insert(ws, tree);
        self.layouts.entry(ws).active_name = Some(name.to_owned());
        self.layouts_changed();
        self.notice(format!("{} {name}", tr("msg.layout_applied")));
        Ok(())
    }

    pub(crate) fn layout_delete(&mut self, name: &str) -> bool {
        let ok = self.layouts.delete_named(self.ws, name);
        if ok {
            self.layouts_changed();
            self.notice(format!("{} {name}", tr("msg.layout_deleted")));
        }
        ok
    }

    /// Mostra ou esconde um painel do catálogo no workspace atual; mostrar um painel que não está na árvore insere o
    /// painel (junto da mesma categoria ou pelo `dock_hint`). O canvas não se esconde nem entra onde não há canvas.
    pub(crate) fn set_panel_visible(&mut self, kind: PanelKind, on: bool) -> Result<bool, String> {
        let ws = self.ws;
        if kind.meta().needs_canvas && !ws.profile().canvas {
            return Err(format!("o painel '{}' precisa de canvas e o workspace {} não tem", kind.key(), ws.key()));
        }
        if kind == PanelKind::Canvas && !on {
            return Err("o canvas não se esconde".into());
        }
        self.ensure_tree(ws);
        let Some(tree) = self.trees.get_mut(&ws) else { return Err("sem árvore".into()) };
        match (layout::find_pane(tree, kind), on) {
            (_, true) => {
                layout::insert_panel(tree, kind);
            }
            (Some(id), false) => tree.tiles.set_visible(id, false),
            (None, false) => {}
        }
        Ok(on)
    }

    /// Faixa "o padrão foi atualizado" no topo do workspace (só quando o usuário tem layout próprio mais velho que a fábrica).
    fn layout_update_banner(&mut self, ui: &mut egui::Ui) {
        let ws = self.ws;
        if !self.layouts.factory_update_pending(ws) {
            return;
        }
        let t = self.tokens();
        let (mut use_new, mut keep) = (false, false);
        let r = egui::Panel::top("layout_update")
            .exact_size(30.0)
            .frame(egui::Frame::NONE.fill(t.bg_header).inner_margin(egui::Margin::symmetric(10, 0)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new(tr("msg.layout_update")).size(12.0).color(t.text));
                    let a = ui.button(tr("msg.layout_update.use"));
                    self.rects.note("layout_update:use", a.rect);
                    use_new = a.clicked();
                    let b = ui.button(tr("msg.layout_update.keep"));
                    self.rects.note("layout_update:keep", b.rect);
                    keep = b.clicked();
                });
            });
        self.rects.note("layout_update", r.response.rect);
        if use_new {
            // o layout do usuário fica guardado com nome antes de a fábrica entrar
            let name = tr("msg.layout_previous").to_owned();
            if let Some(cur) = self.layouts.get(ws).and_then(|e| e.current.clone()) {
                self.layouts.save_named(ws, &name, cur);
            }
            self.reset_layout(ws);
        } else if keep {
            self.layouts.entry(ws).factory_rev_seen = layout::factory_rev(ws);
            self.layouts_changed();
        }
    }

    /// Diálogo "Salvar layout atual como…": `Enter` salva, `Esc` fecha.
    fn layout_name_window(&mut self, ctx: &egui::Context) {
        let Some(mut name) = self.layout_name.take() else { return };
        let mut open = true;
        let (mut save, mut cancel) = (false, false);
        let win = egui::Window::new(tr("dialog.layout_name.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([360.0, 0.0])
            .show(ctx, |ui| {
                ui.label(tr("dialog.layout_name.label"));
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
                self.rects.note("layout_name:text", r.rect);
                if !r.has_focus() && !r.lost_focus() {
                    r.request_focus();
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let b = ui.add_enabled(!name.trim().is_empty(), egui::Button::new(tr("dialog.layout_name.save")));
                    self.rects.note("layout_name:save", b.rect);
                    save = b.clicked() || (enter && !name.trim().is_empty());
                    let c = ui.button(tr("confirm.cancel"));
                    self.rects.note("layout_name:cancel", c.rect);
                    cancel = c.clicked();
                });
            });
        if let Some(w) = win {
            self.rects.note("dialog:layout_name", w.response.rect);
        }
        if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        if save {
            let _ = self.layout_save_named(&name);
        } else if open {
            self.layout_name = Some(name);
        }
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
                    self.icons.paint(ui.painter(), IconId::LogoSmall, rect, Color32::WHITE);
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

    fn menu_file(&mut self, ui: &mut egui::Ui) {
        let r = ui.menu_button(tr("menu.file"), |ui| {
            if self.menu_item(ui, "file.new", "menu.file.new", &self.ed.keys.label(crate::keymap::Action::FileNew), true) {
                self.open_new_project();
                ui.close();
            }
            if self.menu_item(ui, "file.open", "menu.file.open", &self.ed.keys.label(crate::keymap::Action::FileOpen), true) {
                self.request_open();
                ui.close();
            }
            if self.menu_item(ui, "file.save", "menu.file.save", &self.ed.keys.label(crate::keymap::Action::FileSave), true) {
                self.request_save(None);
                ui.close();
            }
            if self.menu_item(ui, "file.save_as", "menu.file.save_as", &self.ed.keys.label(crate::keymap::Action::FileSaveAs), true) {
                self.request_save_as();
                ui.close();
            }
            ui.separator();
            if self.menu_item(ui, "file.import_image", "menu.file.import_image", "", true) {
                self.request_import_image();
                ui.close();
            }
            if self.menu_item(ui, "file.export_frame", "menu.file.export_frame", "", self.ed.cut.is_some()) {
                self.request_export_frame();
                ui.close();
            }
            if self.menu_item(ui, "file.export_psd", "menu.file.export_psd", "", self.ed.cut.is_some()) {
                self.request_export(doc::DialogKind::ExportPsd);
                ui.close();
            }
            if self.menu_item(ui, "file.export_xdts", "menu.file.export_xdts", "", self.ed.cut.is_some()) {
                self.request_export(doc::DialogKind::ExportXdts);
                ui.close();
            }
            if self.menu_item(ui, "file.export_svg", "menu.file.export_svg", "", self.ed.cut.is_some()) {
                self.request_export(doc::DialogKind::ExportSvg);
                ui.close();
            }
            let has_edit = self.active_edit().is_some();
            if self.menu_item(ui, "file.export_otio", "menu.file.export_otio", "", has_edit) {
                self.request_export(doc::DialogKind::ExportOtio);
                ui.close();
            }
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
            if self.menu_item(ui, "edit.undo", "menu.edit.undo", &self.ed.keys.label(crate::keymap::Action::EditUndo), can_undo) {
                self.doc_undo();
                ui.close();
            }
            if self.menu_item(ui, "edit.redo", "menu.edit.redo", &self.ed.keys.label(crate::keymap::Action::EditRedo), can_redo) {
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
            // todos os painéis do catálogo, por categoria: marcar mostra (o que não está no layout entra junto da mesma
            // categoria ou no lado do `dock_hint`), desmarcar esconde
            let r = ui.menu_button(tr("menu.view.panels"), |ui| {
                let ws = self.ws;
                self.ensure_tree(ws);
                let canvas_ws = ws.profile().canvas;
                let mut change: Option<(PanelKind, bool)> = None;
                if self.menu_item(ui, "view.reset_layout", "menu.window.reset", "", true) {
                    self.reset_layout(self.ws);
                    self.notice(tr("msg.layout_reset"));
                    ui.close();
                }
                ui.separator();
                // o catálogo inteiro não cabe numa tela baixa: a lista rola
                let max_h = (ui.ctx().content_rect().height() - 140.0).max(200.0);
                egui::ScrollArea::vertical().max_height(max_h).min_scrolled_height(max_h).auto_shrink([true, true]).show(ui, |ui| {
                    for cat in layout::PanelCategory::ALL {
                        let kinds: Vec<PanelKind> = PanelKind::ALL.into_iter().filter(|k| k.meta().category == cat).collect();
                        if kinds.is_empty() {
                            continue;
                        }
                        ui.add_space(2.0);
                        ui.label(RichText::new(tr(cat.title_key())).size(11.0).color(self.tokens().text_dim));
                        for k in kinds {
                            let tree = &self.trees[&ws];
                            let mut on = layout::find_pane(tree, k).is_some_and(|id| tree.tiles.is_visible(id));
                            let meta = k.meta();
                            // o canvas não se esconde; painel que precisa de canvas não entra onde não há canvas
                            let enabled = k != PanelKind::Canvas && (!meta.needs_canvas || canvas_ws);
                            let suggested = meta.workspaces.contains(&ws);
                            let text = if suggested { RichText::new(tr(k.title_key())) } else { RichText::new(tr(k.title_key())).color(self.tokens().text_dim) };
                            let r = ui.add_enabled(enabled, egui::Checkbox::new(&mut on, text));
                            self.rects.note(format!("menu_item:view.panel.{}", k.key()), r.rect);
                            let r = if !enabled && meta.needs_canvas && !canvas_ws { r.on_disabled_hover_text(tr("menu.view.panels.needs_canvas")) } else if suggested { r.on_hover_text(tr("menu.view.panels.suggested")) } else { r };
                            if r.changed() {
                                change = Some((k, on));
                            }
                        }
                    }
                });
                if let Some((k, on)) = change {
                    if let Err(e) = self.set_panel_visible(k, on) {
                        self.notice(e);
                    }
                }
            });
            self.rects.note("menu_item:view.panels", r.response.rect);
            ui.separator();
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
            let mut group = None;
            for (i, w) in WorkspaceId::ALL.into_iter().enumerate() {
                if group.is_some_and(|g| g != w.group()) {
                    ui.separator();
                }
                group = Some(w.group());
                let tex = self.icons.texture(ui.ctx(), w.icon(), 16.0);
                let selected = self.ws == w;
                let img = egui::Image::new((tex, egui::vec2(16.0, 16.0))).tint(if selected { t.accent } else { t.text_dim });
                let mut b = egui::Button::image_and_text(img, tr(w.title_key())).selected(selected);
                if let Some(k) = workspace_key_label(i) {
                    b = b.shortcut_text(k);
                }
                let item = ui.add(b);
                self.rects.note(format!("menu_item:window.{}", w.key()), item.rect);
                if item.clicked() {
                    self.switch_workspace(w);
                    ui.close();
                }
            }
            ui.separator();
            let r = ui.menu_button(tr("menu.window.layout"), |ui| self.menu_layout(ui));
            self.rects.note("menu_item:window.layout", r.response.rect);
        });
        self.rects.note("menu:window", r.response.rect);
    }

    /// Janela > Layout: os layouts com nome do workspace atual (aplicar, salvar, excluir) e voltar à fábrica.
    fn menu_layout(&mut self, ui: &mut egui::Ui) {
        let ws = self.ws;
        if self.menu_item(ui, "layout.save_as", "menu.window.layout.save_as", "", true) {
            self.layout_name = Some(String::new());
            ui.close();
        }
        ui.separator();
        ui.label(RichText::new(tr("menu.window.layout.mine")).size(11.0).color(self.tokens().text_dim));
        let names: Vec<String> = self.layouts.get(ws).map(|e| e.named.iter().map(|n| n.name.clone()).collect()).unwrap_or_default();
        let active = self.layouts.label(ws);
        if names.is_empty() {
            ui.add_enabled(false, egui::Button::new(tr("menu.window.layout.none")));
        }
        let mut apply = None;
        for n in &names {
            let r = ui.radio(active.as_deref() == Some(n.as_str()), n.as_str());
            self.rects.note(format!("menu_item:layout.apply.{n}"), r.rect);
            if r.clicked() {
                apply = Some(n.clone());
            }
        }
        if let Some(n) = apply {
            let _ = self.layout_apply(&n);
            ui.close();
        }
        let mut delete = None;
        ui.add_enabled_ui(!names.is_empty(), |ui| {
            let r = ui.menu_button(tr("menu.window.layout.delete"), |ui| {
                for n in &names {
                    let r = ui.button(n.as_str());
                    self.rects.note(format!("menu_item:layout.delete.{n}"), r.rect);
                    if r.clicked() {
                        delete = Some(n.clone());
                    }
                }
            });
            self.rects.note("menu_item:layout.delete", r.response.rect);
        });
        if let Some(n) = delete {
            self.layout_delete(&n);
            ui.close();
        }
        ui.separator();
        let r = ui.button(tr("menu.window.reset")).on_hover_text(tr("menu.window.layout.factory_tip"));
        self.rects.note("menu_item:window.reset", r.rect);
        if r.clicked() {
            self.reset_layout(ws);
            self.notice(tr("msg.layout_reset"));
            ui.close();
        }
        let r = ui.button(tr("menu.window.reset_all")).on_hover_text(tr("menu.window.layout.factory_tip"));
        self.rects.note("menu_item:window.reset_all", r.rect);
        if r.clicked() {
            for w in WorkspaceId::ALL {
                self.reset_layout(w);
            }
            self.notice(tr("msg.layout_reset"));
            ui.close();
        }
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

    /// Abas de workspace (como no Blender), com um separador discreto entre os grupos (pré-produção, arte, finalização,
    /// recursos, som e montagem, gestão). Se não couberem com rótulo, só o ativo mostra o nome.
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
                let sep_w = 11.0;
                let widths: Vec<f32> = WorkspaceId::ALL
                    .iter()
                    .map(|w| pad * 2.0 + icon + gap + ui.painter().layout_no_wrap(tr(w.title_key()).to_owned(), font.clone(), Color32::WHITE).size().x)
                    .collect();
                let spacing = 2.0;
                let groups = WorkspaceId::ALL.windows(2).filter(|p| p[0].group() != p[1].group()).count() as f32;
                let total: f32 = widths.iter().sum::<f32>() + spacing * (widths.len() as f32 - 1.0) + groups * (sep_w + spacing);
                let compact = total > ui.available_width();
                ui.spacing_mut().item_spacing.x = spacing;
                ui.horizontal(|ui| {
                    let mut prev_group = None;
                    for (i, (w, full)) in WorkspaceId::ALL.into_iter().zip(widths).enumerate() {
                        if prev_group.is_some_and(|g| g != w.group()) {
                            let (r, resp) = ui.allocate_exact_size(egui::vec2(sep_w, 30.0), Sense::hover());
                            let x = r.center().x.round() + 0.5;
                            ui.painter().vline(x, (r.center().y - 7.0)..=(r.center().y + 9.0), Stroke::new(1.0, t.border));
                            resp.on_hover_text(tr(w.group().title_key()));
                        }
                        prev_group = Some(w.group());
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
                        // dica: nome (quando só o ícone aparece), grupo e atalho
                        let mut tip = format!("{} · {}", tr(w.title_key()), tr(w.group().title_key()));
                        if let Some(k) = workspace_key_label(i) {
                            tip.push_str(&format!(" ({k})"));
                        }
                        if resp.on_hover_text(tip).clicked() {
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
                    let layout_label = match self.layouts.label(self.ws) {
                        None => tr("status.layout.factory").to_owned(),
                        Some(n) if n.is_empty() => tr("status.layout.custom").to_owned(),
                        Some(n) => n,
                    };
                    let r = ui.label(RichText::new(format!("{}: {layout_label}", tr("status.layout"))).size(12.0).color(t.text_dim));
                    self.rects.note("status:layout", r.rect);
                    ui.separator();
                    let project = format!("{}{}", self.doc_name(), if self.doc_dirty() { "*" } else { "" });
                    ui.label(RichText::new(format!("{}: {}  ({} {})", tr("status.project"), project, tr("status.revision"), self.core.project.revision)).size(12.0).color(t.text_dim));
                    if let Some((msg, at)) = &self.notice {
                        if at.elapsed() < Duration::from_secs(6) {
                            ui.separator();
                            // o aviso não passa por cima do lado direito (bridge, memória, fps): corta com reticências
                            // e mostra o texto inteiro ao passar o mouse
                            let w = (ui.available_width() - STATUS_RIGHT_W).max(40.0);
                            ui.allocate_ui_with_layout(egui::vec2(w, 18.0), egui::Layout::left_to_right(Align::Center), |ui| {
                                ui.set_max_width(w);
                                ui.add(egui::Label::new(RichText::new(msg.as_str()).size(12.0).color(t.text)).truncate()).on_hover_text(msg.as_str());
                            });
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

    /// Prateleira de ferramentas dos workspaces de canvas: as do perfil do workspace (`WorkspaceProfile::tools`).
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
                        for &tool in self.ws.profile().tools {
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
                            // dica com o atalho em uso (Preferências > Atalhos)
                            let key = crate::keymap::Action::for_tool(tool).and_then(|a| self.ed.keys.get(a)).map(|c| c.label()).or(tool.comp_key().map(str::to_owned)).or(tool.modeling_key().map(str::to_owned));
                            let tip = match key {
                                Some(k) => format!("{} ({k})", tr(tool.title_key())),
                                None => tr(tool.title_key()).to_owned(),
                            };
                            if resp.on_hover_text(tip).clicked() {
                                // clicar na ferramenta (mesmo a que já está em uso) mostra o painel dela
                                if !matches!(tool, ToolId::Move | ToolId::Hand | ToolId::Zoom | ToolId::Eyedropper) && tool.comp_key().is_none() && !tool.is_modeling() {
                                    self.ed.want_panel = Some(PanelKind::Brushes);
                                }
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
        self.layout_update_banner(ui);
        self.ensure_tree(ws);
        let Some(tree) = self.trees.get_mut(&ws) else { return };
        // ferramenta com itens (caneta, pincel, borracha, balde, seleção, editar pontos) escolhida agora (clique ou
        // atalho): o painel Ferramenta vem para a frente
        if self.last_tool.is_some_and(|(t, w)| t != self.tool && w == ws) && !matches!(self.tool, crate::workspace::ToolId::Move | crate::workspace::ToolId::Hand | crate::workspace::ToolId::Zoom | crate::workspace::ToolId::Eyedropper) && self.tool.comp_key().is_none() && !self.tool.is_modeling() {
            self.ed.want_panel = Some(PanelKind::Brushes);
        }
        self.last_tool = Some((self.tool, ws));
        if let Some(want) = self.ed.want_panel.take() {
            tree.make_active(|_, tile| matches!(tile, egui_tiles::Tile::Pane(k) if *k == want));
        }
        let mut notes: Vec<(String, Rect)> = Vec::new();
        let env = ui_kit::Env { t, core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut notes, tool: &mut self.tool, ws };
        let mut behavior = DockBehavior { t, in_tabs: layout::panes_in_tabs(tree), env, close_req: None };
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.bg_frame).inner_margin(egui::Margin::same(CARD_GAP as i8))).show(ui, |ui| {
            tree.ui(&mut behavior, ui);
        });
        if let Some(id) = behavior.close_req.take() {
            tree.tiles.set_visible(id, false);
        }
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
        if self.key_capture.is_some() {
            return;
        }
        self.workspace_keys(ctx);
        let profile = self.ws.profile();
        if !profile.editor_keys {
            return;
        }
        let t = self.tokens();
        let ws = self.ws;
        let before = self.tool;
        let mut notes = Vec::new();
        let mut env = ui_kit::Env { t, core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut notes, tool: &mut self.tool, ws };
        // na Composição, as teclas do After Effects (A P S R T U, J/K, F9, W/Y/C) antes das gerais
        if ws == WorkspaceId::Composicao {
            comp::shortcuts(ctx, &mut env);
        }
        // as teclas da animação antes (Ctrl+Shift+T não pode cair no Ctrl+T da transformação)
        // a Edição tem atalhos próprios do Premiere (ferramentas, J/K/L, I/O, inserir/sobrescrever...) antes dos do editor
        if ws == WorkspaceId::Edicao {
            panels::edit_timeline::shortcuts(ctx, &mut env);
        }
        anim::shortcuts(ctx, &mut env);
        tools::shortcuts(ctx, &mut env);
        // a tecla de uma ferramenta que não está na prateleira deste workspace não troca a ferramenta
        if !profile.allows(self.tool) && self.tool != before {
            self.tool = before;
        }
    }

    /// Alt+1..9 e Alt+0 trocam para os dez primeiros workspaces da barra (fora de campos de texto e menus).
    fn workspace_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) {
            return;
        }
        use egui::Key as K;
        const KEYS: [egui::Key; 10] = [K::Num1, K::Num2, K::Num3, K::Num4, K::Num5, K::Num6, K::Num7, K::Num8, K::Num9, K::Num0];
        let hit = ctx.input_mut(|i| KEYS.iter().position(|k| i.consume_key(egui::Modifiers::ALT, *k)));
        if let Some(w) = hit.and_then(|n| WorkspaceId::ALL.get(n)) {
            self.switch_workspace(*w);
        }
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
                    let (tex, size) = brand::wordmark_texture(ctx, self.prefs.theme, 250.0);
                    ui.add(egui::Image::new((tex, size)));
                    ui.add_space(10.0);
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
        crate::brand::tick_window_icons();
        if self.frame_count == 2 {
            // a interface já desenhou: a tela de abertura pode sair
            crate::splash::finish();
        }
        let ctx = ui.ctx().clone();
        self.sample_frame(&ctx);
        self.bridge_tick(&ctx);
        self.ed.pressure_curve = (self.prefs.pen_pressure_min, self.prefs.pen_pressure_gamma);
        self.ed.cursor_precise = self.prefs.cursor_precise;
        self.autosave_tick();
        panels::timeline::tick(&self.core, &mut self.ed, &ctx);
        panels::deliver::tick(&self.core, &mut self.ed, &ctx);
        panels::char_folder::tick(&self.core, &mut self.ed, self.ws);
        panels::audio_ws::tick(&mut self.core, &mut self.ed, &ctx);
        {
            // som ao vivo acompanha a reprodução e o cabeçote
            let cut = self.ed.cut.clone();
            let dur = cut.as_ref().and_then(|c| commands::cut_data(&self.core.project, c)).map(|c| c.duration).unwrap_or(1);
            let (_, hi) = self.ed.playback.range(dur);
            let (frame, playing) = (self.ed.frame, self.ed.playback.playing);
            self.ed.live.sync(&self.core, cut.as_ref(), frame, playing, hi);
        }
        self.editor_shortcuts(&ctx);
        self.poll_dialog(&ctx);
        // arquivos soltos na janela: imagem → referência, WAV → clipe no cabeçote
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        for p in dropped {
            // soltar sobre o painel da Biblioteca importa para ela (a janela de importar pede licença e fonte)
            if self.ed.library.accepts_drop(&ctx) {
                self.ed.library.begin_import(vec![p]);
            } else {
                self.import_file(&p);
            }
        }
        self.doc_shortcuts(&ctx);
        self.handle_close_request(&ctx);
        self.sync_layout_variant();

        self.menu_bar(ui);
        self.workspace_bar(ui);
        self.status_bar(ui);
        // a Modelagem não tem canvas, mas tem a prateleira do viewport 3D
        if !self.ws.profile().tools.is_empty() {
            self.tool_shelf(ui);
        }
        self.workspace_ui(ui);
        self.about_window(&ctx);
        self.prefs_window(&ctx);
        self.start_window(&ctx);
        self.confirm_window(&ctx);
        self.layout_name_window(&ctx);
        self.layouts_tick();
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
        // depois do fim do quadro: os retângulos lidos são os deste quadro (painéis na tela)
        self.gpu_tick(&ctx);
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
                // camada 3D: renderer cel na GPU (no device do egui) para o compositor
                model3d::install_gpu(rs);
                self.gpu.install(rs);
                let info = rs.adapter.get_info();
                self.gpu_name = Some(info.name.clone());
                self.ed.audio_ws.hw.set_gpu(&info.name, &format!("{:?}", info.device_type), &format!("{:?}", info.backend), &info.driver);
                tracing::info!("GPU: {} ({:?}, {:?})", info.name, info.device_type, info.backend);
            }
        }
        self.ed.sanitize_color(&self.core.project);
        if let Some(reason) = crate::gpu_mem::device_lost() {
            self.gpu_lost_exit(&reason);
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
        if self.layouts_dirty_at.is_some() {
            self.save_layouts();
        }
        // saída normal (o usuário já decidiu sobre as alterações): a cópia de recuperação desta execução sai
        self.autosave.discard();
    }
}

// -------------------------------------------------------------------------------------------------------- dock

/// Atalho de troca de workspace da posição `i` da barra: Alt+1..9 e Alt+0 (o décimo).
fn workspace_key_label(i: usize) -> Option<String> {
    match i {
        0..=8 => Some(format!("Alt+{}", i + 1)),
        9 => Some("Alt+0".to_owned()),
        _ => None,
    }
}

struct DockBehavior<'a> {
    /// Painel solto fechado pelo X do cabeçalho neste quadro (escondido depois do desenho da árvore).
    close_req: Option<TileId>,
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
        // fechar (Exibir > Painéis traz de volta); o canvas não fecha
        if kind != PanelKind::Canvas {
            let x = Rect::from_center_size(egui::pos2(hdr.right() - 12.0, hdr.center().y), egui::vec2(16.0, 16.0));
            let xr = ui.interact(x, ui.id().with(("dock_close", tile_id)), Sense::click());
            let col = if xr.hovered() { self.t.text } else { self.t.text_dim };
            let c = x.center();
            for (a, b) in [(egui::vec2(-4.0, -4.0), egui::vec2(4.0, 4.0)), (egui::vec2(-4.0, 4.0), egui::vec2(4.0, -4.0))] {
                ui.painter().line_segment([c + a, c + b], Stroke::new(1.3, col));
            }
            self.env.note(format!("panel_close:{}", kind.key()), x);
            if xr.on_hover_text(tr("panel.close")).clicked() {
                self.close_req = Some(tile_id);
            }
        }
        if resp.drag_started() {
            UiResponse::DragStarted
        } else {
            UiResponse::None
        }
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
            PanelKind::Timeline if self.env.ws == WorkspaceId::Edicao => panels::edit::timeline(&mut self.env, ui, body),
            PanelKind::Timeline => panels::timeline::show(&mut self.env, ui, body),
            PanelKind::Media if self.env.ws == WorkspaceId::Audio => panels::audio::media(&mut self.env, ui, body),
            PanelKind::Preview if self.env.ws == WorkspaceId::Audio => panels::audio::monitor(&mut self.env, ui, body),
            PanelKind::Mixer => panels::audio::mixer(&mut self.env, ui, body),
            PanelKind::Waveform => panels::audio_ws::wave::show(&mut self.env, ui, body),
            PanelKind::EffectsRack => panels::audio_ws::rack::show(&mut self.env, ui, body),
            PanelKind::Meters => panels::audio_ws::meters::show(&mut self.env, ui, body),
            PanelKind::Multitrack => panels::audio_ws::multitrack::show(&mut self.env, ui, body),
            PanelKind::Tts => panels::audio_ws::tts::show(&mut self.env, ui, body),
            PanelKind::Navigator => panels::navigator::show(&mut self.env, ui, body),
            PanelKind::SubView => panels::subview::show(&mut self.env, ui, body),
            PanelKind::Brushes => panels::brushes::show(&mut self.env, ui, body),
            PanelKind::Text => panels::text::show(&mut self.env, ui, body),
            PanelKind::LightTable => panels::light_table::show(&mut self.env, ui, body),
            PanelKind::SceneInfo => panels::scene_info::show(&mut self.env, ui, body),
            PanelKind::CharRef => panels::char_ref::show(&mut self.env, ui, body),
            PanelKind::Rulers => panels::rulers::show(&mut self.env, ui, body),
            PanelKind::Characters => panels::characters::list(&mut self.env, ui, body),
            PanelKind::CharFolder => panels::char_folder::show(&mut self.env, ui, body),
            PanelKind::CompTimeline => panels::comp_timeline::show(&mut self.env, ui, body),
            PanelKind::EffectsBrowser => panels::effects_browser::show(&mut self.env, ui, body),
            PanelKind::EffectControls => panels::effect_controls::show(&mut self.env, ui, body),
            PanelKind::Sheet => panels::characters::sheet(&mut self.env, ui, body),
            PanelKind::Media => panels::edit::media(&mut self.env, ui, body),
            PanelKind::Presets => panels::deliver::presets(&mut self.env, ui, body),
            PanelKind::Queue => panels::deliver_panels::queue(&mut self.env, ui, body),
            PanelKind::Destinations => panels::deliver_panels::destinations(&mut self.env, ui, body),
            PanelKind::Deliverables => panels::deliver_panels::deliverables(&mut self.env, ui, body),
            PanelKind::Poster => panels::deliver_panels::poster_panel(&mut self.env, ui, body),
            PanelKind::Preflight => panels::deliver_panels::preflight_panel(&mut self.env, ui, body),
            PanelKind::Preview => panels::edit::preview(&mut self.env, ui, body),
            PanelKind::SourceMonitor => panels::edit_panels::source_monitor(&mut self.env, ui, body),
            PanelKind::Transitions => panels::edit_panels::transitions(&mut self.env, ui, body),
            PanelKind::ClipEffects => panels::edit_panels::clip_effects(&mut self.env, ui, body),
            PanelKind::Captions => panels::edit_panels::captions_panel(&mut self.env, ui, body),
            PanelKind::Properties => panels::properties::show(&mut self.env, ui, body),
            PanelKind::Project => panels::project::show(&mut self.env, ui, body),
            PanelKind::Outline => panels::outline::show(&mut self.env, ui, body),
            PanelKind::Script => panels::script::show(&mut self.env, ui, body),
            PanelKind::Shots => panels::shots::show(&mut self.env, ui, body),
            PanelKind::Board => panels::board::show(&mut self.env, ui, body),
            PanelKind::Episodes => panels::production::episodes(&mut self.env, ui, body),
            PanelKind::Tasks => panels::production::tasks(&mut self.env, ui, body),
            PanelKind::ProdCharts => panels::prod_charts::show(&mut self.env, ui, body),
            PanelKind::ProdBottleneck => panels::prod_manage::bottleneck(&mut self.env, ui, body),
            PanelKind::ProdAi => panels::prod_manage::ai(&mut self.env, ui, body),
            PanelKind::Library => panels::library::show(&mut self.env, ui, body),
            PanelKind::LibraryPreview => panels::library::preview(&mut self.env, ui, body),
            PanelKind::Modeling => modeling::viewport(&mut self.env, ui, body),
            PanelKind::ModelOutliner => panels::modeling::outliner(&mut self.env, ui, body),
            PanelKind::ModelProps => panels::modeling::properties(&mut self.env, ui, body),
            PanelKind::ModelModifiers => panels::modeling::modifiers(&mut self.env, ui, body),
        }
        ui.painter().rect_stroke(rect, CARD_RADIUS, Stroke::new(1.0, self.t.border), StrokeKind::Inside);
        resp
    }

    fn tab_title_for_pane(&mut self, pane: &PanelKind) -> egui::WidgetText {
        tr(pane.title_key()).into()
    }

    fn is_tab_closable(&self, tiles: &Tiles<PanelKind>, tile_id: TileId) -> bool {
        !matches!(tiles.get(tile_id), Some(egui_tiles::Tile::Pane(PanelKind::Canvas)))
    }

    /// Fechar uma aba só esconde o painel (Exibir > Painéis traz de volta, no mesmo lugar).
    fn on_tab_close(&mut self, tiles: &mut Tiles<PanelKind>, tile_id: TileId) -> bool {
        tiles.set_visible(tile_id, false);
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
