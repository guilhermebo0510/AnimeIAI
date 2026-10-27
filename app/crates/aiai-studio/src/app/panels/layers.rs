//! Painel Camadas (S25, mais ícones e menos texto, à moda do Clip Studio): barra de ícones (nova raster, vetor, pasta,
//! mais tipos | recortar na de baixo, travar, referência do balde | mesclar, apagar), mistura e opacidade da ativa numa
//! linha só, e a árvore do corte ativo (de cima para baixo, pastas indentadas) com olho, cadeado, miniatura do cel
//! exposto, "100% Normal" e o nome em cada linha. Renomear com duplo clique; escolher várias com Ctrl/Shift; arrastar
//! para reordenar ou entrar numa pasta; o menu de contexto sobe/desce, agrupa, move para pasta, mescla e apaga. Embaixo,
//! o aviso do recorte, a ligação do balde às camadas de linha e a lista de cels da camada ativa (número, papel; novo e
//! duplicar). Tudo por comandos: cada ajuste é um passo de desfazer.

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
        LayerType::Model3d => IconId::LayerModel3d,
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
    let ctx = ui.ctx().clone();
    env.ed.workbar.thumbs.drain(&ctx);
    env.ed.workbar.thumbs.retain(&rows.iter().map(|r| r.id.clone()).collect());

    // ---- barra de ícones: criar (raster, vetor, pasta, mais tipos) | recortar, travar, referência do balde | mesclar, apagar
    let mut act: Option<RowAct> = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for (kind, icon, tip, name) in [(LayerType::Raster, IconId::LayerRaster, "layers.new_raster", "layers:new_raster"), (LayerType::Vector, IconId::LayerVector, "layers.new_vector", "layers:new_vector"), (LayerType::Group, IconId::LayerGroup, "layers.new_folder", "layers:new_folder")] {
            if add_button(env, ui, icon, tip, name).clicked() {
                new_layer(env, &cut, kind, active.as_ref());
            }
        }
        let add = env.icon_button(ui, IconId::LayerAdd, "layers.add", false, true, "layers:add");
        egui::Popup::menu(&add).show(|ui| add_menu(env, ui, &cut, active.as_ref()));
        bar_sep(ui, &t);
        let al = active.as_ref().and_then(|a| cmds::layer_data(&env.core.project, a).cloned());
        let has = al.is_some();
        let clip_on = al.as_ref().is_some_and(|l| l.clip_to_below);
        if env.icon_button(ui, IconId::ClipBelow, "layers.clip", clip_on, has, "layers:clip").clicked() {
            if let Some(a) = &active {
                {
                let r = cmds::set_layer_props(env.core, a, json!({ "clip_to_below": !clip_on }));
                notify(env, r);
            }
            }
        }
        let lock_on = al.as_ref().is_some_and(|l| l.locked);
        if env.icon_button(ui, IconId::Lock, "layers.lock_active", lock_on, has, "layers:lock").clicked() {
            if let Some(a) = &active {
                {
                let r = cmds::set_layer_props(env.core, a, json!({ "locked": !lock_on }));
                notify(env, r);
            }
            }
        }
        // Bloquear transparência (raster): o pincel só pinta onde já tem tinta
        let raster = al.as_ref().is_some_and(|l| l.kind == LayerType::Raster);
        let la_on = al.as_ref().is_some_and(|l| l.params.get("lock_alpha").and_then(|v| v.as_bool()).unwrap_or(false));
        if env.icon_button(ui, IconId::LockAlpha, "layers.lock_alpha", la_on, raster, "layers:lock_alpha").clicked() {
            if let (Some(a), Some(l)) = (&active, &al) {
                let mut params = if l.params.is_object() { l.params.clone() } else { json!({}) };
                params["lock_alpha"] = json!(!la_on);
                let r = cmds::set_layer_props(env.core, a, json!({ "params": params }));
                notify(env, r);
            }
        }
        let line = al.as_ref().is_some_and(|l| matches!(l.kind, LayerType::Vector | LayerType::Raster));
        let ref_on = active.as_ref().is_some_and(|a| is_fill_ref(&env.core.project, &rows, a));
        if env.icon_button(ui, IconId::LayerReference, "layers.fill_ref", ref_on, line, "layers:fill_ref").clicked() {
            if let Some(a) = &active {
                toggle_fill_ref(env, &rows, a, ref_on);
            }
        }
        bar_sep(ui, &t);
        // mesclar: a ativa com a de baixo, ou as escolhidas numa só (Ctrl+E); no menu de contexto também as visíveis
        if env.icon_button(ui, IconId::LayerMerge, "layers.merge", false, has, "layers:merge").clicked() {
            env.ed.merge_layers(env.core, false);
        }
        if env.icon_button(ui, IconId::Trash, "layers.delete", false, has, "layers:delete").clicked() {
            act = Some(RowAct::Delete);
        }
    });
    if del_key {
        act = Some(RowAct::Delete);
    }
    if group_key {
        act = Some(RowAct::Group);
    }
    // ---- mistura e opacidade da ativa, numa linha
    if let Some(a) = &active {
        blend_opacity_row(env, ui, a);
    }
    ui.add_space(2.0);

    // ---- tudo rola junto num painel pequeno; a lista fica com quase toda a altura (os cels rolam embaixo dela)
    let total_h = ui.available_height();
    let list_h = ((rows.len().max(1) as f32) * (ROW_H + 1.0) + 4.0).clamp(ROW_H + 8.0, (total_h - 64.0).max(ROW_H * 2.0 + 8.0));
    let snap = env.ed.render.as_mut().map(|r| r.snapshot(env.core));
    let idle = matches!(env.ed.gesture, crate::app::editor::Gesture::None) && !ctx.input(|i| i.pointer.any_down());
    egui::ScrollArea::vertical().id_salt("layers_outer").auto_shrink([false, false]).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("layers_list").max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            if rows.is_empty() {
                ui.label(egui::RichText::new(tr("panel.layers.empty")).color(t.text_dim));
            }
            let mut rects = Vec::with_capacity(rows.len());
            for (i, row) in rows.iter().enumerate() {
                let (r, a) = layer_row(env, ui, i, row, active.as_ref(), &rows, &chosen, &cut, snap.as_ref().filter(|_| idle));
                rects.push(r);
                if a.is_some() {
                    act = a;
                }
            }
            drag_and_drop(env, ui, &rows, &rects);
        });
        ui.add_space(2.0);
        ui.separator();
        if let Some(a) = &active {
            layer_settings(env, ui, &cut, a, &rows);
            cel_list(env, ui, a);
        }
    });
    if let Some(a) = act {
        row_action(env, a, &cut, active.as_ref(), &rows, &chosen);
    }
}

