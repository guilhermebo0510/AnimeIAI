//! Painel Camadas: a árvore do corte ativo (de cima para baixo, pastas indentadas), olho/cadeado, renomear com duplo clique,
//! menu de nova camada, apagar, subir/descer, opacidade, modo de mistura, ligação do balde às camadas de linha e a lista de
//! cels da camada ativa (número, papel; novo e duplicar). Tudo por comandos: cada ajuste é um passo de desfazer.

use super::commit_on_release;
use crate::app::commands::{self as cmds, BLEND_MODES, LayerDrop, LayerRow, NodeId, blend_key};
use crate::app::ui_kit::{Env, body_ui, dim_label, paint_icon};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::*;
use egui::{Align2, FontId, Id, Rect, Sense, Stroke, Ui};
use serde_json::json;

pub(crate) const ADD_KINDS: [LayerType; 9] = [
    LayerType::Vector,
    LayerType::Raster,
    LayerType::Text,
    LayerType::Fill,
    LayerType::Group,
    LayerType::Guide,
    LayerType::Reference,
    LayerType::Effect,
    LayerType::Particles,
];

pub(crate) fn kind_icon(k: LayerType) -> IconId {
    match k {
        LayerType::Group => IconId::LayerGroup,
        LayerType::Vector => IconId::LayerVector,
        LayerType::Raster => IconId::LayerRaster,
        LayerType::Fill => IconId::LayerFill,
        LayerType::Text => IconId::LayerText,
        LayerType::Guide => IconId::LayerGuide,
        LayerType::Reference => IconId::LayerReference,
        LayerType::Adjustment => IconId::LayerAdjustment,
        LayerType::Effect => IconId::LayerEffect,
        LayerType::Particles => IconId::LayerParticles,
    }
}

