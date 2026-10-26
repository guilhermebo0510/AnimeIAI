//! Pasta do personagem (workspace Personagens): a árvore dos materiais do personagem agrupados por tipo (folha de modelo,
//! expressões, tabela de boca, olhos, figurinos e folhas extras), os botões que criam cada tipo a partir de um modelo
//! (grade de células com rótulo) e a importação de imagem, e as operações sobre o material escolhido (abrir para
//! desenhar, renomear, duplicar, apagar, subir e descer). O canvas do workspace mostra o material aberto: quem manda o
//! corte ativo é `tick`, chamado a cada quadro.
//!
//! Tudo passa pela ação `CHARACTER_MATERIAL` da bridge (o mesmo caminho do MCP): um gesto = um passo de desfazer. Os
//! projetos antigos, sem lista de materiais, mostram os cortes soltos do personagem (ver `aiai_tools::char_material`).

use super::characters::sync_sel;
use crate::app::actions;
use crate::app::commands::{self as cmds, NodeId};
use crate::app::editor::Editor;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::workspace::WorkspaceId;
use aiai_core::{CharMaterial, CoreSession, Id, MaterialContent, MaterialKind, NodeKind, Project};
use aiai_tools::char_material as mat;
use egui::{Rect, RichText, Ui};
use serde_json::{Value, json};

/// Título (plural, do grupo da árvore) de um tipo de material.
pub(crate) fn kind_title(k: MaterialKind) -> &'static str {
    tr(match k {
        MaterialKind::ModelSheet => "char.kind.model_sheet",
        MaterialKind::Expressions => "char.kind.expressions",
        MaterialKind::MouthChart => "char.kind.mouth_chart",
        MaterialKind::Eyes => "char.kind.eyes",
        MaterialKind::Outfit => "char.kind.outfit",
        MaterialKind::Other => "char.kind.other",
    })
}

/// Texto do botão que cria um material do tipo.
fn new_title(k: MaterialKind) -> &'static str {
    tr(match k {
        MaterialKind::ModelSheet => "char.new.model_sheet",
        MaterialKind::Expressions => "char.new.expressions",
        MaterialKind::MouthChart => "char.new.mouth_chart",
        MaterialKind::Eyes => "char.new.eyes",
        MaterialKind::Outfit => "char.new.outfit",
        MaterialKind::Other => "char.new.other",
    })
}

/// Dica do botão de criar (o que o modelo traz).
fn new_tip(k: MaterialKind) -> &'static str {
    tr(match k {
        MaterialKind::ModelSheet => "char.new.model_sheet.tip",
        MaterialKind::Expressions => "char.new.expressions.tip",
        MaterialKind::MouthChart => "char.new.mouth_chart.tip",
        MaterialKind::Eyes => "char.new.eyes.tip",
        MaterialKind::Outfit => "char.new.outfit.tip",
        MaterialKind::Other => "char.new.other.tip",
    })
}

/// Nome padrão de um material novo no idioma da interface ("Expressões", "Figurino 2"...).
fn default_name(k: MaterialKind, existing: &[CharMaterial]) -> String {
    let base = tr(match k {
        MaterialKind::ModelSheet => "char.default.model_sheet",
        MaterialKind::Expressions => "char.default.expressions",
        MaterialKind::MouthChart => "char.default.mouth_chart",
        MaterialKind::Eyes => "char.default.eyes",
        MaterialKind::Outfit => "char.default.outfit",
        MaterialKind::Other => "char.default.other",
    });
    mat::name_with(base, k, existing)
}

/// Rótulos das células do modelo no idioma da interface (lista separada por vírgulas no i18n).
fn labels_for(k: MaterialKind) -> Option<Vec<String>> {
    let key = match k {
        MaterialKind::Expressions => "char.labels.expressions",
        MaterialKind::MouthChart => "char.labels.mouth_chart",
        MaterialKind::Eyes => "char.labels.eyes",
        _ => return None,
    };
    Some(tr(key).split(',').map(|s| s.trim().to_owned()).collect())
}

/// Materiais do personagem (a lista guardada mais os cortes soltos de projetos antigos).
pub(crate) fn list_of(p: &Project, ch: &NodeId) -> Vec<CharMaterial> {
    mat::materials(p, ch)
}