/// Altura de uma linha da lista (miniatura, "100% Normal" e o nome).
const ROW_H: f32 = 36.0;

/// Ações pedidas pela barra, pela linha (menu de contexto) ou pelo teclado, feitas depois de desenhar a lista.
#[derive(Debug, Clone, PartialEq)]
enum RowAct {
    Delete,
    Group,
    Up,
    Down,
    Into(Option<NodeId>),
    Merge(bool),
}

fn notify(env: &mut Env, r: Result<(), String>) {
    if let Err(e) = r {
        env.ed.notify(e);
    }
}

fn bar_sep(ui: &mut Ui, t: &crate::theme::Tokens) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(7.0, 20.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, t.border));
}

/// Botão de criar: o ícone do tipo com um "+" pequeno no canto.
fn add_button(env: &mut Env, ui: &mut Ui, icon: IconId, tip: &str, name: &str) -> egui::Response {
    let r = env.icon_button(ui, icon, tip, false, true, name);
    let c = r.rect.right_top() + egui::vec2(-5.5, 5.5);
    let (ink, bg) = (if r.hovered() { env.t.text } else { env.t.accent }, env.t.bg_panel);
    ui.painter().circle_filled(c, 4.5, bg);
    ui.painter().line_segment([c - egui::vec2(3.0, 0.0), c + egui::vec2(3.0, 0.0)], Stroke::new(1.4, ink));
    ui.painter().line_segment([c - egui::vec2(0.0, 3.0), c + egui::vec2(0.0, 3.0)], Stroke::new(1.4, ink));
    r
}

