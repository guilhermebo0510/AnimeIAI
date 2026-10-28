//! Atalhos da Modelagem: um contexto próprio do mapa de atalhos (só valem com o ponteiro no viewport 3D), com dois
//! perfis escolhidos em Preferências > Atalhos:
//!
//! * **AnimeIAI** (padrão): as teclas do resto do app — V mover, R girar, T escalar, S seleção — escolhem a ferramenta
//!   da prateleira (arrastar no viewport aplica); E/I/Ctrl+B/Ctrl+R escolhem extrude, inset, bevel e loop cut.
//! * **Blender/ANIGO**: G/R/S/E/I/Ctrl+B/Ctrl+R começam a operação na hora (modal, segue o mouse), como no Blender.
//!
//! Cada ação pode ser trocada; um atalho pertence a uma ação só dentro do contexto (dar a uma ação o atalho de outra
//! tira dele a outra), e nenhum perfil usa os atalhos fixos do app (Ctrl+Z/Y/S/N/O, Alt+1..0 dos workspaces). As sobreposições vão para o mesmo
//! `prefs.shortcuts` do app com o prefixo `modelagem.` (o mapa geral ignora esses ids).

use crate::i18n::tr;
use crate::keymap::Chord;
use crate::workspace::ToolId;
use egui::{Key, RichText};
use std::collections::{BTreeMap, HashMap};

/// Perfil de atalhos.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Preset {
    #[default]
    AnimeIai,
    Blender,
}

impl Preset {
    pub const ALL: [Preset; 2] = [Preset::AnimeIai, Preset::Blender];

    pub fn key(self) -> &'static str {
        match self {
            Preset::AnimeIai => "animeiai",
            Preset::Blender => "blender",
        }
    }

    pub fn from_key(k: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.key() == k)
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Preset::AnimeIai => "mdl.keys.preset.animeiai",
            Preset::Blender => "mdl.keys.preset.blender",
        }
    }
}

/// Ações do contexto Modelagem.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub(crate) enum MAction {
    Select,
    Move,
    Rotate,
    Scale,
    Extrude,
    Inset,
    Bevel,
    LoopCut,
    Slide,
    ToggleEdit,
    VertexMode,
    EdgeMode,
    FaceMode,
    SelectAll,
    SelectNone,
    SelectInvert,
    SelectLinked,
    SelectMore,
    SelectLess,
    Delete,
    Dissolve,
    Merge,
    MergeNear,
    FillHole,
    Bisect,
    Triangulate,
    RecalcNormals,
    Hide,
    Reveal,
    Duplicate,
    AddMenu,
    Frame,
    FrameAll,
    ViewFront,
    ViewRight,
    ViewTop,
    ViewOrtho,
    MaximizeView,
    QuadView,
    Proportional,
    Xray,
    ShadingToggle,
}

