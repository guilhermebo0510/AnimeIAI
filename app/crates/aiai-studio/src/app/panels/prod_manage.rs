//! Painéis de gestão da Produção: **Gargalos** (análise determinística: fila, vazão e espera por etapa, travados,
//! atrasados e retakes; cada achado, ao clicar, filtra a Lista) e **Análise por IA** (opcional, desligada por padrão: monta
//! o pacote de dados que um agente lê pelo MCP e guarda as notas dele com carimbo de hora; nunca aplica mudanças).

use super::prod_logic::{Filters, ProdView, id_set};
use super::prod_views::strip_color;
use super::production::{self, stage_key};
use crate::app::actions;
use crate::app::doc::AUTHOR;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::theme::Tokens;
use aiai_core::{Id, Stage, StageStatus};
use aiai_tools::production::{ALL_STAGES, CutRow};
use aiai_tools::production_history::{DAY_MS, day_of, format_date};
use aiai_tools::production_stats::{Analysis, Finding, FindingKind, StageFlow, predecessors};
use egui::{Rect, Sense, Stroke, StrokeKind, Ui};
use serde_json::{Value, json};

fn stage_label(s: Stage) -> &'static str {
    tr(stage_key(s))
}

/// Aplica um achado como filtro da Lista (e das outras visões): os cortes envolvidos, com o texto do achado.
fn apply_filter(env: &mut Env, label: String, cuts: &[Id]) {
    let p = &mut env.ed.prod;
    p.filters = Filters::default();
    p.finding = Some((label, id_set(cuts)));
    p.prefs.view = ProdView::List;
}

/// Dias formatados ("11,7"): `None` = sem vazão.
fn days_text(d: Option<f64>) -> String {
    d.map_or_else(|| tr("prod.bn.no_throughput").to_owned(), |d| format!("{d:.1} {}", tr("prod.days_short")))
}

/// Título de um achado na interface (traduzido).
pub(crate) fn finding_title(f: &Finding) -> String {
    let st = f.stage.map_or("", stage_label);
    match f.kind {
        FindingKind::Bottleneck => format!("{}: {st} — {} {} · {} {:.1}/{} · {} {}", tr("prod.bn.bottleneck"), f.queue, tr("prod.bn.in_queue"), tr("prod.bn.throughput"), f.per_day, tr("prod.days_short"), tr("prod.bn.wait"), days_text(f.days)),
        FindingKind::Idle => format!("{}: {st} — {}", tr("prod.bn.idle"), tr("prod.bn.idle_why")),
        FindingKind::Stuck => format!("{}: {st} — {} {} ({} {:.0} {})", tr("prod.bn.stuck"), f.cuts.len(), tr("prod.cuts_short"), tr("prod.bn.stuck_for"), f.days.unwrap_or(0.0), tr("prod.days_short")),
        FindingKind::Overdue => format!("{}: {} {} ({} {:.0} {})", tr("prod.bn.overdue"), f.cuts.len(), tr("prod.cuts_short"), tr("prod.bn.up_to"), f.days.unwrap_or(0.0), tr("prod.days_short")),
        FindingKind::OpenRetake => format!("{}: {} {}", tr("prod.bn.retakes"), f.cuts.len(), tr("prod.cuts_short")),
    }
}

fn finding_color(t: &Tokens, k: FindingKind) -> egui::Color32 {
    match k {
        FindingKind::Bottleneck => t.accent,
        FindingKind::Idle => t.text_dim,
        FindingKind::Stuck => strip_color(t, StageStatus::Review),
        FindingKind::Overdue | FindingKind::OpenRetake => strip_color(t, StageStatus::Retake),
    }
}

// ------------------------------------------------------------------------------------------------ Gargalos

