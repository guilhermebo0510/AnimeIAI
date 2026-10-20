//! Painel Linha do tempo: régua de quadros com cabeçote (clicar/arrastar rola o quadro), uma linha por camada com os blocos
//! de exposição do timesheet (número do cel), clique na célula = expõe o cel escolhido naquele quadro (cria a trilha e o
//! cel se faltarem; Shift = quadro vazio; Ctrl/Alt = segurar), menu de contexto, laço, duração e reprodução na taxa do
//! projeto (os quadros são renderizados à frente em segundo plano; os que não dão tempo são pulados, e o fps real aparece).

use crate::app::commands::{self as cmds, Exposure, LayerRow, NodeId};
use crate::app::editor::*;
use crate::app::panels::commit_on_release;
use crate::app::panels::layers::kind_icon;
use crate::app::ui_kit::{Env, body_ui, paint_icon};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::*;
use egui::{Align2, Color32, FontId, Id, Rect, Sense, Stroke, Ui};
use std::time::Instant;

pub(crate) const HEADER_W: f32 = 150.0;
pub(crate) const RULER_H: f32 = 22.0;
pub(crate) const ROW_H: f32 = 22.0;

/// Blocos de exposição de uma trilha: (quadro inicial, quadro final exclusivo, o que aparece). Entradas repetidas que não
/// mudam nada se fundem; o último bloco vai até o fim do corte.
pub(crate) fn exposure_runs(track: &Track, duration: u32) -> Vec<(u32, u32, CellRef)> {
    let mut runs: Vec<(u32, u32, CellRef)> = Vec::new();
    for (&f, &c) in track.cells.range(..duration) {
        match runs.last_mut() {
            Some(last) if last.2 == c => {}
            Some(last) => {
                last.1 = f;
                runs.push((f, duration, c));
            }
            None => runs.push((f, duration, c)),
        }
    }
    runs
}

/// Reprodução: avança o quadro pelo relógio (na taxa do projeto) dentro do laço.
pub(crate) fn tick(core: &CoreSession, ed: &mut Editor, ctx: &egui::Context) {
    if !ed.playback.playing {
        return;
    }
    let Some(cut) = ed.cut.clone() else {
        ed.playback.playing = false;
        return;
    };
    let dur = cmds::cut_data(&core.project, &cut).map(|c| c.duration).unwrap_or(1);
    let fps = core.project.settings.fps().max(1.0);
    let (lo, hi) = ed.playback.range(dur);
    let len = (hi - lo + 1) as u64;
    let n = (ed.playback.started.elapsed().as_secs_f64() * fps).floor() as u64;
    let rel = ed.playback.start_pos.clamp(lo, hi).saturating_sub(lo) as u64 + n;
    if !ed.playback.looping && rel >= len {
        ed.frame = hi;
        ed.playback.playing = false;
    } else {
        ed.frame = lo + (rel % len) as u32;
    }
    ctx.request_repaint();
}

