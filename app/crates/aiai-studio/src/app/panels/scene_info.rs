//! Painel Cena (só leitura): para o corte ativo, a cena do roteiro com as falas (as do corte em destaque), o painel do
//! storyboard (desenho, enquadramento, ação, fala, som, segundos), as notas e o tempo do corte (quadros, segundos,
//! fase), com o caminho para abrir no Roteiro ou no Storyboard e para passar ao corte anterior/seguinte sem sair da
//! Animação. A ligação vem do que `CUTS_FROM_STORYBOARD` grava no corte (`storyboard_panel`, `dialogue_links`) e da
//! cena do painel; o resultado fica em cache por revisão do documento.

use crate::app::actions::{self, DocKind};
use crate::app::commands::{self as cmds, NodeId};
use crate::app::panels::board::RENDER_BUDGET;
use crate::app::panels::shots::{angle_key, shot_key};
use crate::app::ui_kit::{Env, body_ui, section};
use crate::i18n::tr;
use crate::workspace::WorkspaceId;
use aiai_core::{Id, NodeKind, Project};
use aiai_tools::preprod::{self, ElementKind, Panel, ScriptScene};
use egui::{Color32, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use std::collections::HashSet;
use std::sync::Arc;

/// O que o corte sabe da pré-produção.
#[derive(Default)]
pub(crate) struct SceneInfo {
    /// (documento de storyboard, painel, canvas dos painéis).
    pub story: Option<(NodeId, Panel, [u32; 2])>,
    /// (documento de roteiro, cena).
    pub script: Option<(NodeId, ScriptScene)>,
    /// Elementos do roteiro ligados ao corte (falas do corte e do painel).
    pub linked: HashSet<Id>,
    /// Personagens do corte: os das falas do corte e do áudio primeiro, depois os da cena.
    pub characters: Vec<NodeId>,
}

/// Cache por (revisão do documento, corte).
#[derive(Default)]
pub(crate) struct SceneCache {
    key: Option<(u64, NodeId)>,
    info: Arc<SceneInfo>,
    /// Corte cuja fala já foi rolada para a vista (ao trocar de corte o painel rola até a fala dele).
    scrolled: Option<NodeId>,
}

impl SceneCache {
    pub(crate) fn get(&mut self, core: &aiai_core::CoreSession, cut: &NodeId) -> Arc<SceneInfo> {
        let key = (core.change_counter, cut.clone());
        if self.key.as_ref() != Some(&key) {
            self.info = Arc::new(gather(&core.project, cut));
            self.key = Some(key);
        }
        Arc::clone(&self.info)
    }
}

/// Junta o painel do storyboard, a cena do roteiro e os personagens do corte.
pub(crate) fn gather(p: &Project, cut: &NodeId) -> SceneInfo {
    let mut info = SceneInfo::default();
    let Some(cd) = cmds::cut_data(p, cut) else { return info };
    if let Some(pid) = &cd.storyboard_panel {
        for (doc, _) in actions::docs_of(p, DocKind::Storyboard) {
            if let Ok(sb) = preprod::storyboard_read(p, &doc) {
                if let Some(pn) = sb.panel(pid) {
                    info.story = Some((doc, pn.clone(), sb.canvas));
                    break;
                }
            }
        }
    }
    info.linked.extend(cd.dialogue_links.iter().cloned());
    if let Some((_, pn, _)) = &info.story {
        info.linked.extend(pn.dialogue_refs.iter().cloned());
    }
    let panel_scene = info.story.as_ref().and_then(|(_, pn, _)| pn.scene.clone());
    for (doc, _) in actions::docs_of(p, DocKind::Script) {
        let Ok(sd) = preprod::script_read(p, &doc) else { continue };
        let scene = panel_scene.as_ref().and_then(|s| sd.scene(s)).or_else(|| sd.scenes.iter().find(|s| s.elements.iter().any(|e| info.linked.contains(&e.id))));
        if let Some(sc) = scene {
            info.script = Some((doc, sc.clone()));
            break;
        }
    }
    let is_char = |id: &NodeId| matches!(p.node(id).map(|n| &n.data), Some(NodeKind::Character(_)));
    let mut chars: Vec<NodeId> = Vec::new();
    let mut push = |id: &NodeId| {
        if is_char(id) && !chars.contains(id) {
            chars.push(id.clone());
        }
    };
    for a in &cd.audio {
        if let Some(c) = &a.character {
            push(c);
        }
    }
    if let Some((_, sc)) = &info.script {
        for e in sc.elements.iter().filter(|e| info.linked.contains(&e.id)) {
            if let Some(c) = &e.character {
                push(c);
            }
        }
        for e in &sc.elements {
            if let Some(c) = &e.character {
                push(c);
            }
        }
    }
    info.characters = chars;
    info
}

/// Cortes do documento em ordem (episódio, cena, corte).
fn all_cuts(p: &Project) -> Vec<NodeId> {
    cmds::episode_tree(p).into_iter().flat_map(|e| e.scenes).flat_map(|(_, cuts)| cuts).collect()
}

/// Quadros em "s+qq" (segundos e quadros, como na folha de exposição).
fn sec_frames(frames: u32, fps: u32) -> String {
    let fps = fps.max(1);
    format!("{}+{:02}", frames / fps, frames % fps)
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    let Some(cut) = env.ed.active_cut(env.core) else {
        ui.label(RichText::new(tr("timeline.no_cut")).color(t.text_dim));
        return;
    };
    let p = &env.core.project;
    let Some(cd) = cmds::cut_data(p, &cut).cloned() else { return };
    let cut_name = p.node(&cut).map(|n| n.name.clone()).unwrap_or_default();
    let scene_node = p.node(&cut).and_then(|n| n.parent.clone()).and_then(|s| p.node(&s).cloned());
    let info = env.ed.anim.scene.get(env.core, &cut);

    // ---- corte: anterior/seguinte e tempo
    let cuts = all_cuts(&env.core.project);
    let idx = cuts.iter().position(|c| *c == cut);
    ui.horizontal(|ui| {
        let prev = idx.and_then(|i| i.checked_sub(1)).and_then(|i| cuts.get(i)).cloned();
        let next = idx.and_then(|i| cuts.get(i + 1)).cloned();
        let b = ui.add_enabled(prev.is_some(), egui::Button::new("◀").small()).on_hover_text(tr("scene.prev_cut"));
        env.note("scene:prev_cut", b.rect);
        if b.clicked() {
            if let Some(c) = prev {
                env.ed.set_cut(c);
            }
        }
        let b = ui.add_enabled(next.is_some(), egui::Button::new("▶").small()).on_hover_text(tr("scene.next_cut"));
        env.note("scene:next_cut", b.rect);
        if b.clicked() {
            if let Some(c) = next {
                env.ed.set_cut(c);
            }
        }
        let scene_name = scene_node.as_ref().map(|n| n.name.clone()).unwrap_or_default();
        ui.label(RichText::new(format!("{scene_name} · {cut_name}")).strong().color(t.text));
    });
    let fps = env.core.project.settings.fps();
    let fpsi = fps.round().max(1.0) as u32;
    let phase = crate::app::anim::cut_phase(&env.core.project, &cut);
    ui.label(
        RichText::new(format!(
            "{} {} ({})  ·  {:.2} s  ·  {:.3} fps  ·  {}: {}",
            cd.duration,
            tr("unit.frames_short"),
            sec_frames(cd.duration, fpsi),
            cd.duration as f64 / fps.max(1.0),
            fps,
            tr("anim.phase"),
            tr(crate::app::anim::phase_key(phase))
        ))
        .size(11.5)
        .color(t.text_dim),
    );
    ui.label(RichText::new(format!("{} {} / {}  ({})", tr("timeline.frame"), env.ed.frame + 1, cd.duration, sec_frames(env.ed.frame, fpsi))).size(11.5).color(t.text_dim));

    let nothing = info.story.is_none() && info.script.is_none() && cd.notes.trim().is_empty();
    egui::ScrollArea::vertical().id_salt("scene_info").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        if nothing {
            ui.add_space(6.0);
            ui.label(RichText::new(tr("panel.scene_info.empty")).color(t.text_dim));
            if let Some(desc) = scene_node.as_ref().and_then(|n| match &n.data {
                NodeKind::Scene(s) => Some(s.description.clone()),
                _ => None,
            }) {
                if !desc.trim().is_empty() {
                    ui.label(RichText::new(desc).color(t.text));
                }
            }
            return;
        }
        // ---- roteiro
        if let Some((doc, sc)) = &info.script {
            section(ui, &t, "scene.script");
            if !sc.heading.is_empty() {
                ui.label(RichText::new(&sc.heading).strong().color(t.text));
            }
            let w = ui.available_width();
            for e in &sc.elements {
                let hot = info.linked.contains(&e.id);
                let speaker = e.character.as_ref().and_then(|c| env.core.project.node(c)).map(|n| n.name.clone()).or_else(|| e.speaker.clone()).unwrap_or_default();
                let frame = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(5, 3)).corner_radius(3.0).fill(if hot { t.accent_dim } else { Color32::TRANSPARENT });
                let shown = frame.show(ui, |ui| {
                    ui.set_width(w - 10.0);
                    match e.kind {
                        ElementKind::Dialogue => {
                            ui.label(RichText::new(speaker.to_uppercase()).size(11.0).strong().color(if hot { t.accent } else { t.text_dim }));
                            ui.label(RichText::new(&e.text).color(t.text));
                            if !e.translation.is_empty() {
                                ui.label(RichText::new(&e.translation).size(11.5).italics().color(t.text_dim));
                            }
                            let tags: Vec<&str> = [e.emotion.as_deref(), e.direction.as_deref()].into_iter().flatten().filter(|s| !s.is_empty()).collect();
                            if !tags.is_empty() {
                                ui.label(RichText::new(format!("({})", tags.join(" · "))).size(11.0).color(t.text_dim));
                            }
                        }
                        ElementKind::Parenthetical => {
                            ui.label(RichText::new(format!("({})", e.text)).italics().color(t.text_dim));
                        }
                        ElementKind::Transition => {
                            ui.label(RichText::new(e.text.to_uppercase()).size(11.5).color(t.text_dim));
                        }
                        ElementKind::Action | ElementKind::Note => {
                            ui.label(RichText::new(&e.text).color(if e.kind == ElementKind::Note { t.text_dim } else { t.text }));
                        }
                    }
                });
                if hot && env.ed.anim.scene.scrolled.as_ref() != Some(&cut) {
                    shown.response.scroll_to_me(Some(egui::Align::Center));
                    env.ed.anim.scene.scrolled = Some(cut.clone());
                }
            }
            let b = ui.button(tr("scene.open_script"));
            env.note("scene:open_script", b.rect);
            if b.clicked() {
                env.ed.sel_script = Some(doc.clone());
                env.ed.sel_scene = Some(sc.id.clone());
                env.ed.sel_element = sc.elements.iter().find(|e| info.linked.contains(&e.id)).map(|e| e.id.clone());
                env.ed.want_workspace = Some(WorkspaceId::Roteiro);
            }
        }
        // ---- storyboard
        if let Some((doc, pn, canvas)) = &info.story {
            section(ui, &t, "scene.storyboard");
            let w = ui.available_width().min(360.0);
            let h = w * canvas[1].max(1) as f32 / canvas[0].max(1) as f32;
            let (img, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
            env.note("scene:panel_image", img);
            ui.painter().rect_filled(img, 2.0, Color32::WHITE);
            let ctx = ui.ctx().clone();
            let mut budget = RENDER_BUDGET;
            if let Some(tex) = env.ed.thumbs.get(&ctx, env.core, pn, *canvas, &mut budget) {
                ui.painter().image(tex, img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            ui.painter().rect_stroke(img, 2.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
            ui.label(RichText::new(format!("C{:03}  ·  {} · {}  ·  {:.1} s", pn.cut_number, tr(shot_key(pn.shot)), tr(angle_key(pn.angle)), pn.seconds)).size(11.5).color(t.text_dim));
            for (k, v) in [("scene.action", &pn.action), ("scene.dialogue", &pn.dialogue_text), ("scene.sfx", &pn.sfx)] {
                if !v.trim().is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(tr(k)).size(11.0).strong().color(t.text_dim));
                        ui.label(RichText::new(v.as_str()).color(t.text));
                    });
                }
            }
            if let Some(cm) = &pn.camera_move {
                ui.label(RichText::new(format!("{}: {:?}", tr("scene.camera"), cm.kind)).size(11.0).color(t.text_dim));
            }
            let b = ui.button(tr("scene.open_storyboard"));
            env.note("scene:open_storyboard", b.rect);
            if b.clicked() {
                env.ed.sel_storyboard = Some(doc.clone());
                env.ed.sel_panel = Some(pn.id.clone());
                env.ed.want_workspace = Some(WorkspaceId::Storyboard);
            }
        }
        // ---- notas do corte
        if !cd.notes.trim().is_empty() {
            section(ui, &t, "scene.notes");
            ui.label(RichText::new(&cd.notes).color(t.text));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_core::CoreSession;
    use serde_json::json;

    #[test]
    fn the_cut_finds_its_script_scene_panel_and_characters() {
        let mut core = CoreSession::for_tests("t");
        let ch = crate::app::panels::characters::add_character_named(&mut core, "Rin", 6.5).unwrap();
        // roteiro com uma cena e duas falas; storyboard com um painel ligado à primeira fala; cortes gerados
        let v = actions::run(&mut core, "SCRIPT", json!({ "op": "create", "name": "Roteiro", "title": "T" })).unwrap();
        let doc = Id(v["doc"].as_str().unwrap().to_owned());
        let ids = actions::script_edit(&mut core, &doc, serde_json::from_value(json!([{ "op": "insert_scene", "heading": "INT. SALA — DIA" }])).unwrap()).unwrap();
        let scene = ids[0].clone();
        let el = actions::script_edit(
            &mut core,
            &doc,
            serde_json::from_value(json!([
                { "op": "insert_element", "scene": scene, "element": { "kind": "dialogue", "character": ch, "text": "おはよう" } },
                { "op": "insert_element", "scene": scene, "element": { "kind": "action", "text": "Ela sorri." } }
            ]))
            .unwrap(),
        )
        .unwrap();
        let sb = actions::storyboard_op(&mut core, "create", json!({ "name": "SB", "title": "SB" })).unwrap();
        let sb_id = Id(sb["storyboard"].as_str().unwrap().to_owned());
        actions::storyboard_op(&mut core, "panel_add", json!({ "storyboard": sb_id, "spec": { "seconds": 2.0, "action": "Rin acena", "dialogue_refs": [el[0]], "scene": scene } })).unwrap();
        let ep = cmds::add_episode(&mut core).unwrap();
        actions::run(&mut core, "CUTS_FROM_STORYBOARD", json!({ "storyboard": sb_id, "episode": ep, "script": doc })).unwrap();
        let cut = cmds::first_cut(&core.project).unwrap();
        let info = gather(&core.project, &cut);
        let (_, pn, _) = info.story.as_ref().expect("o painel do corte");
        assert_eq!(pn.action, "Rin acena");
        let (_, sc) = info.script.as_ref().expect("a cena do roteiro");
        assert_eq!(sc.heading, "INT. SALA — DIA");
        assert!(info.linked.contains(&el[0]) && !info.linked.contains(&el[1]), "só a fala do corte em destaque");
        assert_eq!(info.characters, vec![ch]);
        // corte sem nada ligado: vazio, sem pânico
        let mut c2 = CoreSession::for_tests("u");
        let cut2 = cmds::quick_start(&mut c2, 12, None).unwrap();
        let empty = gather(&c2.project, &cut2);
        assert!(empty.story.is_none() && empty.script.is_none() && empty.characters.is_empty());
        assert_eq!(sec_frames(30, 24), "1+06");
    }
}