/// (ação, id guardado, chave i18n, atalho AnimeIAI, atalho Blender/ANIGO).
pub(crate) const ACTIONS: [(MAction, &str, &str, Option<Chord>, Option<Chord>); 42] = {
    use Key as K;
    const fn p(k: Key) -> Option<Chord> {
        Some(Chord::plain(k))
    }
    const fn c(k: Key) -> Option<Chord> {
        Some(Chord::ctrl(k))
    }
    const fn s(k: Key) -> Option<Chord> {
        Some(Chord::shift(k))
    }
    const fn a(k: Key) -> Option<Chord> {
        Some(Chord::alt(k))
    }
    const fn cs(k: Key) -> Option<Chord> {
        Some(Chord::ctrl_shift(k))
    }
    const fn ca(k: Key) -> Option<Chord> {
        Some(Chord { ctrl: true, shift: false, alt: true, key: k })
    }
    [
        (MAction::Select, "select", "mdl.key.select", p(K::S), p(K::B)),
        (MAction::Move, "move", "mdl.key.move", p(K::V), p(K::G)),
        (MAction::Rotate, "rotate", "mdl.key.rotate", p(K::R), p(K::R)),
        (MAction::Scale, "scale", "mdl.key.scale", p(K::T), p(K::S)),
        (MAction::Extrude, "extrude", "mdl.key.extrude", p(K::E), p(K::E)),
        (MAction::Inset, "inset", "mdl.key.inset", p(K::I), p(K::I)),
        (MAction::Bevel, "bevel", "mdl.key.bevel", c(K::B), c(K::B)),
        (MAction::LoopCut, "loop_cut", "mdl.key.loop_cut", c(K::R), c(K::R)),
        (MAction::Slide, "slide", "mdl.key.slide", s(K::V), s(K::G)),
        (MAction::ToggleEdit, "toggle_edit", "mdl.key.toggle_edit", p(K::Tab), p(K::Tab)),
        (MAction::VertexMode, "vertex_mode", "mdl.key.vertex_mode", p(K::Num1), p(K::Num1)),
        (MAction::EdgeMode, "edge_mode", "mdl.key.edge_mode", p(K::Num2), p(K::Num2)),
        (MAction::FaceMode, "face_mode", "mdl.key.face_mode", p(K::Num3), p(K::Num3)),
        (MAction::SelectAll, "select_all", "mdl.key.select_all", c(K::A), p(K::A)),
        (MAction::SelectNone, "select_none", "mdl.key.select_none", c(K::D), a(K::A)),
        (MAction::SelectInvert, "select_invert", "mdl.key.select_invert", cs(K::I), c(K::I)),
        (MAction::SelectLinked, "select_linked", "mdl.key.select_linked", p(K::L), p(K::L)),
        (MAction::SelectMore, "select_more", "mdl.key.select_more", c(K::Plus), c(K::Plus)),
        (MAction::SelectLess, "select_less", "mdl.key.select_less", c(K::Minus), c(K::Minus)),
        (MAction::Delete, "delete", "mdl.key.delete", p(K::Delete), p(K::X)),
        (MAction::Dissolve, "dissolve", "mdl.key.dissolve", c(K::Delete), c(K::X)),
        (MAction::Merge, "merge", "mdl.key.merge", p(K::M), p(K::M)),
        (MAction::MergeNear, "merge_near", "mdl.key.merge_near", s(K::M), s(K::M)),
        (MAction::FillHole, "fill_hole", "mdl.key.fill_hole", a(K::F), p(K::F)),
        (MAction::Bisect, "bisect", "mdl.key.bisect", p(K::K), p(K::K)),
        (MAction::Triangulate, "triangulate", "mdl.key.triangulate", c(K::T), c(K::T)),
        (MAction::RecalcNormals, "recalc_normals", "mdl.key.recalc_normals", a(K::N), s(K::N)),
        (MAction::Hide, "hide", "mdl.key.hide", c(K::H), p(K::H)),
        (MAction::Reveal, "reveal", "mdl.key.reveal", cs(K::H), a(K::H)),
        (MAction::Duplicate, "duplicate", "mdl.key.duplicate", s(K::D), s(K::D)),
        (MAction::AddMenu, "add_menu", "mdl.key.add_menu", s(K::A), s(K::A)),
        (MAction::Frame, "frame", "mdl.key.frame", p(K::F), p(K::Period)),
        (MAction::FrameAll, "frame_all", "mdl.key.frame_all", p(K::Home), p(K::Home)),
        (MAction::ViewFront, "view_front", "mdl.key.view_front", c(K::Num1), c(K::Num1)),
        (MAction::ViewRight, "view_right", "mdl.key.view_right", c(K::Num3), c(K::Num3)),
        (MAction::ViewTop, "view_top", "mdl.key.view_top", c(K::Num7), c(K::Num7)),
        (MAction::ViewOrtho, "view_ortho", "mdl.key.view_ortho", c(K::Num5), c(K::Num5)),
        (MAction::MaximizeView, "maximize_view", "mdl.key.maximize_view", c(K::Space), c(K::Space)),
        (MAction::QuadView, "quad_view", "mdl.key.quad_view", ca(K::Q), ca(K::Q)),
        (MAction::Proportional, "proportional", "mdl.key.proportional", p(K::O), p(K::O)),
        (MAction::Xray, "xray", "mdl.key.xray", a(K::Z), a(K::Z)),
        (MAction::ShadingToggle, "shading_toggle", "mdl.key.shading_toggle", s(K::Z), s(K::Z)),
    ]
};

/// Atalhos fixos do app que nenhum perfil da Modelagem pode usar (desfazer/refazer e arquivo valem em todo lugar).
/// Alt+1..9/0 trocam de workspace em todo o app.
pub(crate) const APP_RESERVED: [Chord; 17] = [
    Chord::ctrl(Key::Z),
    Chord::ctrl_shift(Key::Z),
    Chord::ctrl(Key::Y),
    Chord::ctrl(Key::S),
    Chord::ctrl_shift(Key::S),
    Chord::ctrl(Key::N),
    Chord::ctrl(Key::O),
    Chord::alt(Key::Num1),
    Chord::alt(Key::Num2),
    Chord::alt(Key::Num3),
    Chord::alt(Key::Num4),
    Chord::alt(Key::Num5),
    Chord::alt(Key::Num6),
    Chord::alt(Key::Num7),
    Chord::alt(Key::Num8),
    Chord::alt(Key::Num9),
    Chord::alt(Key::Num0),
];

