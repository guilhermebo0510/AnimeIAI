//! Workspace Produção: painel Episódios (episódios e cenas como filtro, resumo das etapas e relatório), painel Tarefas
//! (a カット表: miniatura, nº, cena, duração, as 8 etapas e as correções/retakes abertos de cada corte) e, no painel
//! Propriedades, o corte ativo (etapas, lint, correções e retakes). Tudo pelas ações `PRODUCTION`, `CORRECTION` e
//! `RETAKE` da bridge (`actions`), com o autor do usuário: o mesmo undo e o mesmo log do MCP.
//!
//! O corte selecionado aqui é o corte ativo do editor (`Editor::cut`): escolher um corte na tabela e abrir o Animação
//! continua no mesmo corte. A miniatura é o render do quadro 0 (`production::cut_thumbnail`), em cache por assinatura do
//! desenho do corte, no máximo [`THUMB_BUDGET`] renders por quadro.

use super::board::drawing_signature;
use crate::app::actions;
use crate::app::commands::NodeId;
use crate::app::doc::AUTHOR;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::theme::Tokens;
use crate::workspace::WorkspaceId;
use aiai_core::{CoreSession, Id, NodeKind, Project, Stage, StageStatus};
use aiai_tools::production::{self, ALL_STAGES, CutRow, ProductionDoc};
use egui::{Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

/// Largura da textura da miniatura do corte.
pub(crate) const THUMB_W: u32 = 192;
/// Renders de miniatura por quadro.
pub(crate) const THUMB_BUDGET: u32 = 1;

pub(crate) const STATUSES: [StageStatus; 5] = [StageStatus::NotStarted, StageStatus::InProgress, StageStatus::Review, StageStatus::Approved, StageStatus::Retake];

pub(crate) fn stage_key(s: Stage) -> &'static str {
    match s {
        Stage::Layout => "prod.stage.layout",
        Stage::Genga => "prod.stage.genga",
        Stage::Douga => "prod.stage.douga",
        Stage::Shiage => "prod.stage.shiage",
        Stage::Background => "prod.stage.background",
        Stage::Satsuei => "prod.stage.satsuei",
        Stage::Color => "prod.stage.color",
        Stage::Sound => "prod.stage.sound",
    }
}

pub(crate) fn status_key(s: StageStatus) -> &'static str {
    match s {
        StageStatus::NotStarted => "prod.status.not_started",
        StageStatus::InProgress => "prod.status.in_progress",
        StageStatus::Review => "prod.status.review",
        StageStatus::Approved => "prod.status.approved",
        StageStatus::Retake => "prod.status.retake",
    }
}

/// Próximo estado no clique da etiqueta (o retake só entra pelo menu: é uma decisão da revisão, não um avanço).
pub(crate) fn next_status(s: StageStatus) -> StageStatus {
    match s {
        StageStatus::NotStarted => StageStatus::InProgress,
        StageStatus::InProgress => StageStatus::Review,
        StageStatus::Review => StageStatus::Approved,
        StageStatus::Approved | StageStatus::Retake => StageStatus::NotStarted,
    }
}

/// Fundo da etiqueta de um estado (tokens do tema; o texto por cima é `text`).
pub(crate) fn status_fill(t: &Tokens, s: StageStatus) -> Color32 {
    match s {
        StageStatus::NotStarted => t.bg_widget,
        StageStatus::InProgress => t.st_progress,
        StageStatus::Review => t.st_review,
        StageStatus::Approved => t.st_ok,
        StageStatus::Retake => t.st_retake,
    }
}

// ------------------------------------------------------------------------------------------------ leitura em cache

/// Tabela de cortes e documento de produção decodificados, válidos enquanto o `change_counter` não muda.
#[derive(Default)]
pub(crate) struct ProdCache {
    at: Option<u64>,
    rows: Arc<Vec<CutRow>>,
    doc: Arc<ProductionDoc>,
}

impl ProdCache {
    pub(crate) fn get(&mut self, core: &CoreSession) -> (Arc<Vec<CutRow>>, Arc<ProductionDoc>) {
        if self.at != Some(core.change_counter) {
            self.rows = Arc::new(production::cut_table(&core.project));
            self.doc = Arc::new(production::production_read(&core.project).unwrap_or_default());
            self.at = Some(core.change_counter);
        }
        (self.rows.clone(), self.doc.clone())
    }
}

struct Thumb {
    checked: u64,
    sig: u64,
    tex: Option<egui::TextureHandle>,
}

