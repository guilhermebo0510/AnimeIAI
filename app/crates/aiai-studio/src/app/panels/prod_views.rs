//! Painel principal da Produção: o seletor de visão (Lista, Miniaturas, Grande, Quadro), a barra de filtros e as
//! ações em lote. Tudo o que muda o documento passa pelas ações `PRODUCTION`/`RETAKE` da bridge (`actions`): o mesmo
//! undo, o mesmo log e o mesmo autor do MCP. A lógica de ordenar, agrupar e filtrar mora em `prod_logic` (testada sem janela).
//!
//! - **Lista** (カット表): colunas escolhidas pelo usuário, ordenar pelo cabeçalho, agrupar, chips de filtro, visões salvas,
//!   seleção múltipla (Ctrl/Shift) e ações em lote (etapa/estado, responsável, prazo, peso).
//! - **Miniaturas**: grade de cartões com o quadro 0 do corte (`production::cut_thumbnail`, em cache por assinatura do
//!   desenho), selo da etapa atual, cor do estado e barra de 8 segmentos.
//! - **Grande**: um corte por vez com prévia grande e reprodução, linha do tempo das 8 etapas (com o último autor e data),
//!   notas, correções e retakes; `←/→` quadros, `↑/↓` corte anterior/próximo, `Espaço` toca.
//! - **Quadro** (kanban): colunas = estados da etapa escolhida; arrastar o cartão muda o estado.

use super::board::drawing_signature;
use super::prod_logic::{Col, FilterCtx, Filters, Group, GroupKey, ProdView, SavedView, Sort, cut_status, compare, current_stage, group_rows, is_done, progress};
use super::production::{self, STATUSES, THUMB_BUDGET, cut_title, next_status, open_in_animation, secs, set_stage, stage_key, status_fill, status_key};
use super::text_commit_multi;
use crate::app::actions;
use crate::app::commands::{self as cmds, NodeId};
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::theme::Tokens;
use aiai_core::{CoreSession, Stage, StageStatus};
use aiai_tools::production::{ALL_STAGES, CutRow};
use aiai_tools::production_history::{day_of, format_date, parse_date};
use aiai_tools::production_stats::FindingKind;
use egui::{Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};

const ROW_H: f32 = 44.0;
const HEAD_H: f32 = 22.0;
const GAP: f32 = 4.0;
/// Largura das prévias da visão Grande (px).
const BIG_W: u32 = 640;
/// Quadros da visão Grande mantidos em textura.
const BIG_CACHE: usize = 48;

// ------------------------------------------------------------------------------------------------ entrada

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    production::sync_prefs(env);
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    // retângulo útil do painel: as visões se medem por ele, não pela largura que a barra de filtros ocupou (um widget
    // largo demais alarga o `max_rect` do `Ui` e a visão Grande/Quadro sairiam do painel)
    let inner = body.shrink(6.0);
    let (rows, _) = env.ed.prod.cache.get(env.core);
    // marcados que deixaram de existir saem da seleção
    {
        let alive: HashSet<&str> = rows.iter().map(|r| r.cut.0.as_str()).collect();
        env.ed.prod.selection.retain(|s| alive.contains(s.as_str()));
    }
    if rows.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("panel.tasks.empty")).color(t.text_dim));
        return;
    }
    let idx = visible(env, &rows);
    header(env, ui, idx.len(), rows.len());
    if matches!(env.ed.prod.prefs.view, ProdView::List | ProdView::Thumbs) {
        batch_bar(env, ui);
    }
    ui.add_space(2.0);
    match env.ed.prod.prefs.view {
        ProdView::List => list(env, ui, inner, &rows, &idx),
        ProdView::Thumbs => thumbs(env, ui, inner, &rows, &idx),
        ProdView::Big => big(env, ui, inner, &rows, &idx),
        ProdView::Board => board(env, ui, inner, &rows, &idx),
    }
    let alive: HashSet<&str> = rows.iter().map(|r| r.cut.0.as_str()).collect();
    env.ed.prod.thumbs.retain(|k| alive.contains(k));
}

/// Índices (em `rows`) dos cortes que passam no filtro do episódio/cena, nos chips e no achado, na ordem escolhida.
fn visible(env: &mut Env, rows: &[CutRow]) -> Vec<usize> {
    let today = day_of(env.core.clock.now_ms());
    let stuck: HashSet<String> = if env.ed.prod.filters.stuck {
        let a = env.ed.prod.cache.analysis(env.core, env.ed.prod.prefs.stuck_days);
        a.findings.iter().filter(|f| f.kind == FindingKind::Stuck).flat_map(|f| f.cuts.iter().map(|c| c.0.clone())).collect()
    } else {
        HashSet::new()
    };
    let st = &env.ed.prod;
    let cx = FilterCtx { today, stuck: &stuck, only: st.finding.as_ref().map(|(_, s)| s) };
    let mut idx: Vec<usize> = (0..rows.len()).filter(|&i| production::passes(st.filter.as_ref(), &rows[i]) && st.filters.matches(&rows[i], &cx)).collect();
    let sort = st.prefs.sort;
    idx.sort_by(|&a, &b| compare(&rows[a], &rows[b], sort));
    idx
}

// ------------------------------------------------------------------------------------------------ cabeçalho

fn stage_label(s: Stage) -> &'static str {
    tr(stage_key(s))
}