fn new_layer(env: &mut Env, cut: &NodeId, kind: LayerType, active: Option<&NodeId>) {
    match cmds::add_layer(env.core, cut, kind, active) {
        Ok(id) => {
            env.ed.layer = Some(id);
            env.ed.layer_sel.clear();
            env.ed.cel_number = None;
        }
        Err(e) => env.ed.notify(e),
    }
}

/// Menu "mais tipos": todos os tipos de camada, o modelo 3D e o manequim.
fn add_menu(env: &mut Env, ui: &mut Ui, cut: &NodeId, active: Option<&NodeId>) {
    let t = env.t;
    for kind in ADD_KINDS {
        let tex = env.icons.texture(ui.ctx(), kind_icon(kind), 16.0);
        let img = egui::Image::new((tex, egui::vec2(16.0, 16.0))).tint(t.text_dim);
        let item = ui.add(egui::Button::image_and_text(img, tr(add_kind_key(kind))));
        env.note(format!("layers:add.{}", kind_id(kind)), item.rect);
        if item.clicked() {
            new_layer(env, cut, kind, active);
            ui.close();
        }
    }
    // camada de ajuste (auditoria 4.5f): uma camada de efeito com a correção escolhida, que muda o que está abaixo dela
    // sem tocar nos desenhos; a janela de parâmetros abre em seguida
    let (w, h) = cmds::canvas_size(&env.core.project, cut);
    let mut pick: Option<(&'static str, serde_json::Value)> = None;
    let tex = env.icons.texture(ui.ctx(), IconId::LayerAdjustment, 16.0);
    let r = ui.horizontal(|ui| {
        ui.add(egui::Image::new((tex, egui::vec2(16.0, 16.0))).tint(t.text_dim));
        ui.menu_button(tr("layers.add.adjustment"), |ui| {
            for (group, items) in crate::app::workbar::filter_groups(w as f32, h as f32) {
                if group != "filter.group.correction" {
                    continue;
                }
                for (key, effect) in items {
                    let b = ui.button(tr(key));
                    env.note(format!("layers:add.adjustment.{}", key.trim_start_matches("filter.")), b.rect);
                    if b.clicked() {
                        pick = Some((key, effect));
                        ui.close();
                    }
                }
            }
        })
    });
    env.note("layers:add.adjustment", r.inner.response.rect);
    if let Some((key, effect)) = pick {
        ui.close();
        new_layer(env, cut, LayerType::Effect, active);
        if let Some(l) = env.ed.layer.clone() {
            let _ = cmds::rename_node(env.core, &l, &format!("{} ({})", tr("layers.adjustment_name"), tr(key)));
            crate::app::workbar::apply_filter(env, effect);
        }
    }
    // camada 3D de referência: importa .glb/.gltf/.obj (cria o ativo e a camada no topo)
    ui.separator();
    let tex = env.icons.texture(ui.ctx(), IconId::LayerModel3d, 16.0);
    let img = egui::Image::new((tex, egui::vec2(16.0, 16.0))).tint(t.text_dim);
    let item = ui.add(egui::Button::image_and_text(img, tr("layers.add.model3d")));
    env.note("layers:add.model3d", item.rect);
    if item.clicked() {
        ui.close();
        crate::app::model3d::import_dialog(env, cut);
    }
    // manequim posável (S24 3D, fase 8): corpo anime com esqueleto, em pé, pronto para posar
    let tex = env.icons.texture(ui.ctx(), IconId::LayerModel3d, 16.0);
    let img = egui::Image::new((tex, egui::vec2(16.0, 16.0))).tint(t.text_dim);
    let item = ui.add(egui::Button::image_and_text(img, tr("layers.add.mannequin")));
    env.note("layers:add.mannequin", item.rect);
    if item.clicked() {
        ui.close();
        crate::app::model3d::add_mannequin(env, cut);
    }
}

/// A camada de linha está ligada ao balde de alguma camada de preenchimento do corte?
fn is_fill_ref(p: &aiai_core::Project, rows: &[LayerRow], layer: &NodeId) -> bool {
    rows.iter().filter_map(|r| cmds::layer_data(p, &r.id)).any(|l| l.kind == LayerType::Fill && l.line_refs.contains(layer))
}

/// Liga a camada de linha ao balde (a camada de preenchimento mais próxima abaixo dela, senão a primeira do corte) ou
/// desliga de todas. Um passo de desfazer.
fn toggle_fill_ref(env: &mut Env, rows: &[LayerRow], layer: &NodeId, on: bool) {
    let p = &env.core.project;
    let fills: Vec<(usize, NodeId, Vec<NodeId>)> = rows.iter().enumerate().filter_map(|(i, r)| cmds::layer_data(p, &r.id).filter(|l| l.kind == LayerType::Fill).map(|l| (i, r.id.clone(), l.line_refs.clone()))).collect();
    let mut cmds_ = Vec::new();
    if on {
        for (_, id, refs) in &fills {
            if refs.contains(layer) {
                let keep: Vec<NodeId> = refs.iter().filter(|x| *x != layer).cloned().collect();
                cmds_.push(Command::SetProps { id: id.clone(), name: None, data: json!({ "line_refs": keep }) });
            }
        }
    } else {
        let at = rows.iter().position(|r| &r.id == layer).unwrap_or(0);
        let target = fills.iter().find(|(i, _, _)| *i > at).or(fills.first());
        let Some((_, id, refs)) = target else {
            env.ed.notify(tr("layers.fill_ref.none"));
            return;
        };
        let mut refs = refs.clone();
        refs.push(layer.clone());
        cmds_.push(Command::SetProps { id: id.clone(), name: None, data: json!({ "line_refs": refs }) });
    }
    if cmds_.is_empty() {
        return;
    }
    if let Err(e) = cmds::run(env.core, Command::Batch { commands: cmds_, label: Some(tr("layers.fill_ref").to_owned()) }) {
        env.ed.notify(e);
    }
}

/// Mistura e opacidade da camada ativa numa linha só (como no topo do painel de camadas do Clip Studio).
fn blend_opacity_row(env: &mut Env, ui: &mut Ui, layer: &NodeId) {
    let Some(l) = cmds::layer_data(&env.core.project, layer).cloned() else { return };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let mut blend = l.blend;
        let combo = egui::ComboBox::from_id_salt("layer_blend").width(96.0).selected_text(tr(blend_key(blend))).show_ui(ui, |ui| {
            for b in BLEND_MODES {
                if b == BlendMode::PassThrough && l.kind != LayerType::Group {
                    continue;
                }
                let item = ui.selectable_value(&mut blend, b, tr(blend_key(b)));
                env.notes.push((format!("layers:blend.{}", serde_json::to_value(b).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()), item.rect));
            }
        });
        env.notes.push(("layers:blend".into(), combo.response.rect));
        combo.response.on_hover_text(tr("layers.blend"));
        if blend != l.blend {
            {
                let r = cmds::set_layer_props(env.core, layer, json!({ "blend": blend }));
                notify(env, r);
            }
        }
        ui.spacing_mut().slider_width = (ui.available_width() - 44.0).clamp(40.0, 260.0);
        let cur = (l.opacity * 100.0).round();
        let r = commit_on_release(ui, Id::new(("opacity", layer)), cur, |ui, v| {
            let r = ui.add(egui::Slider::new(v, 0.0..=100.0).suffix("%").max_decimals(0).trailing_fill(true)).on_hover_text(tr("layers.opacity"));
            env.notes.push(("layers:opacity".into(), r.rect));
            r
        });
        if let Some(v) = r {
            {
                let r = cmds::set_layer_props(env.core, layer, json!({ "opacity": (v / 100.0).clamp(0.0, 1.0) }));
                notify(env, r);
            }
        }
    });
}