/// Miniaturas dos cortes (quadro 0), por corte.
#[derive(Default)]
pub(crate) struct CutThumbs {
    map: HashMap<String, Thumb>,
    pub renders: u64,
}

impl CutThumbs {
    pub(crate) fn get(&mut self, ctx: &egui::Context, core: &CoreSession, cut: &NodeId, budget: &mut u32) -> Option<egui::TextureId> {
        let counter = core.change_counter;
        if let Some(e) = self.map.get_mut(&cut.0) {
            if e.checked == counter {
                return e.tex.as_ref().map(|t| t.id());
            }
        }
        let sig = drawing_signature(&core.project, cut);
        if let Some(e) = self.map.get_mut(&cut.0) {
            if e.sig == sig {
                e.checked = counter;
                return e.tex.as_ref().map(|t| t.id());
            }
        }
        if *budget == 0 {
            ctx.request_repaint();
            return self.map.get(&cut.0).and_then(|e| e.tex.as_ref().map(|t| t.id()));
        }
        *budget -= 1;
        self.renders += 1;
        let tex = production::cut_thumbnail(&core.project, &core.blobs, cut, 0, THUMB_W).ok().map(|img| {
            let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.to_rgba8());
            ctx.load_texture(format!("cut-thumb-{}", cut.0), color, egui::TextureOptions::LINEAR)
        });
        let id = tex.as_ref().map(|t| t.id());
        self.map.insert(cut.0.clone(), Thumb { checked: counter, sig, tex });
        id
    }

    pub(crate) fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.map.retain(|k, _| keep(k));
    }
}

/// Estado do workspace Produção que não mora no documento.
pub(crate) struct ProdState {
    /// Filtro da tabela: episódio ou cena (`None` = todos os cortes).
    pub filter: Option<NodeId>,
    pub cache: ProdCache,
    pub thumbs: CutThumbs,
    /// Último lint do corte: (corte, `change_counter` do lint, resultado).
    pub lint: Option<(NodeId, u64, Value)>,
    /// Comparação do retake aberta: (retake, `change_counter`, folha ANTES | DEPOIS | DIFERENÇA, resumo).
    pub compare: Option<(Id, u64, egui::TextureHandle, String)>,
    /// Formulários de correção e retake.
    pub frame: u32,
    pub note: String,
    pub reason: String,
}

impl Default for ProdState {
    fn default() -> Self {
        ProdState { filter: None, cache: ProdCache::default(), thumbs: CutThumbs::default(), lint: None, compare: None, frame: 0, note: String::new(), reason: String::new() }
    }
}

/// O corte passa no filtro (episódio ou cena)?
pub(crate) fn passes(filter: Option<&NodeId>, row: &CutRow) -> bool {
    match filter {
        None => true,
        Some(f) => row.episode.as_ref() == Some(f) || row.scene.as_ref() == Some(f),
    }
}

/// Corte ativo do editor, se for um dos cortes da tabela.
fn selected<'a>(env: &mut Env, rows: &'a [CutRow]) -> Option<&'a CutRow> {
    let cut = env.ed.active_cut(env.core)?;
    rows.iter().find(|r| r.cut == cut)
}

fn set_stage(env: &mut Env, cut: &NodeId, stage: Stage, status: StageStatus) {
    let r = actions::run(env.core, "PRODUCTION", json!({ "op": "set_stage", "cut": cut, "stage": stage, "status": status }));
    actions::notify_err(env.ed, r);
}

/// Abre o corte (e o quadro) no workspace Animação.
fn open_in_animation(env: &mut Env, cut: &NodeId, frame: Option<u32>) {
    env.ed.set_cut(cut.clone());
    if let Some(f) = frame {
        env.ed.frame = f;
    }
    env.ed.sel_node = Some(cut.clone());
    env.ed.want_workspace = Some(WorkspaceId::Animacao);
}

/// "C003" e, se o nome do corte disser mais que o número, o nome ("C999  Ensaio do rig").
pub(crate) fn cut_title(r: &CutRow) -> String {
    let code = format!("C{:03}", r.number);
    if r.name.trim().is_empty() || r.name.trim() == code { code } else { format!("{code}  {}", r.name) }
}

fn secs(frames: u32, p: &Project) -> f64 {
    frames as f64 / p.settings.fps()
}

// ------------------------------------------------------------------------------------------------ Episódios

