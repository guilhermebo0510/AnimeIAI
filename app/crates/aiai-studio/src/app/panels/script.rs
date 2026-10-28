//! Painel Roteiro: a cena selecionada (ou todas) como roteiro de verdade, ligada ao documento. Cada elemento (ação, fala,
//! parentético, transição, nota) mostra o tipo, quem fala, o texto original e a tradução, emoção e direção, os segundos
//! estimados e os botões de subir, descer e apagar. Toda edição vira uma operação `SCRIPT edit` (a mesma do MCP), aplicada
//! uma vez por quadro: um campo confirmado = UM passo de desfazer.

use super::{commit_on_release, text_commit, text_commit_multi, text_commit_opt};
use crate::app::actions;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::Id;
use aiai_tools::preprod::{ElementKind, ElementPatch, ElementSpec, ScriptDoc, ScriptElement, ScriptOp, ScriptScene};
use egui::{Align, Key, Layout, Modifiers, Stroke, Ui};

const KINDS: [(ElementKind, &str); 5] = [
    (ElementKind::Action, "script.kind.action"),
    (ElementKind::Dialogue, "script.kind.dialogue"),
    (ElementKind::Parenthetical, "script.kind.parenthetical"),
    (ElementKind::Transition, "script.kind.transition"),
    (ElementKind::Note, "script.kind.note"),
];

pub(crate) fn kind_key(k: ElementKind) -> &'static str {
    KINDS.iter().find(|(x, _)| *x == k).map(|(_, key)| *key).unwrap_or("script.kind.action")
}

/// O elemento que vem depois de `e` com Ctrl+Enter (como o Enter do Final Draft): ação puxa ação; fala puxa fala do
/// outro personagem da conversa (o último que falou antes e não é este, como o WriterDuet); parentético puxa a fala do
/// mesmo personagem; transição e nota puxam ação.
pub(crate) fn next_element(scene: &ScriptScene, i: usize) -> (ElementKind, Option<Id>) {
    let e = &scene.elements[i];
    match e.kind {
        ElementKind::Dialogue => {
            let other = scene.elements[..i].iter().rev().filter(|x| x.kind == ElementKind::Dialogue).filter_map(|x| x.character.clone()).find(|c| Some(c) != e.character.as_ref());
            (ElementKind::Dialogue, other.or_else(|| e.character.clone()))
        }
        ElementKind::Parenthetical => {
            let who = e.character.clone().or_else(|| scene.elements[..i].iter().rev().find(|x| x.kind == ElementKind::Dialogue).and_then(|x| x.character.clone()));
            (ElementKind::Dialogue, who)
        }
        _ => (ElementKind::Action, None),
    }
}

/// O que fazer com o id criado pela edição do quadro.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Select {
    Nothing,
    /// Seleciona e põe o cursor no elemento criado.
    NewElement,
    NewScene,
}

struct Pending {
    ops: Vec<ScriptOp>,
    select: Select,
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: egui::Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 12.0);
    let ui = &mut child;
    let Some((doc_id, doc)) = actions::sync_script_sel(env.ed, env.core) else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(tr("panel.script.empty")).color(t.text_dim));
            ui.add_space(8.0);
            let b = ui.button(tr("outline.new_script"));
            env.note("script:new", b.rect);
            if b.clicked() {
                super::outline::new_script(env);
            }
        });
        return;
    };
    let chars = actions::characters(&env.core.project);
    let mut pend = Pending { ops: Vec::new(), select: Select::Nothing };

    // ---- cabeçalho: título do roteiro e filtro de cenas
    ui.horizontal(|ui| {
        let title = if doc.title.trim().is_empty() { tr("outline.untitled_script") } else { doc.title.as_str() };
        ui.label(egui::RichText::new(title).strong().size(15.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let r = ui.checkbox(&mut env.ed.script_all, tr("script.all_scenes"));
            env.note("script:all", r.rect);
            let total: f32 = doc.scenes.iter().map(|s| actions::scene_secs(&doc, s)).sum();
            dim_label(ui, &t, &format!("{:.1} s", total));
        });
    });
    ui.add_space(2.0);

    egui::ScrollArea::vertical().id_salt("script_scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        if doc.scenes.is_empty() {
            ui.label(egui::RichText::new(tr("script.no_scenes")).color(t.text_dim));
            let b = ui.button(tr("outline.add_scene"));
            env.note("script:add_scene", b.rect);
            if b.clicked() {
                pend.ops.push(ScriptOp::InsertScene { index: None, heading: format!("{} 1", tr("outline.scene_word")) });
                pend.select = Select::NewScene;
            }
        }
        for (si, scene) in doc.scenes.iter().enumerate() {
            if !env.ed.script_all && env.ed.sel_scene.as_ref() != Some(&scene.id) {
                continue;
            }
            scene_block(env, ui, &doc, si, scene, &chars, &mut pend);
            ui.add_space(10.0);
        }
    });

    if !pend.ops.is_empty() {
        let r = actions::script_edit(env.core, &doc_id, pend.ops);
        if let Some(created) = actions::notify_err(env.ed, r) {
            match pend.select {
                Select::NewElement => {
                    env.ed.sel_element = created.first().cloned();
                    env.ed.script_focus = created.into_iter().next();
                }
                Select::NewScene => {
                    env.ed.sel_scene = created.into_iter().next();
                    env.ed.sel_element = None;
                }
                Select::Nothing => {}
            }
        }
    }
}

