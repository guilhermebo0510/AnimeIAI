//! Painel Navegador dos workspaces de canvas (S25: no topo da coluna da direita, como no Clip Studio): miniatura do quadro
//! que o canvas está mostrando (a mesma textura, sem render novo) com o contorno da área visível — que gira junto com a
//! visão —, e embaixo o controle da visão à vista: zoom (diminuir, controle deslizante, aumentar, 100% e enquadrar) e
//! rotação (girar para a esquerda, controle deslizante, girar para a direita, sem rotação, espelho horizontal e
//! vertical). Clicar ou arrastar na miniatura centraliza ali; a roda aproxima/afasta. Tudo só de visão: o desenho não muda.
//!
//! O espelho vertical é o horizontal com meia volta (`View` guarda um espelho só): a visão fica `flip = H xor V` e
//! `rot = θ + V·180°`, com `V` guardado em `WorkbarState::flip_v`; a rotação mostrada é a do usuário (θ).

use crate::app::editor::{MAX_ZOOM, MIN_ZOOM, View};
use crate::app::ui_kit::{Env, body_ui};
use crate::i18n::tr;
use crate::icons::IconId;
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use std::f32::consts::{PI, TAU};

/// Passo dos botões de girar.
pub(crate) const ROT_STEP_DEG: f32 = 15.0;

/// Cantos da área visível do canvas em coordenadas do desenho (na ordem do contorno).
pub(crate) fn visible_quad(view: &View, canvas_rect: Rect, ppp: f32) -> [Vec2; 4] {
    let xf = view.xf(canvas_rect, ppp);
    [canvas_rect.left_top(), canvas_rect.right_top(), canvas_rect.right_bottom(), canvas_rect.left_bottom()].map(|p| xf.to_canvas(p))
}

/// Espelhos (horizontal, vertical) que o usuário vê.
pub(crate) fn flips(view: &View, flip_v: bool) -> (bool, bool) {
    (view.flip ^ flip_v, flip_v)
}

/// Espelho horizontal da visão.
pub(crate) fn toggle_flip_h(view: &mut View) {
    view.flip = !view.flip;
}

/// Espelho vertical da visão (= horizontal + meia volta).
pub(crate) fn toggle_flip_v(view: &mut View, flip_v: &mut bool) {
    *flip_v = !*flip_v;
    view.flip = !view.flip;
    view.rotate_by(PI);
}

/// Rotação do usuário em graus (−180, 180], sem a meia volta do espelho vertical.
pub(crate) fn user_rotation_deg(view: &View, flip_v: bool) -> f32 {
    let r = (view.rot - if flip_v { PI } else { 0.0 }).rem_euclid(TAU).to_degrees();
    if r > 180.0 + 1e-3 { r - 360.0 } else { r }
}

/// Põe a rotação do usuário (graus), mantendo o espelho vertical.
pub(crate) fn set_user_rotation(view: &mut View, flip_v: bool, deg: f32) {
    view.rot = (deg.to_radians() + if flip_v { PI } else { 0.0 }).rem_euclid(TAU);
}