pub(crate) fn episodes(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let (rows, _) = env.ed.prod.cache.get(env.core);
    if env.ed.prod.filter.as_ref().is_some_and(|f| env.core.project.node(f).is_none()) {
        env.ed.prod.filter = None;
    }
    egui::ScrollArea::vertical().id_salt("prod_eps").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        if rows.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(tr("panel.episodes.empty")).color(t.text_dim));
            return;
        }
        let total: u32 = rows.iter().map(|r| r.duration).sum();
        let p = &env.core.project;
        let all_line = format!("{}  ·  {} · {:.1} s", tr("prod.all_cuts"), rows.len(), secs(total, p));
        let mut entries: Vec<(Option<NodeId>, String, f32)> = vec![(None, all_line, 0.0)];
        for ep in &p.roots.episodes {
            let Some(epn) = p.node(ep) else { continue };
            let of_ep: Vec<&CutRow> = rows.iter().filter(|r| r.episode.as_ref() == Some(ep)).collect();
            let d: u32 = of_ep.iter().map(|r| r.duration).sum();
            entries.push((Some(ep.clone()), format!("{}  ·  {} · {:.1} s", epn.name, of_ep.len(), secs(d, p)), 0.0));
            for sc in &epn.children {
                let Some(scn) = p.node(sc) else { continue };
                if !matches!(scn.data, NodeKind::Scene(_)) {
                    continue;
                }
                let of_sc: Vec<&CutRow> = rows.iter().filter(|r| r.scene.as_ref() == Some(sc)).collect();
                let d: u32 = of_sc.iter().map(|r| r.duration).sum();
                entries.push((Some(sc.clone()), format!("{}  ·  {} · {:.1} s", scn.name, of_sc.len(), secs(d, p)), 14.0));
            }
        }
        for (i, (id, text, indent)) in entries.into_iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::click());
            env.note(format!("prod:filter:{i}"), rect);
            let on = env.ed.prod.filter == id;
            let painter = ui.painter();
            if on {
                painter.rect_filled(rect, 4.0, t.accent_dim);
            } else if resp.hovered() {
                painter.rect_filled(rect, 4.0, t.bg_hover);
            }
            let painter = painter.with_clip_rect(rect.shrink2(egui::vec2(4.0, 0.0)));
            let g = painter.layout_no_wrap(text, FontId::proportional(12.5), t.text);
            painter.galley(egui::pos2(rect.left() + 6.0 + indent, rect.center().y - g.size().y / 2.0), g, t.text);
            if resp.clicked() {
                env.ed.prod.filter = id;
            }
        }

        // ---- resumo das etapas (cortes do filtro): aprovados / total, com barra
        let shown: Vec<&CutRow> = rows.iter().filter(|r| passes(env.ed.prod.filter.as_ref(), r)).collect();
        section(ui, &t, "prod.summary");
        for s in ALL_STAGES {
            let n = shown.len().max(1) as f32;
            let count = |st: StageStatus| shown.iter().filter(|r| r.stages.get(&s) == Some(&st)).count();
            let (ok, review, prog, retake) = (count(StageStatus::Approved), count(StageStatus::Review), count(StageStatus::InProgress), count(StageStatus::Retake));
            ui.horizontal(|ui| {
                ui.add_sized([58.0, 18.0], egui::Label::new(egui::RichText::new(tr(stage_key(s))).size(12.0)));
                let w = (ui.available_width() - 44.0).max(30.0);
                let (bar, _) = ui.allocate_exact_size(egui::vec2(w, 10.0), Sense::hover());
                let painter = ui.painter();
                painter.rect_filled(bar, 2.0, t.bg_widget);
                let mut x = bar.left();
                for (c, st) in [(ok, StageStatus::Approved), (review, StageStatus::Review), (prog, StageStatus::InProgress), (retake, StageStatus::Retake)] {
                    let dx = bar.width() * c as f32 / n;
                    if dx > 0.0 {
                        painter.rect_filled(Rect::from_min_size(egui::pos2(x, bar.top()), egui::vec2(dx, bar.height())), 0.0, status_fill(&t, st));
                        x += dx;
                    }
                }
                painter.rect_stroke(bar, 2.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
                dim_label(ui, &t, &format!("{ok}/{}", shown.len()));
            });
        }
        let open_c: usize = shown.iter().map(|r| r.open_corrections).sum();
        let open_r: usize = shown.iter().map(|r| r.open_retakes).sum();
        ui.add_space(2.0);
        dim_label(ui, &t, &format!("{}: {open_c}   ·   {}: {open_r}", tr("prod.open_corrections"), tr("prod.open_retakes")));
        ui.add_space(6.0);
        let b = ui.button(tr("prod.report"));
        env.note("prod:report", b.rect);
        if b.on_hover_text(tr("prod.report.tip")).clicked() {
            write_report(env);
        }
    });
}