impl MAction {
    #[allow(dead_code)]
    pub fn id(self) -> &'static str {
        ACTIONS.iter().find(|a| a.0 == self).map(|a| a.1).unwrap_or("")
    }

    pub fn label_key(self) -> &'static str {
        ACTIONS.iter().find(|a| a.0 == self).map(|a| a.2).unwrap_or("")
    }

    pub fn default_for(self, p: Preset) -> Option<Chord> {
        ACTIONS.iter().find(|a| a.0 == self).and_then(|a| match p {
            Preset::AnimeIai => a.3,
            Preset::Blender => a.4,
        })
    }

    pub fn from_id(id: &str) -> Option<MAction> {
        ACTIONS.iter().find(|a| a.1 == id).map(|a| a.0)
    }

    /// Ferramenta da prateleira da ação (perfil AnimeIAI: a tecla escolhe a ferramenta).
    pub fn tool(self) -> Option<ToolId> {
        Some(match self {
            MAction::Select => ToolId::MSelect,
            MAction::Move => ToolId::MMove,
            MAction::Rotate => ToolId::MRotate,
            MAction::Scale => ToolId::MScale,
            MAction::Extrude => ToolId::MExtrude,
            MAction::Inset => ToolId::MInset,
            MAction::LoopCut => ToolId::MLoopCut,
            MAction::Bevel => ToolId::MBevel,
            _ => return None,
        })
    }

    pub fn for_tool(t: ToolId) -> Option<MAction> {
        ACTIONS.iter().map(|a| a.0).find(|a| a.tool() == Some(t))
    }
}

/// O mapa em uso no contexto Modelagem.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModelKeymap {
    pub preset: Preset,
    /// As teclas das operações começam a operação na hora (modal, estilo Blender) em vez de escolher a ferramenta.
    pub modal_keys: bool,
    map: HashMap<MAction, Option<Chord>>,
    /// Captura de atalho em curso no editor de Preferências.
    capturing: Option<MAction>,
}

impl Default for ModelKeymap {
    fn default() -> Self {
        ModelKeymap::preset(Preset::AnimeIai)
    }
}

impl ModelKeymap {
    pub fn preset(p: Preset) -> ModelKeymap {
        ModelKeymap { preset: p, modal_keys: p == Preset::Blender, map: ACTIONS.iter().map(|a| (a.0, a.0.default_for(p))).collect(), capturing: None }
    }

    pub fn get(&self, a: MAction) -> Option<Chord> {
        self.map.get(&a).copied().flatten()
    }

    /// "V", "Ctrl+B"… ou vazio.
    pub fn label(&self, a: MAction) -> String {
        self.get(a).map(|c| c.label()).unwrap_or_default()
    }

    /// Texto da dica: "Mover (V)".
    pub fn tip(&self, a: MAction) -> String {
        let k = self.label(a);
        if k.is_empty() { tr(a.label_key()).to_owned() } else { format!("{} ({k})", tr(a.label_key())) }
    }

    /// Tecla da ferramenta (dica da prateleira).
    pub fn tool_label(&self, t: ToolId) -> Option<String> {
        MAction::for_tool(t).map(|a| self.label(a)).filter(|s| !s.is_empty())
    }

    pub fn owner(&self, c: Chord, except: Option<MAction>) -> Option<MAction> {
        ACTIONS.iter().map(|a| a.0).find(|&a| Some(a) != except && self.get(a) == Some(c))
    }

    /// Dá `chord` a `a`; quem o usava fica sem atalho (devolvido). Atalho fixo do app é recusado (`Err`).
    pub fn set(&mut self, a: MAction, chord: Option<Chord>) -> Result<Option<MAction>, Chord> {
        if let Some(c) = chord.filter(|c| APP_RESERVED.contains(c)) {
            return Err(c);
        }
        let lost = chord.and_then(|c| self.owner(c, Some(a)));
        if let Some(o) = lost {
            self.map.insert(o, None);
        }
        self.map.insert(a, chord);
        Ok(lost)
    }

    pub fn conflicts(&self) -> Vec<(MAction, MAction)> {
        let mut out = Vec::new();
        for (i, a) in ACTIONS.iter().enumerate() {
            for b in &ACTIONS[i + 1..] {
                if let (Some(x), Some(y)) = (self.get(a.0), self.get(b.0)) {
                    if x == y {
                        out.push((a.0, b.0));
                    }
                }
            }
        }
        out
    }