fn scene_block(env: &mut Env, ui: &mut Ui, doc: &ScriptDoc, si: usize, scene: &ScriptScene, chars: &[(Id, String)], pend: &mut Pending) {
    let t = env.t;
    // cabeçalho da cena (editável)
    ui.horizontal(|ui| {
        dim_label(ui, &t, &format!("{}", si + 1));
        let secs = actions::scene_secs(doc, scene);
        let w = (ui.available_width() - 70.0).max(80.0);
        let (r, committed) = ui.scope(|ui| {
            ui.style_mut().text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            ui.style_mut().text_styles.insert(egui::TextStyle::Monospace, egui::FontId::proportional(15.0));
            text_commit(ui, egui::Id::new(("script_heading", &scene.id.0)), &scene.heading, w)
        })
        .inner;
        env.note(format!("script:heading:{si}"), r.rect);
        if r.gained_focus() {
            env.ed.sel_scene = Some(scene.id.clone());
        }
        if let Some(h) = committed {
            pend.ops.push(ScriptOp::UpdateScene { scene: scene.id.clone(), heading: h });
        }
        dim_label(ui, &t, &format!("{secs:.1} s"));
    });
    if scene.elements.is_empty() {
        ui.label(egui::RichText::new(tr("script.scene_empty")).color(t.text_dim));
    }
    for (i, e) in scene.elements.iter().enumerate() {
        element_card(env, ui, doc, si, scene, i, e, chars, pend);
    }
    add_bar(env, ui, si, scene, chars, pend);
}