/// Zoom em volta do centro da área do canvas (o ponto do meio fica parado).
fn zoom_to(env: &mut Env, ppp: f32, z: f32) {
    match env.ed.canvas_rect {
        Some(r) => env.ed.view.set_zoom_at(r, ppp, r.center(), z),
        None => env.ed.view.zoom = z.clamp(MIN_ZOOM, MAX_ZOOM),
    }
    env.ed.auto_fit = false;
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let ppp = ui.ctx().pixels_per_point();
    let full = ui.available_rect_before_wrap();
    let row_h = 26.0;
    let rows_h = row_h * 2.0 + 6.0;
    let area = Rect::from_min_max(full.min, egui::pos2(full.max.x, (full.max.y - rows_h).max(full.min.y + 10.0)));
    match (env.ed.shown.as_ref().map(|s| (s.tex.id(), s.rendered.canvas, s.rendered.uv_max)), env.ed.canvas_rect) {
        (Some((tex, canvas, uv)), Some(canvas_rect)) => preview(env, ui, area, tex, canvas, uv, canvas_rect, ppp),
        _ => {
            ui.painter().rect_filled(area, 3.0, t.canvas_bg);
            ui.painter().text(area.center(), egui::Align2::CENTER_CENTER, tr("panel.navigator.empty"), egui::FontId::proportional(12.0), t.text_dim);
        }
    }
    let has = env.ed.canvas_rect.is_some();
    let rows = Rect::from_min_max(egui::pos2(full.min.x, area.max.y + 4.0), full.max);
    let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(rows).layout(egui::Layout::top_down(egui::Align::Min)));
    let rui = &mut rui;
    rui.spacing_mut().item_spacing = egui::vec2(2.0, 4.0);
    // ---- zoom
    rui.horizontal(|ui| {
        ui.set_height(row_h);
        if env.icon_button(ui, IconId::ZoomOut, "nav.zoom_out", false, has, "nav:zoom_out").clicked() {
            let z = env.ed.view.zoom / 1.25;
            zoom_to(env, ppp, z);
        }
        let mut pct = env.ed.view.zoom * 100.0;
        ui.spacing_mut().slider_width = (ui.available_width() - 3.0 * 26.0 - 54.0).max(40.0);
        let r = ui.add_enabled(has, egui::Slider::new(&mut pct, MIN_ZOOM * 100.0..=MAX_ZOOM * 100.0).logarithmic(true).show_value(false));
        env.note("nav:zoom_slider", r.rect);
        if r.changed() {
            zoom_to(env, ppp, pct / 100.0);
        }
        if env.icon_button(ui, IconId::ToolZoom, "nav.zoom_in", false, has, "nav:zoom_in").clicked() {
            let z = env.ed.view.zoom * 1.25;
            zoom_to(env, ppp, z);
        }
        if env.icon_button(ui, IconId::OneToOne, "canvas.actual", false, has, "nav:actual").clicked() {
            zoom_to(env, ppp, 1.0);
        }
        if env.icon_button(ui, IconId::Fit, "canvas.fit", false, has, "nav:fit").clicked() {
            env.ed.fit_pending = true;
        }
        let mut pct = env.ed.view.zoom * 100.0;
        let dv = ui.add_enabled(has, egui::DragValue::new(&mut pct).range(MIN_ZOOM * 100.0..=MAX_ZOOM * 100.0).speed(1.0).max_decimals(0).suffix("%"));
        env.note("nav:zoom", dv.rect);
        if dv.changed() {
            zoom_to(env, ppp, pct / 100.0);
        }
    });
    // ---- rotação e espelhos
    rui.horizontal(|ui| {
        ui.set_height(row_h);
        let v = env.ed.workbar.flip_v;
        if env.icon_button_sized(ui, IconId::RotateView, "canvas.rotate_left", false, has, "nav:rot_left", 24.0, true).clicked() {
            env.ed.view.rotate_by(-ROT_STEP_DEG.to_radians());
            env.ed.auto_fit = false;
        }
        let mut deg = user_rotation_deg(&env.ed.view, v);
        ui.spacing_mut().slider_width = (ui.available_width() - 4.0 * 26.0 - 54.0).max(40.0);
        let r = ui.add_enabled(has, egui::Slider::new(&mut deg, -180.0..=180.0).show_value(false));
        env.note("nav:rot_slider", r.rect);
        if r.changed() {
            set_user_rotation(&mut env.ed.view, v, deg);
            env.ed.auto_fit = false;
        }
        if env.icon_button(ui, IconId::RotateView, "canvas.rotate_right", false, has, "nav:rot_right").clicked() {
            env.ed.view.rotate_by(ROT_STEP_DEG.to_radians());
            env.ed.auto_fit = false;
        }
        let rotated = deg.abs() > 0.05;
        if env.icon_button(ui, IconId::RotateReset, "nav.rot_reset", false, has && rotated, "nav:rot_reset").clicked() {
            set_user_rotation(&mut env.ed.view, v, 0.0);
        }
        let (h, vv) = flips(&env.ed.view, v);
        if env.icon_button(ui, IconId::Flip, "canvas.flip", h, has, "nav:flip_h").clicked() {
            toggle_flip_h(&mut env.ed.view);
        }
        if env.icon_button(ui, IconId::FlipV, "nav.flip_v", vv, has, "nav:flip_v").clicked() {
            toggle_flip_v(&mut env.ed.view, &mut env.ed.workbar.flip_v);
        }
        let mut d = user_rotation_deg(&env.ed.view, env.ed.workbar.flip_v);
        let dv = ui.add_enabled(has, egui::DragValue::new(&mut d).range(-180.0..=180.0).speed(0.5).max_decimals(0).suffix("°"));
        env.note("nav:rotation", dv.rect);
        if dv.changed() {
            set_user_rotation(&mut env.ed.view, env.ed.workbar.flip_v, d);
            env.ed.auto_fit = false;
        }
    });
    ui.allocate_rect(full, Sense::hover());
}

