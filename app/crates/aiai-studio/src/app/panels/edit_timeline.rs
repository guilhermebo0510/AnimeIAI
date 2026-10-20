//! Linha do tempo da Edição (S24 3.14, estilo Premiere): trilhas separadas (legendas S em cima, vídeo Vn..V1, áudio
//! A1..An embaixo) com cabeçalho (destino, olho, cadeado, mudo/solo), régua com cabeçote, zoom e rolagem, imã (snap),
//! seleção vinculada e as ferramentas do Premiere: Seleção V (mover entre trilhas, aparar nas pontas), Ripple B, Roll N,
//! Navalha C, Slip Y, Slide U, Mão H e Zoom Z. Fontes arrastadas da Mídia ou do monitor de origem entram por
//! sobrescrever (Ctrl = inserir); transições arrastadas do painel Transições caem no ponto de corte mais perto.
//!
//! Atalhos da Edição (`shortcuts`, só neste workspace e fora de campos de texto): ferramentas, espaço/J/K/L,
//! setas, Home/End, cima/baixo (pontos de corte), I/O, `,` inserir, `.` sobrescrever, Ctrl+K dividir no cabeçote,
//! Delete/Shift+Delete, Q/W, Ctrl+D transição padrão, Ctrl+L vínculo, S imã, `=`/`-`/`\` zoom, T legenda.

use super::edit::{self, DragKind, EditTool, Focus, MediaDrag, Opened, TlDrag, sync};
use crate::app::ui_kit::Env;
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_tools::edit::ops::Edge;
use aiai_tools::edit::{EditDoc, EditItem, ItemSource, TrackKind, TransitionKind};
use egui::{Color32, FontId, Id as EId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui};
use std::sync::Arc;

const TOOLBAR_H: f32 = 28.0;
const RULER_H: f32 = 22.0;
const HEADER_W: f32 = 124.0;
const EDGE_PX: f32 = 6.0;
const SNAP_PX: f32 = 8.0;

/// Transição arrastada do painel Transições.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TrDrag(pub TransitionKind);

/// Uma linha da linha do tempo: trilha, tipo e faixa vertical.
#[derive(Clone, Copy)]
struct Row {
    track: u32,
    kind: TrackKind,
    rect: Rect,
}

fn row_height(k: TrackKind) -> f32 {
    match k {
        TrackKind::Video => 46.0,
        TrackKind::Audio => 36.0,
        TrackKind::Subtitle => 26.0,
    }
}

/// Ordem das linhas: legendas, vídeo de cima (Vn) para baixo (V1), áudio A1..An.
fn row_order(doc: &EditDoc) -> Vec<(u32, TrackKind)> {
    let mut v: Vec<(u32, TrackKind)> = doc.tracks_of(TrackKind::Subtitle).iter().map(|t| (t.id, t.kind)).collect();
    v.extend(doc.tracks_of(TrackKind::Video).iter().rev().map(|t| (t.id, t.kind)));
    v.extend(doc.tracks_of(TrackKind::Audio).iter().map(|t| (t.id, t.kind)));
    v
}

fn item_color(s: &ItemSource) -> Color32 {
    match s {
        ItemSource::Cut { .. } => Color32::from_rgb(62, 104, 165),
        ItemSource::Image { .. } => Color32::from_rgb(128, 92, 160),
        ItemSource::Color { .. } => Color32::from_rgb(105, 105, 112),
        ItemSource::CutAudio { .. } | ItemSource::Audio { .. } => Color32::from_rgb(52, 128, 88),
        ItemSource::Caption { .. } => Color32::from_rgb(170, 140, 50),
    }
}

/// Geometria horizontal: pixels por quadro, primeiro quadro visível e onde começa o conteúdo.
#[derive(Clone, Copy)]
struct View {
    ppf: f32,
    scroll: f32,
    x0: f32,
}

impl View {
    fn x(&self, f: f32) -> f32 {
        self.x0 + (f - self.scroll) * self.ppf
    }
    fn frame_at(&self, x: f32) -> f32 {
        self.scroll + (x - self.x0) / self.ppf
    }
}

/// Pontos onde o imã gruda: pontas dos itens (fora `except`), cabeçote e zero.
fn snap_points(doc: &EditDoc, except: &[u32], playhead: u32) -> Vec<u32> {
    let mut v = vec![0, playhead];
    for it in doc.items.iter().filter(|i| !except.contains(&i.id)) {
        v.push(it.start);
        v.push(it.end());
    }
    v
}

