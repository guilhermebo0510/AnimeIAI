//! Painel Gráficos da Produção: barras empilhadas por etapa (cortes ↔ quadros), rosca geral, matriz cena × etapa (mapa de
//! calor), burndown com linha ideal, fluxo cumulativo, vazão semanal com previsão, tempo médio por etapa e taxa de
//! retake. Tudo desenhado com o `Painter` do egui (sem biblioteca de gráficos) a partir de `production_stats::analyze`,
//! que calcula os números do histórico de status (`production_history`). Exportar CSV grava os mesmos números.

use super::prod_views::strip_color;
use super::production::{self, stage_key, status_key};
use crate::app::actions;
use crate::app::ui_kit::{Env, body_ui, dim_label};
use crate::i18n::tr;
use crate::theme::Tokens;
use aiai_core::StageStatus;
use aiai_tools::production::ALL_STAGES;
use aiai_tools::production_history::format_date;
use aiai_tools::production_stats::{Analysis, STATUS_ORDER, status_idx};
use egui::{Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui};
use serde_json::json;

/// Aviso de amostra pequena (auditoria V2): com poucas aprovações na janela, vazão, espera, previsão e gargalo são
/// indicativos. Fica acima dos números nos Gráficos e nos Gargalos.
pub(crate) fn sample_warning(env: &mut Env, ui: &mut Ui, a: &Analysis) {
    if !a.stats.has_history || !a.stats.low_sample {
        return;
    }
    let t = env.t;
    let text = tr("prod.low_sample").replace("{n}", &a.stats.sample_approvals.to_string()).replace("{d}", &a.stats.window_days.to_string()).replace("{min}", &aiai_tools::production_stats::MIN_SAMPLE.to_string());
    let r = ui.label(egui::RichText::new(text).size(11.5).color(t.st_review)).on_hover_text(tr("prod.low_sample.tip"));
    env.note("prod:low_sample", r.rect);
}