pub(crate) fn bottleneck(env: &mut Env, ui: &mut Ui, body: Rect) {
    production::sync_prefs(env);
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let (rows, _) = env.ed.prod.cache.get(env.core);
    if rows.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("panel.prod_bottleneck.empty")).color(t.text_dim));
        return;
    }
    let a = env.ed.prod.cache.analysis(env.core, env.ed.prod.prefs.stuck_days);
    ui.horizontal_wrapped(|ui| {
        let mut days = env.ed.prod.prefs.stuck_days;
        let r = ui.add(egui::DragValue::new(&mut days).range(0.5..=60.0).speed(0.1).max_decimals(1).prefix(format!("{} ", tr("prod.stuck_days"))).suffix(format!(" {}", tr("prod.days_short"))));
        env.note("prod:bn:stuck_days", r.rect);
        env.ed.prod.prefs.stuck_days = days;
        if !a.stats.has_history {
            dim_label(ui, &t, tr("prod.charts.no_history"));
        }
    });
    egui::ScrollArea::vertical().id_salt("prod_bn").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        // ---- achados
        section(ui, &t, "prod.bn.findings");
        if a.findings.is_empty() {
            dim_label(ui, &t, tr("prod.bn.none"));
        }
        for (i, f) in a.findings.iter().enumerate() {
            finding_card(env, ui, i, f);
        }
        // ---- fluxo por etapa
        section(ui, &t, "prod.bn.flow");
        flow_table(env, ui, &a, &rows);
        // ---- sugestões por regra (não por IA)
        let tips = rule_tips(&a, &rows);
        if !tips.is_empty() {
            section(ui, &t, "prod.bn.tips");
            for s in tips {
                ui.label(egui::RichText::new(format!("•  {s}")).size(12.0));
            }
        }
    });
}

fn finding_card(env: &mut Env, ui: &mut Ui, i: usize, f: &Finding) {
    let t = env.t;
    let title = finding_title(f);
    let on = env.ed.prod.finding.as_ref().is_some_and(|(l, _)| *l == title);
    let galley = ui.painter().layout(title.clone(), egui::FontId::proportional(12.5), t.text, ui.available_width() - 24.0);
    let h = galley.size().y + 12.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    env.note(format!("prod:bn:finding:{i}"), rect);
    let p = ui.painter();
    p.rect(rect, 5.0, if on { t.accent_dim } else if resp.hovered() { t.bg_hover } else { t.bg_widget }, Stroke::new(1.0, t.border), StrokeKind::Inside);
    p.rect_filled(Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 4.0), egui::vec2(4.0, h - 8.0)), 2.0, finding_color(&t, f.kind));
    p.galley(rect.left_top() + egui::vec2(12.0, 6.0), galley, t.text);
    if resp.on_hover_text(tr("prod.bn.click_tip")).clicked() {
        apply_filter(env, title, &f.cuts);
    }
}

fn flow_table(env: &mut Env, ui: &mut Ui, a: &Analysis, rows: &[CutRow]) {
    let t = env.t;
    let bottleneck = a.findings.iter().find(|f| f.kind == FindingKind::Bottleneck).and_then(|f| f.stage);
    let max = a.flows.iter().map(|f| f.queue + f.in_progress + f.review + f.retake).max().unwrap_or(1).max(1) as f32;
    for (i, f) in a.flows.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::click());
        env.note(format!("prod:bn:stage:{i}"), rect);
        let p = ui.painter();
        let is_bn = bottleneck == Some(f.stage);
        p.rect(rect, 4.0, if resp.hovered() { t.bg_hover } else { t.bg_widget }, Stroke::new(if is_bn { 1.5 } else { 1.0 }, if is_bn { t.accent } else { t.border }), StrokeKind::Inside);
        p.text(egui::pos2(rect.left() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, stage_label(f.stage), egui::FontId::proportional(12.0), t.text);
        // barra: fila | andamento | revisão | retake
        let bx = rect.left() + 66.0;
        let bw = (rect.width() - 66.0 - 190.0).max(40.0);
        let segs = [(f.queue, t.text_dim.gamma_multiply(0.6)), (f.in_progress, strip_color(&t, StageStatus::InProgress)), (f.review, strip_color(&t, StageStatus::Review)), (f.retake, strip_color(&t, StageStatus::Retake))];
        let mut x = bx;
        for (n, c) in segs {
            let w = bw * n as f32 / max;
            if w > 0.0 {
                p.rect_filled(Rect::from_min_size(egui::pos2(x, rect.top() + 6.0), egui::vec2(w, 12.0)), 0.0, c);
                x += w;
            }
        }
        let info = format!("{} {}  ·  {:.1}/{}  ·  {}", f.queue, tr("prod.bn.in_queue"), f.throughput_per_day, tr("prod.days_short"), if f.queue == 0 { "—".to_owned() } else { days_text(f.wait_days) });
        p.with_clip_rect(Rect::from_min_max(egui::pos2(bx + bw + 6.0, rect.top()), rect.max)).text(egui::pos2(bx + bw + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, info, egui::FontId::proportional(11.0), t.text_dim);
        let tip = format!(
            "{}\n{}: {}  ·  {}: {}  ·  {}: {}  ·  {}: {}\n{}: {}",
            stage_label(f.stage),
            tr("prod.bn.in_queue"),
            f.queue,
            tr("prod.status.in_progress"),
            f.in_progress,
            tr("prod.status.review"),
            f.review,
            tr("prod.status.retake"),
            f.retake,
            tr("prod.bn.queue_age"),
            f.avg_queue_age_days.map_or("—".to_owned(), |d| format!("{d:.1} {}", tr("prod.days_short")))
        );
        if resp.on_hover_text(tip).clicked() && f.queue > 0 {
            let cuts: Vec<Id> = queue_of(rows, f);
            let label = format!("{} {}", tr("prod.bn.queue_of"), stage_label(f.stage));
            apply_filter(env, label, &cuts);
        }
    }
}

