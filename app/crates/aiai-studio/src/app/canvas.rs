//! O canvas interativo: o corte ativo renderizado (thread de fundo, `render_service`) como textura, com pan, zoom ancorado no
//! cursor, rotação e espelho da visão, quadro de câmera com área segura, grade de pixels, e as ferramentas (gestos) que
//! viram comandos do documento (`tools`). Toda conversão tela↔desenho passa por `editor::Xf` (considera `pixels_per_point`).

use super::commands::{self as cmds, NodeId};
use super::editor::*;
use super::render_service::{FrameKey, RenderService, pick_reduce};
use super::tools;
use super::ui_kit::{Env, body_ui};
use crate::i18n::tr;
use crate::icons::IconId;
use crate::theme::CARD_RADIUS;
use crate::workspace::ToolId;
use aiai_core::*;
use egui::{Color32, CornerRadius, Event, Mesh, Pos2, Rect, Sense, Shape, Stroke, TextureOptions, Ui, UiBuilder, Vec2, epaint::PathStroke};
use std::time::Instant;

const MARGIN: f32 = 30.0;

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let painter = ui.painter().with_clip_rect(body);
    painter.rect_filled(body, CornerRadius { nw: 0, ne: 0, sw: CARD_RADIUS, se: CARD_RADIUS }, t.canvas_bg);

    // ---- barra de ferramentas (quebra em duas linhas se faltar largura)
    let bg_slot = painter.add(Shape::Noop);
    let tb_max = Rect::from_min_size(body.min, egui::vec2(body.width(), 96.0));
    let mut tb = ui.new_child(UiBuilder::new().max_rect(tb_max.shrink2(egui::vec2(6.0, 3.0))));
    tb.set_clip_rect(tb_max.intersect(body));
    let cut = env.ed.active_cut(env.core);
    toolbar(env, &mut tb, cut.is_some());
    let tb_h = (tb.min_rect().height() + 6.0).max(32.0);
    let bar = Rect::from_min_size(body.min, egui::vec2(body.width(), tb_h));
    painter.set(bg_slot, Shape::rect_filled(bar, 0.0, t.bg_panel));
    painter.hline(bar.x_range(), bar.bottom(), Stroke::new(1.0, t.border));
    let rect = Rect::from_min_max(egui::pos2(body.min.x, bar.bottom() + 1.0), body.max);
    env.note("canvas:toolbar", bar);
    if rect.width() < 60.0 || rect.height() < 40.0 {
        return;
    }

    let Some(cut) = cut else {
        env.ed.canvas_rect = None;
        empty_state(env, ui, rect);
        return;
    };
    env.ed.canvas_rect = Some(rect);
    env.note("canvas:area", rect);
    let canvas = cmds::canvas_size(&env.core.project, &cut);

    if env.ed.fit_pending {
        env.ed.fit_pending = false;
        env.ed.view.fit(rect, ppp, canvas, MARGIN);
    }
    update_render(env, &ctx, &cut, canvas);

    let resp = ui.interact(rect, ui.id().with("canvas_area"), Sense::click_and_drag());
    let xf = env.ed.view.xf(rect, ppp);
    handle_input(env, ui, rect, &resp, ppp);
    let xf = if xf.origin == env.ed.view.xf(rect, ppp).origin { xf } else { env.ed.view.xf(rect, ppp) };

    // a borracha vetorial destaca o que vai sair enquanto o cursor passa
    if matches!(env.ed.gesture, Gesture::Stroke(_)) {
        if let Gesture::Stroke(mut g) = std::mem::replace(&mut env.ed.gesture, Gesture::None) {
            tools::update_eraser_hits(env, &mut g);
            env.ed.gesture = Gesture::Stroke(g);
        }
    }

    let painter = ui.painter().with_clip_rect(rect);
    paint_canvas(env, &painter, rect, &xf, &cut, canvas, ppp);
    set_cursor(env, &ctx, &resp, *env.tool);
    if resp.hovered() {
        if let Some(p) = env.ed.cursor {
            let c = xf.to_canvas(p);
            readout(env, &painter, rect, &xf, c);
        }
    } else {
        readout(env, &painter, rect, &xf, Vec2::new(f32::NAN, f32::NAN));
    }
}

fn empty_state(env: &mut Env, ui: &mut Ui, rect: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, rect, 0.0);
    child.allocate_ui_at_rect_centered(rect, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space((rect.height() * 0.5 - 60.0).max(8.0));
            ui.label(egui::RichText::new(tr("canvas.empty_title")).size(15.0).color(t.text));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(tr("canvas.empty_hint")).size(12.5).color(t.text_dim));
            ui.add_space(10.0);
            let b = ui.button(tr("project.quick_start"));
            env.note("canvas:quick_start", b.rect);
            if b.clicked() {
                let (d, size) = (env.ed.form_duration, None);
                match cmds::quick_start(env.core, d, size) {
                    Ok(cut) => {
                        env.ed.set_cut(cut);
                    }
                    Err(e) => env.ed.notify(e),
                }
            }
        });
    });
}