/// Material aberto no canvas: o escolhido na árvore, senão a folha de modelo (ou o primeiro material).
pub(crate) fn open_of(env: &Env, ch: &NodeId) -> Option<CharMaterial> {
    pick_open(&list_of(&env.core.project, ch), env.ed.chars.open.as_ref())
}

fn pick_open(list: &[CharMaterial], open: Option<&Id>) -> Option<CharMaterial> {
    open.and_then(|id| list.iter().find(|m| &m.id == id)).or_else(|| list.iter().find(|m| m.kind == MaterialKind::ModelSheet)).or_else(|| list.first()).cloned()
}

/// Corte do material aberto do personagem selecionado.
pub(crate) fn open_cut(env: &Env, ch: &NodeId) -> Option<NodeId> {
    open_of(env, ch).map(|m| m.content.cut().clone())
}

fn is_character_cut(p: &Project, cut: &NodeId) -> bool {
    p.node(cut).and_then(|n| n.parent.as_ref()).and_then(|par| p.node(par)).is_some_and(|n| matches!(n.data, NodeKind::Character(_)))
}

/// A cada quadro: no workspace Personagens o corte ativo é o do material aberto (o canvas, as camadas e a paleta
/// seguem). Ao sair, volta ao corte que estava ativo antes (a menos que o material tenha sido aberto na Animação).
pub(crate) fn tick(core: &CoreSession, ed: &mut Editor, ws: WorkspaceId) {
    let st = &mut ed.chars;
    if ws != WorkspaceId::Personagens {
        if st.entered {
            st.entered = false;
            let (back, pushed) = (st.return_cut.take(), st.pushed.take());
            if let Some((camera, guides)) = st.saved_view.take() {
                ed.show_camera = camera;
                ed.show_guides = guides;
            }
            if !std::mem::take(&mut st.keep_cut) && pushed.is_some() && ed.cut == pushed {
                if let Some(rc) = back.filter(|c| cmds::is_cut(&core.project, c)) {
                    ed.set_cut(rc);
                }
            }
        }
        return;
    }
    let p = &core.project;
    if !st.entered {
        st.entered = true;
        st.pushed = None;
        st.return_cut = ed.cut.clone().filter(|c| cmds::is_cut(p, c) && !is_character_cut(p, c));
        // as folhas não têm câmera e as guias (grade das células) são o ponto delas: ao sair volta como estava
        st.saved_view = Some((ed.show_camera, ed.show_guides));
        ed.show_camera = false;
        ed.show_guides = true;
    }
    let st = &mut ed.chars;
    let all = &p.roots.characters;
    if st.sel.as_ref().is_none_or(|s| !all.contains(s)) {
        st.sel = all.first().cloned();
    }
    let Some(ch) = st.sel.clone() else { return };
    let list = list_of(p, &ch);
    let Some(m) = pick_open(&list, st.open.as_ref()) else { return };
    st.open = Some(m.id.clone());
    let cut = m.content.cut().clone();
    if ed.cut.as_ref() != Some(&cut) {
        ed.set_cut(cut.clone());
        ed.chars.pushed = Some(cut.clone());
        // desenhar cai na camada de linha, não na de cima por acaso (o rótulo ou a imagem)
        ed.layer = line_layer(p, &cut);
        // as cores são as do figurino (variante de paleta), senão as de settei do personagem
        let pal = m.palette.clone().filter(|pal| cmds::palette_data(p, pal).is_some()).or_else(|| super::characters::char_data(p, &ch).and_then(|c| c.palette.clone()));
        if pal.is_some() {
            ed.palette = pal;
        }
    }
}

/// Camada "linha" do corte do material (onde se desenha), se existir.
fn line_layer(p: &Project, cut: &NodeId) -> Option<NodeId> {
    cmds::layer_rows(p, cut).into_iter().map(|r| r.id).find(|id| p.node(id).is_some_and(|n| n.name == "linha"))
}