    /// Foi apertado agora (modificadores exatos)? Consome o evento.
    pub fn take(&self, i: &mut egui::InputState, a: MAction) -> bool {
        self.get(a).is_some_and(|c| c.take(i))
    }

    /// O que vai para `prefs.shortcuts`: o perfil, o modo das teclas e o que difere do perfil.
    pub fn to_overrides(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        if self.preset != Preset::AnimeIai {
            m.insert("modelagem.preset".to_owned(), self.preset.key().to_owned());
        }
        if self.modal_keys != (self.preset == Preset::Blender) {
            m.insert("modelagem.modal_keys".to_owned(), self.modal_keys.to_string());
        }
        for a in ACTIONS {
            let cur = self.get(a.0);
            if cur != a.0.default_for(self.preset) {
                m.insert(format!("modelagem.{}", a.1), cur.map(|c| c.label()).unwrap_or_default());
            }
        }
        m
    }

    /// Lê do `prefs.shortcuts` (ignora o que não é `modelagem.*` e o ilegível).
    pub fn from_overrides(over: &BTreeMap<String, String>) -> ModelKeymap {
        let preset = over.get("modelagem.preset").and_then(|p| Preset::from_key(p)).unwrap_or_default();
        let mut k = ModelKeymap::preset(preset);
        if let Some(v) = over.get("modelagem.modal_keys") {
            k.modal_keys = v == "true";
        }
        for (id, text) in over {
            let Some(a) = id.strip_prefix("modelagem.").and_then(MAction::from_id) else { continue };
            if text.trim().is_empty() {
                k.map.insert(a, None);
            } else if let Some(c) = Chord::parse(text) {
                let _ = k.set(a, Some(c));
            }
        }
        k
    }
}