fn add_kind_key(k: LayerType) -> &'static str {
    cmds::layer_type_key(k)
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let Some(cut) = env.ed.active_cut(env.core) else {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(tr("layers.no_cut")).color(t.text_dim));
        return;
    };
    let active = env.ed.active_layer(env.core);
    let rows = cmds::layer_rows(&env.core.project, &cut);
    env.ed.layer_sel.retain(|id| rows.iter().any(|r| &r.id == id));
    // as escolhidas (de cima para baixo), com a ativa
    let chosen: Vec<NodeId> = rows.iter().filter(|r| Some(&r.id) == active.as_ref() || env.ed.layer_sel.contains(&r.id)).map(|r| r.id.clone()).collect();
    // Delete com o mouse no painel apaga as escolhidas (no canvas, Delete é do Editar pontos)
    let del_key = ui.rect_contains_pointer(body) && !ui.ctx().egui_wants_keyboard_input() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete));
    let group_key = ui.rect_contains_pointer(body) && !ui.ctx().egui_wants_keyboard_input() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::G));

    // ---- barra de ações
    ui.horizontal(|ui| {
        let add = env.icon_button(ui, IconId::LayerAdd, "layers.add", false, true, "layers:add");
        egui::Popup::menu(&add).show(|ui| {
            for kind in ADD_KINDS {
                let tex = env.icons.texture(ui.ctx(), kind_icon(kind), 16.0);
                let img = egui::Image::new((tex, egui::vec2(16.0, 16.0))).tint(t.text_dim);
                let item = ui.add(egui::Button::image_and_text(img, tr(add_kind_key(kind))));
                env.note(format!("layers:add.{}", kind_id(kind)), item.rect);
                if item.clicked() {
                    match cmds::add_layer(env.core, &cut, kind, active.as_ref()) {
                        Ok(id) => {
                            env.ed.layer = Some(id);
                            env.ed.cel_number = None;
                        }
                        Err(e) => env.ed.notify(e),
                    }
                    ui.close();
                }
            }
        });
        let has = active.is_some();
        let up = env.icon_button(ui, IconId::ArrowUp, "layers.up", false, has, "layers:up");
        if up.clicked() {
            if let Some(a) = &active {
                if let Err(e) = cmds::move_layer(env.core, a, 1) {
                    env.ed.notify(e);
                }
            }
        }
        let down = env.icon_button(ui, IconId::ArrowDown, "layers.down", false, has, "layers:down");
        if down.clicked() {
            if let Some(a) = &active {
                if let Err(e) = cmds::move_layer(env.core, a, -1) {
                    env.ed.notify(e);
                }
            }
        }
        let del = env.icon_button(ui, IconId::Trash, "layers.delete", false, has, "layers:delete");
        if (del.clicked() || del_key) && !chosen.is_empty() {
            // a próxima ativa: a primeira linha abaixo que não sai
            let last = rows.iter().rposition(|r| chosen.contains(&r.id)).unwrap_or(0);
            let next = rows.iter().skip(last + 1).chain(rows.iter().take(last).rev()).find(|r| !chosen.contains(&r.id) && !chosen.iter().any(|c| env.core.project.subtree(c).contains(&r.id))).map(|r| r.id.clone());
            match cmds::delete_layers(env.core, &chosen) {
                Ok(()) => {
                    env.ed.layer = next;
                    env.ed.layer_sel.clear();
                    env.ed.cel_number = None;
                }
                Err(e) => env.ed.notify(e),
            }
        }
        // agrupar: pasta nova no lugar da de cima, com as escolhidas dentro (Ctrl+G)
        let grp = env.icon_button(ui, IconId::LayerGroup, "layers.group", false, has, "layers:group");
        if (grp.clicked() || group_key) && !chosen.is_empty() {
            match cmds::add_layer(env.core, &cut, LayerType::Group, chosen.first()) {
                Ok(g) => {
                    // a pasta nasce acima da de cima (ou dentro dela, se a de cima for pasta: aí entra ao lado)
                    if let Err(e) = cmds::move_layers(env.core, &chosen, &LayerDrop::Into(g.clone())) {
                        env.ed.notify(e);
                    }
                    env.ed.layer = Some(g);
                    env.ed.layer_sel.clear();
                }
                Err(e) => env.ed.notify(e),
            }
        }
        // mover para pasta: lista as pastas do corte
        let groups: Vec<(NodeId, String)> = rows.iter().filter(|r| cmds::layer_data(&env.core.project, &r.id).is_some_and(|l| l.kind == LayerType::Group)).map(|r| (r.id.clone(), env.core.project.node(&r.id).map(|n| n.name.clone()).unwrap_or_default())).collect();
        let into = env.icon_button(ui, IconId::ChevronRight, "layers.move_to_group", false, has && !groups.is_empty(), "layers:to_group");
        egui::Popup::menu(&into).show(|ui| {
            if ui.button(tr("layers.to_cut")).clicked() {
                let mut res = Ok(());
                for a in &chosen {
                    res = res.and(cmds::move_layer_to(env.core, a, &cut));
                }
                if let Err(e) = res {
                    env.ed.notify(e);
                }
                ui.close();
            }
            for (gid, name) in &groups {
                if !chosen.contains(gid) && ui.button(name).clicked() {
                    if let Err(e) = cmds::move_layers(env.core, &chosen, &LayerDrop::Into(gid.clone())) {
                        env.ed.notify(e);
                    }
                    ui.close();
                }
            }
        });
    });
    ui.add_space(2.0);

    // ---- tudo rola junto num painel pequeno; a lista tem altura própria (até ~45% do painel) com rolagem interna
    let total_h = ui.available_height();
    let list_h = ((rows.len().max(1) as f32) * 27.0 + 4.0).clamp(54.0, (total_h * 0.55).max(54.0));
    egui::ScrollArea::vertical().id_salt("layers_outer").auto_shrink([false, false]).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("layers_list").max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            if rows.is_empty() {
                ui.label(egui::RichText::new(tr("panel.layers.empty")).color(t.text_dim));
            }
            let mut rects = Vec::with_capacity(rows.len());
            for (i, row) in rows.iter().enumerate() {
                rects.push(layer_row(env, ui, i, row, active.as_ref(), &rows, &chosen));
            }
            drag_and_drop(env, ui, &rows, &rects);
        });
        ui.add_space(2.0);
        ui.separator();
        if let Some(a) = active {
            layer_settings(env, ui, &cut, &a, &rows);
            cel_list(env, ui, &a);
        }
    });
}

