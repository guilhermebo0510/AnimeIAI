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

    let resized = (rect.size() - env.ed.last_canvas_size).abs().max_elem() > 1.0;
    env.ed.last_canvas_size = rect.size();
    if env.ed.fit_pending || (env.ed.auto_fit && resized) {
        env.ed.fit_pending = false;
        env.ed.auto_fit = true;
        env.ed.view.fit(rect, ppp, canvas, MARGIN);
    }
    update_render(env, &ctx, &cut, canvas);
    super::anim::onion_frame(env, &ctx, &cut, canvas);
    super::text_tool::frame(env, &ctx);

    let resp = ui.interact(rect, ui.id().with("canvas_area"), Sense::click_and_drag());
    let view_before = env.ed.view;
    handle_input(env, ui, rect, &resp, ppp);
    if env.ed.view != view_before {
        env.ed.auto_fit = false;
    }
    let xf = env.ed.view.xf(rect, ppp);

    // a borracha vetorial destaca o que vai sair enquanto o cursor passa
    if matches!(env.ed.gesture, Gesture::Stroke(_)) {
        if let Gesture::Stroke(mut g) = std::mem::replace(&mut env.ed.gesture, Gesture::None) {
            tools::update_eraser_hits(env, &mut g);
            if env.ed.live_stroke.started_for() != Some(g.t0) {
                tools::start_live(env, &ctx, &g, xf.scale);
            }
            tools::feed_live(env, &g);
            env.ed.gesture = Gesture::Stroke(g);
        }
    } else if let Gesture::DragNode { t0, .. } | Gesture::DragHandle { t0, .. } = env.ed.gesture {
        // Editar pontos: a linha de verdade anda junto com o ponto
        if env.ed.live_stroke.started_for() != Some(t0) {
            super::nodes::start_live(env, &ctx, t0, xf.scale);
        }
        super::nodes::feed_live(env);
    } else if env.ed.live_stroke.active() {
        env.ed.live_stroke.release(None); // gesto cancelado
    }
    let shown_counter = env.ed.shown.as_ref().map(|s| s.rendered.key.counter);
    env.ed.live_stroke.poll(&ctx, shown_counter);
    if *env.tool == ToolId::Move {
        tools::move_prefetch(env, &ctx);
    }
    env.ed.move_live.poll(&ctx, shown_counter);

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
                let (d, size) = (env.ed.form_duration, env.ed.form_custom_size.then_some(env.ed.form_size));
                match cmds::quick_start_with_layers(env.core, d, size) {
                    Ok((cut, line)) => {
                        env.ed.set_cut(cut);
                        env.ed.layer = Some(line);
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
        // desenho de um painel do storyboard: o caminho de volta fica à vista
        if env.ed.back_to_board.is_some() && env.ed.back_to_board == env.ed.cut {
            let b = ui.button(tr("canvas.back_to_board")).on_hover_text(tr("canvas.back_to_board.tip"));
            env.note("canvas:back_to_board", b.rect);
            if b.clicked() {
                env.ed.back_to_board = None;
                env.ed.want_workspace = Some(crate::workspace::WorkspaceId::Storyboard);
            }
            ui.separator();
        }
        if env.icon_button(ui, IconId::Fit, "canvas.fit", false, has_cut, "canvas:fit").clicked() {
            env.ed.fit_pending = true;
        }
        if env.icon_button(ui, IconId::OneToOne, "canvas.actual", false, has_cut, "canvas:actual").clicked() {
            if let Some(r) = rect {
                env.ed.view.set_zoom_at(r, ppp, r.center(), 1.0);
                env.ed.auto_fit = false;
            }
        }
        // zoom em % (100% = 1 pixel do desenho por pixel físico)
        let mut pct = env.ed.view.zoom * 100.0;
        let dv = ui.add_enabled(has_cut, egui::DragValue::new(&mut pct).range(MIN_ZOOM * 100.0..=MAX_ZOOM * 100.0).speed(2.0).suffix("%").max_decimals(0));
        env.note("canvas:zoom", dv.rect);
        if dv.changed() {
            if let Some(r) = rect {
                env.ed.view.set_zoom_at(r, ppp, r.center(), pct / 100.0);
                env.ed.auto_fit = false;
            }
        }
        sep(ui, &t);
        if env.icon_button_sized(ui, IconId::RotateView, "canvas.rotate_left", false, has_cut, "canvas:rot_left", 24.0, true).clicked() {
            env.ed.view.rotate_by(-15f32.to_radians());
            env.ed.auto_fit = false;
        }
        if env.icon_button(ui, IconId::RotateView, "canvas.rotate_right", false, has_cut, "canvas:rot_right").clicked() {
            env.ed.view.rotate_by(15f32.to_radians());
            env.ed.auto_fit = false;
        }
        let mut deg = env.ed.view.rotation_deg();
        let r = ui.add_enabled(has_cut, egui::DragValue::new(&mut deg).range(-180.0..=180.0).speed(0.5).suffix("°").max_decimals(0));
        env.note("canvas:rotation", r.rect);
        if r.changed() {
            env.ed.view.rot = deg.to_radians().rem_euclid(std::f32::consts::TAU);
            env.ed.auto_fit = false;
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
        let on = env.ed.anim.onion.on;
        if env.icon_button(ui, IconId::OnionSkin, "canvas.onion", on, has_cut, "canvas:onion").clicked() {
            env.ed.anim.onion.on = !on;
        }
        // réguas: linhas à mostra (Ctrl+;) e encaixe dos traços (Ctrl+Shift+;)
        let (rs, rn) = (env.ed.rulers.show, env.ed.rulers.snap);
        if env.icon_button(ui, IconId::ToolRuler, "canvas.rulers", rs, has_cut, "canvas:rulers").clicked() {
            env.ed.rulers.show = !rs;
        }
        if env.icon_button(ui, IconId::Snap, "canvas.ruler_snap", rn, has_cut, "canvas:ruler_snap").clicked() {
            env.ed.rulers.snap = !rn;
        }
        sep(ui, &t);
        if env.ws == crate::workspace::WorkspaceId::Animacao && has_cut {
            phase_bar(env, ui);
            sep(ui, &t);
        }
        super::panels::properties::tool_options(env, ui, true);
    });
}