/// Cortes na fila da etapa (anteriores aprovadas, esta não iniciada).
fn queue_of(rows: &[CutRow], f: &StageFlow) -> Vec<Id> {
    let preds = predecessors(f.stage);
    rows.iter()
        .filter(|r| r.stages.get(&f.stage).copied().unwrap_or_default() == StageStatus::NotStarted && preds.iter().all(|p| r.stages.get(p).copied().unwrap_or_default() == StageStatus::Approved))
        .map(|r| r.cut.clone())
        .collect()
}

/// Sugestões por regra (nunca por IA): o que o gargalo, o retake e os atrasos sugerem fazer.
pub(crate) fn rule_tips(a: &Analysis, rows: &[CutRow]) -> Vec<String> {
    let mut tips = Vec::new();
    if let Some(bn) = a.findings.iter().find(|f| f.kind == FindingKind::Bottleneck) {
        if let Some(s) = bn.stage {
            let next: Vec<&str> = ALL_STAGES.iter().filter(|x| predecessors(**x).contains(&s)).map(|x| stage_label(*x)).collect();
            if !next.is_empty() {
                tips.push(format!("{} {} → {}", tr("prod.tip.prioritize"), stage_label(s), next.join(", ")));
            }
            // cortes longos na fila: mais que uma vez e meia a média do projeto
            let avg = rows.iter().map(|r| r.duration as f32).sum::<f32>() / rows.len().max(1) as f32;
            let long = rows.iter().filter(|r| bn.cuts.contains(&r.cut) && r.duration as f32 > avg * 1.5).count();
            if long > 0 {
                tips.push(format!("{} {} ({long})", tr("prod.tip.split_long"), stage_label(s)));
            }
        }
    }
    if let Some(r) = a.stats.retake_rates.iter().filter(|r| r.touched > 0 && r.rate >= 0.2).max_by(|x, y| x.rate.partial_cmp(&y.rate).unwrap_or(std::cmp::Ordering::Equal)) {
        tips.push(format!("{} {} ({:.0}%)", tr("prod.tip.lint_before"), stage_label(r.stage), r.rate * 100.0));
    }
    if let Some(f) = a.findings.iter().find(|f| f.kind == FindingKind::Overdue) {
        tips.push(format!("{} ({})", tr("prod.tip.overdue"), f.cuts.len()));
    }
    tips
}

// ------------------------------------------------------------------------------------------------ Análise por IA

/// "2026-10-20 14:03 UTC" de um instante em ms.
pub(crate) fn stamp(ms: u64) -> String {
    let secs = (ms % DAY_MS) / 1000;
    format!("{} {:02}:{:02} UTC", format_date(day_of(ms)), secs / 3600, secs % 3600 / 60)
}

/// Pedido pronto para colar no agente (o roteiro completo vai dentro do pacote).
const AGENT_REQUEST: &str = "Analise a produção do AnimeIAI: chame aiai_production com {\"op\":\"analysis_package\"}, leia o pacote e devolva o texto com {\"op\":\"note_add\",\"ai\":true,\"text\":\"...\"}. Não aplique nenhuma mudança no projeto.";

