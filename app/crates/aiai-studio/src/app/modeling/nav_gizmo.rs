//! Gizmo de navegação do viewport (a "bolinha de eixos" do Blender): clicar num eixo alinha a câmera, arrastar o disco orbita,
//! e dois botões redondos fazem pan (mão) e zoom (lupa) arrastando; um terceiro alterna perspectiva/ortogonal.
//!
//! Só desenha e devolve se mexeu na câmera — a câmera é do chamador (cada workspace tem a sua). Y para cima (o mesmo
//! mapeamento do widget de eixos da Modelagem: X ↔ Direita, Y ↔ Topo, Z ↔ Frente).
//!
//! Copiado do `anigo-studio/src/viewport/nav_gizmo.rs` do ANIGO; no AnimeIAI os botões usam os ícones SVG do app e as
//! cores do tema (`Tokens`), e cada parte registra o retângulo para a bridge (`mdl:nav.*`).

use super::camera::{OrbitCamera, ViewPreset};
use crate::icons::{IconId, Icons};
use crate::theme::Tokens;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use glam::Vec3;

const BALL_R: f32 = 36.0;
const NODE_R: f32 = 8.0;
const BTN_R: f32 = 13.0;

/// Resultado de um quadro do gizmo.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct NavResult {
    /// A câmera foi alterada neste quadro.
    pub changed: bool,
    /// O ponteiro está sobre o gizmo (o viewport não deve começar um gesto por baixo).
    pub hovered: bool,
}

struct AxisNode {
    dir: Vec3,
    label: &'static str,
    color: Color32,
    preset: ViewPreset,
}

fn nodes() -> [AxisNode; 6] {
    [
        AxisNode { dir: Vec3::X, label: "X", color: Color32::from_rgb(0xe8, 0x45, 0x4b), preset: ViewPreset::Right },
        AxisNode { dir: Vec3::Y, label: "Y", color: Color32::from_rgb(0x6c, 0xc0, 0x4a), preset: ViewPreset::Top },
        AxisNode { dir: Vec3::Z, label: "Z", color: Color32::from_rgb(0x3d, 0x7a, 0xf0), preset: ViewPreset::Front },
        AxisNode { dir: -Vec3::X, label: "", color: Color32::from_rgb(0x7a, 0x30, 0x34), preset: ViewPreset::Left },
        AxisNode { dir: -Vec3::Y, label: "", color: Color32::from_rgb(0x42, 0x68, 0x32), preset: ViewPreset::Bottom },
        AxisNode { dir: -Vec3::Z, label: "", color: Color32::from_rgb(0x28, 0x4a, 0x8a), preset: ViewPreset::Back },
    ]
}

/// Posição de um eixo na tela do gizmo (relativa ao centro) e profundidade (maior = mais perto da câmera).
pub fn project_axis(cam: &OrbitCamera, dir: Vec3) -> (Vec2, f32) {
    (Vec2::new(dir.dot(cam.right()), -dir.dot(cam.up())) * (BALL_R - NODE_R - 2.0), -dir.dot(cam.forward()))
}