fn header(env: &mut Env, ui: &mut Ui, shown: usize, total: usize) {
    let t = env.t;
    ui.horizontal_wrapped(|ui| {
        for v in ProdView::ALL {
            let on = env.ed.prod.prefs.view == v;
            let r = ui.selectable_label(on, tr(v.key()));
            env.note(format!("prod:view:{}", v.tag()), r.rect);
            if r.clicked() {
                env.ed.prod.prefs.view = v;
            }
        }
        ui.separator();
        let mut search = env.ed.prod.filters.search.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut search).desired_width(120.0).hint_text(tr("prod.search")));
        env.note("prod:search", r.rect);
        env.ed.prod.filters.search = search;
        dim_label(ui, &t, &format!("{shown}/{total}"));
        ui.separator();
        // ações do corte ativo: sugerir etapas pelo conteúdo, lint e abrir na Animação
        let sel = env.ed.active_cut(env.core);
        let m = ui.menu_button(tr("prod.cut_menu"), |ui| {
            let Some(c) = sel.clone() else {
                dim_label(ui, &t, tr("prod.props_none"));
                return;
            };
            let b = ui.button(tr("prod.suggest")).on_hover_text(tr("prod.suggest.tip"));
            env.note("prod:suggest", b.rect);
            if b.clicked() {
                production::suggest(env, &c);
                ui.close();
            }
            let b = ui.button(tr("prod.lint")).on_hover_text(tr("prod.lint.tip"));
            env.note("prod:lint", b.rect);
            if b.clicked() {
                production::lint(env, &c);
                ui.close();
            }
            let b = ui.button(tr("prod.open_cut")).on_hover_text(tr("prod.open_cut.tip"));
            env.note("prod:open_cut", b.rect);
            if b.clicked() {
                open_in_animation(env, &c, None);
                ui.close();
            }
        });
        env.note("prod:cut_menu", m.response.rect);
    });
    ui.horizontal_wrapped(|ui| {
        let mut f = env.ed.prod.filters.clone();
        // estado
        let cur = f.status;
        let r = egui::ComboBox::from_id_salt("prod_f_status").width(86.0).selected_text(cur.map_or(tr("prod.f.status_all"), |s| tr(status_key(s)))).show_ui(ui, |ui| {
            if ui.selectable_label(cur.is_none(), tr("prod.f.any")).clicked() {
                f.status = None;
            }
            for s in STATUSES {
                if ui.selectable_label(cur == Some(s), tr(status_key(s))).clicked() {
                    f.status = Some(s);
                }
            }
        });
        env.note("prod:chip:status", r.response.rect);
        // etapa
        let cur = f.stage;
        let r = egui::ComboBox::from_id_salt("prod_f_stage").width(76.0).selected_text(cur.map_or(tr("prod.f.stage_all"), stage_label)).show_ui(ui, |ui| {
            if ui.selectable_label(cur.is_none(), tr("prod.f.any")).clicked() {
                f.stage = None;
            }
            for s in ALL_STAGES {
                if ui.selectable_label(cur == Some(s), stage_label(s)).clicked() {
                    f.stage = Some(s);
                }
            }
        });
        env.note("prod:chip:stage", r.response.rect);
        let r = ui.toggle_value(&mut f.retake, tr("prod.f.retake"));
        env.note("prod:chip:retake", r.rect);
        let r = ui.toggle_value(&mut f.overdue, tr("prod.f.overdue"));
        env.note("prod:chip:overdue", r.rect);
        let r = ui.toggle_value(&mut f.stuck, tr("prod.f.stuck")).on_hover_text(tr("prod.f.stuck.tip"));
        env.note("prod:chip:stuck", r.rect);
        // responsável
        let mut who: Vec<String> = env.ed.prod.cache.get(env.core).0.iter().filter_map(|r| r.assignee.clone()).collect();
        who.sort_by_key(|s| s.to_lowercase());
        who.dedup();
        if !who.is_empty() {
            let cur = f.assignee.clone();
            egui::ComboBox::from_id_salt("prod_f_who").width(86.0).selected_text(cur.clone().unwrap_or_else(|| tr("prod.f.who_all").to_owned())).show_ui(ui, |ui| {
                if ui.selectable_label(cur.is_none(), tr("prod.f.any")).clicked() {
                    f.assignee = None;
                }
                for w in &who {
                    if ui.selectable_label(cur.as_deref() == Some(w.as_str()), w).clicked() {
                        f.assignee = Some(w.clone());
                    }
                }
            });
        }
        if !f.is_empty() || env.ed.prod.finding.is_some() {
            let r = ui.small_button(tr("prod.f.clear"));
            env.note("prod:chip:clear", r.rect);
            if r.clicked() {
                f = Filters::default();
                env.ed.prod.finding = None;
            }
        }
        env.ed.prod.filters = f;
    });
    ui.horizontal_wrapped(|ui| {
        // agrupar
        let cur = env.ed.prod.prefs.group;
        let mut group = cur;
        let r = egui::ComboBox::from_id_salt("prod_group").width(100.0).selected_text(tr(cur.key())).show_ui(ui, |ui| {
            for g in Group::ALL {
                ui.selectable_value(&mut group, g, tr(g.key()));
            }
        });
        env.note("prod:group", r.response.rect);
        env.ed.prod.prefs.group = group;
        // colunas (só na Lista)
        if env.ed.prod.prefs.view == ProdView::List {
            let b = ui.menu_button(tr("prod.columns"), |ui| {
                let mut cols = env.ed.prod.prefs.cols.clone();
                for c in Col::all() {
                    let mut on = cols.contains(&c);
                    if ui.checkbox(&mut on, tr(c.title_key())).changed() {
                        if on {
                            // mantém a ordem do menu
                            cols.push(c);
                            let all = Col::all();
                            cols.sort_by_key(|x| all.iter().position(|y| y == x).unwrap_or(usize::MAX));
                        } else {
                            cols.retain(|x| *x != c);
                        }
                    }
                }
                if ui.button(tr("prod.columns.default")).clicked() {
                    cols = Col::defaults();
                }
                env.ed.prod.prefs.cols = cols;
            });
            env.note("prod:columns", b.response.rect);
        }
        // visões salvas
        let b = ui.menu_button(tr("prod.saved_views"), |ui| saved_views_menu(env, ui));
        env.note("prod:saved_views", b.response.rect);
    });
    if let Some((label, ids)) = env.ed.prod.finding.clone() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{}: {label} ({})", tr("prod.finding_filter"), ids.len())).size(12.0).color(t.accent));
        });
    }
    if let Some(w) = env.ed.prod.cache.hist.warn.take() {
        env.ed.notify(w);
    }
}

fn saved_views_menu(env: &mut Env, ui: &mut Ui) {
    let saved = env.ed.prod.prefs.saved.clone();
    if saved.is_empty() {
        ui.label(egui::RichText::new(tr("prod.saved_none")).color(env.t.text_dim));
    }
    for (i, v) in saved.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui.button(&v.name).clicked() {
                let p = &mut env.ed.prod;
                p.prefs.view = v.view;
                p.prefs.cols = v.cols.clone();
                p.prefs.sort = v.sort;
                p.prefs.group = v.group;
                p.filters = v.filters.clone();
                p.finding = None;
                ui.close();
            }
            if ui.small_button("x").on_hover_text(tr("prod.saved_delete")).clicked() {
                env.ed.prod.prefs.saved.remove(i);
                ui.close();
            }
        });
    }
    ui.separator();
    ui.horizontal(|ui| {
        let mut name = env.ed.prod.save_view_name.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(110.0).hint_text(tr("prod.saved_name")));
        env.note("prod:saved_name", r.rect);
        env.ed.prod.save_view_name = name.clone();
        let b = ui.add_enabled(!name.trim().is_empty(), egui::Button::new(tr("prod.saved_save")));
        env.note("prod:saved_save", b.rect);
        if b.clicked() {
            let p = &mut env.ed.prod;
            let view = SavedView { name: name.trim().to_owned(), view: p.prefs.view, cols: p.prefs.cols.clone(), sort: p.prefs.sort, group: p.prefs.group, filters: p.filters.clone() };
            match p.prefs.saved.iter().position(|v| v.name == view.name) {
                Some(at) => p.prefs.saved[at] = view,
                None => p.prefs.saved.push(view),
            }
            p.save_view_name.clear();
            ui.close();
        }
    });
}

// ------------------------------------------------------------------------------------------------ ações em lote

