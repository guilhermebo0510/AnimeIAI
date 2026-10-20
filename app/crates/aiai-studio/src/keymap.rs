//! Mapa de atalhos do editor e do documento (S20): padrão inspirado em Clip Studio/Photoshop (B pincel, E borracha, G balde,
//! V mover, Ctrl+Z…), sobreposições do usuário guardadas nas preferências como texto ("Ctrl+Shift+Z") e detecção de
//! conflito: um atalho pertence a uma ação só — dar a uma ação o atalho de outra tira dele a outra (e diz qual).
//! Enter/Esc do mover e o espaço da mão temporária ficam fixos (convenção de todos os programas de desenho).

use egui::{InputState, Key, Modifiers};
use std::collections::{BTreeMap, HashMap};

/// Tecla + modificadores. `ctrl` é o Ctrl do Windows (Cmd no macOS, via `Modifiers::COMMAND`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl Chord {
    pub const fn plain(key: Key) -> Chord {
        Chord { ctrl: false, shift: false, alt: false, key }
    }
    pub const fn ctrl(key: Key) -> Chord {
        Chord { ctrl: true, shift: false, alt: false, key }
    }
    pub const fn shift(key: Key) -> Chord {
        Chord { ctrl: false, shift: true, alt: false, key }
    }
    pub const fn ctrl_shift(key: Key) -> Chord {
        Chord { ctrl: true, shift: true, alt: false, key }
    }
    pub const fn alt(key: Key) -> Chord {
        Chord { ctrl: false, shift: false, alt: true, key }
    }

    pub fn modifiers(&self) -> Modifiers {
        let mut m = Modifiers::NONE;
        if self.ctrl {
            m = m | Modifiers::COMMAND;
        }
        if self.shift {
            m = m | Modifiers::SHIFT;
        }
        if self.alt {
            m = m | Modifiers::ALT;
        }
        m
    }

    /// Texto para mostrar e para guardar: "Ctrl+Shift+Z", "B", "[" (símbolo quando a tecla tem um).
    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        // setas por nome: os símbolos do egui (⏴⏵) não existem na Noto Sans
        s.push_str(match self.key {
            Key::ArrowLeft => "Left",
            Key::ArrowRight => "Right",
            Key::ArrowUp => "Up",
            Key::ArrowDown => "Down",
            k => k.symbol_or_name(),
        });
        s
    }

    /// Lê o texto de `label` (aceita também os nomes do egui, "OpenBracket", e caixa qualquer nos modificadores).
    pub fn parse(text: &str) -> Option<Chord> {
        let text = text.trim();
        // "Ctrl++" (tecla mais): a última parte é a tecla, mesmo sendo "+"
        let (mods, key) = match text.rsplit_once('+') {
            Some((m, "")) if m.ends_with('+') => (&m[..m.len() - 1], "+"),
            Some((m, k)) => (m, k),
            None => ("", text),
        };
        let key = Key::from_name(key).or_else(|| Key::from_name(&key.to_uppercase()))?;
        let mut c = Chord::plain(key);
        for m in mods.split('+').map(str::trim).filter(|m| !m.is_empty()) {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "command" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" | "option" => c.alt = true,
                _ => return None,
            }
        }
        Some(c)
    }

    /// Foi apertado neste quadro, com EXATAMENTE estes modificadores? Consome o evento.
    /// (O `consume_key` do egui ignora Shift/Alt a mais: Shift+B dispararia o B.)
    pub fn take(&self, i: &mut InputState) -> bool {
        let m = i.modifiers;
        if (m.ctrl || m.command) != self.ctrl || m.shift != self.shift || m.alt != self.alt {
            return false;
        }
        i.consume_key(self.modifiers(), self.key)
    }
}

