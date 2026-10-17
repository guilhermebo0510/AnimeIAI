//! Painel Estrutura (Roteiro): o roteiro aberto, a lista de cenas (cabeçalho, nº de elementos e segundos estimados) e os
//! quadros de beats do projeto (só leitura). Criar roteiro e cena, apagar e mover cena são ações `SCRIPT` da bridge
//! (`actions`): cada clique é UM passo de desfazer, igual à edição de um agente.

use crate::app::actions::{self, DocKind};
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::Id;
use aiai_tools::preprod::{self, ScriptOp};
use egui::{FontId, Rect, Sense, Ui};
use serde_json::json;

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let scripts = actions::docs_of(&env.core.project, DocKind::Script);
    let sel = actions::sync_script_sel(env.ed, env.core);
    let sel_id = sel.as_ref().map(|(id, _)| id.clone());

    // ---- roteiro aberto
    let name = sel_id.as_ref().and_then(|id| scripts.iter().find(|(i, _)| i == id)).map(|(_, n)| n.clone()).unwrap_or_else(|| tr("outline.no_script").to_owned());
    let cb = egui::ComboBox::from_id_salt("outline_doc").width((ui.available_width() - 28.0).max(60.0)).selected_text(name).show_ui(ui, |ui| {
        for (id, n) in &scripts {
            let r = ui.selectable_label(sel_id.as_ref() == Some(id), n);
            env.notes.push((format!("outline:doc:{n}"), r.rect));
            if r.clicked() {
                env.ed.sel_script = Some(id.clone());
                env.ed.sel_scene = None;
                env.ed.sel_element = None;
            }
        }
    });
    env.note("outline:doc_picker", cb.response.rect);

    // ---- ações
    ui.horizontal_wrapped(|ui| {
        let b = ui.button(tr("outline.new_script"));
        env.note("outline:new_script", b.rect);
        if b.clicked() {
            new_script(env);
        }
        let has = sel.is_some();
        let pos = sel.as_ref().and_then(|(_, d)| env.ed.sel_scene.as_ref().and_then(|s| d.scenes.iter().position(|x| &x.id == s)));
        let count = sel.as_ref().map(|(_, d)| d.scenes.len()).unwrap_or(0);
        let add = env.icon_button(ui, IconId::Plus, "outline.add_scene", false, has, "outline:add_scene");
        let del = env.icon_button(ui, IconId::Trash, "outline.delete_scene", false, pos.is_some(), "outline:delete_scene");
        let up = env.icon_button(ui, IconId::ArrowUp, "outline.scene_up", false, pos.is_some_and(|p| p > 0), "outline:scene_up");
        let down = env.icon_button(ui, IconId::ArrowDown, "outline.scene_down", false, pos.is_some_and(|p| p + 1 < count), "outline:scene_down");
        let Some((doc_id, doc)) = &sel else { return };
        if add.clicked() {
            let index = pos.map(|p| p + 1);
            let heading = format!("{} {}", tr("outline.scene_word"), count + 1);
            let r = actions::script_edit(env.core, doc_id, vec![ScriptOp::InsertScene { index, heading }]);
            if let Some(created) = actions::notify_err(env.ed, r) {
                env.ed.sel_scene = created.into_iter().next();
                env.ed.sel_element = None;
            }
        }
        if let (true, Some(p)) = (del.clicked(), pos) {
            let next = doc.scenes.get(p + 1).or_else(|| p.checked_sub(1).and_then(|q| doc.scenes.get(q))).map(|s| s.id.clone());
            let r = actions::script_edit(env.core, doc_id, vec![ScriptOp::DeleteScene { scene: doc.scenes[p].id.clone() }]);
            if actions::notify_err(env.ed, r).is_some() {
                env.ed.sel_scene = next;
                env.ed.sel_element = None;
            }
        }
        if let (true, Some(p)) = (up.clicked(), pos) {
            let r = actions::script_edit(env.core, doc_id, vec![ScriptOp::MoveScene { scene: doc.scenes[p].id.clone(), index: p - 1 }]);
            actions::notify_err(env.ed, r);
        }
        if let (true, Some(p)) = (down.clicked(), pos) {
            let r = actions::script_edit(env.core, doc_id, vec![ScriptOp::MoveScene { scene: doc.scenes[p].id.clone(), index: p + 1 }]);
            actions::notify_err(env.ed, r);
        }
    });
    ui.add_space(2.0);
    ui.separator();

    egui::ScrollArea::vertical().id_salt("outline_scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        match &sel {
            None => {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(tr("panel.outline.empty")).color(t.text_dim));
            }
            Some((_, doc)) => {
                let total: f32 = doc.scenes.iter().map(|s| actions::scene_secs(doc, s)).sum();
                dim_label(ui, &t, &format!("{} · {} {} · {:.1} s", doc.title_or(&name_of(&scripts, sel_id.as_ref())), doc.scenes.len(), tr("outline.scenes"), total));
                ui.add_space(2.0);
                for (i, s) in doc.scenes.iter().enumerate() {
                    scene_row(env, ui, i, s, doc);
                }
            }
        }
        beats(env, ui);
    });
}