/// Barra das ações em lote (aparece com cortes marcados): etapa/estado, responsável, prazo e peso.
fn batch_bar(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let n = env.ed.prod.selection.len();
    if n == 0 {
        return;
    }
    let cuts: Vec<String> = env.ed.prod.selection.clone();
    egui::Frame::new().fill(t.bg_widget).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("{n} {}", tr("prod.batch.selected"))).size(12.0).strong());
            let b = &mut env.ed.prod.batch;
            let mut stage = b.stage;
            let mut status = b.status;
            egui::ComboBox::from_id_salt("prod_b_stage").width(80.0).selected_text(stage_label(stage)).show_ui(ui, |ui| {
                for s in ALL_STAGES {
                    ui.selectable_value(&mut stage, s, stage_label(s));
                }
            });
            egui::ComboBox::from_id_salt("prod_b_status").width(100.0).selected_text(tr(status_key(status))).show_ui(ui, |ui| {
                for s in STATUSES {
                    ui.selectable_value(&mut status, s, tr(status_key(s)));
                }
            });
            b.stage = stage;
            b.status = status;
            let r = ui.button(tr("prod.batch.apply_stage"));
            env.note("prod:batch:stage", r.rect);
            if r.clicked() {
                let res = actions::run(env.core, "PRODUCTION", json!({ "op": "set_stages", "cuts": cuts, "stage": stage, "status": status }));
                actions::notify_err(env.ed, res);
            }
            ui.separator();
            let mut who = env.ed.prod.batch.assignee.clone();
            let r = ui.add(egui::TextEdit::singleline(&mut who).desired_width(80.0).hint_text(tr("prod.col.assignee")));
            env.note("prod:batch:assignee", r.rect);
            env.ed.prod.batch.assignee = who.clone();
            let r = ui.button(tr("prod.batch.apply_assignee")).on_hover_text(tr("prod.batch.clear_tip"));
            env.note("prod:batch:assignee_apply", r.rect);
            if r.clicked() {
                let res = actions::run(env.core, "PRODUCTION", json!({ "op": "set_meta", "cuts": cuts, "assignee": who.trim() }));
                actions::notify_err(env.ed, res);
            }
            ui.separator();
            let mut due = env.ed.prod.batch.due.clone();
            let r = ui.add(egui::TextEdit::singleline(&mut due).desired_width(78.0).hint_text("AAAA-MM-DD"));
            env.note("prod:batch:due", r.rect);
            env.ed.prod.batch.due = due.clone();
            let valid = due.trim().is_empty() || parse_date(due.trim()).is_some();
            let r = ui.add_enabled(valid, egui::Button::new(tr("prod.batch.apply_due"))).on_hover_text(tr("prod.batch.clear_tip"));
            env.note("prod:batch:due_apply", r.rect);
            if r.clicked() {
                let res = actions::run(env.core, "PRODUCTION", json!({ "op": "set_meta", "cuts": cuts, "due": due.trim() }));
                actions::notify_err(env.ed, res);
            }
            ui.separator();
            let mut w = env.ed.prod.batch.weight;
            let r = ui.add(egui::DragValue::new(&mut w).range(0.0..=100.0).speed(0.05).max_decimals(2).prefix(format!("{} ", tr("prod.col.weight"))));
            env.note("prod:batch:weight", r.rect);
            env.ed.prod.batch.weight = w;
            let r = ui.button(tr("prod.batch.apply_weight")).on_hover_text(tr("prod.batch.clear_tip"));
            env.note("prod:batch:weight_apply", r.rect);
            if r.clicked() {
                let res = actions::run(env.core, "PRODUCTION", json!({ "op": "set_meta", "cuts": cuts, "weight": w }));
                actions::notify_err(env.ed, res);
            }
            ui.separator();
            let r = ui.button(tr("prod.batch.clear_selection"));
            env.note("prod:batch:clear", r.rect);
            if r.clicked() {
                env.ed.prod.selection.clear();
            }
        });
    });

}

/// Seleção por clique: simples troca, Ctrl soma/tira, Shift marca o intervalo (na ordem em que a lista está na tela).
fn click_select(env: &mut Env, id: &str, order: &[String], ctrl: bool, shift: bool) {
    let st = &mut env.ed.prod;
    if shift {
        let a = st.anchor.clone().unwrap_or_else(|| id.to_owned());
        let (i, j) = (order.iter().position(|x| *x == a), order.iter().position(|x| x == id));
        if let (Some(i), Some(j)) = (i, j) {
            let (lo, hi) = (i.min(j), i.max(j));
            for x in &order[lo..=hi] {
                if !st.selection.contains(x) {
                    st.selection.push(x.clone());
                }
            }
        }
    } else if ctrl {
        match st.selection.iter().position(|x| x == id) {
            Some(p) => {
                st.selection.remove(p);
            }
            None => st.selection.push(id.to_owned()),
        }
        st.anchor = Some(id.to_owned());
    } else {
        st.selection = vec![id.to_owned()];
        st.anchor = Some(id.to_owned());
        env.ed.set_cut(aiai_core::Id(id.to_owned()));
    }
}

fn group_title(k: &GroupKey) -> String {
    match k {
        GroupKey::All => String::new(),
        GroupKey::Scene(n) => n.clone(),
        GroupKey::Stage(Some(s)) => stage_label(*s).to_owned(),
        GroupKey::Stage(None) => tr("prod.done").to_owned(),
        GroupKey::Assignee(Some(a)) => a.clone(),
        GroupKey::Assignee(None) => tr("prod.group.unassigned").to_owned(),
        GroupKey::Status(s) => tr(status_key(*s)).to_owned(),
    }
}

/// Cabeçalho de grupo (recolhível): título, nº de cortes e a média de etapas aprovadas.
fn group_header(env: &mut Env, ui: &mut Ui, key: &str, title: &str, members: &[&CutRow], width: f32) {
    let t = env.t;
    let collapsed = env.ed.prod.collapsed.contains(key);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 22.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, if resp.hovered() { t.bg_hover } else { t.bg_widget });
    let avg = members.iter().map(|r| progress(r)).sum::<f32>() / members.len().max(1) as f32;
    // seta
    let c = egui::pos2(rect.left() + 11.0, rect.center().y);
    let pts = if collapsed { vec![c + egui::vec2(-3.0, -4.0), c + egui::vec2(-3.0, 4.0), c + egui::vec2(3.0, 0.0)] } else { vec![c + egui::vec2(-4.0, -3.0), c + egui::vec2(4.0, -3.0), c + egui::vec2(0.0, 3.0)] };
    p.add(egui::Shape::convex_polygon(pts, t.text_dim, Stroke::NONE));
    p.text(egui::pos2(rect.left() + 22.0, rect.center().y), egui::Align2::LEFT_CENTER, format!("{title}  ·  {} {}", members.len(), tr("prod.cuts_short")), FontId::proportional(12.5), t.text);
    let bar = Rect::from_min_size(egui::pos2(rect.right() - 90.0, rect.center().y - 4.0), egui::vec2(80.0, 8.0));
    p.rect_filled(bar, 2.0, t.bg_panel);
    p.rect_filled(Rect::from_min_size(bar.min, egui::vec2(bar.width() * avg, bar.height())), 2.0, t.st_ok);
    p.rect_stroke(bar, 2.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    if resp.clicked() {
        if collapsed {
            env.ed.prod.collapsed.remove(key);
        } else {
            env.ed.prod.collapsed.insert(key.to_owned());
        }
    }
}

// ------------------------------------------------------------------------------------------------ pequenos desenhos

/// Etiqueta de estado: fundo do estado, texto por cima.
fn chip(painter: &egui::Painter, rect: Rect, t: &Tokens, st: StageStatus, text: &str, hovered: bool) {
    painter.rect(rect, 3.0, status_fill(t, st), Stroke::new(1.0, if hovered { t.text_dim } else { t.border }), StrokeKind::Inside);
    painter.with_clip_rect(rect.shrink(1.0)).text(rect.center(), egui::Align2::CENTER_CENTER, text, FontId::proportional(10.5), t.text);
}

fn short_status(st: StageStatus) -> &'static str {
    tr(match st {
        StageStatus::NotStarted => "prod.short.not_started",
        StageStatus::InProgress => "prod.short.in_progress",
        StageStatus::Review => "prod.short.review",
        StageStatus::Approved => "prod.short.approved",
        StageStatus::Retake => "prod.short.retake",
    })
}

/// Barra de 8 segmentos (um por etapa) com a cor do estado de cada uma.
fn stage_strip(painter: &egui::Painter, rect: Rect, t: &Tokens, r: &CutRow) {
    let n = ALL_STAGES.len() as f32;
    let seg = (rect.width() - (n - 1.0)) / n;
    for (k, s) in ALL_STAGES.iter().enumerate() {
        let st = r.stages.get(s).copied().unwrap_or_default();
        let x = rect.left() + k as f32 * (seg + 1.0);
        painter.rect_filled(Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(seg, rect.height())), 1.0, strip_color(t, st));
    }
}

