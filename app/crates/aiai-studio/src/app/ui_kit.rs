//! Peças comuns dos painéis do editor: o ambiente (`Env`: documento, estado do editor, ícones, tema) e widgets pequenos
//! (botão de ícone, cabeçalho de seção) que também registram o retângulo para o `GET_UI_STATE` (o harness de mouse
//! real clica pelos retângulos registrados).

use super::editor::Editor;
use crate::i18n::tr;
use crate::icons::{IconId, Icons};
use crate::theme::Tokens;
use crate::workspace::{ToolId, WorkspaceId};
use aiai_core::CoreSession;
use egui::{Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};

pub(crate) struct Env<'a> {
    pub t: Tokens,
    pub core: &'a mut CoreSession,
    pub ed: &'a mut Editor,
    pub icons: &'a mut Icons,
    pub notes: &'a mut Vec<(String, Rect)>,
    pub tool: &'a mut ToolId,
    /// Workspace ativo (o painel Propriedades mostra o que combina com ele).
    pub ws: WorkspaceId,
}

impl Env<'_> {
    pub(crate) fn note(&mut self, name: impl Into<String>, rect: Rect) {
        self.notes.push((name.into(), rect));
    }

    /// Botão quadrado só com ícone. `selected` = ligado (acento). Registra o retângulo como `name` (se não vazio).
    pub(crate) fn icon_button(&mut self, ui: &mut Ui, icon: IconId, tip_key: &str, selected: bool, enabled: bool, name: &str) -> Response {
        self.icon_button_sized(ui, icon, tip_key, selected, enabled, name, 24.0, false)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn icon_button_sized(&mut self, ui: &mut Ui, icon: IconId, tip_key: &str, selected: bool, enabled: bool, name: &str, size: f32, mirror: bool) -> Response {
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), if enabled { Sense::click() } else { Sense::hover() });
        if !name.is_empty() {
            self.note(name, rect);
        }
        let t = self.t;
        let p = ui.painter();
        if selected {
            p.rect(rect, 4.0, t.accent_dim, Stroke::new(1.0, t.accent), StrokeKind::Inside);
        } else if resp.hovered() && enabled {
            p.rect_filled(rect, 4.0, t.bg_hover);
        }
        let ink = if !enabled {
            t.text_dim.gamma_multiply(0.45)
        } else if selected || resp.hovered() {
            t.text
        } else {
            t.text_dim
        };
        let inner = rect.shrink((size * 0.2).round());
        paint_icon(self.icons, p, icon, inner, ink, mirror);
        resp.on_hover_text(tr(tip_key))
    }
}

/// Ícone com opção de espelhamento horizontal (o botão de girar para a esquerda usa o mesmo desenho).
pub(crate) fn paint_icon(icons: &mut Icons, painter: &egui::Painter, icon: IconId, rect: Rect, tint: Color32, mirror: bool) {
    let side = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), Vec2::splat(side));
    let tex = icons.texture(painter.ctx(), icon, side);
    let (u0, u1) = if mirror { (1.0, 0.0) } else { (0.0, 1.0) };
    painter.image(tex, r, Rect::from_min_max(egui::pos2(u0, 0.0), egui::pos2(u1, 1.0)), tint);
}

/// Subtítulo de seção dentro de um painel (texto pequeno, atenuado, com linha).
pub(crate) fn section(ui: &mut Ui, t: &Tokens, key: &str) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr(key)).size(11.5).color(t.text_dim).strong());
        let w = ui.available_width();
        if w > 8.0 {
            let (r, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), Sense::hover());
            ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, t.border));
        }
    });
}

/// Rótulo atenuado de uma linha de propriedade.
pub(crate) fn dim_label(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(egui::RichText::new(text).size(12.0).color(t.text_dim));
}

/// Filho do painel com margem e recorte (corpo de um painel do dock).
pub(crate) fn body_ui(ui: &mut Ui, body: Rect, pad: f32) -> Ui {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body.shrink(pad)));
    child.set_clip_rect(body);
    child
}

/// Cor `ColorRef` → `Color32` opaca (para swatches).
pub(crate) fn rgba_to_color32(c: aiai_core::Rgba) -> Color32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Color32::from_rgba_unmultiplied(q(c.0[0]), q(c.0[1]), q(c.0[2]), q(c.0[3]))
}

pub(crate) fn color32_to_rgba(c: Color32) -> aiai_core::Rgba {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    aiai_core::Rgba([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0])
}
