//! Painel Referências do personagem: escolhe o personagem (os do corte primeiro: falas do roteiro ligadas ao corte, a
//! cena e os clipes de áudio; ou todos), mostra as folhas que ele tem (a folha de modelo e os outros cortes guardados
//! no personagem) com zoom pela roda, arrasto para mover e duplo clique para enquadrar, e a paleta de settei: clicar
//! numa cor escolhe a cor do pincel. Só leitura: editar a folha é no workspace Personagens.
//!
//! A folha é renderizada em segundo plano e só de novo quando ELA muda (revisão dos nós da folha e da paleta), não a
//! cada traço feito no canvas.

use super::characters::{char_data, sheet_cut};
use super::edit::EditPreview;
use crate::app::commands::{self as cmds, NodeId};
use crate::app::ui_kit::{Env, body_ui, rgba_to_color32};
use crate::i18n::tr;
use crate::workspace::WorkspaceId;
use aiai_core::{ColorRef, Project, Rgba};
use egui::{Color32, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};

pub(crate) struct CharRefState {
    pub sel: Option<NodeId>,
    /// Corte para o qual a escolha foi feita (trocar de corte volta ao primeiro personagem dele).
    cut: Option<NodeId>,
    /// Folha escolhida (corte do personagem).
    pub sheet: Option<NodeId>,
    /// 1 = folha inteira no painel.
    pub zoom: f32,
    pub pan: Vec2,
    /// Mostrar todos os personagens, não só os do corte.
    pub all: bool,
    preview: EditPreview,
}

impl Default for CharRefState {
    fn default() -> Self {
        CharRefState { sel: None, cut: None, sheet: None, zoom: 1.0, pan: Vec2::ZERO, all: false, preview: EditPreview::default() }
    }
}

/// Folhas do personagem: a folha de modelo primeiro, depois os outros cortes guardados nele.
pub(crate) fn sheets(p: &Project, ch: &NodeId) -> Vec<NodeId> {
    let mut v: Vec<NodeId> = sheet_cut(p, ch).into_iter().collect();
    if let Some(n) = p.node(ch) {
        for c in &n.children {
            if cmds::is_cut(p, c) && !v.contains(c) {
                v.push(c.clone());
            }
        }
    }
    v
}