/// Do mais "pronto" ao menos: a ordem em que as faixas empilham (aprovado embaixo/à esquerda).
const STACK: [StageStatus; 5] = [StageStatus::Approved, StageStatus::Review, StageStatus::InProgress, StageStatus::Retake, StageStatus::NotStarted];

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    production::sync_prefs(env);
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let (rows, _) = env.ed.prod.cache.get(env.core);
    if rows.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("panel.prod_charts.empty")).color(t.text_dim));
        return;
    }
    let stuck = env.ed.prod.prefs.stuck_days;
    let a = env.ed.prod.cache.analysis(env.core, stuck);
    // ---- barra
    ui.horizontal_wrapped(|ui| {
        let frames = env.ed.prod.charts_frames;
        let r = ui.selectable_label(!frames, tr("prod.charts.cuts"));
        env.note("prod:charts:cuts", r.rect);
        if r.clicked() {
            env.ed.prod.charts_frames = false;
        }
        let r = ui.selectable_label(frames, tr("prod.charts.frames"));
        env.note("prod:charts:frames", r.rect);
        if r.clicked() {
            env.ed.prod.charts_frames = true;
        }
        ui.separator();
        let mut days = env.ed.prod.prefs.stuck_days;
        let r = ui.add(egui::DragValue::new(&mut days).range(0.5..=60.0).speed(0.1).max_decimals(1).prefix(format!("{} ", tr("prod.stuck_days"))).suffix(format!(" {}", tr("prod.days_short"))));
        env.note("prod:charts:stuck_days", r.rect);
        env.ed.prod.prefs.stuck_days = days;
        ui.separator();
        let b = ui.button(tr("prod.charts.csv")).on_hover_text(tr("prod.charts.csv.tip"));
        env.note("prod:charts:csv", b.rect);
        if b.clicked() {
            export_csv(env);
        }
        if !a.stats.has_history {
            dim_label(ui, &t, tr("prod.charts.no_history"));
        }
    });
    sample_warning(env, ui, &a);
    ui.add_space(2.0);
    let frames = env.ed.prod.charts_frames;
    egui::ScrollArea::vertical().id_salt("prod_charts").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        let w = ui.available_width();
        let two = w >= 640.0;
        let cw = if two { (w - 8.0) / 2.0 } else { w };
        let h = 190.0;
        let scenes_h = 56.0 + 18.0 * (a.stats.scenes.len() as f32 + 1.0);
        let card = |ui: &mut Ui, title: &str, size: egui::Vec2, draw: &dyn Fn(&mut Ui, Rect)| {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter().rect(rect, 6.0, t.bg_widget, Stroke::new(1.0, t.border), StrokeKind::Inside);
            ui.painter().text(rect.left_top() + egui::vec2(10.0, 8.0), egui::Align2::LEFT_TOP, title, FontId::proportional(12.5), t.text);
            draw(ui, Rect::from_min_max(rect.min + egui::vec2(10.0, 28.0), rect.max - egui::vec2(10.0, 8.0)));
        };
        let size = egui::vec2(cw, h);
        if two {
            ui.horizontal(|ui| {
                card(ui, tr("prod.chart.stages"), size, &|ui, r| chart_stages(ui, &t, r, &a, frames));
                card(ui, tr("prod.chart.donut"), size, &|ui, r| chart_donut(ui, &t, r, &a, frames));
            });
            ui.horizontal(|ui| {
                card(ui, tr("prod.chart.burndown"), size, &|ui, r| chart_burndown(ui, &t, r, &a));
                card(ui, tr("prod.chart.flow"), size, &|ui, r| chart_flow(ui, &t, r, &a));
            });
            card(ui, tr("prod.chart.matrix"), egui::vec2(w, scenes_h), &|ui, r| chart_matrix(ui, &t, r, &a));
            ui.horizontal(|ui| {
                card(ui, tr("prod.chart.throughput"), size, &|ui, r| chart_throughput(ui, &t, r, &a));
                card(ui, tr("prod.chart.timing"), size, &|ui, r| chart_timing(ui, &t, r, &a));
            });
            card(ui, tr("prod.chart.retake"), size, &|ui, r| chart_retake(ui, &t, r, &a));
        } else {
            card(ui, tr("prod.chart.stages"), size, &|ui, r| chart_stages(ui, &t, r, &a, frames));
            card(ui, tr("prod.chart.donut"), size, &|ui, r| chart_donut(ui, &t, r, &a, frames));
            card(ui, tr("prod.chart.matrix"), egui::vec2(w, scenes_h), &|ui, r| chart_matrix(ui, &t, r, &a));
            card(ui, tr("prod.chart.burndown"), size, &|ui, r| chart_burndown(ui, &t, r, &a));
            card(ui, tr("prod.chart.flow"), size, &|ui, r| chart_flow(ui, &t, r, &a));
            card(ui, tr("prod.chart.throughput"), size, &|ui, r| chart_throughput(ui, &t, r, &a));
            card(ui, tr("prod.chart.timing"), size, &|ui, r| chart_timing(ui, &t, r, &a));
            card(ui, tr("prod.chart.retake"), size, &|ui, r| chart_retake(ui, &t, r, &a));
        }
    });
}

/// Grava o CSV dos gráficos em `<pasta do projeto>/saida/producao-graficos.csv` (ação `PRODUCTION stats_csv`).
fn export_csv(env: &mut Env) {
    let Some(path) = env.core.path.clone() else {
        env.ed.notify(tr("board.save_first"));
        return;
    };
    let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or(path);
    let out = dir.join("saida").join("producao-graficos.csv");
    let r = actions::run(env.core, "PRODUCTION", json!({ "op": "stats_csv", "out": out.display().to_string() }));
    if actions::notify_err(env.ed, r).is_some() {
        env.ed.notify(format!("{}: {}", tr("prod.charts.csv_done"), out.display()));
    }
}

// ------------------------------------------------------------------------------------------------ utilidades

/// Passo "redondo" (1, 2, 5 × 10^k) para ~`n` marcas até `max`.
pub(crate) fn nice_step(max: f64, n: usize) -> f64 {
    let raw = (max / n.max(1) as f64).max(1e-9);
    let mag = 10f64.powf(raw.log10().floor());
    let f = raw / mag;
    let m = if f <= 1.0 { 1.0 } else if f <= 2.0 { 2.0 } else if f <= 5.0 { 5.0 } else { 10.0 };
    m * mag
}

fn txt(p: &egui::Painter, pos: Pos2, a: egui::Align2, s: impl ToString, size: f32, c: Color32) {
    p.text(pos, a, s, FontId::proportional(size), c);
}

fn hover_rows(ui: &Ui, rect: Rect, id: &str) -> egui::Response {
    ui.interact(rect, egui::Id::new(("prod_chart", id)), Sense::hover())
}

fn status_name(s: StageStatus) -> &'static str {
    tr(status_key(s))
}