pub(crate) fn kind_id(k: LayerType) -> &'static str {
    match k {
        LayerType::Group => "group",
        LayerType::Vector => "vector",
        LayerType::Raster => "raster",
        LayerType::Fill => "fill",
        LayerType::Text => "text",
        LayerType::Guide => "guide",
        LayerType::Reference => "reference",
        LayerType::Adjustment => "adjustment",
        LayerType::Effect => "effect",
        LayerType::Particles => "particles",
    }
}

fn layer_row(env: &mut Env, ui: &mut Ui, i: usize, row: &LayerRow, active: Option<&NodeId>, rows: &[LayerRow], chosen: &[NodeId]) -> Rect {
    let t = env.t;
    let h = 26.0;
    let Some(node) = env.core.project.node(&row.id) else { return Rect::NOTHING };
    let Some(l) = cmds::layer_data(&env.core.project, &row.id).cloned() else { return Rect::NOTHING };
    let name = node.name.clone();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click_and_drag());
    env.note(format!("layers:row:{i}"), rect);
    let is_active = active == Some(&row.id);
    let is_chosen = chosen.contains(&row.id);
    let p = ui.painter();
    if is_active {
        p.rect_filled(rect, 4.0, t.accent_dim);
        p.rect_filled(Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 3.0), egui::vec2(3.0, h - 6.0)), 1.5, t.accent);
    } else if is_chosen {
        p.rect_filled(rect, 4.0, t.accent_dim.gamma_multiply(0.6));
    } else if resp.hovered() {
        p.rect_filled(rect, 4.0, t.bg_hover);
    }
    // camada recortada na de baixo: seta dobrada e nome recuado (como no Photoshop)
    let clip_in = if l.clip_to_below { 12.0 } else { 0.0 };
    let indent = 8.0 + row.depth as f32 * 14.0 + clip_in;
    if l.clip_to_below {
        let x = rect.left() + indent - 8.0;
        let (a, b, c) = (egui::pos2(x, rect.top() + 6.0), egui::pos2(x, rect.center().y + 3.0), egui::pos2(x + 6.0, rect.center().y + 3.0));
        p.line_segment([a, b], Stroke::new(1.3, t.text_dim));
        p.line_segment([b, c], Stroke::new(1.3, t.text_dim));
        p.line_segment([c, c + egui::vec2(-3.0, -3.0)], Stroke::new(1.3, t.text_dim));
        p.line_segment([c, c + egui::vec2(-3.0, 3.0)], Stroke::new(1.3, t.text_dim));
    }
    let ink = if l.visible { t.text } else { t.text_dim.gamma_multiply(0.7) };
    let icon_rect = Rect::from_center_size(egui::pos2(rect.left() + indent + 8.0, rect.center().y), egui::vec2(15.0, 15.0));
    paint_icon(env.icons, p, kind_icon(l.kind), icon_rect, if is_active { t.accent } else { t.text_dim }, false);
    // olho e cadeado à direita
    let (eye_r, lock_r) = (
        Rect::from_center_size(egui::pos2(rect.right() - 36.0, rect.center().y), egui::vec2(20.0, 20.0)),
        Rect::from_center_size(egui::pos2(rect.right() - 12.0, rect.center().y), egui::vec2(20.0, 20.0)),
    );
    let name_rect = Rect::from_min_max(egui::pos2(rect.left() + indent + 22.0, rect.top()), egui::pos2(eye_r.left() - 4.0, rect.bottom()));
    let renaming = env.ed.rename.as_ref().is_some_and(|(id, _)| id == &row.id);
    if renaming {
        let mut commit = false;
        let mut cancel = false;
        if let Some((_, buf)) = env.ed.rename.as_mut() {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(name_rect.shrink2(egui::vec2(0.0, 2.0))));
            let te = child.add(egui::TextEdit::singleline(buf).desired_width(name_rect.width()).margin(egui::vec2(4.0, 2.0)));
            env.notes.push(("layers:rename".into(), te.rect));
            if !te.has_focus() && !te.lost_focus() {
                te.request_focus();
            }
            if te.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                } else {
                    commit = true;
                }
            }
        }
        if commit || cancel {
            if let Some((id, buf)) = env.ed.rename.take() {
                if commit {
                    if let Err(e) = cmds::rename_node(env.core, &id, &buf) {
                        env.ed.notify(e);
                    }
                }
            }
        }
    } else {
        ui.painter().with_clip_rect(name_rect).text(egui::pos2(name_rect.left(), rect.center().y), Align2::LEFT_CENTER, &name, FontId::proportional(13.0), ink);
    }
    // alvos de clique (depois da linha, ficam por cima dela)
    let eye = ui.interact(eye_r, Id::new(("layer_eye", &row.id)), Sense::click());
    let lock = ui.interact(lock_r, Id::new(("layer_lock", &row.id)), Sense::click());
    env.note(format!("layers:eye:{i}"), eye_r);
    env.note(format!("layers:lock:{i}"), lock_r);
    let painter = ui.painter();
    if eye.hovered() {
        painter.rect_filled(eye_r, 3.0, t.bg_hover);
    }
    if lock.hovered() {
        painter.rect_filled(lock_r, 3.0, t.bg_hover);
    }
    paint_icon(env.icons, painter, if l.visible { IconId::Eye } else { IconId::EyeOff }, eye_r.shrink(3.0), if l.visible { t.text } else { t.text_dim.gamma_multiply(0.8) }, false);
    paint_icon(env.icons, painter, if l.locked { IconId::Lock } else { IconId::Unlock }, lock_r.shrink(3.0), if l.locked { t.accent } else { t.text_dim.gamma_multiply(0.55) }, false);
    if eye.on_hover_text(tr("layers.visible")).clicked() {
        if let Err(e) = cmds::set_layer_props(env.core, &row.id, json!({ "visible": !l.visible })) {
            env.ed.notify(e);
        }
    } else if lock.on_hover_text(tr("layers.locked")).clicked() {
        if let Err(e) = cmds::set_layer_props(env.core, &row.id, json!({ "locked": !l.locked })) {
            env.ed.notify(e);
        }
    } else if resp.double_clicked() {
        env.ed.layer = Some(row.id.clone());
        env.ed.rename = Some((row.id.clone(), name));
    } else if resp.clicked() {
        let m = ui.input(|i| i.modifiers);
        if m.command {
            // Ctrl+clique: soma ou tira da escolha (a ativa continua)
            if is_active {
                // tirar a ativa: a próxima escolhida assume
                if let Some(n) = env.ed.layer_sel.first().cloned() {
                    env.ed.layer_sel.retain(|x| x != &n);
                    env.ed.layer = Some(n);
                }
            } else if let Some(k) = env.ed.layer_sel.iter().position(|x| x == &row.id) {
                env.ed.layer_sel.remove(k);
            } else {
                env.ed.layer_sel.push(row.id.clone());
            }
        } else if m.shift {
            // Shift+clique: do ativo até aqui
            let a = active.and_then(|a| rows.iter().position(|r| &r.id == a)).unwrap_or(i);
            let (lo, hi) = (a.min(i), a.max(i));
            env.ed.layer_sel = rows[lo..=hi].iter().map(|r| r.id.clone()).collect();
            env.ed.layer = Some(row.id.clone());
        } else {
            if env.ed.layer.as_ref() != Some(&row.id) {
                env.ed.cel_number = None;
            }
            env.ed.layer_sel.clear();
            env.ed.layer = Some(row.id.clone());
        }
    }
    if resp.drag_started() {
        // arrasta as escolhidas se a linha é uma delas; senão, só ela
        let ids: Vec<NodeId> = if is_chosen { chosen.to_vec() } else { vec![row.id.clone()] };
        env.ed.layer_drag = Some(ids);
    }
    rect
}

