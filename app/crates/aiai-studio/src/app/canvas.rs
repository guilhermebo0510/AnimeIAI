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
use crate::behavior_prefs::{CursorShape, SmallCursor, ToolCursor};
use crate::workspace::ToolId;
use aiai_core::*;
use egui::{Color32, CornerRadius, Event, Mesh, Pos2, Rect, Sense, Shape, Stroke, TextureOptions, Ui, UiBuilder, Vec2, epaint::PathStroke};
use std::time::Instant;

const MARGIN: f32 = 30.0;

/// Altura da barra de ícones do canvas numa linha (S25: ela também é o cabeçalho do painel quando o canvas não está em
/// abas: arrastar, maximizar e soltar ficam nela, sem a linha de título "Canvas").
pub(crate) const TOOLBAR_H: f32 = 32.0;

/// O canvas com a barra de ícones no topo; `reserve` = largura deixada livre no fim da barra (botões do cabeçalho do
/// painel). Embaixo da barra, à direita, a barra de materiais (`materials`), que tira espaço do canvas quando abre.
pub(crate) fn show_with_header(env: &mut Env, ui: &mut Ui, body: Rect, reserve: f32) {
    let t = env.t;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let painter = ui.painter().with_clip_rect(body);
    let top_r = if reserve > 0.0 { CARD_RADIUS } else { 0 };
    painter.rect_filled(body, CornerRadius { nw: top_r, ne: top_r, sw: CARD_RADIUS, se: CARD_RADIUS }, t.canvas_bg);

    // ---- barra de ferramentas (quebra em duas linhas se faltar largura)
    let bg_slot = painter.add(Shape::Noop);
    let tb_max = Rect::from_min_size(body.min, egui::vec2((body.width() - reserve).max(40.0), 96.0));
    let mut tb = ui.new_child(UiBuilder::new().max_rect(tb_max.shrink2(egui::vec2(6.0, 3.0))));
    tb.set_clip_rect(tb_max.intersect(body));
    let cut = env.ed.active_cut(env.core);
    toolbar(env, &mut tb, cut.is_some());
    let tb_h = (tb.min_rect().height() + 6.0).max(TOOLBAR_H);
    let bar = Rect::from_min_size(body.min, egui::vec2(body.width(), tb_h));
    painter.set(bg_slot, Shape::rect_filled(bar, CornerRadius { nw: top_r, ne: top_r, sw: 0, se: 0 }, t.bg_panel));
    painter.hline(bar.x_range(), bar.bottom(), Stroke::new(1.0, t.border));
    let rect = Rect::from_min_max(egui::pos2(body.min.x, bar.bottom() + 1.0), body.max);
    env.note("canvas:toolbar", bar);
    if rect.width() < 60.0 || rect.height() < 40.0 {
        return;
    }
    let rect = super::materials::strip(env, ui, rect);

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
    super::comp_viewer::drop_target(env, ui, &resp);
    super::library::drop_target(env, ui, &resp);
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
    } else if matches!(env.ed.gesture, Gesture::LineFix(_)) {
        super::line_fix::live(env, &ctx, xf.scale);
    } else if env.ed.live_stroke.active() {
        env.ed.live_stroke.release(None); // gesto cancelado
    }
    let shown_counter = env.ed.shown.as_ref().map(|s| s.rendered.key.counter);
    env.ed.live_stroke.poll(&ctx, shown_counter);
    if *env.tool == ToolId::Move && env.ws != crate::workspace::WorkspaceId::Composicao {
        tools::move_prefetch(env, &ctx);
    }
    tools::erase_prefetch(env);
    env.ed.move_live.poll(&ctx, shown_counter);

    let painter = ui.painter().with_clip_rect(rect);
    paint_canvas(env, &painter, rect, &xf, &cut, canvas, ppp);
    // S25: lançador da seleção (some enquanto se desenha) e Delete limpa a seleção
    super::sel_ops::launcher(env, &ctx, &xf, rect);
    super::sel_ops::shortcuts(env, &ctx, rect);
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
        // Composição: viewer (resolução, xadrez, canais, região de interesse) no lugar das opções de pintura
        if env.ws == crate::workspace::WorkspaceId::Composicao {
            super::comp_viewer::toolbar(env, ui, has_cut);
            return;
        }
        if env.icon_button_sized(ui, IconId::RotateView, "canvas.rotate_left", false, has_cut, "canvas:rot_left", 24.0, true).clicked() {
            env.ed.view.rotate_by(-env.ed.beh.prefs.rotate_step_deg.to_radians());
            env.ed.auto_fit = false;
        }
        if env.icon_button(ui, IconId::RotateView, "canvas.rotate_right", false, has_cut, "canvas:rot_right").clicked() {
            env.ed.view.rotate_by(env.ed.beh.prefs.rotate_step_deg.to_radians());
            env.ed.auto_fit = false;
        }
        let rotated = super::panels::navigator::user_rotation_deg(&env.ed.view, env.ed.workbar.flip_v).abs() > 0.05;
        if env.icon_button(ui, IconId::RotateReset, "nav.rot_reset", false, has_cut && rotated, "canvas:rot_reset").clicked() {
            super::panels::navigator::set_user_rotation(&mut env.ed.view, env.ed.workbar.flip_v, 0.0);
        }
        let (fh, fv) = super::panels::navigator::flips(&env.ed.view, env.ed.workbar.flip_v);
        if env.icon_button(ui, IconId::Flip, "canvas.flip", fh, has_cut, "canvas:flip").clicked() {
            super::panels::navigator::toggle_flip_h(&mut env.ed.view);
        }
        if env.icon_button(ui, IconId::FlipV, "nav.flip_v", fv, has_cut, "canvas:flip_v").clicked() {
            super::panels::navigator::toggle_flip_v(&mut env.ed.view, &mut env.ed.workbar.flip_v);
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
        // papel cebola só onde se anima e se pinta quadro a quadro
        if matches!(env.ws, crate::workspace::WorkspaceId::Animacao | crate::workspace::WorkspaceId::Pintura) {
            let on = env.ed.anim.onion.on;
            if env.icon_button(ui, IconId::OnionSkin, "canvas.onion", on, has_cut, "canvas:onion").clicked() {
                env.ed.anim.onion.on = !on;
            }
        }
        // réguas à mostra (Ctrl+;); o encaixe dos traços (Ctrl+Shift+;) está na barra de comandos
        let rs = env.ed.rulers.show;
        if env.icon_button(ui, IconId::ToolRuler, "canvas.rulers", rs, has_cut, "canvas:rulers").clicked() {
            env.ed.rulers.show = !rs;
        }
        if env.ws == crate::workspace::WorkspaceId::Animacao && has_cut {
            sep(ui, &t);
            phase_bar(env, ui);
        }
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
    // xadrez: as cores das Preferências > Tela de pintura (ou as do tema); refaz a textura quando qualquer coisa muda
    if ed.checker.is_none() || ed.checker_dark != dark || ed.beh.checker_key != Some(ed.beh.prefs.checker) {
        let [a, b] = ed.beh.prefs.checker_colors().unwrap_or_else(|| checker_colors(dark));
        ed.beh.checker_key = Some(ed.beh.prefs.checker);
        let img = egui::ColorImage::new([2, 2], vec![a, b, b, a]);
        ed.checker = Some(ctx.load_texture("canvas-checker", img, TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Nearest, wrap_mode: egui::TextureWrapMode::Repeat, mipmap_mode: None }));
        ed.checker_dark = dark;
    }
    let svc = ed.render.get_or_insert_with(|| RenderService::new(ctx));
    svc.poll();
    let playing = ed.playback.playing;
    let max_side = ctx.input(|i| i.max_texture_side) as u32;
    let want_reduce = if playing { ((canvas.0 as f32 * canvas.1 as f32 / 700_000.0).sqrt().ceil() as u32).max(1) } else { 1 };
    let want_reduce = want_reduce.max(super::comp_viewer::reduce(env.ws, &ed.comp));
    let solo = if env.ws == crate::workspace::WorkspaceId::Composicao { super::comp_viewer::solo_ids(&core.project, cut, &ed.comp.solo) } else { Vec::new() };
    let reduce = pick_reduce(canvas.0, canvas.1, want_reduce, max_side);
    let key_for = |frame: u32| FrameKey { counter: core.change_counter, cut: cut.clone(), frame, guides: ed.show_guides, reference: ed.show_reference, reduce, solo: solo.clone() };
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
    env.ed.comp.ctrl = ui.input(|i| i.modifiers.command);
    env.ed.alt = ui.input(|i| i.modifiers.alt);
    if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
        env.ed.cursor = Some(p);
    }
    let _ = resp;
    // laço poligonal aberto e a ferramenta trocou: o laço sai (os cliques voltam para a ferramenta nova)
    if *env.tool != ToolId::Lasso && matches!(env.ed.gesture, Gesture::PolyLasso { .. }) {
        env.ed.gesture = Gesture::None;
    }
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
            Event::PointerButton { pos, button, pressed: true, .. } if matches!(env.ed.gesture, Gesture::PolyLasso { .. }) && over_canvas(ui, rect, *pos) => {
                let xf = env.ed.view.xf(rect, ppp);
                super::selection::poly_click(env, xf.to_canvas(*pos), *button, xf.scale);
            }
            Event::PointerButton { pos, button, pressed: true, modifiers } if matches!(env.ed.gesture, Gesture::PolyCurve { .. }) && over_canvas(ui, rect, *pos) => {
                // curva/linha por cliques: clique põe ponto, no primeiro fecha; duplo clique ou botão direito termina
                let _ = modifiers;
                let xf = env.ed.view.xf(rect, ppp);
                super::pen::press(env, xf.to_canvas(*pos), xf.scale, *button);
            }
            // Figura em vários passos (curva de dois passos, Bézier): o clique continua a forma aberta
            Event::PointerButton { pos, button, pressed: true, .. } if matches!(env.ed.gesture, Gesture::Figure) && over_canvas(ui, rect, *pos) => {
                let xf = env.ed.view.xf(rect, ppp);
                super::figure::press_open(env, xf.to_canvas(*pos), xf.scale, *button);
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
    if !any_down {
        env.ed.beh.size_drag = None;
    }
    if !any_down && env.ed.comp.drag.is_some() {
        super::comp_viewer::release(env);
    }
    if !any_down && super::model3d::dragging(env) {
        super::model3d::release(env);
    }
    if !any_down && !matches!(env.ed.gesture, Gesture::None) && !events.iter().any(|e| matches!(e, Event::PointerButton { pressed: true, .. })) {
        let p = env.ed.cursor.unwrap_or(rect.center());
        on_release(env, rect, ppp, p, egui::PointerButton::Primary);
        if !matches!(env.ed.gesture, Gesture::None) {
            on_release(env, rect, ppp, p, egui::PointerButton::Middle);
        }
    }
}