/// Relatório de produção (Markdown) em `<pasta do projeto>/saida/producao.md`.
fn write_report(env: &mut Env) {
    let Some(path) = env.core.path.clone() else {
        env.ed.notify(tr("board.save_first"));
        return;
    };
    let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or(path);
    let out = dir.join("saida").join("producao.md");
    let r = actions::run(env.core, "PRODUCTION", json!({ "op": "report", "out": out.display().to_string() }));
    if actions::notify_err(env.ed, r).is_some() {
        env.ed.notify(format!("{}: {}", tr("prod.report_done"), out.display()));
    }
}

// ------------------------------------------------------------------------------------------------ Tarefas (カット表)

const ROW_H: f32 = 62.0;

pub(crate) fn tasks(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let (rows, _) = env.ed.prod.cache.get(env.core);
    let sel = selected(env, &rows).map(|r| r.cut.clone());

    // ---- barra
    ui.horizontal_wrapped(|ui| {
        let b = ui.add_enabled(sel.is_some(), egui::Button::new(tr("prod.suggest")));
        env.note("prod:suggest", b.rect);
        if let (true, Some(c)) = (b.on_hover_text(tr("prod.suggest.tip")).clicked(), &sel) {
            suggest(env, c);
        }
        let b = ui.add_enabled(sel.is_some(), egui::Button::new(tr("prod.lint")));
        env.note("prod:lint", b.rect);
        if let (true, Some(c)) = (b.on_hover_text(tr("prod.lint.tip")).clicked(), &sel) {
            lint(env, c);
        }
        let b = ui.add_enabled(sel.is_some(), egui::Button::new(tr("prod.open_cut")));
        env.note("prod:open_cut", b.rect);
        if let (true, Some(c)) = (b.on_hover_text(tr("prod.open_cut.tip")).clicked(), &sel) {
            open_in_animation(env, c, None);
        }
    });
    ui.add_space(2.0);

    let shown: Vec<&CutRow> = rows.iter().filter(|r| passes(env.ed.prod.filter.as_ref(), r)).collect();
    let ctx = ui.ctx().clone();
    let mut budget = THUMB_BUDGET;
    egui::ScrollArea::vertical().id_salt("prod_tasks").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        if shown.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(tr(if rows.is_empty() { "panel.tasks.empty" } else { "prod.none_in_filter" })).color(t.text_dim));
            return;
        }
        for (i, r) in shown.iter().enumerate() {
            let tex = env.ed.prod.thumbs.get(&ctx, env.core, &r.cut, &mut budget);
            cut_row(env, ui, i, r, tex, sel.as_ref() == Some(&r.cut));
        }
    });
    let alive: std::collections::HashSet<&str> = rows.iter().map(|r| r.cut.0.as_str()).collect();
    env.ed.prod.thumbs.retain(|k| alive.contains(k));
}