/// Ações com atalho configurável.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    ToolMove,
    ToolBrush,
    ToolPen,
    ToolPoints,
    ToolEraser,
    ToolFill,
    ToolLasso,
    ToolEyedropper,
    ToolHand,
    ToolZoom,
    ToolText,
    /// Régua (U, como no Clip Studio: o R segurado gira a visão).
    ToolRuler,
    /// Camada 3D: gizmo e câmera da camada (M).
    ToolModel3d,
    /// Réguas: mostrar/ocultar as linhas no canvas.
    RulersShow,
    /// Réguas: encaixe dos traços liga/desliga.
    RulerSnap,
    /// Réguas: passa para a régua seguinte da camada.
    RulerNext,
    SizeDown,
    SizeUp,
    FramePrev,
    FrameNext,
    FrameFirst,
    FrameLast,
    ViewFit,
    ViewActual,
    FileNew,
    FileOpen,
    FileSave,
    FileSaveAs,
    EditUndo,
    EditRedo,
    PointDelete,
    PointNext,
    PointPrev,
    PointCorner,
    PointAutoHandle,
    /// Animação: papel do cel atual (chave → breakdown → intercalação).
    CelRoleCycle,
    KeyNext,
    KeyPrev,
    DrawingNext,
    DrawingPrev,
    /// Mesa de luz (papel cebola) liga/desliga.
    OnionToggle,
    /// Linha do tempo normal/compacta.
    TimelineDensity,
}

/// (ação, id guardado nas preferências, chave i18n do nome, atalho padrão), na ordem do editor de atalhos.
pub const ACTIONS: [(Action, &str, &str, Chord); 42] = [
    (Action::ToolMove, "tool.move", "tool.move", Chord::plain(Key::V)),
    (Action::ToolBrush, "tool.brush", "tool.brush", Chord::plain(Key::B)),
    (Action::ToolPen, "tool.pen", "tool.pen", Chord::plain(Key::P)),
    (Action::ToolPoints, "tool.points", "tool.points", Chord::plain(Key::A)),
    (Action::ToolEraser, "tool.eraser", "tool.eraser", Chord::plain(Key::E)),
    (Action::ToolFill, "tool.fill", "tool.fill", Chord::plain(Key::G)),
    (Action::ToolLasso, "tool.lasso", "tool.lasso", Chord::plain(Key::S)),
    (Action::ToolEyedropper, "tool.eyedropper", "tool.eyedropper", Chord::plain(Key::I)),
    (Action::ToolHand, "tool.hand", "tool.hand", Chord::plain(Key::H)),
    (Action::ToolZoom, "tool.zoom", "tool.zoom", Chord::plain(Key::Z)),
    (Action::ToolText, "tool.text", "tool.text", Chord::plain(Key::T)),
    (Action::ToolRuler, "tool.ruler", "tool.ruler", Chord::plain(Key::U)),
    (Action::ToolModel3d, "tool.model3d", "tool.model3d", Chord::plain(Key::M)),
    (Action::RulersShow, "ruler.show", "keys.rulers_show", Chord::ctrl(Key::Semicolon)),
    (Action::RulerSnap, "ruler.snap", "keys.ruler_snap", Chord::ctrl_shift(Key::Semicolon)),
    (Action::RulerNext, "ruler.next", "keys.ruler_next", Chord::shift(Key::U)),
    (Action::SizeDown, "size.down", "keys.size_down", Chord::plain(Key::OpenBracket)),
    (Action::SizeUp, "size.up", "keys.size_up", Chord::plain(Key::CloseBracket)),
    (Action::FramePrev, "frame.prev", "keys.frame_prev", Chord::plain(Key::ArrowLeft)),
    (Action::FrameNext, "frame.next", "keys.frame_next", Chord::plain(Key::ArrowRight)),
    (Action::FrameFirst, "frame.first", "keys.frame_first", Chord::plain(Key::Home)),
    (Action::FrameLast, "frame.last", "keys.frame_last", Chord::plain(Key::End)),
    (Action::ViewFit, "view.fit", "keys.view_fit", Chord::ctrl(Key::Num0)),
    (Action::ViewActual, "view.actual", "keys.view_actual", Chord::ctrl(Key::Num1)),
    (Action::FileNew, "file.new", "menu.file.new", Chord::ctrl(Key::N)),
    (Action::FileOpen, "file.open", "menu.file.open", Chord::ctrl(Key::O)),
    (Action::FileSave, "file.save", "menu.file.save", Chord::ctrl(Key::S)),
    (Action::FileSaveAs, "file.save_as", "menu.file.save_as", Chord::ctrl_shift(Key::S)),
    (Action::EditUndo, "edit.undo", "menu.edit.undo", Chord::ctrl(Key::Z)),
    (Action::EditRedo, "edit.redo", "menu.edit.redo", Chord::ctrl_shift(Key::Z)),
    (Action::PointDelete, "point.delete", "keys.point_delete", Chord::plain(Key::Delete)),
    (Action::PointNext, "point.next", "keys.point_next", Chord::plain(Key::Period)),
    (Action::PointPrev, "point.prev", "keys.point_prev", Chord::plain(Key::Comma)),
    (Action::PointCorner, "point.corner", "keys.point_corner", Chord::plain(Key::C)),
    (Action::PointAutoHandle, "point.auto_handle", "keys.point_auto_handle", Chord::shift(Key::C)),
    (Action::CelRoleCycle, "animation.cel_role", "keys.cel_role", Chord::plain(Key::K)),
    (Action::KeyNext, "animation.key_next", "keys.key_next", Chord::ctrl(Key::ArrowRight)),
    (Action::KeyPrev, "animation.key_prev", "keys.key_prev", Chord::ctrl(Key::ArrowLeft)),
    (Action::DrawingNext, "animation.drawing_next", "keys.drawing_next", Chord::alt(Key::ArrowRight)),
    (Action::DrawingPrev, "animation.drawing_prev", "keys.drawing_prev", Chord::alt(Key::ArrowLeft)),
    (Action::OnionToggle, "animation.onion", "keys.onion", Chord::plain(Key::O)),
    (Action::TimelineDensity, "animation.timeline_density", "keys.timeline_density", Chord::ctrl_shift(Key::T)),
];