/// Ajusta `delta` para que alguma das `edges` (+delta) grude no ponto mais perto, se estiver a menos de SNAP_PX.
fn snap_delta(points: &[u32], edges: &[i64], delta: i64, ppf: f32) -> i64 {
    let mut best: Option<(f32, i64)> = None;
    for e in edges {
        let moved = e + delta;
        for p in points {
            let d = (*p as i64 - moved) as f32 * ppf;
            if d.abs() <= SNAP_PX && best.is_none_or(|b| d.abs() < b.0) {
                best = Some((d.abs(), *p as i64 - e));
            }
        }
    }
    best.map(|b| b.1).unwrap_or(delta)
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = crate::app::ui_kit::body_ui(ui, body, 4.0);
    let ui = &mut child;
    let Some(o) = sync(env) else {
        ui.label(egui::RichText::new(tr("editing.no_edit_long")).color(t.text_dim));
        return;
    };
    let tl = match &o.tl {
        Ok(x) => *x,
        Err(e) => {
            ui.label(egui::RichText::new(format!("{}: {e}", tr("editing.invalid"))).color(t.accent));
            return;
        }
    };
    let fps = env.core.project.settings.fps();
    let full = ui.max_rect();

    // ---- barra de ferramentas
    let bar = Rect::from_min_size(full.min, egui::vec2(full.width(), TOOLBAR_H));
    let content_w = (full.width() - HEADER_W).max(50.0);
    let span = (tl.total.max(1) as f32 * 1.08 + fps as f32).max(1.0);
    let fit = content_w / span;
    let ppf = env.ed.edit.zoom.unwrap_or(fit).clamp(0.05, 60.0);
    toolbar(env, ui, bar, &o, ppf, fit, tl.total, fps);

    // ---- área: cabeçalhos à esquerda, régua e trilhas à direita
    let area = Rect::from_min_max(egui::pos2(full.left(), bar.bottom() + 2.0), full.max);
    let ppf = env.ed.edit.zoom.unwrap_or(fit).clamp(0.05, 60.0);
    let view_doc: Arc<EditDoc> = env.ed.edit.drag.as_ref().and_then(|d| d.cur.clone()).unwrap_or_else(|| o.doc.clone());
    let max_scroll = (span - content_w / ppf).max(0.0);
    env.ed.edit.scroll = env.ed.edit.scroll.clamp(0.0, max_scroll);
    let view = View { ppf, scroll: env.ed.edit.scroll, x0: area.left() + HEADER_W };
    let ruler = Rect::from_min_max(egui::pos2(view.x0, area.top()), egui::pos2(area.right(), area.top() + RULER_H));
    let rows_area = Rect::from_min_max(egui::pos2(area.left(), ruler.bottom() + 1.0), area.max);
    let order = row_order(&view_doc);
    let total_h: f32 = order.iter().map(|(_, k)| row_height(*k) + 2.0).sum();
    env.ed.edit.scroll_y = env.ed.edit.scroll_y.clamp(0.0, (total_h - rows_area.height()).max(0.0));
    let mut y = rows_area.top() - env.ed.edit.scroll_y;
    let rows: Vec<Row> = order
        .iter()
        .map(|(id, k)| {
            let r = Rect::from_min_size(egui::pos2(area.left(), y), egui::vec2(area.width(), row_height(*k)));
            y += row_height(*k) + 2.0;
            Row { track: *id, kind: *k, rect: r }
        })
        .collect();
    let content = Rect::from_min_max(egui::pos2(view.x0, rows_area.top()), rows_area.max);
    env.note("edit:timeline_rows", content);

    // ---- rolagem e zoom pela roda (roda = horizontal, Shift = vertical, Alt/Ctrl = zoom no ponteiro)
    let hover_area = ui.rect_contains_pointer(area);
    if hover_area {
        let (scroll, mods, ptr) = ui.input(|i| (i.smooth_scroll_delta, i.modifiers, i.pointer.hover_pos()));
        let d = if scroll.x.abs() > scroll.y.abs() { scroll.x } else { scroll.y };
        if d != 0.0 {
            if mods.alt || mods.ctrl || mods.command {
                let px = ptr.map(|p| p.x).unwrap_or(view.x0);
                let f_at = view.frame_at(px);
                let nz = (ppf * (1.0 + d * 0.004).clamp(0.5, 2.0)).clamp(0.05, 60.0);
                env.ed.edit.zoom = Some(nz);
                env.ed.edit.scroll = (f_at - (px - view.x0) / nz).max(0.0);
            } else if mods.shift {
                env.ed.edit.scroll_y = (env.ed.edit.scroll_y - d).max(0.0);
            } else {
                env.ed.edit.scroll = (env.ed.edit.scroll - d / ppf).max(0.0);
            }
        }
    }

    let painter = ui.painter().clone();
    let clip_rows = painter.with_clip_rect(rows_area);
    // ---- régua
    painter.rect_filled(ruler, 2.0, t.bg_widget);
    let px_per_s = ppf * fps as f32;
    let step_s = [0.25f32, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0].into_iter().find(|s| s * px_per_s >= 60.0).unwrap_or(600.0);
    let first_s = (view.scroll / fps as f32 / step_s).floor() * step_s;
    let mut s = first_s.max(0.0);
    let rp = painter.with_clip_rect(ruler);
    while (s * fps as f32) <= view.frame_at(ruler.right()) {
        let x = view.x((s * fps as f32).round());
        rp.vline(x, ruler.y_range(), Stroke::new(1.0, t.border));
        rp.text(egui::pos2(x + 3.0, ruler.center().y), egui::Align2::LEFT_CENTER, edit::timecode((s * fps as f32).round() as u32, fps), FontId::proportional(10.0), t.text_dim);
        s += step_s;
    }
    // marcas de cada quadro quando o zoom deixa
    if ppf >= 6.0 {
        let (f0, f1) = (view.scroll.floor() as i64, view.frame_at(ruler.right()).ceil() as i64);
        for f in f0.max(0)..f1 {
            let x = view.x(f as f32);
            rp.vline(x, (ruler.bottom() - 4.0)..=ruler.bottom(), Stroke::new(1.0, t.border));
        }
    }
    if tl.total > tl.credits_start {
        let r = Rect::from_min_max(egui::pos2(view.x(tl.credits_start as f32), ruler.bottom() - 4.0), egui::pos2(view.x(tl.total as f32), ruler.bottom()));
        rp.rect_filled(r, 0.0, t.text_dim.gamma_multiply(0.5));
    }
    let rr = ui.interact(ruler, EId::new("edit_ruler"), Sense::click_and_drag());
    env.note("edit:ruler", ruler);
    if rr.drag_started() || rr.clicked() {
        env.ed.edit.stop_play();
        env.ed.edit.focus = Focus::Program;
    }
    if let Some(p) = rr.interact_pointer_pos().filter(|_| rr.clicked() || rr.dragged()) {
        let mut f = view.frame_at(p.x).floor().max(0.0) as u32;
        if env.ed.edit.snap {
            let pts = snap_points(&view_doc, &[], u32::MAX);
            let d = snap_delta(&pts, &[f as i64], 0, ppf);
            f = (f as i64 + d).max(0) as u32;
        }
        env.ed.edit.frame = f.min(tl.total.saturating_sub(1));
    }

    // ---- cabeçalhos e fundo das trilhas
    for row in &rows {
        if row.rect.bottom() < rows_area.top() || row.rect.top() > rows_area.bottom() {
            continue;
        }
        let lane = Rect::from_min_max(egui::pos2(view.x0, row.rect.top()), row.rect.right_bottom());
        clip_rows.rect_filled(lane, 2.0, t.bg_frame);
        track_header(env, ui, &view_doc, &o, row, rows_area);
    }
    if tl.total > tl.credits_start {
        if let Some(v1) = rows.iter().rev().find(|r| r.kind == TrackKind::Video) {
            let r = Rect::from_min_max(egui::pos2(view.x(tl.credits_start as f32), v1.rect.top()), egui::pos2(view.x(tl.total as f32), v1.rect.bottom())).shrink2(egui::vec2(0.5, 2.0));
            clip_rows.with_clip_rect(content).rect(r, 3.0, t.bg_header, Stroke::new(1.0, t.border), StrokeKind::Inside);
            clip_rows.with_clip_rect(r.intersect(content)).text(r.left_top() + egui::vec2(5.0, 4.0), egui::Align2::LEFT_TOP, tr("editing.credits"), FontId::proportional(11.5), t.text_dim);
        }
    }

    // ---- itens
    let ctx = ui.ctx().clone();
    let mut budget = super::production::THUMB_BUDGET;
    let cp = painter.with_clip_rect(content);
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let mut hovered: Option<(u32, Option<Edge>)> = None;
    for row in &rows {
        let locked = view_doc.track(row.track).is_some_and(|x| x.locked);
        for it in view_doc.items_on(row.track) {
            let (xa, xb) = (view.x(it.start as f32), view.x(it.end() as f32));
            if xb < content.left() || xa > content.right() {
                continue;
            }
            let r = Rect::from_min_max(egui::pos2(xa, row.rect.top() + 1.0), egui::pos2(xb.max(xa + 2.0), row.rect.bottom() - 1.0));
            env.note(format!("edit:item:{}", it.id), r.intersect(content));
            let sel = env.ed.edit.sel.contains(&it.id);
            let mut fill = item_color(&it.source);
            if sel {
                fill = fill.gamma_multiply(1.35);
            }
            if !it.enabled {
                fill = fill.gamma_multiply(0.4);
            }
            if locked {
                fill = fill.gamma_multiply(0.7);
            }
            cp.rect(r, 3.0, fill, Stroke::new(if sel { 2.0 } else { 1.0 }, if sel { t.accent } else { Color32::from_black_alpha(90) }), StrokeKind::Inside);
            let ip = cp.with_clip_rect(r.shrink(2.0).intersect(content));
            let mut tx = r.left() + 5.0;
            if let (ItemSource::Cut { cut }, true) = (&it.source, r.height() >= 30.0 && r.width() >= 70.0) {
                let (w, h) = (env.core.project.settings.width.max(1) as f32, env.core.project.settings.height.max(1) as f32);
                let th = r.height() - 6.0;
                let thumb = Rect::from_min_size(r.left_top() + egui::vec2(3.0, 3.0), egui::vec2(th * w / h, th));
                if let Some(tex) = env.ed.edit.thumbs.get(&ctx, env.core, cut, &mut budget) {
                    ip.image(tex, thumb, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    tx = thumb.right() + 5.0;
                }
            }
            let label = edit::source_label(&env.core.project, &it.source);
            ip.text(egui::pos2(tx, r.top() + 3.0), egui::Align2::LEFT_TOP, &label, FontId::proportional(11.5), Color32::WHITE);
            let mut badges = Vec::new();
            if it.speed != 1.0 {
                badges.push(format!("{:.0}%", it.speed * 100.0));
            }
            if !it.filters.is_empty() || !it.audio_fx.is_empty() {
                badges.push("fx".to_owned());
            }
            if it.link.is_some() {
                badges.push("∞".to_owned());
            }
            if it.anchor.is_some() {
                badges.push("⚓".to_owned());
            }
            if r.height() >= 30.0 && !badges.is_empty() {
                ip.text(egui::pos2(tx, r.bottom() - 3.0), egui::Align2::LEFT_BOTTOM, badges.join(" "), FontId::proportional(10.0), Color32::from_white_alpha(210));
            }
            // forma de onda simples do som (picos do trecho)
            if matches!(it.source, ItemSource::Audio { .. } | ItemSource::CutAudio { .. }) && r.height() >= 24.0 {
                let mid = r.center().y + 4.0;
                ip.hline(r.x_range(), mid, Stroke::new(1.0, Color32::from_black_alpha(60)));
            }
            // transições (faixa clara com diagonal sobre o trecho)
            for (edge, tr_) in [(Edge::In, &it.transition_in), (Edge::Out, &it.transition_out)] {
                let Some(trn) = tr_.as_ref() else { continue };
                let (s, e) = transition_span(&view_doc, it, edge, trn);
                let br = Rect::from_min_max(egui::pos2(view.x(s as f32), r.top() + 2.0), egui::pos2(view.x(e as f32).max(view.x(s as f32) + 4.0), r.bottom() - 2.0));
                let tsel = env.ed.edit.sel_tr == Some((it.id, edge));
                cp.rect(br, 2.0, Color32::from_white_alpha(if tsel { 120 } else { 70 }), Stroke::new(if tsel { 2.0 } else { 1.0 }, if tsel { t.accent } else { Color32::from_white_alpha(160) }), StrokeKind::Inside);
                cp.line_segment([br.left_bottom(), br.right_top()], Stroke::new(1.0, Color32::from_black_alpha(120)));
                env.note(format!("edit:tr:{}:{}", it.id, if edge == Edge::In { "in" } else { "out" }), br.intersect(content));
                if pointer.is_some_and(|p| br.contains(p)) {
                    hovered = Some((it.id, Some(edge)));
                }
            }
        }
    }

    // ---- interação com o mouse (uma área só: o gesto decide pelo que está sob o ponteiro)
    let resp = ui.interact(content, EId::new("edit_tl_content"), Sense::click_and_drag());
    let mods = ui.input(|i| i.modifiers);
    let tool = env.ed.edit.tool;
    let hit = |p: Pos2| -> Option<(Row, EditItem, Option<Edge>)> {
        let row = rows.iter().find(|r| r.rect.y_range().contains(p.y))?;
        let f = view.frame_at(p.x);
        let it = view_doc.items_on(row.track).into_iter().find(|i| f >= i.start as f32 && f < i.end() as f32 || (view.x(i.start as f32) - p.x).abs() <= EDGE_PX || (view.x(i.end() as f32) - p.x).abs() <= EDGE_PX)?;
        let (xa, xb) = (view.x(it.start as f32), view.x(it.end() as f32));
        let edge = if (xb - xa) > EDGE_PX * 3.0 {
            if (p.x - xa).abs() <= EDGE_PX {
                Some(Edge::In)
            } else if (p.x - xb).abs() <= EDGE_PX {
                Some(Edge::Out)
            } else {
                None
            }
        } else {
            None
        };
        Some((*row, it.clone(), edge))
    };
    // cursor conforme a ferramenta
    if let Some(p) = pointer.filter(|p| content.contains(*p)) {
        let h = hit(p);
        let icon = match (tool, &h) {
            (EditTool::Hand, _) => egui::CursorIcon::Grab,
            (EditTool::Zoom, _) => egui::CursorIcon::ZoomIn,
            (EditTool::Razor, Some(_)) => egui::CursorIcon::Crosshair,
            (EditTool::Select | EditTool::Ripple | EditTool::Roll, Some((_, _, Some(_)))) => egui::CursorIcon::ResizeHorizontal,
            (EditTool::Slip | EditTool::Slide, Some(_)) => egui::CursorIcon::ResizeColumn,
            _ => egui::CursorIcon::Default,
        };
        ui.ctx().set_cursor_icon(icon);
        if tool == EditTool::Razor && h.is_some() {
            cp.vline(p.x, rows_area.y_range(), Stroke::new(1.0, t.accent.gamma_multiply(0.7)));
        }
    }
    let middle = ui.input(|i| i.pointer.middle_down());
    if resp.drag_started() {
        let p = resp.interact_pointer_pos().unwrap_or(content.center());
        let f = view.frame_at(p.x);
        let base = o.doc.clone();
        let kind = if middle || tool == EditTool::Hand {
            Some(DragKind::Pan { scroll0: env.ed.edit.scroll, scroll_y0: env.ed.edit.scroll_y })
        } else if let Some((iid, Some(edge))) = hovered.filter(|_| tool == EditTool::Select) {
            env.ed.edit.sel_tr = Some((iid, edge));
            env.ed.edit.sel.clear();
            Some(DragKind::Transition { item: iid, edge })
        } else {
            match (tool, hit(p)) {
                (EditTool::Select, Some((_, it, Some(edge)))) => Some(DragKind::Trim { ids: group(env, &base, it.id, mods.alt), edge, ripple: false, primary: it.id }),
                (EditTool::Ripple, Some((_, it, Some(edge)))) => Some(DragKind::Trim { ids: group(env, &base, it.id, mods.alt), edge, ripple: true, primary: it.id }),
                (EditTool::Roll, Some((_, it, Some(edge)))) => {
                    // o ponto de corte: o item que sai é o da esquerda
                    let left = match edge {
                        Edge::Out => Some(it.id),
                        Edge::In => base.neighbor_before(it.id).map(|x| x.id),
                    };
                    left.map(|l| DragKind::Roll { left: l })
                }
                (EditTool::Select | EditTool::Ripple | EditTool::Roll, Some((_, it, None))) => {
                    select_item(env, &base, it.id, mods.shift || mods.ctrl);
                    let ids = if env.ed.edit.sel.contains(&it.id) { env.ed.edit.sel.clone() } else { group(env, &base, it.id, mods.alt) };
                    Some(DragKind::Move { ids, primary: it.id })
                }
                (EditTool::Slip, Some((_, it, _))) => Some(DragKind::Slip { ids: group(env, &base, it.id, mods.alt) }),
                (EditTool::Slide, Some((_, it, _))) => Some(DragKind::Slide { id: it.id }),
                _ => None,
            }
        };
        if let Some(kind) = kind {
            env.ed.edit.drag = Some(TlDrag { kind, start_doc: base, origin_frame: f, origin_y: p.y, cur: None });
            env.ed.edit.focus = Focus::Program;
        }
    }
    if let (Some(d), Some(p)) = (env.ed.edit.drag.clone(), ui.input(|i| i.pointer.interact_pos())) {
        if let DragKind::Pan { scroll0, scroll_y0 } = d.kind {
            let pf = view.frame_at(p.x);
            env.ed.edit.scroll = (scroll0 - (pf - d.origin_frame)).max(0.0);
            env.ed.edit.scroll_y = (scroll_y0 - (p.y - d.origin_y)).max(0.0);
        } else {
            let raw = (view.frame_at(p.x) - d.origin_frame).round() as i64;
            let cur = drag_result(env, &d, raw, p.y, &rows, ppf);
            if let Some(dd) = env.ed.edit.drag.as_mut() {
                dd.cur = Some(Arc::new(cur));
            }
        }
        ui.ctx().request_repaint();
    }
    if resp.drag_stopped() || (env.ed.edit.drag.is_some() && !ui.input(|i| i.pointer.any_down())) {
        if let Some(d) = env.ed.edit.drag.take() {
            if let Some(cur) = d.cur.filter(|c| **c != *d.start_doc) {
                edit::save(env, &o.id, &cur);
            }
        }
    }
    if resp.clicked() {
        let p = resp.interact_pointer_pos().unwrap_or(content.center());
        let f = view.frame_at(p.x).max(0.0);
        env.ed.edit.focus = Focus::Program;
        match tool {
            EditTool::Razor => {
                if let Some((_, it, _)) = hit(p) {
                    let mut at = f.round() as u32;
                    if env.ed.edit.snap && ((env.ed.edit.frame as f32 - f) * ppf).abs() <= SNAP_PX {
                        at = env.ed.edit.frame;
                    }
                    let shift = mods.shift;
                    let alt = mods.alt;
                    edit::modify(env, |d, _| {
                        let made = if shift {
                            let ids: Vec<u32> = d.items.iter().filter(|i| i.source.track_kind() != TrackKind::Subtitle).map(|i| i.id).collect();
                            d.razor(at, Some(&ids))
                        } else if alt {
                            match d.split_item(it.id, at) {
                                Some(n) => {
                                    if let Some(x) = d.item_mut(n) {
                                        x.link = None;
                                    }
                                    vec![n]
                                }
                                None => Vec::new(),
                            }
                        } else {
                            d.razor(at, Some(&[it.id]))
                        };
                        !made.is_empty()
                    });
                }
            }
            EditTool::Zoom => {
                let k = if mods.alt { 1.0 / 1.6 } else { 1.6 };
                let nz = (ppf * k).clamp(0.05, 60.0);
                env.ed.edit.zoom = Some(nz);
                env.ed.edit.scroll = (f - (p.x - view.x0) / nz).max(0.0);
            }
            EditTool::Hand => {}
            _ => {
                if let Some((iid, Some(edge))) = hovered {
                    env.ed.edit.sel_tr = Some((iid, edge));
                    env.ed.edit.sel.clear();
                } else if let Some((_, it, _)) = hit(p) {
                    select_item(env, &o.doc, it.id, mods.shift || mods.ctrl);
                } else {
                    env.ed.edit.sel.clear();
                    env.ed.edit.sel_tr = None;
                    env.ed.edit.frame = (f.floor() as u32).min(tl.total.saturating_sub(1));
                    env.ed.edit.stop_play();
                }
            }
        }
    }

    // ---- soltar mídia ou transição arrastada de outro painel
    drop_targets(env, ui, &o, &view_doc, &rows, view, content);

    // ---- cabeçote
    let x = view.x(env.ed.edit.frame as f32 + 0.5);
    if x >= view.x0 && x <= area.right() {
        painter.with_clip_rect(area).vline(x, ruler.top()..=area.bottom(), Stroke::new(1.5, t.accent));
        let tri = vec![egui::pos2(x - 5.0, ruler.top()), egui::pos2(x + 5.0, ruler.top()), egui::pos2(x, ruler.top() + 7.0)];
        painter.add(egui::Shape::convex_polygon(tri, t.accent, Stroke::NONE));
    }
    // seguir o cabeçote na reprodução
    if env.ed.edit.play.is_some() && env.ed.edit.zoom.is_some() {
        let f = env.ed.edit.frame as f32;
        let vis = content_w / ppf;
        if f < env.ed.edit.scroll || f > env.ed.edit.scroll + vis * 0.95 {
            env.ed.edit.scroll = (f - vis * 0.1).max(0.0);
        }
    }
}

/// Itens que andam juntos com `id` (o grupo vinculado quando a seleção vinculada está ligada; Alt solta o vínculo).
fn group(env: &Env, doc: &EditDoc, id: u32, alt: bool) -> Vec<u32> {
    if env.ed.edit.linked_sel && !alt { doc.linked(id) } else { vec![id] }
}

fn select_item(env: &mut Env, doc: &EditDoc, id: u32, add: bool) {
    let g = if env.ed.edit.linked_sel { doc.linked(id) } else { vec![id] };
    let st = &mut env.ed.edit;
    st.sel_tr = None;
    if add {
        if st.sel.contains(&id) {
            st.sel.retain(|x| !g.contains(x));
        } else {
            st.sel.extend(g);
        }
    } else if !st.sel.contains(&id) {
        st.sel = g;
    }
}

/// Trecho (quadros da edição) desenhado de uma transição.
fn transition_span(doc: &EditDoc, it: &EditItem, edge: Edge, tr_: &aiai_tools::edit::Transition) -> (u32, u32) {
    match edge {
        Edge::In => {
            if tr_.kind.is_fade_in() || doc.neighbor_before(it.id).is_none() {
                (it.start, it.start + tr_.frames.min(it.dur))
            } else {
                let (l, r) = tr_.split();
                (it.start.saturating_sub(l), it.start + r)
            }
        }
        Edge::Out => (it.end().saturating_sub(tr_.frames.min(it.dur)), it.end()),
    }
}

/// Resultado do arraste com o deslocamento `raw` (quadros) e o ponteiro em `y`.
fn drag_result(env: &Env, d: &TlDrag, raw: i64, y: f32, rows: &[Row], ppf: f32) -> EditDoc {
    let mut doc = (*d.start_doc).clone();
    let project = &env.core.project;
    let media = edit::media_of(project);
    let snap = env.ed.edit.snap;
    let playhead = env.ed.edit.frame;
    match &d.kind {
        DragKind::Move { ids, primary } => {
            let Some(p) = doc.item(*primary).cloned() else { return doc };
            let delta = if snap { snap_delta(&snap_points(&doc, ids, playhead), &[p.start as i64, p.end() as i64], raw, ppf) } else { raw };
            // trilha sob o ponteiro (do mesmo tipo): quantas trilhas andou
            let kind = p.source.track_kind();
            let order: Vec<u32> = doc.tracks_of(kind).iter().map(|t| t.id).collect();
            let shift = rows.iter().find(|r| r.rect.y_range().contains(y) && r.kind == kind).and_then(|r| {
                let a = order.iter().position(|x| *x == p.track)? as i32;
                let b = order.iter().position(|x| *x == r.track)? as i32;
                Some(b - a)
            });
            let _ = doc.move_items(ids, kind, delta, shift.unwrap_or(0));
        }
        DragKind::Trim { ids, edge, ripple, primary } => {
            let Some(p) = doc.item(*primary).cloned() else { return doc };
            let e = match edge {
                Edge::In => p.start as i64,
                Edge::Out => p.end() as i64,
            };
            let delta = if snap { snap_delta(&snap_points(&doc, ids, playhead), &[e], raw, ppf) } else { raw };
            doc.trim(ids, *edge, delta, *ripple, &media);
        }
        DragKind::Roll { left } => {
            let e = doc.item(*left).map(|i| i.end() as i64).unwrap_or(0);
            let delta = if snap { snap_delta(&snap_points(&doc, &doc.linked(*left), playhead), &[e], raw, ppf) } else { raw };
            doc.roll(*left, delta, &media);
        }
        DragKind::Slip { ids } => {
            // arrastar para a direita mostra mídia anterior (como no Premiere): a entrada recua
            doc.slip(ids, -raw, &media);
        }
        DragKind::Slide { id } => {
            doc.slide(*id, raw, &media);
        }
        DragKind::Transition { item, edge } => {
            if let Some(it) = doc.item(*item).cloned() {
                let cur = match edge {
                    Edge::In => it.transition_in.clone(),
                    Edge::Out => it.transition_out.clone(),
                };
                if let Some(mut c) = cur {
                    c.frames = (c.frames as i64 + raw).clamp(1, 600) as u32;
                    doc.set_transition(*item, *edge, Some(c));
                }
            }
        }
        DragKind::Pan { .. } => {}
    }
    doc
}

/// Destinos de soltar: mídia (sobrescrever; Ctrl = inserir) e transição (no ponto de corte mais perto).
fn drop_targets(env: &mut Env, ui: &Ui, o: &Opened, doc: &EditDoc, rows: &[Row], view: View, content: Rect) {
    let t = env.t;
    let ctx = ui.ctx().clone();
    let Some(p) = ctx.pointer_hover_pos().filter(|p| content.contains(*p)) else { return };
    let released = ctx.input(|i| i.pointer.any_released());
    let ctrl = ctx.input(|i| i.modifiers.ctrl || i.modifiers.command);
    let painter = ui.painter().with_clip_rect(content);
    let row = rows.iter().find(|r| r.rect.y_range().contains(p.y)).copied();
    if let Some(m) = egui::DragAndDrop::payload::<MediaDrag>(&ctx) {
        let at = view.frame_at(p.x).round().max(0.0) as u32;
        let (mut vt, mut at_) = (env.ed.edit.target_v, env.ed.edit.target_a);
        if let Some(r) = row {
            match r.kind {
                TrackKind::Video => vt = Some(r.track),
                TrackKind::Audio => at_ = Some(r.track),
                TrackKind::Subtitle => {}
            }
        }
        let items = EditDoc::source_items(&m.src, m.src_in, m.dur, at, vt, at_);
        for it in &items {
            if let Some(r) = rows.iter().find(|r| r.track == it.track) {
                let g = Rect::from_min_max(egui::pos2(view.x(it.start as f32), r.rect.top() + 1.0), egui::pos2(view.x(it.end() as f32), r.rect.bottom() - 1.0));
                painter.rect(g, 3.0, t.accent.gamma_multiply(0.35), Stroke::new(1.5, t.accent), StrokeKind::Inside);
            }
        }
        painter.text(p + egui::vec2(12.0, -14.0), egui::Align2::LEFT_BOTTOM, format!("{} · {}", m.label, if ctrl { tr("editing.insert") } else { tr("editing.overwrite") }), FontId::proportional(11.5), t.text);
        if released && egui::DragAndDrop::take_payload::<MediaDrag>(&ctx).is_some() && !items.is_empty() {
            let mut d = (*o.doc).clone();
            if items.iter().any(|i| d.is_locked(i.track)) {
                env.ed.toast = Some(tr("editing.track_locked").to_owned());
                return;
            }
            let ids = if ctrl { d.insert_items(items, at) } else { d.overwrite_items(items) };
            if edit::save(env, &o.id, &d) {
                env.ed.edit.sel = ids;
                env.ed.edit.frame = at;
            }
        }
        return;
    }
    if let Some(trd) = egui::DragAndDrop::payload::<TrDrag>(&ctx) {
        let Some(r) = row.filter(|r| r.kind != TrackKind::Subtitle) else { return };
        let f = view.frame_at(p.x).max(0.0) as u32;
        let tol = (12.0 / view.ppf).ceil() as u32;
        let Some((a, b, at)) = doc.edit_point_near(r.track, f, tol.max(1)) else { return };
        let x = view.x(at as f32);
        painter.rect_filled(Rect::from_center_size(egui::pos2(x, r.rect.center().y), egui::vec2(10.0, r.rect.height())), 2.0, t.accent.gamma_multiply(0.6));
        painter.text(p + egui::vec2(12.0, -14.0), egui::Align2::LEFT_BOTTOM, tr(&edit::transition_key(trd.0)), FontId::proportional(11.5), t.text);
        if released && egui::DragAndDrop::take_payload::<TrDrag>(&ctx).is_some() {
            let trn = edit::new_transition(env, trd.0);
            let (target, edge) = match (a, b) {
                (_, Some(b)) => (b, Edge::In),
                (Some(a), None) => (a, Edge::Out),
                _ => return,
            };
            let mut d = (*o.doc).clone();
            if d.set_transition(target, edge, Some(trn)) && edit::save(env, &o.id, &d) {
                env.ed.edit.sel_tr = Some((target, edge));
                env.ed.edit.sel.clear();
            } else {
                env.ed.toast = Some(tr("editing.track_locked").to_owned());
            }
        }
    }
}

/// Cabeçalho de uma trilha: nome (clique = destino), olho/mudo/solo e cadeado.
fn track_header(env: &mut Env, ui: &mut Ui, doc: &EditDoc, o: &Opened, row: &Row, clip: Rect) {
    let t = env.t;
    let Some(track) = doc.track(row.track).cloned() else { return };
    let label = doc.track_label(row.track);
    let hr = Rect::from_min_size(row.rect.min, egui::vec2(HEADER_W - 4.0, row.rect.height()));
    let painter = ui.painter().with_clip_rect(clip);
    let target = match row.kind {
        TrackKind::Video => env.ed.edit.target_v == Some(row.track),
        TrackKind::Audio => env.ed.edit.target_a == Some(row.track),
        TrackKind::Subtitle => env.ed.edit.cap_track == Some(row.track),
    };
    painter.rect_filled(hr, 3.0, t.bg_header);
    // destino (patch): bloco do nome aceso
    let name_r = Rect::from_min_size(hr.min + egui::vec2(3.0, 3.0), egui::vec2(32.0, (hr.height() - 6.0).min(20.0)));
    painter.rect(name_r, 3.0, if target { t.accent_dim } else { t.bg_widget }, Stroke::new(1.0, if target { t.accent } else { t.border }), StrokeKind::Inside);
    painter.text(name_r.center(), egui::Align2::CENTER_CENTER, &label, FontId::proportional(11.5), t.text);
    let nr = ui.interact(name_r.intersect(clip), EId::new(("edit_trk_target", row.track)), Sense::click());
    env.note(format!("edit:track:{label}:target"), name_r);
    if nr.on_hover_text(tr("editing.track_target.tip")).clicked() {
        match row.kind {
            TrackKind::Video => env.ed.edit.target_v = if target { None } else { Some(row.track) },
            TrackKind::Audio => env.ed.edit.target_a = if target { None } else { Some(row.track) },
            TrackKind::Subtitle => env.ed.edit.cap_track = Some(row.track),
        }
    }
    // botões pequenos
    let mut x = name_r.right() + 4.0;
    let bs = 20.0f32.min(hr.height() - 6.0);
    let mut button = |env: &mut Env, ui: &mut Ui, id: &str, on: bool, icon: Option<IconId>, text: &str, tip: &str| -> bool {
        let r = Rect::from_min_size(egui::pos2(x, hr.top() + 3.0), egui::vec2(bs, bs));
        x += bs + 2.0;
        let resp = ui.interact(r.intersect(clip), EId::new(("edit_trk_btn", row.track, id.to_owned())), Sense::click());
        env.note(format!("edit:track:{label}:{id}"), r);
        let p = ui.painter().with_clip_rect(clip);
        p.rect(r, 3.0, if on { t.accent_dim } else if resp.hovered() { t.bg_hover } else { t.bg_widget }, Stroke::new(1.0, if on { t.accent } else { t.border }), StrokeKind::Inside);
        match icon {
            Some(ic) => crate::app::ui_kit::paint_icon(env.icons, &p, ic, r.shrink(3.0), t.text, false),
            None => {
                p.text(r.center(), egui::Align2::CENTER_CENTER, text, FontId::proportional(11.0), t.text);
            }
        }
        resp.on_hover_text(tr(tip)).clicked()
    };
    let mut patch: Option<Box<dyn FnOnce(&mut aiai_tools::edit::EditTrack)>> = None;
    match row.kind {
        TrackKind::Video | TrackKind::Subtitle => {
            if button(env, ui, "eye", !track.hidden, Some(if track.hidden { IconId::EyeOff } else { IconId::Eye }), "", "editing.track_eye.tip") {
                patch = Some(Box::new(|t| t.hidden = !t.hidden));
            }
        }
        TrackKind::Audio => {
            if button(env, ui, "mute", track.muted, None, "M", "editing.track_mute.tip") {
                patch = Some(Box::new(|t| t.muted = !t.muted));
            }
            if button(env, ui, "solo", track.solo, None, "S", "editing.track_solo.tip") {
                patch = Some(Box::new(|t| t.solo = !t.solo));
            }
        }
    }
    if button(env, ui, "lock", track.locked, Some(if track.locked { IconId::Lock } else { IconId::Unlock }), "", "editing.track_lock.tip") {
        patch = Some(Box::new(|t| t.locked = !t.locked));
    }
    if row.kind == TrackKind::Subtitle && hr.height() >= 20.0 {
        let burn = track.burn;
        if button(env, ui, "burn", burn, None, "B", "editing.track_burn.tip") {
            patch = Some(Box::new(|t| t.burn = !t.burn));
        }
    }
    if let Some(f) = patch {
        let mut d = (*o.doc).clone();
        if let Some(tk) = d.track_mut(row.track) {
            f(tk);
        }
        edit::save(env, &o.id, &d);
    }
}

#[allow(clippy::too_many_arguments)]
fn toolbar(env: &mut Env, ui: &mut Ui, bar: Rect, o: &Opened, ppf: f32, fit: f32, total: u32, fps: f64) {
    let t = env.t;
    let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
    bui.spacing_mut().item_spacing.x = 3.0;
    for tool in EditTool::ALL {
        let on = env.ed.edit.tool == tool;
        let label = format!("{}", tool.shortcut().symbol_or_name());
        let r = bui.add(egui::Button::selectable(on, egui::RichText::new(label).size(12.0).strong()).min_size(egui::vec2(24.0, 22.0)));
        env.note(format!("edit:tool:{}", tool.key()), r.rect);
        if r.on_hover_text(format!("{} ({})", tr(&format!("editing.tool.{}", tool.key())), tool.shortcut().symbol_or_name())).clicked() {
            env.ed.edit.tool = tool;
        }
    }
    bui.separator();
    let snap = env.ed.edit.snap;
    if env.icon_button(&mut bui, IconId::Snap, "editing.snap.tip", snap, true, "edit:snap").clicked() {
        env.ed.edit.snap = !snap;
    }
    let linked = env.ed.edit.linked_sel;
    let r = bui.add(egui::Button::selectable(linked, egui::RichText::new("∞").size(13.0)).min_size(egui::vec2(24.0, 22.0)));
    env.note("edit:linked_sel", r.rect);
    if r.on_hover_text(tr("editing.linked_sel.tip")).clicked() {
        env.ed.edit.linked_sel = !linked;
    }
    bui.separator();
    if env.icon_button(&mut bui, IconId::ZoomOut, "editing.zoom_out", false, true, "edit:zoom_out").clicked() {
        env.ed.edit.zoom = Some((ppf / 1.5).max(0.05));
    }
    if env.icon_button(&mut bui, IconId::Fit, "editing.fit", env.ed.edit.zoom.is_none(), true, "edit:fit").clicked() {
        env.ed.edit.zoom = None;
        env.ed.edit.scroll = 0.0;
    }
    if env.icon_button(&mut bui, IconId::ToolZoom, "editing.zoom_in", false, true, "edit:zoom_in").clicked() {
        env.ed.edit.zoom = Some((ppf * 1.5).min(60.0));
    }
    let _ = fit;
    bui.separator();
    for (kind, key, name) in [(TrackKind::Video, "editing.add_track_v", "edit:add_track_v"), (TrackKind::Audio, "editing.add_track_a", "edit:add_track_a"), (TrackKind::Subtitle, "editing.add_track_s", "edit:add_track_s")] {
        let r = bui.add(egui::Button::new(egui::RichText::new(tr(key)).size(11.5)));
        env.note(name, r.rect);
        if r.on_hover_text(tr("editing.add_track.tip")).clicked() {
            let mut d = (*o.doc).clone();
            d.add_track(kind);
            edit::save(env, &o.id, &d);
        }
    }
    bui.separator();
    bui.label(egui::RichText::new(format!("{} {} · {}", o.doc.items.iter().filter(|i| i.source.track_kind() != TrackKind::Subtitle).count(), tr("editing.clips"), edit::timecode(total, fps))).size(11.5).color(t.text_dim));
}

// ------------------------------------------------------------------------------------------------ atalhos

/// Atalhos da Edição (chamado antes dos atalhos do editor, só neste workspace).
pub(crate) fn shortcuts(ctx: &egui::Context, env: &mut Env) {
    if ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) {
        return;
    }
    use egui::{Key as K, Modifiers as M};
    let Some(o) = sync(env) else { return };
    let fps = env.core.project.settings.fps();
    let last = o.tl.as_ref().map(|t| t.total.saturating_sub(1)).unwrap_or(0);
    // lê as teclas (consome só as que usa)
    struct Keys {
        tool: Option<EditTool>,
        space: bool,
        j: bool,
        k: bool,
        l: bool,
        step: i64,
        home: bool,
        end: bool,
        up: bool,
        down: bool,
        mark_in: bool,
        mark_out: bool,
        insert: bool,
        overwrite: bool,
        add_edit: bool,
        lift: bool,
        ripple_delete: bool,
        q: bool,
        w: bool,
        default_tr: bool,
        link: bool,
        snap: bool,
        zoom: i32,
        fit: bool,
        esc: bool,
        caption: bool,
    }
    let k = ctx.input_mut(|i| {
        let plain = |i: &mut egui::InputState, key: K| crate::keymap::Chord::plain(key).take(i);
        let mut tool = None;
        for tl in EditTool::ALL {
            if plain(i, tl.shortcut()) {
                tool = Some(tl);
            }
        }
        let shift_r = crate::keymap::Chord::shift(K::ArrowRight).take(i);
        let shift_l = crate::keymap::Chord::shift(K::ArrowLeft).take(i);
        let step = if shift_r { 5 } else if shift_l { -5 } else { plain(i, K::ArrowRight) as i64 - plain(i, K::ArrowLeft) as i64 };
        Keys {
            tool,
            space: plain(i, K::Space),
            j: plain(i, K::J),
            k: plain(i, K::K),
            l: plain(i, K::L),
            step,
            home: plain(i, K::Home),
            end: plain(i, K::End),
            up: plain(i, K::ArrowUp),
            down: plain(i, K::ArrowDown),
            mark_in: plain(i, K::I),
            mark_out: plain(i, K::O),
            insert: plain(i, K::Comma),
            overwrite: plain(i, K::Period),
            add_edit: i.consume_key(M::COMMAND, K::K),
            ripple_delete: crate::keymap::Chord::shift(K::Delete).take(i) || plain(i, K::Backspace),
            lift: plain(i, K::Delete),
            q: plain(i, K::Q),
            w: plain(i, K::W),
            default_tr: i.consume_key(M::COMMAND, K::D),
            link: i.consume_key(M::COMMAND, K::L),
            snap: plain(i, K::S),
            zoom: plain(i, K::Equals) as i32 + plain(i, K::Plus) as i32 - plain(i, K::Minus) as i32,
            fit: plain(i, K::Backslash),
            esc: plain(i, K::Escape),
            caption: plain(i, K::T),
        }
    });
    let st = &mut env.ed.edit;
    if let Some(tl) = k.tool {
        st.tool = tl;
    }
    if k.snap {
        st.snap = !st.snap;
    }
    if k.zoom != 0 {
        let cur = st.zoom.unwrap_or(2.0);
        st.zoom = Some(if k.zoom > 0 { (cur * 1.5).min(60.0) } else { (cur / 1.5).max(0.05) });
    }
    if k.fit {
        st.zoom = None;
        st.scroll = 0.0;
    }
    if k.esc {
        st.sel.clear();
        st.sel_tr = None;
        st.tool = EditTool::Select;
    }
    let source = st.focus == Focus::Source && st.source.src.is_some();
    // reprodução: espaço alterna; L acelera para frente, J para trás, K para
    if k.space {
        if source {
            super::edit_panels::toggle_source_play(env, 1.0);
        } else {
            edit::toggle_program_play(env, &o, 1.0);
        }
    }
    if k.k {
        env.ed.edit.stop_play();
        env.ed.edit.stop_source();
    }
    if k.l || k.j {
        let cur = if source { env.ed.edit.source.play.as_ref().map(|p| p.rate) } else { env.ed.edit.play.as_ref().map(|p| p.rate) };
        let rate = match (k.l, cur) {
            (true, Some(r)) if r > 0.0 => (r * 2.0).min(8.0),
            (true, _) => 1.0,
            (false, Some(r)) if r < 0.0 => (r * 2.0).max(-8.0),
            (false, _) => -1.0,
        };
        if source {
            env.ed.edit.stop_source();
            super::edit_panels::toggle_source_play(env, rate);
        } else {
            env.ed.edit.stop_play();
            edit::toggle_program_play(env, &o, rate);
        }
    }
    let st = &mut env.ed.edit;
    if k.step != 0 || k.home || k.end || k.up || k.down {
        if source {
            st.stop_source();
            let len = st.source.src.as_ref().and_then(|s| aiai_tools::edit::media_frames(&env.core.project, s)).unwrap_or(1);
            let f = &mut st.source.frame;
            *f = if k.home { 0 } else if k.end { len.saturating_sub(1) } else { (*f as i64 + k.step).clamp(0, len.saturating_sub(1) as i64) as u32 };
        } else {
            st.stop_play();
            let f = st.frame;
            st.frame = if k.home {
                0
            } else if k.end {
                last
            } else if k.up || k.down {
                // pontos de corte das trilhas de vídeo e áudio
                let mut pts: Vec<u32> = o.doc.items.iter().filter(|i| i.source.track_kind() != TrackKind::Subtitle).flat_map(|i| [i.start, i.end()]).collect();
                pts.push(0);
                pts.sort_unstable();
                pts.dedup();
                if k.up { pts.iter().rev().find(|p| **p < f).copied().unwrap_or(0) } else { pts.iter().find(|p| **p > f).copied().unwrap_or(last) }
            } else {
                (f as i64 + k.step).clamp(0, last as i64) as u32
            };
        }
    }
    if k.mark_in || k.mark_out {
        if st.source.src.is_some() && st.focus == Focus::Source {
            let f = st.source.frame;
            if k.mark_in {
                st.source.mark_in = Some(f);
                if st.source.mark_out.is_some_and(|o| o <= f) {
                    st.source.mark_out = None;
                }
            } else {
                st.source.mark_out = Some(f + 1);
                if st.source.mark_in.is_some_and(|i| i > f) {
                    st.source.mark_in = None;
                }
            }
        }
    }
    if k.insert || k.overwrite {
        super::edit_panels::place_from_source(env, k.overwrite);
    }
    let f = env.ed.edit.frame;
    if k.add_edit {
        edit::modify(env, |d, _| {
            let ids: Vec<u32> = d.items.iter().filter(|i| i.source.track_kind() != TrackKind::Subtitle && i.start < f && f < i.end()).map(|i| i.id).collect();
            !d.razor(f, Some(&ids)).is_empty()
        });
    }
    if k.lift || k.ripple_delete {
        let st = &env.ed.edit;
        if let Some((iid, edge)) = st.sel_tr {
            edit::modify(env, |d, _| d.set_transition(iid, edge, None));
            env.ed.edit.sel_tr = None;
        } else if !st.sel.is_empty() {
            let ids = st.sel.clone();
            let ripple = k.ripple_delete;
            edit::modify(env, |d, _| {
                d.delete_items(&ids, ripple);
                true
            });
            env.ed.edit.sel.clear();
        }
    }
    if k.q || k.w {
        // aparar com ripple o clipe sob o cabeçote na trilha de destino de vídeo: Q do início até o cabeçote, W do cabeçote ao fim
        let vt = env.ed.edit.target_v;
        let linked = env.ed.edit.linked_sel;
        let mut moved_to = None;
        edit::modify(env, |d, p| {
            let Some(it) = d.items.iter().find(|i| Some(i.track) == vt && i.start < f && f < i.end()).cloned() else { return false };
            let ids = if linked { d.linked(it.id) } else { vec![it.id] };
            let media = edit::media_of(p);
            let got = if k.q { d.trim(&ids, Edge::In, (f - it.start) as i64, true, &media) } else { d.trim(&ids, Edge::Out, -((it.end() - f) as i64), true, &media) };
            if k.q {
                moved_to = Some(it.start);
            }
            got != 0
        });
        // Q: o cabeçote vai para o ponto de corte (o resto da edição puxou para lá)
        if let Some(s) = moved_to {
            env.ed.edit.frame = s;
        }
    }
    if k.default_tr {
        let vt = env.ed.edit.target_v;
        let trn = edit::new_transition(env, env.ed.edit.tr_default);
        let tol = (fps * 0.5) as u32;
        let mut target = None;
        edit::modify(env, |d, _| {
            let Some(track) = vt else { return false };
            let Some((a, b, _)) = d.edit_point_near(track, f, tol) else { return false };
            let (id, edge) = match (a, b) {
                (_, Some(b)) => (b, Edge::In),
                (Some(a), None) => (a, Edge::Out),
                _ => return false,
            };
            target = Some((id, edge));
            d.set_transition(id, edge, Some(trn.clone()))
        });
        if target.is_some() {
            env.ed.edit.sel_tr = target;
        }
    }
    if k.link {
        let sel = env.ed.edit.sel.clone();
        if !sel.is_empty() {
            edit::modify(env, |d, _| {
                if sel.iter().any(|i| d.item(*i).is_some_and(|x| x.link.is_some())) {
                    d.unlink(&sel);
                } else {
                    d.link(&sel);
                }
                true
            });
        }
    }
    if k.caption {
        super::edit_panels::add_caption_at_playhead(env);
    }
}
