//! Painel Histórico (S25): os passos de desfazer do documento numa lista com nomes legíveis (ferramenta ou comando e a
//! camada), do mais antigo ao mais novo, com os passos desfeitos (que ainda podem ser refeitos) atenuados embaixo. Clicar
//! num passo leva o documento até ele (desfaz ou refaz quantos passos for preciso); o passo atual fica destacado. A
//! primeira linha é o estado antes de tudo. Vale nos workspaces de canvas (a Modelagem tem histórico próprio).

use crate::app::doc::AUTHOR;
use crate::app::ui_kit::{Env, body_ui, paint_icon};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::{Command, NodeKind, Project};
use egui::{Align2, FontId, Rect, Sense, Ui};

/// Uma linha da lista: ícone, texto e se o passo está aplicado (acima do atual) ou desfeito (abaixo).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Step {
    pub icon: IconId,
    pub label: String,
    pub applied: bool,
}

/// Quantos desfazer (`true`) ou refazer (`false`) levam do estado `cur` (passos aplicados) ao estado `target`.
/// `None` = já está lá.
pub(crate) fn plan_jump(cur: usize, target: usize) -> Option<(bool, usize)> {
    match target.cmp(&cur) {
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Less => Some((true, cur - target)),
        std::cmp::Ordering::Greater => Some((false, target - cur)),
    }
}

/// Nome da camada dona de um cel (ou do próprio nó, se for camada).
fn owner_name(p: &Project, id: &aiai_core::Id) -> Option<String> {
    let n = p.node(id)?;
    match &n.data {
        NodeKind::Cel(_) => n.parent.as_ref().and_then(|l| p.node(l)).map(|l| l.name.clone()),
        _ => Some(n.name.clone()),
    }
}

fn with_name(base: &str, name: Option<String>) -> String {
    match name.filter(|n| !n.is_empty()) {
        Some(n) => format!("{base} · {n}"),
        None => base.to_owned(),
    }
}

fn count(base: &str, n: usize) -> String {
    if n > 1 { format!("{base} ×{n}") } else { base.to_owned() }
}

/// Ícone e texto legível de um comando do histórico.
pub(crate) fn step_label(p: &Project, c: &Command) -> (IconId, String) {
    match c {
        Command::Batch { label: Some(l), commands } => {
            let icon = commands.first().map(|c| step_label(p, c).0).unwrap_or(IconId::History);
            (icon, l.clone())
        }
        Command::Batch { label: None, commands } if commands.len() == 1 => step_label(p, &commands[0]),
        Command::Batch { label: None, commands } => (IconId::History, tr("hist.batch").replace("{n}", &commands.len().to_string())),
        Command::InsertStrokes { cel, items } => (IconId::ToolPen, with_name(&count(tr("hist.strokes"), items.len()), owner_name(p, cel))),
        Command::RemoveStrokes { cel, ids } => (IconId::ToolEraser, with_name(&count(tr("hist.erase_strokes"), ids.len()), owner_name(p, cel))),
        Command::ReplaceStrokes { cel, .. } => (IconId::ToolPoints, with_name(tr("hist.edit_strokes"), owner_name(p, cel))),
        Command::SetTiles { cel, .. } => (IconId::ToolBrush, with_name(tr("hist.paint"), owner_name(p, cel))),
        Command::ReplaceFills { cel, .. } => (IconId::ToolFill, with_name(tr("hist.fill"), owner_name(p, cel))),
        Command::CreateNode { name, data, .. } => created(data, name),
        // refazer uma criação volta como "restaurar": mesmo rótulo da criação
        Command::RestoreNodes { nodes, .. } => match nodes.first() {
            Some(n) => created(&n.data, &n.name),
            None => (IconId::History, tr("hist.restore").to_owned()),
        },
        Command::DeleteNode { id } => (IconId::Trash, with_name(tr("hist.delete"), owner_name(p, id))),
        Command::MoveNode { id, .. } => (IconId::ArrowUp, with_name(tr("hist.move"), owner_name(p, id))),
        Command::SetProps { id, name, data } => {
            let keys: Vec<&str> = data.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
            let has = |k: &str| keys.contains(&k);
            let (icon, key) = if has("visible") {
                (IconId::Eye, "hist.visible")
            } else if has("locked") {
                (IconId::Lock, "hist.lock")
            } else if has("opacity") {
                (IconId::LayerRaster, "hist.opacity")
            } else if has("blend") {
                (IconId::LayerRaster, "hist.blend")
            } else if has("clip_to_below") {
                (IconId::ClipBelow, "hist.clip")
            } else if has("effects") {
                (IconId::Filter, "hist.effects")
            } else if keys.is_empty() && name.is_some() {
                (IconId::ToolText, "hist.rename")
            } else {
                (IconId::History, "hist.props")
            };
            (icon, with_name(tr(key), owner_name(p, id)))
        }
        Command::ReplaceData { id, .. } => (IconId::History, with_name(tr("hist.edit"), owner_name(p, id))),
        Command::SetProjectProps { .. } => (IconId::NodeDocument, tr("hist.project").to_owned()),
        Command::SetCells { cut, .. } => (IconId::NodeCut, with_name(tr("hist.cells"), owner_name(p, cut))),
        Command::AddTrack { cut, .. } | Command::RemoveTrack { cut, .. } => (IconId::NodeCut, with_name(tr("hist.track"), owner_name(p, cut))),
    }
}