pub(crate) fn toggle_play(ed: &mut Editor, dur: u32) {
    let pb = &mut ed.playback;
    if pb.playing {
        pb.playing = false;
        return;
    }
    let (lo, hi) = pb.range(dur);
    pb.start_pos = if ed.frame >= hi && !pb.looping { lo } else { ed.frame.clamp(lo, hi) };
    pb.started = Instant::now();
    pb.playing = true;
    pb.shown_times.clear();
    pb.fps_real = 0.0;
    pb.last_shown = None;
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 4.0);
    let ui = &mut child;
    let Some(cut) = env.ed.active_cut(env.core) else {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(tr("timeline.no_cut")).color(t.text_dim));
        return;
    };
    let Some(cd) = cmds::cut_data(&env.core.project, &cut).cloned() else { return };
    let dur = cd.duration;
    let fps = env.core.project.settings.fps();

    // ---- barra de transporte
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 3.0);
        if env.icon_button(ui, IconId::StepBack, "timeline.step_back", false, true, "timeline:step_back").clicked() {
            env.ed.playback.playing = false;
            env.ed.frame = env.ed.frame.saturating_sub(1);
        }
        let playing = env.ed.playback.playing;
        let play = env.icon_button(ui, if playing { IconId::Pause } else { IconId::Play }, if playing { "timeline.pause" } else { "timeline.play" }, playing, true, "timeline:play");
        if play.clicked() {
            toggle_play(env.ed, dur);
        }
        if env.icon_button(ui, IconId::StepForward, "timeline.step_forward", false, true, "timeline:step_forward").clicked() {
            env.ed.playback.playing = false;
            env.ed.frame = (env.ed.frame + 1).min(dur - 1);
        }
        let looping = env.ed.playback.looping;
        if env.icon_button(ui, IconId::Loop, "timeline.loop", looping, true, "timeline:loop").clicked() {
            env.ed.playback.looping = !looping;
        }
        let mut f1 = env.ed.frame + 1;
        let r = ui.add(egui::DragValue::new(&mut f1).range(1..=dur).speed(0.25).prefix(format!("{} ", tr("timeline.frame"))));
        env.note("timeline:frame", r.rect);
        if r.changed() {
            env.ed.playback.playing = false;
            env.ed.frame = f1 - 1;
        }
        ui.label(egui::RichText::new(format!("/ {dur}")).color(t.text_dim));
        if let Some(v) = commit_on_release(ui, Id::new("cut_duration"), dur, |ui, v| ui.add(egui::DragValue::new(v).range(1..=36000).speed(0.25).prefix(format!("{} ", tr("timeline.duration"))).suffix(format!(" {}", tr("unit.frames_short"))))) {
            if let Err(e) = cmds::set_cut_props(env.core, &cut, serde_json::json!({ "duration": v })) {
                env.ed.notify(e);
            }
        }
        // laço (1-based; "fim" 0 = até o fim do corte)
        let (lo, hi) = env.ed.playback.range(dur);
        let (mut lin, mut lout) = (lo + 1, hi + 1);
        ui.label(egui::RichText::new(tr("timeline.loop_range")).size(12.0).color(t.text_dim));
        let a = ui.add(egui::DragValue::new(&mut lin).range(1..=dur).speed(0.25));
        let b = ui.add(egui::DragValue::new(&mut lout).range(1..=dur).speed(0.25));
        env.note("timeline:loop_in", a.rect);
        env.note("timeline:loop_out", b.rect);
        if a.changed() {
            env.ed.playback.loop_in = lin - 1;
        }
        if b.changed() {
            env.ed.playback.loop_out = if lout >= dur { 0 } else { lout - 1 };
        }
        let sel = env.ed.cel_number.map(|n| format!("{} {n}", tr("layers.cel"))).unwrap_or_else(|| "-".into());
        ui.label(egui::RichText::new(format!("{} {sel}", tr("timeline.selected_cel"))).size(12.0).color(t.text_dim));
        let pb = &env.ed.playback;
        if pb.playing {
            ui.label(egui::RichText::new(format!("{:.1} / {:.2} fps", pb.fps_real, fps)).monospace().size(12.0).color(t.accent));
        } else {
            ui.label(egui::RichText::new(format!("{fps:.3} fps")).monospace().size(12.0).color(t.text_dim));
        }
    });

    // ---- régua + linhas
    let area = Rect::from_min_max(egui::pos2(ui.min_rect().left(), ui.cursor().top() + 2.0), ui.max_rect().max);
    if area.width() < HEADER_W + 40.0 || area.height() < RULER_H + 10.0 {
        return;
    }
    env.note("timeline:area", area);
    grid(env, ui, area, &cut, &cd);
}

