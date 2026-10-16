//! Painel Projeto: a árvore episódio → cena → corte, personagens e documentos; escolhe o corte ativo; cria episódio, cena
//! e corte (duração e tamanho) e edita as configurações do projeto (nome, tamanho do quadro, taxa de quadros).

use super::commit_on_release;
use crate::app::commands::{self as cmds, NodeId};
use crate::app::ui_kit::{Env, body_ui, dim_label, paint_icon, section};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::*;
use egui::{Align2, FontId, Id, Rect, Sense, Ui};
use serde_json::json;

const FPS_PRESETS: [(u32, u32, &str); 6] = [(24000, 1001, "23.976"), (24, 1, "24"), (25, 1, "25"), (30000, 1001, "29.97"), (30, 1, "30"), (60, 1, "60")];

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    env.ed.active_cut(env.core);
    let sel = env.ed.sel_node.clone().filter(|id| env.core.project.node(id).is_some());
    let sel_kind = sel.as_ref().and_then(|id| env.core.project.node(id)).map(|n| n.data.type_name());

    // ---- criar
    ui.horizontal_wrapped(|ui| {
        let b = ui.add(egui::Button::new(tr("project.add_episode")));
        env.note("project:add_episode", b.rect);
        if b.clicked() {
            match cmds::add_episode(env.core) {
                Ok(id) => env.ed.sel_node = Some(id),
                Err(e) => env.ed.notify(e),
            }
        }
        let ep = episode_of(env, sel.as_ref());
        let b = ui.add_enabled(ep.is_some(), egui::Button::new(tr("project.add_scene")));
        env.note("project:add_scene", b.rect);
        if b.on_disabled_hover_text(tr("project.need_episode")).clicked() {
            if let Some(ep) = ep {
                match cmds::add_scene(env.core, &ep) {
                    Ok(id) => env.ed.sel_node = Some(id),
                    Err(e) => env.ed.notify(e),
                }
            }
        }
        let sc = scene_of(env, sel.as_ref());
        let b = ui.add_enabled(sc.is_some(), egui::Button::new(tr("project.add_cut")));
        env.note("project:add_cut", b.rect);
        if b.on_disabled_hover_text(tr("project.need_scene")).clicked() {
            if let Some(sc) = sc {
                let canvas = env.ed.form_custom_size.then_some(env.ed.form_size);
                match cmds::add_cut(env.core, &sc, env.ed.form_duration, canvas) {
                    Ok(id) => {
                        env.ed.sel_node = Some(id.clone());
                        env.ed.set_cut(id);
                    }
                    Err(e) => env.ed.notify(e),
                }
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        dim_label(ui, &t, tr("project.cut_duration"));
        let r = ui.add(egui::DragValue::new(&mut env.ed.form_duration).range(1..=36000).speed(0.5).suffix(format!(" {}", tr("unit.frames"))));
        env.note("project:duration", r.rect);
        ui.checkbox(&mut env.ed.form_custom_size, tr("project.cut_size"));
        if env.ed.form_custom_size {
            ui.add(egui::DragValue::new(&mut env.ed.form_size[0]).range(16..=16384).speed(4.0));
            ui.label("x");
            ui.add(egui::DragValue::new(&mut env.ed.form_size[1]).range(16..=16384).speed(4.0));
        }
    });
    ui.add_space(2.0);
    ui.separator();

    // ---- árvore
    let avail = ui.available_height();
    egui::ScrollArea::vertical().id_salt("project_tree").max_height((avail - 4.0).max(40.0)).auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        let tree = cmds::episode_tree(&env.core.project);
        if tree.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(tr("project.empty")).color(t.text_dim));
            ui.add_space(6.0);
            let b = ui.button(tr("project.quick_start"));
            env.note("project:quick_start", b.rect);
            if b.clicked() {
                match cmds::quick_start(env.core, env.ed.form_duration, env.ed.form_custom_size.then_some(env.ed.form_size)) {
                    Ok(cut) => env.ed.set_cut(cut),
                    Err(e) => env.ed.notify(e),
                }
            }
        }
        for ep in &tree {
            let open = node_row(env, ui, &ep.id, 0, IconId::NodeEpisode, true, None);
            if open {
                for (sc, cuts) in &ep.scenes {
                    let open = node_row(env, ui, sc, 1, IconId::NodeScene, true, None);
                    if open {
                        for cut in cuts {
                            let frames = cmds::cut_data(&env.core.project, cut).map(|c| c.duration).unwrap_or(0);
                            node_row(env, ui, cut, 2, IconId::NodeCut, false, Some(format!("{frames} {}", tr("unit.frames_short"))));
                        }
                    }
                }
            }
        }
        // personagens e documentos (somente listagem por enquanto)
        let chars: Vec<NodeId> = env.core.project.roots.characters.clone();
        if !chars.is_empty() {
            section(ui, &t, "project.characters");
            for c in chars {
                node_row(env, ui, &c, 0, IconId::NodeCharacter, false, None);
            }
        }
        let docs: Vec<NodeId> = env.core.project.roots.documents.clone();
        if !docs.is_empty() {
            section(ui, &t, "project.documents");
            for d in docs {
                node_row(env, ui, &d, 0, IconId::NodeDocument, false, None);
            }
        }
        // configurações do projeto
        section(ui, &t, "project.settings");
        project_settings(env, ui);
        let _ = sel_kind;
    });
}