/// Tamanho da ponta da ferramenta (px do desenho) e os limites dele; `None` nas ferramentas sem ponta.
fn tip_size(env: &Env, tool: ToolId) -> Option<(f32, (f32, f32))> {
    let o = &env.ed.opts;
    match tool {
        ToolId::Pen => Some((o.pen_width, (0.5, 200.0))),
        ToolId::Brush => Some((o.brush_size, (1.0, 600.0))),
        ToolId::Eraser => Some((o.eraser_size, (1.0, 600.0))),
        ToolId::Points if o.fix.tool.is_some_and(|f| f.has_size()) => Some((o.fix.size, (1.0, 500.0))),
        _ => None,
    }
}

fn set_tip_size(env: &mut Env, tool: ToolId, v: f32) {
    let o = &mut env.ed.opts;
    match tool {
        ToolId::Pen => o.pen_width = v,
        ToolId::Brush => o.brush_size = v,
        ToolId::Eraser => o.eraser_size = v,
        ToolId::Points => o.fix.size = v,
        _ => {}
    }
}

/// Pressão do `Touch Start` mais próximo do evento `idx` no mesmo quadro.
/// Pressão bruta da caneta → pressão do traço pela curva das Preferências.
fn curve(env: &Env, force: f32) -> f32 {
    // Preferências > Tablet: com a pressão desligada, a caneta desenha como o mouse (sempre cheia)
    if !env.ed.beh.prefs.pen_pressure {
        return 1.0;
    }
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
    // arrasto mínimo (Preferências > Ferramentas): de onde o botão foi apertado
    env.ed.beh.drag_origin = Some(pos);
    env.ed.beh.drag_armed = false;
    // Ctrl+Alt+arrastar: muda o tamanho da ponta (pincel, caneta, borracha) pelo deslocamento horizontal
    if button == egui::PointerButton::Primary && !space && !r_held && mods.alt && (mods.ctrl || mods.command) && env.ed.beh.prefs.size_drag {
        if let Some((start, _)) = tip_size(env, tool) {
            env.ed.beh.size_drag = Some(crate::behavior_prefs::SizeDrag { origin: pos, start, tool });
            return;
        }
    }
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
    // Composição: as ferramentas agem na camada e na câmera (viewer), não nos pixels
    if button == egui::PointerButton::Primary && !space && !r_held && (super::comp_viewer::handles(env, tool) || env.ed.comp.roi_pick) && super::comp_viewer::press(env, &xf, pos, mods) {
        return;
    }
    // camada 3D: gizmo e câmera da camada (ferramenta 3D; com o Mover, Alt+arrastar orbita)
    if button == egui::PointerButton::Primary && !space && !r_held && super::model3d::press(env, &xf, pos, mods) {
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
            // Mão > Girar (S25): o arrasto gira a visão, como segurar R
            let rotate_tool = tool == ToolId::Hand && env.ed.opts.area.hand_rotate;
            if space || (tool == ToolId::Hand && !rotate_tool) {
                env.ed.gesture = Gesture::Pan { last: pos, button };
            } else if r_held || rotate_tool {
                let ctr = rect.center();
                env.ed.gesture = Gesture::Rotate { last_angle: (pos - ctr).angle() };
            } else if mods.alt && !matches!(tool, ToolId::Zoom | ToolId::Ruler | ToolId::Lasso) {
                tools::do_eyedrop(env, c);
                env.ed.gesture = Gesture::Eyedrop;
            } else {
                match tool {
                    // Figura: formas de arrastar, de cliques, curva de dois passos, Bézier, laço, linhas
                    ToolId::Pen if env.ed.opts.pen_shape != PenShape::Freehand => super::figure::press(env, c),
                    ToolId::Gradient => super::gradient_tool::press(env, c, xf.scale),
                    ToolId::Pen | ToolId::Brush | ToolId::Eraser => {
                        let pressure = env.ed.pressure;
                        env.ed.gesture = tools::begin_stroke(env, tool, c, pressure, xf.scale);
                    }
                    // o clique age ao soltar; cercar e preencher / área não preenchida desenham um caminho
                    ToolId::Fill => env.ed.gesture = super::bucket::press(env, c),
                    ToolId::Eyedropper => {
                        tools::do_eyedrop(env, c);
                        env.ed.gesture = Gesture::Eyedrop;
                    }
                    // Seleção: retângulo/elipse/laço/laço poligonal/varinha/Selecionar traços (Shift soma, Alt subtrai)
                    ToolId::Lasso => super::selection::press(env, c, mods, xf.scale),
                    // Mover > Selecionar camada (S25): o clique escolhe a camada desenhada sob o cursor
                    ToolId::Move if env.ed.opts.area.pick_layer => super::area_tools::pick_layer(env, c),
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
                        // Mover só move: a seleção (área ou traços) ou, sem seleção, a camada inteira. Com "Seleção
                        // automática" (desligada por padrão), o clique num traço escolhe esse traço antes de arrastar.
                        if hit.is_none() && env.ed.opts.move_auto {
                            super::selection::auto_pick(env, c, 6.0 / xf.scale.max(1e-3));
                        }
                        if let Some(g) = hit {
                            env.ed.gesture = g;
                        } else if tools::begin_move(env) {
                            env.ed.gesture = Gesture::MoveSel { last: c };
                        }
                    }
                    ToolId::Zoom => {
                        let f = if mods.shift { 0.5 } else { 2.0 };
                        env.ed.view.zoom_at(rect, ppp, pos, f);
                    }
                    ToolId::Ruler => env.ed.gesture = super::rulers::press(env, c, &xf),
                    // as da Composição agem pelo viewer (`comp_viewer::press`)
                    ToolId::Hand | ToolId::Points | ToolId::Text | ToolId::Rotate | ToolId::Anchor | ToolId::Camera | ToolId::Model3d => {}
                    // as da Modelagem agem no viewport 3D (não há canvas lá)
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn on_move(env: &mut Env, rect: Rect, ppp: f32, pos: Pos2, pressure: f32) {
    let xf = env.ed.view.xf(rect, ppp);
    let c = xf.to_canvas(pos);
    if let Some(sd) = env.ed.beh.size_drag {
        if let Some((_, range)) = tip_size(env, sd.tool) {
            let v = crate::behavior_prefs::size_from_drag(sd.start, pos.x - sd.origin.x, xf.scale, range);
            set_tip_size(env, sd.tool, v);
        }
        return;
    }
    if super::text_tool::dragging(env) {
        super::text_tool::drag(env, c);
        return;
    }
    // arrasto mínimo: os arrastos de objeto só começam depois que o ponteiro anda a distância das Preferências
    let object_drag = match &env.ed.gesture {
        Gesture::Select { tool, .. } => !tool.is_path(),
        Gesture::MoveSel { .. } | Gesture::ScaleSel { .. } | Gesture::RotateSel { .. } | Gesture::StrokePick { .. } | Gesture::Shape { .. } | Gesture::DragNode { .. } | Gesture::DragHandle { .. } | Gesture::RulerDrag { .. } | Gesture::RulerCreate { .. } => true,
        _ => false,
    };
    if object_drag && !env.ed.beh.drag_armed {
        let min = env.ed.beh.prefs.min_drag_px;
        match env.ed.beh.drag_origin {
            Some(o) if !crate::behavior_prefs::drag_armed(o, pos, min) => return,
            _ => env.ed.beh.drag_armed = true,
        }
    }
    if super::model3d::dragging(env) {
        super::model3d::drag(env, &xf, pos);
        return;
    }
    if env.ed.comp.drag.is_some() {
        let mods = egui::Modifiers { shift: env.ed.shift, alt: env.ed.alt, ..Default::default() };
        let ctrl = env.ed.comp.ctrl;
        super::comp_viewer::drag(env, &xf, pos, egui::Modifiers { ctrl, command: ctrl, ..mods });
        return;
    }
    // Figura em vários passos e Degradê
    if super::figure::moved(env, c) || super::gradient_tool::moved(env, c) {
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
        Gesture::Select { start, pts, tool, .. } => match tool {
            t if t.is_path() => {
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
        Gesture::LineFix(_) => super::line_fix::drag(env, c),
        Gesture::PolyCurve { .. } => {}
        Gesture::ScaleSel { handle, start_scale, start_offset, start_rotate } => {
            let (shift, alt) = (env.ed.shift != env.ed.opts.keep_aspect, env.ed.alt);
            if let Some(s) = env.ed.selection.as_mut() {
                scale_drag(s, *handle, *start_scale, *start_offset, *start_rotate, c, shift, alt);
            }
        }
        Gesture::StrokePick { cur, .. } => *cur = c,
        Gesture::PolyLasso { .. } => {}
        Gesture::RotateSel { start_angle, start_rot } => {
            if let Some(s) = env.ed.selection.as_mut() {
                let d = c - (s.center() + s.offset);
                s.rotate = *start_rot + d.y.atan2(d.x).to_degrees() - *start_angle;
            }
        }
        Gesture::Eyedrop => tools::do_eyedrop(env, c),
        Gesture::FillClick => {}
        Gesture::FillPath { pts } => {
            if pts.last().is_none_or(|l| (*l - c).length() * xf.scale >= 2.0) {
                pts.push(c);
            }
        }
        Gesture::RulerDrag { .. } => super::rulers::drag_move(env, c),
        Gesture::RulerCreate { .. } => super::rulers::create_move(env, c),
        Gesture::Figure | Gesture::GradDrag { .. } | Gesture::GradFree { .. } => {}
    }
}

fn on_release(env: &mut Env, rect: Rect, ppp: f32, pos: Pos2, button: egui::PointerButton) {
    let at = Instant::now();
    let xf = env.ed.view.xf(rect, ppp);
    let c = xf.to_canvas(pos);
    if button == egui::PointerButton::Primary && env.ed.beh.size_drag.take().is_some() {
        return;
    }
    if button == egui::PointerButton::Primary && super::text_tool::dragging(env) {
        super::text_tool::release(env, c, xf.scale);
        return;
    }
    if button == egui::PointerButton::Primary && env.ed.comp.drag.is_some() {
        super::comp_viewer::release(env);
        return;
    }
    if button == egui::PointerButton::Primary && super::model3d::dragging(env) {
        super::model3d::release(env);
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
        Gesture::Select { start, mut pts, tool, op } => {
            if !tool.is_path() {
                pts.truncate(1);
                pts.push(c);
            }
            super::selection::finish_select(env, tool, start, pts, op, xf.scale);
        }
        Gesture::StrokePick { start, op, .. } => super::selection::strokes_release(env, start, c, op, xf.scale),
        g @ Gesture::PolyLasso { .. } => env.ed.gesture = g, // o laço poligonal continua até fechar
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
        Gesture::LineFix(g) => {
            super::line_fix::release(env, *g);
            let counter = env.core.change_counter;
            env.ed.live_stroke.release(Some(counter));
        }
        // formas de arrastar da Figura (reta com um clique sem arrasto vira polilinha)
        Gesture::Shape { start, .. } => super::figure::release_shape(env, start, c, xf.scale),
        Gesture::Figure => super::figure::release(env, c, xf.scale),
        Gesture::GradDrag { start, handle, .. } => {
            let end = if env.ed.shift { super::figure::snap_angle(start, c, env.ed.grad_tool.angle_step) } else { c };
            super::gradient_tool::release(env, start, end, handle, xf.scale);
        }
        Gesture::GradFree { .. } => {}
        g @ Gesture::PolyCurve { .. } => env.ed.gesture = g, // a curva continua até Enter/duplo clique
        Gesture::Eyedrop => tools::do_eyedrop(env, c),
        Gesture::FillClick => super::bucket::click(env, c, at),
        Gesture::FillPath { pts } => super::bucket::finish_path(env, pts, at),
        Gesture::RulerDrag { layer, orig, .. } => super::rulers::drag_release(env, layer, *orig),
        Gesture::RulerCreate { start, .. } => super::rulers::create_release(env, start, c, xf.scale),
    }
}

/// Diâmetro na tela abaixo do qual o contorno da ponta some e a mira entra no lugar (px lógicos).
pub(crate) const OUTLINE_MIN_PX: f32 = 7.0;
/// Contorno a partir deste diâmetro ganha uma marca pequena no centro (onde a tinta cai).
pub(crate) const CENTER_MARK_PX: f32 = 24.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CenterMark {
    None,
    /// Mira fina (contorno pequeno demais, ou modo preciso).
    Cross,
    /// Marca pequena no centro de um contorno grande.
    Dot,
    /// Ponto cheio (preferência "Ponto" do cursor).
    Point,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CursorLook {
    pub outline: bool,
    pub center: CenterMark,
}

/// Como o cursor da ponta aparece pelo diâmetro dela na tela (px lógicos): contorno quando dá para ver, mira quando
/// não dá (ou sempre no modo preciso), marca de centro nos contornos grandes. Com as Preferências > Cursor: a forma
/// escolhida para o tipo de ferramenta e o que mostrar quando a ponta fica pequena demais para o contorno (nada, ponto
/// ou mira). Ponta de tamanho 0 (borracha de preenchimento) nunca fica sem mira.
pub(crate) fn cursor_look_with(diameter_px: f32, precise: bool, shape: CursorShape, small: SmallCursor) -> CursorLook {
    match shape {
        CursorShape::Cross => return CursorLook { outline: false, center: CenterMark::Cross },
        CursorShape::Dot => return CursorLook { outline: false, center: CenterMark::Point },
        CursorShape::Tip => {}
    }
    let outline = diameter_px >= OUTLINE_MIN_PX;
    let center = if precise || diameter_px <= 0.0 {
        CenterMark::Cross
    } else if !outline {
        match small {
            SmallCursor::None => CenterMark::None,
            SmallCursor::Dot => CenterMark::Point,
            SmallCursor::Cross => CenterMark::Cross,
        }
    } else if diameter_px >= CENTER_MARK_PX {
        CenterMark::Dot
    } else {
        CenterMark::None
    };
    CursorLook { outline, center }
}

/// Diâmetro da ponta (px do desenho) e o preset dela (para o contorno da forma) da ferramenta de pintura atual.
fn cursor_tip(env: &Env) -> Option<(f32, Option<aiai_paint::brush::BrushPreset>)> {
    let o = &env.ed.opts;
    match *env.tool {
        ToolId::Pen => Some((o.pen_width, None)),
        ToolId::Brush => {
            let sp = tools::stroke_preset(env, false);
            Some((sp.size, Some(sp)))
        }
        ToolId::Eraser => match env.ed.layer.as_ref().and_then(|l| cmds::layer_data(&env.core.project, l)).map(|l| l.kind) {
            // vetor: o raio que toca os traços; preenchimento: tira a região sob o ponto (só a mira)
            Some(LayerType::Vector) => Some((o.eraser_size, None)),
            Some(LayerType::Fill) => Some((0.0, None)),
            _ => {
                let sp = tools::stroke_preset(env, true);
                Some((sp.size, Some(sp)))
            }
        },
        _ => None,
    }
}

/// Caps Lock ligado (o modo preciso do Photoshop).
fn caps_lock() -> bool {
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetKeyState(vk: i32) -> i16;
        }
        // SAFETY: GetKeyState só lê o estado do teclado da thread (VK_CAPITAL = 0x14; bit 0 = ligado)
        unsafe { GetKeyState(0x14) & 1 != 0 }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Linhas em duas cores (escura larga embaixo, clara fina em cima): aparecem sobre desenho claro e escuro.
fn two_tone_segments(painter: &egui::Painter, segs: &[[Pos2; 2]]) {
    for s in segs {
        painter.line_segment(*s, Stroke::new(2.5, Color32::from_black_alpha(150)));
    }
    for s in segs {
        painter.line_segment(*s, Stroke::new(1.0, Color32::from_white_alpha(225)));
    }
}

/// Mira em `p` com vão `gap` e braços `arm` (px lógicos); vão 0 = cruz cheia.
fn crosshair(painter: &egui::Painter, p: Pos2, (gap, arm): (f32, f32)) {
    let segs: Vec<[Pos2; 2]> = [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)].iter().map(|&(dx, dy)| [p + egui::vec2(dx, dy) * gap, p + egui::vec2(dx, dy) * (gap + arm)]).collect();
    for s in &segs {
        painter.line_segment(*s, Stroke::new(3.0, Color32::from_black_alpha(150)));
    }
    for s in &segs {
        painter.line_segment(*s, Stroke::new(1.0, Color32::WHITE));
    }
}

/// Ponto cheio de 2 px de raio com borda escura (aparece sobre desenho claro e escuro).
fn point_mark(painter: &egui::Painter, p: Pos2) {
    painter.circle(p, 2.5, Color32::from_black_alpha(170), Stroke::NONE);
    painter.circle(p, 1.6, Color32::WHITE, Stroke::NONE);
}

/// Contorno da ponta com raio `rc` (px do desenho) em volta de `p`, no giro/espelho da visão: a elipse da ponta
/// redonda, a forma de cenário ou a máscara da ponta de imagem; a ponta redonda comum é um círculo.
fn paint_tip_outline(painter: &egui::Painter, p: Pos2, xf: &Xf, rc: f32, preset: Option<&aiai_paint::brush::BrushPreset>, blobs: &BlobStore) {
    match preset.and_then(|sp| tip_outline_unit(sp, blobs)) {
        Some(segs) => {
            let m = xf.m;
            let map = |v: Vec2| {
                let w = v * rc;
                p + egui::vec2(m[0] * w.x + m[1] * w.y, m[2] * w.x + m[3] * w.y)
            };
            let segs: Vec<[Pos2; 2]> = segs.iter().map(|[a, b]| [map(*a), map(*b)]).collect();
            two_tone_segments(painter, &segs);
        }
        None => {
            let r = rc * xf.scale;
            painter.circle_stroke(p, r + 0.5, Stroke::new(1.5, Color32::from_black_alpha(150)));
            painter.circle_stroke(p, r - 0.5, Stroke::new(1.0, Color32::from_white_alpha(225)));
        }
    }
}

/// Contorno da ponta em raio 1 (px do desenho ÷ raio, sem o giro da visão), como segmentos: a elipse da ponta
/// redonda achatada/girada, ou o contorno da forma/máscara tirado da própria cobertura do motor. `None` = círculo.
fn tip_outline_unit(sp: &aiai_paint::brush::BrushPreset, blobs: &BlobStore) -> Option<Vec<[Vec2; 2]>> {
    use aiai_paint::brush::BrushTip;
    match &sp.tip {
        BrushTip::Round { roundness, angle, .. } if *roundness < 0.98 || angle.abs() > 0.5 => {
            let (s, c) = angle.to_radians().sin_cos();
            let n = 64;
            let pt = |k: usize| {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                let (x, y) = (a.cos(), a.sin() * roundness.clamp(0.05, 1.0));
                Vec2::new(x * c - y * s, x * s + y * c)
            };
            Some((0..n).map(|k| [pt(k), pt(k + 1)]).collect())
        }
        BrushTip::Shape { shape, aspect } => {
            let two = sp.two_tone.is_some();
            let a = aspect.clamp(0.1, 4.0);
            let unit = outline_cached(&format!("{shape:?}{two}"), |u, v| aiai_paint::brush::shape_coverage_tone(*shape, u, v, 0.02, two));
            Some(unit.iter().map(|[p, q]| [Vec2::new(p.x * a, p.y), Vec2::new(q.x * a, q.y)]).collect())
        }
        BrushTip::Image { blob, .. } => {
            let key = format!("img:{blob}");
            if let Some(v) = outline_cache_get(&key) {
                return Some(v);
            }
            let loaded = aiai_tools::paint::load_tip_mask(blobs, sp).ok()?;
            let BrushTip::Image { mask: Some(m), .. } = &loaded.tip else { return None };
            // a máscara ocupa o quadrado do diâmetro: (u, v) ∈ [-1, 1] → pixel da máscara
            Some(outline_cached(&key, |u, v| {
                let (x, y) = ((u * 0.5 + 0.5) * m.width as f32, (v * 0.5 + 0.5) * m.height as f32);
                if x < 0.0 || y < 0.0 || x >= m.width as f32 || y >= m.height as f32 { 0.0 } else { m.get(x as u32, y as u32) }
            }))
        }
        _ => None,
    }
}

type OutlineCache = std::sync::Mutex<std::collections::HashMap<String, Vec<[Vec2; 2]>>>;

fn outline_cache() -> &'static OutlineCache {
    static CACHE: std::sync::OnceLock<OutlineCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn outline_cache_get(key: &str) -> Option<Vec<[Vec2; 2]>> {
    outline_cache().lock().ok().and_then(|c| c.get(key).cloned())
}

/// Contorno no nível 0,5 de `f(u, v)` em [-1,25; 1,25]² (grade 96), em cache por `key`.
fn outline_cached(key: &str, f: impl Fn(f32, f32) -> f32) -> Vec<[Vec2; 2]> {
    if let Some(v) = outline_cache_get(key) {
        return v;
    }
    let segs = marching_squares(f, 96, 1.25);
    if let Ok(mut c) = outline_cache().lock() {
        c.insert(key.to_owned(), segs.clone());
    }
    segs
}

/// Quadrados marchantes no nível 0,5 de `f` numa grade `n`×`n` em [-half, half]².
pub(crate) fn marching_squares(f: impl Fn(f32, f32) -> f32, n: usize, half: f32) -> Vec<[Vec2; 2]> {
    let step = 2.0 * half / n as f32;
    let at = |i: usize, j: usize| Vec2::new(-half + i as f32 * step, -half + j as f32 * step);
    let vals: Vec<f32> = (0..=n)
        .flat_map(|j| (0..=n).map(move |i| (i, j)))
        .map(|(i, j)| {
            let p = at(i, j);
            f(p.x, p.y) - 0.5
        })
        .collect();
    let v = |i: usize, j: usize| vals[j * (n + 1) + i];
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
    segs
}

/// Ferramentas cuja mira o canvas desenha (no mesmo quadro do contorno do pincel).
fn own_cursor(tool: ToolId) -> bool {
    matches!(tool, ToolId::Pen | ToolId::Brush | ToolId::Eraser | ToolId::Fill | ToolId::Eyedropper | ToolId::Lasso)
}

/// Cursor próprio das ferramentas sem ponta (o sistema não tem estes ícones): conta-gotas com a ponta no ponto de
/// leitura, balde com a mira no ponto em que derrama, mira de seleção.
fn tool_glyph(painter: &egui::Painter, p: Pos2, tool: ToolId) {
    let both = |pts: Vec<Pos2>, closed: bool| {
        for s in [Stroke::new(3.5, Color32::from_black_alpha(160)), Stroke::new(1.5, Color32::WHITE)] {
            painter.add(if closed { Shape::closed_line(pts.clone(), s) } else { Shape::line(pts.clone(), s) });
        }
    };
    match tool {
        ToolId::Eyedropper => {
            let d = egui::vec2(1.0, -1.0).normalized();
            let n = egui::vec2(d.y, -d.x);
            both(vec![p, p + d * 9.0], false);
            both(vec![p + d * 9.0 + n * 2.5, p + d * 15.0 + n * 2.5, p + d * 15.0 - n * 2.5, p + d * 9.0 - n * 2.5], true);
            both(vec![p + d * 15.0, p + d * 19.0], false);
        }
        ToolId::Fill => {
            crosshair(painter, p, (2.0, 4.0));
            let o = p + egui::vec2(9.0, -16.0);
            both(vec![o, o + egui::vec2(10.0, 0.0), o + egui::vec2(8.5, 10.0), o + egui::vec2(1.5, 10.0)], true);
            both(vec![o, o + egui::vec2(-3.0, 5.0)], false);
        }
        _ => crosshair(painter, p, (2.5, 7.0)),
    }
}

fn set_cursor(env: &mut Env, ctx: &egui::Context, resp: &egui::Response, tool: ToolId) {
    use egui::CursorIcon as C;
    if !resp.hovered() && matches!(env.ed.gesture, Gesture::None) {
        return;
    }
    // Pan: a mão do sistema (aberta; fechada enquanto arrasta) na ferramenta Pan, com Espaço apertado e no botão do meio.
    // Não é o ícone Grab do egui: o winit no Windows o mostra como a seta de quatro pontas (mover).
    let space = ctx.input(|i| i.key_down(egui::Key::Space));
    let hand = match &env.ed.gesture {
        Gesture::Pan { .. } => Some(true),
        Gesture::None if tool == ToolId::Hand || tool == ToolId::Camera => Some(false),
        _ if space => Some(false),
        _ => None,
    };
    if let Some(grabbing) = hand {
        env.ed.cursor_kind = if grabbing { "grabbing" } else { "hand" }.to_owned();
        return crate::icons::set_hand_cursor(ctx, grabbing);
    }
    let icon = match &env.ed.gesture {
        Gesture::Rotate { .. } => C::Grabbing,
        Gesture::RulerDrag { .. } => C::Grabbing,
        _ => {
            if ctx.input(|i| i.key_down(egui::Key::R)) {
                C::AllScroll
            } else {
                match tool {
                    ToolId::Zoom => C::ZoomIn,
                    ToolId::Move => C::Move,
                    ToolId::Text => C::Text,
                    ToolId::Rotate => C::Alias,
                    ToolId::Ruler if env.ed.rulers.hover.is_some() => C::Grab,
                    // a mira é desenhada pelo canvas junto com o contorno do pincel (paint_canvas): a do Windows anda
                    // na hora e o contorno um quadro depois, e os dois se separavam ao mover rápido
                    t if own_cursor(t) => C::None,
                    _ => C::Crosshair,
                }
            }
        }
    };
    env.ed.cursor_kind = format!("{icon:?}").to_lowercase();
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

    // xadrez de transparência: células fixas de 8 pontos na tela (giram junto com o desenho); o viewer da Composição
    // pode trocar por fundo preto
    let comp_ws = env.ws == crate::workspace::WorkspaceId::Composicao;
    let alpha_tex = if comp_ws && env.ed.comp.alpha_view { super::comp_viewer::alpha_texture(env.ed, painter.ctx()) } else { None };
    if comp_ws && (!env.ed.comp.checker || env.ed.comp.alpha_view) {
        painter.add(Shape::convex_polygon(sp.to_vec(), Color32::BLACK, Stroke::NONE));
    } else if let Some(chk) = &env.ed.checker {
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
    let shown_counter = env.ed.shown.as_ref().map(|s| s.rendered.key.counter);
    if super::model3d::paint_live(env, painter, xf, cut, shown_counter) {
        // camada 3D em arrasto: a prévia reduzida no lugar do quadro
    } else if let (Some(aff), Some(mt)) = (live_aff, env.ed.move_live.tex.as_ref()) {
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
            // canal alfa da Composição: a textura em cinza do mesmo quadro
            painter.add(quad(alpha_tex.unwrap_or(shown.tex.id()), sp, uv));
        }
    }
    // mesa de luz: os desenhos vizinhos, tingidos, por baixo das linhas do desenho atual (só visão)
    super::anim::paint_onion(env.ed, painter, sp, cut);
    painter.add(Shape::closed_line(sp.to_vec(), Stroke::new(1.0, t.canvas_frame_stroke)));

    // grade de pixels em zoom alto (1 pixel do desenho ≥ 8 pixels físicos)
    if env.ed.show_grid || env.ed.view.zoom >= 8.0 {
        let bp = &env.ed.beh.prefs;
        paint_grid(painter, rect, xf, env.ed.view.zoom, canvas, env.ed.show_grid, crate::behavior_prefs::rgb(bp.grid_color), bp.grid_alpha);
    }

    // quadro de câmera e área segura
    if env.ed.show_camera {
        if let Some(cd) = cmds::cut_data(&env.core.project, cut).cloned() {
            paint_camera(env, painter, xf, &cd, canvas);
        }
    }

    // réguas da camada de guia (só visão) e, com a ferramenta Régua, as alças
    super::rulers::paint(env, painter, rect, xf, cut, canvas);
    super::model3d::paint(env, painter, rect, xf, cut);
    paint_selection(env, painter, xf, cut);
    paint_gestures(env, painter, xf, cut);
    super::gradient_tool::paint(env, painter, xf, cut);
    super::text_tool::paint(env, painter, xf, cut);
    if comp_ws {
        super::comp_viewer::paint_overlay(env, painter, rect, xf, cut, canvas);
    }

    // cursor das ferramentas de pintura (como o Photoshop e o Clip Studio): o contorno da ponta no tamanho de verdade;
    // a mira fina só quando o contorno fica pequeno demais para ver (ou sempre, no modo preciso: preferência ou Caps
    // Lock); com o contorno grande, uma marca pequena no centro. Tudo em volta do mesmo ponto, o pixel do ponteiro.
    // Ctrl+Alt+arrastar: o círculo do tamanho novo em volta de onde o arrasto começou, com o valor
    if let Some(sd) = env.ed.beh.size_drag.filter(|_| env.ed.beh.prefs.size_drag_preview) {
        if let Some((v, _)) = tip_size(env, sd.tool) {
            let r = (v * xf.scale / 2.0).max(1.0);
            painter.circle_stroke(sd.origin, r, Stroke::new(3.0, Color32::from_black_alpha(150)));
            painter.circle_stroke(sd.origin, r, Stroke::new(1.0, Color32::WHITE));
            let g = painter.layout_no_wrap(format!("{v:.1} px"), egui::FontId::proportional(12.0), Color32::WHITE);
            let at = sd.origin + egui::vec2(-g.size().x / 2.0, r + 6.0);
            painter.rect_filled(Rect::from_min_size(at, g.size()).expand(3.0), 3.0, Color32::from_black_alpha(170));
            painter.galley(at, g, Color32::WHITE);
        }
    }
    if let Some(p) = env.ed.cursor.filter(|_| env.ed.beh.size_drag.is_none()) {
        let ring = matches!(env.ed.gesture, Gesture::None | Gesture::Stroke(_));
        // espaço/R = mão/girar (o set_cursor mostra o ícone do sistema): sem a mira própria
        let nav = matches!(env.ed.gesture, Gesture::Pan { .. } | Gesture::Rotate { .. }) || painter.ctx().input(|i| i.key_down(egui::Key::Space) || i.key_down(egui::Key::R));
        if rect.contains(p) && !nav {
            let p = Pos2::new(painter.round_to_pixel_center(p.x), painter.round_to_pixel_center(p.y));
            let precise = env.ed.cursor_precise || caps_lock();
            let tip = if ring { cursor_tip(env) } else { None };
            let bp = env.ed.beh.prefs.clone();
            match tip {
                Some((d, preset)) => {
                    let shape = match *env.tool {
                        ToolId::Brush => bp.cursor_brush,
                        ToolId::Pen => bp.cursor_pen,
                        _ => bp.cursor_eraser,
                    };
                    let look = cursor_look_with(d * xf.scale, precise, shape, bp.cursor_small);
                    if look.outline {
                        paint_tip_outline(painter, p, xf, d / 2.0, preset.as_ref(), &env.core.blobs);
                    }
                    match look.center {
                        CenterMark::Cross => crosshair(painter, p, if precise { (2.5, 7.0) } else { (2.0, 5.0) }),
                        CenterMark::Dot => crosshair(painter, p, (0.0, 2.5)),
                        CenterMark::Point => point_mark(painter, p),
                        CenterMark::None => {}
                    }
                }
                None if own_cursor(*env.tool) && bp.cursor_other == ToolCursor::Cross => crosshair(painter, p, (2.5, 7.0)),
                None if own_cursor(*env.tool) => tool_glyph(painter, p, *env.tool),
                None => {}
            }
        }
    }
    let _ = ppp;
}

fn paint_grid(painter: &egui::Painter, rect: Rect, xf: &Xf, zoom: f32, canvas: (u32, u32), forced: bool, tint: Color32, alpha: u8) {
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
    // a grade grossa (zoom baixo) é um pouco mais fraca que a de pixel (padrão 38 e 28 de 255)
    let a = if step == 1.0 { alpha } else { (alpha as f32 * 28.0 / 38.0).round() as u8 };
    let color = Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), a);
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

fn paint_selection(env: &mut Env, painter: &egui::Painter, xf: &Xf, cut: &NodeId) {
    // traços escolhidos de outra camada (trocou a camada ativa): a escolha não vale mais
    let active = env.ed.active_layer(env.core);
    if env.ed.selection.as_ref().is_some_and(|s| !s.strokes.is_empty() && s.strokes_layer != active) {
        env.ed.selection = None;
    }
    let Some(sel) = env.ed.selection.clone() else { return };
    super::selection::paint_strokes(env, painter, xf, cut, &sel, false);
    // formigas: a forma de verdade (máscara da varinha/somas, laço, elipse, retângulo), com a transformação pendente
    if !sel.whole && env.ed.workbar.sel_border {
        super::selection::paint_outline(painter, xf, &sel);
    }
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
        // os mesmos traços que o commit leva (decididos no canvas), pela curva desenhada, no lugar do canvas
        super::selection::paint_strokes(env, painter, xf, cut, &sel, true);
    }
}

pub(super) fn polyline(painter: &egui::Painter, pts: &[Vec2], width_canvas: f32, color: Color32, xf: &Xf) {
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
pub(super) fn variable_line(painter: &egui::Painter, pts: &[Vec2], radii: &[f32], color: Color32, xf: &Xf) {
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
    if *env.tool == ToolId::Lasso {
        super::selection::paint_gesture(env, painter, xf);
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
                // prévia ao vivo ainda não chegou: nada por cima do desenho (a faixa branca de antes cobria o traço
                // pela metade e não era o resultado)
                ToolId::Eraser if env.ed.live_stroke.active() => {}
                _ => {
                    // sem prévia ao vivo para esta camada: os traços vetoriais que a borracha apagaria agora
                    if let Some(cel) = cmds::exposed_cel(&env.core.project, cut, &g.layer, env.ed.frame).and_then(|c| cmds::cel_data(&env.core.project, &c)) {
                        for s in cel.strokes.iter().filter(|s| g.hits.contains(&s.id)) {
                            let sp: Vec<Vec2> = aiai_paint::sample_stroke(s).iter().map(|q| g.to_canvas(Vec2::new(q.x, q.y))).collect();
                            polyline(painter, &sp, s.width.max(2.0), env.t.danger.gamma_multiply(0.8), xf);
                        }
                    }
                }
            }
        }
        // prévia da forma da Figura (com as opções: do centro, proporção, preenchimento) e das linhas
        Gesture::Shape { start, cur } => {
            let (start, cur) = (*start, *cur);
            super::figure::paint_shape(env, painter, xf, start, cur);
        }
        Gesture::Figure => super::figure::paint_live(env, painter, xf),
        // prévia da curva/linha por cliques (próximo trecho até o cursor, fechar no primeiro ponto)
        Gesture::PolyCurve { .. } => super::pen::paint(env, painter, xf),
        // balde: o laço do cercar e preencher / o rastro do pincel da área não preenchida
        Gesture::FillPath { pts } => super::bucket::paint_path(env, painter, xf, pts),
        _ => {}
    }
    let _ = t;
}

fn readout(env: &mut Env, painter: &egui::Painter, rect: Rect, xf: &Xf, c: Vec2) {
    let t = env.t;
    let _ = xf;
    let mut s = format!("{:.0}%   {:+.0}°", env.ed.view.zoom * 100.0, super::panels::navigator::user_rotation_deg(&env.ed.view, env.ed.workbar.flip_v));
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

    /// Contorno pequeno demais vira mira; contorno visível sem nada no meio; contorno grande com marca de centro;
    /// modo preciso sempre com a mira.
    #[test]
    fn cursor_look_by_screen_size() {
        let cursor_look = |d: f32, precise: bool| cursor_look_with(d, precise, CursorShape::Tip, SmallCursor::Cross);
        assert_eq!(cursor_look(2.0, false), CursorLook { outline: false, center: CenterMark::Cross });
        assert_eq!(cursor_look(OUTLINE_MIN_PX - 0.1, false).outline, false);
        assert_eq!(cursor_look(12.0, false), CursorLook { outline: true, center: CenterMark::None });
        assert_eq!(cursor_look(300.0, false), CursorLook { outline: true, center: CenterMark::Dot });
        assert_eq!(cursor_look(300.0, true), CursorLook { outline: true, center: CenterMark::Cross });
        assert_eq!(cursor_look(3.0, true), CursorLook { outline: false, center: CenterMark::Cross });
        // pincel de 24 px: 10% = 2,4 px (mira), 100% = 24 px (contorno + marca), 400% = 96 px
        assert_eq!(cursor_look(24.0 * 0.1, false).center, CenterMark::Cross);
        assert!(cursor_look(24.0, false).outline && cursor_look(96.0, false).center == CenterMark::Dot);
    }

    /// Preferências > Cursor: a forma por tipo de ferramenta e o que mostrar quando a ponta é pequena demais.
    #[test]
    fn cursor_look_follows_the_preferences() {
        use CenterMark as M;
        // forma da ponta com a mira como padrão: ponta pequena vira mira, contorno visível fica sozinho
        assert_eq!(cursor_look_with(2.0, false, CursorShape::Tip, SmallCursor::Cross), CursorLook { outline: false, center: M::Cross });
        assert_eq!(cursor_look_with(12.0, false, CursorShape::Tip, SmallCursor::None), CursorLook { outline: true, center: M::None });
        // ponta pequena: o que as preferências escolhem aparece no lugar do contorno
        assert_eq!(cursor_look_with(2.0, false, CursorShape::Tip, SmallCursor::None), CursorLook { outline: false, center: M::None });
        assert_eq!(cursor_look_with(2.0, false, CursorShape::Tip, SmallCursor::Dot), CursorLook { outline: false, center: M::Point });
        // ponta de tamanho 0 (borracha de preenchimento) nunca fica sem mira
        assert_eq!(cursor_look_with(0.0, false, CursorShape::Tip, SmallCursor::None).center, M::Cross);
        // mira e ponto valem para qualquer tamanho, sem contorno
        assert_eq!(cursor_look_with(300.0, false, CursorShape::Cross, SmallCursor::None), CursorLook { outline: false, center: M::Cross });
        assert_eq!(cursor_look_with(300.0, false, CursorShape::Dot, SmallCursor::Cross), CursorLook { outline: false, center: M::Point });
        // o modo preciso continua mandando na forma da ponta
        assert_eq!(cursor_look_with(300.0, true, CursorShape::Tip, SmallCursor::None).center, M::Cross);
    }

    /// O contorno das pontas de forma fica em volta do mesmo ponto em que o motor carimba (o centro da mira): a caixa
    /// do contorno bate com a caixa da tinta de um carimbo no ponto.
    #[test]
    fn tip_outline_sits_on_the_dab_hotspot() {
        use aiai_paint::brush::{BrushPreset, BrushTip, TipShape};
        for shape in [TipShape::GrassBlade, TipShape::Rose, TipShape::CloudPuff, TipShape::Leaf, TipShape::Square] {
            let mut p = BrushPreset::round("forma", 60.0, 1.0);
            p.tip = BrushTip::Shape { shape, aspect: 1.0 };
            let mut img = aiai_paint::Image::new(128, 128);
            aiai_paint::brush::paint_stroke(&mut img, &p, &[aiai_paint::brush::BrushPoint { x: 64.0, y: 64.0, pressure: 1.0 }], [0.0, 0.0, 0.0, 1.0], 1);
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for y in 0..128u32 {
                for x in 0..128u32 {
                    if img.get(x, y)[3] >= 0.5 {
                        let (u, v) = ((x as f32 + 0.5 - 64.0) / 30.0, (y as f32 + 0.5 - 64.0) / 30.0);
                        (x0, y0, x1, y1) = (x0.min(u), y0.min(v), x1.max(u), y1.max(v));
                    }
                }
            }
            let segs = tip_outline_unit(&p, &BlobStore::default()).expect("contorno da forma");
            let (mut a0, mut b0, mut a1, mut b1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for q in segs.iter().flatten() {
                (a0, b0, a1, b1) = (a0.min(q.x), b0.min(q.y), a1.max(q.x), b1.max(q.y));
            }
            let tol = 0.08;
            assert!((x0 - a0).abs() < tol && (x1 - a1).abs() < tol && (y0 - b0).abs() < tol && (y1 - b1).abs() < tol, "{shape:?}: tinta [{x0:.2},{y0:.2}..{x1:.2},{y1:.2}] × contorno [{a0:.2},{b0:.2}..{a1:.2},{b1:.2}]");
        }
    }

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