/// Desenha o gizmo no canto superior direito de `rect` e trata o mouse. `salt` separa os gizmos de vistas diferentes.
pub fn show(ui: &mut egui::Ui, rect: Rect, cam: &mut OrbitCamera, salt: &str, icons: &mut Icons, theme: &Tokens, notes: &mut Vec<(String, Rect)>) -> NavResult {
    let mut out = NavResult::default();
    let center = Pos2::new(rect.right() - 86.0, rect.top() + 62.0);
    let painter = ui.painter_at(rect);
    let id = egui::Id::new("nav-gizmo").with(salt);
    let pointer = ui.input(|i| i.pointer.hover_pos());

    // disco de fundo: arrastar = orbitar
    let disc_rect = Rect::from_center_size(center, Vec2::splat(BALL_R * 2.0));
    let disc = ui.interact(disc_rect, id.with("disc"), Sense::click_and_drag());
    notes.push(("mdl:nav.disc".into(), disc_rect));
    let over_disc = pointer.is_some_and(|p| p.distance(center) <= BALL_R);
    let active_disc = disc.dragged();
    painter.circle_filled(center, BALL_R, Color32::from_rgba_unmultiplied(18, 18, 24, if over_disc || active_disc { 170 } else { 120 }));
    if over_disc || active_disc {
        painter.circle_stroke(center, BALL_R, Stroke::new(1.0, theme.border));
    }

    // eixos, do mais longe para o mais perto
    let mut axes: Vec<(Vec2, f32, usize)> = nodes().iter().enumerate().map(|(i, n)| {
        let (p, z) = project_axis(cam, n.dir);
        (p, z, i)
    }).collect();
    axes.sort_by(|a, b| a.1.total_cmp(&b.1));
    let table = nodes();
    let mut picked: Option<ViewPreset> = None;
    let mut hot_node = false;
    for (p, depth, i) in axes {
        let n = &table[i];
        let pos = center + p;
        let hovered = pointer.is_some_and(|h| h.distance(pos) <= NODE_R + 1.0);
        hot_node |= hovered;
        let positive = !n.label.is_empty();
        if positive {
            painter.line_segment([center, pos], Stroke::new(2.0, n.color.gamma_multiply(if depth >= 0.0 { 1.0 } else { 0.55 })));
        }
        let radius = if positive { NODE_R } else { NODE_R - 2.5 };
        let fill = if hovered { Color32::WHITE } else if positive { n.color } else { n.color.gamma_multiply(0.9) };
        painter.circle_filled(pos, radius, fill);
        if !positive {
            painter.circle_stroke(pos, radius, Stroke::new(1.0, n.color.gamma_multiply(1.6)));
        }
        if positive {
            painter.text(pos, Align2::CENTER_CENTER, n.label, FontId::proportional(10.5), if hovered { Color32::BLACK } else { Color32::from_rgb(0x10, 0x10, 0x14) });
        }
        if hovered && disc.clicked() {
            picked = Some(n.preset);
        }
        notes.push((format!("mdl:nav.axis.{}", preset_key(n.preset)), Rect::from_center_size(pos, Vec2::splat(radius * 2.0))));
    }
    if let Some(p) = picked {
        cam.set_preset(p);
        out.changed = true;
    } else if disc.dragged() && !hot_node {
        let d = disc.drag_delta();
        cam.orbit(d.x, d.y);
        out.changed = true;
    }
    if over_disc || active_disc {
        ui.output_mut(|o| o.cursor_icon = if disc.dragged() { egui::CursorIcon::Grabbing } else if hot_node { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Grab });
    }

    // botões: pan (mão), zoom (lupa) e projeção
    let bx = rect.right() - 34.0;
    let by = rect.top() + 34.0;
    let mut btn = |ui: &mut egui::Ui, painter: &egui::Painter, c: Pos2, icon: IconId, tip: &str, key: &str, active: bool| -> egui::Response {
        let r = ui.interact(Rect::from_center_size(c, Vec2::splat(BTN_R * 2.0)), id.with(key), Sense::click_and_drag());
        notes.push((format!("mdl:nav.{key}"), r.rect));
        let hot = r.hovered() || r.dragged();
        let fill = if r.dragged() { theme.accent_dim } else if hot { theme.bg_hover } else if active { theme.accent_dim.gamma_multiply(0.8) } else { Color32::from_rgba_unmultiplied(18, 18, 24, 120) };
        painter.circle_filled(c, BTN_R, fill);
        if hot {
            painter.circle_stroke(c, BTN_R, Stroke::new(1.0, theme.accent_dim));
        }
        let ink = if hot || active { Color32::from_gray(235) } else { Color32::from_gray(170) };
        icons.paint(painter, icon, Rect::from_center_size(c, Vec2::splat(BTN_R * 1.15)), ink);
        r.on_hover_text(tip.to_string())
    };
    let pan = btn(ui, &painter, Pos2::new(bx, by), IconId::ToolHand, crate::i18n::tr("mdl.nav.pan"), "pan", false);
    if pan.dragged() {
        let d = pan.drag_delta();
        cam.pan(d.x, d.y, rect.height());
        out.changed = true;
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Grabbing);
    }
    let zoom = btn(ui, &painter, Pos2::new(bx, by + 34.0), IconId::ToolZoom, crate::i18n::tr("mdl.nav.zoom"), "zoom", false);
    if zoom.dragged() {
        cam.zoom((1.0 + zoom.drag_delta().y * 0.01).clamp(0.5, 1.5));
        out.changed = true;
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeVertical);
    }
    let proj_icon = if cam.orthographic { IconId::CameraFrame } else { IconId::ToolModel3d };
    let proj_tip = if cam.orthographic { crate::i18n::tr("mdl.nav.to_perspective") } else { crate::i18n::tr("mdl.nav.to_ortho") };
    let proj = btn(ui, &painter, Pos2::new(bx, by + 68.0), proj_icon, proj_tip, "proj", cam.orthographic);
    if proj.clicked() {
        cam.orthographic = !cam.orthographic;
        out.changed = true;
    }
    out.hovered = over_disc || pan.hovered() || zoom.hovered() || proj.hovered() || disc.dragged() || pan.dragged() || zoom.dragged();
    out
}

/// Nome estável da vista (bridge e dicas).
pub fn preset_key(p: ViewPreset) -> &'static str {
    match p {
        ViewPreset::Front => "front",
        ViewPreset::Back => "back",
        ViewPreset::Right => "right",
        ViewPreset::Left => "left",
        ViewPreset::Top => "top",
        ViewPreset::Bottom => "bottom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_view_puts_z_in_front_and_x_to_the_right() {
        let mut cam = OrbitCamera::default();
        cam.set_preset(ViewPreset::Front);
        let (px, _) = project_axis(&cam, Vec3::X);
        let (pz, depth_z) = project_axis(&cam, Vec3::Z);
        assert!(px.x > 10.0 && px.y.abs() < 1e-3, "X para a direita: {px:?}");
        assert!(pz.length() < 1e-3 && depth_z > 0.9, "Z aponta para o observador: {pz:?} {depth_z}");
    }

    #[test]
    fn top_view_puts_y_toward_the_viewer() {
        let mut cam = OrbitCamera::default();
        cam.set_preset(ViewPreset::Top);
        let (_, depth_y) = project_axis(&cam, Vec3::Y);
        assert!(depth_y > 0.9, "{depth_y}");
    }

    #[test]
    fn every_axis_projects_inside_the_ball() {
        let cam = OrbitCamera::default();
        for n in nodes() {
            let (p, _) = project_axis(&cam, n.dir);
            assert!(p.length() <= BALL_R - NODE_R + 0.01);
        }
    }
}