/// Animação: a fase do corte (Genga | Douga; troca o layout e o padrão da mesa de luz) e, no douga, o aviso do
/// cel-chave travado com o botão de destravar.
fn phase_bar(env: &mut Env, ui: &mut Ui) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let ph = super::anim::cut_phase(&env.core.project, &cut);
    ui.label(egui::RichText::new(tr("anim.phase")).size(12.0).color(env.t.text_dim));
    for (p, name) in [(CutPhase::Genga, "genga"), (CutPhase::Douga, "douga")] {
        let r = ui.selectable_label(ph == p, tr(super::anim::phase_key(p))).on_hover_text(tr(if p == CutPhase::Genga { "anim.phase.genga_tip" } else { "anim.phase.douga_tip" }));
        env.note(format!("canvas:phase.{name}"), r.rect);
        if r.clicked() {
            super::anim::set_phase(env, &cut, p);
        }
    }
    if let Some(cel) = super::anim::locked_key_cel(env.core, env.ed) {
        let b = ui.button(egui::RichText::new(tr("anim.lock.unlock")).color(env.t.accent)).on_hover_text(tr("anim.lock.tip"));
        env.note("canvas:unlock_key", b.rect);
        if b.clicked() {
            env.ed.anim.unlocked.insert(cel);
        }
    }
}

fn sep(ui: &mut Ui, t: &crate::theme::Tokens) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(1.0, 20.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, t.border));
}

// ------------------------------------------------------------------------------------------------ render e textura