/// Cor de segmento/barra: o estado em tom visível sobre o fundo do painel (os `st_*` do tema são fundos de etiqueta).
pub(crate) fn strip_color(t: &Tokens, st: StageStatus) -> Color32 {
    let dark = (t.bg_panel.r() as u32 + t.bg_panel.g() as u32 + t.bg_panel.b() as u32) / 3 < 128;
    match (st, dark) {
        (StageStatus::NotStarted, _) => t.border,
        (StageStatus::InProgress, true) => Color32::from_rgb(0x4a, 0x86, 0xd8),
        (StageStatus::InProgress, false) => Color32::from_rgb(0x2b, 0x62, 0xb0),
        (StageStatus::Review, true) => Color32::from_rgb(0xd9, 0xa8, 0x2e),
        (StageStatus::Review, false) => Color32::from_rgb(0xb0, 0x80, 0x10),
        (StageStatus::Approved, true) => Color32::from_rgb(0x4c, 0xaf, 0x6a),
        (StageStatus::Approved, false) => Color32::from_rgb(0x2a, 0x8a, 0x48),
        (StageStatus::Retake, true) => Color32::from_rgb(0xe0, 0x5a, 0x50),
        (StageStatus::Retake, false) => Color32::from_rgb(0xc0, 0x30, 0x30),
    }
}

fn due_text(r: &CutRow, today: i64) -> Option<(String, bool)> {
    let d = r.due.as_deref().and_then(parse_date)?;
    let text = r.due.clone().unwrap_or_default();
    Some((text, d < today && !is_done(r)))
}

fn thumb_rect(env: &Env, avail: Rect) -> Rect {
    let p = &env.core.project;
    let aspect = p.settings.width.max(1) as f32 / p.settings.height.max(1) as f32;
    let (mut w, mut h) = (avail.width(), avail.width() / aspect);
    if h > avail.height() {
        h = avail.height();
        w = h * aspect;
    }
    Rect::from_center_size(avail.center(), egui::vec2(w, h))
}

fn paint_thumb(env: &Env, painter: &egui::Painter, rect: Rect, tex: Option<egui::TextureId>) {
    let t = env.t;
    painter.rect_filled(rect, 2.0, Color32::WHITE);
    match tex {
        Some(id) => {
            painter.image(id, rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "…", FontId::proportional(16.0), Color32::from_gray(120));
        }
    }
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
}

// ------------------------------------------------------------------------------------------------ Lista

fn list(env: &mut Env, ui: &mut Ui, inner: Rect, rows: &[CutRow], idx: &[usize]) {
    let t = env.t;
    let cols = env.ed.prod.prefs.cols.clone();
    if cols.is_empty() {
        dim_label(ui, &t, tr("prod.no_columns"));
        return;
    }
    if idx.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("prod.none_in_filter")).color(t.text_dim));
        return;
    }
    let group = env.ed.prod.prefs.group;
    let groups = group_rows(idx.iter().map(|&i| &rows[i]).collect(), group);
    let key_of = |k: &GroupKey| format!("{k:?}");
    let order: Vec<String> = groups.iter().filter(|(k, _)| !env.ed.prod.collapsed.contains(&key_of(k))).flat_map(|(_, v)| v.iter().map(|r| r.cut.0.clone())).collect();
    let total_w: f32 = cols.iter().map(|c| c.width() + GAP).sum::<f32>() + 12.0;
    let row_w = total_w.max(inner.width() - 12.0);
    let today = day_of(env.core.clock.now_ms());
    let ctx = ui.ctx().clone();
    let mut budget = THUMB_BUDGET;
    egui::ScrollArea::both().id_salt("prod_list").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        list_head(env, ui, &cols, row_w);
        let mut i = 0usize;
        for (k, members) in &groups {
            if group != Group::None {
                group_header(env, ui, &key_of(k), &group_title(k), members, row_w);
                if env.ed.prod.collapsed.contains(&key_of(k)) {
                    continue;
                }
            }
            for r in members {
                let tex = if cols.contains(&Col::Thumb) { env.ed.prod.thumbs.get(&ctx, env.core, &r.cut, &mut budget) } else { None };
                list_row(env, ui, i, r, &cols, tex, &order, today, row_w);
                i += 1;
            }
        }
    });
}

fn list_head(env: &mut Env, ui: &mut Ui, cols: &[Col], row_w: f32) {
    let t = env.t;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(row_w, HEAD_H), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, t.bg_header);
    let mut x = rect.left() + 6.0;
    let sort = env.ed.prod.prefs.sort;
    for c in cols {
        let cell = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(c.width(), HEAD_H));
        x += c.width() + GAP;
        let resp = ui.interact(cell, egui::Id::new(("prod_head", c.title_key(), format!("{c:?}"))), if c.sortable() { Sense::click() } else { Sense::hover() });
        env.note(format!("prod:head:{c:?}"), cell);
        let on = sort.col == *c;
        let painter = ui.painter().with_clip_rect(cell);
        if resp.hovered() && c.sortable() {
            painter.rect_filled(cell, 2.0, t.bg_hover);
        }
        painter.text(egui::pos2(cell.left() + 2.0, cell.center().y), egui::Align2::LEFT_CENTER, tr(c.title_key()), FontId::proportional(11.5), if on { t.text } else { t.text_dim });
        if on {
            let cx = cell.right() - 7.0;
            let cy = cell.center().y;
            let pts = if sort.asc { vec![egui::pos2(cx - 3.5, cy + 2.0), egui::pos2(cx + 3.5, cy + 2.0), egui::pos2(cx, cy - 3.0)] } else { vec![egui::pos2(cx - 3.5, cy - 2.0), egui::pos2(cx + 3.5, cy - 2.0), egui::pos2(cx, cy + 3.0)] };
            painter.add(egui::Shape::convex_polygon(pts, t.accent, Stroke::NONE));
        }
        if resp.clicked() && c.sortable() {
            env.ed.prod.prefs.sort = if on { Sort { col: *c, asc: !sort.asc } } else { Sort { col: *c, asc: true } };
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn list_row(env: &mut Env, ui: &mut Ui, i: usize, r: &CutRow, cols: &[Col], tex: Option<egui::TextureId>, order: &[String], today: i64, row_w: f32) {
    let t = env.t;
    let selected = env.ed.prod.selection.contains(&r.cut.0);
    let active = env.ed.cut.as_ref() == Some(&r.cut);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(row_w, ROW_H), Sense::click());
    env.note(format!("prod:cut:{i}"), rect);
    let painter = ui.painter().clone();
    if selected {
        painter.rect_filled(rect, 4.0, t.accent_dim);
    } else if resp.hovered() {
        painter.rect_filled(rect, 4.0, t.bg_hover);
    } else if i % 2 == 1 {
        painter.rect_filled(rect, 4.0, t.bg_widget.gamma_multiply(0.35));
    }
    if active {
        painter.rect_filled(Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 4.0), egui::vec2(3.0, ROW_H - 8.0)), 1.5, t.accent);
    }
    let mut x = rect.left() + 6.0;
    let mut chip_clicked = false;
    for c in cols {
        let w = c.width();
        let cell = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, ROW_H));
        x += w + GAP;
        let clip = painter.with_clip_rect(cell);
        let mid = |dx: f32| egui::pos2(cell.left() + dx, cell.center().y);
        match *c {
            Col::Thumb => {
                let tr_ = thumb_rect(env, cell.shrink2(egui::vec2(2.0, 4.0)));
                paint_thumb(env, &painter, tr_, tex);
            }
            Col::Cut => {
                let mut head = cut_title(r);
                if r.open_retakes > 0 {
                    head.push_str(&format!("  ·  {} {}", r.open_retakes, tr("prod.retakes_short")));
                }
                clip.text(mid(2.0), egui::Align2::LEFT_CENTER, head, FontId::proportional(12.5), t.text);
            }
            Col::Scene => {
                clip.text(mid(2.0), egui::Align2::LEFT_CENTER, &r.scene_name, FontId::proportional(12.0), t.text_dim);
            }
            Col::Frames => {
                clip.text(mid(2.0), egui::Align2::LEFT_CENTER, format!("{}", r.duration), FontId::proportional(12.0), t.text_dim);
            }
            Col::Current => {
                let rr = cell.shrink2(egui::vec2(1.0, 12.0));
                match current_stage(r) {
                    Some(s) => chip(&painter, rr, &t, r.stages.get(&s).copied().unwrap_or_default(), stage_label(s), false),
                    None => chip(&painter, rr, &t, StageStatus::Approved, tr("prod.done"), false),
                }
            }
            Col::Status => {
                let st = cut_status(r);
                chip(&painter, cell.shrink2(egui::vec2(1.0, 12.0)), &t, st, tr(status_key(st)), false);
            }
            Col::Stage(s) => {
                let st = r.stages.get(&s).copied().unwrap_or_default();
                let cr = cell.shrink2(egui::vec2(1.0, 11.0));
                let cresp = ui.interact(cr, egui::Id::new(("prod_chip", &r.cut.0, s as u8)), Sense::click());
                env.note(format!("prod:stage:{i}:{}", s as u8), cr);
                chip(&painter, cr, &t, st, short_status(st), cresp.hovered());
                let cresp = cresp.on_hover_text(format!("{}: {}\n{}", stage_label(s), tr(status_key(st)), tr("prod.chip.tip")));
                if cresp.clicked() {
                    chip_clicked = true;
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
            Col::Progress => {
                let bar = Rect::from_min_size(egui::pos2(cell.left() + 2.0, cell.center().y - 4.0), egui::vec2(w - 4.0, 8.0));
                stage_strip(&painter, bar, &t, r);
            }
            Col::Assignee => match &r.assignee {
                Some(a) => {
                    clip.text(mid(2.0), egui::Align2::LEFT_CENTER, a, FontId::proportional(12.0), t.text);
                }
                None => {
                    clip.text(mid(2.0), egui::Align2::LEFT_CENTER, "—", FontId::proportional(12.0), t.text_dim);
                }
            },
            Col::Due => match due_text(r, today) {
                Some((d, late)) => {
                    clip.text(mid(2.0), egui::Align2::LEFT_CENTER, d, FontId::proportional(12.0), if late { t.danger } else { t.text });
                }
                None => {
                    clip.text(mid(2.0), egui::Align2::LEFT_CENTER, "—", FontId::proportional(12.0), t.text_dim);
                }
            },
            Col::Weight => {
                let s = r.weight.map_or("—".to_owned(), |w| format!("{w:.1}"));
                clip.text(mid(2.0), egui::Align2::LEFT_CENTER, s, FontId::proportional(12.0), if r.weight.is_some() { t.text } else { t.text_dim });
            }
            Col::Retakes => {
                clip.text(mid(2.0), egui::Align2::LEFT_CENTER, format!("{}", r.open_retakes), FontId::proportional(12.0), if r.open_retakes > 0 { t.danger } else { t.text_dim });
            }
            Col::Corrections => {
                clip.text(mid(2.0), egui::Align2::LEFT_CENTER, format!("{}", r.open_corrections), FontId::proportional(12.0), if r.open_corrections > 0 { t.accent } else { t.text_dim });
            }
        }
    }
    if resp.clicked() && !chip_clicked {
        let m = ui.input(|i| i.modifiers);
        click_select(env, &r.cut.0, order, m.command || m.ctrl, m.shift);
    }
    if resp.double_clicked() {
        open_in_animation(env, &r.cut, None);
    }
}

