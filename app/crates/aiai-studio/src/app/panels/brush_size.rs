//! Paleta Tamanho do pincel (S25): os tamanhos prontos (0,7 a 200 de fábrica) desenhados como bolinhas proporcionais ao
//! tamanho (com teto, para caber na célula) e o número embaixo; o tamanho em uso fica destacado e o clique escolhe. A
//! lista é do usuário (lápis no rodapé). Ferramenta sem tamanho (balde, seleção, texto...): a paleta continua no lugar,
//! cinza e sem resposta, para o layout não pular.

use crate::app::shelf;
use crate::app::ui_kit::{Env, body_ui};
use crate::drawing_prefs::{DEFAULT_SIZES, format_size, parse_sizes, sizes_text};
use crate::i18n::tr;
use crate::icons::IconId;
use crate::workspace::ToolId;
use egui::{Rect, Sense, Stroke, StrokeKind, Ui};

#[derive(Default)]
pub(crate) struct SizeUi {
    /// Editando a lista (texto) e o erro da última tentativa.
    editing: Option<String>,
    error: Option<String>,
}

/// Largura mínima de uma célula da grade.
const CELL_W: f32 = 38.0;

/// O tamanho em uso da ferramenta (px), se ela tem tamanho.
pub(crate) fn current_size(env: &Env) -> Option<f32> {
    let has = shelf::current_sub(env).map(|s| s.has_size()).unwrap_or(matches!(*env.tool, ToolId::Brush | ToolId::Pen | ToolId::Eraser));
    if !has {
        return None;
    }
    let o = &env.ed.opts;
    Some(match *env.tool {
        ToolId::Pen => o.pen_width,
        ToolId::Eraser => o.eraser_size,
        _ => o.brush_size,
    })
}

/// Põe o tamanho na ferramenta (dentro dos limites dela).
pub(crate) fn set_size(env: &mut Env, s: f32) {
    let o = &mut env.ed.opts;
    match *env.tool {
        ToolId::Pen => o.pen_width = s.clamp(0.5, 200.0),
        ToolId::Eraser => o.eraser_size = s.clamp(0.5, 600.0),
        ToolId::Brush => o.brush_size = s.clamp(0.5, 2000.0),
        _ => {}
    }
}

/// Raio da bolinha de um tamanho na célula: o tamanho de verdade (diâmetro em px) até o teto da célula; mínimo visível.
pub(crate) fn dot_radius(size: f32, cap: f32) -> f32 {
    (size / 2.0).clamp(0.9, cap.max(1.0))
}

