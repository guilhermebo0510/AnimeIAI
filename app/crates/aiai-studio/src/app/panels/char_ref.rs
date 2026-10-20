//! Painel Referências do personagem: escolhe o personagem (os do corte primeiro: falas do roteiro ligadas ao corte, a
//! cena e os clipes de áudio; ou todos), mostra os materiais da pasta dele (folha de modelo, expressões, tabela de
//! boca, olhos, figurinos e folhas extras) com troca rápida por tipo e, dentro do tipo, pelo nome, com zoom pela roda,
//! arrasto para mover e duplo clique para enquadrar, e a paleta de settei (a variante do figurino, quando o material é
//! um): clicar numa cor escolhe a cor do pincel. Só leitura: editar o material é no workspace Personagens.
//!
//! A folha é renderizada em segundo plano e só de novo quando ELA muda (revisão dos nós da folha e da paleta), não a
//! cada traço feito no canvas.

use super::char_folder::kind_title;
use super::characters::char_data;
use super::edit::EditPreview;
use crate::app::commands::{self as cmds, NodeId};
use crate::app::ui_kit::{Env, body_ui, rgba_to_color32};
use crate::i18n::tr;
use crate::workspace::WorkspaceId;
use aiai_core::{CharMaterial, ColorRef, MaterialKind, Project, Rgba};
use aiai_tools::char_material as mat;
use egui::{Color32, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};

pub(crate) struct CharRefState {
    pub sel: Option<NodeId>,
    /// Corte para o qual a escolha foi feita (trocar de corte volta ao primeiro personagem dele).
    cut: Option<NodeId>,
    /// Tipo de material escolhido (`None` = o primeiro que o personagem tem, a folha de modelo se houver).
    pub kind: Option<MaterialKind>,
    /// Material escolhido (corte do personagem).
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
        CharRefState { sel: None, cut: None, kind: None, sheet: None, zoom: 1.0, pan: Vec2::ZERO, all: false, preview: EditPreview::default() }
    }
}

/// Cortes dos materiais do personagem, na ordem da pasta.
#[cfg(test)]
pub(crate) fn sheets(p: &Project, ch: &NodeId) -> Vec<NodeId> {
    mat::materials(p, ch).iter().map(|m| m.content.cut().clone()).collect()
}

/// Tipos que o personagem tem, na ordem de `MaterialKind::ALL`.
fn kinds_of(list: &[CharMaterial]) -> Vec<MaterialKind> {
    MaterialKind::ALL.into_iter().filter(|k| list.iter().any(|m| m.kind == *k)).collect()
}