#[allow(clippy::too_many_arguments)]
fn element_card(env: &mut Env, ui: &mut Ui, doc: &ScriptDoc, si: usize, scene: &ScriptScene, i: usize, e: &ScriptElement, chars: &[(Id, String)], pend: &mut Pending) {
    let t = env.t;
    let selected = env.ed.sel_element.as_ref() == Some(&e.id);
    let mut touched = false;
    let frame = egui::Frame::new().fill(t.bg_widget).stroke(Stroke::new(1.0, if selected { t.accent } else { t.border })).corner_radius(6).inner_margin(egui::Margin::same(8));
    let fr = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
        let eid = &e.id.0;
        // ---- linha 1: tipo, falante, segundos e botões
        ui.horizontal(|ui| {
            let kc = egui::ComboBox::from_id_salt(("script_kind", eid)).width(96.0).selected_text(tr(kind_key(e.kind))).show_ui(ui, |ui| {
                for (k, key) in KINDS {
                    if ui.selectable_label(e.kind == k, tr(key)).clicked() {
                        touched = true;
                        pend.ops.push(ScriptOp::UpdateElement { element: e.id.clone(), patch: ElementPatch { kind: Some(k), ..Default::default() } });
                    }
                }
            });
            env.note(format!("script:kind:{si}:{i}"), kc.response.rect);
            if e.kind == ElementKind::Dialogue {
                let cur = e.character.as_ref().and_then(|c| chars.iter().find(|(id, _)| id == c)).map(|(_, n)| n.clone()).or_else(|| e.speaker.clone()).unwrap_or_else(|| tr("script.no_character").to_owned());
                let cc = egui::ComboBox::from_id_salt(("script_char", eid)).width(130.0).selected_text(cur).show_ui(ui, |ui| {
                    if ui.selectable_label(e.character.is_none(), tr("script.no_character")).clicked() {
                        touched = true;
                        pend.ops.push(ScriptOp::UpdateElement { element: e.id.clone(), patch: ElementPatch { character: Some(Id(String::new())), ..Default::default() } });
                    }
                    for (cid, name) in chars {
                        if ui.selectable_label(e.character.as_ref() == Some(cid), name).clicked() {
                            touched = true;
                            pend.ops.push(ScriptOp::UpdateElement { element: e.id.clone(), patch: ElementPatch { character: Some(cid.clone()), ..Default::default() } });
                        }
                    }
                });
                env.note(format!("script:char:{si}:{i}"), cc.response.rect);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let del = env.icon_button(ui, IconId::Trash, "script.delete_element", false, true, &format!("script:del:{si}:{i}"));
                let down = env.icon_button(ui, IconId::ArrowDown, "script.element_down", false, i + 1 < scene.elements.len(), &format!("script:down:{si}:{i}"));
                let up = env.icon_button(ui, IconId::ArrowUp, "script.element_up", false, i > 0, &format!("script:up:{si}:{i}"));
                if del.clicked() {
                    pend.ops.push(ScriptOp::DeleteElement { element: e.id.clone() });
                }
                if down.clicked() {
                    touched = true;
                    pend.ops.push(ScriptOp::MoveElement { element: e.id.clone(), scene: scene.id.clone(), index: Some(i + 1) });
                }
                if up.clicked() {
                    touched = true;
                    pend.ops.push(ScriptOp::MoveElement { element: e.id.clone(), scene: scene.id.clone(), index: Some(i - 1) });
                }
                if matches!(e.kind, ElementKind::Action | ElementKind::Dialogue) {
                    let cur = actions::element_secs(doc, e);
                    let mut secs_rect = egui::Rect::NOTHING;
                    let r = commit_on_release(ui, egui::Id::new(("script_secs", eid)), cur, |ui, v| {
                        let r = ui.add(egui::DragValue::new(v).range(0.0..=600.0).speed(0.1).max_decimals(1).suffix(" s"));
                        secs_rect = r.rect;
                        r
                    });
                    env.note(format!("script:secs:{si}:{i}"), secs_rect);
                    if let Some(v) = r {
                        touched = true;
                        pend.ops.push(ScriptOp::UpdateElement { element: e.id.clone(), patch: ElementPatch { est_seconds: Some(v), ..Default::default() } });
                    }
                }
            });
        });
        // ---- texto original e tradução
        let label_w = 64.0;
        let lang_orig = doc.language.to_uppercase();
        let lang_tr = doc.translation_language.to_uppercase();
        ui.horizontal_top(|ui| {
            ui.add_sized([label_w, 18.0], egui::Label::new(egui::RichText::new(lang_orig).size(11.5).color(t.text_dim)));
            let (r, c) = text_commit_multi(ui, egui::Id::new(("script_text", eid)), &e.text, ui.available_width(), tr("script.text_hint"));
            env.note(format!("script:text:{si}:{i}"), r.rect);
            if env.ed.script_focus.as_ref() == Some(&e.id) {
                env.ed.script_focus = None;
                r.request_focus();
                // o elemento novo pode nascer abaixo da área visível
                r.scroll_to_me(Some(Align::Center));
            }
            if r.gained_focus() {
                touched = true;
            }
            if let Some(x) = c {
                pend.ops.push(ScriptOp::UpdateElement { element: e.id.clone(), patch: ElementPatch { text: Some(x), ..Default::default() } });
            }
            ctrl_enter_new(ui, &r, scene, i, pend);
        });
        ui.horizontal_top(|ui| {
            ui.add_sized([label_w, 18.0], egui::Label::new(egui::RichText::new(lang_tr).size(11.5).color(t.text_dim)));
            let (r, c) = text_commit_multi(ui, egui::Id::new(("script_trans", eid)), &e.translation, ui.available_width(), tr("script.translation_hint"));
            env.note(format!("script:trans:{si}:{i}"), r.rect);
            if r.gained_focus() {
                touched = true;
            }
            if let Some(x) = c {
                pend.ops.push(ScriptOp::UpdateElement { element: e.id.clone(), patch: ElementPatch { translation: Some(x), ..Default::default() } });
            }
            ctrl_enter_new(ui, &r, scene, i, pend);
        });
        // ---- emoção e direção (falas)
        if e.kind == ElementKind::Dialogue {
            ui.horizontal(|ui| {
                ui.add_sized([label_w, 18.0], egui::Label::new(egui::RichText::new(tr("script.emotion")).size(11.5).color(t.text_dim)));
                let half = ((ui.available_width() - label_w - 12.0) / 2.0).max(60.0);
                let (r, c) = text_commit_opt(ui, egui::Id::new(("script_emotion", eid)), e.emotion.as_deref().unwrap_or(""), half, "");
                env.note(format!("script:emotion:{si}:{i}"), r.rect);
                if r.gained_focus() {
                    touched = true;
                }
                if let Some(x) = c {
                    pend.ops.push(ScriptOp::UpdateElement { element: e.id.clone(), patch: ElementPatch { emotion: Some(x), ..Default::default() } });
                }
                ui.add_sized([label_w, 18.0], egui::Label::new(egui::RichText::new(tr("script.direction")).size(11.5).color(t.text_dim)));
                let (r, c) = text_commit_opt(ui, egui::Id::new(("script_direction", eid)), e.direction.as_deref().unwrap_or(""), ui.available_width(), "");
                env.note(format!("script:direction:{si}:{i}"), r.rect);
                if r.gained_focus() {
                    touched = true;
                }
                if let Some(x) = c {
                    pend.ops.push(ScriptOp::UpdateElement { element: e.id.clone(), patch: ElementPatch { direction: Some(x), ..Default::default() } });
                }
            });
        }
    });
    env.note(format!("script:el:{si}:{i}"), fr.response.rect);
    if touched {
        env.ed.sel_element = Some(e.id.clone());
        env.ed.sel_scene = Some(scene.id.clone());
    }
}

