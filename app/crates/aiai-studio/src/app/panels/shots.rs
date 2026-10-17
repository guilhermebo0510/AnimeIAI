//! Painel Cenas (Storyboard): o storyboard aberto como lista de painéis (nº do corte, enquadramento, segundos e trecho da
//! ação), com criar, apagar, mover e renumerar, e as propriedades do painel selecionado (usadas pelo painel Propriedades
//! do workspace Storyboard). Tudo por ações `STORYBOARD` da bridge (`actions`): um clique ou campo = UM passo de desfazer.

use super::{commit_on_release, text_commit_multi, text_commit_opt};
use crate::app::actions::{self, DocKind};
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_core::Id;
use aiai_tools::preprod::{Angle, Panel, PanelPatch, Shot};
use egui::{FontId, Id as EId, Rect, Sense, Ui};
use serde_json::json;

pub(crate) const SHOTS: [(Shot, &str); 9] = [
    (Shot::Establishing, "shots.shot.establishing"),
    (Shot::Wide, "shots.shot.wide"),
    (Shot::Full, "shots.shot.full"),
    (Shot::Medium, "shots.shot.medium"),
    (Shot::MediumClose, "shots.shot.medium_close"),
    (Shot::Close, "shots.shot.close"),
    (Shot::ExtremeClose, "shots.shot.extreme_close"),
    (Shot::Detail, "shots.shot.detail"),
    (Shot::OverShoulder, "shots.shot.over_shoulder"),
];

pub(crate) const ANGLES: [(Angle, &str); 6] = [
    (Angle::Eye, "shots.angle.eye"),
    (Angle::High, "shots.angle.high"),
    (Angle::Low, "shots.angle.low"),
    (Angle::Dutch, "shots.angle.dutch"),
    (Angle::Bird, "shots.angle.bird"),
    (Angle::Worm, "shots.angle.worm"),
];

pub(crate) fn shot_key(s: Shot) -> &'static str {
    SHOTS.iter().find(|(x, _)| *x == s).map(|(_, k)| *k).unwrap_or("shots.shot.medium")
}

pub(crate) fn angle_key(a: Angle) -> &'static str {
    ANGLES.iter().find(|(x, _)| *x == a).map(|(_, k)| *k).unwrap_or("shots.angle.eye")
}

/// Cria um storyboard novo (vazio) e o abre.
pub(crate) fn new_storyboard(env: &mut Env) {
    let name = actions::next_doc_name(&env.core.project, DocKind::Storyboard, tr("shots.default_name"));
    let r = actions::storyboard_op(env.core, "create", json!({ "name": name, "title": name }));
    if let Some(v) = actions::notify_err(env.ed, r) {
        if let Some(id) = v.get("storyboard").and_then(|d| d.as_str()) {
            env.ed.sel_storyboard = Some(Id(id.to_owned()));
            env.ed.sel_panel = None;
        }
    }
}