fn row_action(env: &mut Env, a: RowAct, cut: &NodeId, active: Option<&NodeId>, rows: &[LayerRow], chosen: &[NodeId]) {
    match a.clone() {
        RowAct::Delete if !chosen.is_empty() => {
            // a próxima ativa: a primeira linha abaixo que não sai
            let last = rows.iter().rposition(|r| chosen.contains(&r.id)).unwrap_or(0);
            let next = rows.iter().skip(last + 1).chain(rows.iter().take(last).rev()).find(|r| !chosen.contains(&r.id) && !chosen.iter().any(|c| env.core.project.subtree(c).contains(&r.id))).map(|r| r.id.clone());
            match cmds::delete_layers(env.core, chosen) {
                Ok(()) => {
                    env.ed.layer = next;
                    env.ed.layer_sel.clear();
                    env.ed.cel_number = None;
                }
                Err(e) => env.ed.notify(e),
            }
        }
        // agrupar: pasta nova no lugar da de cima, com as escolhidas dentro (Ctrl+G)
        RowAct::Group if !chosen.is_empty() => match cmds::add_layer(env.core, cut, LayerType::Group, chosen.first()) {
            Ok(g) => {
                // a pasta nasce acima da de cima (ou dentro dela, se a de cima for pasta: aí entra ao lado)
                {
                let r = cmds::move_layers(env.core, chosen, &LayerDrop::Into(g.clone()));
                notify(env, r);
            }
                env.ed.layer = Some(g);
                env.ed.layer_sel.clear();
            }
            Err(e) => env.ed.notify(e),
        },
        RowAct::Up | RowAct::Down => {
            if let Some(l) = active {
                let dir = if a == RowAct::Up { 1 } else { -1 };
                {
                let r = cmds::move_layer(env.core, l, dir).map(|_| ());
                notify(env, r);
            }
            }
        }
        RowAct::Into(target) => {
            let r = match target {
                Some(g) => cmds::move_layers(env.core, chosen, &LayerDrop::Into(g)),
                None => chosen.iter().try_for_each(|a| cmds::move_layer_to(env.core, a, cut)),
            };
            notify(env, r);
        }
        RowAct::Merge(visible) => {
            env.ed.merge_layers(env.core, visible);
        }
        _ => {}
    }
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
        LayerType::Model3d => "model3d",
    }
}