/// Chama `CHARACTER_MATERIAL`; erro vira aviso. Devolve a resposta.
fn call(env: &mut Env, mut params: Value) -> Option<Value> {
    if let Some(o) = params.as_object_mut() {
        o.entry("character").or_insert_with(|| json!(env.ed.chars.sel));
    }
    let r = actions::run(env.core, "CHARACTER_MATERIAL", params);
    actions::notify_err(env.ed, r)
}

fn id_of(v: &Value, key: &str) -> Option<Id> {
    v.get(key).and_then(Value::as_str).map(|s| Id(s.to_owned()))
}

/// Cria um material do tipo e o abre.
fn create(env: &mut Env, kind: MaterialKind) {
    let Some(ch) = env.ed.chars.sel.clone() else { return };
    let list = list_of(&env.core.project, &ch);
    let mut params = json!({ "action": "create", "kind": kind.key(), "name": default_name(kind, &list) });
    if let Some(l) = labels_for(kind) {
        params["labels"] = json!(l);
    }
    if matches!(kind, MaterialKind::ModelSheet | MaterialKind::Outfit) {
        params["figure_px"] = json!(env.ed.chars.figure_px);
    }
    if let Some(v) = call(env, params) {
        if let Some(id) = id_of(&v, "material") {
            env.ed.chars.open = Some(id);
        }
    }
}

/// Abre o diálogo de arquivo (thread própria) para importar uma imagem como material.
fn start_import(env: &mut Env) {
    if env.ed.chars.import_job.is_some() {
        return;
    }
    let (tx, rx) = crossbeam_channel::bounded(1);
    let start = env.core.path.clone();
    let _ = std::thread::Builder::new().name("aiai-import-material".into()).spawn(move || {
        let mut d = rfd::FileDialog::new().add_filter("Imagem", &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "gif"]);
        if let Some(p) = start {
            d = d.set_directory(p);
        }
        let _ = tx.send(d.pick_file());
    });
    env.ed.chars.import_job = Some(rx);
}