/// Material mostrado: o do corte escolhido, senão o primeiro do tipo escolhido, senão o primeiro tipo que existe.
fn pick<'a>(list: &'a [CharMaterial], kind: Option<MaterialKind>, cut: Option<&NodeId>) -> Option<&'a CharMaterial> {
    if let Some(m) = cut.and_then(|c| list.iter().find(|m| m.content.cut() == c)) {
        return Some(m);
    }
    let kind = kind.filter(|k| list.iter().any(|m| m.kind == *k)).or_else(|| kinds_of(list).first().copied())?;
    list.iter().find(|m| m.kind == kind)
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
    let mats = mat::materials(&env.core.project, &ch);
    let st = &mut env.ed.anim.char_ref;
    let shown = pick(&mats, st.kind, st.sheet.as_ref()).cloned();
    if let Some(m) = &shown {
        st.kind = Some(m.kind);
        st.sheet = Some(m.content.cut().clone());
    }
    let p = &env.core.project;
    // a paleta do figurino, quando o material é um e tem variante; senão a de settei do personagem
    let palette = shown.as_ref().and_then(|m| m.palette.clone()).filter(|pal| cmds::palette_data(p, pal).is_some()).or_else(|| char_data(p, &ch).and_then(|c| c.palette.clone()));

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

    // ---- materiais: tipo (troca rápida) e, dentro do tipo, o nome
    let Some(shown) = shown else {
        ui.add_space(6.0);
        ui.label(RichText::new(tr("char_ref.no_sheet")).color(t.text_dim));
        let b = ui.button(tr("char_ref.open_chars"));
        env.note("charref:open_chars", b.rect);
        if b.clicked() {
            env.ed.chars.sel = Some(ch.clone());
            env.ed.want_workspace = Some(WorkspaceId::Personagens);
        }
        return;
    };
    let sheet = shown.content.cut().clone();
    let kinds = kinds_of(&mats);
    let same: Vec<&CharMaterial> = mats.iter().filter(|m| m.kind == shown.kind).collect();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        // tipo do material (troca rápida: um menu com todos os tipos que o personagem tem)
        let count = |k: MaterialKind| mats.iter().filter(|m| m.kind == k).count();
        let mut pick_kind: Option<MaterialKind> = None;
        let cb = egui::ComboBox::from_id_salt(("charref_kind", &ch.0)).width(150.0).selected_text(format!("{} ({})", kind_title(shown.kind), count(shown.kind))).show_ui(ui, |ui| {
            for k in &kinds {
                let r = ui.selectable_label(shown.kind == *k, format!("{} ({})", kind_title(*k), count(*k)));
                env.note(format!("charref:kind:{}", k.key()), r.rect);
                if r.clicked() {
                    pick_kind = Some(*k);
                }
            }
        });
        env.note("charref:kind", cb.response.rect);
        cb.response.on_hover_text(tr("char_ref.kind_tip"));
        if let Some(k) = pick_kind.filter(|k| *k != shown.kind) {
            if let Some(first) = mats.iter().find(|m| m.kind == k) {
                let st = &mut env.ed.anim.char_ref;
                st.kind = Some(k);
                st.sheet = Some(first.content.cut().clone());
                st.zoom = 1.0;
                st.pan = Vec2::ZERO;
            }
        }
        if same.len() > 1 {
            for (i, m) in same.iter().enumerate() {
                let r = ui.selectable_label(m.content.cut() == &sheet, &m.name);
                env.note(format!("charref:sheet:{i}"), r.rect);
                if r.clicked() {
                    let st = &mut env.ed.anim.char_ref;
                    st.sheet = Some(m.content.cut().clone());
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
        move || aiai_render::render_frame(&project, &blobs, &c2, 0, &aiai_render::RenderOptions { background: Some([1.0, 1.0, 1.0]), include_guides: false, include_reference: true, camera: false, ..Default::default() }).map(|o| o.image).map_err(|e| e.to_string())
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
    use serde_json::json;

    fn create(core: &mut CoreSession, ch: &NodeId, kind: &str) -> serde_json::Value {
        crate::app::actions::run(core, "CHARACTER_MATERIAL", json!({ "character": ch, "action": "create", "kind": kind })).unwrap()
    }

    #[test]
    fn materials_list_the_model_sheet_first_and_group_by_kind() {
        let mut core = CoreSession::for_tests("t");
        let ch = super::super::characters::add_character_named(&mut core, "Rin", 6.5).unwrap();
        assert!(sheets(&core.project, &ch).is_empty());
        create(&mut core, &ch, "expressions");
        let r = crate::app::actions::run(&mut core, "MODEL_SHEET", json!({ "character": ch, "name": "Folha", "heads_tall": 6.5, "figure_px": 200.0, "views": [0.0, 90.0] }));
        assert!(r.is_ok(), "{r:?}");
        create(&mut core, &ch, "outfit");
        create(&mut core, &ch, "outfit");
        let list = mat::materials(&core.project, &ch);
        assert_eq!(list.len(), 4);
        // sem escolha, abre o primeiro tipo na ordem fixa (folha de modelo antes das expressões, mesmo criada depois)
        assert_eq!(pick(&list, None, None).unwrap().kind, MaterialKind::ModelSheet);
        assert_eq!(kinds_of(&list), vec![MaterialKind::ModelSheet, MaterialKind::Expressions, MaterialKind::Outfit]);
        // tipo escolhido sem corte = o primeiro do tipo; corte escolhido ganha do tipo; tipo que sumiu cai no primeiro
        assert_eq!(pick(&list, Some(MaterialKind::Outfit), None).unwrap().name, "Figurino 1");
        let second = list.iter().filter(|m| m.kind == MaterialKind::Outfit).nth(1).unwrap();
        assert_eq!(pick(&list, Some(MaterialKind::Expressions), Some(second.content.cut())).unwrap().id, second.id);
        assert_eq!(pick(&list, Some(MaterialKind::Eyes), None).unwrap().kind, MaterialKind::ModelSheet);
        // a variante de paleta do figurino é a que o painel mostra
        assert!(second.palette.is_some());
        // a revisão da folha não muda com edições fora dela
        let cut = list[0].content.cut().clone();
        let rev = sheet_revision(&core.project, &cut, None);
        let other = cmds::quick_start(&mut core, 12, None).unwrap();
        cmds::add_layer(&mut core, &other, aiai_core::LayerType::Vector, None).unwrap();
        assert_eq!(sheet_revision(&core.project, &cut, None), rev);
    }
}