fn episode_of(env: &Env, sel: Option<&NodeId>) -> Option<NodeId> {
    let p = &env.core.project;
    let id = sel?;
    match p.node(id)?.data {
        NodeKind::Episode(_) => Some(id.clone()),
        NodeKind::Scene(_) | NodeKind::Cut(_) => p.ancestors(id).into_iter().find(|a| matches!(p.node(a).map(|n| &n.data), Some(NodeKind::Episode(_)))),
        _ => None,
    }
}

fn scene_of(env: &Env, sel: Option<&NodeId>) -> Option<NodeId> {
    let p = &env.core.project;
    let id = sel?;
    match p.node(id)?.data {
        NodeKind::Scene(_) => Some(id.clone()),
        NodeKind::Cut(_) => p.node(id)?.parent.clone(),
        _ => None,
    }
}

/// Uma linha da árvore. Devolve se o nó está aberto (só vale para nós com filhos).
fn node_row(env: &mut Env, ui: &mut Ui, id: &NodeId, depth: usize, icon: IconId, expandable: bool, extra: Option<String>) -> bool {
    let t = env.t;
    let Some(node) = env.core.project.node(id) else { return false };
    let name = node.name.clone();
    let is_cut = matches!(node.data, NodeKind::Cut(_));
    let open = !env.ed.tree_closed.contains(id);
    let h = 24.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    env.note(format!("project:node:{}", name), rect);
    let selected = env.ed.sel_node.as_ref() == Some(id);
    let active_cut = is_cut && env.ed.cut.as_ref() == Some(id);
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 4.0, t.accent_dim);
    } else if resp.hovered() {
        p.rect_filled(rect, 4.0, t.bg_hover);
    }
    if active_cut {
        p.rect_filled(Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 3.0), egui::vec2(3.0, h - 6.0)), 1.5, t.accent);
    }
    let x0 = rect.left() + 6.0 + depth as f32 * 14.0;
    let chev_rect = Rect::from_center_size(egui::pos2(x0 + 7.0, rect.center().y), egui::vec2(14.0, 14.0));
    let mut toggled = false;
    if expandable {
        let chev = ui.interact(chev_rect.expand(3.0), Id::new(("tree_chev", id)), Sense::click());
        paint_icon(env.icons, ui.painter(), if open { IconId::ChevronDown } else { IconId::ChevronRight }, chev_rect, t.text_dim, false);
        toggled = chev.clicked();
    }
    let ix = x0 + 22.0;
    paint_icon(env.icons, ui.painter(), icon, Rect::from_center_size(egui::pos2(ix + 7.0, rect.center().y), egui::vec2(15.0, 15.0)), if active_cut { t.accent } else { t.text_dim }, false);
    let renaming = env.ed.rename.as_ref().is_some_and(|(r, _)| r == id);
    let name_left = ix + 20.0;
    if renaming {
        let mut commit = false;
        let mut cancel = false;
        if let Some((_, buf)) = env.ed.rename.as_mut() {
            let nr = Rect::from_min_max(egui::pos2(name_left, rect.top() + 1.0), egui::pos2(rect.right() - 4.0, rect.bottom() - 1.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(nr));
            let te = child.add(egui::TextEdit::singleline(buf).desired_width(nr.width()).margin(egui::vec2(4.0, 2.0)));
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
            if let Some((rid, buf)) = env.ed.rename.take() {
                if commit {
                    if let Err(e) = cmds::rename_node(env.core, &rid, &buf) {
                        env.ed.notify(e);
                    }
                }
            }
        }
    } else {
        let clip = Rect::from_min_max(egui::pos2(name_left, rect.top()), egui::pos2(rect.right() - 4.0, rect.bottom()));
        let painter = ui.painter().with_clip_rect(clip);
        let galley = painter.layout_no_wrap(name.clone(), FontId::proportional(13.0), if selected || active_cut { t.text } else { t.text });
        let w = galley.size().x;
        painter.galley(egui::pos2(name_left, rect.center().y - galley.size().y / 2.0), galley, t.text);
        if let Some(x) = extra {
            painter.text(egui::pos2(name_left + w + 8.0, rect.center().y), Align2::LEFT_CENTER, x, FontId::proportional(11.5), t.text_dim);
        }
    }
    // clique, duplo clique e menu de contexto
    let mut action_delete = false;
    let mut action_rename = false;
    resp.context_menu(|ui| {
        let b = ui.button(tr("project.rename"));
        env.notes.push(("project:ctx.rename".into(), b.rect));
        if b.clicked() {
            action_rename = true;
            ui.close();
        }
        let b = ui.button(tr("project.delete"));
        env.notes.push(("project:ctx.delete".into(), b.rect));
        if b.clicked() {
            action_delete = true;
            ui.close();
        }
    });
    if toggled {
        if open {
            env.ed.tree_closed.insert(id.clone());
        } else {
            env.ed.tree_closed.remove(id);
        }
    } else if resp.double_clicked() || action_rename {
        env.ed.sel_node = Some(id.clone());
        env.ed.rename = Some((id.clone(), name));
    } else if resp.clicked() || resp.secondary_clicked() {
        env.ed.sel_node = Some(id.clone());
        if is_cut {
            env.ed.set_cut(id.clone());
        }
    }
    if action_delete {
        match cmds::delete_node(env.core, id) {
            Ok(()) => {
                env.ed.sel_node = None;
                env.ed.rename = None;
            }
            Err(e) => env.ed.notify(e),
        }
    }
    open
}