/// Nome do documento aberto (quando o roteiro não tem título).
fn name_of(scripts: &[(Id, String)], id: Option<&Id>) -> String {
    id.and_then(|id| scripts.iter().find(|(i, _)| i == id)).map(|(_, n)| n.clone()).unwrap_or_default()
}

trait TitleOr {
    fn title_or(&self, fallback: &str) -> String;
}

impl TitleOr for preprod::ScriptDoc {
    fn title_or(&self, fallback: &str) -> String {
        if self.title.trim().is_empty() { fallback.to_owned() } else { self.title.clone() }
    }
}

/// Cria um roteiro novo (uma cena de partida) e o abre.
pub(crate) fn new_script(env: &mut Env) {
    let name = actions::next_doc_name(&env.core.project, DocKind::Script, tr("outline.default_name"));
    let params = json!({ "op": "create", "name": name, "title": name, "scenes": [{ "heading": tr("outline.default_heading") }] });
    let r = actions::run(env.core, "SCRIPT", params);
    if let Some(v) = actions::notify_err(env.ed, r) {
        if let Some(id) = v.get("doc").and_then(|d| d.as_str()) {
            env.ed.sel_script = Some(Id(id.to_owned()));
            env.ed.sel_scene = None;
            env.ed.sel_element = None;
        }
    }
}

fn scene_row(env: &mut Env, ui: &mut Ui, i: usize, s: &preprod::ScriptScene, doc: &preprod::ScriptDoc) {
    let t = env.t;
    let h = 40.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    env.note(format!("outline:scene:{i}"), rect);
    let selected = env.ed.sel_scene.as_ref() == Some(&s.id);
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 4.0, t.accent_dim);
        p.rect_filled(Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 4.0), egui::vec2(3.0, h - 8.0)), 1.5, t.accent);
    } else if resp.hovered() {
        p.rect_filled(rect, 4.0, t.bg_hover);
    }
    let clip = Rect::from_min_max(egui::pos2(rect.left() + 8.0, rect.top()), egui::pos2(rect.right() - 6.0, rect.bottom()));
    let painter = p.with_clip_rect(clip);
    let heading = if s.heading.trim().is_empty() { tr("outline.untitled_scene").to_owned() } else { s.heading.clone() };
    let g = painter.layout_no_wrap(format!("{}  {}", i + 1, heading), FontId::proportional(13.0), t.text);
    painter.galley(egui::pos2(clip.left(), rect.top() + 5.0), g, t.text);
    let sub = format!("{} {} · {:.1} s", s.elements.len(), tr("outline.elements"), actions::scene_secs(doc, s));
    let g = painter.layout_no_wrap(sub, FontId::proportional(11.5), t.text_dim);
    painter.galley(egui::pos2(clip.left(), rect.top() + 22.0), g, t.text_dim);
    if resp.clicked() {
        env.ed.sel_scene = Some(s.id.clone());
        env.ed.sel_element = None;
    }
}

/// Quadros de beats do projeto (atos e cartões), só leitura.
fn beats(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    for (id, name) in actions::docs_of(&env.core.project, DocKind::Beats) {
        let Ok(board) = preprod::beats_read(&env.core.project, &id) else { continue };
        section(ui, &t, "outline.beats");
        ui.label(egui::RichText::new(format!("{name} · {:.0} s", board.total_target_seconds())).size(12.0).color(t.text_dim));
        for act in &board.acts {
            ui.add_space(2.0);
            ui.label(egui::RichText::new(&act.name).strong().size(12.5));
            for b in &act.beats {
                let line = if b.target_seconds > 0.0 { format!("{}  ({:.0} s)", b.title, b.target_seconds) } else { b.title.clone() };
                let r = ui.label(egui::RichText::new(line).size(12.0)).on_hover_text(&b.summary);
                env.notes.push(("outline:beat".into(), r.rect));
            }
        }
    }
}