// ------------------------------------------------------------------------------------------------ Miniaturas

fn thumbs(env: &mut Env, ui: &mut Ui, inner: Rect, rows: &[CutRow], idx: &[usize]) {
    let t = env.t;
    // tamanho: P / M / G
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("prod.thumb_size"));
        for (k, key) in ["prod.size.s", "prod.size.m", "prod.size.l"].into_iter().enumerate() {
            let on = env.ed.prod.prefs.thumb_size as usize == k;
            let r = ui.selectable_label(on, tr(key));
            env.note(format!("prod:thumb_size:{k}"), r.rect);
            if r.clicked() {
                env.ed.prod.prefs.thumb_size = k as u8;
            }
        }
    });
    if idx.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("prod.none_in_filter")).color(t.text_dim));
        return;
    }
    let card_w = [128.0f32, 176.0, 252.0][env.ed.prod.prefs.thumb_size.min(2) as usize];
    let p = &env.core.project;
    let aspect = p.settings.width.max(1) as f32 / p.settings.height.max(1) as f32;
    let card_h = card_w / aspect + 46.0;
    let group = env.ed.prod.prefs.group;
    let groups = group_rows(idx.iter().map(|&i| &rows[i]).collect(), group);
    let key_of = |k: &GroupKey| format!("{k:?}");
    let order: Vec<String> = groups.iter().filter(|(k, _)| !env.ed.prod.collapsed.contains(&key_of(k))).flat_map(|(_, v)| v.iter().map(|r| r.cut.0.clone())).collect();
    let today = day_of(env.core.clock.now_ms());
    let ctx = ui.ctx().clone();
    let mut budget = THUMB_BUDGET;
    egui::ScrollArea::vertical().id_salt("prod_thumbs").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        let per_row = (((inner.width() - 12.0 + 8.0) / (card_w + 8.0)).floor() as usize).max(1);
        let mut n = 0usize;
        for (k, members) in &groups {
            if group != Group::None {
                group_header(env, ui, &key_of(k), &group_title(k), members, inner.width() - 12.0);
                if env.ed.prod.collapsed.contains(&key_of(k)) {
                    continue;
                }
            }
            for chunk in members.chunks(per_row) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for r in chunk {
                        let tex = env.ed.prod.thumbs.get(&ctx, env.core, &r.cut, &mut budget);
                        thumb_card(env, ui, n, r, tex, egui::vec2(card_w, card_h), &order, today);
                        n += 1;
                    }
                });
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn thumb_card(env: &mut Env, ui: &mut Ui, i: usize, r: &CutRow, tex: Option<egui::TextureId>, size: Vec2, order: &[String], today: i64) {
    let t = env.t;
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    env.note(format!("prod:card:{i}"), rect);
    let selected = env.ed.prod.selection.contains(&r.cut.0);
    let active = env.ed.cut.as_ref() == Some(&r.cut);
    let status = cut_status(r);
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 6.0, if resp.hovered() { t.bg_hover } else { t.bg_widget });
    // faixa de cor do estado geral à esquerda
    painter.rect_filled(Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 6.0), egui::vec2(3.0, rect.height() - 12.0)), 1.5, strip_color(&t, status));
    let pic = Rect::from_min_max(rect.left_top() + egui::vec2(6.0, 6.0), egui::pos2(rect.right() - 6.0, rect.bottom() - 40.0));
    let pr = thumb_rect(env, pic);
    paint_thumb(env, &painter, pr, tex);
    // selo de retake no canto da miniatura
    if r.open_retakes > 0 {
        let b = Rect::from_min_size(egui::pos2(pr.right() - 34.0, pr.top() + 3.0), egui::vec2(31.0, 15.0));
        painter.rect_filled(b, 3.0, t.st_retake);
        painter.text(b.center(), egui::Align2::CENTER_CENTER, format!("R{}", r.open_retakes), FontId::proportional(10.5), t.text);
    }
    // linha 1: título e duração; linha 2: selo da etapa atual e prazo; linha 3: barra de etapas
    let y0 = pic.bottom() + 4.0;
    let clip = painter.with_clip_rect(Rect::from_min_max(egui::pos2(rect.left() + 6.0, y0), egui::pos2(rect.right() - 6.0, rect.bottom())));
    clip.text(egui::pos2(rect.left() + 8.0, y0 + 6.0), egui::Align2::LEFT_CENTER, cut_title(r), FontId::proportional(12.0), t.text);
    clip.text(egui::pos2(rect.right() - 8.0, y0 + 6.0), egui::Align2::RIGHT_CENTER, format!("{:.1} s", secs(r.duration, &env.core.project)), FontId::proportional(11.0), t.text_dim);
    let badge = Rect::from_min_size(egui::pos2(rect.left() + 8.0, y0 + 14.0), egui::vec2(60.0, 13.0));
    match current_stage(r) {
        Some(s) => chip(&painter, badge, &t, r.stages.get(&s).copied().unwrap_or_default(), stage_label(s), false),
        None => chip(&painter, badge, &t, StageStatus::Approved, tr("prod.done"), false),
    }
    if let Some((d, late)) = due_text(r, today) {
        clip.text(egui::pos2(rect.right() - 8.0, y0 + 20.0), egui::Align2::RIGHT_CENTER, d, FontId::proportional(10.5), if late { t.danger } else { t.text_dim });
    } else if let Some(a) = &r.assignee {
        clip.text(egui::pos2(rect.right() - 8.0, y0 + 20.0), egui::Align2::RIGHT_CENTER, a, FontId::proportional(10.5), t.text_dim);
    }
    stage_strip(&painter, Rect::from_min_size(egui::pos2(rect.left() + 8.0, y0 + 30.0), egui::vec2(rect.width() - 16.0, 5.0)), &t, r);
    if selected || active {
        painter.rect_stroke(rect, 6.0, Stroke::new(if selected { 2.0 } else { 1.0 }, t.accent), StrokeKind::Inside);
    }
    if resp.clicked() {
        let m = ui.input(|i| i.modifiers);
        click_select(env, &r.cut.0, order, m.command || m.ctrl, m.shift);
    }
    if resp.double_clicked() {
        open_in_animation(env, &r.cut, None);
    }
    resp.context_menu(|ui| {
        if ui.button(tr("prod.open_cut")).clicked() {
            open_in_animation(env, &r.cut, None);
            ui.close();
        }
        if ui.button(tr("prod.open_big")).clicked() {
            env.ed.set_cut(r.cut.clone());
            env.ed.prod.prefs.view = ProdView::Big;
            ui.close();
        }
    });
}