/// Barra no fim da cena: nova ação, nova fala (com o personagem escolhido), transição e nota.
/// Ctrl+Enter num campo do elemento: cria o próximo logo abaixo e põe o cursor nele (o texto digitado é gravado quando o
/// campo perde o foco, no quadro seguinte).
fn ctrl_enter_new(ui: &mut Ui, r: &egui::Response, scene: &ScriptScene, i: usize, pend: &mut Pending) {
    if !r.has_focus() || !ui.input_mut(|inp| inp.consume_key(Modifiers::COMMAND, Key::Enter)) {
        return;
    }
    let (kind, character) = next_element(scene, i);
    pend.ops.push(ScriptOp::InsertElement { scene: scene.id.clone(), index: Some(i + 1), element: ElementSpec { kind, character, ..Default::default() } });
    pend.select = Select::NewElement;
}

fn add_bar(env: &mut Env, ui: &mut Ui, si: usize, scene: &ScriptScene, chars: &[(Id, String)], pend: &mut Pending) {
    // entra logo depois do elemento selecionado desta cena; sem seleção, no fim
    let after = env.ed.sel_element.as_ref().and_then(|s| scene.elements.iter().position(|e| &e.id == s)).map(|p| p + 1);
    let speaker = env.ed.script_speaker.clone().filter(|s| chars.iter().any(|(id, _)| id == s)).or_else(|| chars.first().map(|(id, _)| id.clone()));
    let add = |kind: ElementKind, character: Option<Id>, pend: &mut Pending| {
        pend.ops.push(ScriptOp::InsertElement { scene: scene.id.clone(), index: after, element: ElementSpec { kind, character, ..Default::default() } });
        pend.select = Select::NewElement;
    };
    ui.horizontal_wrapped(|ui| {
        let b = ui.button(tr("script.add_action")).on_hover_text(tr("script.ctrl_enter.tip"));
        env.note(format!("script:add_action:{si}"), b.rect);
        if b.clicked() {
            add(ElementKind::Action, None, pend);
        }
        let b = ui.button(tr("script.add_dialogue"));
        env.note(format!("script:add_dialogue:{si}"), b.rect);
        if b.clicked() {
            add(ElementKind::Dialogue, speaker.clone(), pend);
        }
        let cur = speaker.as_ref().and_then(|s| chars.iter().find(|(id, _)| id == s)).map(|(_, n)| n.clone()).unwrap_or_else(|| tr("script.no_character").to_owned());
        let cb = egui::ComboBox::from_id_salt(("script_speaker", si)).width(120.0).selected_text(cur).show_ui(ui, |ui| {
            for (cid, name) in chars {
                if ui.selectable_label(speaker.as_ref() == Some(cid), name).clicked() {
                    env.ed.script_speaker = Some(cid.clone());
                }
            }
        });
        env.note(format!("script:speaker:{si}"), cb.response.rect);
        let b = ui.button(tr("script.add_transition"));
        env.note(format!("script:add_transition:{si}"), b.rect);
        if b.clicked() {
            add(ElementKind::Transition, None, pend);
        }
        let b = ui.button(tr("script.add_note"));
        env.note(format!("script:add_note:{si}"), b.rect);
        if b.clicked() {
            add(ElementKind::Note, None, pend);
        }
    });
}

