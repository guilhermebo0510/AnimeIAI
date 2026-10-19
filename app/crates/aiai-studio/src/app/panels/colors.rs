//! Painel Cores: a cor atual (direta ou referência de paleta), as paletas do projeto (criar), as amostras (clicar = cor
//! atual; ela fica ligada à paleta, então trocar a cor na paleta recolore tudo que a usa) e a edição de nome/cor da
//! amostra por `ReplaceData` na paleta (um passo de desfazer por ajuste, aplicado quando o ponteiro solta).

use crate::app::commands::{self as cmds, NodeId};
use crate::app::ui_kit::{Env, body_ui, color32_to_rgba, dim_label, rgba_to_color32};
use crate::i18n::tr;
use aiai_core::*;
use egui::{Color32, Rect, Sense, Stroke, StrokeKind, Ui};

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    egui::ScrollArea::vertical().id_salt("colors_scroll").auto_shrink([false, false]).show(ui, |ui| {
        current_color(env, ui);
        ui.add_space(2.0);
        palette_picker(env, ui);
        match env.ed.palette.clone() {
            None => {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(tr("panel.palette.empty")).color(t.text_dim));
            }
            Some(pal) => {
                swatches(env, ui, &pal);
                edit_selected(env, ui, &pal);
            }
        }
        ui.add_space(4.0);
        color_wheel(env, ui);
    });
}

/// Seletor fixo (quadrado saturação × valor + faixa de matiz) para a cor direta, sempre à mão como nos programas de
/// pintura; recolhível.
fn color_wheel(env: &mut Env, ui: &mut Ui) {
    let rgba = cmds::resolve_color(&env.core.project, &env.ed.color);
    let mut c32 = rgba_to_color32(Rgba([rgba.0[0], rgba.0[1], rgba.0[2], 1.0]));
    let r = egui::CollapsingHeader::new(tr("colors.wheel")).id_salt("colors_wheel").default_open(true).show(ui, |ui| {
        ui.spacing_mut().slider_width = (ui.available_width() - 40.0).clamp(120.0, 260.0);
        let changed = egui::color_picker::color_picker_color32(ui, &mut c32, egui::color_picker::Alpha::Opaque);
        if changed {
            env.ed.set_color_direct(color32_to_rgba(c32));
        }
    });
    env.note("colors:wheel", r.header_response.rect);
}

fn current_color(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let rgba = cmds::resolve_color(&env.core.project, &env.ed.color);
    let c32 = rgba_to_color32(Rgba([rgba.0[0], rgba.0[1], rgba.0[2], 1.0]));
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), Sense::hover());
        env.note("colors:current", r);
        ui.painter().rect(r, 6.0, c32, Stroke::new(1.5, t.border), StrokeKind::Inside);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            let (label, tip) = match &env.ed.color {
                ColorRef::Direct { .. } => (tr("palette.direct").to_owned(), String::new()),
                ColorRef::Palette { palette, color } => {
                    let name = cmds::palette_data(&env.core.project, palette).and_then(|p| p.color(color)).map(|c| c.name.clone()).unwrap_or_default();
                    (format!("{}: {name}", tr("palette.linked")), tr("palette.linked.tip").to_owned())
                }
            };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).size(13.0)).on_hover_text(tip);
                ui.label(egui::RichText::new(rgba.to_hex()[..7].to_uppercase()).monospace().size(12.0).color(t.text_dim));
            });
            ui.horizontal(|ui| {
                let mut edit = c32;
                let r = egui::color_picker::color_edit_button_srgba(ui, &mut edit, egui::color_picker::Alpha::Opaque);
                env.note("colors:picker", r.rect);
                if r.changed() {
                    env.ed.set_color_direct(color32_to_rgba(edit));
                }
                r.on_hover_text(tr("opt.color.tip"));
                let has_pal = env.ed.palette.is_some();
                let add = ui.add_enabled(has_pal, egui::Button::new(tr("palette.add_to")));
                env.note("colors:add_to_palette", add.rect);
                if add.on_disabled_hover_text(tr("palette.need_palette")).clicked() {
                    if let Some(pal) = env.ed.palette.clone() {
                        match cmds::add_palette_color(env.core, &pal, rgba) {
                            Ok(id) => env.ed.color = ColorRef::Palette { palette: pal, color: id },
                            Err(e) => env.ed.notify(e),
                        }
                    }
                }
            });
        });
    });
}

fn palette_picker(env: &mut Env, ui: &mut Ui) {
    let pals: Vec<(NodeId, String)> = env.core.project.roots.palettes.iter().filter_map(|id| env.core.project.node(id).map(|n| (id.clone(), n.name.clone()))).collect();
    if env.ed.palette.as_ref().is_none_or(|p| !pals.iter().any(|(id, _)| id == p)) {
        env.ed.palette = pals.first().map(|(id, _)| id.clone());
    }
    ui.horizontal(|ui| {
        let cur = env.ed.palette.as_ref().and_then(|p| pals.iter().find(|(id, _)| id == p)).map(|(_, n)| n.clone()).unwrap_or_else(|| tr("palette.none").to_owned());
        let combo = egui::ComboBox::from_id_salt("palette_pick").width(150.0).selected_text(cur).show_ui(ui, |ui| {
            for (id, name) in &pals {
                if ui.selectable_label(env.ed.palette.as_ref() == Some(id), name).clicked() {
                    env.ed.palette = Some(id.clone());
                }
            }
        });
        env.note("colors:palette_combo", combo.response.rect);
        let b = ui.button(tr("palette.new"));
        env.note("colors:new_palette", b.rect);
        if b.clicked() {
            match cmds::add_palette(env.core) {
                Ok(id) => env.ed.palette = Some(id),
                Err(e) => env.ed.notify(e),
            }
        }
    });
}