// ------------------------------------------------------------------------------------------------ Grande

struct BigFrame {
    tex: egui::TextureHandle,
}

/// Visão Grande: quadro mostrado, reprodução e as texturas dos quadros já renderizados do corte aberto.
#[derive(Default)]
pub(crate) struct BigState {
    pub frame: u32,
    pub playing: bool,
    /// Quadros acumulados desde o último avanço (reprodução em tempo real).
    acc: f64,
    cut: Option<String>,
    /// Assinatura do desenho do corte e o `change_counter` em que foi conferida.
    sig: u64,
    checked: u64,
    frames: HashMap<u32, BigFrame>,
    order: VecDeque<u32>,
    /// Retake: motivo em edição.
    pub reason: String,
}

impl BigState {
    /// Textura do quadro, renderizando no máximo um por chamada (`budget`); sem ela, o quadro mais próximo já pronto.
    fn texture(&mut self, ctx: &egui::Context, core: &CoreSession, cut: &NodeId, frame: u32, budget: &mut u32) -> Option<(egui::TextureId, Vec2)> {
        if self.cut.as_deref() != Some(cut.0.as_str()) {
            self.cut = Some(cut.0.clone());
            self.frames.clear();
            self.order.clear();
            self.checked = u64::MAX;
        }
        if self.checked != core.change_counter {
            let sig = drawing_signature(&core.project, cut);
            if sig != self.sig {
                self.sig = sig;
                self.frames.clear();
                self.order.clear();
            }
            self.checked = core.change_counter;
        }
        if let Some(f) = self.frames.get(&frame) {
            return Some((f.tex.id(), f.tex.size_vec2()));
        }
        if *budget > 0 {
            *budget -= 1;
            if let Ok(img) = aiai_tools::production::cut_thumbnail(&core.project, &core.blobs, cut, frame, BIG_W) {
                let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.to_rgba8());
                let tex = ctx.load_texture(format!("prod-big-{}-{frame}", cut.0), color, egui::TextureOptions::LINEAR);
                let out = (tex.id(), tex.size_vec2());
                self.frames.insert(frame, BigFrame { tex });
                self.order.push_back(frame);
                while self.order.len() > BIG_CACHE {
                    if let Some(old) = self.order.pop_front() {
                        self.frames.remove(&old);
                    }
                }
                return Some(out);
            }
        }
        ctx.request_repaint();
        // o mais próximo já renderizado
        self.frames.iter().min_by_key(|(f, _)| f.abs_diff(frame)).map(|(_, f)| (f.tex.id(), f.tex.size_vec2()))
    }
}

fn big(env: &mut Env, ui: &mut Ui, inner: Rect, rows: &[CutRow], idx: &[usize]) {
    let t = env.t;
    if idx.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("prod.none_in_filter")).color(t.text_dim));
        return;
    }
    // corte aberto: o ativo do editor se estiver na lista; senão o primeiro
    let active = env.ed.active_cut(env.core);
    let pos = active.as_ref().and_then(|c| idx.iter().position(|&i| &rows[i].cut == c)).unwrap_or(0);
    let r = &rows[idx[pos]];
    if active.as_ref() != Some(&r.cut) {
        env.ed.set_cut(r.cut.clone());
    }
    let cut = r.cut.clone();
    let duration = r.duration.max(1);
    let fps = env.core.project.settings.fps();
    let area = Rect::from_min_max(ui.cursor().min, inner.max);
    let hover = ui.rect_contains_pointer(area);
    let typing = ui.ctx().egui_wants_keyboard_input();

    // ---- teclado e reprodução
    let mut go: Option<usize> = None;
    if hover && !typing {
        let (left, right, up, down, space) = ui.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::Space)));
        let b = &mut env.ed.prod.big;
        if left {
            b.playing = false;
            b.frame = b.frame.saturating_sub(1);
        }
        if right {
            b.playing = false;
            b.frame = (b.frame + 1).min(duration - 1);
        }
        if space {
            b.playing = !b.playing;
        }
        if up && pos > 0 {
            go = Some(pos - 1);
        }
        if down && pos + 1 < idx.len() {
            go = Some(pos + 1);
        }
    }
    if let Some(p) = go {
        env.ed.set_cut(rows[idx[p]].cut.clone());
        env.ed.prod.big.frame = 0;
        env.ed.prod.big.playing = false;
        return;
    }
    {
        let dt = f64::from(ui.input(|i| i.stable_dt)).min(0.25);
        let b = &mut env.ed.prod.big;
        if b.playing {
            b.acc += dt * fps;
            let n = b.acc.floor();
            if n >= 1.0 {
                b.acc -= n;
                b.frame = (b.frame + n as u32) % duration;
            }
            ui.ctx().request_repaint();
        }
        b.frame = b.frame.min(duration - 1);
    }

    // ---- divisão: prévia à esquerda, etapas e notas à direita
    let side_w = 300.0f32.min(area.width() * 0.42);
    let left = Rect::from_min_max(area.min, egui::pos2(area.right() - side_w - 8.0, area.bottom()));
    let right = Rect::from_min_max(egui::pos2(area.right() - side_w, area.top()), area.max);
    {
        let mut lu = ui.new_child(egui::UiBuilder::new().max_rect(left));
        big_preview(env, &mut lu, left, rows, idx, pos);
    }
    {
        let mut ru = ui.new_child(egui::UiBuilder::new().max_rect(right));
        egui::ScrollArea::vertical().id_salt("prod_big_side").auto_shrink([false, false]).show(&mut ru, |ui| big_side(env, ui, &cut));
    }
    ui.advance_cursor_after_rect(area);
}