/// Onde a camada arrastada cai: no terço de cima de uma linha = acima dela; no de baixo = abaixo; no meio de uma pasta
/// = dentro dela. Desenha a marca e, ao soltar, move todas (um passo de desfazer).
fn drag_and_drop(env: &mut Env, ui: &mut Ui, rows: &[LayerRow], rects: &[Rect]) {
    let Some(dragging) = env.ed.layer_drag.clone() else { return };
    let t = env.t;
    let pos = ui.input(|i| i.pointer.interact_pos());
    let released = ui.input(|i| i.pointer.any_released());
    let target = pos.and_then(|p| {
        let k = rects.iter().position(|r| r.y_range().contains(p.y) && p.x >= r.left() - 20.0 && p.x <= r.right() + 20.0)?;
        let r = rects[k];
        let id = rows[k].id.clone();
        let group = cmds::layer_data(&env.core.project, &id).is_some_and(|l| l.kind == LayerType::Group);
        let f = (p.y - r.top()) / r.height().max(1.0);
        Some((k, if group && (0.3..0.7).contains(&f) { LayerDrop::Into(id) } else if f < 0.5 { LayerDrop::Above(id) } else { LayerDrop::Below(id) }))
    });
    let valid = |d: &LayerDrop| match d {
        LayerDrop::Above(a) | LayerDrop::Below(a) | LayerDrop::Into(a) => !dragging.contains(a),
    };
    if let Some((k, d)) = &target {
        if valid(d) {
            let r = rects[*k];
            let p = ui.painter();
            match d {
                LayerDrop::Into(_) => {
                    p.rect_stroke(r.shrink(1.0), 4.0, Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
                }
                LayerDrop::Above(_) => {
                    p.hline(r.x_range(), r.top(), Stroke::new(2.0, t.accent));
                }
                LayerDrop::Below(_) => {
                    p.hline(r.x_range(), r.bottom(), Stroke::new(2.0, t.accent));
                }
            }
        }
        if let Some(pp) = pos {
            // etiqueta com quantas camadas vão
            let txt = if dragging.len() > 1 { format!("{} {}", dragging.len(), tr("layers.n_layers")) } else { env.core.project.node(&dragging[0]).map(|n| n.name.clone()).unwrap_or_default() };
            ui.painter().text(pp + egui::vec2(14.0, 0.0), Align2::LEFT_CENTER, txt, FontId::proportional(12.0), t.accent);
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if released {
        env.ed.layer_drag = None;
        if let Some((_, d)) = target.filter(|(_, d)| valid(d)) {
            if let Err(e) = cmds::move_layers(env.core, &dragging, &d) {
                env.ed.notify(e);
            }
        }
    }
}

fn layer_settings(env: &mut Env, ui: &mut Ui, cut: &NodeId, layer: &NodeId, rows: &[LayerRow]) {
    let t = env.t;
    let Some(l) = cmds::layer_data(&env.core.project, layer).cloned() else { return };
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("layers.opacity"));
        ui.spacing_mut().slider_width = (ui.available_width() - 70.0).clamp(50.0, 220.0);
        let cur = (l.opacity * 100.0).round();
        let r = commit_on_release(ui, Id::new(("opacity", layer)), cur, |ui, v| {
            let r = ui.add(egui::Slider::new(v, 0.0..=100.0).suffix("%").max_decimals(0).trailing_fill(true));
            env.notes.push(("layers:opacity".into(), r.rect));
            r
        });
        if let Some(v) = r {
            if let Err(e) = cmds::set_layer_props(env.core, layer, json!({ "opacity": (v / 100.0).clamp(0.0, 1.0) })) {
                env.ed.notify(e);
            }
        }
    });
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("layers.blend"));
        let mut blend = l.blend;
        let combo = egui::ComboBox::from_id_salt("layer_blend").width(120.0).selected_text(tr(blend_key(blend))).show_ui(ui, |ui| {
            for b in BLEND_MODES {
                if b == BlendMode::PassThrough && l.kind != LayerType::Group {
                    continue;
                }
                let item = ui.selectable_value(&mut blend, b, tr(blend_key(b)));
                env.notes.push((format!("layers:blend.{}", serde_json::to_value(b).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()), item.rect));
            }
        });
        env.notes.push(("layers:blend".into(), combo.response.rect));
        if blend != l.blend {
            if let Err(e) = cmds::set_layer_props(env.core, layer, json!({ "blend": blend })) {
                env.ed.notify(e);
            }
        }
        let mut clip = l.clip_to_below;
        let r = ui.checkbox(&mut clip, tr("layers.clip.short"));
        env.note("layers:clip", r.rect);
        if r.on_hover_text(tr("layers.clip")).changed() {
            if let Err(e) = cmds::set_layer_props(env.core, layer, json!({ "clip_to_below": clip })) {
                env.ed.notify(e);
            }
        }
    });
    // com recorte: a camada só aparece onde a de baixo tem pintura; avisa quando a de baixo não mostra nada
    if l.clip_to_below {
        let below = rows.iter().position(|r| &r.id == layer).and_then(|i| {
            let depth = rows[i].depth;
            rows.iter().skip(i + 1).take_while(|r| r.depth >= depth).find(|r| r.depth == depth)
        });
        let why = match below.and_then(|b| cmds::layer_data(&env.core.project, &b.id).map(|d| (b, d))) {
            None => Some(tr("layers.clip.none_below")),
            Some((_, d)) if !d.visible => Some(tr("layers.clip.below_hidden")),
            Some((b, d)) if d.kind.holds_cels() && d.kind != LayerType::Text && env.ed.cut.as_ref().and_then(|c| cmds::exposed_number(&env.core.project, c, &b.id, env.ed.frame)).is_none() => Some(tr("layers.clip.below_empty")),
            _ => None,
        };
        if let Some(w) = why {
            ui.label(egui::RichText::new(w).size(11.5).color(t.st_retake));
        }
    }
    if l.kind == LayerType::Fill {
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr("layers.line_refs"));
            let candidates: Vec<(NodeId, String)> = rows
                .iter()
                .filter(|r| r.id != *layer && cmds::layer_data(&env.core.project, &r.id).is_some_and(|c| matches!(c.kind, LayerType::Vector | LayerType::Raster)))
                .map(|r| (r.id.clone(), env.core.project.node(&r.id).map(|n| n.name.clone()).unwrap_or_default()))
                .collect();
            let label = if l.line_refs.is_empty() {
                tr("layers.line_refs.none").to_owned()
            } else {
                l.line_refs.iter().filter_map(|id| env.core.project.node(id).map(|n| n.name.clone())).collect::<Vec<_>>().join(", ")
            };
            let _ = cut;
            let b = ui.add(egui::Button::new(label));
            env.note("layers:line_refs", b.rect);
            egui::Popup::menu(&b).show(|ui| {
                let mut refs = l.line_refs.clone();
                let mut changed = false;
                if candidates.is_empty() {
                    ui.label(tr("layers.line_refs.empty"));
                }
                for (id, name) in &candidates {
                    let mut on = refs.contains(id);
                    let r = ui.checkbox(&mut on, name);
                    env.notes.push((format!("layers:line_ref.{}", name), r.rect));
                    if r.changed() {
                        changed = true;
                        if on {
                            refs.push(id.clone());
                        } else {
                            refs.retain(|x| x != id);
                        }
                    }
                }
                if changed {
                    let _ = cmds::set_layer_props(env.core, layer, json!({ "line_refs": refs }));
                }
            });
        });
    }
}