fn swatches(env: &mut Env, ui: &mut Ui, pal: &NodeId) {
    let t = env.t;
    let Some(pd) = cmds::palette_data(&env.core.project, pal).cloned() else { return };
    ui.add_space(4.0);
    let size = 26.0;
    let mut rename_target: Option<NodeId> = None;
    let mut delete_target: Option<NodeId> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for (i, c) in pd.colors.iter().enumerate() {
            let rgba = pd.resolve(&c.id).unwrap_or(c.rgba);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
            env.note(format!("colors:swatch:{i}"), rect);
            let selected = matches!(&env.ed.color, ColorRef::Palette { palette, color } if palette == pal && *color == c.id);
            let p = ui.painter();
            p.rect_filled(rect, 4.0, rgba_to_color32(Rgba([rgba.0[0], rgba.0[1], rgba.0[2], 1.0])));
            p.rect_stroke(rect, 4.0, Stroke::new(if selected { 2.5 } else { 1.0 }, if selected { t.accent } else { t.border }), StrokeKind::Inside);
            let resp = resp.on_hover_text(format!("{}  {}", c.name, rgba.to_hex()[..7].to_uppercase()));
            if resp.clicked() {
                env.ed.color = ColorRef::Palette { palette: pal.clone(), color: c.id.clone() };
                env.ed.color_edit = None;
            }
            resp.context_menu(|ui| {
                if ui.button(tr("palette.rename")).clicked() {
                    rename_target = Some(c.id.clone());
                    ui.close();
                }
                if ui.button(tr("palette.delete_color")).clicked() {
                    delete_target = Some(c.id.clone());
                    ui.close();
                }
            });
        }
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
        env.note("colors:add_swatch", rect);
        let p = ui.painter();
        p.rect(rect, 4.0, if resp.hovered() { t.bg_hover } else { t.bg_widget }, Stroke::new(1.0, t.border), StrokeKind::Inside);
        p.text(rect.center(), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional(16.0), t.text_dim);
        if resp.on_hover_text(tr("palette.add_color")).clicked() {
            let rgba = cmds::resolve_color(&env.core.project, &env.ed.color);
            match cmds::add_palette_color(env.core, pal, rgba) {
                Ok(id) => env.ed.color = ColorRef::Palette { palette: pal.clone(), color: id },
                Err(e) => env.ed.notify(e),
            }
        }
    });
    if let Some(id) = rename_target {
        env.ed.color = ColorRef::Palette { palette: pal.clone(), color: id };
    }
    if let Some(id) = delete_target {
        let mut data = pd.clone();
        data.colors.retain(|c| c.id != id);
        match cmds::replace_palette(env.core, pal, data) {
            Ok(()) => {
                if matches!(&env.ed.color, ColorRef::Palette { color, .. } if *color == id) {
                    env.ed.color = ColorRef::Direct { rgba: Rgba::BLACK };
                }
            }
            Err(e) => env.ed.notify(e),
        }
    }
}

/// Nome e cor da amostra atualmente escolhida. A edição fica numa cópia local até o ponteiro soltar; aí vira um `ReplaceData`.
fn edit_selected(env: &mut Env, ui: &mut Ui, pal: &NodeId) {
    let t = env.t;
    let ColorRef::Palette { palette, color } = env.ed.color.clone() else { return };
    if &palette != pal {
        return;
    }
    let Some(pd) = cmds::palette_data(&env.core.project, pal).cloned() else { return };
    let Some(nc) = pd.color(&color).cloned() else { return };
    ui.add_space(2.0);
    let key = (pal.clone(), color.clone());
    let (mut name, mut c32) = match &env.ed.color_edit {
        Some((p, c, n, col)) if (p, c) == (&key.0, &key.1) => (n.clone(), *col),
        _ => (nc.name.clone(), rgba_to_color32(Rgba([nc.rgba.0[0], nc.rgba.0[1], nc.rgba.0[2], 1.0]))),
    };
    let mut touched = false;
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("palette.color_name"));
        let te = ui.add(egui::TextEdit::singleline(&mut name).desired_width(110.0));
        env.note("colors:name", te.rect);
        touched |= te.changed();
        let r = egui::color_picker::color_edit_button_srgba(ui, &mut c32, egui::color_picker::Alpha::Opaque);
        env.note("colors:edit_picker", r.rect);
        touched |= r.changed();
        ui.label(egui::RichText::new(color32_to_rgba(c32).to_hex()[..7].to_uppercase()).monospace().size(12.0).color(t.text_dim));
    });
    if touched {
        env.ed.color_edit = Some((key.0.clone(), key.1.clone(), name.clone(), c32));
    }
    // aplica quando nada está sendo arrastrado nem digitado
    let busy = ui.input(|i| i.pointer.any_down()) || ui.ctx().egui_wants_keyboard_input();
    if !busy {
        if let Some((p, c, n, col)) = env.ed.color_edit.take() {
            if (&p, &c) == (&key.0, &key.1) {
                let rgba = color32_to_rgba(col);
                let changed = n.trim() != nc.name || (0..3).any(|i| (rgba.0[i] - nc.rgba.0[i]).abs() > 0.002);
                if changed {
                    let mut data = pd.clone();
                    if let Some(entry) = data.colors.iter_mut().find(|e| e.id == c) {
                        if !n.trim().is_empty() {
                            entry.name = n.trim().to_owned();
                        }
                        entry.rgba = Rgba([rgba.0[0], rgba.0[1], rgba.0[2], nc.rgba.0[3]]);
                    }
                    if let Err(e) = cmds::replace_palette(env.core, pal, data) {
                        env.ed.notify(e);
                    }
                }
            }
        }
    }
    let _: Option<Color32> = None;
}