impl Action {
    pub fn id(self) -> &'static str {
        ACTIONS.iter().find(|a| a.0 == self).map(|a| a.1).unwrap_or("")
    }
    pub fn label_key(self) -> &'static str {
        ACTIONS.iter().find(|a| a.0 == self).map(|a| a.2).unwrap_or("")
    }
    pub fn default_chord(self) -> Chord {
        ACTIONS.iter().find(|a| a.0 == self).map(|a| a.3).expect("toda ação tem padrão")
    }
    pub fn from_id(id: &str) -> Option<Action> {
        ACTIONS.iter().find(|a| a.1 == id).map(|a| a.0)
    }

    /// A ação de escolher a ferramenta (dicas da prateleira). As ferramentas da Composição (girar, âncora, câmera) têm
    /// tecla fixa naquele workspace (`ToolId::comp_key`) e não entram no mapa geral.
    pub fn for_tool(t: crate::workspace::ToolId) -> Option<Action> {
        use crate::workspace::ToolId as T;
        Some(match t {
            T::Move => Action::ToolMove,
            T::Brush => Action::ToolBrush,
            T::Pen => Action::ToolPen,
            T::Points => Action::ToolPoints,
            T::Eraser => Action::ToolEraser,
            T::Fill => Action::ToolFill,
            T::Lasso => Action::ToolLasso,
            T::Eyedropper => Action::ToolEyedropper,
            T::Hand => Action::ToolHand,
            T::Zoom => Action::ToolZoom,
            T::Text => Action::ToolText,
            T::Ruler => Action::ToolRuler,
            T::Model3d => Action::ToolModel3d,
            T::Rotate | T::Anchor | T::Camera => return None,
            // Modelagem: as teclas do Blender, só no viewport 3D (fora do mapa de atalhos)
            _ => return None,
        })
    }
}

/// O mapa em uso: cada ação com um atalho ou nenhum; nunca dois iguais.
#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    map: HashMap<Action, Option<Chord>>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap { map: ACTIONS.iter().map(|a| (a.0, Some(a.3))).collect() }
    }
}

impl Keymap {
    pub fn get(&self, a: Action) -> Option<Chord> {
        self.map.get(&a).copied().flatten()
    }

    pub fn label(&self, a: Action) -> String {
        self.get(a).map(|c| c.label()).unwrap_or_default()
    }

    /// Quem usa este atalho (fora `except`).
    pub fn owner(&self, c: Chord, except: Option<Action>) -> Option<Action> {
        ACTIONS.iter().map(|a| a.0).find(|&a| Some(a) != except && self.get(a) == Some(c))
    }