/// Revisão do que a folha mostra (os nós da folha e a paleta do personagem).
fn sheet_revision(p: &Project, cut: &NodeId, palette: Option<&NodeId>) -> u64 {
    let r = p.subtree(cut).iter().filter_map(|n| p.node(n)).map(|n| n.provenance.revision).max().unwrap_or(0);
    r.max(palette.and_then(|pal| p.node(pal)).map(|n| n.provenance.revision).unwrap_or(0))
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let all_chars = env.core.project.roots.characters.clone();
    if all_chars.is_empty() {
        ui.label(RichText::new(tr("panel.char_ref.empty")).color(t.text_dim));
        return;
    }
    let cut = env.ed.active_cut(env.core);
    let linked = match &cut {
        Some(cut) => env.ed.anim.scene.get(env.core, cut).characters.clone(),
        None => Vec::new(),
    };
    let st = &mut env.ed.anim.char_ref;
    if st.cut != cut {
        st.cut = cut;
        st.sel = None;
    }
    let list: Vec<NodeId> = if st.all || linked.is_empty() { all_chars.clone() } else { linked.clone() };
    if st.sel.as_ref().is_none_or(|s| !all_chars.contains(s) || (!st.all && !linked.is_empty() && !linked.contains(s))) {
        st.sel = list.first().cloned();
        st.sheet = None;
    }
    // ---- personagens (os do corte marcados com ●)
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for (i, id) in list.iter().enumerate() {
            let name = env.core.project.node(id).map(|n| n.name.clone()).unwrap_or_default();
            let label = if linked.contains(id) { format!("● {name}") } else { name };
            let on = env.ed.anim.char_ref.sel.as_ref() == Some(id);
            let r = ui.selectable_label(on, label);
            env.note(format!("charref:char:{i}"), r.rect);
            if r.clicked() && !on {
                let st = &mut env.ed.anim.char_ref;
                st.sel = Some(id.clone());
                st.sheet = None;
                st.zoom = 1.0;
                st.pan = Vec2::ZERO;
            }
        }
        if !linked.is_empty() {
            let mut all = env.ed.anim.char_ref.all;
            let r = ui.checkbox(&mut all, tr("char_ref.all")).on_hover_text(tr("char_ref.all_tip"));
            env.note("charref:all", r.rect);
            env.ed.anim.char_ref.all = all;
        }
    });
    if linked.is_empty() {
        ui.label(RichText::new(tr("char_ref.none_linked")).size(11.0).color(t.text_dim));
    }
    let Some(ch) = env.ed.anim.char_ref.sel.clone() else { return };
    let p = &env.core.project;
    let palette = char_data(p, &ch).and_then(|c| c.palette.clone());

    // ---- paleta de settei (clique = cor do pincel)
    if let Some(pal) = palette.clone() {
        let colors = cmds::palette_data(p, &pal).map(|pd| pd.colors.clone()).unwrap_or_default();
        if !colors.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
                for (i, c) in colors.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                    env.note(format!("charref:color:{i}"), rect);
                    let selected = matches!(&env.ed.color, ColorRef::Palette { palette, color } if *palette == pal && *color == c.id);
                    ui.painter().rect_filled(rect, 3.0, rgba_to_color32(Rgba([c.rgba.0[0], c.rgba.0[1], c.rgba.0[2], 1.0])));
                    ui.painter().rect_stroke(rect, 3.0, Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { t.accent } else { t.border }), StrokeKind::Inside);
                    let tip = [c.name.as_str(), c.group.as_str(), c.role.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
                    if resp.on_hover_text(format!("{tip}  {}", &c.rgba.to_hex()[..7].to_uppercase())).clicked() {
                        env.ed.color = ColorRef::Palette { palette: pal.clone(), color: c.id.clone() };
                        env.ed.palette = Some(pal.clone());
                        env.ed.color_edit = None;
                    }
                }
            });
        }
    }

    // ---- folhas
    let list_sheets = sheets(&env.core.project, &ch);
    if list_sheets.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new(tr("char_ref.no_sheet")).color(t.text_dim));
        let b = ui.button(tr("char_ref.open_chars"));
        env.note("charref:open_chars", b.rect);
        if b.clicked() {
            env.ed.chars.sel = Some(ch.clone());
            env.ed.want_workspace = Some(WorkspaceId::Personagens);
        }
        return;
    }
    let st = &mut env.ed.anim.char_ref;
    if st.sheet.as_ref().is_none_or(|s| !list_sheets.contains(s)) {
        st.sheet = list_sheets.first().cloned();
    }
    let sheet = st.sheet.clone().unwrap_or_else(|| list_sheets[0].clone());
    ui.horizontal_wrapped(|ui| {
        if list_sheets.len() > 1 {
            for (i, s) in list_sheets.iter().enumerate() {
                let name = env.core.project.node(s).map(|n| n.name.clone()).unwrap_or_default();
                let r = ui.selectable_label(*s == sheet, name);
                env.note(format!("charref:sheet:{i}"), r.rect);
                if r.clicked() {
                    let st = &mut env.ed.anim.char_ref;
                    st.sheet = Some(s.clone());
                    st.zoom = 1.0;
                    st.pan = Vec2::ZERO;
                }
            }
        }
        let st = &mut env.ed.anim.char_ref;
        for (txt, f, name) in [("−", 1.0 / 1.25, "charref:zoom_out"), ("+", 1.25, "charref:zoom_in")] {
            let b = ui.small_button(txt);
            env.notes.push((name.to_owned(), b.rect));
            if b.clicked() {
                st.zoom = (st.zoom * f).clamp(0.25, 16.0);
            }
        }
        let b = ui.small_button(tr("char_ref.fit"));
        env.notes.push(("charref:fit".to_owned(), b.rect));
        if b.clicked() {
            st.zoom = 1.0;
            st.pan = Vec2::ZERO;
        }
        ui.label(RichText::new(format!("{:.0}%", st.zoom * 100.0)).size(11.0).color(t.text_dim));
    });

    // render da folha (só quando ela muda)
    let rev = sheet_revision(&env.core.project, &sheet, palette.as_ref());
    let ctx = ui.ctx().clone();
    let core = &*env.core;
    env.ed.anim.char_ref.preview.request(&ctx, (rev, sheet.0.clone(), 0), || {
        let (project, blobs, c2) = (core.project.clone(), core.blobs.clone(), sheet.clone());
        move || aiai_render::render_frame(&project, &blobs, &c2, 0, &aiai_render::RenderOptions { background: Some([1.0, 1.0, 1.0]), include_guides: false, camera: false, ..Default::default() }).map(|o| o.image).map_err(|e| e.to_string())
    });

    let area = ui.available_rect_before_wrap();
    if area.height() < 20.0 {
        return;
    }
    let resp = ui.allocate_rect(area, Sense::click_and_drag());
    env.note("charref:view", area);
    let (w, h) = cmds::canvas_size(&env.core.project, &sheet);
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let st = &mut env.ed.anim.char_ref;
    let fit = (area.width() / w).min(area.height() / h);
    // roda = zoom no ponto do cursor; arrasto = mover; duplo clique = enquadrar
    if resp.hovered() {
        let scroll: f32 = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::MouseWheel { unit, delta, .. } => Some(
                        delta.y
                            * match unit {
                                egui::MouseWheelUnit::Line => 40.0,
                                egui::MouseWheelUnit::Page => 400.0,
                                egui::MouseWheelUnit::Point => 1.0,
                            },
                    ),
                    _ => None,
                })
                .sum()
        });
        if scroll != 0.0 {
            let old = st.zoom;
            st.zoom = (st.zoom * 1.0015f32.powf(scroll)).clamp(0.25, 16.0);
            if let Some(m) = ui.input(|i| i.pointer.hover_pos()) {
                let c = area.center() + st.pan;
                st.pan += (m - c) * (1.0 - st.zoom / old);
            }
        }
    }
    if resp.dragged() {
        st.pan += resp.drag_delta();
    }
    if resp.double_clicked() {
        st.zoom = 1.0;
        st.pan = Vec2::ZERO;
    }
    let s = fit * st.zoom;
    let img = Rect::from_center_size(area.center() + st.pan, egui::vec2(w * s, h * s));
    let painter = ui.painter().with_clip_rect(area);
    painter.rect_filled(area, 3.0, t.canvas_bg);
    painter.rect_filled(img, 0.0, Color32::WHITE);
    if let Some(tex) = &st.preview.texture {
        painter.image(tex.id(), img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
    painter.rect_stroke(img, 0.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
    if let Some(e) = &st.preview.error {
        painter.text(area.center(), egui::Align2::CENTER_CENTER, e, egui::FontId::proportional(12.0), t.text_dim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_core::CoreSession;

    #[test]
    fn sheets_list_the_model_sheet_first() {
        let mut core = CoreSession::for_tests("t");
        let ch = super::super::characters::add_character_named(&mut core, "Rin", 6.5).unwrap();
        assert!(sheets(&core.project, &ch).is_empty());
        let r = crate::app::actions::run(&mut core, "MODEL_SHEET", serde_json::json!({ "character": ch, "name": "Folha", "heads_tall": 6.5, "figure_px": 200.0, "views": [0.0, 90.0] }));
        assert!(r.is_ok(), "{r:?}");
        let v = sheets(&core.project, &ch);
        assert_eq!(v.len(), 1);
        assert_eq!(sheet_cut(&core.project, &ch), Some(v[0].clone()));
        // a revisão da folha não muda com edições fora dela
        let rev = sheet_revision(&core.project, &v[0], None);
        let cut = cmds::quick_start(&mut core, 12, None).unwrap();
        cmds::add_layer(&mut core, &cut, aiai_core::LayerType::Vector, None).unwrap();
        assert_eq!(sheet_revision(&core.project, &v[0], None), rev);
    }
}