pub(crate) fn checker_colors(dark: bool) -> [Color32; 2] {
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
    env.ed.shift = ui.input(|i| i.modifiers.shift);
    env.ed.alt = ui.input(|i| i.modifiers.alt);
    if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
        env.ed.cursor = Some(p);
    }
    let _ = resp;
    for (idx, ev) in events.iter().enumerate() {
        match ev {
            Event::Touch { force: Some(f), phase, .. } => {
                // o fim do toque chega antes do botão solto: com traço aberto, os pontos de alcance do estabilizador
                // ainda usam a última pressão; volta a 1,0 (mouse) depois de soltar
                let ended = *phase == egui::TouchPhase::End || *phase == egui::TouchPhase::Cancel;
                env.ed.pen_seen = true;
                if !ended {
                    env.ed.pressure = curve(env, *f);
                } else if matches!(env.ed.gesture, Gesture::None) {
                    env.ed.pressure = 1.0;
                }
            }
            Event::PointerMoved(pos) => {
                env.ed.cursor = Some(*pos);
                // a pressão deste ponto vem do toque que chega logo depois (caneta: PointerMoved + Touch)
                let pressure = events[idx + 1..events.len().min(idx + 3)].iter().find_map(|e| match e {
                    Event::Touch { force: Some(f), phase, .. } if *phase != egui::TouchPhase::End => Some(curve(env, *f)),
                    _ => None,
                });
                on_move(env, rect, ppp, *pos, pressure.unwrap_or(env.ed.pressure));
            }
            Event::PointerButton { pos, button, pressed: true, modifiers } if matches!(env.ed.gesture, Gesture::PolyCurve { .. }) && over_canvas(ui, rect, *pos) => {
                // curva por cliques: clique põe ponto; duplo clique ou botão direito termina
                let _ = modifiers;
                let c = env.ed.view.xf(rect, ppp).to_canvas(*pos);
                let mut finish = *button == egui::PointerButton::Secondary;
                if let Gesture::PolyCurve { pts, last_click } = &mut env.ed.gesture {
                    let double = last_click.elapsed().as_millis() < 350 && pts.last().is_some_and(|l| (*l - c).length() * env.ed.view.xf(rect, ppp).scale < 6.0);
                    if *button == egui::PointerButton::Primary && !double {
                        pts.push(c);
                    }
                    *last_click = Instant::now();
                    finish |= double;
                }
                if finish {
                    if let Gesture::PolyCurve { pts, .. } = std::mem::replace(&mut env.ed.gesture, Gesture::None) {
                        tools::commit_vector_shape(env, pts, aiai_core::StrokeGeometry::Spline, false);
                    }
                }
            }
            Event::PointerButton { pos, button, pressed: true, modifiers } => {
                if matches!(env.ed.gesture, Gesture::None) && over_canvas(ui, rect, *pos) {
                    // o 1º ponto usa a pressão do toque que abriu o traço neste quadro, venha antes (egui-winit)
                    // ou depois (botão promovido do Windows, fila da bridge) do botão; sem toque, a última lida
                    if let Some(p) = touch_start_pressure(&events, idx) {
                        env.ed.pressure = curve(env, p);
                    }
                    on_press(env, rect, ppp, *pos, *button, *modifiers, space, r_held);
                }
            }
            Event::PointerButton { pos, button, pressed: false, .. } => {
                on_release(env, rect, ppp, *pos, *button);
                if events.iter().any(|e| matches!(e, Event::Touch { phase: egui::TouchPhase::End | egui::TouchPhase::Cancel, .. })) {
                    env.ed.pressure = 1.0;
                }
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

/// Pressão do `Touch Start` mais próximo do evento `idx` no mesmo quadro.
/// Pressão bruta da caneta → pressão do traço pela curva das Preferências.
fn curve(env: &Env, force: f32) -> f32 {
    let (min, gamma) = env.ed.pressure_curve;
    crate::prefs::pressure_curve(force, min, gamma)
}

fn touch_start_pressure(events: &[Event], idx: usize) -> Option<f32> {
    events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Event::Touch { force: Some(f), phase: egui::TouchPhase::Start, .. } => Some((i.abs_diff(idx), f.clamp(0.02, 1.0))),
            _ => None,
        })
        .min_by_key(|(d, _)| *d)
        .map(|(_, f)| f)
}

#[allow(clippy::too_many_arguments)]
fn on_press(env: &mut Env, rect: Rect, ppp: f32, pos: Pos2, button: egui::PointerButton, mods: egui::Modifiers, space: bool, r_held: bool) {
    let xf = env.ed.view.xf(rect, ppp);
    let tool = *env.tool;
    // douga: o cel-chave com desenho fica travado contra edição acidental (destrava na barra do canvas ou na linha do tempo)
    let edits = matches!(tool, ToolId::Pen | ToolId::Brush | ToolId::Eraser | ToolId::Points | ToolId::Move);
    if edits && matches!(button, egui::PointerButton::Primary) && !space && !r_held && !mods.alt && super::anim::edit_blocked(env) {
        return;
    }
    if tool == ToolId::Points && !space && matches!(button, egui::PointerButton::Primary | egui::PointerButton::Secondary) {
        // editar pontos: o tamanho de 1 px de tela em px do canvas dá a tolerância do clique
        super::nodes::press(env, xf.to_canvas(pos), 1.0 / xf.scale.max(1e-3), button, mods);
        return;
    }
    // Texto: clique cria/edita; o Mover leva a camada de texto ativa
    if button == egui::PointerButton::Primary && !space && !r_held {
        if tool == ToolId::Text {
            super::text_tool::press(env, xf.to_canvas(pos), xf.scale, mods);
            return;
        }
        if tool == ToolId::Move && super::text_tool::move_press(env, xf.to_canvas(pos)) {
            return;
        }
    }
    match button {
        egui::PointerButton::Middle => env.ed.gesture = Gesture::Pan { last: pos, button },
        egui::PointerButton::Primary => {
            let c = xf.to_canvas(pos);
            if space || tool == ToolId::Hand {
                env.ed.gesture = Gesture::Pan { last: pos, button };
            } else if r_held {
                let ctr = rect.center();
                env.ed.gesture = Gesture::Rotate { last_angle: (pos - ctr).angle() };
            } else if mods.alt && !matches!(tool, ToolId::Zoom | ToolId::Ruler) {
                tools::do_eyedrop(env, c);
                env.ed.gesture = Gesture::Eyedrop;
            } else {
                match tool {
                    ToolId::Pen if env.ed.opts.pen_shape == PenShape::Curve => {
                        env.ed.gesture = Gesture::PolyCurve { pts: vec![c], last_click: Instant::now() };
                    }
                    ToolId::Pen if env.ed.opts.pen_shape != PenShape::Freehand => {
                        env.ed.gesture = Gesture::Shape { start: c, cur: c };
                    }
                    ToolId::Pen | ToolId::Brush | ToolId::Eraser => {
                        let pressure = env.ed.pressure;
                        env.ed.gesture = tools::begin_stroke(env, tool, c, pressure, xf.scale);
                    }
                    ToolId::Fill => env.ed.gesture = Gesture::FillClick, // o clique age ao soltar
                    ToolId::Eyedropper => {
                        tools::do_eyedrop(env, c);
                        env.ed.gesture = Gesture::Eyedrop;
                    }
                    ToolId::Lasso if env.ed.opts.select_mode == SelMode::Wand => {
                        tools::wand_select(env, c);
                    }
                    ToolId::Lasso => env.ed.gesture = Gesture::Select { start: c, pts: vec![c], mode: env.ed.opts.select_mode },
                    ToolId::Move => {
                        // alças da seleção: cantos escalam, a de cima gira; fora delas, mover
                        let hit = env.ed.selection.as_ref().filter(|s| !s.whole).and_then(|s| {
                            let (corners, knob, center) = handles(s);
                            let near = |q: Vec2| (xf.to_screen(q) - pos).length() <= 9.0;
                            if near(knob) {
                                let d = c - center;
                                Some(Gesture::RotateSel { start_angle: d.y.atan2(d.x).to_degrees(), start_rot: s.rotate })
                            } else if let Some(h) = s.rect_of().and_then(|r| handle_points(r).iter().position(|q| near(s.map(*q)))) {
                                let _ = (corners, center);
                                Some(Gesture::ScaleSel { handle: h, start_scale: s.scale, start_offset: s.offset, start_rotate: s.rotate })
                            } else {
                                None
                            }
                        });
                        let mut boxpick = false;
                        if hit.is_none() {
                            // camada vetorial: clicar num traço escolhe só ele (Shift soma); arrastar fora de qualquer
                            // traço desenha a caixa de seleção (como o Mover com seleção automática do Photoshop)
                            boxpick = tools::move_pick(env, c, 6.0 / xf.scale.max(1e-3), mods.shift);
                        }
                        if let Some(g) = hit {
                            env.ed.gesture = g;
                        } else if boxpick {
                            env.ed.gesture = Gesture::BoxPick { start: c, cur: c, add: mods.shift };
                        } else if tools::begin_move(env) {
                            env.ed.gesture = Gesture::MoveSel { last: c };
                        }
                    }
                    ToolId::Zoom => {
                        let f = if mods.shift { 0.5 } else { 2.0 };
                        env.ed.view.zoom_at(rect, ppp, pos, f);
                    }
                    ToolId::Ruler => env.ed.gesture = super::rulers::press(env, c, &xf),
                    ToolId::Hand | ToolId::Points | ToolId::Text => {}
                }
            }
        }
        _ => {}
    }
}

fn on_move(env: &mut Env, rect: Rect, ppp: f32, pos: Pos2, pressure: f32) {
    let xf = env.ed.view.xf(rect, ppp);
    let c = xf.to_canvas(pos);
    if super::text_tool::dragging(env) {
        super::text_tool::drag(env, c);
        return;
    }
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
        Gesture::Shape { cur, .. } => *cur = c,
        Gesture::DragNode { .. } | Gesture::DragHandle { .. } => super::nodes::drag(env, c),
        Gesture::PolyCurve { .. } => {}
        Gesture::ScaleSel { handle, start_scale, start_offset, start_rotate } => {
            let (shift, alt) = (env.ed.shift != env.ed.opts.keep_aspect, env.ed.alt);
            if let Some(s) = env.ed.selection.as_mut() {
                scale_drag(s, *handle, *start_scale, *start_offset, *start_rotate, c, shift, alt);
            }
        }
        Gesture::BoxPick { cur, .. } => *cur = c,
        Gesture::RotateSel { start_angle, start_rot } => {
            if let Some(s) = env.ed.selection.as_mut() {
                let d = c - (s.center() + s.offset);
                s.rotate = *start_rot + d.y.atan2(d.x).to_degrees() - *start_angle;
            }
        }
        Gesture::Eyedrop => tools::do_eyedrop(env, c),
        Gesture::FillClick => {}
        Gesture::RulerDrag { .. } => super::rulers::drag_move(env, c),
        Gesture::RulerCreate { cur, .. } => *cur = c,
    }
}