/// Índice do tamanho da lista que está em uso (diferença menor que 3%).
pub(crate) fn active_index(sizes: &[f32], cur: f32) -> Option<usize> {
    sizes.iter().position(|s| (s - cur).abs() <= (0.03 * s.max(cur)).max(0.05))
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let t = env.t;
    let cur = current_size(env);
    let enabled = cur.is_some();
    let sizes = env.ed.draw_ui.prefs.sizes.clone();
    let active = cur.and_then(|c| active_index(&sizes, c));
    let footer_h = if env.ed.draw_ui.sizes.editing.is_some() { 64.0 } else { 26.0 };
    let max_h = (ui.available_height() - footer_h).max(CELL_W);
    let mut pick: Option<f32> = None;
    let r = ui.add_enabled_ui(enabled, |ui| {
        egui::ScrollArea::vertical().id_salt("brush_size_scroll").max_height(max_h).auto_shrink([false, false]).show(ui, |ui| {
            let w = ui.available_width();
            let cols = ((w / CELL_W).floor() as usize).max(1);
            let cw = w / cols as f32;
            let ch = (cw * 1.08).clamp(34.0, 52.0);
            let cap = (cw.min(ch - 14.0) / 2.0 - 3.0).max(2.0);
            for (row, chunk) in sizes.chunks(cols).enumerate() {
                let (rr, _) = ui.allocate_exact_size(egui::vec2(w, ch), Sense::hover());
                for (k, s) in chunk.iter().enumerate() {
                    let i = row * cols + k;
                    let cell = Rect::from_min_size(egui::pos2(rr.left() + k as f32 * cw, rr.top()), egui::vec2(cw, ch)).shrink(1.0);
                    let resp = ui.interact(cell, ui.id().with(("size_cell", i)), if enabled { Sense::click() } else { Sense::hover() });
                    env.note(format!("size:{i}"), cell);
                    let on = active == Some(i);
                    let p = ui.painter();
                    if on {
                        p.rect(cell, 3.0, t.accent_dim, Stroke::new(1.0, t.accent), StrokeKind::Inside);
                    } else if resp.hovered() && enabled {
                        p.rect_filled(cell, 3.0, t.bg_hover);
                    }
                    let ink = if !enabled { t.text_dim.gamma_multiply(0.45) } else if on { t.text } else { t.text_dim };
                    let c = egui::pos2(cell.center().x, cell.top() + (cell.height() - 13.0) / 2.0 + 1.0);
                    p.circle_filled(c, dot_radius(*s, cap), ink);
                    p.text(egui::pos2(cell.center().x, cell.bottom() - 1.0), egui::Align2::CENTER_BOTTOM, format_size(*s), egui::FontId::proportional(10.5), ink);
                    if resp.on_hover_text(format!("{} px", format_size(*s))).clicked() && enabled {
                        pick = Some(*s);
                    }
                }
            }
        });
    });
    if !enabled {
        r.response.on_hover_text(tr("bsize.disabled"));
    }
    if let Some(s) = pick {
        set_size(env, s);
    }
    // ---- rodapé: o tamanho em uso e o lápis que edita a lista
    ui.add_space(2.0);
    let editing = env.ed.draw_ui.sizes.editing.clone();
    match editing {
        None => {
            ui.horizontal(|ui| {
                let txt = match cur {
                    Some(c) => format!("{} px", format_size(c)),
                    None => "—".to_owned(),
                };
                ui.label(egui::RichText::new(txt).size(11.5).color(if enabled { t.text } else { t.text_dim }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if env.icon_button(ui, IconId::ToolPencil, "bsize.edit", false, true, "size:edit").clicked() {
                        env.ed.draw_ui.sizes.editing = Some(sizes_text(&sizes));
                        env.ed.draw_ui.sizes.error = None;
                    }
                });
            });
        }
        Some(mut text) => {
            let te = ui.add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY).hint_text(tr("bsize.edit.hint")));
            env.note("size:list", te.rect);
            let mut done = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                let b = ui.button(tr("bsize.ok"));
                env.note("size:ok", b.rect);
                done |= b.clicked();
                let b = ui.button(tr("bsize.factory"));
                env.note("size:factory", b.rect);
                if b.on_hover_text(tr("bsize.factory.tip")).clicked() {
                    text = sizes_text(&DEFAULT_SIZES);
                }
                if ui.button(tr("confirm.cancel")).clicked() {
                    env.ed.draw_ui.sizes.editing = None;
                    return;
                }
                if let Some(e) = &env.ed.draw_ui.sizes.error {
                    ui.colored_label(t.danger, format!("{}: {e}", tr("bsize.bad")));
                }
            });
            if env.ed.draw_ui.sizes.editing.is_none() {
                return;
            }
            if done {
                match parse_sizes(&text) {
                    Ok(v) => {
                        env.ed.draw_ui.prefs.sizes = v;
                        env.ed.draw_ui.sizes.editing = None;
                        env.ed.draw_ui.sizes.error = None;
                        return;
                    }
                    Err(e) => env.ed.draw_ui.sizes.error = Some(e),
                }
            }
            env.ed.draw_ui.sizes.editing = Some(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_follow_the_size_up_to_the_cell() {
        assert!(dot_radius(0.7, 14.0) >= 0.9, "o menor tamanho ainda aparece");
        assert!((dot_radius(10.0, 14.0) - 5.0).abs() < 1e-6, "proporcional: diâmetro = tamanho");
        assert_eq!(dot_radius(200.0, 14.0), 14.0, "com teto");
        let mut last = 0.0;
        for s in DEFAULT_SIZES {
            let r = dot_radius(s, 14.0);
            assert!(r >= last, "nunca diminui");
            last = r;
        }
    }

    #[test]
    fn the_size_in_use_is_highlighted() {
        let v = DEFAULT_SIZES.to_vec();
        assert_eq!(active_index(&v, 10.0), Some(11));
        assert_eq!(active_index(&v, 10.2), Some(11), "3% de folga");
        assert_eq!(active_index(&v, 11.0), None);
        assert_eq!(active_index(&v, 0.7), Some(0));
        assert_eq!(active_index(&v, 200.0), Some(v.len() - 1));
    }
}