/// Propriedades do roteiro aberto (workspace Roteiro): título, idiomas e totais; aplica a estimativa de tempo aos elementos.
pub(crate) fn properties(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let Some((doc_id, doc)) = actions::sync_script_sel(env.ed, env.core) else {
        dim_label(ui, &t, tr("script.props_none"));
        return;
    };
    section(ui, &t, "script.props");
    let width = (ui.available_width() - 110.0).max(80.0);
    let mut ops: Vec<ScriptOp> = Vec::new();
    egui::Grid::new("script_props_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("script.title"));
        let (r, c) = text_commit_opt(ui, egui::Id::new("script_meta_title"), &doc.title, width, "");
        env.note("script:meta_title", r.rect);
        if let Some(x) = c {
            ops.push(ScriptOp::SetMeta { title: Some(x), language: None, translation_language: None });
        }
        ui.end_row();
        dim_label(ui, &t, tr("script.language"));
        let (r, c) = text_commit(ui, egui::Id::new("script_meta_lang"), &doc.language, width);
        env.note("script:meta_language", r.rect);
        if let Some(x) = c {
            ops.push(ScriptOp::SetMeta { title: None, language: Some(x), translation_language: None });
        }
        ui.end_row();
        dim_label(ui, &t, tr("script.translation_language"));
        let (r, c) = text_commit(ui, egui::Id::new("script_meta_tlang"), &doc.translation_language, width);
        env.note("script:meta_translation_language", r.rect);
        if let Some(x) = c {
            ops.push(ScriptOp::SetMeta { title: None, language: None, translation_language: Some(x) });
        }
        ui.end_row();
    });
    if !ops.is_empty() {
        let r = actions::script_edit(env.core, &doc_id, ops);
        actions::notify_err(env.ed, r);
    }
    ui.add_space(4.0);
    let elements: usize = doc.scenes.iter().map(|s| s.elements.len()).sum();
    let total: f32 = doc.scenes.iter().map(|s| actions::scene_secs(&doc, s)).sum();
    dim_label(ui, &t, &format!("{} {} · {} {} · {:.1} s", doc.scenes.len(), tr("outline.scenes"), elements, tr("outline.elements"), total));
    ui.add_space(6.0);
    let b = ui.button(tr("script.apply_timing"));
    env.note("script:apply_timing", b.rect);
    if b.on_hover_text(tr("script.apply_timing.tip")).clicked() {
        let r = actions::run(env.core, "SCRIPT", serde_json::json!({ "op": "apply_timing", "doc": doc_id }));
        actions::notify_err(env.ed, r);
    }
    // ---- personagens: falas e tempo de cada um (elenco, orçamento de voz)
    let chars = actions::characters(&env.core.project);
    let mut rows: Vec<(String, usize, f32)> = Vec::new();
    for s in &doc.scenes {
        for e in s.elements.iter().filter(|e| e.kind == ElementKind::Dialogue) {
            let name = e.character.as_ref().and_then(|c| chars.iter().find(|(id, _)| id == c)).map(|(_, n)| n.clone()).or_else(|| e.speaker.clone()).unwrap_or_else(|| tr("script.no_character").to_owned());
            let secs = actions::element_secs(&doc, e);
            match rows.iter_mut().find(|r| r.0 == name) {
                Some(r) => {
                    r.1 += 1;
                    r.2 += secs;
                }
                None => rows.push((name, 1, secs)),
            }
        }
    }
    if !rows.is_empty() {
        ui.add_space(8.0);
        section(ui, &t, "script.cast");
        rows.sort_by(|a, b| b.2.total_cmp(&a.2));
        egui::Grid::new("script_cast_grid").num_columns(3).spacing([12.0, 4.0]).striped(true).show(ui, |ui| {
            dim_label(ui, &t, tr("script.cast.name"));
            dim_label(ui, &t, tr("script.cast.lines"));
            dim_label(ui, &t, tr("script.cast.seconds"));
            ui.end_row();
            for (name, n, secs) in &rows {
                ui.label(name);
                ui.label(n.to_string());
                ui.label(format!("{secs:.1} s"));
                ui.end_row();
            }
        });
    }
    // ---- Fountain (o formato aberto de roteiro do Final Draft, Highland, WriterDuet…)
    ui.add_space(8.0);
    section(ui, &t, "script.fountain");
    ui.horizontal_wrapped(|ui| {
        let busy = env.ed.script_file.is_some();
        let e = ui.add_enabled(!busy, egui::Button::new(tr("script.fountain.export"))).on_hover_text(tr("script.fountain.tip"));
        env.note("script:fountain_export", e.rect);
        let i = ui.add_enabled(!busy, egui::Button::new(tr("script.fountain.import"))).on_hover_text(tr("script.fountain.tip"));
        env.note("script:fountain_import", i.rect);
        if e.clicked() || i.clicked() {
            let export = e.clicked();
            let (tx, rx) = crossbeam_channel::bounded(1);
            let start = env.core.path.clone();
            let title = if doc.title.trim().is_empty() { "roteiro".to_owned() } else { doc.title.clone() };
            let _ = std::thread::Builder::new().name("aiai-fountain-file".into()).spawn(move || {
                let mut d = rfd::FileDialog::new().add_filter("Fountain", &["fountain", "txt"]);
                if let Some(p) = start {
                    d = d.set_directory(p);
                }
                let r = if export { d.set_file_name(format!("{}.fountain", sanitize(&title))).save_file() } else { d.pick_file() };
                let _ = tx.send(r);
            });
            env.ed.script_file = Some((export, rx));
        }
    });
    poll_fountain(env, &doc_id);
}