fn on_release(env: &mut Env, rect: Rect, ppp: f32, pos: Pos2, button: egui::PointerButton) {
    let at = Instant::now();
    let xf = env.ed.view.xf(rect, ppp);
    let c = xf.to_canvas(pos);
    if button == egui::PointerButton::Primary && super::text_tool::dragging(env) {
        super::text_tool::release(env, c, xf.scale);
        return;
    }
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
            tools::feed_live(env, &g);
            tools::finish_stroke(env, g, at);
            let counter = env.core.change_counter;
            env.ed.live_stroke.release(Some(counter));
        }
        Gesture::Select { start, mut pts, mode } => {
            if mode != SelMode::Lasso {
                pts.truncate(1);
                pts.push(c);
            }
            tools::finish_select(env, mode, start, pts);
        }
        Gesture::BoxPick { start, add, .. } => {
            // arrasto curto = clique fora: desfaz a escolha; caixa = os traços que ela toca
            if (c - start).length() * xf_scale(env, rect, ppp) < 4.0 {
                if !add {
                    env.ed.selection = None;
                }
            } else {
                tools::box_pick(env, start, c, add);
            }
        }
        Gesture::MoveSel { .. } | Gesture::ScaleSel { .. } | Gesture::RotateSel { .. } => {
            let key = tools::move_key(env);
            let ready = key.as_ref().is_some_and(|k| env.ed.move_live.ready_for(k));
            let aff = env.ed.selection.as_ref().filter(|s| !s.is_identity()).map(|s| s.affine());
            if let Some(aff) = aff {
                let before = env.core.change_counter;
                tools::commit_move(env);
                if ready && env.core.change_counter != before {
                    env.ed.move_live.hold = Some((env.core.change_counter, aff, Instant::now()));
                }
            }
        }
        g @ (Gesture::DragNode { .. } | Gesture::DragHandle { .. }) => {
            super::nodes::release(env, g);
            let counter = env.core.change_counter;
            env.ed.live_stroke.release(Some(counter));
        }
        Gesture::Shape { start, .. } => {
            let c = super::rulers::snap_line_end(env, start, c);
            let (pts, geo, closed) = tools::shape_points(env.ed.opts.pen_shape, start, c, env.ed.shift);
            tools::commit_vector_shape(env, pts, geo, closed);
        }
        g @ Gesture::PolyCurve { .. } => env.ed.gesture = g, // a curva continua até Enter/duplo clique
        Gesture::Eyedrop => tools::do_eyedrop(env, c),
        Gesture::FillClick => tools::do_fill(env, c, at),
        Gesture::RulerDrag { layer, orig, .. } => super::rulers::drag_release(env, layer, *orig),
        Gesture::RulerCreate { start, .. } => super::rulers::create_release(env, start, c, xf.scale),
    }
}

/// Contorno da ponta do pincel em pontos da tela (raio `r`, sem giro), como segmentos: a elipse da ponta redonda
/// (achatamento e ângulo) ou o contorno da forma de cenário, tirado da própria cobertura do motor (quadrados
/// marchantes numa grade, calculado uma vez por forma). `None` = círculo comum.
fn tip_outline(tip: &aiai_paint::brush::BrushTip, two_tone: bool, r: f32) -> Option<Vec<[Vec2; 2]>> {
    use aiai_paint::brush::BrushTip;
    match tip {
        BrushTip::Round { roundness, angle, .. } if *roundness < 0.98 || angle.abs() > 0.5 => {
            let (s, c) = angle.to_radians().sin_cos();
            let n = 48;
            let pt = |k: usize| {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                let (x, y) = (a.cos() * r, a.sin() * r * roundness.clamp(0.05, 1.0));
                Vec2::new(x * c - y * s, x * s + y * c)
            };
            Some((0..n).map(|k| [pt(k), pt(k + 1)]).collect())
        }
        BrushTip::Shape { shape, aspect } => {
            let unit = shape_outline_unit(*shape, two_tone);
            let a = aspect.clamp(0.1, 4.0);
            Some(unit.iter().map(|[p, q]| [Vec2::new(p.x * r * a, p.y * r), Vec2::new(q.x * r * a, q.y * r)]).collect())
        }
        _ => None,
    }
}

/// Contorno (coordenadas da forma, -1..1) da forma de ponta, em cache.
fn shape_outline_unit(shape: aiai_paint::brush::TipShape, two_tone: bool) -> Vec<[Vec2; 2]> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<[Vec2; 2]>>>> = OnceLock::new();
    let key = format!("{shape:?}{two_tone}");
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return v;
    }
    // grade de amostras da cobertura e quadrados marchantes no nível 0,5
    let n = 64usize;
    let (lo, hi) = (-1.25f32, 1.25f32);
    let step = (hi - lo) / n as f32;
    let at = |i: usize, j: usize| Vec2::new(lo + i as f32 * step, lo + j as f32 * step);
    let f: Vec<f32> = (0..=n).flat_map(|j| (0..=n).map(move |i| (i, j))).map(|(i, j)| {
        let p = at(i, j);
        aiai_paint::brush::shape_coverage_tone(shape, p.x, p.y, step, two_tone) - 0.5
    }).collect();
    let v = |i: usize, j: usize| f[j * (n + 1) + i];
    let mut segs = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let c = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            let val = c.map(|(x, y)| v(x, y));
            let mut cross = Vec::new();
            for k in 0..4 {
                let (a, b) = (k, (k + 1) % 4);
                if (val[a] > 0.0) != (val[b] > 0.0) {
                    let t = val[a] / (val[a] - val[b]);
                    let (pa, pb) = (at(c[a].0, c[a].1), at(c[b].0, c[b].1));
                    cross.push(pa + (pb - pa) * t);
                }
            }
            if cross.len() >= 2 {
                segs.push([cross[0], cross[1]]);
            }
            if cross.len() == 4 {
                segs.push([cross[2], cross[3]]);
            }
        }
    }
    if let Ok(mut c) = cache.lock() {
        c.insert(key, segs.clone());
    }
    segs
}

/// Ferramentas cuja mira o canvas desenha (no mesmo quadro do contorno do pincel).
fn own_cursor(tool: ToolId) -> bool {
    matches!(tool, ToolId::Pen | ToolId::Brush | ToolId::Eraser | ToolId::Fill | ToolId::Eyedropper | ToolId::Lasso)
}