fn grid(env: &mut Env, ui: &mut Ui, area: Rect, cut: &NodeId, cd: &CutData) {
    let t = env.t;
    let dur = cd.duration;
    let fps = env.core.project.settings.fps().round().max(1.0) as u32;
    let rows: Vec<(LayerRow, LayerData, String)> = cmds::layer_rows(&env.core.project, cut)
        .into_iter()
        .filter_map(|r| {
            let l = cmds::layer_data(&env.core.project, &r.id)?.clone();
            if !l.kind.holds_cels() || l.kind == LayerType::Text {
                return None;
            }
            let name = env.core.project.node(&r.id).map(|n| n.name.clone()).unwrap_or_default();
            Some((r, l, name))
        })
        .collect();
    let active = env.ed.active_layer(env.core);
    let cw = env.ed.tl_cell_w.clamp(4.0, 48.0);
    let x0 = area.min.x + HEADER_W;
    let view_w = area.width() - HEADER_W;
    let view_h = area.height() - RULER_H;
    let content_h = rows.len() as f32 * ROW_H;
    let resp = ui.interact(area, ui.id().with("tl_area"), Sense::click_and_drag());
    let hovered = resp.hovered();

    // ---- rolagem e zoom pela roda
    if hovered {
        let (events, mods) = ui.input(|i| (i.events.clone(), i.modifiers));
        for e in events {
            if let egui::Event::MouseWheel { unit, delta, .. } = e {
                let k = match unit {
                    egui::MouseWheelUnit::Line => 40.0,
                    egui::MouseWheelUnit::Page => 400.0,
                    egui::MouseWheelUnit::Point => 1.0,
                };
                if mods.command {
                    let f = 1.12f32.powf(delta.y * k / 40.0);
                    let at = ui.input(|i| i.pointer.hover_pos()).map(|p| p.x).unwrap_or(x0);
                    let frame_under = (at - x0 + env.ed.tl_scroll.x) / cw;
                    let ncw = (cw * f).clamp(4.0, 48.0);
                    env.ed.tl_cell_w = ncw;
                    env.ed.tl_scroll.x = frame_under * ncw - (at - x0);
                } else if mods.shift || delta.x.abs() > delta.y.abs() {
                    env.ed.tl_scroll.x -= (if delta.x.abs() > delta.y.abs() { delta.x } else { delta.y }) * k;
                } else {
                    env.ed.tl_scroll.y -= delta.y * k;
                }
            }
        }
    }
    let cw = env.ed.tl_cell_w.clamp(4.0, 48.0);
    // acompanha o cabeçote quando o quadro muda (reprodução, setas)
    let head_x = env.ed.frame as f32 * cw;
    let last_seen: u32 = ui.data(|d| d.get_temp(Id::new("tl_last_frame"))).unwrap_or(u32::MAX);
    if last_seen != env.ed.frame {
        ui.data_mut(|d| d.insert_temp(Id::new("tl_last_frame"), env.ed.frame));
        if head_x < env.ed.tl_scroll.x {
            env.ed.tl_scroll.x = (head_x - 20.0).max(0.0);
        } else if head_x + cw > env.ed.tl_scroll.x + view_w {
            env.ed.tl_scroll.x = head_x + cw - view_w + 40.0;
        }
    }
    env.ed.tl_scroll.x = env.ed.tl_scroll.x.clamp(0.0, (dur as f32 * cw - view_w + 40.0).max(0.0));
    env.ed.tl_scroll.y = env.ed.tl_scroll.y.clamp(0.0, (content_h - view_h).max(0.0));
    let (sx, sy) = (env.ed.tl_scroll.x, env.ed.tl_scroll.y);

    let frame_at = |x: f32| -> u32 { (((x - x0 + sx) / cw).floor().max(0.0) as u32).min(dur - 1) };
    let row_at = |y: f32| -> Option<usize> {
        let r = ((y - area.min.y - RULER_H + sy) / ROW_H).floor();
        (r >= 0.0 && (r as usize) < rows.len()).then_some(r as usize)
    };

    // ---- interação
    let press = ui.input(|i| i.pointer.press_origin());
    let ptr = resp.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos()));
    let in_cells = |p: egui::Pos2| p.x >= x0 && p.y >= area.min.y + RULER_H;
    let in_ruler = |p: egui::Pos2| p.x >= x0 && p.y < area.min.y + RULER_H && p.y >= area.min.y;
    if resp.dragged_by(egui::PointerButton::Primary) {
        // arrastar a régua ou as células rola o quadro; arrastar o cabeçalho não faz nada
        if let (Some(p), Some(o)) = (ptr, press) {
            if (in_ruler(o) || in_cells(o)) && p.x >= area.min.x {
                env.ed.playback.playing = false;
                env.ed.frame = frame_at(p.x.max(x0));
                if let Some(r) = row_at(o.y) {
                    if in_cells(o) && env.ed.layer.as_ref() != Some(&rows[r].0.id) {
                        env.ed.layer = Some(rows[r].0.id.clone());
                        env.ed.cel_number = None;
                    }
                }
            }
        }
    }
    if resp.secondary_clicked() {
        if let Some(p) = ptr {
            env.ed.tl_ctx_cell = row_at(p.y).filter(|_| in_cells(p)).map(|r| (rows[r].0.id.clone(), frame_at(p.x)));
        }
    }
    if resp.clicked() {
        if let Some(p) = ptr {
            let mods = ui.input(|i| i.modifiers);
            if in_ruler(p) {
                env.ed.playback.playing = false;
                env.ed.frame = frame_at(p.x);
            } else if in_cells(p) {
                if let Some(r) = row_at(p.y) {
                    let lid = rows[r].0.id.clone();
                    let f = frame_at(p.x);
                    let was_active = active.as_ref() == Some(&lid);
                    env.ed.playback.playing = false;
                    env.ed.frame = f;
                    env.ed.layer = Some(lid.clone());
                    if !was_active {
                        env.ed.cel_number = None;
                    } else {
                        let what = if mods.shift {
                            Exposure::Empty
                        } else if mods.command || mods.alt {
                            Exposure::Hold
                        } else {
                            let n = env.ed.cel_number.or_else(|| cmds::exposed_number(&env.core.project, cut, &lid, f)).or_else(|| cmds::layer_cels(&env.core.project, &lid).first().map(|(n, _)| *n)).unwrap_or(1);
                            Exposure::Cel(n)
                        };
                        if let Err(e) = cmds::set_exposure(env.core, cut, &lid, f, what) {
                            env.ed.notify(e);
                        } else if let Exposure::Cel(n) = what {
                            env.ed.cel_number = Some(n);
                        }
                    }
                }
            } else if let Some(r) = row_at(p.y) {
                if env.ed.layer.as_ref() != Some(&rows[r].0.id) {
                    env.ed.cel_number = None;
                }
                env.ed.layer = Some(rows[r].0.id.clone());
            }
        }
    }
    let ctx_cell = env.ed.tl_ctx_cell.clone();
    resp.context_menu(|ui| {
        let Some((lid, f)) = ctx_cell.clone() else {
            ui.label(tr("timeline.ctx.none"));
            return;
        };
        let n = env.ed.cel_number.or_else(|| cmds::exposed_number(&env.core.project, cut, &lid, f)).unwrap_or(1);
        let mut act: Option<Exposure> = None;
        let b = ui.button(format!("{} {n} {} {}", tr("timeline.ctx.expose"), tr("timeline.ctx.at"), f + 1));
        env.note("timeline:ctx.expose", b.rect);
        if b.clicked() {
            act = Some(Exposure::Cel(n));
            ui.close();
        }
        let b = ui.button(tr("timeline.ctx.hold"));
        env.note("timeline:ctx.hold", b.rect);
        if b.clicked() {
            act = Some(Exposure::Hold);
            ui.close();
        }
        let b = ui.button(tr("timeline.ctx.clear"));
        env.note("timeline:ctx.clear", b.rect);
        if b.clicked() {
            act = Some(Exposure::Empty);
            ui.close();
        }
        ui.separator();
        if ui.button(tr("timeline.ctx.loop_in")).clicked() {
            env.ed.playback.loop_in = f;
            ui.close();
        }
        if ui.button(tr("timeline.ctx.loop_out")).clicked() {
            env.ed.playback.loop_out = if f + 1 >= dur { 0 } else { f };
            ui.close();
        }
        if let Some(a) = act {
            env.ed.layer = Some(lid.clone());
            if let Err(e) = cmds::set_exposure(env.core, cut, &lid, f, a) {
                env.ed.notify(e);
            }
        }
    });
    // espaço toca/pausa com o ponteiro sobre a linha do tempo
    if hovered && !ui.ctx().egui_wants_keyboard_input() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
        toggle_play(env.ed, dur);
    }

    // ---- pintura
    let painter = ui.painter().with_clip_rect(area);
    painter.rect_filled(area, 4.0, t.bg_panel);
    // régua
    let ruler = Rect::from_min_size(area.min, egui::vec2(area.width(), RULER_H));
    painter.rect_filled(ruler, 0.0, t.bg_header);
    let f_lo = (sx / cw).floor().max(0.0) as u32;
    let f_hi = (((sx + view_w) / cw).ceil() as u32 + 1).min(dur);
    let label_step = [1u32, 2, 5, 10, 12, 24, 48, 96].into_iter().find(|s| *s as f32 * cw >= 30.0).unwrap_or(96);
    let cells_clip = Rect::from_min_max(egui::pos2(x0, area.min.y), area.max);
    let cp = painter.with_clip_rect(cells_clip);
    let (pb_lo, pb_hi) = env.ed.playback.range(dur);
    if env.ed.playback.looping && (pb_lo > 0 || pb_hi + 1 < dur) {
        let band = Rect::from_min_max(egui::pos2(x0 - sx + pb_lo as f32 * cw, ruler.top()), egui::pos2(x0 - sx + (pb_hi + 1) as f32 * cw, ruler.bottom()));
        cp.rect_filled(band, 0.0, t.accent_dim);
    }
    for f in f_lo..f_hi {
        let x = x0 - sx + f as f32 * cw;
        let major = f % fps == 0;
        cp.vline(x, (ruler.bottom() - if major { 9.0 } else { 5.0 })..=ruler.bottom(), Stroke::new(1.0, t.border));
        if f % label_step == 0 {
            cp.text(egui::pos2(x + 3.0, ruler.top() + 2.0), Align2::LEFT_TOP, (f + 1).to_string(), FontId::proportional(10.5), t.text_dim);
        }
    }
    painter.hline(area.x_range(), ruler.bottom(), Stroke::new(1.0, t.border));

    // linhas
    let rows_clip = Rect::from_min_max(egui::pos2(area.min.x, ruler.bottom()), area.max);
    let rp = painter.with_clip_rect(rows_clip);
    let rcp = painter.with_clip_rect(Rect::from_min_max(egui::pos2(x0, ruler.bottom()), area.max));
    let selected_cel = env.ed.cel_number;
    for (i, (row, l, name)) in rows.iter().enumerate() {
        let y = ruler.bottom() + i as f32 * ROW_H - sy;
        if y + ROW_H < ruler.bottom() || y > area.max.y {
            continue;
        }
        let row_rect = Rect::from_min_max(egui::pos2(area.min.x, y), egui::pos2(area.max.x, y + ROW_H));
        env.note(format!("timeline:row:{i}"), Rect::from_min_max(egui::pos2(x0, y), egui::pos2(area.max.x, y + ROW_H)));
        let is_active = active.as_ref() == Some(&row.id);
        if is_active {
            rp.rect_filled(row_rect, 0.0, t.accent_dim.gamma_multiply(0.45));
        } else if i % 2 == 1 {
            rp.rect_filled(row_rect, 0.0, t.bg_header.gamma_multiply(0.45));
        }
        // quadros: linhas finas e segundos
        for f in f_lo..f_hi {
            let x = x0 - sx + f as f32 * cw;
            if cw >= 9.0 || f % fps == 0 {
                rcp.vline(x, y..=y + ROW_H, Stroke::new(1.0, if f % fps == 0 { t.border } else { t.border.gamma_multiply(0.45) }));
            }
        }
        // blocos de exposição
        let block = |from: u32, to: u32| Rect::from_min_max(egui::pos2(x0 - sx + from as f32 * cw + 1.0, y + 2.0), egui::pos2(x0 - sx + to as f32 * cw - 1.0, y + ROW_H - 2.0));
        let cels = cmds::layer_cels(&env.core.project, &row.id);
        match cd.xsheet.track_for_layer(&row.id) {
            Some(track) => {
                for (a, b, c) in exposure_runs(track, dur) {
                    if b <= f_lo || a >= f_hi {
                        continue;
                    }
                    let r = block(a, b);
                    match c {
                        CellRef::Cel { number } => {
                            let sel = is_active && selected_cel == Some(number);
                            rcp.rect(r, 3.0, if sel { t.accent_dim } else { t.bg_widget }, Stroke::new(1.0, if sel { t.accent } else { t.border }), egui::StrokeKind::Inside);
                            rcp.text(egui::pos2(r.left().max(x0) + 4.0, y + ROW_H / 2.0), Align2::LEFT_CENTER, number.to_string(), FontId::proportional(11.5), t.text);
                        }
                        CellRef::Empty => {
                            rcp.rect(r, 3.0, Color32::TRANSPARENT, Stroke::new(1.0, t.text_dim.gamma_multiply(0.5)), egui::StrokeKind::Inside);
                            let mut x = r.left() - ROW_H;
                            while x < r.right() {
                                let (p0, p1) = (egui::pos2(x, r.bottom()), egui::pos2(x + ROW_H - 4.0, r.top()));
                                rcp.with_clip_rect(r).line_segment([p0, p1], Stroke::new(1.0, t.text_dim.gamma_multiply(0.35)));
                                x += 7.0;
                            }
                        }
                    }
                }
            }
            None => {
                if let Some((n, _)) = cels.first() {
                    let r = block(0, dur);
                    rcp.rect(r, 3.0, t.bg_widget.gamma_multiply(0.6), Stroke::new(1.0, t.border.gamma_multiply(0.7)), egui::StrokeKind::Inside);
                    rcp.text(egui::pos2(r.left().max(x0) + 4.0, y + ROW_H / 2.0), Align2::LEFT_CENTER, format!("{n}  ({})", tr("timeline.static")), FontId::proportional(11.0), t.text_dim);
                }
            }
        }
        // cabeçalho da linha
        let hdr = Rect::from_min_max(egui::pos2(area.min.x, y), egui::pos2(x0, y + ROW_H));
        let hp = rp.with_clip_rect(hdr);
        hp.rect_filled(hdr, 0.0, if is_active { t.accent_dim } else { t.bg_header });
        paint_icon(env.icons, &hp, kind_icon(l.kind), Rect::from_center_size(egui::pos2(hdr.left() + 14.0 + row.depth as f32 * 10.0, y + ROW_H / 2.0), egui::vec2(13.0, 13.0)), if is_active { t.accent } else { t.text_dim }, false);
        hp.text(egui::pos2(hdr.left() + 28.0 + row.depth as f32 * 10.0, y + ROW_H / 2.0), Align2::LEFT_CENTER, name, FontId::proportional(12.0), if l.visible { t.text } else { t.text_dim });
        rp.hline(area.x_range(), y + ROW_H, Stroke::new(1.0, t.border.gamma_multiply(0.6)));
    }
    painter.vline(x0, ruler.top()..=area.max.y, Stroke::new(1.0, t.border));
    // cabeçote
    let hx = x0 - sx + (env.ed.frame as f32 + 0.5) * cw;
    if hx >= x0 && hx <= area.max.x {
        let hl = painter.with_clip_rect(cells_clip);
        hl.rect_filled(Rect::from_min_max(egui::pos2(hx - cw / 2.0, ruler.bottom()), egui::pos2(hx + cw / 2.0, area.max.y)), 0.0, t.accent.gamma_multiply(0.12));
        hl.vline(hx, ruler.top()..=area.max.y, Stroke::new(1.5, t.accent));
        hl.add(egui::Shape::convex_polygon(vec![egui::pos2(hx - 5.0, ruler.top()), egui::pos2(hx + 5.0, ruler.top()), egui::pos2(hx, ruler.top() + 8.0)], t.accent, Stroke::NONE));
    }
    painter.rect_stroke(area, 4.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_merge_repeated_entries_and_extend_to_the_end() {
        let mut t = Track::new(NodeId::from("t"), "A", TrackKind::Drawing, None);
        t.cells.insert(0, CellRef::Cel { number: 1 });
        t.cells.insert(2, CellRef::Cel { number: 1 });
        t.cells.insert(4, CellRef::Cel { number: 2 });
        t.cells.insert(8, CellRef::Empty);
        let runs = exposure_runs(&t, 12);
        assert_eq!(runs, vec![(0, 4, CellRef::Cel { number: 1 }), (4, 8, CellRef::Cel { number: 2 }), (8, 12, CellRef::Empty)]);
        // entradas depois do fim do corte não contam
        t.cells.insert(20, CellRef::Cel { number: 3 });
        assert_eq!(exposure_runs(&t, 12).len(), 3);
        assert!(exposure_runs(&Track::new(NodeId::from("x"), "B", TrackKind::Drawing, None), 12).is_empty());
    }
}