/// Nome de arquivo sem os caracteres que o Windows recusa.
fn sanitize(s: &str) -> String {
    s.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect::<String>().trim().to_owned()
}

/// Resposta do diálogo do Fountain: exporta o roteiro aberto ou importa como roteiro novo (e abre).
fn poll_fountain(env: &mut Env, doc_id: &crate::app::commands::NodeId) {
    let Some((export, rx)) = env.ed.script_file.as_ref() else { return };
    let export = *export;
    let Ok(res) = rx.try_recv() else { return };
    env.ed.script_file = None;
    let Some(path) = res else { return };
    if export {
        let r = actions::run(env.core, "SCRIPT", serde_json::json!({ "op": "to_fountain", "doc": doc_id, "out": path.display().to_string() }));
        if actions::notify_err(env.ed, r).is_some() {
            env.ed.toast = Some(format!("{} {}", tr("script.fountain.exported"), path.display()));
        }
        return;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            env.ed.notify(format!("{}: {e}", path.display()));
            return;
        }
    };
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Roteiro".into());
    let r = actions::run(env.core, "SCRIPT", serde_json::json!({ "op": "from_fountain", "name": name, "fountain": text }));
    if let Some(v) = actions::notify_err(env.ed, r) {
        if let Some(id) = v.get("doc").and_then(|d| d.as_str()) {
            env.ed.sel_script = Some(Id(id.to_owned()));
            env.ed.sel_scene = None;
            env.ed.sel_element = None;
        }
        env.ed.toast = Some(format!("{} {}", tr("script.fountain.imported"), path.display()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(kind: ElementKind, who: Option<&str>) -> ScriptElement {
        let mut e = ElementSpec { kind, character: who.map(|w| Id(w.into())), ..Default::default() }.into_element(Id(format!("e{}", who.unwrap_or("-"))));
        e.kind = kind;
        e
    }

    #[test]
    fn ctrl_enter_alternates_the_speakers_and_follows_the_screenplay_order() {
        let scene = ScriptScene { id: Id("s".into()), heading: "S".into(), elements: vec![el(ElementKind::Action, None), el(ElementKind::Dialogue, Some("hana")), el(ElementKind::Dialogue, Some("ren")), el(ElementKind::Parenthetical, None), el(ElementKind::Transition, None)] };
        assert_eq!(next_element(&scene, 0), (ElementKind::Action, None));
        assert_eq!(next_element(&scene, 1), (ElementKind::Dialogue, Some(Id("hana".into()))), "sem conversa antes: o mesmo");
        assert_eq!(next_element(&scene, 2), (ElementKind::Dialogue, Some(Id("hana".into()))), "a conversa alterna");
        assert_eq!(next_element(&scene, 3), (ElementKind::Dialogue, Some(Id("ren".into()))), "parentético: a fala de quem falava");
        assert_eq!(next_element(&scene, 4), (ElementKind::Action, None));
        assert_eq!(sanitize("A: rosa?"), "A_ rosa_");
    }
}