    /// Dá `chord` a `a`. Se outra ação o usava, ela fica sem atalho e é devolvida (o editor avisa).
    pub fn set(&mut self, a: Action, chord: Option<Chord>) -> Option<Action> {
        let lost = chord.and_then(|c| self.owner(c, Some(a)));
        if let Some(o) = lost {
            self.map.insert(o, None);
        }
        self.map.insert(a, chord);
        lost
    }

    /// Pares de ações com o mesmo atalho (o `set` impede; serve de verificação do que veio do arquivo).
    pub fn conflicts(&self) -> Vec<(Action, Action)> {
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

    /// Só o que difere do padrão, como texto ("" = sem atalho): é o que vai para `prefs.json`.
    pub fn to_overrides(&self) -> BTreeMap<String, String> {
        ACTIONS
            .iter()
            .filter(|a| self.get(a.0) != Some(a.3))
            .map(|a| (a.1.to_owned(), self.get(a.0).map(|c| c.label()).unwrap_or_default()))
            .collect()
    }

    /// Padrão + sobreposições. Ids e textos ilegíveis são ignorados; se uma sobreposição pega o atalho padrão de outra
    /// ação, a outra perde (vale o que o usuário escolheu).
    pub fn from_overrides(over: &BTreeMap<String, String>) -> Keymap {
        let mut k = Keymap::default();
        for (id, text) in over {
            let Some(a) = Action::from_id(id) else { continue };
            if text.trim().is_empty() {
                k.map.insert(a, None);
            } else if let Some(c) = Chord::parse(text) {
                k.set(a, Some(c));
            }
        }
        k
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_round_trip() {
        for (_, _, _, c) in ACTIONS {
            assert_eq!(Chord::parse(&c.label()), Some(c), "{}", c.label());
        }
        assert_eq!(Chord::parse("ctrl+shift+z"), Some(Chord::ctrl_shift(Key::Z)));
        assert_eq!(Chord::parse("OpenBracket"), Some(Chord::plain(Key::OpenBracket)));
        assert_eq!(Chord::parse("Ctrl++"), Some(Chord::ctrl(Key::Plus)));
        assert_eq!(Chord::parse("Hyper+B"), None);
        assert_eq!(Chord::parse(""), None);
    }

    #[test]
    fn defaults_have_no_conflicts_and_ids_are_unique() {
        let k = Keymap::default();
        assert!(k.conflicts().is_empty());
        let mut ids: Vec<_> = ACTIONS.iter().map(|a| a.1).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ACTIONS.len());
        assert!(k.to_overrides().is_empty());
    }

    #[test]
    fn taking_a_used_chord_unassigns_the_other_action() {
        let mut k = Keymap::default();
        let lost = k.set(Action::ToolBrush, Some(Chord::plain(Key::V)));
        assert_eq!(lost, Some(Action::ToolMove));
        assert_eq!(k.get(Action::ToolMove), None);
        assert_eq!(k.get(Action::ToolBrush), Some(Chord::plain(Key::V)));
        assert!(k.conflicts().is_empty());
        let over = k.to_overrides();
        assert_eq!(over.get("tool.brush").map(String::as_str), Some("V"));
        assert_eq!(over.get("tool.move").map(String::as_str), Some(""));
        assert_eq!(Keymap::from_overrides(&over), k);
    }

    #[test]
    fn overrides_from_file_win_and_garbage_is_ignored() {
        let mut over = BTreeMap::new();
        over.insert("tool.fill".to_owned(), "B".to_owned()); // pega o B do pincel
        over.insert("nao.existe".to_owned(), "K".to_owned());
        over.insert("tool.pen".to_owned(), "Hyper+Q".to_owned());
        let k = Keymap::from_overrides(&over);
        assert_eq!(k.get(Action::ToolFill), Some(Chord::plain(Key::B)));
        assert_eq!(k.get(Action::ToolBrush), None);
        assert_eq!(k.get(Action::ToolPen), Some(Chord::plain(Key::P)));
        assert!(k.conflicts().is_empty());
    }
}