pub(crate) fn ai(env: &mut Env, ui: &mut Ui, body: Rect) {
    production::sync_prefs(env);
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let mut on = env.ed.prod.prefs.ai_enabled;
    let r = ui.checkbox(&mut on, tr("prod.ai.enable"));
    env.note("prod:ai:enable", r.rect);
    env.ed.prod.prefs.ai_enabled = on;
    if !on {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(tr("prod.ai.off")).size(12.0).color(t.text_dim));
        return;
    }
    let (rows, doc) = env.ed.prod.cache.get(env.core);
    egui::ScrollArea::vertical().id_salt("prod_ai").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(egui::RichText::new(tr("prod.ai.explain")).size(12.0).color(t.text_dim));
        ui.horizontal_wrapped(|ui| {
            let b = ui.add_enabled(!rows.is_empty(), egui::Button::new(tr("prod.ai.build")));
            env.note("prod:ai:build", b.rect);
            if b.on_hover_text(tr("prod.ai.build.tip")).clicked() {
                build_package(env);
            }
            let b = ui.add_enabled(!rows.is_empty(), egui::Button::new(tr("prod.ai.copy_package")));
            env.note("prod:ai:copy_package", b.rect);
            if b.clicked() {
                copy_package(env, ui.ctx());
            }
            let b = ui.button(tr("prod.ai.copy_request"));
            env.note("prod:ai:copy_request", b.rect);
            if b.on_hover_text(tr("prod.ai.copy_request.tip")).clicked() {
                ui.ctx().copy_text(AGENT_REQUEST.to_owned());
                env.ed.notify(tr("prod.ai.copied"));
            }
        });
        if let Some((path, at, bytes)) = &env.ed.prod.ai_package {
            dim_label(ui, &t, &format!("{}: {path}  ({bytes} B, {})", tr("prod.ai.last_package"), stamp(*at)));
        }
        section(ui, &t, "prod.ai.ask");
        ui.label(egui::RichText::new(AGENT_REQUEST).size(11.5).color(t.text).monospace());
        // ---- notas
        section(ui, &t, "prod.ai.notes");
        if doc.notes.is_empty() {
            dim_label(ui, &t, tr("prod.ai.no_notes"));
        }
        let mut delete: Option<Id> = None;
        for (i, n) in doc.notes.iter().enumerate().rev() {
            egui::Frame::new().fill(t.bg_widget).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    if n.ai {
                        ui.label(egui::RichText::new(tr("prod.ai.badge")).size(10.5).strong().color(t.accent));
                    }
                    dim_label(ui, &t, &format!("{} · {}", stamp(n.t_ms), n.author));
                    if let Some(c) = &n.cut {
                        if let Some(r) = rows.iter().find(|r| &r.cut == c) {
                            dim_label(ui, &t, &production::cut_title(r));
                        }
                    }
                    let b = ui.small_button("x").on_hover_text(tr("prod.ai.note_delete"));
                    env.note(format!("prod:ai:note_delete:{i}"), b.rect);
                    if b.clicked() {
                        delete = Some(n.id.clone());
                    }
                });
                ui.label(egui::RichText::new(&n.text).size(12.0));
            });
        }
        if let Some(id) = delete {
            let r = actions::run(env.core, "PRODUCTION", json!({ "op": "note_delete", "note": id }));
            actions::notify_err(env.ed, r);
        }
        // ---- nova nota / colar a resposta da análise
        let mut text = env.ed.prod.ai_note.clone();
        let r = ui.add(egui::TextEdit::multiline(&mut text).desired_width(f32::INFINITY).desired_rows(3).hint_text(tr("prod.ai.note_hint")));
        env.note("prod:ai:note_text", r.rect);
        env.ed.prod.ai_note = text.clone();
        ui.horizontal(|ui| {
            let b = ui.add_enabled(!text.trim().is_empty(), egui::Button::new(tr("prod.ai.add_note")));
            env.note("prod:ai:add_note", b.rect);
            if b.clicked() {
                add_note(env, &text, false);
            }
            let b = ui.add_enabled(!text.trim().is_empty(), egui::Button::new(tr("prod.ai.paste_analysis"))).on_hover_text(tr("prod.ai.paste_analysis.tip"));
            env.note("prod:ai:paste_analysis", b.rect);
            if b.clicked() {
                add_note(env, &text, true);
            }
        });
        dim_label(ui, &t, tr("prod.ai.never_applies"));
    });
}

