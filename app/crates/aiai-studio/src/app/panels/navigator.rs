//! Painel Navegador dos workspaces de canvas (Layout, Pintura, Cenário, Composição): miniatura do quadro que o canvas está
//! mostrando (a mesma textura, sem render novo) com o contorno da área visível — que gira junto com a visão — e o
//! controle da visão: clicar ou arrastar na miniatura centraliza ali, a roda aproxima/afasta, e a linha de baixo tem o
//! zoom em % e o enquadrar.

use crate::app::editor::{MAX_ZOOM, MIN_ZOOM};
use crate::app::ui_kit::{Env, body_ui};
use crate::i18n::tr;
use crate::icons::IconId;
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};

/// Cantos da área visível do canvas em coordenadas do desenho (na ordem do contorno).
pub(crate) fn visible_quad(view: &crate::app::editor::View, canvas_rect: Rect, ppp: f32) -> [Vec2; 4] {
    let xf = view.xf(canvas_rect, ppp);
    [canvas_rect.left_top(), canvas_rect.right_top(), canvas_rect.right_bottom(), canvas_rect.left_bottom()].map(|p| xf.to_canvas(p))
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let ppp = ui.ctx().pixels_per_point();
    let (Some(shown), Some(canvas_rect)) = (env.ed.shown.as_ref(), env.ed.canvas_rect) else {
        ui.label(egui::RichText::new(tr("panel.navigator.empty")).color(t.text_dim));
        return;
    };
    let (tex, canvas, uv) = (shown.tex.id(), shown.rendered.canvas, shown.rendered.uv_max);
    let (cw, ch) = (canvas[0].max(1) as f32, canvas[1].max(1) as f32);
    let full = ui.available_rect_before_wrap();
    let bar_h = 26.0;
    let area = Rect::from_min_max(full.min, egui::pos2(full.max.x, (full.max.y - bar_h).max(full.min.y + 10.0)));
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
    let shade = Color32::from_black_alpha(110);
    shade_outside(&painter, img, &quad, shade);
    painter.add(Shape::closed_line(quad.to_vec(), Stroke::new(1.5, t.accent)));
    env.note("nav:view", Rect::from_points(&quad));

    // linha de baixo: enquadrar e zoom em %
    let bar = Rect::from_min_max(egui::pos2(full.min.x, area.max.y + 2.0), full.max);
    let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
    if env.icon_button(&mut bui, IconId::Fit, "canvas.fit", false, true, "nav:fit").clicked() {
        env.ed.fit_pending = true;
    }
    let mut pct = env.ed.view.zoom * 100.0;
    let dv = bui.add(egui::DragValue::new(&mut pct).range(MIN_ZOOM * 100.0..=MAX_ZOOM * 100.0).speed(1.0).max_decimals(0).suffix("%"));
    env.note("nav:zoom", dv.rect);
    if dv.changed() {
        env.ed.view.zoom = (pct / 100.0).clamp(MIN_ZOOM, MAX_ZOOM);
        env.ed.auto_fit = false;
    }
    ui.allocate_rect(full, Sense::hover());
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
    use crate::app::editor::View;

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
}