/// `STORYBOARD panel_update` com um trecho do `PanelPatch` (um passo de desfazer).
fn update_panel(env: &mut Env, sb: &Id, panel: &Id, patch: PanelPatch) {
    let r = actions::storyboard_op(env.core, "panel_update", json!({ "storyboard": sb, "panel": panel, "patch": patch }));
    actions::notify_err(env.ed, r);
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let boards = actions::docs_of(&env.core.project, DocKind::Storyboard);
    let sel = actions::sync_story_sel(env.ed, env.core);
    let sel_id = sel.as_ref().map(|(id, _)| id.clone());

    // ---- storyboard aberto
    let name = sel_id.as_ref().and_then(|id| boards.iter().find(|(i, _)| i == id)).map(|(_, n)| n.clone()).unwrap_or_else(|| tr("shots.no_storyboard").to_owned());
    let cb = egui::ComboBox::from_id_salt("shots_doc").width((ui.available_width() - 28.0).max(60.0)).selected_text(name).show_ui(ui, |ui| {
        for (id, n) in &boards {
            let r = ui.selectable_label(sel_id.as_ref() == Some(id), n);
            env.notes.push((format!("shots:doc:{n}"), r.rect));
            if r.clicked() {
                env.ed.sel_storyboard = Some(id.clone());
                env.ed.sel_panel = None;
            }
        }
    });
    env.note("shots:doc_picker", cb.response.rect);

    // ---- ações
    ui.horizontal_wrapped(|ui| {
        let b = ui.button(tr("shots.new_storyboard"));
        env.note("shots:new_storyboard", b.rect);
        if b.clicked() {
            new_storyboard(env);
        }
        let has = sel.is_some();
        let pos = sel.as_ref().and_then(|(_, d)| env.ed.sel_panel.as_ref().and_then(|s| d.panels.iter().position(|x| &x.id == s)));
        let count = sel.as_ref().map(|(_, d)| d.panels.len()).unwrap_or(0);
        let add = env.icon_button(ui, IconId::Plus, "shots.add_panel", false, has, "shots:add_panel");
        let del = env.icon_button(ui, IconId::Trash, "shots.delete_panel", false, pos.is_some(), "shots:delete_panel");
        let up = env.icon_button(ui, IconId::ArrowUp, "shots.panel_up", false, pos.is_some_and(|p| p > 0), "shots:panel_up");
        let down = env.icon_button(ui, IconId::ArrowDown, "shots.panel_down", false, pos.is_some_and(|p| p + 1 < count), "shots:panel_down");
        let ren = env.icon_button(ui, IconId::Copy, "shots.renumber", false, count > 0, "shots:renumber");
        let Some((sb, doc)) = &sel else { return };
        if add.clicked() {
            let index = pos.map(|p| p + 1);
            let r = actions::storyboard_op(env.core, "panel_add", json!({ "storyboard": sb, "index": index, "spec": {} }));
            if let Some(v) = actions::notify_err(env.ed, r) {
                env.ed.sel_panel = v.get("panel").and_then(|x| x.as_str()).map(|s| Id(s.to_owned()));
            }
        }
        if let (true, Some(p)) = (del.clicked(), pos) {
            let next = doc.panels.get(p + 1).or_else(|| p.checked_sub(1).and_then(|q| doc.panels.get(q))).map(|x| x.id.clone());
            let r = actions::storyboard_op(env.core, "panel_delete", json!({ "storyboard": sb, "panel": doc.panels[p].id }));
            if actions::notify_err(env.ed, r).is_some() {
                env.ed.sel_panel = next;
            }
        }
        if let (true, Some(p)) = (up.clicked(), pos) {
            let r = actions::storyboard_op(env.core, "panel_move", json!({ "storyboard": sb, "panel": doc.panels[p].id, "index": p - 1 }));
            actions::notify_err(env.ed, r);
        }
        if let (true, Some(p)) = (down.clicked(), pos) {
            let r = actions::storyboard_op(env.core, "panel_move", json!({ "storyboard": sb, "panel": doc.panels[p].id, "index": p + 1 }));
            actions::notify_err(env.ed, r);
        }
        if ren.clicked() {
            let r = actions::storyboard_op(env.core, "renumber", json!({ "storyboard": sb, "step": 1 }));
            actions::notify_err(env.ed, r);
        }
    });
    ui.add_space(2.0);
    ui.separator();

    egui::ScrollArea::vertical().id_salt("shots_scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let Some((_, doc)) = &sel else {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(tr("panel.shots.empty")).color(t.text_dim));
            return;
        };
        dim_label(ui, &t, &format!("{} {} · {:.1} s", doc.panels.len(), tr("shots.panels"), doc.total_seconds()));
        ui.add_space(2.0);
        for (i, p) in doc.panels.iter().enumerate() {
            panel_row(env, ui, i, p);
        }
    });
}

fn panel_row(env: &mut Env, ui: &mut Ui, i: usize, p: &Panel) {
    let t = env.t;
    let h = 44.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    env.note(format!("shots:panel:{i}"), rect);
    let selected = env.ed.sel_panel.as_ref() == Some(&p.id);
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 4.0, t.accent_dim);
        painter.rect_filled(Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 4.0), egui::vec2(3.0, h - 8.0)), 1.5, t.accent);
    } else if resp.hovered() {
        painter.rect_filled(rect, 4.0, t.bg_hover);
    }
    let clip = Rect::from_min_max(egui::pos2(rect.left() + 8.0, rect.top()), egui::pos2(rect.right() - 6.0, rect.bottom()));
    let painter = painter.with_clip_rect(clip);
    let head = format!("{}   {}   {:.1} s", p.cut_number, p.shot.code(), p.seconds);
    let g = painter.layout_no_wrap(head, FontId::proportional(13.0), t.text);
    painter.galley(egui::pos2(clip.left(), rect.top() + 5.0), g, t.text);
    let action = if p.action.trim().is_empty() { tr("shots.no_action").to_owned() } else { actions::excerpt(&p.action, 120) };
    let mut job = egui::text::LayoutJob::simple(action, FontId::proportional(11.5), t.text_dim, clip.width().max(20.0));
    job.wrap.max_rows = 1;
    let g = painter.layout_job(job);
    painter.galley(egui::pos2(clip.left(), rect.top() + 24.0), g, t.text_dim);
    if resp.clicked() {
        env.ed.sel_panel = Some(p.id.clone());
    }
    if resp.double_clicked() {
        super::board::open_drawing(env, p);
    }
}