/// Resultado do diálogo de importação, se já veio: cria o material com a imagem.
fn poll_import(env: &mut Env, ui: &Ui) {
    let Some(rx) = &env.ed.chars.import_job else { return };
    match rx.try_recv() {
        Ok(picked) => {
            env.ed.chars.import_job = None;
            if let Some(path) = picked {
                if let Some(v) = call(env, json!({ "action": "import", "kind": "other", "path": path.display().to_string() })) {
                    if let Some(id) = id_of(&v, "material") {
                        env.ed.chars.open = Some(id);
                    }
                }
            }
        }
        Err(crossbeam_channel::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
        Err(_) => env.ed.chars.import_job = None,
    }
}

/// Sobe ou desce o material entre os do mesmo tipo (troca de lugar na lista com o vizinho do grupo).
fn shift(env: &mut Env, list: &[CharMaterial], id: &Id, up: bool) {
    let Some(i) = list.iter().position(|m| &m.id == id) else { return };
    let kind = list[i].kind;
    let neighbor = if up { list[..i].iter().rposition(|m| m.kind == kind) } else { list[i + 1..].iter().position(|m| m.kind == kind).map(|k| k + i + 1) };
    if let Some(to) = neighbor {
        call(env, json!({ "action": "move", "material": id, "index": to }));
    }
}

/// Primeiro e último do grupo (para desligar os botões Subir/Descer).
fn group_edges(list: &[CharMaterial], id: &Id) -> (bool, bool) {
    let Some(i) = list.iter().position(|m| &m.id == id) else { return (false, false) };
    let kind = list[i].kind;
    (list[..i].iter().any(|m| m.kind == kind), list[i + 1..].iter().any(|m| m.kind == kind))
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let Some(ch) = sync_sel(env) else {
        ui.label(RichText::new(tr("char.folder.empty")).color(t.text_dim));
        return;
    };
    poll_import(env, ui);
    let name = env.core.project.node(&ch).map(|n| n.name.clone()).unwrap_or_default();
    ui.label(RichText::new(name).strong().size(13.0));
    let list = list_of(&env.core.project, &ch);
    let open_id = open_of(env, &ch).map(|m| m.id);

    // criar: um botão por tipo (cada um já traz o modelo pronto) e a importação de imagem
    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
    ui.horizontal_wrapped(|ui| {
        for kind in [MaterialKind::ModelSheet, MaterialKind::Expressions, MaterialKind::MouthChart, MaterialKind::Eyes, MaterialKind::Outfit, MaterialKind::Other] {
            let b = ui.small_button(new_title(kind)).on_hover_text(new_tip(kind));
            env.note(format!("charfolder:new:{}", kind.key()), b.rect);
            if b.clicked() {
                create(env, kind);
            }
        }
        let b = ui.add_enabled(env.ed.chars.import_job.is_none(), egui::Button::new(tr("char.import")).small()).on_hover_text(tr("char.import.tip"));
        env.note("charfolder:import", b.rect);
        if b.clicked() {
            start_import(env);
        }
    });
    ui.add_space(2.0);
    section(ui, &t, "char.folder.materials");
    if list.is_empty() {
        dim_label(ui, &t, tr("char.folder.none"));
        return;
    }

    // operações sobre o aberto
    let mut act: Option<Act> = None;
    ui.horizontal_wrapped(|ui| {
        let Some(id) = &open_id else { return };
        let (can_up, can_down) = group_edges(&list, id);
        for (label, tip, name, enabled, a) in [
            (tr("char.act.rename"), tr("char.act.rename.tip"), "charfolder:rename", true, Act::Rename),
            (tr("char.act.duplicate"), tr("char.act.duplicate.tip"), "charfolder:duplicate", true, Act::Duplicate),
            (tr("char.act.up"), tr("char.act.up.tip"), "charfolder:up", can_up, Act::Up),
            (tr("char.act.down"), tr("char.act.down.tip"), "charfolder:down", can_down, Act::Down),
            (tr("char.act.delete"), tr("char.act.delete.tip"), "charfolder:delete", true, Act::Delete),
        ] {
            let b = ui.add_enabled(enabled, egui::Button::new(label).small()).on_hover_text(tip);
            env.note(name, b.rect);
            if b.clicked() {
                act = Some(a);
            }
        }
    });
    ui.add_space(2.0);

    // árvore: um grupo por tipo, só dos que têm material
    let mut open_click: Option<Id> = None;
    let mut menu_act: Option<(Id, Act)> = None;
    egui::ScrollArea::vertical().id_salt("char_folder_tree").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        let mut row_i = 0usize;
        for kind in MaterialKind::ALL {
            let group: Vec<&CharMaterial> = list.iter().filter(|m| m.kind == kind).collect();
            if group.is_empty() {
                continue;
            }
            let head = format!("{} ({})", kind_title(kind), group.len());
            let r = egui::CollapsingHeader::new(RichText::new(head).size(12.0).strong()).id_salt(("char_folder_kind", &ch.0, kind.key())).default_open(true).show(ui, |ui| {
                for m in group {
                    let selected = open_id.as_ref() == Some(&m.id);
                    let renaming = env.ed.chars.rename.as_ref().is_some_and(|(id, _)| id == &m.id);
                    if renaming {
                        let (id, buf) = env.ed.chars.rename.as_mut().expect("renomeando");
                        let r = ui.add(egui::TextEdit::singleline(buf).desired_width(ui.available_width() - 4.0));
                        env.notes.push((format!("charfolder:rename_edit:{row_i}"), r.rect));
                        if !r.has_focus() && !r.lost_focus() {
                            r.request_focus();
                        }
                        let (esc, enter) = ui.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Enter)));
                        if esc {
                            env.ed.chars.rename = None;
                        } else if enter || r.lost_focus() {
                            let (id, buf) = (id.clone(), buf.trim().to_owned());
                            env.ed.chars.rename = None;
                            if !buf.is_empty() && buf != m.name {
                                call(env, json!({ "action": "rename", "material": id, "name": buf }));
                            }
                        }
                    } else {
                        let suffix = if matches!(m.content, MaterialContent::Image { .. }) { format!("  ·  {}", tr("char.imported_short")) } else { String::new() };
                        let r = ui.selectable_label(selected, format!("{}{suffix}", m.name));
                        env.note(format!("charfolder:row:{row_i}"), r.rect);
                        if r.clicked() {
                            open_click = Some(m.id.clone());
                        }
                        if r.double_clicked() {
                            menu_act = Some((m.id.clone(), Act::Rename));
                        }
                        r.context_menu(|ui| {
                            for (label, a) in [(tr("char.act.open_anim"), Act::OpenAnim), (tr("char.act.rename"), Act::Rename), (tr("char.act.duplicate"), Act::Duplicate), (tr("char.act.up"), Act::Up), (tr("char.act.down"), Act::Down), (tr("char.act.delete"), Act::Delete)] {
                                if ui.button(label).clicked() {
                                    menu_act = Some((m.id.clone(), a));
                                    ui.close();
                                }
                            }
                        });
                    }
                    row_i += 1;
                }
            });
            env.note(format!("charfolder:group:{}", kind.key()), r.header_response.rect);
        }
    });
    if let Some(id) = open_click {
        env.ed.chars.open = Some(id);
    }
    if let Some((id, a)) = menu_act {
        env.ed.chars.open = Some(id);
        act = Some(a);
    }
    if let (Some(a), Some(id)) = (act, open_of(env, &ch).map(|m| m.id)) {
        apply_act(env, &list, &id, a);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Rename,
    Duplicate,
    Delete,
    Up,
    Down,
    OpenAnim,
}