/// Eixos: grade horizontal com valores à esquerda; devolve a área do gráfico (sem a coluna de rótulos).
fn y_axis(p: &egui::Painter, t: &Tokens, rect: Rect, max: f64) -> (Rect, f64) {
    let step = nice_step(max.max(1.0), 4);
    let top = (max / step).ceil().max(1.0) * step;
    let plot = Rect::from_min_max(rect.min + egui::vec2(30.0, 4.0), rect.max - egui::vec2(2.0, 16.0));
    let mut v = 0.0;
    while v <= top + 1e-6 {
        let y = plot.bottom() - plot.height() * (v / top) as f32;
        p.hline(plot.x_range(), y, Stroke::new(1.0, t.border.gamma_multiply(0.6)));
        txt(p, egui::pos2(plot.left() - 4.0, y), egui::Align2::RIGHT_CENTER, format!("{v:.0}"), 10.0, t.text_dim);
        v += step;
    }
    (plot, top)
}

/// Rótulos de data no eixo x: até `n` marcas entre o primeiro e o último dia.
fn x_dates(p: &egui::Painter, t: &Tokens, plot: Rect, first: i64, last: i64, n: usize) {
    let span = (last - first).max(1);
    for k in 0..=n {
        let d = first + span * k as i64 / n as i64;
        let x = plot.left() + plot.width() * (d - first) as f32 / span as f32;
        let s = format_date(d);
        txt(p, egui::pos2(x, plot.bottom() + 3.0), if k == 0 { egui::Align2::LEFT_TOP } else if k == n { egui::Align2::RIGHT_TOP } else { egui::Align2::CENTER_TOP }, &s[5..], 10.0, t.text_dim);
    }
}

fn legend(p: &egui::Painter, t: &Tokens, at: Pos2, items: &[(Color32, &str)]) {
    let mut x = at.x;
    for (c, name) in items {
        p.rect_filled(Rect::from_min_size(egui::pos2(x, at.y + 2.0), egui::vec2(9.0, 9.0)), 2.0, *c);
        let g = p.layout_no_wrap((*name).to_owned(), FontId::proportional(10.5), t.text_dim);
        let w = g.size().x;
        p.galley(egui::pos2(x + 12.0, at.y), g, t.text_dim);
        x += 12.0 + w + 7.0;
    }
}

fn status_legend(p: &egui::Painter, t: &Tokens, at: Pos2) {
    let items: Vec<(Color32, &str)> = STACK.iter().map(|s| (strip_color(t, *s), status_name(*s))).collect();
    legend(p, t, at, &items);
}

// ------------------------------------------------------------------------------------------------ 1. barras por etapa