/// Propriedades do painel selecionado (workspace Storyboard): nº do corte, enquadramento, ângulo, segundos, ação, fala e SE.
pub(crate) fn properties(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let Some((sb, doc)) = actions::sync_story_sel(env.ed, env.core) else {
        dim_label(ui, &t, tr("shots.props_none"));
        return;
    };
    let Some(p) = env.ed.sel_panel.as_ref().and_then(|id| doc.panel(id)).cloned() else {
        dim_label(ui, &t, tr("shots.props_none"));
        return;
    };
    section(ui, &t, "shots.props");
    let pid = p.id.clone();
    let width = (ui.available_width() - 4.0).max(80.0);
    egui::Grid::new("panel_props_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("shots.cut_number"));
        if let Some(v) = commit_on_release(ui, EId::new(("pp_num", &pid.0)), p.cut_number, |ui, v| ui.add(egui::DragValue::new(v).range(0..=9999).speed(0.2))) {
            update_panel(env, &sb, &pid, PanelPatch { cut_number: Some(v), ..Default::default() });
        }
        ui.end_row();
        dim_label(ui, &t, tr("shots.shot"));
        let mut shot = p.shot;
        let r = egui::ComboBox::from_id_salt("pp_shot").width(150.0).selected_text(tr(shot_key(shot))).show_ui(ui, |ui| {
            for (s, key) in SHOTS {
                ui.selectable_value(&mut shot, s, tr(key));
            }
        });
        env.note("props:shot", r.response.rect);
        if shot != p.shot {
            update_panel(env, &sb, &pid, PanelPatch { shot: Some(shot), ..Default::default() });
        }
        ui.end_row();
        dim_label(ui, &t, tr("shots.angle"));
        let mut angle = p.angle;
        let r = egui::ComboBox::from_id_salt("pp_angle").width(150.0).selected_text(tr(angle_key(angle))).show_ui(ui, |ui| {
            for (a, key) in ANGLES {
                ui.selectable_value(&mut angle, a, tr(key));
            }
        });
        env.note("props:angle", r.response.rect);
        if angle != p.angle {
            update_panel(env, &sb, &pid, PanelPatch { angle: Some(angle), ..Default::default() });
        }
        ui.end_row();
        dim_label(ui, &t, tr("shots.seconds"));
        let mut secs_rect = Rect::NOTHING;
        let r = commit_on_release(ui, EId::new(("pp_secs", &pid.0)), p.seconds, |ui, v| {
            let r = ui.add(egui::DragValue::new(v).range(0.1..=600.0).speed(0.05).max_decimals(2).suffix(" s"));
            secs_rect = r.rect;
            r
        });
        env.note("props:seconds", secs_rect);
        if let Some(v) = r {
            update_panel(env, &sb, &pid, PanelPatch { seconds: Some(v), ..Default::default() });
        }
        ui.end_row();
    });
    ui.add_space(4.0);
    dim_label(ui, &t, tr("shots.action"));
    let (r, c) = text_commit_multi(ui, EId::new(("pp_action", &pid.0)), &p.action, width, "");
    env.note("props:action", r.rect);
    if let Some(x) = c {
        update_panel(env, &sb, &pid, PanelPatch { action: Some(x), ..Default::default() });
    }
    dim_label(ui, &t, tr("shots.dialogue"));
    let (r, c) = text_commit_multi(ui, EId::new(("pp_dialogue", &pid.0)), &p.dialogue_text, width, "");
    env.note("props:dialogue", r.rect);
    if let Some(x) = c {
        update_panel(env, &sb, &pid, PanelPatch { dialogue_text: Some(x), ..Default::default() });
    }
    dim_label(ui, &t, tr("shots.sfx"));
    let (r, c) = text_commit_opt(ui, EId::new(("pp_sfx", &pid.0)), &p.sfx, width, "");
    env.note("props:sfx", r.rect);
    if let Some(x) = c {
        update_panel(env, &sb, &pid, PanelPatch { sfx: Some(x), ..Default::default() });
    }
    ui.add_space(6.0);
    let b = ui.add_enabled(p.drawing_cut.is_some(), egui::Button::new(tr("board.open_drawing")));
    env.note("props:open_drawing", b.rect);
    if b.clicked() {
        super::board::open_drawing(env, &p);
    }
}