fn big_preview(env: &mut Env, ui: &mut Ui, area: Rect, rows: &[CutRow], idx: &[usize], pos: usize) {
    let t = env.t;
    let r = &rows[idx[pos]];
    let duration = r.duration.max(1);
    // navegação entre cortes
    ui.horizontal(|ui| {
        let prev = ui.add_enabled(pos > 0, egui::Button::new(tr("prod.big.prev")));
        env.note("prod:big:prev", prev.rect);
        if prev.clicked() {
            env.ed.set_cut(rows[idx[pos - 1]].cut.clone());
            env.ed.prod.big.frame = 0;
        }
        let next = ui.add_enabled(pos + 1 < idx.len(), egui::Button::new(tr("prod.big.next")));
        env.note("prod:big:next", next.rect);
        if next.clicked() {
            env.ed.set_cut(rows[idx[pos + 1]].cut.clone());
            env.ed.prod.big.frame = 0;
        }
        ui.label(egui::RichText::new(format!("{}   ({}/{})", cut_title(r), pos + 1, idx.len())).size(13.0).strong());
        if ui.small_button(tr("prod.open_cut")).clicked() {
            open_in_animation(env, &r.cut, Some(env.ed.prod.big.frame));
        }
    });
    // transporte
    ui.horizontal(|ui| {
        let playing = env.ed.prod.big.playing;
        let b = ui.button(if playing { tr("prod.big.pause") } else { tr("prod.big.play") });
        env.note("prod:big:play", b.rect);
        if b.clicked() {
            env.ed.prod.big.playing = !playing;
        }
        let mut f = env.ed.prod.big.frame;
        let w = (ui.available_width() - 110.0).max(60.0);
        ui.spacing_mut().slider_width = w;
        let s = ui.add(egui::Slider::new(&mut f, 0..=duration - 1).show_value(false));
        env.note("prod:big:slider", s.rect);
        if s.changed() {
            env.ed.prod.big.frame = f;
            env.ed.prod.big.playing = false;
        }
        dim_label(ui, &t, &format!("{} / {}", env.ed.prod.big.frame + 1, duration));
    });
    // prévia
    let avail = ui.available_rect_before_wrap();
    let avail = Rect::from_min_max(avail.min, egui::pos2(area.right(), area.bottom()));
    let ctx = ui.ctx().clone();
    let mut budget = 1u32;
    let frame = env.ed.prod.big.frame;
    let tex = env.ed.prod.big.texture(&ctx, env.core, &r.cut, frame, &mut budget);
    let pr = thumb_rect(env, avail.shrink(2.0));
    let painter = ui.painter().clone();
    painter.rect_filled(avail, 4.0, t.canvas_bg);
    paint_thumb_big(&painter, pr, tex, &t);
    env.note("prod:big:preview", pr);
    ui.allocate_rect(avail, Sense::hover());
}

fn paint_thumb_big(painter: &egui::Painter, rect: Rect, tex: Option<(egui::TextureId, Vec2)>, t: &Tokens) {
    painter.rect_filled(rect, 2.0, Color32::WHITE);
    match tex {
        Some((id, _)) => {
            painter.image(id, rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "…", FontId::proportional(22.0), Color32::from_gray(120));
        }
    }
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
}

/// Lado direito do Grande: as 8 etapas com autor e data da última mudança, aprovar/retake, notas, correções e retakes.
fn big_side(env: &mut Env, ui: &mut Ui, cut: &NodeId) {
    let t = env.t;
    let (rows, doc) = env.ed.prod.cache.get(env.core);
    let Some(row) = rows.iter().find(|r| &r.cut == cut).cloned() else { return };
    let events: Vec<_> = env.ed.prod.cache.hist.idx.events.iter().filter(|e| &e.cut == cut && e.stage.is_some()).cloned().collect();
    ui.spacing_mut().item_spacing.y = 3.0;
    section(ui, &t, "prod.stages");
    for (k, s) in ALL_STAGES.into_iter().enumerate() {
        let st = row.stages.get(&s).copied().unwrap_or_default();
        ui.horizontal(|ui| {
            let (cr, cresp) = ui.allocate_exact_size(egui::vec2(84.0, 18.0), Sense::click());
            env.note(format!("prod:big:stage:{k}"), cr);
            chip(ui.painter(), cr, &t, st, &format!("{}: {}", stage_label(s), short_status(st)), cresp.hovered());
            let cresp = cresp.on_hover_text(format!("{}: {}\n{}", stage_label(s), tr(status_key(st)), tr("prod.chip.tip")));
            if cresp.clicked() {
                set_stage(env, cut, s, next_status(st));
            }
            cresp.context_menu(|ui| {
                for x in STATUSES {
                    if ui.selectable_label(x == st, tr(status_key(x))).clicked() {
                        set_stage(env, cut, s, x);
                        ui.close();
                    }
                }
            });
            if let Some(e) = events.iter().rev().find(|e| e.stage == Some(s)) {
                dim_label(ui, &t, &format!("{} · {}", format_date(day_of(e.t_ms)), e.author));
            }
        });
    }
    // aprovar / pedir retake na etapa atual
    let cur = current_stage(&row);
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        let b = ui.add_enabled(cur.is_some(), egui::Button::new(match cur {
            Some(s) => format!("{} {}", tr("prod.big.approve"), stage_label(s)),
            None => tr("prod.done").to_owned(),
        }));
        env.note("prod:big:approve", b.rect);
        if let (true, Some(s)) = (b.clicked(), cur) {
            set_stage(env, cut, s, StageStatus::Approved);
        }
    });
    ui.horizontal(|ui| {
        let mut reason = env.ed.prod.big.reason.clone();
        let w = (ui.available_width() - 100.0).max(60.0);
        let r = ui.add(egui::TextEdit::singleline(&mut reason).desired_width(w).hint_text(tr("prod.reason_hint")));
        env.note("prod:big:reason", r.rect);
        env.ed.prod.big.reason = reason.clone();
        let b = ui.add_enabled(!reason.trim().is_empty(), egui::Button::new(tr("prod.add_retake"))).on_hover_text(tr("prod.big.retake.tip"));
        env.note("prod:big:retake", b.rect);
        if b.clicked() {
            let frame = env.ed.prod.big.frame;
            let rr = actions::run(env.core, "RETAKE", json!({ "op": "create", "cut": cut, "frame": frame, "reason": reason.trim() }));
            if actions::notify_err(env.ed, rr).is_some() {
                if let Some(s) = cur {
                    set_stage(env, cut, s, StageStatus::Retake);
                }
                env.ed.prod.big.reason.clear();
            }
        }
    });
    // notas do corte
    section(ui, &t, "prod.big.notes");
    let width = (ui.available_width() - 4.0).max(80.0);
    let (r, c) = text_commit_multi(ui, egui::Id::new(("prod_big_notes", &cut.0)), &row.notes, width, tr("prod.big.notes_hint"));
    env.note("prod:big:notes", r.rect);
    if let Some(text) = c {
        let res = cmds::set_cut_props(env.core, cut, json!({ "notes": text }));
        actions::notify_err(env.ed, res);
    }
    // correções e retakes do corte
    section(ui, &t, "prod.corrections");
    let corrections: Vec<_> = doc.corrections.iter().filter(|c| &c.cut == cut).collect();
    if corrections.is_empty() {
        dim_label(ui, &t, tr("prod.no_corrections"));
    }
    for c in corrections {
        ui.label(egui::RichText::new(format!("q{}  {}  ({})", c.frame + 1, c.note, c.author)).size(12.0).color(if c.resolved { t.text_dim } else { t.text }));
    }
    section(ui, &t, "prod.retakes");
    let retakes: Vec<_> = doc.retakes.iter().filter(|r| &r.cut == cut).cloned().collect();
    if retakes.is_empty() {
        dim_label(ui, &t, tr("prod.no_retakes"));
    }
    for (i, rt) in retakes.iter().enumerate() {
        ui.horizontal(|ui| {
            let b = ui.small_button(tr("prod.compare"));
            env.note(format!("prod:big:compare:{i}"), b.rect);
            if b.on_hover_text(tr("prod.compare.tip")).clicked() {
                production::compare(env, ui.ctx(), &rt.id);
            }
            ui.label(egui::RichText::new(format!("q{}  {}  ({})", rt.frame + 1, rt.reason, rt.author)).size(12.0).color(if rt.resolved { t.text_dim } else { t.text }));
        });
    }
    if let Some((rid, _, tex, summary)) = &env.ed.prod.compare {
        if retakes.iter().any(|r| &r.id == rid) {
            dim_label(ui, &t, summary);
            let size = tex.size_vec2();
            let w = ui.available_width().min(size.x);
            ui.add(egui::Image::new((tex.id(), egui::vec2(w, w * size.y / size.x.max(1.0)))));
        }
    }
}