fn apply_act(env: &mut Env, list: &[CharMaterial], id: &Id, a: Act) {
    let name = list.iter().find(|m| &m.id == id).map(|m| m.name.clone()).unwrap_or_default();
    match a {
        Act::Rename => env.ed.chars.rename = Some((id.clone(), name)),
        Act::Duplicate => {
            if let Some(v) = call(env, json!({ "action": "duplicate", "material": id, "name": crate::behavior_prefs::copy_name_tr(&name, env.ed.beh.prefs.copy_name) })) {
                if let Some(n) = id_of(&v, "material") {
                    env.ed.chars.open = Some(n);
                }
            }
        }
        Act::Delete => {
            if call(env, json!({ "action": "delete", "material": id })).is_some() {
                env.ed.chars.open = None;
                env.ed.chars.rename = None;
            }
        }
        Act::Up => shift(env, list, id, true),
        Act::Down => shift(env, list, id, false),
        Act::OpenAnim => open_in_animation(env, list, id),
    }
}

/// Abre o corte do material no canvas da Animação (mantém o corte ativo ao sair do workspace).
pub(crate) fn open_in_animation(env: &mut Env, list: &[CharMaterial], id: &Id) {
    let Some(m) = list.iter().find(|m| &m.id == id) else { return };
    env.ed.set_cut(m.content.cut().clone());
    env.ed.chars.keep_cut = true;
    env.ed.want_workspace = Some(WorkspaceId::Animacao);
}

// ------------------------------------------------------------------------------------------------ aba Material (ficha)