fn add_note(env: &mut Env, text: &str, ai: bool) {
    let author = if ai { "agent:analysis" } else { AUTHOR };
    let params = json!({ "op": "note_add", "text": text.trim(), "ai": ai, "author": author });
    let r = actions::run(env.core, "PRODUCTION", params);
    if actions::notify_err(env.ed, r).is_some() {
        env.ed.prod.ai_note.clear();
    }
}

/// Grava o pacote em `<pasta do projeto>/saida/producao-analise.json` (ação `PRODUCTION analysis_package`).
fn build_package(env: &mut Env) {
    let Some(path) = env.core.path.clone() else {
        env.ed.notify(tr("board.save_first"));
        return;
    };
    let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or(path);
    let out = dir.join("saida").join("producao-analise.json");
    let days = env.ed.prod.prefs.stuck_days;
    let r = actions::run(env.core, "PRODUCTION", json!({ "op": "analysis_package", "out": out.display().to_string(), "stuck_days": days }));
    if actions::notify_err(env.ed, r).is_some() {
        let bytes = std::fs::metadata(&out).map(|m| m.len() as usize).unwrap_or(0);
        env.ed.prod.ai_package = Some((out.display().to_string(), env.core.clock.now_ms(), bytes));
        env.ed.notify(format!("{}: {}", tr("prod.ai.package_done"), out.display()));
    }
}

fn copy_package(env: &mut Env, ctx: &egui::Context) {
    let days = env.ed.prod.prefs.stuck_days;
    let r = actions::run(env.core, "PRODUCTION", json!({ "op": "analysis_package", "stuck_days": days }));
    if let Some(v) = actions::notify_err(env.ed, r) {
        let pkg = v.get("package").cloned().unwrap_or(Value::Null);
        ctx.copy_text(serde_json::to_string_pretty(&pkg).unwrap_or_default());
        env.ed.notify(tr("prod.ai.copied"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_tools::production_stats::{StatsConfig, analyze};

    fn row(n: u32, dur: u32, stages: &[(Stage, StageStatus)]) -> CutRow {
        let mut map: std::collections::BTreeMap<Stage, StageStatus> = ALL_STAGES.iter().map(|s| (*s, StageStatus::NotStarted)).collect();
        for (s, st) in stages {
            map.insert(*s, *st);
        }
        CutRow {
            cut: Id(format!("c{n}")),
            number: n,
            name: format!("C{n:03}"),
            episode: None,
            scene: None,
            scene_name: "S".into(),
            duration: dur,
            stages: map,
            open_corrections: 0,
            retakes: 0,
            open_retakes: 0,
            due: None,
            assignee: None,
            weight: None,
            notes: String::new(),
            thumbnail: None,
        }
    }

    /// Uma chave de tradução que vazou para a tela (palavra que começa com "prod" e tem ponto).
    fn leaks_key(s: &str) -> bool {
        s.split_whitespace().any(|w| w.starts_with("prod") && w.contains('.'))
    }

    #[test]
    fn timestamps_are_utc_with_minutes() {
        assert_eq!(stamp(0), "1970-01-01 00:00 UTC");
        assert_eq!(stamp(DAY_MS + 3 * 3_600_000 + 7 * 60_000 + 59_000), "1970-01-02 03:07 UTC");
    }

    #[test]
    fn titles_and_tips_are_built_from_the_findings_in_both_languages() {
        let rows = vec![
            row(1, 100, &[(Stage::Layout, StageStatus::Approved), (Stage::Genga, StageStatus::Approved)]),
            row(2, 10, &[(Stage::Layout, StageStatus::Approved), (Stage::Genga, StageStatus::Approved)]),
            row(3, 10, &[(Stage::Layout, StageStatus::Approved), (Stage::Genga, StageStatus::Retake)]),
        ];
        let a = analyze(&rows, &[], &StatsConfig::new(10 * DAY_MS));
        {
            for f in &a.findings {
                let title = finding_title(f);
                assert!(!leaks_key(&title), "chave sem tradução em «{title}»");
            }
            for tip in rule_tips(&a, &rows) {
                assert!(!leaks_key(&tip), "chave sem tradução em «{tip}»");
            }
        }
        // Genga em retake em 1 de 3 cortes (33%) pede lint antes dela; há retake aberto
        let tips = rule_tips(&a, &rows);
        assert!(tips.iter().any(|t| t.contains("Genga")), "{tips:?}");
        assert!(a.findings.iter().any(|f| f.kind == FindingKind::OpenRetake));
    }
}