trait AllocCentered {
    fn allocate_ui_at_rect_centered<R>(&mut self, rect: Rect, f: impl FnOnce(&mut Ui) -> R) -> R;
}

impl AllocCentered for Ui {
    fn allocate_ui_at_rect_centered<R>(&mut self, rect: Rect, f: impl FnOnce(&mut Ui) -> R) -> R {
        let mut child = self.new_child(UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Center)));
        f(&mut child)
    }
}

// ------------------------------------------------------------------------------------------------ barra

fn toolbar(env: &mut Env, ui: &mut Ui, has_cut: bool) {
    let t = env.t;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    ui.spacing_mut().item_spacing = egui::vec2(4.0, 3.0);
    ui.horizontal_wrapped(|ui| {
        let rect = env.ed.canvas_rect;
        if env.icon_button(ui, IconId::Fit, "canvas.fit", false, has_cut, "canvas:fit").clicked() {
            env.ed.fit_pending = true;
        }
        if env.icon_button(ui, IconId::OneToOne, "canvas.actual", false, has_cut, "canvas:actual").clicked() {
            if let Some(r) = rect {
                env.ed.view.set_zoom_at(r, ppp, r.center(), 1.0);
            }
        }
        // zoom em % (100% = 1 pixel do desenho por pixel físico)
        let mut pct = env.ed.view.zoom * 100.0;
        let dv = ui.add_enabled(has_cut, egui::DragValue::new(&mut pct).range(MIN_ZOOM * 100.0..=MAX_ZOOM * 100.0).speed(2.0).suffix("%").max_decimals(0));
        env.note("canvas:zoom", dv.rect);
        if dv.changed() {
            if let Some(r) = rect {
                env.ed.view.set_zoom_at(r, ppp, r.center(), pct / 100.0);
            }
        }
        sep(ui, &t);
        if env.icon_button_sized(ui, IconId::RotateView, "canvas.rotate_left", false, has_cut, "canvas:rot_left", 24.0, true).clicked() {
            env.ed.view.rotate_by(-15f32.to_radians());
        }
        if env.icon_button(ui, IconId::RotateView, "canvas.rotate_right", false, has_cut, "canvas:rot_right").clicked() {
            env.ed.view.rotate_by(15f32.to_radians());
        }
        let mut deg = env.ed.view.rotation_deg();
        let r = ui.add_enabled(has_cut, egui::DragValue::new(&mut deg).range(-180.0..=180.0).speed(0.5).suffix("°").max_decimals(0));
        env.note("canvas:rotation", r.rect);
        if r.changed() {
            env.ed.view.rot = deg.to_radians().rem_euclid(std::f32::consts::TAU);
        }
        let flip = env.ed.view.flip;
        if env.icon_button(ui, IconId::Flip, "canvas.flip", flip, has_cut, "canvas:flip").clicked() {
            env.ed.view.flip = !flip;
        }
        sep(ui, &t);
        let (g, c, gd, rf) = (env.ed.show_grid, env.ed.show_camera, env.ed.show_guides, env.ed.show_reference);
        if env.icon_button(ui, IconId::Grid, "canvas.grid", g, has_cut, "canvas:grid").clicked() {
            env.ed.show_grid = !g;
        }
        if env.icon_button(ui, IconId::CameraFrame, "canvas.camera", c, has_cut, "canvas:camera").clicked() {
            env.ed.show_camera = !c;
        }
        if env.icon_button(ui, IconId::LayerGuide, "canvas.guides", gd, has_cut, "canvas:guides").clicked() {
            env.ed.show_guides = !gd;
        }
        if env.icon_button(ui, IconId::LayerReference, "canvas.reference", rf, has_cut, "canvas:reference").clicked() {
            env.ed.show_reference = !rf;
        }
        sep(ui, &t);
        super::panels::properties::tool_options(env, ui, true);
    });
}

fn sep(ui: &mut Ui, t: &crate::theme::Tokens) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(1.0, 20.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, t.border));
}

// ------------------------------------------------------------------------------------------------ render e textura

fn checker_colors(dark: bool) -> [Color32; 2] {
    if dark { [Color32::from_rgb(0x9c, 0x9e, 0xa5), Color32::from_rgb(0x8d, 0x8f, 0x96)] } else { [Color32::from_rgb(0xe6, 0xe7, 0xea), Color32::from_rgb(0xcf, 0xd0, 0xd5)] }
}