/// Aba "Material" da ficha: nome, tipo, notas, variante de paleta do figurino e o atalho para a Animação.
pub(crate) fn material_tab(env: &mut Env, ui: &mut Ui, ch: &NodeId, w: f32) {
    let t = env.t;
    let list = list_of(&env.core.project, ch);
    let Some(m) = pick_open(&list, env.ed.chars.open.as_ref()) else {
        dim_label(ui, &t, tr("char.folder.none"));
        return;
    };
    section(ui, &t, "char.sec.material");
    egui::Grid::new(("char_mat", &m.id.0)).num_columns(2).spacing([8.0, 5.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("char.f.name"));
        let (r, out) = super::text_commit(ui, egui::Id::new(("char_mat_name", &m.id.0)), &m.name, (w - 90.0).max(60.0));
        env.note("charmat:name", r.rect);
        if let Some(n) = out {
            call(env, json!({ "action": "rename", "material": m.id, "name": n }));
        }
        ui.end_row();
        dim_label(ui, &t, tr("char.f.kind"));
        let mut pick: Option<MaterialKind> = None;
        let cb = egui::ComboBox::from_id_salt(("char_mat_kind", &m.id.0)).width((w - 90.0).max(80.0)).selected_text(kind_title(m.kind)).show_ui(ui, |ui| {
            for k in MaterialKind::ALL {
                if ui.selectable_label(m.kind == k, kind_title(k)).clicked() {
                    pick = Some(k);
                }
            }
        });
        env.note("charmat:kind", cb.response.rect);
        if let Some(k) = pick.filter(|k| *k != m.kind) {
            call(env, json!({ "action": "update", "material": m.id, "kind": k.key() }));
        }
        ui.end_row();
    });
    dim_label(ui, &t, tr("char.f.notes"));
    let (r, out) = super::text_commit_multi(ui, egui::Id::new(("char_mat_notes", &m.id.0)), &m.notes, w, tr("char.f.notes_hint"));
    env.note("charmat:notes", r.rect);
    if let Some(n) = out {
        call(env, json!({ "action": "update", "material": m.id, "notes": n }));
    }
    if m.kind == MaterialKind::Outfit {
        section(ui, &t, "char.sec.outfit_palette");
        let pals: Vec<(NodeId, String)> = env.core.project.roots.palettes.iter().filter_map(|id| env.core.project.node(id).map(|n| (id.clone(), n.name.clone()))).collect();
        let cur = m.palette.as_ref().and_then(|p| pals.iter().find(|(id, _)| id == p)).map(|(_, n)| n.clone()).unwrap_or_else(|| tr("palette.none").to_owned());
        let mut pick: Option<NodeId> = None;
        ui.horizontal(|ui| {
            let cb = egui::ComboBox::from_id_salt(("char_mat_pal", &m.id.0)).width((w - 110.0).max(80.0)).selected_text(cur).show_ui(ui, |ui| {
                for (id, n) in &pals {
                    if ui.selectable_label(m.palette.as_ref() == Some(id), n).clicked() {
                        pick = Some(id.clone());
                    }
                }
            });
            env.note("charmat:palette", cb.response.rect);
            let b = ui.button(tr("char.new_palette"));
            env.note("charmat:new_palette", b.rect);
            if b.clicked() {
                let r = cmds::add_palette(env.core);
                if let Some(pal) = actions::notify_err(env.ed, r) {
                    let who = env.core.project.node(ch).map(|n| n.name.clone()).unwrap_or_default();
                    let _ = cmds::rename_node(env.core, &pal, &format!("{who} · {}", m.name));
                    pick = Some(pal);
                }
            }
        });
        if let Some(pal) = pick {
            env.ed.palette = Some(pal.clone());
            call(env, json!({ "action": "update", "material": m.id, "palette": pal }));
        }
        dim_label(ui, &t, tr("char.outfit_palette_hint"));
    }
    if let MaterialContent::Image { blob, .. } = &m.content {
        dim_label(ui, &t, &format!("{}: {}", tr("char.imported"), &blob.0[..blob.0.len().min(12)]));
    }
    ui.add_space(6.0);
    let b = ui.button(tr("char.act.open_anim"));
    env.note("charmat:open_anim", b.rect);
    if b.on_hover_text(tr("char.open_sheet.tip")).clicked() {
        open_in_animation(env, &list, &m.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::doc::AUTHOR;

    fn core_with_char() -> (CoreSession, NodeId) {
        let mut core = CoreSession::for_tests("t");
        core.author = AUTHOR.into();
        let ch = super::super::characters::add_character_named(&mut core, "Hana", 6.5).unwrap();
        (core, ch)
    }

    fn create_via_action(core: &mut CoreSession, ch: &NodeId, kind: MaterialKind) -> Value {
        let list = list_of(&core.project, ch);
        let mut params = json!({ "character": ch, "action": "create", "kind": kind.key(), "name": default_name(kind, &list) });
        if let Some(l) = labels_for(kind) {
            params["labels"] = json!(l);
        }
        actions::run(core, "CHARACTER_MATERIAL", params).unwrap()
    }

    #[test]
    fn every_kind_creates_from_its_template_with_ui_labels() {
        let (mut core, ch) = core_with_char();
        for kind in MaterialKind::ALL {
            let v = create_via_action(&mut core, &ch, kind);
            assert!(v["material"].is_string() && v["cut"].is_string(), "{kind:?}: {v}");
            if matches!(kind, MaterialKind::Expressions | MaterialKind::MouthChart | MaterialKind::Eyes) {
                let cells = v["cells"].as_array().unwrap();
                assert!(cells.len() >= 9, "{kind:?}");
                assert!(cells.iter().all(|c| !c["label"].as_str().unwrap_or("").is_empty()), "{kind:?}: rótulo vazio");
            }
        }
        let list = list_of(&core.project, &ch);
        assert_eq!(list.len(), 6);
        assert_eq!(list.iter().map(|m| m.kind).collect::<Vec<_>>(), MaterialKind::ALL.to_vec());
        core.project.validate().unwrap();
        // abrir: sem escolha vale a folha de modelo; com escolha vale a escolhida; id solto cai na folha
        assert_eq!(pick_open(&list, None).unwrap().kind, MaterialKind::ModelSheet);
        assert_eq!(pick_open(&list, Some(&list[3].id)).unwrap().id, list[3].id);
        assert_eq!(pick_open(&list, Some(&Id("sumiu".into()))).unwrap().kind, MaterialKind::ModelSheet);
    }

    #[test]
    fn labels_cover_every_cell_in_both_languages() {
        for lang in [crate::i18n::Lang::Pt, crate::i18n::Lang::En] {
            crate::i18n::set_lang(lang);
            for (k, cells) in [(MaterialKind::Expressions, 12), (MaterialKind::MouthChart, 9), (MaterialKind::Eyes, 9)] {
                assert_eq!(labels_for(k).unwrap().len(), cells, "{lang:?} {k:?}");
            }
        }
        crate::i18n::set_lang(crate::i18n::Lang::Pt);
    }

    #[test]
    fn group_edges_and_default_names_follow_the_kind() {
        let (mut core, ch) = core_with_char();
        create_via_action(&mut core, &ch, MaterialKind::Outfit);
        create_via_action(&mut core, &ch, MaterialKind::Eyes);
        create_via_action(&mut core, &ch, MaterialKind::Outfit);
        let list = list_of(&core.project, &ch);
        assert_eq!(list[0].name, "Figurino 1");
        assert_eq!(list[2].name, "Figurino 2");
        // o 1º figurino só desce; o 2º só sobe; o dos olhos não mexe
        assert_eq!(group_edges(&list, &list[0].id), (false, true));
        assert_eq!(group_edges(&list, &list[2].id), (true, false));
        assert_eq!(group_edges(&list, &list[1].id), (false, false));
    }

    #[test]
    fn tick_follows_the_open_material_and_returns_to_the_scene_cut() {
        let (mut core, ch) = core_with_char();
        let scene_cut = cmds::quick_start(&mut core, 12, None).unwrap();
        let a = create_via_action(&mut core, &ch, MaterialKind::Expressions);
        let b = create_via_action(&mut core, &ch, MaterialKind::Eyes);
        let (ca, cb) = (id_of(&a, "cut").unwrap(), id_of(&b, "cut").unwrap());
        let mut ed = Editor::default();
        ed.cut = Some(scene_cut.clone());
        ed.chars.sel = Some(ch.clone());
        ed.chars.open = id_of(&a, "material");
        tick(&core, &mut ed, WorkspaceId::Personagens);
        assert_eq!(ed.cut, Some(ca.clone()));
        // a camada ativa é a de linha (onde se desenha)
        assert_eq!(ed.layer.as_ref().and_then(|l| core.project.node(l)).map(|n| n.name.as_str()), Some("linha"));
        ed.chars.open = id_of(&b, "material");
        tick(&core, &mut ed, WorkspaceId::Personagens);
        assert_eq!(ed.cut, Some(cb));
        // sair do workspace devolve o corte da cena
        tick(&core, &mut ed, WorkspaceId::Layout);
        assert_eq!(ed.cut, Some(scene_cut.clone()));
        // abrir na Animação mantém o material
        ed.chars.open = id_of(&a, "material");
        tick(&core, &mut ed, WorkspaceId::Personagens);
        ed.chars.keep_cut = true;
        tick(&core, &mut ed, WorkspaceId::Animacao);
        assert_eq!(ed.cut, Some(ca));
    }
}