fn chart_stages(ui: &mut Ui, t: &Tokens, rect: Rect, a: &Analysis, frames: bool) {
    let p = ui.painter_at(rect);
    let resp = hover_rows(ui, rect, "stages");
    let label_w = 54.0;
    let row_h = ((rect.height() - 16.0) / ALL_STAGES.len() as f32).min(20.0);
    let bar_x0 = rect.left() + label_w;
    let bar_w = (rect.width() - label_w - 44.0).max(40.0);
    let mut hovered = None;
    for (k, s) in a.stats.stages.iter().enumerate() {
        let y = rect.top() + k as f32 * row_h;
        let vals: Vec<f64> = STACK.iter().map(|st| if frames { s.frames[status_idx(*st)] as f64 } else { s.counts[status_idx(*st)] as f64 }).collect();
        let total: f64 = vals.iter().sum::<f64>().max(1.0);
        txt(&p, egui::pos2(rect.left(), y + row_h / 2.0), egui::Align2::LEFT_CENTER, tr(stage_key(s.stage)), 11.5, t.text);
        let mut x = bar_x0;
        for (i, st) in STACK.iter().enumerate() {
            let w = bar_w * (vals[i] / total) as f32;
            if w > 0.0 {
                let r = Rect::from_min_size(egui::pos2(x, y + 2.0), egui::vec2(w, row_h - 4.0));
                p.rect_filled(r, 0.0, strip_color(t, *st));
                if w > 20.0 && *st != StageStatus::NotStarted {
                    txt(&p, r.center(), egui::Align2::CENTER_CENTER, format!("{:.0}", vals[i]), 10.0, Color32::BLACK.gamma_multiply(0.75));
                }
                x += w;
            }
        }
        p.rect_stroke(Rect::from_min_size(egui::pos2(bar_x0, y + 2.0), egui::vec2(bar_w, row_h - 4.0)), 0.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
        let approved = vals[0] / total * 100.0;
        txt(&p, egui::pos2(rect.right(), y + row_h / 2.0), egui::Align2::RIGHT_CENTER, format!("{approved:.0}%"), 11.0, t.text_dim);
        if resp.hover_pos().is_some_and(|hp| hp.y >= y && hp.y < y + row_h) {
            hovered = Some(k);
        }
    }
    status_legend(&p, t, egui::pos2(rect.left(), rect.bottom() - 13.0));
    if let Some(k) = hovered {
        let s = &a.stats.stages[k];
        resp.on_hover_ui_at_pointer(|ui| {
            ui.label(egui::RichText::new(tr(stage_key(s.stage))).strong());
            for st in STATUS_ORDER {
                let v = if frames { s.frames[status_idx(st)] as usize } else { s.counts[status_idx(st)] };
                ui.label(format!("{}: {v}", status_name(st)));
            }
        });
    }
}

// ------------------------------------------------------------------------------------------------ 2. rosca

/// Anel (setor entre dois raios) como malha de triângulos: o `convex_polygon` do egui não serve para setores côncavos.
fn ring_segment(p: &egui::Painter, center: Pos2, r_in: f32, r_out: f32, a0: f32, a1: f32, color: Color32) {
    let n = (((a1 - a0).abs() / 0.12).ceil() as usize).max(1);
    let mut mesh = egui::Mesh::default();
    for i in 0..=n {
        let a = a0 + (a1 - a0) * i as f32 / n as f32;
        let (s, c) = a.sin_cos();
        mesh.colored_vertex(center + egui::vec2(c, s) * r_out, color);
        mesh.colored_vertex(center + egui::vec2(c, s) * r_in, color);
    }
    for i in 0..n as u32 {
        let b = i * 2;
        mesh.add_triangle(b, b + 1, b + 2);
        mesh.add_triangle(b + 1, b + 3, b + 2);
    }
    p.add(Shape::mesh(mesh));
}

fn chart_donut(ui: &mut Ui, t: &Tokens, rect: Rect, a: &Analysis, frames: bool) {
    let p = ui.painter_at(rect);
    let vals: Vec<f64> = STACK
        .iter()
        .map(|st| if frames { a.stats.stages.iter().map(|s| s.frames[status_idx(*st)] as f64).sum() } else { a.stats.overall[status_idx(*st)] as f64 })
        .collect();
    let total: f64 = vals.iter().sum::<f64>().max(1.0);
    let side = rect.height().min(rect.width() * 0.5) - 6.0;
    let center = egui::pos2(rect.left() + side / 2.0 + 4.0, rect.top() + (rect.height() - 14.0) / 2.0);
    let (r_out, r_in) = (side / 2.0 - 4.0, side / 2.0 - 4.0 - 20.0);
    let mut ang = -std::f32::consts::FRAC_PI_2;
    for (i, st) in STACK.iter().enumerate() {
        let sweep = (vals[i] / total) as f32 * std::f32::consts::TAU;
        if sweep > 0.0 {
            ring_segment(&p, center, r_in, r_out, ang, ang + sweep.min(std::f32::consts::TAU - 1e-3), strip_color(t, *st));
            ang += sweep;
        }
    }
    let done = vals[0] / total * 100.0;
    txt(&p, center, egui::Align2::CENTER_CENTER, format!("{done:.0}%"), 18.0, t.text);
    // legenda com os números
    let x = center.x + side / 2.0 + 18.0;
    for (i, st) in STACK.iter().enumerate() {
        let y = rect.top() + 14.0 + i as f32 * 22.0;
        p.rect_filled(Rect::from_min_size(egui::pos2(x, y), egui::vec2(10.0, 10.0)), 2.0, strip_color(t, *st));
        txt(&p, egui::pos2(x + 16.0, y + 5.0), egui::Align2::LEFT_CENTER, format!("{}  {:.0}", status_name(*st), vals[i]), 11.5, t.text);
    }
    txt(&p, egui::pos2(x, rect.bottom() - 12.0), egui::Align2::LEFT_CENTER, format!("{}/{}  {}", a.stats.done_cuts, a.stats.total_cuts, tr("prod.chart.cuts_done")), 11.0, t.text_dim);
}

// ------------------------------------------------------------------------------------------------ 3. matriz cena × etapa

/// Cor da célula: do fundo (nada aprovado) ao verde (tudo aprovado); retake pinta de vermelho.
fn heat(t: &Tokens, counts: &[usize; 5]) -> Color32 {
    let n: usize = counts.iter().sum();
    if n == 0 {
        return t.bg_panel;
    }
    if counts[status_idx(StageStatus::Retake)] > 0 {
        return strip_color(t, StageStatus::Retake).gamma_multiply(0.85);
    }
    let ok = counts[status_idx(StageStatus::Approved)] as f32 / n as f32;
    let wip = (counts[status_idx(StageStatus::InProgress)] + counts[status_idx(StageStatus::Review)]) as f32 / n as f32;
    let base = t.bg_panel;
    let green = strip_color(t, StageStatus::Approved);
    let amber = strip_color(t, StageStatus::Review);
    let mix = |a: Color32, b: Color32, k: f32| {
        let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
        Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
    };
    mix(mix(base, amber, wip * 0.8), green, ok)
}

fn chart_matrix(ui: &mut Ui, t: &Tokens, rect: Rect, a: &Analysis) {
    let p = ui.painter_at(rect);
    let resp = hover_rows(ui, rect, "matrix");
    let label_w = 110.0f32.min(rect.width() * 0.25);
    let cell_w = ((rect.width() - label_w) / ALL_STAGES.len() as f32).max(24.0);
    let row_h = 18.0;
    for (k, s) in ALL_STAGES.iter().enumerate() {
        txt(&p, egui::pos2(rect.left() + label_w + k as f32 * cell_w + cell_w / 2.0, rect.top() + row_h / 2.0), egui::Align2::CENTER_CENTER, tr(stage_key(*s)), 10.5, t.text_dim);
    }
    let mut hovered: Option<(usize, usize)> = None;
    for (i, sc) in a.stats.scenes.iter().enumerate() {
        let y = rect.top() + (i as f32 + 1.0) * row_h;
        p.with_clip_rect(Rect::from_min_size(egui::pos2(rect.left(), y), egui::vec2(label_w - 4.0, row_h))).text(egui::pos2(rect.left(), y + row_h / 2.0), egui::Align2::LEFT_CENTER, format!("{} ({})", sc.name, sc.cuts), FontId::proportional(11.5), t.text);
        for (k, c) in sc.cells.iter().enumerate() {
            let r = Rect::from_min_size(egui::pos2(rect.left() + label_w + k as f32 * cell_w, y), egui::vec2(cell_w - 2.0, row_h - 2.0));
            p.rect_filled(r, 3.0, heat(t, c));
            let n: usize = c.iter().sum();
            let ok = c[status_idx(StageStatus::Approved)];
            txt(&p, r.center(), egui::Align2::CENTER_CENTER, format!("{ok}/{n}"), 10.0, t.text);
            if resp.hover_pos().is_some_and(|hp| r.contains(hp)) {
                hovered = Some((i, k));
                p.rect_stroke(r, 3.0, Stroke::new(1.5, t.accent), StrokeKind::Inside);
            }
        }
    }
    if let Some((i, k)) = hovered {
        let sc = &a.stats.scenes[i];
        resp.on_hover_ui_at_pointer(|ui| {
            ui.label(egui::RichText::new(format!("{} · {}", sc.name, tr(stage_key(ALL_STAGES[k])))).strong());
            for st in STATUS_ORDER {
                ui.label(format!("{}: {}", status_name(st), sc.cells[k][status_idx(st)]));
            }
        });
    }
}

// ------------------------------------------------------------------------------------------------ 4. burndown

fn chart_burndown(ui: &mut Ui, t: &Tokens, rect: Rect, a: &Analysis) {
    let p = ui.painter_at(rect);
    let resp = hover_rows(ui, rect, "burndown");
    let days = &a.stats.days;
    let (Some(first), Some(last)) = (days.first(), days.last()) else { return };
    let max = days.iter().map(|d| d.remaining_cuts as f64).chain(a.stats.ideal.iter().map(|(_, v)| *v)).fold(1.0, f64::max);
    let (plot, top) = y_axis(&p, t, Rect::from_min_max(rect.min, rect.max - egui::vec2(0.0, 14.0)), max);
    let x_last = a.stats.ideal.last().map_or(last.day, |(d, _)| (*d).max(last.day));
    let span = (x_last - first.day).max(1) as f32;
    let px = |d: i64| plot.left() + plot.width() * (d - first.day) as f32 / span;
    let py = |v: f64| plot.bottom() - plot.height() * (v / top) as f32;
    if !a.stats.ideal.is_empty() {
        let pts: Vec<Pos2> = a.stats.ideal.iter().map(|(d, v)| egui::pos2(px(*d), py(*v))).collect();
        p.extend(Shape::dashed_line(&pts, Stroke::new(1.5, t.text_dim), 5.0, 4.0));
    }
    let pts: Vec<Pos2> = days.iter().map(|d| egui::pos2(px(d.day), py(d.remaining_cuts as f64))).collect();
    p.add(Shape::line(pts, Stroke::new(2.0, t.accent)));
    // hoje
    p.vline(px(last.day), plot.y_range(), Stroke::new(1.0, t.text_dim.gamma_multiply(0.7)));
    x_dates(&p, t, plot, first.day, x_last, 4);
    legend(&p, t, egui::pos2(plot.left() + 6.0, plot.top() + 2.0), &[(t.accent, tr("prod.chart.remaining")), (t.text_dim, tr("prod.chart.ideal"))]);
    if let Some(hp) = resp.hover_pos().filter(|hp| plot.contains(*hp)) {
        let d = first.day + ((hp.x - plot.left()) / plot.width() * span).round() as i64;
        if let Some(pt) = days.iter().find(|x| x.day == d) {
            resp.on_hover_text(format!("{}: {} {}", format_date(d), pt.remaining_cuts, tr("prod.chart.cuts_remaining")));
        }
    }
}

// ------------------------------------------------------------------------------------------------ 5. fluxo cumulativo

fn chart_flow(ui: &mut Ui, t: &Tokens, rect: Rect, a: &Analysis) {
    let p = ui.painter_at(rect);
    let resp = hover_rows(ui, rect, "flow");
    let days = &a.stats.days;
    let (Some(first), Some(last)) = (days.first(), days.last()) else { return };
    let total = days.iter().map(|d| d.flow.iter().sum::<usize>()).max().unwrap_or(1) as f64;
    let (plot, top) = y_axis(&p, t, Rect::from_min_max(rect.min, rect.max - egui::vec2(0.0, 14.0)), total);
    let span = (last.day - first.day).max(1) as f32;
    let px = |d: i64| plot.left() + plot.width() * (d - first.day) as f32 / span;
    let py = |v: f64| plot.bottom() - plot.height() * (v / top) as f32;
    // faixas empilhadas (Aprovado embaixo): cada par de dias vira um quadrilátero por faixa
    let mut mesh = egui::Mesh::default();
    for w in days.windows(2) {
        let (d0, d1) = (&w[0], &w[1]);
        let (mut lo0, mut lo1) = (0.0f64, 0.0f64);
        for st in STACK {
            let (v0, v1) = (d0.flow[status_idx(st)] as f64, d1.flow[status_idx(st)] as f64);
            let c = strip_color(t, st);
            let i = mesh.vertices.len() as u32;
            for pos in [egui::pos2(px(d0.day), py(lo0)), egui::pos2(px(d1.day), py(lo1)), egui::pos2(px(d1.day), py(lo1 + v1)), egui::pos2(px(d0.day), py(lo0 + v0))] {
                mesh.colored_vertex(pos, c);
            }
            mesh.add_triangle(i, i + 1, i + 2);
            mesh.add_triangle(i, i + 2, i + 3);
            lo0 += v0;
            lo1 += v1;
        }
    }
    if days.len() == 1 {
        // um dia só: barra única
        let d = &days[0];
        let mut lo = 0.0;
        for st in STACK {
            let v = d.flow[status_idx(st)] as f64;
            p.rect_filled(Rect::from_min_max(egui::pos2(plot.left(), py(lo + v)), egui::pos2(plot.right(), py(lo))), 0.0, strip_color(t, st));
            lo += v;
        }
    }
    p.add(Shape::mesh(mesh));
    p.rect_stroke(plot, 0.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    x_dates(&p, t, plot, first.day, last.day, 4);
    if let Some(hp) = resp.hover_pos().filter(|hp| plot.contains(*hp)) {
        let d = first.day + ((hp.x - plot.left()) / plot.width() * span).round() as i64;
        if let Some(pt) = days.iter().find(|x| x.day == d) {
            p.vline(px(d), plot.y_range(), Stroke::new(1.0, t.text));
            resp.on_hover_ui_at_pointer(|ui| {
                ui.label(egui::RichText::new(format_date(d)).strong());
                for st in STATUS_ORDER {
                    ui.label(format!("{}: {}", status_name(st), pt.flow[status_idx(st)]));
                }
            });
        }
    }
}

// ------------------------------------------------------------------------------------------------ 6. vazão

fn chart_throughput(ui: &mut Ui, t: &Tokens, rect: Rect, a: &Analysis) {
    let p = ui.painter_at(rect);
    let resp = hover_rows(ui, rect, "throughput");
    let weeks = &a.stats.weeks;
    if weeks.is_empty() {
        return;
    }
    let max = weeks.iter().map(|w| w.approvals.max(w.cuts_done)).max().unwrap_or(1) as f64;
    let plot_rect = Rect::from_min_max(rect.min, rect.max - egui::vec2(0.0, 14.0));
    let (plot, top) = y_axis(&p, t, plot_rect, max);
    let slot = plot.width() / weeks.len() as f32;
    let mut hovered = None;
    for (i, w) in weeks.iter().enumerate() {
        let x = plot.left() + i as f32 * slot;
        let bw = (slot * 0.55).min(34.0);
        let h1 = plot.height() * (w.approvals as f64 / top) as f32;
        p.rect_filled(Rect::from_min_max(egui::pos2(x + slot * 0.1, plot.bottom() - h1), egui::pos2(x + slot * 0.1 + bw, plot.bottom())), 2.0, strip_color(t, StageStatus::Approved));
        let h2 = plot.height() * (w.cuts_done as f64 / top) as f32;
        let cx = x + slot * 0.1 + bw + 2.0;
        p.rect_filled(Rect::from_min_max(egui::pos2(cx, plot.bottom() - h2), egui::pos2(cx + bw * 0.45, plot.bottom())), 2.0, t.accent);
        if i % 2 == weeks.len() % 2 || weeks.len() <= 6 {
            txt(&p, egui::pos2(x + slot * 0.1, plot.bottom() + 3.0), egui::Align2::LEFT_TOP, &format_date(w.start_day)[5..], 10.0, t.text_dim);
        }
        if resp.hover_pos().is_some_and(|hp| hp.x >= x && hp.x < x + slot) {
            hovered = Some(i);
        }
    }
    legend(&p, t, egui::pos2(plot.left() + 6.0, plot.top() - 2.0), &[(strip_color(t, StageStatus::Approved), tr("prod.chart.approvals")), (t.accent, tr("prod.chart.cuts_finished"))]);
    // previsão de término
    let f = &a.stats.forecast;
    let line = match f.days_left {
        Some(_) if f.remaining_cuts == 0 => tr("prod.chart.forecast_done").to_owned(),
        Some(d) => format!("{} {} · {:.1}/{} → {:.0} {}", f.remaining_cuts, tr("prod.chart.cuts_remaining"), f.per_day, tr("prod.days_short"), d, tr("prod.days_short")),
        None => format!("{} {} · {}", f.remaining_cuts, tr("prod.chart.cuts_remaining"), tr("prod.chart.no_throughput")),
    };
    txt(&p, egui::pos2(rect.right(), rect.bottom() - 6.0), egui::Align2::RIGHT_CENTER, line, 10.5, t.text_dim);
    if let Some(i) = hovered {
        let w = &weeks[i];
        resp.on_hover_text(format!("{} – {}\n{}: {}\n{}: {}", format_date(w.start_day), format_date(w.start_day + 6), tr("prod.chart.approvals"), w.approvals, tr("prod.chart.cuts_finished"), w.cuts_done));
    }
}

// ------------------------------------------------------------------------------------------------ 7 e 8. barras horizontais por etapa

/// Barras horizontais por etapa: `value` em [0, max]; `None` mostra "—".
fn hbars(ui: &mut Ui, t: &Tokens, rect: Rect, id: &str, items: &[(String, Option<f64>, String)], max: f64, color: Color32) {
    let p = ui.painter_at(rect);
    let resp = hover_rows(ui, rect, id);
    let row_h = (rect.height() / items.len().max(1) as f32).min(20.0);
    let x0 = rect.left() + 54.0;
    let w = (rect.width() - 54.0 - 64.0).max(30.0);
    let mut hovered = None;
    for (i, (name, v, label)) in items.iter().enumerate() {
        let y = rect.top() + i as f32 * row_h;
        txt(&p, egui::pos2(rect.left(), y + row_h / 2.0), egui::Align2::LEFT_CENTER, name, 11.5, t.text);
        p.rect_filled(Rect::from_min_size(egui::pos2(x0, y + 3.0), egui::vec2(w, row_h - 6.0)), 2.0, t.bg_panel);
        if let Some(v) = v {
            let bw = w * (*v / max.max(1e-9)).clamp(0.0, 1.0) as f32;
            p.rect_filled(Rect::from_min_size(egui::pos2(x0, y + 3.0), egui::vec2(bw, row_h - 6.0)), 2.0, color);
        }
        txt(&p, egui::pos2(rect.right(), y + row_h / 2.0), egui::Align2::RIGHT_CENTER, label, 11.0, if v.is_some() { t.text } else { t.text_dim });
        if resp.hover_pos().is_some_and(|hp| hp.y >= y && hp.y < y + row_h) {
            hovered = Some(i);
        }
    }
    if let Some(i) = hovered {
        resp.on_hover_text(format!("{}: {}", items[i].0, items[i].2));
    }
}

fn chart_timing(ui: &mut Ui, t: &Tokens, rect: Rect, a: &Analysis) {
    let max = a.stats.timing.iter().filter_map(|x| x.avg_days).fold(1.0, f64::max);
    let items: Vec<(String, Option<f64>, String)> = a
        .stats
        .timing
        .iter()
        .map(|x| (tr(stage_key(x.stage)).to_owned(), x.avg_days, x.avg_days.map_or("—".to_owned(), |d| format!("{d:.1} {} ({})", tr("prod.days_short"), x.samples))))
        .collect();
    hbars(ui, t, Rect::from_min_max(rect.min, rect.max - egui::vec2(0.0, 2.0)), "timing", &items, max, strip_color(t, StageStatus::InProgress));
}

fn chart_retake(ui: &mut Ui, t: &Tokens, rect: Rect, a: &Analysis) {
    let items: Vec<(String, Option<f64>, String)> = a
        .stats
        .retake_rates
        .iter()
        .map(|x| (tr(stage_key(x.stage)).to_owned(), (x.touched > 0).then_some(x.rate), if x.touched > 0 { format!("{:.0}% ({}/{})", x.rate * 100.0, x.retaken, x.touched) } else { "—".to_owned() }))
        .collect();
    let max = a.stats.retake_rates.iter().map(|x| x.rate).fold(0.25, f64::max);
    hbars(ui, t, Rect::from_min_max(rect.min, rect.max - egui::vec2(0.0, 2.0)), "retake", &items, max, strip_color(t, StageStatus::Retake));
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_steps_are_round_numbers() {
        assert_eq!(nice_step(10.0, 4), 5.0);
        assert_eq!(nice_step(7.0, 4), 2.0);
        assert_eq!(nice_step(100.0, 4), 50.0);
        assert_eq!(nice_step(3.0, 4), 1.0);
        assert_eq!(nice_step(1000.0, 5), 200.0);
    }

    #[test]
    fn stack_covers_every_status_once() {
        let mut idx: Vec<usize> = STACK.iter().map(|s| status_idx(*s)).collect();
        idx.sort();
        assert_eq!(idx, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn charts_draw_without_panicking_on_empty_and_full_analyses() {
        use aiai_tools::production::CutRow;
        use aiai_tools::production_stats::{StatsConfig, analyze};
        use aiai_core::Id;
        let t = Tokens::dark();
        let mk = |n: u32, st: StageStatus| CutRow {
            cut: Id(format!("c{n}")),
            number: n,
            name: format!("C{n:03}"),
            episode: None,
            scene: Some(Id("s".into())),
            scene_name: "Cena".into(),
            duration: 24,
            stages: ALL_STAGES.iter().map(|s| (*s, st)).collect(),
            open_corrections: 0,
            retakes: 0,
            open_retakes: 0,
            due: Some("1970-03-01".into()),
            assignee: None,
            weight: None,
            notes: String::new(),
            thumbnail: None,
        };
        for rows in [vec![], vec![mk(1, StageStatus::Approved), mk(2, StageStatus::Retake), mk(3, StageStatus::NotStarted)]] {
            let a = analyze(&rows, &[], &StatsConfig::new(40 * 86_400_000));
            let ctx = egui::Context::default();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let r = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(300.0, 180.0));
                chart_stages(ui, &t, r, &a, false);
                chart_stages(ui, &t, r, &a, true);
                chart_donut(ui, &t, r, &a, false);
                chart_matrix(ui, &t, r, &a);
                chart_burndown(ui, &t, r, &a);
                chart_flow(ui, &t, r, &a);
                chart_throughput(ui, &t, r, &a);
                chart_timing(ui, &t, r, &a);
                chart_retake(ui, &t, r, &a);
            });
            out.textures_delta.clear();
        }
    }
}