#[allow(clippy::too_many_arguments)]
fn layer_row(env: &mut Env, ui: &mut Ui, i: usize, row: &LayerRow, active: Option<&NodeId>, rows: &[LayerRow], chosen: &[NodeId], cut: &NodeId, snap: Option<&std::sync::Arc<crate::app::render_service::Snapshot>>) -> (Rect, Option<RowAct>) {
    let t = env.t;
    let h = ROW_H;
    let Some(node) = env.core.project.node(&row.id) else { return (Rect::NOTHING, None) };
    let Some(l) = cmds::layer_data(&env.core.project, &row.id).cloned() else { return (Rect::NOTHING, None) };
    let name = node.name.clone();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click_and_drag());
    env.note(format!("layers:row:{i}"), rect);
    let is_active = active == Some(&row.id);
    let is_chosen = chosen.contains(&row.id);
    let p = ui.painter();
    if is_active {
        p.rect_filled(rect, 4.0, t.accent_dim);
        p.rect_filled(Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 4.0), egui::vec2(3.0, h - 8.0)), 1.5, t.accent);
    } else if is_chosen {
        p.rect_filled(rect, 4.0, t.accent_dim.gamma_multiply(0.6));
    } else if resp.hovered() {
        p.rect_filled(rect, 4.0, t.bg_hover);
    }
    // olho e cadeado à esquerda (como no Clip Studio), depois o recuo da pasta, a miniatura e o texto
    let eye_r = Rect::from_center_size(egui::pos2(rect.left() + 14.0, rect.center().y), egui::vec2(20.0, 20.0));
    let lock_r = Rect::from_center_size(egui::pos2(rect.left() + 34.0, rect.center().y), egui::vec2(18.0, 18.0));
    p.vline(rect.left() + 45.0, (rect.top() + 6.0)..=(rect.bottom() - 6.0), Stroke::new(1.0, t.border));
    // camada recortada na de baixo: seta dobrada antes da miniatura (como no Photoshop)
    let clip_in = if l.clip_to_below { 12.0 } else { 0.0 };
    let indent = 50.0 + row.depth as f32 * 12.0 + clip_in;
    if l.clip_to_below {
        let x = rect.left() + indent - 7.0;
        let (a, b, c) = (egui::pos2(x, rect.top() + 8.0), egui::pos2(x, rect.center().y + 3.0), egui::pos2(x + 5.0, rect.center().y + 3.0));
        p.line_segment([a, b], Stroke::new(1.3, t.text_dim));
        p.line_segment([b, c], Stroke::new(1.3, t.text_dim));
        p.line_segment([c, c + egui::vec2(-3.0, -3.0)], Stroke::new(1.3, t.text_dim));
        p.line_segment([c, c + egui::vec2(-3.0, 3.0)], Stroke::new(1.3, t.text_dim));
    }
    let thumb = Rect::from_min_size(egui::pos2(rect.left() + indent, rect.top() + 3.0), egui::vec2(44.0, h - 6.0));
    paint_thumb(env, ui, thumb, &row.id, &l, cut, snap);
    let ink = if l.visible { t.text } else { t.text_dim.gamma_multiply(0.7) };
    let text_left = thumb.right() + 7.0;
    let name_rect = Rect::from_min_max(egui::pos2(text_left, rect.center().y), egui::pos2(rect.right() - 4.0, rect.bottom()));
    // linha de cima: opacidade e mistura ("100% Normal"); embaixo o nome
    let sub = format!("{:.0}% {}", l.opacity * 100.0, tr(blend_key(l.blend)));
    let sub_r = ui.painter().with_clip_rect(Rect::from_min_max(egui::pos2(text_left, rect.top()), egui::pos2(rect.right() - 4.0, rect.center().y))).text(egui::pos2(text_left, rect.top() + 11.0), Align2::LEFT_CENTER, sub, FontId::proportional(10.5), t.text_dim);
    // camada 3D só de referência: selo à direita da linha de cima (não sai na entrega; auditoria V2)
    if l.kind == LayerType::Model3d && aiai_core::model3d::Model3dParams::from_layer_params(&l.params).and_then(|r| r.ok()).is_some_and(|m| m.render.reference) {
        let font = FontId::proportional(10.0);
        let room = rect.right() - 6.0 - (sub_r.right() + 8.0);
        let full = ui.painter().layout_no_wrap(tr("layers.badge.ref3d").to_owned(), font.clone(), t.text);
        let galley = if full.size().x + 10.0 <= room { full } else { ui.painter().layout_no_wrap(tr("layers.badge.ref3d.short").to_owned(), font, t.text) };
        let size = galley.size() + egui::vec2(10.0, 2.0);
        let badge = Rect::from_min_size(egui::pos2(rect.right() - 6.0 - size.x, rect.top() + 11.0 - size.y / 2.0), size);
        ui.painter().rect(badge, 3.0, t.bg_frame, Stroke::new(1.0, t.st_review), egui::StrokeKind::Inside);
        ui.painter().galley(badge.min + egui::vec2(5.0, 1.0), galley, t.text);
        env.note(format!("layers:badge3d:{i}"), badge);
    }
    let renaming = env.ed.rename.as_ref().is_some_and(|(id, _)| id == &row.id);
    if renaming {
        let mut commit = false;
        let mut cancel = false;
        if let Some((_, buf)) = env.ed.rename.as_mut() {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(name_rect.shrink2(egui::vec2(0.0, 1.0))));
            let te = child.add(egui::TextEdit::singleline(buf).desired_width(name_rect.width()).margin(egui::vec2(4.0, 1.0)));
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
                    {
                let r = cmds::rename_node(env.core, &id, &buf);
                notify(env, r);
            }
                }
            }
        }
    } else {
        ui.painter().with_clip_rect(name_rect).text(egui::pos2(text_left, rect.top() + 25.0), Align2::LEFT_CENTER, &name, FontId::proportional(12.5), ink);
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
    // destravada: o cadeado só aparece com o mouse em cima (a linha fica mais limpa)
    if l.locked || lock.hovered() || resp.hovered() {
        paint_icon(env.icons, painter, if l.locked { IconId::Lock } else { IconId::Unlock }, lock_r.shrink(2.5), if l.locked { t.accent } else { t.text_dim.gamma_multiply(0.55) }, false);
    }
    let mut act = None;
    if eye.on_hover_text(tr("layers.visible")).clicked() {
        {
                let r = cmds::set_layer_props(env.core, &row.id, json!({ "visible": !l.visible }));
                notify(env, r);
            }
    } else if lock.on_hover_text(tr("layers.locked")).clicked() {
        {
                let r = cmds::set_layer_props(env.core, &row.id, json!({ "locked": !l.locked }));
                notify(env, r);
            }
    } else if resp.double_clicked() {
        env.ed.layer = Some(row.id.clone());
        env.ed.rename = Some((row.id.clone(), name.clone()));
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
    // botão direito: a linha entra na escolha (se não estava) e o menu age a partir dela
    if resp.secondary_clicked() && !is_chosen {
        env.ed.layer = Some(row.id.clone());
        env.ed.layer_sel.clear();
    }
    resp.context_menu(|ui| {
        let several = chosen.len() >= 2 && is_chosen;
        // renomear não é ação do documento: começa a edição do nome aqui
        let rn = ui.button(tr("layers.rename"));
        env.note("layers:ctx.rename", rn.rect);
        if rn.clicked() {
            env.ed.layer = Some(row.id.clone());
            env.ed.rename = Some((row.id.clone(), name.clone()));
            ui.close();
        }
        let item = |ui: &mut Ui, env: &mut Env, key: &str, name: &str| -> bool {
            let b = ui.button(tr(key));
            env.note(name, b.rect);
            if b.clicked() {
                ui.close();
            }
            b.clicked()
        };
        ui.separator();
        if item(ui, env, "layers.up", "layers:ctx.up") {
            act = Some(RowAct::Up);
        }
        if item(ui, env, "layers.down", "layers:ctx.down") {
            act = Some(RowAct::Down);
        }
        if item(ui, env, "layers.group", "layers:group") {
            act = Some(RowAct::Group);
        }
        // mover para pasta: lista as pastas do corte
        let groups: Vec<(NodeId, String)> = rows.iter().filter(|r| cmds::layer_data(&env.core.project, &r.id).is_some_and(|l| l.kind == LayerType::Group) && !chosen.contains(&r.id)).map(|r| (r.id.clone(), env.core.project.node(&r.id).map(|n| n.name.clone()).unwrap_or_default())).collect();
        let sub = ui.menu_button(tr("layers.move_to_group"), |ui| {
            if ui.button(tr("layers.to_cut")).clicked() {
                act = Some(RowAct::Into(None));
                ui.close();
            }
            for (gid, gname) in &groups {
                if ui.button(gname).clicked() {
                    act = Some(RowAct::Into(Some(gid.clone())));
                    ui.close();
                }
            }
        });
        env.note("layers:to_group", sub.response.rect);
        ui.separator();
        if item(ui, env, if several { "layers.merge_selected" } else { "layers.merge_down" }, "layers:ctx.merge") {
            act = Some(RowAct::Merge(false));
        }
        if item(ui, env, "layers.merge_visible", "layers:ctx.merge_visible") {
            act = Some(RowAct::Merge(true));
        }
        ui.separator();
        if item(ui, env, "layers.delete", "layers:ctx.delete") {
            act = Some(RowAct::Delete);
        }
    });
    if resp.drag_started() {
        // arrasta as escolhidas se a linha é uma delas; senão, só ela
        let ids: Vec<NodeId> = if is_chosen { chosen.to_vec() } else { vec![row.id.clone()] };
        env.ed.layer_drag = Some(ids);
    }
    (rect, act)
}

/// Miniatura da linha: o cel exposto (xadrez por baixo) ou, nas camadas sem cel (pasta, texto, 3D, efeito), o ícone do
/// tipo; um selo pequeno com o tipo no canto.
fn paint_thumb(env: &mut Env, ui: &Ui, r: Rect, layer: &NodeId, l: &aiai_core::LayerData, cut: &NodeId, snap: Option<&std::sync::Arc<crate::app::render_service::Snapshot>>) {
    let t = env.t;
    let p = ui.painter();
    let has_cel = matches!(l.kind, LayerType::Raster | LayerType::Vector | LayerType::Fill | LayerType::Reference | LayerType::Guide);
    let tex = if has_cel {
        let p_ = &env.core.project;
        let cel = cmds::exposed_cel(p_, cut, layer, env.ed.frame);
        match cel.as_ref().and_then(|c| cmds::cel_data(p_, c).map(|d| (c.clone(), d))) {
            Some((cid, d)) => {
                let canvas = cmds::canvas_size(p_, cut);
                let sig = crate::app::layer_thumbs::cel_sig(&cid, d, canvas);
                let idle = snap.is_some();
                let s = snap.cloned();
                env.ed.workbar.thumbs.get(ui.ctx(), layer, &cid, sig, canvas, idle, move || s.expect("snapshot"))
            }
            None => None,
        }
    } else {
        None
    };
    match tex {
        Some(tx) => {
            // xadrez claro por baixo (o transparente da camada)
            let [a, b] = crate::app::canvas::checker_colors(false);
            p.rect_filled(r, 2.0, a);
            let cs = 5.0;
            let (nx, ny) = ((r.width() / cs).ceil() as i32, (r.height() / cs).ceil() as i32);
            for yy in 0..ny {
                for xx in 0..nx {
                    if (xx + yy) % 2 == 1 {
                        let c = Rect::from_min_size(r.min + egui::vec2(xx as f32 * cs, yy as f32 * cs), egui::vec2(cs, cs)).intersect(r);
                        p.rect_filled(c, 0.0, b);
                    }
                }
            }
            let canvas = cmds::canvas_size(&env.core.project, cut);
            let k = (r.width() / canvas.0.max(1) as f32).min(r.height() / canvas.1.max(1) as f32);
            let ir = Rect::from_center_size(r.center(), egui::vec2(canvas.0 as f32 * k, canvas.1 as f32 * k));
            p.image(tx, ir, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        None => {
            p.rect_filled(r, 2.0, t.bg_widget);
            paint_icon(env.icons, p, kind_icon(l.kind), Rect::from_center_size(r.center(), egui::vec2(18.0, 18.0)), t.text_dim, false);
        }
    }
    p.rect_stroke(r, 2.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    if has_cel {
        let b = Rect::from_min_size(egui::pos2(r.right() - 13.0, r.bottom() - 13.0), egui::vec2(12.0, 12.0));
        p.rect_filled(b.expand(1.0), 2.0, t.bg_panel);
        paint_icon(env.icons, p, kind_icon(l.kind), b, t.text_dim, false);
    }
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