fn cut_row(env: &mut Env, ui: &mut Ui, i: usize, r: &CutRow, tex: Option<egui::TextureId>, selected: bool) {
    let t = env.t;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click());
    env.note(format!("prod:cut:{i}"), rect);
    let painter = ui.painter().clone();
    if selected {
        painter.rect_filled(rect, 4.0, t.accent_dim);
        painter.rect_filled(Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 4.0), egui::vec2(3.0, ROW_H - 8.0)), 1.5, t.accent);
    } else if resp.hovered() {
        painter.rect_filled(rect, 4.0, t.bg_hover);
    }
    // miniatura 16:9 (ou a proporção do projeto)
    let p = &env.core.project;
    let (cw, ch) = (p.settings.width, p.settings.height);
    let th = ROW_H - 8.0;
    let tw = (th * cw.max(1) as f32 / ch.max(1) as f32).min(120.0);
    let thumb = Rect::from_min_size(rect.left_top() + egui::vec2(8.0, 4.0), egui::vec2(tw, th));
    painter.rect_filled(thumb, 2.0, Color32::WHITE);
    match tex {
        Some(id) => {
            painter.image(id, thumb, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            painter.text(thumb.center(), egui::Align2::CENTER_CENTER, "…", FontId::proportional(16.0), Color32::from_gray(170));
        }
    }
    painter.rect_stroke(thumb, 2.0, Stroke::new(1.0, t.border), StrokeKind::Inside);

    // texto: nº, nome, cena, duração; correções/retakes abertos
    let x0 = thumb.right() + 10.0;
    let text_clip = Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(rect.right() - 6.0, rect.bottom()));
    let tp = painter.with_clip_rect(text_clip);
    let head = cut_title(r);
    let g = tp.layout_no_wrap(head, FontId::proportional(13.0), t.text);
    tp.galley(egui::pos2(x0, rect.top() + 4.0), g, t.text);
    let mut sub = format!("{}  ·  {} {}  ·  {:.1} s", r.scene_name, r.duration, tr("unit.frames"), secs(r.duration, p));
    if r.open_corrections > 0 {
        sub.push_str(&format!("  ·  {} {}", r.open_corrections, tr("prod.corrections_short")));
    }
    if r.open_retakes > 0 {
        sub.push_str(&format!("  ·  {} {}", r.open_retakes, tr("prod.retakes_short")));
    }
    let g = tp.layout_no_wrap(sub, FontId::proportional(11.5), t.text_dim);
    tp.galley(egui::pos2(x0, rect.top() + 22.0), g, t.text_dim);

    // etiquetas das 8 etapas: clique avança, botão direito escolhe
    let chip_h = 17.0;
    let gap = 3.0;
    let avail = (text_clip.width()).max(80.0);
    let chip_w = ((avail - gap * 7.0) / 8.0).clamp(30.0, 64.0);
    let mut clicked_chip = false;
    for (k, s) in ALL_STAGES.into_iter().enumerate() {
        let st = r.stages.get(&s).copied().unwrap_or_default();
        let cr = Rect::from_min_size(egui::pos2(x0 + k as f32 * (chip_w + gap), rect.bottom() - chip_h - 5.0), egui::vec2(chip_w, chip_h));
        if cr.right() > text_clip.right() + 0.5 {
            break;
        }
        let cresp = ui.interact(cr, egui::Id::new(("prod_chip", &r.cut.0, k)), Sense::click());
        env.note(format!("prod:stage:{i}:{k}"), cr);
        painter.rect(cr, 3.0, status_fill(&t, st), Stroke::new(1.0, if cresp.hovered() { t.text_dim } else { t.border }), StrokeKind::Inside);
        let lp = painter.with_clip_rect(cr.shrink(1.0));
        lp.text(cr.center(), egui::Align2::CENTER_CENTER, tr(stage_key(s)), FontId::proportional(10.5), t.text);
        let cresp = cresp.on_hover_text(format!("{}: {}\n{}", tr(stage_key(s)), tr(status_key(st)), tr("prod.chip.tip")));
        if cresp.clicked() {
            clicked_chip = true;
            env.ed.set_cut(r.cut.clone());
            set_stage(env, &r.cut, s, next_status(st));
        }
        cresp.context_menu(|ui| {
            for x in STATUSES {
                if ui.selectable_label(x == st, tr(status_key(x))).clicked() {
                    set_stage(env, &r.cut, s, x);
                    ui.close();
                }
            }
        });
    }
    if resp.clicked() && !clicked_chip {
        env.ed.set_cut(r.cut.clone());
    }
    if resp.double_clicked() {
        open_in_animation(env, &r.cut, None);
    }
}

fn suggest(env: &mut Env, cut: &NodeId) {
    let r = actions::run(env.core, "PRODUCTION", json!({ "op": "suggest_stages", "cut": cut, "apply": true }));
    if let Some(v) = actions::notify_err(env.ed, r) {
        let list = v.get("suggestions").and_then(Value::as_array).cloned().unwrap_or_default();
        if list.is_empty() {
            env.ed.notify(tr("prod.suggest_none"));
        } else {
            // só os nomes das etapas na barra de status; os motivos ficam no log da ação
            let names: Vec<&str> = list.iter().filter_map(|s| serde_json::from_value::<Stage>(s.get("stage")?.clone()).ok()).map(|st| tr(stage_key(st))).collect();
            env.ed.notify(format!("{} {}: {}", list.len(), tr("prod.suggest_done"), names.join(", ")));
        }
    }
}

fn lint(env: &mut Env, cut: &NodeId) {
    let r = actions::run(env.core, "PRODUCTION", json!({ "op": "lint_cut", "cut": cut }));
    if let Some(v) = actions::notify_err(env.ed, r) {
        let clean = v.get("clean").and_then(Value::as_bool).unwrap_or(false);
        let n = v.get("unpainted_regions").and_then(Value::as_u64).unwrap_or(0);
        env.ed.notify(if clean { tr("prod.lint_clean").to_owned() } else { format!("{}: {n}", tr("prod.lint_dirty")) });
        env.ed.prod.lint = Some((cut.clone(), env.core.change_counter, v));
    }
}