/// Seção "Modelagem (viewport 3D)" da aba Atalhos. `true` = algo mudou (o chamador grava as preferências).
pub(crate) fn prefs_section(ui: &mut egui::Ui, t: &crate::theme::Tokens, keys: &mut ModelKeymap, note: &mut dyn FnMut(String, egui::Rect), notice: &mut Option<String>) -> bool {
    let mut changed = false;
    ui.add_space(10.0);
    ui.label(RichText::new(tr("mdl.keys.title")).strong());
    ui.label(RichText::new(tr("mdl.keys.help")).size(11.5).color(t.text_dim));
    ui.horizontal(|ui| {
        ui.label(tr("mdl.keys.preset"));
        for p in Preset::ALL {
            let r = ui.selectable_label(keys.preset == p, tr(p.label_key()));
            note(format!("keys:mdl_preset.{}", p.key()), r.rect);
            if r.clicked() && keys.preset != p {
                *keys = ModelKeymap::preset(p);
                changed = true;
            }
        }
    });
    let r = ui.checkbox(&mut keys.modal_keys, tr("mdl.keys.modal"));
    note("keys:mdl_modal".into(), r.rect);
    changed |= r.changed();
    // captura em curso: a próxima tecla vira o atalho (Esc cancela, Backspace/Delete limpa)
    if let Some(a) = keys.capturing {
        let pressed = ui.ctx().input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((k, m)) = pressed {
            keys.capturing = None;
            let chord = Chord { ctrl: m.ctrl || m.command, shift: m.shift, alt: m.alt, key: k };
            match k {
                Key::Escape => {}
                Key::Backspace | Key::Delete if !chord.ctrl && !chord.shift && !chord.alt && a != MAction::Delete => {
                    let _ = keys.set(a, None);
                    changed = true;
                }
                _ => match keys.set(a, Some(chord)) {
                    Ok(lost) => {
                        changed = true;
                        if let Some(o) = lost {
                            *notice = Some(tr("keys.moved").replace("{key}", &chord.label()).replace("{from}", tr(o.label_key())).replace("{to}", tr(a.label_key())));
                        }
                    }
                    Err(c) => *notice = Some(tr("mdl.keys.reserved").replace("{key}", &c.label())),
                },
            }
            ui.ctx().input_mut(|i| i.events.clear());
        }
    }
    egui::ScrollArea::vertical().id_salt("mdl_keys_scroll").max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new("mdl_keys_grid").num_columns(3).spacing([12.0, 4.0]).striped(true).show(ui, |ui| {
            for (a, id, label_key, _, _) in ACTIONS {
                ui.label(tr(label_key));
                let capturing = keys.capturing == Some(a);
                let text = if capturing {
                    RichText::new(tr("keys.press")).color(t.accent)
                } else {
                    match keys.get(a) {
                        Some(c) => RichText::new(c.label()).monospace(),
                        None => RichText::new(tr("keys.none")).color(t.text_dim),
                    }
                };
                let r = ui.add_sized([150.0, 22.0], egui::Button::selectable(capturing, text)).on_hover_text(tr("keys.click_to_change"));
                note(format!("keys:mdl_chord:{id}"), r.rect);
                if r.clicked() {
                    keys.capturing = if capturing { None } else { Some(a) };
                }
                let def = a.default_for(keys.preset);
                let rr = ui.add_enabled(keys.get(a) != def, egui::Button::new(tr("keys.default")).small()).on_hover_text(def.map(|c| c.label()).unwrap_or_default());
                note(format!("keys:mdl_reset:{id}"), rr.rect);
                if rr.clicked() {
                    let _ = keys.set(a, def);
                    changed = true;
                }
                ui.end_row();
            }
        });
    });
    let n = keys.conflicts().len();
    let (txt, col) = if n == 0 { (tr("keys.no_conflicts").to_owned(), t.text_dim) } else { (tr("keys.conflicts").replace("{n}", &n.to_string()), t.st_retake) };
    ui.label(RichText::new(txt).color(col));
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_presets_have_no_conflicts_and_avoid_app_keys() {
        for p in Preset::ALL {
            let k = ModelKeymap::preset(p);
            assert!(k.conflicts().is_empty(), "{p:?}: {:?}", k.conflicts());
            for a in ACTIONS {
                if let Some(c) = k.get(a.0) {
                    assert!(!APP_RESERVED.contains(&c), "{p:?}: {} usa {}", a.1, c.label());
                }
            }
            assert!(k.to_overrides().keys().all(|id| id == "modelagem.preset"), "{p:?}");
        }
        let mut ids: Vec<_> = ACTIONS.iter().map(|a| a.1).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ACTIONS.len());
    }

    #[test]
    fn animeiai_preset_follows_the_app() {
        let k = ModelKeymap::default();
        assert_eq!(k.preset, Preset::AnimeIai);
        assert!(!k.modal_keys);
        assert_eq!(k.label(MAction::Move), "V");
        assert_eq!(k.label(MAction::Rotate), "R");
        assert_eq!(k.label(MAction::Scale), "T");
        assert_eq!(k.label(MAction::Select), "S");
        assert_eq!(k.tool_label(ToolId::MMove).as_deref(), Some("V"));
        assert_eq!(k.tool_label(ToolId::MLoopCut).as_deref(), Some("Ctrl+R"));
        let b = ModelKeymap::preset(Preset::Blender);
        assert!(b.modal_keys);
        assert_eq!((b.label(MAction::Move), b.label(MAction::Rotate), b.label(MAction::Scale)), ("G".into(), "R".into(), "S".into()));
        assert_eq!(b.tip(MAction::Move), format!("{} (G)", tr("mdl.key.move")));
    }

    #[test]
    fn overrides_round_trip_and_reserved_keys_are_refused() {
        let mut k = ModelKeymap::preset(Preset::Blender);
        let lost = k.set(MAction::Extrude, Some(Chord::plain(Key::G))).unwrap();
        assert_eq!(lost, Some(MAction::Move));
        assert_eq!(k.get(MAction::Move), None);
        assert!(k.set(MAction::Merge, Some(Chord::ctrl(Key::Z))).is_err(), "Ctrl+Z é do app");
        k.modal_keys = false;
        let over = k.to_overrides();
        assert_eq!(over.get("modelagem.preset").map(String::as_str), Some("blender"));
        assert_eq!(over.get("modelagem.extrude").map(String::as_str), Some("G"));
        assert_eq!(over.get("modelagem.move").map(String::as_str), Some(""));
        assert_eq!(over.get("modelagem.modal_keys").map(String::as_str), Some("false"));
        let mut all = over.clone();
        all.insert("tool.fill".into(), "K".into());
        let back = ModelKeymap::from_overrides(&all);
        assert_eq!(back.preset, Preset::Blender);
        assert_eq!(back.get(MAction::Extrude), Some(Chord::plain(Key::G)));
        assert_eq!(back.get(MAction::Move), None);
        assert!(!back.modal_keys);
        // o mapa geral ignora os ids da Modelagem
        assert!(crate::keymap::Keymap::from_overrides(&all).conflicts().is_empty());
    }
}