fn cel_list(env: &mut Env, ui: &mut Ui, layer: &NodeId) {
    let t = env.t;
    let Some(l) = cmds::layer_data(&env.core.project, layer) else { return };
    // a camada de texto guarda o texto em params, sem cels
    if !l.kind.holds_cels() || l.kind == LayerType::Text {
        return;
    }
    let cels = cmds::layer_cels(&env.core.project, layer);
    let exposed = env.ed.cut.as_ref().and_then(|c| cmds::exposed_number(&env.core.project, c, layer, env.ed.frame));
    let selected = env.ed.cel_number.or(exposed);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("layers.cels")).size(11.5).color(t.text_dim).strong());
        let add = env.icon_button(ui, IconId::Plus, "layers.add_cel", false, true, "cels:add");
        if add.clicked() {
            match cmds::add_cel(env.core, layer, None) {
                Ok(n) => env.ed.cel_number = Some(n),
                Err(e) => env.ed.notify(e),
            }
        }
        let dup = env.icon_button(ui, IconId::Copy, "layers.duplicate_cel", false, selected.is_some(), "cels:duplicate");
        if dup.clicked() {
            match cmds::add_cel(env.core, layer, selected) {
                Ok(n) => env.ed.cel_number = Some(n),
                Err(e) => env.ed.notify(e),
            }
        }
        // a coluna da direita é o papel do desenho na animação (não um botão da linha)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(30.0);
            ui.label(egui::RichText::new(tr("layers.cel_role")).size(11.0).color(t.text_dim)).on_hover_text(tr("layers.cel_role.tip"));
        });
    });
    ui.spacing_mut().item_spacing.y = 2.0;
    for (n, id) in cels {
        let role = cmds::cel_data(&env.core.project, &id).map(|c| c.role).unwrap_or_default();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::click());
        env.note(format!("cels:row:{n}"), rect);
        let is_sel = selected == Some(n);
        let p = ui.painter();
        if is_sel {
            p.rect(rect, 4.0, t.accent_dim, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            p.rect_filled(rect, 4.0, t.bg_hover);
        }
        let name_r = p.text(egui::pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, format!("{} {n}", tr("layers.cel")), FontId::proportional(13.0), t.text);
        if exposed == Some(n) {
            let dot = egui::pos2(name_r.right() + 8.0, rect.center().y);
            p.circle_filled(dot, 3.0, t.accent);
            ui.interact(Rect::from_center_size(dot, egui::vec2(12.0, 12.0)), ui.id().with(("cel_exposed", n)), Sense::hover()).on_hover_text(tr("layers.cel_exposed"));
        }
        // papel do cel: botão pequeno dentro da linha (à direita, sem fundo próprio), menu ao clicar
        let label = tr(cmds::cel_role_key(role));
        let g = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(11.5), t.text_dim);
        let role_rect = Rect::from_min_max(egui::pos2(rect.right() - g.size().x - 24.0, rect.top() + 3.0), egui::pos2(rect.right() - 4.0, rect.bottom() - 3.0));
        let rr = ui.interact(role_rect, ui.id().with(("cel_role", n)), Sense::click());
        env.note(format!("cels:role:{n}"), role_rect);
        if rr.hovered() {
            ui.painter().rect_filled(role_rect, 3.0, t.bg_hover);
        }
        ui.painter().galley(egui::pos2(role_rect.left() + 5.0, role_rect.center().y - g.size().y / 2.0), g, t.text_dim);
        // seta do menu (desenhada: a fonte não tem o caractere)
        let c = egui::pos2(role_rect.right() - 8.0, role_rect.center().y);
        ui.painter().add(egui::Shape::convex_polygon(vec![c + egui::vec2(-3.5, -1.5), c + egui::vec2(3.5, -1.5), c + egui::vec2(0.0, 2.5)], t.text_dim, Stroke::NONE));
        let mut new_role = role;
        egui::Popup::menu(&rr).show(|ui| {
            for r in [CelRole::Key, CelRole::Breakdown, CelRole::Inbetween] {
                if ui.selectable_label(role == r, tr(cmds::cel_role_key(r))).clicked() {
                    new_role = r;
                    ui.close();
                }
            }
        });
        rr.on_hover_text(tr("layers.cel_role.tip"));
        if new_role != role {
            if let Err(e) = cmds::set_cel_role(env.core, &id, new_role) {
                env.ed.notify(e);
            }
        }
        if resp.clicked() {
            env.ed.cel_number = Some(n);
        }
    }
}