fn created(data: &NodeKind, name: &str) -> (IconId, String) {
    let (icon, key) = match data {
        NodeKind::Layer(l) => (super::layers::kind_icon(l.kind), "hist.new_layer"),
        NodeKind::Cel(_) => (IconId::Plus, "hist.new_cel"),
        NodeKind::Cut(_) => (IconId::NodeCut, "hist.new_cut"),
        NodeKind::Palette(_) => (IconId::Palette, "hist.new_palette"),
        _ => (IconId::Plus, "hist.new_node"),
    };
    (icon, with_name(tr(key), Some(name.to_owned())))
}

/// As linhas do histórico (sem a linha do estado inicial) e quantos passos estão aplicados.
pub(crate) fn steps(core: &aiai_core::CoreSession) -> (Vec<Step>, usize) {
    let (undo, redo) = core.history_commands();
    let cur = undo.len();
    let p = &core.project;
    let rows = undo.iter().map(|c| (c, true)).chain(redo.iter().map(|c| (c, false))).map(|(c, applied)| {
        let (icon, label) = step_label(p, c);
        Step { icon, label, applied }
    });
    (rows.collect(), cur)
}

/// Leva o documento ao estado `target` (passos aplicados). Para no primeiro erro e avisa.
pub(crate) fn jump(env: &mut Env, target: usize) {
    let (cur, _) = {
        let (u, r) = env.core.history_commands();
        (u.len(), r.len())
    };
    let Some((undo, n)) = plan_jump(cur, target) else { return };
    let prev = std::mem::replace(&mut env.core.author, AUTHOR.into());
    for _ in 0..n {
        let r = if undo { env.core.undo() } else { env.core.redo() };
        if let Err(e) = r {
            env.ed.notify(e.to_string());
            break;
        }
    }
    env.core.author = prev;
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 4.0);
    let ui = &mut child;
    let (rows, cur) = steps(env.core);
    // a lista acompanha o passo atual quando o histórico muda (desenhar, desfazer)
    let sig = (rows.len(), cur);
    let id = ui.id().with("hist_sig");
    let changed = ui.data(|d| d.get_temp::<(usize, usize)>(id)) != Some(sig);
    ui.data_mut(|d| d.insert_temp(id, sig));
    let mut target = None;
    egui::ScrollArea::vertical().id_salt("history_list").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        let h = 22.0;
        for i in 0..=rows.len() {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
            env.note(format!("history:row:{i}"), rect);
            let is_cur = i == cur;
            let applied = i <= cur;
            let p = ui.painter();
            if is_cur {
                p.rect_filled(rect, 4.0, t.accent_dim);
                p.rect_filled(Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 3.0), egui::vec2(3.0, h - 6.0)), 1.5, t.accent);
            } else if resp.hovered() {
                p.rect_filled(rect, 4.0, t.bg_hover);
            }
            let (icon, label) = if i == 0 { (IconId::NodeDocument, tr("hist.start").to_owned()) } else { (rows[i - 1].icon, rows[i - 1].label.clone()) };
            let ink = if applied { t.text } else { t.text_dim.gamma_multiply(0.7) };
            let ir = Rect::from_center_size(egui::pos2(rect.left() + 14.0, rect.center().y), egui::vec2(14.0, 14.0));
            paint_icon(env.icons, p, icon, ir, if is_cur { t.accent } else if applied { t.text_dim } else { t.text_dim.gamma_multiply(0.6) }, false);
            let text_r = Rect::from_min_max(egui::pos2(rect.left() + 28.0, rect.top()), egui::pos2(rect.right() - 4.0, rect.bottom()));
            p.with_clip_rect(text_r).text(egui::pos2(text_r.left(), rect.center().y), Align2::LEFT_CENTER, &label, FontId::proportional(12.5), ink);
            if changed && is_cur {
                ui.scroll_to_rect(rect, Some(egui::Align::Center));
            }
            if resp.on_hover_text(&label).clicked() {
                target = Some(i);
            }
        }
    });
    if let Some(i) = target {
        jump(env, i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_core::{CoreSession, Id, LayerData, LayerType};
    use serde_json::json;

    #[test]
    fn jump_plans_undo_or_redo_by_the_distance() {
        assert_eq!(plan_jump(5, 5), None);
        assert_eq!(plan_jump(5, 2), Some((true, 3)));
        assert_eq!(plan_jump(2, 6), Some((false, 4)));
        assert_eq!(plan_jump(3, 0), Some((true, 3)));
    }

    fn session_with_three_steps() -> (CoreSession, Id) {
        let mut core = CoreSession::for_tests("hist");
        // um corte (passo 1 do histórico) e a camada criada direto por comando (passo 2)
        let (cut, _) = crate::app::commands::quick_start_with_layers(&mut core, 12, None).unwrap();
        let o = core.apply(Command::CreateNode { id: None, parent: Some(cut), index: None, name: "Linha".into(), data: NodeKind::Layer(LayerData::new(LayerType::Raster)) }).unwrap();
        let layer = o.created[0].clone();
        core.apply(Command::SetProps { id: layer.clone(), name: None, data: json!({ "opacity": 0.5 }) }).unwrap();
        core.apply(Command::SetProps { id: layer.clone(), name: None, data: json!({ "visible": false }) }).unwrap();
        (core, layer)
    }

    #[test]
    fn steps_have_readable_names_with_the_layer() {
        let (mut core, _) = session_with_three_steps();
        let (rows, cur) = steps(&core);
        assert_eq!((rows.len(), cur), (4, 4));
        // o lote com rótulo mostra o rótulo
        assert_eq!(rows[0].label, tr("cmd.quick_start"));
        assert!(rows[1].label.contains("Linha") && rows[1].label.starts_with(tr("hist.new_layer")), "{:?}", rows[1]);
        assert_eq!(rows[2].label, format!("{} · Linha", tr("hist.opacity")));
        assert_eq!(rows[3].icon, IconId::Eye);
        assert!(rows.iter().all(|r| r.applied));
        // nada de id cru no texto (a descrição do core mostra o id)
        assert!(rows.iter().all(|r| !r.label.contains("id-")), "{rows:?}");
        // desfeito: continua na lista, atenuado, com o mesmo nome (criar volta como "restaurar" no redo)
        core.undo().unwrap();
        core.undo().unwrap();
        core.undo().unwrap();
        let (after, cur) = steps(&core);
        assert_eq!(cur, 1);
        assert_eq!(after.len(), 4);
        assert!(after[0].applied && after[1..].iter().all(|r| !r.applied));
        assert!(after[1].label.starts_with(tr("hist.new_layer")), "{:?}", after[1]);
    }

    #[test]
    fn jumping_moves_the_document_to_the_clicked_step() {
        let (mut core, layer) = session_with_three_steps();
        let mut ed = crate::app::editor::Editor::default();
        let mut icons = crate::icons::Icons::default();
        let mut notes = Vec::new();
        let mut tool = crate::workspace::ToolId::Brush;
        let t = crate::theme::Tokens::dark();
        let mut env = Env { t, core: &mut core, ed: &mut ed, icons: &mut icons, notes: &mut notes, tool: &mut tool, ws: crate::workspace::WorkspaceId::Pintura };
        let opacity = |env: &Env| crate::app::commands::layer_data(&env.core.project, &layer).map(|l| (l.opacity, l.visible));
        jump(&mut env, 2);
        assert_eq!(steps(env.core).1, 2);
        assert_eq!(opacity(&env), Some((1.0, true)));
        jump(&mut env, 4);
        assert_eq!(steps(env.core).1, 4);
        assert_eq!(opacity(&env), Some((0.5, false)));
        jump(&mut env, 1);
        assert!(crate::app::commands::layer_data(&env.core.project, &layer).is_none(), "antes de criar: a camada não existe");
        // pular para o mesmo passo não faz nada; o estado inicial desfaz tudo
        jump(&mut env, 1);
        assert_eq!(steps(env.core).1, 1);
        jump(&mut env, 0);
        assert_eq!(steps(env.core).1, 0);
        assert!(!env.core.can_undo() && env.core.can_redo());
    }
}