#[allow(clippy::too_many_arguments)]
fn preview(env: &mut Env, ui: &mut Ui, area: Rect, tex: egui::TextureId, canvas: [u32; 2], uv: [f32; 2], canvas_rect: Rect, ppp: f32) {
    let t = env.t;
    let (cw, ch) = (canvas[0].max(1) as f32, canvas[1].max(1) as f32);
    let s = (area.width() / cw).min(area.height() / ch).max(1e-4);
    let img = Rect::from_center_size(area.center(), egui::vec2(cw * s, ch * s));
    let to_nav = |p: Vec2| Pos2::new(img.left() + p.x * s, img.top() + p.y * s);
    let from_nav = |q: Pos2| Vec2::new((q.x - img.left()) / s, (q.y - img.top()) / s);

    let resp = ui.interact(area, ui.id().with("navigator_area"), Sense::click_and_drag());
    env.note("nav:area", img);
    if resp.clicked() || resp.dragged() {
        if let Some(q) = resp.interact_pointer_pos() {
            let p = from_nav(q);
            env.ed.view.center = Vec2::new(p.x.clamp(0.0, cw), p.y.clamp(0.0, ch));
            env.ed.auto_fit = false;
        }
    }
    if resp.hovered() {
        let notches = ui.input(|i| i.smooth_scroll_delta.y) / 50.0;
        if notches.abs() > 1e-3 {
            env.ed.view.zoom = (env.ed.view.zoom * 1.15f32.powf(notches)).clamp(MIN_ZOOM, MAX_ZOOM);
            env.ed.auto_fit = false;
        }
    }
    let painter = ui.painter().with_clip_rect(area);
    painter.rect_filled(area, 2.0, t.canvas_bg);
    painter.image(tex, img, Rect::from_min_max(Pos2::ZERO, egui::pos2(uv[0], uv[1])), Color32::WHITE);
    painter.rect_stroke(img, 0.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
    // área visível: o que fica fora dela escurece um pouco, o contorno leva o acento
    let quad = visible_quad(&env.ed.view, canvas_rect, ppp).map(to_nav);
    shade_outside(&painter, img, &quad, Color32::from_black_alpha(110));
    painter.add(Shape::closed_line(quad.to_vec(), Stroke::new(1.5, t.accent)));
    env.note("nav:view", Rect::from_points(&quad));
}

/// Escurece a miniatura fora do quadrilátero visível (quatro faixas, recortadas pela miniatura). O contorno girado não
/// precisa de recorte exato: as faixas cobrem o retângulo envolvente, e o que sobra dentro dele fica claro.
fn shade_outside(painter: &egui::Painter, img: Rect, quad: &[Pos2; 4], color: Color32) {
    let bb = Rect::from_points(quad).intersect(img);
    if !bb.is_positive() {
        painter.rect_filled(img, 0.0, color);
        return;
    }
    let bands = [
        Rect::from_min_max(img.min, egui::pos2(img.max.x, bb.min.y)),
        Rect::from_min_max(egui::pos2(img.min.x, bb.max.y), img.max),
        Rect::from_min_max(egui::pos2(img.min.x, bb.min.y), egui::pos2(bb.min.x, bb.max.y)),
        Rect::from_min_max(egui::pos2(bb.max.x, bb.min.y), egui::pos2(img.max.x, bb.max.y)),
    ];
    for b in bands {
        if b.is_positive() {
            painter.rect_filled(b, 0.0, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_quad_matches_zoom_and_center() {
        // área de 400×200 pontos, ppp 1, zoom 2: vê 200×100 px do desenho em torno do centro (500, 300)
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), egui::vec2(400.0, 200.0));
        let view = View { zoom: 2.0, center: Vec2::new(500.0, 300.0), rot: 0.0, flip: false };
        let q = visible_quad(&view, rect, 1.0);
        assert!((q[0] - Vec2::new(400.0, 250.0)).length() < 0.6, "{q:?}");
        assert!((q[2] - Vec2::new(600.0, 350.0)).length() < 0.6, "{q:?}");
        // girada 90°: largura e altura vistas trocam de eixo, o centro fica
        let view = View { rot: std::f32::consts::FRAC_PI_2, ..view };
        let q = visible_quad(&view, rect, 1.0);
        let c = (q[0] + q[1] + q[2] + q[3]) / 4.0;
        assert!((c - Vec2::new(500.0, 300.0)).length() < 0.6);
        let bb = Rect::from_points(&q.map(|v| v.to_pos2()));
        assert!((bb.width() - 100.0).abs() < 1.0 && (bb.height() - 200.0).abs() < 1.0, "{bb:?}");
    }

    fn screen(view: &View, p: Vec2) -> Pos2 {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 200.0));
        view.xf(rect, 1.0).to_screen(p)
    }

    #[test]
    fn vertical_flip_mirrors_top_and_bottom_and_keeps_the_user_rotation() {
        let base = View { zoom: 1.0, center: Vec2::new(100.0, 100.0), rot: 0.0, flip: false };
        let mut v = base;
        let mut fv = false;
        toggle_flip_v(&mut v, &mut fv);
        assert_eq!(flips(&v, fv), (false, true));
        // um ponto acima do centro aparece embaixo; esquerda e direita ficam
        let p = screen(&v, Vec2::new(60.0, 40.0));
        let q = screen(&base, Vec2::new(60.0, 40.0));
        assert!((p.x - q.x).abs() < 0.01 && (p.y - (200.0 - q.y)).abs() < 0.01, "{p:?} {q:?}");
        // a rotação mostrada continua 0 (a meia volta é do espelho)
        assert!(user_rotation_deg(&v, fv).abs() < 0.01);
        // horizontal por cima: os dois ligados = meia volta pura
        toggle_flip_h(&mut v);
        assert_eq!(flips(&v, fv), (true, true));
        let p = screen(&v, Vec2::new(60.0, 40.0));
        assert!((p.x - 140.0).abs() < 0.01 && (p.y - 160.0).abs() < 0.01, "{p:?}");
        // desligar os dois volta à visão original
        toggle_flip_v(&mut v, &mut fv);
        toggle_flip_h(&mut v);
        assert_eq!(flips(&v, fv), (false, false));
        assert!(screen(&v, Vec2::new(60.0, 40.0)).distance(q) < 0.01);
        assert!(user_rotation_deg(&v, fv).abs() < 0.01);
    }

    #[test]
    fn user_rotation_is_set_and_read_back_with_and_without_the_vertical_flip() {
        for fv in [false, true] {
            let mut v = View { zoom: 1.0, center: Vec2::ZERO, rot: 0.0, flip: fv };
            for deg in [-170.0f32, -45.0, 0.0, 30.0, 180.0] {
                set_user_rotation(&mut v, fv, deg);
                let back = user_rotation_deg(&v, fv);
                assert!((back - deg).abs() < 0.01 || (deg == 180.0 && (back + 180.0).abs() < 0.01), "{fv} {deg} {back}");
            }
            // girar pelos botões soma o passo
            set_user_rotation(&mut v, fv, 0.0);
            v.rotate_by(ROT_STEP_DEG.to_radians());
            assert!((user_rotation_deg(&v, fv) - ROT_STEP_DEG).abs() < 0.01);
        }
    }
}