// ------------------------------------------------------------------------------------------------ Quadro (kanban)

fn board(env: &mut Env, ui: &mut Ui, inner: Rect, rows: &[CutRow], idx: &[usize]) {
    let t = env.t;
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("prod.board.stage"));
        let cur = env.ed.prod.board_stage;
        let mut s = cur;
        let r = egui::ComboBox::from_id_salt("prod_board_stage").width(100.0).selected_text(stage_label(cur)).show_ui(ui, |ui| {
            for x in ALL_STAGES {
                ui.selectable_value(&mut s, x, stage_label(x));
            }
        });
        env.note("prod:board:stage", r.response.rect);
        env.ed.prod.board_stage = s;
        dim_label(ui, &t, tr("prod.board.hint"));
    });
    let area = Rect::from_min_max(ui.cursor().min, inner.max);
    let stage = env.ed.prod.board_stage;
    let ncol = STATUSES.len() as f32;
    let col_w = (area.width() - (ncol - 1.0) * 6.0) / ncol;
    let col_rects: Vec<Rect> = (0..STATUSES.len()).map(|k| Rect::from_min_size(egui::pos2(area.left() + k as f32 * (col_w + 6.0), area.top()), egui::vec2(col_w, area.height()))).collect();
    let ctx = ui.ctx().clone();
    let mut budget = THUMB_BUDGET;
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let mut drop: Option<(String, StageStatus)> = None;
    for (k, st) in STATUSES.into_iter().enumerate() {
        let cr = col_rects[k];
        let hot = env.ed.prod.board_drag.is_some() && pointer.is_some_and(|p| cr.contains(p));
        ui.painter().rect(cr, 6.0, t.bg_panel, Stroke::new(if hot { 2.0 } else { 1.0 }, if hot { t.accent } else { t.border }), StrokeKind::Inside);
        let members: Vec<&CutRow> = idx.iter().map(|&i| &rows[i]).filter(|r| r.stages.get(&stage).copied().unwrap_or_default() == st).collect();
        let head = Rect::from_min_size(cr.min, egui::vec2(cr.width(), 24.0));
        ui.painter().rect_filled(head.shrink2(egui::vec2(1.0, 1.0)), 5.0, status_fill(&t, st));
        ui.painter().text(egui::pos2(head.left() + 8.0, head.center().y), egui::Align2::LEFT_CENTER, format!("{}  ({})", tr(status_key(st)), members.len()), FontId::proportional(12.0), t.text);
        let body = Rect::from_min_max(egui::pos2(cr.left() + 4.0, head.bottom() + 4.0), egui::pos2(cr.right() - 4.0, cr.bottom() - 4.0));
        let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(body));
        cu.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt(("prod_board_col", k)).auto_shrink([false, false]).show(&mut cu, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for r in members {
                let tex = env.ed.prod.thumbs.get(&ctx, env.core, &r.cut, &mut budget);
                if let Some(d) = board_card(env, ui, r, tex, body.width() - 8.0, &col_rects, pointer) {
                    drop = Some(d);
                }
            }
        });
    }
    ui.advance_cursor_after_rect(area);
    if let Some((cut, to)) = drop {
        env.ed.prod.board_drag = None;
        let cur = rows.iter().find(|r| r.cut.0 == cut).and_then(|r| r.stages.get(&stage).copied()).unwrap_or_default();
        if cur != to {
            set_stage(env, &aiai_core::Id(cut), stage, to);
        }
    }
}

/// Cartão do quadro: arrastável; devolve (corte, estado) quando é solto sobre outra coluna.
#[allow(clippy::too_many_arguments)]
fn board_card(env: &mut Env, ui: &mut Ui, r: &CutRow, tex: Option<egui::TextureId>, w: f32, cols: &[Rect], pointer: Option<egui::Pos2>) -> Option<(String, StageStatus)> {
    let t = env.t;
    let h = 54.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w.max(60.0), h), Sense::click_and_drag());
    env.note(format!("prod:board:card:{}", r.number), rect);
    let dragging = env.ed.prod.board_drag.as_deref() == Some(r.cut.0.as_str());
    let painter = ui.painter().clone();
    painter.rect(rect, 5.0, t.bg_widget, Stroke::new(1.0, if resp.hovered() { t.text_dim } else { t.border }), StrokeKind::Inside);
    if dragging {
        painter.rect_filled(rect, 5.0, t.bg_panel.gamma_multiply(0.7));
    }
    let tr_ = thumb_rect(env, Rect::from_min_size(rect.min + egui::vec2(4.0, 4.0), egui::vec2(66.0, h - 8.0)));
    paint_thumb(env, &painter, tr_, tex);
    let x0 = tr_.right() + 6.0;
    let clip = painter.with_clip_rect(Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(rect.right() - 4.0, rect.bottom())));
    clip.text(egui::pos2(x0, rect.top() + 13.0), egui::Align2::LEFT_CENTER, cut_title(r), FontId::proportional(12.0), t.text);
    clip.text(egui::pos2(x0, rect.top() + 29.0), egui::Align2::LEFT_CENTER, &r.scene_name, FontId::proportional(11.0), t.text_dim);
    stage_strip(&painter, Rect::from_min_size(egui::pos2(x0, rect.bottom() - 11.0), egui::vec2((rect.right() - 4.0 - x0).max(10.0), 4.0)), &t, r);
    if resp.drag_started() {
        env.ed.prod.board_drag = Some(r.cut.0.clone());
        env.ed.set_cut(r.cut.clone());
    }
    if resp.clicked() {
        env.ed.set_cut(r.cut.clone());
    }
    if resp.double_clicked() {
        open_in_animation(env, &r.cut, None);
    }
    if dragging {
        // fantasma do cartão no ponteiro
        if let Some(p) = pointer {
            let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("prod_board_ghost"));
            let gp = ui.ctx().layer_painter(layer);
            let g = Rect::from_min_size(p - egui::vec2(w * 0.3, 14.0), egui::vec2(w.min(190.0), 30.0));
            gp.rect(g, 5.0, t.bg_hover, Stroke::new(1.5, t.accent), StrokeKind::Inside);
            gp.text(g.center(), egui::Align2::CENTER_CENTER, cut_title(r), FontId::proportional(12.0), t.text);
        }
        if resp.drag_stopped() {
            if let Some(p) = pointer {
                if let Some(k) = cols.iter().position(|c| c.contains(p)) {
                    return Some((r.cut.0.clone(), STATUSES[k]));
                }
            }
            env.ed.prod.board_drag = None;
        }
    }

    None
}