// ------------------------------------------------------------------------------------------------ Propriedades

/// Propriedades do corte ativo no workspace Produção: etapas, lint, correções e retakes.
pub(crate) fn properties(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let (rows, doc) = env.ed.prod.cache.get(env.core);
    let Some(row) = selected(env, &rows).cloned() else {
        dim_label(ui, &t, tr("prod.props_none"));
        return;
    };
    let cut = row.cut.clone();
    ui.label(egui::RichText::new(cut_title(&row)).strong().size(13.0));
    dim_label(ui, &t, &format!("{}  ·  {} {}", row.scene_name, row.duration, tr("unit.frames")));

    // ---- etapas
    section(ui, &t, "prod.stages");
    egui::Grid::new("prod_stage_grid").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
        for (k, s) in ALL_STAGES.into_iter().enumerate() {
            dim_label(ui, &t, tr(stage_key(s)));
            let cur = row.stages.get(&s).copied().unwrap_or_default();
            let mut v = cur;
            let r = egui::ComboBox::from_id_salt(("prod_stage", k)).width(140.0).selected_text(tr(status_key(cur))).show_ui(ui, |ui| {
                for x in STATUSES {
                    ui.selectable_value(&mut v, x, tr(status_key(x)));
                }
            });
            env.note(format!("prod:props:stage:{k}"), r.response.rect);
            if v != cur {
                set_stage(env, &cut, s, v);
            }
            ui.end_row();
        }
    });

    // ---- lint
    section(ui, &t, "prod.lint_section");
    ui.horizontal(|ui| {
        let b = ui.button(tr("prod.lint"));
        env.note("prod:props:lint", b.rect);
        if b.clicked() {
            lint(env, &cut);
        }
    });
    if let Some((lc, at, v)) = env.ed.prod.lint.clone() {
        if lc == cut {
            if at != env.core.change_counter {
                dim_label(ui, &t, tr("prod.lint_stale"));
            }
            let layers = v.get("fill_layers").and_then(Value::as_array).cloned().unwrap_or_default();
            if layers.is_empty() {
                dim_label(ui, &t, tr("prod.lint_no_fill"));
            }
            for l in layers {
                let name = l.get("layer_name").and_then(Value::as_str).unwrap_or("?");
                let frames = l.get("frames_checked").and_then(Value::as_u64).unwrap_or(0);
                let un = l.get("unpainted_regions").and_then(Value::as_u64).unwrap_or(0);
                let area = l.get("unpainted_area").and_then(Value::as_u64).unwrap_or(0);
                let line = if un == 0 {
                    format!("{name}: {} ({frames} {})", tr("prod.lint_clean"), tr("prod.cels"))
                } else {
                    format!("{name}: {un} {} · {area} px ({frames} {})", tr("prod.regions"), tr("prod.cels"))
                };
                ui.label(egui::RichText::new(line).size(12.0).color(if un == 0 { t.text } else { t.accent }));
            }
        }
    }

    // ---- correções
    section(ui, &t, "prod.corrections");
    let corrections: Vec<_> = doc.corrections.iter().filter(|c| c.cut == cut).cloned().collect();
    if corrections.is_empty() {
        dim_label(ui, &t, tr("prod.no_corrections"));
    }
    for (i, c) in corrections.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut done = c.resolved;
            let r = ui.checkbox(&mut done, "");
            env.note(format!("prod:correction:{i}:resolve"), r.rect);
            if done != c.resolved {
                let rr = actions::run(env.core, "CORRECTION", json!({ "op": "resolve", "correction": c.id, "resolved": done, "by": AUTHOR }));
                actions::notify_err(env.ed, rr);
            }
            let b = ui.small_button(format!("q{}", c.frame + 1));
            env.note(format!("prod:correction:{i}:goto"), b.rect);
            if b.on_hover_text(tr("prod.goto_frame")).clicked() {
                open_in_animation(env, &cut, Some(c.frame));
            }
            let text = format!("{}  ({})", c.note, c.author);
            ui.label(egui::RichText::new(text).size(12.0).color(if c.resolved { t.text_dim } else { t.text }));
        });
    }
    form_frame(env, ui, row.duration);
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 130.0).max(80.0);
        let r = ui.add(egui::TextEdit::singleline(&mut env.ed.prod.note).desired_width(w).hint_text(tr("prod.note_hint")));
        env.note("prod:correction:note", r.rect);
        let b = ui.add_enabled(!env.ed.prod.note.trim().is_empty(), egui::Button::new(tr("prod.add_correction")));
        env.note("prod:correction:add", b.rect);
        if b.clicked() {
            let note = env.ed.prod.note.trim().to_owned();
            let rr = actions::run(env.core, "CORRECTION", json!({ "op": "add", "cut": cut, "frame": env.ed.prod.frame, "note": note }));
            if actions::notify_err(env.ed, rr).is_some() {
                env.ed.prod.note.clear();
            }
        }
    });
    dim_label(ui, &t, tr("prod.correction_hint"));

    // ---- retakes
    section(ui, &t, "prod.retakes");
    let retakes: Vec<_> = doc.retakes.iter().filter(|r| r.cut == cut).cloned().collect();
    if retakes.is_empty() {
        dim_label(ui, &t, tr("prod.no_retakes"));
    }
    for (i, rt) in retakes.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut done = rt.resolved;
            let r = ui.checkbox(&mut done, "");
            env.note(format!("prod:retake:{i}:resolve"), r.rect);
            if done != rt.resolved {
                let rr = actions::run(env.core, "RETAKE", json!({ "op": "resolve", "retake": rt.id, "resolved": done }));
                actions::notify_err(env.ed, rr);
            }
            let b = ui.small_button(tr("prod.compare"));
            env.note(format!("prod:retake:{i}:compare"), b.rect);
            if b.on_hover_text(tr("prod.compare.tip")).clicked() {
                compare(env, ui.ctx(), &rt.id);
            }
            let text = format!("q{}  {}  ({})", rt.frame + 1, rt.reason, rt.author);
            ui.label(egui::RichText::new(text).size(12.0).color(if rt.resolved { t.text_dim } else { t.text }));
        });
    }
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 130.0).max(80.0);
        let r = ui.add(egui::TextEdit::singleline(&mut env.ed.prod.reason).desired_width(w).hint_text(tr("prod.reason_hint")));
        env.note("prod:retake:reason", r.rect);
        let b = ui.add_enabled(!env.ed.prod.reason.trim().is_empty(), egui::Button::new(tr("prod.add_retake")));
        env.note("prod:retake:add", b.rect);
        if b.on_hover_text(tr("prod.add_retake.tip")).clicked() {
            let reason = env.ed.prod.reason.trim().to_owned();
            let rr = actions::run(env.core, "RETAKE", json!({ "op": "create", "cut": cut, "frame": env.ed.prod.frame, "reason": reason }));
            if actions::notify_err(env.ed, rr).is_some() {
                env.ed.prod.reason.clear();
            }
        }
    });

    // ---- comparação aberta
    if let Some((rid, at, tex, summary)) = &env.ed.prod.compare {
        if retakes.iter().any(|r| &r.id == rid) {
            ui.add_space(4.0);
            if *at != env.core.change_counter {
                dim_label(ui, &t, tr("prod.compare_stale"));
            }
            dim_label(ui, &t, summary);
            let size = tex.size_vec2();
            let w = ui.available_width().min(size.x);
            let r = ui.add(egui::Image::new((tex.id(), egui::vec2(w, w * size.y / size.x.max(1.0)))));
            env.notes.push(("prod:compare:sheet".into(), r.rect));
        }
    }
}