fn set_cursor(env: &Env, ctx: &egui::Context, resp: &egui::Response, tool: ToolId) {
    use egui::CursorIcon as C;
    if !resp.hovered() && matches!(env.ed.gesture, Gesture::None) {
        return;
    }
    let icon = match &env.ed.gesture {
        Gesture::Pan { .. } => C::Grabbing,
        Gesture::Rotate { .. } => C::Grabbing,
        Gesture::RulerDrag { .. } => C::Grabbing,
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
                    ToolId::Text => C::Text,
                    ToolId::Ruler if env.ed.rulers.hover.is_some() => C::Grab,
                    // a mira é desenhada pelo canvas junto com o contorno do pincel (paint_canvas): a do Windows anda
                    // na hora e o contorno um quadro depois, e os dois se separavam ao mover rápido
                    t if own_cursor(t) => C::None,
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
    // Mover ao vivo: o que fica abaixo, a parte que fica, a que anda (transformada) e o que fica acima
    let moving = matches!(env.ed.gesture, Gesture::MoveSel { .. } | Gesture::ScaleSel { .. } | Gesture::RotateSel { .. });
    let live_aff = if moving {
        tools::move_key(env).filter(|k| env.ed.move_live.ready_for(k)).and_then(|_| env.ed.selection.as_ref().map(|s| s.affine()))
    } else {
        env.ed.move_live.hold.as_ref().filter(|_| env.ed.move_live.key.as_ref().is_some_and(|k| k.cut == *cut)).map(|h| h.1)
    };
    if let (Some(aff), Some(mt)) = (live_aff, env.ed.move_live.tex.as_ref()) {
        let full = [Pos2::new(0.0, 0.0), Pos2::new(1.0, 0.0), Pos2::new(1.0, 1.0), Pos2::new(0.0, 1.0)];
        let (mw, mh) = (mt.size[0] as f32, mt.size[1] as f32);
        let plain: [Pos2; 4] = std::array::from_fn(|i| xf.to_screen(corners(mw, mh)[i]));
        painter.add(quad(mt.below.id(), plain, full));
        if let Some(st) = &mt.stay {
            painter.add(quad(st.id(), plain, full));
        }
        let moved: [Pos2; 4] = std::array::from_fn(|i| {
            let c = corners(mw, mh)[i];
            let (x, y) = aff.apply(c.x, c.y);
            xf.to_screen(Vec2::new(x, y))
        });
        painter.add(quad(mt.moving.id(), moved, full));
        painter.add(quad(mt.above.id(), plain, full));
    } else if let Some(shown) = &env.ed.shown {
        if shown.rendered.key.cut == *cut {
            let uvm = shown.rendered.uv_max;
            let uv = [Pos2::new(0.0, 0.0), Pos2::new(uvm[0], 0.0), Pos2::new(uvm[0], uvm[1]), Pos2::new(0.0, uvm[1])];
            painter.add(quad(shown.tex.id(), sp, uv));
        }
    }
    // mesa de luz: os desenhos vizinhos, tingidos, por baixo das linhas do desenho atual (só visão)
    super::anim::paint_onion(env.ed, painter, sp, cut);
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

    // réguas da camada de guia (só visão) e, com a ferramenta Régua, as alças
    super::rulers::paint(env, painter, rect, xf, cut, canvas);
    paint_selection(env, painter, xf, cut);
    paint_gestures(env, painter, xf, cut);
    super::text_tool::paint(env, painter, xf, cut);

    // ponta/pincel sob o cursor
    if let Some(p) = env.ed.cursor {
        let ring = matches!(env.ed.gesture, Gesture::None | Gesture::Stroke(_));
        // espaço/R = mão/girar (o set_cursor mostra o ícone do sistema): sem a mira própria
        let nav = matches!(env.ed.gesture, Gesture::Pan { .. } | Gesture::Rotate { .. }) || painter.ctx().input(|i| i.key_down(egui::Key::Space) || i.key_down(egui::Key::R));
        if rect.contains(p) {
            let o = &env.ed.opts;
            let d = match *env.tool {
                ToolId::Pen => Some(o.pen_width),
                ToolId::Brush => Some(o.brush_size),
                ToolId::Eraser => Some(o.eraser_size),
                _ => None,
            };
            if let Some(d) = d.filter(|_| ring && !nav) {
                let r = (d * xf.scale / 2.0).max(1.5);
                // pincel: o contorno da ponta de verdade (forma de cenário ou elipse da ponta redonda)
                let tip = (*env.tool == ToolId::Brush).then(|| tools::stroke_preset(env, false).tip);
                let segs = tip.as_ref().and_then(|t| tip_outline(t, tools::stroke_preset(env, false).two_tone.is_some(), r));
                match segs {
                    Some(segs) => {
                        for [a, b] in &segs {
                            painter.line_segment([p + *a, p + *b], Stroke::new(2.5, Color32::from_black_alpha(150)));
                        }
                        for [a, b] in &segs {
                            painter.line_segment([p + *a, p + *b], Stroke::new(1.0, Color32::from_white_alpha(220)));
                        }
                    }
                    None => {
                        painter.circle_stroke(p, r + 0.5, Stroke::new(1.0, Color32::from_black_alpha(160)));
                        painter.circle_stroke(p, r - 0.5, Stroke::new(1.0, Color32::from_white_alpha(200)));
                    }
                }
            }
            if own_cursor(*env.tool) && !nav {
                // mira fina em duas cores (aparece em fundo claro e escuro), com um vão no meio para ver o ponto
                let (gap, arm) = (2.5, 7.0);
                for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    let (a, b) = (p + egui::vec2(dx, dy) * gap, p + egui::vec2(dx, dy) * (gap + arm));
                    painter.line_segment([a, b], Stroke::new(3.0, Color32::from_black_alpha(150)));
                    painter.line_segment([a, b], Stroke::new(1.0, Color32::WHITE));
                }
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
    // áreas de segurança de TV, dentro do quadro da câmera: ação (93%) e título (90%)
    for (k, key) in [(0.93f32, "canvas.safe_action"), (0.9, "canvas.safe_title")] {
        let mut pts = scaled(k);
        pts.push(pts[0]);
        painter.extend(Shape::dashed_line(&pts, dash, 6.0, 4.0));
        let g = painter.layout_no_wrap(tr(key).to_owned(), egui::FontId::proportional(9.5), t.accent.gamma_multiply(0.8));
        let at = if k > 0.92 { pts[1] + egui::vec2(-g.size().x - 3.0, 2.0) } else { pts[1] + egui::vec2(-g.size().x - 3.0, 13.0) };
        painter.galley(at, g, t.accent);
    }
    let label = format!("{}  {:.0} x {:.0}", tr("canvas.camera_label"), ow, oh);
    let galley = painter.layout_no_wrap(label, egui::FontId::proportional(11.0), t.accent);
    painter.galley(frame[0] + egui::vec2(2.0, -galley.size().y - 3.0), galley, t.accent);
    // o canvas do corte maior que a câmera (fundo para movimento de câmera): o tamanho dele, no canto do canvas
    if canvas.0 as f32 > ow + 0.5 || canvas.1 as f32 > oh + 0.5 {
        let text = format!("{}  {} x {}", tr("canvas.cut_canvas_label"), canvas.0, canvas.1);
        let g = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), t.text_dim);
        let top_left = xf.to_screen(Vec2::new(0.0, 0.0));
        painter.galley(top_left + egui::vec2(2.0, -g.size().y - 3.0), g, t.text_dim);
    }
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

/// Cantos (transformados), alça de rotação e centro (transformado) da seleção, em coordenadas do canvas.
/// Alças da transformação na caixa original (sem a transformação pendente): 0..4 cantos (sup-esq, sup-dir, inf-dir,
/// inf-esq) e 4..8 meios dos lados (cima, direita, baixo, esquerda).
pub(crate) fn handle_points(r: [f32; 4]) -> [Vec2; 8] {
    let (x0, y0, x1, y1) = (r[0], r[1], r[0] + r[2], r[1] + r[3]);
    let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    [Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1), Vec2::new(mx, y0), Vec2::new(x1, my), Vec2::new(mx, y1), Vec2::new(x0, my)]
}

/// Alça oposta (âncora da escala) de cada alça.
fn opposite(h: usize) -> usize {
    [2, 3, 0, 1, 6, 7, 4, 5][h]
}

fn handles(s: &Selection) -> ([Vec2; 4], Vec2, Vec2) {
    let r = s.rect_of().unwrap_or([0.0, 0.0, 0.0, 0.0]);
    let corners = [Vec2::new(r[0], r[1]), Vec2::new(r[0] + r[2], r[1]), Vec2::new(r[0] + r[2], r[1] + r[3]), Vec2::new(r[0], r[1] + r[3])].map(|q| s.map(q));
    let top = s.map(Vec2::new(r[0] + r[2] / 2.0, r[1]));
    let center = s.map(Vec2::new(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0));
    let up = (top - center).normalized();
    let knob = top + up * (r[3].max(r[2]) * 0.08).max(18.0);
    (corners, knob, center)
}

fn selection_outline(sel_mode: SelMode, pts: &[Vec2], off: Vec2, xf: &Xf) -> Vec<Pos2> {
    selection_outline_map(sel_mode, pts, |p| p + off, xf)
}

fn selection_outline_map(sel_mode: SelMode, pts: &[Vec2], map: impl Fn(Vec2) -> Vec2, xf: &Xf) -> Vec<Pos2> {
    let to = |p: Vec2| xf.to_screen(map(p));
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
            if sel_mode == SelMode::Rect || sel_mode == SelMode::Wand {
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
    // traços escolhidos de outra camada (trocou a camada ativa): a escolha não vale mais
    let active = env.ed.active_layer(env.core);
    if env.ed.selection.as_ref().is_some_and(|s| !s.strokes.is_empty() && s.strokes_layer != active) {
        env.ed.selection = None;
    }
    let Some(sel) = env.ed.selection.clone() else { return };
    paint_picked_strokes(env, painter, xf, cut, &sel);
    // varinha: o contorno é a caixa da região
    let outline_pts: Vec<Vec2> = match (sel.mode, sel.rect_of()) {
        (SelMode::Wand, Some(r)) => vec![Vec2::new(r[0], r[1]), Vec2::new(r[0] + r[2], r[1] + r[3])],
        _ => sel.pts.clone(),
    };
    two_tone(painter, selection_outline_map(sel.mode, &outline_pts, |p| sel.map(p), xf), true);
    // alças do Mover (cantos: escala; círculo de cima: rotação)
    if *env.tool == ToolId::Points {
        return;
    }
    if *env.tool == ToolId::Move && !sel.whole {
        let (corners, knob, _) = handles(&sel);
        if let Some(r) = sel.rect_of() {
            for (k, q) in handle_points(r).iter().enumerate() {
                let p = xf.to_screen(sel.map(*q));
                let side = if k < 4 { 9.0 } else { 7.0 };
                painter.rect(Rect::from_center_size(p, egui::vec2(side, side)), 1.0, Color32::WHITE, Stroke::new(1.5, env.t.accent), egui::StrokeKind::Inside);
                env.note(format!("transform:handle:{k}"), Rect::from_center_size(p, egui::vec2(side, side)));
            }
        }
        let top_mid = xf.to_screen((corners[0] + corners[1]) / 2.0);
        let kp = xf.to_screen(knob);
        painter.line_segment([top_mid, kp], Stroke::new(1.0, env.t.accent));
        painter.circle(kp, 5.5, Color32::WHITE, Stroke::new(1.5, env.t.accent));
    }
    // prévia do movimento: os traços que vão junto, transformados — só enquanto as imagens do Mover ao vivo não
    // ficaram prontas (com elas, o conteúdo de verdade já está andando na tela)
    let live_ready = tools::move_key(env).is_some_and(|k| env.ed.move_live.ready_for(&k));
    if !sel.is_identity() && !live_ready {
        let Some(layer) = env.ed.layer.clone() else { return };
        if cmds::layer_data(&env.core.project, &layer).is_none_or(|l| l.kind != LayerType::Vector) {
            return;
        }
        let Some(cel) = cmds::exposed_cel(&env.core.project, cut, &layer, env.ed.frame) else { return };
        let Some(cd) = cmds::cel_data(&env.core.project, &cel) else { return };
        let accent = env.t.accent;
        for s in &cd.strokes {
            let inside = if sel.strokes.is_empty() { aiai_tools::transform::stroke_in_selection(s, |x, y| sel.contains(Vec2::new(x, y))) } else { sel.strokes.contains(&s.id) };
            if !sel.whole && !inside {
                continue;
            }
            let pts: Vec<Pos2> = s.points.iter().map(|p| xf.to_screen(sel.map(Vec2::new(p.x, p.y)))).collect();
            if pts.len() >= 2 {
                painter.add(Shape::line(pts, Stroke::new((s.width * sel.scale.x.abs().max(sel.scale.y.abs()) * xf.scale).max(1.0), accent.gamma_multiply(0.7))));
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

/// Traço de largura variável (raio por ponto, unidades do canvas): segmentos como trapézios + discos nos pontos.
/// Com cor opaca as sobreposições não aparecem; é a forma que o motor vetorial rasteriza (cápsulas).
fn variable_line(painter: &egui::Painter, pts: &[Vec2], radii: &[f32], color: Color32, xf: &Xf) {
    if pts.is_empty() || pts.len() != radii.len() {
        return;
    }
    let mut mesh = egui::Mesh::default();
    let sp: Vec<Pos2> = pts.iter().map(|p| xf.to_screen(*p)).collect();
    let sr: Vec<f32> = radii.iter().map(|r| (r * xf.scale).max(0.5)).collect();
    for i in 0..sp.len().saturating_sub(1) {
        let (a, b) = (sp[i], sp[i + 1]);
        let d = b - a;
        let len = d.length();
        if len < 1e-3 {
            continue;
        }
        let n = egui::vec2(-d.y, d.x) / len;
        let base = mesh.vertices.len() as u32;
        for p in [a + n * sr[i], a - n * sr[i], b - n * sr[i + 1], b + n * sr[i + 1]] {
            mesh.colored_vertex(p, color);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(Shape::mesh(mesh));
    // discos só onde a direção muda (juntas) e nas pontas, para não pesar em traços longos
    let step = (sp.len() / 400).max(1);
    for (i, (p, r)) in sp.iter().zip(&sr).enumerate() {
        if i == 0 || i + 1 == sp.len() || i % step == 0 {
            painter.circle_filled(*p, *r, color);
        }
    }
}

fn paint_gestures(env: &mut Env, painter: &egui::Painter, xf: &Xf, cut: &NodeId) {
    // o que o gesto anterior acabou de fazer, até o render novo chegar
    env.ed.ghosts.retain(|g| g.born.elapsed().as_secs_f32() < 5.0);
    for g in &env.ed.ghosts {
        if g.radii.is_empty() {
            polyline(painter, &g.pts, g.width, g.color, xf);
        } else {
            variable_line(painter, &g.pts, &g.radii, g.color, xf);
        }
    }
    // resultado real do motor (pincel/borracha) durante o arrasto e até o render do traço gravado chegar
    let live = env.ed.live_stroke.overlay.as_ref().map(|ov| {
        let (x0, y0) = (ov.origin[0] as f32, ov.origin[1] as f32);
        let (x1, y1) = (x0 + ov.size[0] as f32, y0 + ov.size[1] as f32);
        let to = |x: f32, y: f32| {
            let (cx, cy) = match &ov.fwd {
                Some(a) => a.apply(x, y),
                None => (x, y),
            };
            xf.to_screen(Vec2::new(cx, cy))
        };
        let pts = [to(x0, y0), to(x1, y0), to(x1, y1), to(x0, y1)];
        let uv = [Pos2::new(0.0, 0.0), Pos2::new(1.0, 0.0), Pos2::new(1.0, 1.0), Pos2::new(0.0, 1.0)];
        painter.add(quad(ov.tex.id(), pts, uv));
    });
    let has_live = live.is_some();
    if let Gesture::BoxPick { start, cur, .. } = env.ed.gesture {
        let r = Rect::from_two_pos(xf.to_screen(start), xf.to_screen(cur));
        painter.rect_filled(r, 0.0, env.t.accent.gamma_multiply(0.12));
        let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        two_tone(painter, pts, false);
    }
    // pontos do Editar pontos por cima da prévia do traço
    if *env.tool == ToolId::Points {
        super::nodes::paint(env, painter, |p| xf.to_screen(p), env.t.accent);
    }
    let t = env.t;
    match &env.ed.gesture {
        Gesture::Stroke(g) => {
            let pts: Vec<Vec2> = g.samples.iter().map(|s| g.to_canvas(s.p)).collect();
            let k = g.fwd.map(|a| a.0[0].hypot(a.0[3])).unwrap_or(1.0);
            match g.tool {
                ToolId::Pen if has_live => {}
                ToolId::Pen => {
                    // a prévia é o traço que a caneta gravaria agora (ponta, pressão e afinamento do motor)
                    let c = cmds::resolve_color(&env.core.project, &env.ed.color);
                    let color = super::ui_kit::rgba_to_color32(Rgba([c.0[0], c.0[1], c.0[2], 1.0]));
                    match super::tools::pen_preview(env, g) {
                        Some(stroke) => {
                            let (sp, sr) = super::tools::sampled(&stroke);
                            let sp: Vec<Vec2> = sp.into_iter().map(|p| g.to_canvas(p)).collect();
                            let sr: Vec<f32> = sr.into_iter().map(|r| r * k).collect();
                            variable_line(painter, &sp, &sr, color, xf);
                        }
                        None => polyline(painter, &pts, env.ed.opts.pen_width, color, xf),
                    }
                }
                ToolId::Brush if has_live => {}
                ToolId::Brush => {
                    let c = cmds::resolve_color(&env.core.project, &env.ed.color);
                    let a = (env.ed.opts.brush_opacity * 0.85).clamp(0.1, 0.95);
                    polyline(painter, &pts, env.ed.opts.brush_size, super::ui_kit::rgba_to_color32(Rgba([c.0[0], c.0[1], c.0[2], a])), xf);
                }
                ToolId::Eraser if has_live => {}
                _ => {
                    let path: Vec<Vec2> = g.path.iter().map(|p| g.to_canvas(*p)).collect();
                    polyline(painter, &path, env.ed.opts.eraser_size, Color32::from_white_alpha(70), xf);
                    // traços vetoriais que a borracha apagaria agora
                    if let Some(cel) = cmds::exposed_cel(&env.core.project, cut, &g.layer, env.ed.frame).and_then(|c| cmds::cel_data(&env.core.project, &c)) {
                        for s in cel.strokes.iter().filter(|s| g.hits.contains(&s.id)) {
                            let sp: Vec<Vec2> = aiai_paint::sample_stroke(s).iter().map(|q| g.to_canvas(Vec2::new(q.x, q.y))).collect();
                            polyline(painter, &sp, s.width.max(2.0), env.t.danger.gamma_multiply(0.8), xf);
                        }
                    }
                }
            }
        }
        Gesture::Shape { start, cur } => {
            let (start, cur) = (*start, *cur);
            let cur = super::rulers::snap_line_end(env, start, cur);
            let (pts, _, closed) = tools::shape_points(env.ed.opts.pen_shape, start, cur, env.ed.shift);
            let c = cmds::resolve_color(&env.core.project, &env.ed.color);
            let color = super::ui_kit::rgba_to_color32(Rgba([c.0[0], c.0[1], c.0[2], 1.0]));
            let path: Vec<Vec2> = if closed { pts.iter().copied().chain(pts.first().copied()).collect() } else { pts };
            polyline(painter, &path, env.ed.opts.pen_width, color, xf);
        }
        Gesture::PolyCurve { pts, .. } => {
            let c = cmds::resolve_color(&env.core.project, &env.ed.color);
            let color = super::ui_kit::rgba_to_color32(Rgba([c.0[0], c.0[1], c.0[2], 1.0]));
            let mut path = pts.clone();
            if let Some(cur) = env.ed.cursor {
                path.push(xf.to_canvas(cur));
            }
            // prévia pela spline do motor (a mesma do traço gravado)
            let st = aiai_core::Stroke { id: aiai_core::Id(String::new()), points: path.iter().map(|p| aiai_core::StrokePoint { x: p.x, y: p.y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(), geometry: aiai_core::StrokeGeometry::Spline, tension: 0.0,
                tip: aiai_core::VectorTip::Marker, width: env.ed.opts.pen_width.max(0.1), profile: Default::default(), color: env.ed.color.clone(), opacity: 1.0, closed: false, aliased: false, color_trace: false };
            let (sp, sr) = super::tools::sampled(&st);
            variable_line(painter, &sp, &sr, color, xf);
            for p in pts {
                painter.circle(xf.to_screen(*p), 3.5, Color32::WHITE, Stroke::new(1.2, env.t.accent));
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
                painter.text(rect.center_top() + egui::vec2(0.0, 10.0), egui::Align2::CENTER_TOP, msg, egui::FontId::proportional(12.0), env.t.danger);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(phase: egui::TouchPhase, f: f32) -> Event {
        Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(1), phase, pos: Pos2::ZERO, force: Some(f) }
    }

    #[test]
    fn first_point_pressure_comes_from_the_touch_start_of_the_same_frame() {
        let press = Event::PointerButton { pos: Pos2::ZERO, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE };
        // ordem da bridge / botão promovido: botão antes do toque
        let after = [Event::PointerMoved(Pos2::ZERO), press.clone(), touch(egui::TouchPhase::Start, 0.2)];
        assert_eq!(touch_start_pressure(&after, 1), Some(0.2));
        // ordem do egui-winit: toque antes do botão; o fim do traço anterior no mesmo quadro não conta
        let before = [touch(egui::TouchPhase::End, 0.9), touch(egui::TouchPhase::Start, 0.35), Event::PointerMoved(Pos2::ZERO), press];
        assert_eq!(touch_start_pressure(&before, 3), Some(0.35));
        assert_eq!(touch_start_pressure(&[Event::PointerMoved(Pos2::ZERO)], 0), None);
    }
}

/// Traços escolhidos com o Mover: contorno fino na cor de destaque, acompanhando o arrasto.
fn paint_picked_strokes(env: &mut Env, painter: &egui::Painter, xf: &Xf, cut: &NodeId, sel: &Selection) {
    if sel.strokes.is_empty() {
        return;
    }
    let Some(layer) = sel.strokes_layer.clone() else { return };
    let Some(cel) = cmds::exposed_cel(&env.core.project, cut, &layer, env.ed.frame) else { return };
    let fwd = cmds::layer_placement(&env.core.project, cut, &layer, env.ed.frame);
    let Some(data) = cmds::cel_data(&env.core.project, &cel) else { return };
    let accent = env.t.accent;
    for s in data.strokes.iter().filter(|s| sel.strokes.contains(&s.id)) {
        let pts: Vec<Pos2> = aiai_paint::sample_stroke(s)
            .iter()
            .step_by(2)
            .map(|q| {
                let (x, y) = match &fwd {
                    Some(a) => a.apply(q.x, q.y),
                    None => (q.x, q.y),
                };
                xf.to_screen(sel.map(Vec2::new(x, y)))
            })
            .collect();
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(pts, Stroke::new(1.5, accent)));
        }
    }
}

/// Escala da tela (px de tela por px do canvas) para limiares de clique.
fn xf_scale(env: &Env, rect: Rect, ppp: f32) -> f32 {
    env.ed.view.xf(rect, ppp).scale
}

/// Arrasto de uma alça da transformação: a alça oposta fica parada (Alt = o centro); cantos escalam os dois eixos
/// (livre; Shift mantém a proporção), meios dos lados escalam um eixo só. Passar da âncora espelha.
#[allow(clippy::too_many_arguments)]
pub(crate) fn scale_drag(s: &mut Selection, handle: usize, start_scale: Vec2, start_offset: Vec2, start_rotate: f32, cursor: Vec2, shift: bool, alt: bool) {
    let Some(r) = s.rect_of() else { return };
    let pts = handle_points(r);
    let c = Vec2::new(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    let (h, a) = (pts[handle], if alt { c } else { pts[opposite(handle)] });
    // estado do começo do arrasto
    let mut st = s.clone();
    st.scale = start_scale;
    st.offset = start_offset;
    st.rotate = start_rotate;
    let Some(inv) = st.affine().inverse() else { return };
    let (lx, ly) = inv.apply(cursor.x, cursor.y);
    let fac = |l: f32, hh: f32, aa: f32| if (hh - aa).abs() > 1e-3 { (l - aa) / (hh - aa) } else { 1.0 };
    let (mut fx, mut fy) = (fac(lx, h.x, a.x), fac(ly, h.y, a.y));
    match handle {
        4 | 6 => fx = 1.0,
        5 | 7 => fy = 1.0,
        _ => {
            if shift {
                let f = if fx.abs() > fy.abs() { fx } else { fy };
                (fx, fy) = (f.abs() * fx.signum(), f.abs() * fy.signum());
            }
        }
    }
    let lim = |v: f32| if v.abs() < 0.01 { 0.01f32.copysign(if v == 0.0 { 1.0 } else { v }) } else { v.clamp(-50.0, 50.0) };
    let new_scale = Vec2::new(lim(start_scale.x * fx), lim(start_scale.y * fy));
    // a âncora fica onde estava
    let (ax, ay) = st.affine().apply(a.x, a.y);
    let (sn, cs) = start_rotate.to_radians().sin_cos();
    let d = a - c;
    let sd = Vec2::new(d.x * new_scale.x, d.y * new_scale.y);
    let rd = Vec2::new(cs * sd.x - sn * sd.y, sn * sd.x + cs * sd.y);
    s.scale = new_scale;
    s.rotate = start_rotate;
    s.offset = Vec2::new(ax, ay) - c - rd;
}