fn update_render(env: &mut Env, ctx: &egui::Context, cut: &NodeId, canvas: (u32, u32)) {
    let core = &*env.core;
    let ed = &mut *env.ed;
    let dark = ctx.theme() == egui::Theme::Dark;
    if ed.checker.is_none() || ed.checker_dark != dark {
        let [a, b] = checker_colors(dark);
        let img = egui::ColorImage::new([2, 2], vec![a, b, b, a]);
        ed.checker = Some(ctx.load_texture("canvas-checker", img, TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Nearest, wrap_mode: egui::TextureWrapMode::Repeat, mipmap_mode: None }));
        ed.checker_dark = dark;
    }
    let svc = ed.render.get_or_insert_with(|| RenderService::new(ctx));
    svc.poll();
    let playing = ed.playback.playing;
    let max_side = ctx.input(|i| i.max_texture_side) as u32;
    let want_reduce = if playing { ((canvas.0 as f32 * canvas.1 as f32 / 700_000.0).sqrt().ceil() as u32).max(1) } else { 1 };
    let reduce = pick_reduce(canvas.0, canvas.1, want_reduce, max_side);
    let key_for = |frame: u32| FrameKey { counter: core.change_counter, cut: cut.clone(), frame, guides: ed.show_guides, reference: ed.show_reference, reduce };
    let want = key_for(ed.frame);
    let dur = cmds::cut_data(&core.project, cut).map(|c| c.duration).unwrap_or(1);
    let show_key = if playing && svc.get(&want).is_none() {
        // reprodução atrasada: mostra o quadro pronto mais próximo ATRÁS do alvo (pula os que não deu tempo de renderizar)
        let (lo, hi) = ed.playback.range(dur);
        let len = hi - lo + 1;
        let best = svc.cached_frames(&want).into_iter().filter(|f| *f >= lo && *f <= hi).min_by_key(|f| (ed.frame + len - *f) % len);
        match best {
            Some(f) if (ed.frame + len - f) % len <= len / 2 => key_for(f),
            _ => want.clone(),
        }
    } else {
        want.clone()
    };
    // pedidos: parado = o quadro atual; tocando = os quadros à frente, descontando o tempo que um render leva
    let mut keys = Vec::new();
    if playing {
        let fps = core.project.settings.fps().max(1.0) as f32;
        let ahead = ((fps * svc.avg_ms / 1000.0).ceil() as u32 + 1).min(32);
        let (lo, hi) = ed.playback.range(dur);
        let len = hi - lo + 1;
        let look = (fps * 0.5).clamp(4.0, 12.0) as u32;
        for k in 0..look {
            let f = lo + (ed.frame - lo.min(ed.frame) + ahead + k) % len;
            keys.push(key_for(f));
        }
    } else {
        keys.push(want.clone());
    }
    svc.request(core, &keys);
    let Some(r) = svc.get(&show_key) else {
        // trocar de corte com a imagem do corte anterior na tela distorce: esconde até a nova chegar
        if ed.shown.as_ref().is_some_and(|s| s.rendered.key.cut != *cut) {
            ed.shown = None;
        }
        return;
    };
    let want_nearest = {
        let texel = ed.view.zoom * r.key.reduce as f32;
        match &ed.shown {
            Some(s) if s.nearest => texel >= 2.4,
            _ => texel >= 3.0,
        }
    };
    let opts = if want_nearest { TextureOptions::NEAREST } else { TextureOptions::LINEAR };
    let changed = ed.shown.as_ref().is_none_or(|s| !std::sync::Arc::ptr_eq(&s.rendered, &r) && s.rendered.key != r.key);
    let mode_changed = ed.shown.as_ref().is_some_and(|s| s.nearest != want_nearest);
    if changed || mode_changed {
        match &mut ed.shown {
            Some(s) => {
                s.tex.set(std::sync::Arc::clone(&r.image), opts);
                s.rendered = std::sync::Arc::clone(&r);
                s.nearest = want_nearest;
            }
            None => {
                let tex = ctx.load_texture("canvas-image", std::sync::Arc::clone(&r.image), opts);
                ed.shown = Some(Shown { tex, rendered: std::sync::Arc::clone(&r), nearest: want_nearest });
            }
        }
    }
    if changed {
        if ed.playback.playing && ed.playback.last_shown != Some(r.key.frame) {
            ed.playback.note_shown();
        }
        ed.playback.last_shown = Some(r.key.frame);
        let counter = r.key.counter;
        ed.ghosts.retain(|g| g.until_counter > counter);
        if let Some(pl) = &ed.pending_latency {
            if counter >= pl.counter {
                let lat = Latency { label: pl.label.clone(), tool_ms: pl.tool_ms, render_ms: r.render_ms, total_ms: pl.release_at.elapsed().as_secs_f32() * 1000.0 };
                tracing::info!("latência {}: ferramenta {:.0} ms + render {:.0} ms = {:.0} ms (solta -> canvas atualizado)", lat.label, lat.tool_ms, lat.render_ms, lat.total_ms);
                ed.latency_log.push(lat.clone());
                if ed.latency_log.len() > 64 {
                    ed.latency_log.remove(0);
                }
                ed.latency = Some(lat);
                ed.pending_latency = None;
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------ entrada

fn over_canvas(ui: &Ui, rect: Rect, pos: Pos2) -> bool {
    rect.contains(pos) && ui.ctx().layer_id_at(pos).is_none_or(|l| l == ui.layer_id())
}

fn handle_input(env: &mut Env, ui: &Ui, rect: Rect, resp: &egui::Response, ppp: f32) {
    let events: Vec<Event> = ui.input(|i| i.events.clone());
    let (space, r_held, any_down) = ui.input(|i| (i.key_down(egui::Key::Space), i.key_down(egui::Key::R), i.pointer.any_down()));
    if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
        env.ed.cursor = Some(p);
    }
    let _ = resp;
    for (idx, ev) in events.iter().enumerate() {
        match ev {
            Event::Touch { force: Some(f), phase, .. } => {
                env.ed.pressure = if *phase == egui::TouchPhase::End || *phase == egui::TouchPhase::Cancel { 1.0 } else { f.clamp(0.02, 1.0) };
            }
            Event::PointerMoved(pos) => {
                env.ed.cursor = Some(*pos);
                // a pressão deste ponto vem do toque que chega logo depois (caneta: PointerMoved + Touch)
                let pressure = events[idx + 1..events.len().min(idx + 3)].iter().find_map(|e| match e {
                    Event::Touch { force: Some(f), phase, .. } if *phase != egui::TouchPhase::End => Some(f.clamp(0.02, 1.0)),
                    _ => None,
                });
                on_move(env, rect, ppp, *pos, pressure.unwrap_or(env.ed.pressure));
            }
            Event::PointerButton { pos, button, pressed: true, modifiers } => {
                if matches!(env.ed.gesture, Gesture::None) && over_canvas(ui, rect, *pos) {
                    on_press(env, rect, ppp, *pos, *button, *modifiers, space, r_held);
                }
            }
            Event::PointerButton { pos, button, pressed: false, .. } => {
                on_release(env, rect, ppp, *pos, *button);
            }
            Event::MouseWheel { unit, delta, .. } => {
                let at = env.ed.cursor.unwrap_or(rect.center());
                if over_canvas(ui, rect, at) {
                    let notches = match unit {
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Page => delta.y * 10.0,
                        egui::MouseWheelUnit::Point => delta.y / 50.0,
                    };
                    env.ed.view.zoom_at(rect, ppp, at, 1.15f32.powf(notches));
                }
            }
            _ => {}
        }
    }
    // soltou fora da janela sem o evento de soltar: termina o gesto
    if !any_down && !matches!(env.ed.gesture, Gesture::None) && !events.iter().any(|e| matches!(e, Event::PointerButton { pressed: true, .. })) {
        let p = env.ed.cursor.unwrap_or(rect.center());
        on_release(env, rect, ppp, p, egui::PointerButton::Primary);
        if !matches!(env.ed.gesture, Gesture::None) {
            on_release(env, rect, ppp, p, egui::PointerButton::Middle);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn on_press(env: &mut Env, rect: Rect, ppp: f32, pos: Pos2, button: egui::PointerButton, mods: egui::Modifiers, space: bool, r_held: bool) {
    let xf = env.ed.view.xf(rect, ppp);
    let tool = *env.tool;
    match button {
        egui::PointerButton::Middle => env.ed.gesture = Gesture::Pan { last: pos, button },
        egui::PointerButton::Primary => {
            let c = xf.to_canvas(pos);
            if space || tool == ToolId::Hand {
                env.ed.gesture = Gesture::Pan { last: pos, button };
            } else if r_held {
                let ctr = rect.center();
                env.ed.gesture = Gesture::Rotate { last_angle: (pos - ctr).angle() };
            } else if mods.alt && !matches!(tool, ToolId::Zoom) {
                tools::do_eyedrop(env, c);
                env.ed.gesture = Gesture::Eyedrop;
            } else {
                match tool {
                    ToolId::Pen | ToolId::Brush | ToolId::Eraser => {
                        let pressure = env.ed.pressure;
                        env.ed.gesture = tools::begin_stroke(env, tool, c, pressure, xf.scale);
                    }
                    ToolId::Fill => env.ed.gesture = Gesture::FillClick, // o clique age ao soltar
                    ToolId::Eyedropper => {
                        tools::do_eyedrop(env, c);
                        env.ed.gesture = Gesture::Eyedrop;
                    }
                    ToolId::Lasso => env.ed.gesture = Gesture::Select { start: c, pts: vec![c], mode: env.ed.opts.select_mode },
                    ToolId::Move => {
                        if tools::begin_move(env) {
                            env.ed.gesture = Gesture::MoveSel { last: c };
                        }
                    }
                    ToolId::Zoom => {
                        let f = if mods.shift { 0.5 } else { 2.0 };
                        env.ed.view.zoom_at(rect, ppp, pos, f);
                    }
                    ToolId::Hand => {}
                }
            }
        }
        _ => {}
    }
}

fn on_move(env: &mut Env, rect: Rect, ppp: f32, pos: Pos2, pressure: f32) {
    let xf = env.ed.view.xf(rect, ppp);
    let c = xf.to_canvas(pos);
    match &mut env.ed.gesture {
        Gesture::None => {}
        Gesture::Pan { last, .. } => {
            let d = pos - *last;
            *last = pos;
            env.ed.view.pan_by_screen(rect, ppp, d);
        }
        Gesture::Rotate { last_angle } => {
            let a = (pos - rect.center()).angle();
            let mut d = a - *last_angle;
            if d > std::f32::consts::PI {
                d -= std::f32::consts::TAU;
            } else if d < -std::f32::consts::PI {
                d += std::f32::consts::TAU;
            }
            *last_angle = a;
            env.ed.view.rotate_by(d);
        }
        Gesture::Stroke(g) => g.push(c, pressure),
        Gesture::Select { start, pts, mode } => match mode {
            SelMode::Lasso => {
                if pts.last().is_none_or(|l| (*l - c).length() * xf.scale >= 2.0) {
                    pts.push(c);
                }
            }
            _ => {
                pts.truncate(1);
                pts.push(c);
                let _ = start;
            }
        },
        Gesture::MoveSel { last } => {
            let d = c - *last;
            *last = c;
            if let Some(s) = env.ed.selection.as_mut() {
                s.offset += d;
            }
        }
        Gesture::Eyedrop => tools::do_eyedrop(env, c),
        Gesture::FillClick => {}
    }
}

fn on_release(env: &mut Env, rect: Rect, ppp: f32, pos: Pos2, button: egui::PointerButton) {
    let at = Instant::now();
    let xf = env.ed.view.xf(rect, ppp);
    let c = xf.to_canvas(pos);
    let g = std::mem::replace(&mut env.ed.gesture, Gesture::None);
    match g {
        Gesture::None => {}
        Gesture::Pan { last, button: b } => {
            if b != button {
                env.ed.gesture = Gesture::Pan { last, button: b };
            }
        }
        Gesture::Rotate { .. } => {}
        _ if button != egui::PointerButton::Primary => env.ed.gesture = g,
        Gesture::Stroke(mut g) => {
            g.push(c, env.ed.pressure);
            tools::finish_stroke(env, g, at);
        }
        Gesture::Select { start, mut pts, mode } => {
            if mode != SelMode::Lasso {
                pts.truncate(1);
                pts.push(c);
            }
            tools::finish_select(env, mode, start, pts);
        }
        Gesture::MoveSel { .. } => {}
        Gesture::Eyedrop => tools::do_eyedrop(env, c),
        Gesture::FillClick => tools::do_fill(env, c, at),
    }
}

fn set_cursor(env: &Env, ctx: &egui::Context, resp: &egui::Response, tool: ToolId) {
    use egui::CursorIcon as C;
    if !resp.hovered() && matches!(env.ed.gesture, Gesture::None) {
        return;
    }
    let icon = match &env.ed.gesture {
        Gesture::Pan { .. } => C::Grabbing,
        Gesture::Rotate { .. } => C::Grabbing,
        _ => {
            if ctx.input(|i| i.key_down(egui::Key::Space)) {
                C::Grab
            } else if ctx.input(|i| i.key_down(egui::Key::R)) {
                C::AllScroll
            } else {
                match tool {
                    ToolId::Hand => C::Grab,
                    ToolId::Zoom => C::ZoomIn,
                    ToolId::Move => C::Move,
                    _ => C::Crosshair,
                }
            }
        }
    };
    ctx.set_cursor_icon(icon);
}

// ------------------------------------------------------------------------------------------------ pintura

fn corners(w: f32, h: f32) -> [Vec2; 4] {
    [Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)]
}

fn quad(tex: egui::TextureId, pts: [Pos2; 4], uv: [Pos2; 4]) -> Shape {
    let mut m = Mesh::with_texture(tex);
    for (p, u) in pts.iter().zip(uv.iter()) {
        m.vertices.push(egui::epaint::Vertex { pos: *p, uv: *u, color: Color32::WHITE });
    }
    m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    Shape::mesh(m)
}

fn paint_canvas(env: &mut Env, painter: &egui::Painter, rect: Rect, xf: &Xf, cut: &NodeId, canvas: (u32, u32), ppp: f32) {
    let t = env.t;
    let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
    let cs = corners(cw, ch);
    let sp: [Pos2; 4] = std::array::from_fn(|i| xf.to_screen(cs[i]));

    // xadrez de transparência: células fixas de 8 pontos na tela (giram junto com o desenho)
    if let Some(chk) = &env.ed.checker {
        let k = xf.scale / 16.0;
        let uv: [Pos2; 4] = std::array::from_fn(|i| Pos2::new(cs[i].x * k, cs[i].y * k));
        painter.add(quad(chk.id(), sp, uv));
    }
    if let Some(shown) = &env.ed.shown {
        if shown.rendered.key.cut == *cut {
            let uvm = shown.rendered.uv_max;
            let uv = [Pos2::new(0.0, 0.0), Pos2::new(uvm[0], 0.0), Pos2::new(uvm[0], uvm[1]), Pos2::new(0.0, uvm[1])];
            painter.add(quad(shown.tex.id(), sp, uv));
        }
    }
    painter.add(Shape::closed_line(sp.to_vec(), Stroke::new(1.0, t.canvas_frame_stroke)));

    // grade de pixels em zoom alto (1 pixel do desenho ≥ 8 pixels físicos)
    if env.ed.show_grid || env.ed.view.zoom >= 8.0 {
        paint_grid(painter, rect, xf, env.ed.view.zoom, canvas, env.ed.show_grid);
    }

    // quadro de câmera e área segura
    if env.ed.show_camera {
        if let Some(cd) = cmds::cut_data(&env.core.project, cut).cloned() {
            paint_camera(env, painter, xf, &cd, canvas);
        }
    }

    paint_selection(env, painter, xf, cut);
    paint_gestures(env, painter, xf, cut);

    // ponta/pincel sob o cursor
    if let (Some(p), true) = (env.ed.cursor, matches!(env.ed.gesture, Gesture::None | Gesture::Stroke(_))) {
        if rect.contains(p) {
            let o = &env.ed.opts;
            let d = match *env.tool {
                ToolId::Pen => Some(o.pen_width),
                ToolId::Brush => Some(o.brush_size),
                ToolId::Eraser => Some(o.eraser_size),
                _ => None,
            };
            if let Some(d) = d {
                let r = (d * xf.scale / 2.0).max(1.5);
                painter.circle_stroke(p, r + 0.5, Stroke::new(1.0, Color32::from_black_alpha(160)));
                painter.circle_stroke(p, r - 0.5, Stroke::new(1.0, Color32::from_white_alpha(200)));
            }
        }
    }
    let _ = ppp;
}

fn paint_grid(painter: &egui::Painter, rect: Rect, xf: &Xf, zoom: f32, canvas: (u32, u32), forced: bool) {
    // pixel a pixel só em zoom alto; com a grade ligada em zoom baixo, a cada 64 pixels do desenho
    let step = if zoom >= 8.0 { 1.0 } else if forced { if zoom >= 2.0 { 8.0 } else { 64.0 } } else { return };
    let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
    let ends = [xf.to_canvas(rect.left_top()), xf.to_canvas(rect.right_top()), xf.to_canvas(rect.right_bottom()), xf.to_canvas(rect.left_bottom())];
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for e in ends {
        x0 = x0.min(e.x);
        y0 = y0.min(e.y);
        x1 = x1.max(e.x);
        y1 = y1.max(e.y);
    }
    let (x0, y0, x1, y1) = (x0.max(0.0), y0.max(0.0), x1.min(cw), y1.min(ch));
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let color = Color32::from_white_alpha(if step == 1.0 { 38 } else { 28 });
    let s = Stroke::new(1.0, color);
    let mut n = 0;
    let mut x = (x0 / step).floor() * step;
    while x <= x1 && n < 600 {
        painter.line_segment([xf.to_screen(Vec2::new(x, y0)), xf.to_screen(Vec2::new(x, y1))], s);
        x += step;
        n += 1;
    }
    let mut y = (y0 / step).floor() * step;
    n = 0;
    while y <= y1 && n < 600 {
        painter.line_segment([xf.to_screen(Vec2::new(x0, y)), xf.to_screen(Vec2::new(x1, y))], s);
        y += step;
        n += 1;
    }
}

fn paint_camera(env: &mut Env, painter: &egui::Painter, xf: &Xf, cd: &CutData, canvas: (u32, u32)) {
    let t = env.t;
    let st = aiai_render::compose::CameraState::eval(&cd.camera, env.ed.frame, canvas);
    let s = &env.core.project.settings;
    let (ow, oh) = (s.width as f32, s.height as f32);
    let m = st.affine(0.0, s.width, s.height).0;
    let det = m[0] * m[4] - m[1] * m[3];
    if det.abs() < 1e-9 {
        return;
    }
    // saída → canvas (inversa da afim canvas → saída)
    let inv = |p: Vec2| {
        let (x, y) = (p.x - m[2], p.y - m[5]);
        Vec2::new((m[4] * x - m[1] * y) / det, (-m[3] * x + m[0] * y) / det)
    };
    let scaled = |k: f32| -> Vec<Pos2> {
        let (cx, cy) = (ow / 2.0, oh / 2.0);
        corners(ow, oh).iter().map(|c| xf.to_screen(inv(Vec2::new(cx + (c.x - cx) * k, cy + (c.y - cy) * k)))).collect()
    };
    let frame = scaled(1.0);
    painter.add(Shape::closed_line(frame.clone(), Stroke::new(1.5, t.accent)));
    let dash = Stroke::new(1.0, t.accent.gamma_multiply(0.75));
    for k in [0.93f32, 0.9] {
        let mut pts = scaled(k);
        pts.push(pts[0]);
        painter.extend(Shape::dashed_line(&pts, dash, 6.0, 4.0));
    }
    let label = format!("{}  {:.0} x {:.0}", tr("canvas.camera_label"), ow, oh);
    let galley = painter.layout_no_wrap(label, egui::FontId::proportional(11.0), t.accent);
    painter.galley(frame[0] + egui::vec2(2.0, -galley.size().y - 3.0), galley, t.accent);
}

fn two_tone(painter: &egui::Painter, pts: Vec<Pos2>, closed: bool) {
    if pts.len() < 2 {
        return;
    }
    let mut pts = pts;
    if closed {
        pts.push(pts[0]);
    }
    painter.add(Shape::line(pts.clone(), Stroke::new(2.0, Color32::from_black_alpha(190))));
    painter.extend(Shape::dashed_line(&pts, Stroke::new(1.2, Color32::WHITE), 5.0, 4.0));
}

fn selection_outline(sel_mode: SelMode, pts: &[Vec2], off: Vec2, xf: &Xf) -> Vec<Pos2> {
    let to = |p: Vec2| xf.to_screen(p + off);
    match sel_mode {
        SelMode::Lasso => pts.iter().map(|p| to(*p)).collect(),
        _ => {
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in pts {
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
            if sel_mode == SelMode::Rect {
                vec![to(Vec2::new(x0, y0)), to(Vec2::new(x1, y0)), to(Vec2::new(x1, y1)), to(Vec2::new(x0, y1))]
            } else {
                let (cx, cy, rx, ry) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0, (y1 - y0) / 2.0);
                (0..64).map(|i| {
                    let a = i as f32 / 64.0 * std::f32::consts::TAU;
                    to(Vec2::new(cx + rx * a.cos(), cy + ry * a.sin()))
                }).collect()
            }
        }
    }
}

fn paint_selection(env: &mut Env, painter: &egui::Painter, xf: &Xf, cut: &NodeId) {
    let Some(sel) = env.ed.selection.clone() else { return };
    two_tone(painter, selection_outline(sel.mode, &sel.pts, sel.offset, xf), true);
    // prévia do movimento: os traços que vão junto, deslocados (camada raster mostra só o contorno)
    if sel.offset != Vec2::ZERO {
        let Some(layer) = env.ed.layer.clone() else { return };
        if cmds::layer_data(&env.core.project, &layer).is_none_or(|l| l.kind != LayerType::Vector) {
            return;
        }
        let Some(cel) = cmds::exposed_cel(&env.core.project, cut, &layer, env.ed.frame) else { return };
        let Some(cd) = cmds::cel_data(&env.core.project, &cel) else { return };
        let accent = env.t.accent;
        for s in &cd.strokes {
            if !sel.whole && !sel.contains(stroke_center(s)) {
                continue;
            }
            let pts: Vec<Pos2> = s.points.iter().map(|p| xf.to_screen(Vec2::new(p.x, p.y) + sel.offset)).collect();
            if pts.len() >= 2 {
                painter.add(Shape::line(pts, Stroke::new((s.width * xf.scale).max(1.0), accent.gamma_multiply(0.7))));
            }
        }
    }
}

fn polyline(painter: &egui::Painter, pts: &[Vec2], width_canvas: f32, color: Color32, xf: &Xf) {
    if pts.is_empty() {
        return;
    }
    let w = (width_canvas * xf.scale).max(1.0);
    let sp: Vec<Pos2> = pts.iter().map(|p| xf.to_screen(*p)).collect();
    if sp.len() >= 2 {
        painter.add(Shape::line(sp.clone(), PathStroke::new(w, color)));
    }
    painter.circle_filled(sp[0], w / 2.0, color);
    painter.circle_filled(*sp.last().expect("não vazio"), w / 2.0, color);
}

fn paint_gestures(env: &mut Env, painter: &egui::Painter, xf: &Xf, cut: &NodeId) {
    // o que o gesto anterior acabou de fazer, até o render novo chegar
    for g in &env.ed.ghosts {
        polyline(painter, &g.pts, g.width, g.color, xf);
    }
    let t = env.t;
    match &env.ed.gesture {
        Gesture::Stroke(g) => {
            let pts: Vec<Vec2> = g.samples.iter().map(|s| s.p).collect();
            match g.tool {
                ToolId::Pen => {
                    let c = cmds::resolve_color(&env.core.project, &env.ed.color);
                    polyline(painter, &pts, env.ed.opts.pen_width, super::ui_kit::rgba_to_color32(Rgba([c.0[0], c.0[1], c.0[2], 1.0])), xf);
                }
                ToolId::Brush => {
                    let c = cmds::resolve_color(&env.core.project, &env.ed.color);
                    let a = (env.ed.opts.brush_opacity * 0.85).clamp(0.1, 0.95);
                    polyline(painter, &pts, env.ed.opts.brush_size, super::ui_kit::rgba_to_color32(Rgba([c.0[0], c.0[1], c.0[2], a])), xf);
                }
                _ => {
                    polyline(painter, &g.path, env.ed.opts.eraser_size, Color32::from_white_alpha(70), xf);
                    // traços vetoriais que a borracha apagaria agora
                    if let Some(cel) = cmds::exposed_cel(&env.core.project, cut, &g.layer, env.ed.frame).and_then(|c| cmds::cel_data(&env.core.project, &c)) {
                        for s in cel.strokes.iter().filter(|s| g.hits.contains(&s.id)) {
                            let sp: Vec<Vec2> = aiai_paint::sample_stroke(s).iter().map(|q| Vec2::new(q.x, q.y)).collect();
                            polyline(painter, &sp, s.width.max(2.0), Color32::from_rgba_unmultiplied(235, 70, 60, 200), xf);
                        }
                    }
                }
            }
        }
        Gesture::Select { pts, mode, start } => {
            let o = selection_outline(*mode, pts, Vec2::ZERO, xf);
            let _ = start;
            two_tone(painter, o, *mode != SelMode::Lasso);
        }
        _ => {}
    }
    let _ = t;
}

fn readout(env: &mut Env, painter: &egui::Painter, rect: Rect, xf: &Xf, c: Vec2) {
    let t = env.t;
    let _ = xf;
    let mut s = format!("{:.0}%   {:+.0}°", env.ed.view.zoom * 100.0, env.ed.view.rotation_deg());
    if c.x.is_finite() {
        s = format!("x {:.0}  y {:.0}    {s}", c.x.floor(), c.y.floor());
    }
    if let Some(lat) = &env.ed.latency {
        s = format!("{s}    {:.0} ms", lat.total_ms);
    }
    let galley = painter.layout_no_wrap(s, egui::FontId::proportional(11.5), t.text);
    let pos = egui::pos2(rect.left() + 8.0, rect.bottom() - galley.size().y - 8.0);
    painter.rect_filled(Rect::from_min_size(pos, galley.size()).expand2(egui::vec2(6.0, 3.0)), 3.0, t.bg_header.gamma_multiply(0.88));
    painter.galley(pos, galley, t.text);
    // mensagem de erro do render, se houver
    if let Some(svc) = &env.ed.render {
        if let Some((k, msg)) = &svc.last_error {
            if env.ed.cut.as_ref() == Some(&k.cut) && k.counter == env.core.change_counter {
                painter.text(rect.center_top() + egui::vec2(0.0, 10.0), egui::Align2::CENTER_TOP, msg, egui::FontId::proportional(12.0), Color32::from_rgb(235, 110, 90));
            }
        }
    }
}