/// Quadro dos formulários de correção e retake (1-based na tela, 0-based no documento).
fn form_frame(env: &mut Env, ui: &mut Ui, duration: u32) {
    let t = env.t;
    let max = duration.max(1);
    env.ed.prod.frame = env.ed.prod.frame.min(max - 1);
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("prod.frame"));
        let mut shown = env.ed.prod.frame + 1;
        let r = ui.add(egui::DragValue::new(&mut shown).range(1..=max).speed(0.2));
        env.note("prod:form:frame", r.rect);
        env.ed.prod.frame = shown.clamp(1, max) - 1;
        let b = ui.small_button(tr("prod.frame_current"));
        env.note("prod:form:frame_current", b.rect);
        if b.on_hover_text(tr("prod.frame_current.tip")).clicked() {
            env.ed.prod.frame = env.ed.frame.min(max - 1);
        }
    });
}

/// Compara o snapshot do retake com o render atual e guarda a folha como textura.
fn compare(env: &mut Env, ctx: &egui::Context, retake: &Id) {
    let p = &env.core.project;
    let r = production::retake_compare(p, &env.core.blobs, retake, None).map_err(|e| e.to_string());
    let Some(cmp) = actions::notify_err(env.ed, r) else { return };
    let doc = production::production_read(p).unwrap_or_default();
    let Some(rt) = doc.retakes.iter().find(|r| &r.id == retake) else { return };
    let sheet = production::retake_compare_sheet(&cmp, rt, 320);
    let color = egui::ColorImage::from_rgba_unmultiplied([sheet.width as usize, sheet.height as usize], &sheet.to_rgba8());
    let tex = ctx.load_texture(format!("retake-compare-{}", retake.0), color, egui::TextureOptions::LINEAR);
    let summary = format!("{}: {:.2}% · {} {:.3}", tr("prod.changed"), cmp.changed_fraction * 100.0, tr("prod.mean_diff"), cmp.mean_abs_diff);
    env.ed.prod.compare = Some((retake.clone(), env.core.change_counter, tex, summary));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_cycle_skips_retake_and_wraps() {
        let mut s = StageStatus::NotStarted;
        let mut seen = vec![s];
        for _ in 0..4 {
            s = next_status(s);
            seen.push(s);
        }
        assert_eq!(seen, vec![StageStatus::NotStarted, StageStatus::InProgress, StageStatus::Review, StageStatus::Approved, StageStatus::NotStarted]);
        assert_eq!(next_status(StageStatus::Retake), StageStatus::NotStarted);
    }

    #[test]
    fn stage_and_status_keys_exist_in_both_languages() {
        use crate::i18n::{Lang, tr_in};
        for s in ALL_STAGES {
            for l in [Lang::Pt, Lang::En] {
                assert_ne!(tr_in(l, stage_key(s)), stage_key(s));
            }
        }
        for s in STATUSES {
            for l in [Lang::Pt, Lang::En] {
                assert_ne!(tr_in(l, status_key(s)), status_key(s));
            }
        }
    }

    #[test]
    fn status_fills_keep_text_readable() {
        for t in [Tokens::dark(), Tokens::light()] {
            for s in STATUSES {
                let c = crate::theme::contrast(t.text, status_fill(&t, s));
                assert!(c >= 4.5, "{s:?}: {c:.2}");
            }
        }
    }

    #[test]
    fn stage_correction_and_retake_go_through_the_bridge_and_undo() {
        let mut core = CoreSession::for_tests("t");
        core.author = AUTHOR.into();
        core.project.settings.width = 64;
        core.project.settings.height = 36;
        let cut = crate::app::commands::quick_start(&mut core, 12, None).unwrap();
        let mut cache = ProdCache::default();
        assert_eq!(cache.get(&core).0.len(), 1);
        actions::run(&mut core, "PRODUCTION", json!({ "op": "set_stage", "cut": cut, "stage": "genga", "status": "review" })).unwrap();
        let (rows, _) = cache.get(&core);
        assert_eq!(rows[0].stages.get(&Stage::Genga), Some(&StageStatus::Review));
        assert_eq!(core.log().last().map(|e| e.author.as_str()), Some(AUTHOR));
        actions::run(&mut core, "CORRECTION", json!({ "op": "add", "cut": cut, "frame": 0, "note": "olho torto" })).unwrap();
        actions::run(&mut core, "RETAKE", json!({ "op": "create", "cut": cut, "frame": 0, "reason": "refazer olho" })).unwrap();
        let (rows, doc) = cache.get(&core);
        assert_eq!((rows[0].open_corrections, rows[0].open_retakes), (1, 1));
        assert_eq!(doc.corrections[0].author, AUTHOR);
        let cmp = production::retake_compare(&core.project, &core.blobs, &doc.retakes[0].id, None).unwrap();
        assert_eq!(cmp.changed_pixels, 0, "nada mudou desde o snapshot");
        core.undo().unwrap();
        core.undo().unwrap();
        assert_eq!(cache.get(&core).0[0].open_corrections, 0);
        assert!(passes(None, &rows[0]));
        assert!(passes(rows[0].scene.as_ref(), &rows[0]));
        assert!(!passes(Some(&Id("outro".into())), &rows[0]));
        let mut r = rows[0].clone();
        r.number = 3;
        r.name = "C003".into();
        assert_eq!(cut_title(&r), "C003");
        r.name = "Ensaio".into();
        assert_eq!(cut_title(&r), "C003  Ensaio");
    }
}