fn project_settings(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let s = env.core.project.settings.clone();
    egui::Grid::new("project_settings_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("project.name"));
        let current = env.core.project.name.clone();
        let (te, committed) = super::text_commit(ui, Id::new("project_name"), &current, 150.0);
        env.note("project:name", te.rect);
        if let Some(name) = committed {
            let _ = cmds::set_project_props(env.core, json!({ "name": name }));
        }
        ui.end_row();
        dim_label(ui, &t, tr("project.frame_size"));
        ui.horizontal(|ui| {
            if let Some(v) = commit_on_release(ui, Id::new("proj_w"), s.width, |ui, v| ui.add(egui::DragValue::new(v).range(16..=16384).speed(4.0))) {
                let _ = cmds::set_project_props(env.core, json!({ "settings": { "width": v } }));
            }
            ui.label("x");
            if let Some(v) = commit_on_release(ui, Id::new("proj_h"), s.height, |ui, v| ui.add(egui::DragValue::new(v).range(16..=16384).speed(4.0))) {
                let _ = cmds::set_project_props(env.core, json!({ "settings": { "height": v } }));
            }
        });
        ui.end_row();
        dim_label(ui, &t, tr("project.fps"));
        let cur = FPS_PRESETS.iter().find(|(n, d, _)| *n == s.fps_num && *d == s.fps_den).map(|(_, _, l)| l.to_string()).unwrap_or_else(|| format!("{:.3}", s.fps()));
        egui::ComboBox::from_id_salt("proj_fps").width(90.0).selected_text(cur).show_ui(ui, |ui| {
            for (n, d, label) in FPS_PRESETS {
                if ui.selectable_label(n == s.fps_num && d == s.fps_den, label).clicked() {
                    let _ = cmds::set_project_props(env.core, json!({ "settings": { "fps_num": n, "fps_den": d } }));
                }
            }
        });
        ui.end_row();
    });
}
