//! Prateleira de ferramentas das áreas de desenho (S25, no espírito do Clip Studio, com a nossa cara): uma coluna estreita
//! só de ícones, ferramentas por grupo (Caneta, Lápis, Pincel, Aerógrafo, Decoração, Borracha, Mistura...) e cada uma com
//! as suas sub-ferramentas no painel Grupo de ferramentas.
//!
//! Uma ferramenta da prateleira (`Slot`) é uma camada de interface sobre as ferramentas do motor (`ToolId`):
//! - a Caneta usa a caneta vetorial (penas) e o pincel raster (pincéis dos grupos caneta e marcador);
//! - o Lápis, o Pincel, o Aerógrafo, a Decoração e a Mistura são o pincel raster com os pincéis do grupo;
//! - a Figura são as formas da caneta vetorial (linha, curva, retângulo, elipse);
//! - o Corrigir linha é o Editar pontos; a Seleção automática é a varinha da Seleção.
//!
//! Apertar de novo a letra de uma ferramenta passa para a próxima que divide a letra (P: Caneta → Lápis; B: Pincel →
//! Aerógrafo → Decoração; G: Balde → Degradê; U: Figura → Régua). Cada ferramenta lembra a última sub-ferramenta usada e,
//! nos pincéis, o tamanho e a opacidade ajustados.

use super::editor::{EraserTip, PenShape, PointsMode};
use super::selection::SelTool;
use super::ui_kit::Env;
use crate::i18n::tr;
use crate::icons::IconId;
use crate::keymap::Action;
use crate::workspace::{ToolId, WorkspaceId};
use aiai_core::VectorTip;
use aiai_paint::brush::{BrushMode, BrushPreset, BrushTip};
use aiai_tools::erase::EraseMode;
use egui::{Sense, Stroke, StrokeKind, Ui};
use std::collections::HashMap;

/// Ferramenta da prateleira.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Slot {
    #[default]
    Select,
    AutoSelect,
    MoveLayer,
    Hand,
    Eyedropper,
    Pen,
    Pencil,
    Brush,
    Airbrush,
    Decoration,
    Eraser,
    Blend,
    Fill,
    Gradient,
    Figure,
    Ruler,
    Text,
    CorrectLine,
}

/// Aba de grupo do painel de sub-ferramentas. `group` = grupo de pincéis que a aba lista (além das fixas).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tab {
    pub key: &'static str,
    pub title_key: &'static str,
    pub icon: IconId,
    pub group: Option<&'static str>,
}

/// Grupos de pincéis (o campo `group` do preset; enquanto ele não existe, `preset_group` deduz).
pub(crate) const GROUPS: [&str; 8] = ["caneta", "marcador", "lapis", "pincel", "aerografo", "decoracao", "borracha", "mistura"];

impl Slot {
    /// Ordem da prateleira: seleção e navegação; desenho; cor, figura, texto e correção (a Seleção abre a prateleira).
    pub const ALL: [Slot; 18] = [
        Slot::Select,
        Slot::AutoSelect,
        Slot::MoveLayer,
        Slot::Hand,
        Slot::Eyedropper,
        Slot::Pen,
        Slot::Pencil,
        Slot::Brush,
        Slot::Airbrush,
        Slot::Decoration,
        Slot::Eraser,
        Slot::Blend,
        Slot::Fill,
        Slot::Gradient,
        Slot::Figure,
        Slot::Ruler,
        Slot::Text,
        Slot::CorrectLine,
    ];

    /// Chave estável (retângulos `tool:<chave>` da bridge). Onde a ferramenta da prateleira é a do motor, a chave é a
    /// mesma (`lasso`, `move`, `pen`, `brush`, `eraser`, `fill`...).
    pub fn key(self) -> &'static str {
        match self {
            Slot::Select => "lasso",
            Slot::AutoSelect => "wand",
            Slot::MoveLayer => "move",
            Slot::Hand => "hand",
            Slot::Eyedropper => "eyedropper",
            Slot::Pen => "pen",
            Slot::Pencil => "pencil",
            Slot::Brush => "brush",
            Slot::Airbrush => "airbrush",
            Slot::Decoration => "decoration",
            Slot::Eraser => "eraser",
            Slot::Blend => "blend",
            Slot::Fill => "fill",
            Slot::Gradient => "gradient",
            Slot::Figure => "figure",
            Slot::Ruler => "ruler",
            Slot::Text => "text",
            Slot::CorrectLine => "points",
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn from_key(k: &str) -> Option<Slot> {
        Self::ALL.into_iter().find(|s| s.key() == k)
    }

    pub fn title_key(self) -> &'static str {
        match self {
            Slot::Select => "shelf.select",
            Slot::AutoSelect => "shelf.auto_select",
            Slot::MoveLayer => "shelf.move_layer",
            Slot::Hand => "shelf.hand",
            Slot::Eyedropper => "shelf.eyedropper",
            Slot::Pen => "shelf.pen",
            Slot::Pencil => "shelf.pencil",
            Slot::Brush => "shelf.brush",
            Slot::Airbrush => "shelf.airbrush",
            Slot::Decoration => "shelf.decoration",
            Slot::Eraser => "shelf.eraser",
            Slot::Blend => "shelf.blend",
            Slot::Fill => "shelf.fill",
            Slot::Gradient => "shelf.gradient",
            Slot::Figure => "shelf.figure",
            Slot::Ruler => "shelf.ruler",
            Slot::Text => "shelf.text",
            Slot::CorrectLine => "shelf.correct_line",
        }
    }

    pub fn icon(self) -> IconId {
        match self {
            Slot::Select => IconId::ToolLasso,
            Slot::AutoSelect => IconId::ToolWand,
            Slot::MoveLayer => IconId::ToolMove,
            Slot::Hand => IconId::ToolHand,
            Slot::Eyedropper => IconId::ToolEyedropper,
            Slot::Pen => IconId::ToolPen,
            Slot::Pencil => IconId::ToolPencil,
            Slot::Brush => IconId::ToolBrush,
            Slot::Airbrush => IconId::ToolAirbrush,
            Slot::Decoration => IconId::ToolDecoration,
            Slot::Eraser => IconId::ToolEraser,
            Slot::Blend => IconId::ToolBlend,
            Slot::Fill => IconId::ToolFill,
            Slot::Gradient => IconId::ToolGradient,
            Slot::Figure => IconId::ToolFigure,
            Slot::Ruler => IconId::ToolRuler,
            Slot::Text => IconId::ToolText,
            Slot::CorrectLine => IconId::ToolPoints,
        }
    }

    /// Ação do mapa de atalhos que escolhe a ferramenta (várias ferramentas dividem a ação: a letra alterna entre elas).
    pub fn action(self) -> Action {
        match self {
            Slot::Select => Action::ToolLasso,
            Slot::AutoSelect => Action::ToolWand,
            Slot::MoveLayer => Action::ToolMove,
            Slot::Hand => Action::ToolHand,
            Slot::Eyedropper => Action::ToolEyedropper,
            Slot::Pen | Slot::Pencil => Action::ToolPen,
            Slot::Brush | Slot::Airbrush | Slot::Decoration => Action::ToolBrush,
            Slot::Eraser => Action::ToolEraser,
            Slot::Blend => Action::ToolBlend,
            Slot::Fill | Slot::Gradient => Action::ToolFill,
            Slot::Figure | Slot::Ruler => Action::ToolRuler,
            Slot::Text => Action::ToolText,
            Slot::CorrectLine => Action::ToolPoints,
        }
    }

    /// Ferramentas do motor que as sub-ferramentas usam (vazio = ainda não existe no motor).
    pub fn engines(self) -> &'static [ToolId] {
        use ToolId as T;
        match self {
            Slot::Select | Slot::AutoSelect => &[T::Lasso],
            Slot::MoveLayer => &[T::Move, T::Model3d],
            Slot::Hand => &[T::Hand, T::Zoom],
            Slot::Eyedropper => &[T::Eyedropper],
            Slot::Pen | Slot::Pencil => &[T::Pen, T::Brush],
            Slot::Brush | Slot::Airbrush | Slot::Decoration | Slot::Blend => &[T::Brush],
            Slot::Eraser => &[T::Eraser],
            Slot::Fill => &[T::Fill],
            // degradê por arrasto no canvas (S25): pixels, camada de degradê editável ou degradê livre
            Slot::Gradient => &[T::Gradient],
            Slot::Figure => &[T::Pen],
            Slot::Ruler => &[T::Ruler],
            Slot::Text => &[T::Text],
            Slot::CorrectLine => &[T::Points],
        }
    }

    /// Existe no motor (senão: ícone desabilitado, dica "em breve").
    pub fn available(self) -> bool {
        !self.engines().is_empty()
    }

    /// Abas de grupo do painel de sub-ferramentas.
    pub fn tabs(self) -> &'static [Tab] {
        match self {
            Slot::Select => &[Tab { key: "area", title_key: "shelf.tab.area", icon: IconId::ToolLasso, group: None }, Tab { key: "objects", title_key: "shelf.tab.objects", icon: IconId::LayerVector, group: None }],
            Slot::AutoSelect => &[Tab { key: "wand", title_key: "shelf.tab.wand", icon: IconId::ToolWand, group: None }],
            Slot::MoveLayer => &[Tab { key: "move", title_key: "shelf.tab.move", icon: IconId::ToolMove, group: None }],
            Slot::Hand => &[Tab { key: "view", title_key: "shelf.tab.view", icon: IconId::ToolHand, group: None }],
            Slot::Eyedropper => &[Tab { key: "eyedropper", title_key: "shelf.tab.eyedropper", icon: IconId::ToolEyedropper, group: None }],
            Slot::Pen => &[Tab { key: "caneta", title_key: "shelf.tab.pen", icon: IconId::ToolPen, group: Some("caneta") }, Tab { key: "marcador", title_key: "shelf.tab.marker", icon: IconId::ToolMarker, group: Some("marcador") }],
            Slot::Pencil => &[Tab { key: "lapis", title_key: "shelf.tab.pencil", icon: IconId::ToolPencil, group: Some("lapis") }],
            Slot::Brush => &[Tab { key: "pincel", title_key: "shelf.tab.brush", icon: IconId::ToolBrush, group: Some("pincel") }],
            Slot::Airbrush => &[Tab { key: "aerografo", title_key: "shelf.tab.airbrush", icon: IconId::ToolAirbrush, group: Some("aerografo") }],
            Slot::Decoration => &[Tab { key: "decoracao", title_key: "shelf.tab.decoration", icon: IconId::ToolDecoration, group: Some("decoracao") }],
            Slot::Eraser => &[Tab { key: "raster", title_key: "shelf.tab.eraser", icon: IconId::ToolEraser, group: Some("borracha") }, Tab { key: "vector", title_key: "shelf.tab.eraser_vector", icon: IconId::LayerVector, group: None }],
            Slot::Blend => &[Tab { key: "mistura", title_key: "shelf.tab.blend", icon: IconId::ToolBlend, group: Some("mistura") }],
            Slot::Fill => &[Tab { key: "fill", title_key: "shelf.tab.fill", icon: IconId::ToolFill, group: None }],
            Slot::Gradient => &[Tab { key: "gradient", title_key: "shelf.tab.gradient", icon: IconId::ToolGradient, group: None }, Tab { key: "freeform", title_key: "grad.tab.freeform", icon: IconId::GradFreeform, group: None }],
            Slot::Figure => &[Tab { key: "figure", title_key: "fig.tab.direct", icon: IconId::ToolFigure, group: None }, Tab { key: "speed", title_key: "fig.tab.speed", icon: IconId::FigSpeed, group: None }, Tab { key: "focus", title_key: "fig.tab.focus", icon: IconId::FigFocus, group: None }],
            Slot::Ruler => &[Tab { key: "ruler", title_key: "shelf.tab.ruler", icon: IconId::ToolRuler, group: None }],
            Slot::Text => &[Tab { key: "text", title_key: "shelf.tab.text", icon: IconId::ToolText, group: None }],
            Slot::CorrectLine => &[Tab { key: "points", title_key: "shelf.tab.correct", icon: IconId::ToolPoints, group: None }, Tab { key: "dust", title_key: "fix.tab.dust", icon: IconId::ToolEraser, group: None }],
        }
    }

    /// Grupos de pincéis da ferramenta (Caneta = caneta + marcador...).
    pub fn groups(self) -> Vec<&'static str> {
        self.tabs().iter().filter_map(|t| t.group).collect()
    }

    /// Bloco da prateleira (um respiro separa os blocos): 0 seleção e visão, 1 desenho, 2 cor, forma, texto e correção.
    pub fn block(self) -> u8 {
        match self {
            Slot::Select | Slot::AutoSelect | Slot::MoveLayer | Slot::Hand => 0,
            Slot::Eyedropper | Slot::Pen | Slot::Pencil | Slot::Brush | Slot::Airbrush | Slot::Decoration | Slot::Eraser | Slot::Blend => 1,
            _ => 2,
        }
    }
}

/// Ferramentas da prateleira de um workspace (vazio = o workspace tem prateleira própria ou nenhuma: Composição,
/// Modelagem e os sem canvas). Uma ferramenta aparece se alguma das suas ferramentas do motor vale no perfil do
/// workspace, com as exceções de cada etapa (a Pintura não desenha linha; o Cenário não tem caneta).
pub(crate) fn slots_for(ws: WorkspaceId) -> Vec<Slot> {
    let p = ws.profile();
    if !p.canvas || matches!(ws, WorkspaceId::Composicao) {
        return Vec::new();
    }
    let excluded: &[Slot] = match ws {
        WorkspaceId::Pintura => &[Slot::Pen, Slot::Pencil, Slot::Decoration],
        WorkspaceId::Cenario => &[Slot::Pen],
        _ => &[],
    };
    Slot::ALL
        .into_iter()
        .filter(|s| !excluded.contains(s))
        .filter(|s| s.engines().iter().any(|t| p.allows(*t)))
        .collect()
}

/// A ferramenta que a letra escolhe: a seguinte das que dividem a ação, ou a primeira delas. Ferramentas que não
/// existem no motor (degradê) são puladas.
pub(crate) fn cycle(slots: &[Slot], current: Option<Slot>, action: Action) -> Option<Slot> {
    let group: Vec<Slot> = slots.iter().copied().filter(|s| s.action() == action && s.available()).collect();
    let first = group.first().copied()?;
    match current.and_then(|c| group.iter().position(|s| *s == c)) {
        Some(i) => Some(group[(i + 1) % group.len()]),
        None => Some(first),
    }
}

// ------------------------------------------------------------------------------------------------ sub-ferramentas

/// Sub-ferramenta: um pincel da biblioteca ou um modo de uma ferramenta do motor.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Sub {
    /// Pincel raster da biblioteca pela chave (`preset_key`).
    Preset(String),
    /// Pena da caneta vetorial (à mão livre).
    PenTip(VectorTip),
    /// Forma da caneta vetorial (Figura).
    PenShape(PenShape),
    /// Borracha raster dura (0) ou macia (1).
    EraserRaster(u8),
    EraserTip(EraserTip),
    /// A borracha apaga com a ponta do pincel em uso.
    EraserBrush,
    /// A borracha apaga com um pincel do grupo borracha.
    EraserPreset(String),
    EraserVector(EraseMode),
    Fill(u8),
    Select(SelTool),
    Points(PointsMode),
    /// Corrigir linha além dos pontos de controle: puxar, simplificar, conectar, largura, redesenhar e poeira.
    Fix(super::line_fix::FixTool),
    /// Tipo de régua que a ferramenta Régua cria (linear, curva, figura, caneta de régua, especiais, guia, perspectiva...).
    Ruler(super::rulers::NewKind),
    /// Degradê pronto, de paleta ou livre (ferramenta Degradê).
    Gradient(super::gradient_tool::GradPick),
    /// Ferramenta do motor sem variações (mover, objeto 3D, mão, zoom, conta-gotas, texto).
    Engine(ToolId),
}

/// Borrachas raster (chave, dureza) e jeitos do balde (chave, fresta, expandir, fechar sobras).
pub(crate) const ERASERS: [(&str, f32); 2] = [("eraser.hard", 0.9), ("eraser.soft", 0.0)];
pub(crate) const FILLS: [(&str, f32, u32, bool); 4] = [("fill.normal", 3.0, 2, false), ("fill.wide", 8.0, 2, false), ("fill.exact", 0.0, 0, false), ("fill.leftovers", 3.0, 2, true)];

impl Sub {
    /// Ferramenta do motor que a sub-ferramenta usa.
    pub fn engine(&self) -> ToolId {
        match self {
            Sub::Preset(_) => ToolId::Brush,
            Sub::PenTip(_) | Sub::PenShape(_) => ToolId::Pen,
            Sub::EraserRaster(_) | Sub::EraserTip(_) | Sub::EraserBrush | Sub::EraserPreset(_) | Sub::EraserVector(_) => ToolId::Eraser,
            Sub::Fill(_) => ToolId::Fill,
            Sub::Select(_) => ToolId::Lasso,
            Sub::Points(_) | Sub::Fix(_) => ToolId::Points,
            Sub::Ruler(_) => ToolId::Ruler,
            Sub::Gradient(_) => ToolId::Gradient,
            Sub::Engine(t) => *t,
        }
    }

    /// Chave da sub-ferramenta para guardar escolhas (olhos do painel de propriedades).
    pub fn key(&self) -> String {
        match self {
            Sub::Preset(k) | Sub::EraserPreset(k) => k.clone(),
            Sub::PenTip(t) => format!("pen:{t:?}").to_lowercase(),
            Sub::PenShape(s) => format!("shape:{s:?}").to_lowercase(),
            Sub::EraserRaster(i) => format!("eraser:{i}"),
            Sub::EraserTip(t) => format!("eraser:{t:?}").to_lowercase(),
            Sub::EraserBrush => "eraser:brush".into(),
            Sub::EraserVector(m) => format!("eraser:{m:?}").to_lowercase(),
            Sub::Fill(i) => format!("fill:{i}"),
            Sub::Select(m) => format!("select:{m:?}").to_lowercase(),
            Sub::Points(m) => format!("points:{m:?}").to_lowercase(),
            Sub::Fix(f) => format!("fix:{}", f.key()),
            Sub::Ruler(k) => format!("ruler:{}", k.id()),
            Sub::Gradient(g) => super::gradient_tool::pick_key(g),
            Sub::Engine(t) => t.key().to_owned(),
        }
    }

    /// A sub-ferramenta usa tamanho de pincel (senão a paleta Tamanho do pincel fica desabilitada).
    pub fn has_size(&self) -> bool {
        matches!(self.engine(), ToolId::Brush | ToolId::Pen | ToolId::Eraser) || matches!(self, Sub::Fix(f) if f.has_size())
    }

    /// Sub-ferramenta vetorial (selo no item da lista).
    pub fn is_vector(&self) -> bool {
        matches!(self, Sub::PenTip(_) | Sub::PenShape(_) | Sub::EraserVector(_)) || matches!(self, Sub::Fix(f) if !f.dust())
    }
}

/// Sub-ferramentas fixas de uma aba (as que não são pincéis da biblioteca).
pub(crate) fn fixed_subs(slot: Slot, tab_key: &str) -> Vec<Sub> {
    use ToolId as T;
    match (slot, tab_key) {
        (Slot::Select, "area") => [SelTool::Rect, SelTool::Ellipse, SelTool::Lasso, SelTool::Polygon].into_iter().map(Sub::Select).collect(),
        (Slot::Select, "objects") => vec![Sub::Select(SelTool::Strokes)],
        (Slot::AutoSelect, _) => vec![Sub::Select(SelTool::Wand)],
        (Slot::MoveLayer, _) => vec![Sub::Engine(T::Move), Sub::Engine(T::Model3d)],
        (Slot::Hand, _) => vec![Sub::Engine(T::Hand), Sub::Engine(T::Zoom)],
        (Slot::Eyedropper, _) => vec![Sub::Engine(T::Eyedropper)],
        (Slot::Pen, "caneta") => vec![Sub::PenTip(VectorTip::GPen), Sub::PenTip(VectorTip::MaruPen)],
        (Slot::Pen, "marcador") => vec![Sub::PenTip(VectorTip::Marker)],
        (Slot::Pencil, _) => vec![Sub::PenTip(VectorTip::Pencil)],
        (Slot::Eraser, "raster") => vec![Sub::EraserRaster(0), Sub::EraserRaster(1), Sub::EraserTip(EraserTip::Pencil), Sub::EraserTip(EraserTip::Block), Sub::EraserBrush],
        (Slot::Eraser, "vector") => [EraseMode::Segment, EraseMode::Intersection, EraseMode::Whole].into_iter().map(Sub::EraserVector).collect(),
        (Slot::Fill, _) => (0..FILLS.len() as u8).map(Sub::Fill).collect(),
        (Slot::Figure, "speed") => vec![Sub::PenShape(PenShape::SpeedLines)],
        (Slot::Figure, "focus") => vec![Sub::PenShape(PenShape::FocusLines)],
        (Slot::Figure, _) => super::figure::DIRECT.into_iter().map(Sub::PenShape).collect(),
        (Slot::Gradient, "freeform") => vec![Sub::Gradient(super::gradient_tool::GradPick::Freeform)],
        (Slot::Gradient, _) => super::gradient_tool::fixed_picks().into_iter().map(Sub::Gradient).collect(),
        (Slot::Ruler, _) => super::rulers::NewKind::ALL.into_iter().map(Sub::Ruler).collect(),
        (Slot::Text, _) => vec![Sub::Engine(T::Text)],
        // pontos de controle (os modos do Editar pontos) e as correções, na ordem do Clip Studio; a poeira tem aba própria
        (Slot::CorrectLine, "dust") => super::line_fix::FixTool::DUST.into_iter().map(Sub::Fix).collect(),
        (Slot::CorrectLine, _) => super::panels::properties::POINTS_MODES.iter().map(|m| Sub::Points(*m)).chain(super::line_fix::FixTool::LINE.into_iter().map(Sub::Fix)).collect(),
        _ => Vec::new(),
    }
}

/// Chave de um pincel da biblioteca: o id (pincel com arquivo) ou o nome (de fábrica).
pub(crate) fn preset_key(p: &BrushPreset) -> String {
    if p.id.is_empty() { p.name.clone() } else { p.id.clone() }
}

/// Grupo do pincel (caneta, marcador, lápis, pincel, aerógrafo, decoração, borracha, mistura): o campo `group` do
/// preset; sem ele (pincel antigo do usuário, pacote importado) o grupo sai da categoria e do jeito do pincel:
/// lápis → lápis; cenário (carimbos) → decoração; borracha → borracha; quem mistura a cor da camada → mistura;
/// redondo bem macio de fluxo baixo → aerógrafo; textura de grão fina → lápis; o resto → pincel.
pub(crate) fn preset_group(p: &BrushPreset) -> &'static str {
    if let Some(g) = explicit_group(p) {
        return g;
    }
    let soft_air = matches!(p.tip, BrushTip::Round { hardness, .. } if hardness <= 0.05) && p.flow <= 0.15;
    let grain_pencil = p.texture.as_ref().is_some_and(|t| t.kind == aiai_paint::brush::TextureKind::Grain) && p.size <= 16.0;
    match p.category.as_str() {
        "lapis" => "lapis",
        "cenario" => "decoracao",
        "borracha" => "borracha",
        _ if p.mode == BrushMode::Erase => "borracha",
        _ if p.mix >= 0.5 => "mistura",
        _ if matches!(p.tip, BrushTip::Shape { .. }) => "decoracao",
        _ if soft_air => "aerografo",
        _ if grain_pencil => "lapis",
        _ => "pincel",
    }
}

/// O grupo gravado no preset, se é um dos conhecidos.
fn explicit_group(p: &BrushPreset) -> Option<&'static str> {
    GROUPS.iter().find(|g| **g == p.group).copied()
}
    // degradês guardados nas paletas do projeto entram como prontos
    }

/// Sub-ferramentas de uma aba neste workspace: as fixas cuja ferramenta do motor vale no perfil e os pincéis do grupo.
pub(crate) fn subs(env: &Env, slot: Slot, t: &Tab) -> Vec<Sub> {
    let p = env.ws.profile();
    let mut v: Vec<Sub> = fixed_subs(slot, t.key).into_iter().filter(|s| p.allows(s.engine())).collect();
    if slot == Slot::Gradient && t.key == "gradient" && p.allows(ToolId::Gradient) {
        v.extend(super::gradient_tool::palette_picks(env).into_iter().map(Sub::Gradient));
    if let Some(g) = t.group {
        let eraser = slot == Slot::Eraser;
        if p.allows(if eraser { ToolId::Eraser } else { ToolId::Brush }) {
            for b in env.ed.brushes.items.iter().filter(|b| preset_group(&b.preset) == g) {
                let k = preset_key(&b.preset);
                v.push(if eraser { Sub::EraserPreset(k) } else { Sub::Preset(k) });
            }
        }
    }
    v
}

/// Índice do pincel com esta chave.
pub(crate) fn preset_index(env: &Env, key: &str) -> Option<usize> {
    env.ed.brushes.items.iter().position(|b| preset_key(&b.preset) == key)
}

/// Nome da sub-ferramenta na lista.
pub(crate) fn sub_label(env: &Env, s: &Sub) -> String {
    let name_of = |k: &str| preset_index(env, k).and_then(|i| env.ed.brushes.get(i)).map(|p| p.name.clone()).unwrap_or_else(|| k.to_owned());
    match s {
        Sub::Preset(k) | Sub::EraserPreset(k) => name_of(k),
        Sub::PenTip(t) => tr(match t {
            VectorTip::GPen => "tip.g_pen",
            VectorTip::MaruPen => "tip.maru_pen",
            VectorTip::Marker => "tip.marker",
            VectorTip::Pencil => "tip.pencil",
        })
        .to_owned(),
        Sub::PenShape(m) => tr(super::figure::label_key(*m)).to_owned(),
        Sub::EraserRaster(i) => tr(ERASERS[*i as usize % ERASERS.len()].0).to_owned(),
        Sub::EraserTip(EraserTip::Pencil) => tr("eraser.item.pencil").to_owned(),
        Sub::EraserTip(EraserTip::Block) => tr("eraser.item.block").to_owned(),
        Sub::EraserTip(EraserTip::Brush) => tr("eraser.tip.brush").to_owned(),
        Sub::EraserBrush => tr("eraser.brush_item").to_owned(),
        Sub::EraserVector(m) => tr(match m {
            EraseMode::Segment => "eraser.item.segment",
            EraseMode::Intersection => "eraser.item.intersection",
            EraseMode::Whole => "eraser.item.whole",
        })
        .to_owned(),
        Sub::Fill(i) => tr(FILLS[*i as usize % FILLS.len()].0).to_owned(),
        Sub::Select(m) => tr(m.label_key()).to_owned(),
        Sub::Points(m) => tr(super::panels::properties::points_mode_key(*m)).to_owned(),
        Sub::Fix(f) => tr(f.label_key()).to_owned(),
        Sub::Ruler(k) => tr(k.key()).to_owned(),
        Sub::Gradient(g) => super::gradient_tool::pick_label(env, g),
        Sub::Engine(t) => tr(t.title_key()).to_owned(),
    }
}

/// A sub-ferramenta é a que está em uso (com a ferramenta do motor dela ativa).
pub(crate) fn is_current(env: &Env, s: &Sub) -> bool {
    let o = &env.ed.opts;
    if *env.tool != s.engine() {
        return false;
    }
    let preset_is = |k: &str| env.ed.brushes.get(o.brush_preset).is_some_and(|p| preset_key(p) == k);
    match s {
        Sub::Preset(k) => preset_is(k),
        Sub::PenTip(t) => o.pen_tip == *t && o.pen_shape == PenShape::Freehand,
        Sub::PenShape(m) => o.pen_shape == *m,
        Sub::EraserRaster(i) => o.eraser_tip == EraserTip::Brush && !o.eraser_use_brush && (o.eraser_hardness - ERASERS[*i as usize % ERASERS.len()].1).abs() < 1e-3,
        Sub::EraserTip(m) => o.eraser_tip == *m && !o.eraser_use_brush,
        Sub::EraserBrush => o.eraser_use_brush && !preset_is_eraser(env),
        Sub::EraserPreset(k) => o.eraser_use_brush && preset_is(k),
        Sub::EraserVector(m) => o.eraser_mode == *m,
        Sub::Fill(i) => {
            let (_, gap, expand, left) = FILLS[*i as usize % FILLS.len()];
            (o.fill_gap - gap).abs() < 1e-3 && o.fill_expand == expand && o.fill_close_leftovers == left
        }
        Sub::Select(m) => o.select_mode == *m,
        Sub::Points(m) => o.points_mode == *m && o.fix.tool.is_none(),
        Sub::Fix(f) => o.fix.tool == Some(*f),
        Sub::Ruler(k) => env.ed.rulers.new_kind == *k,
        Sub::Gradient(g) => env.ed.grad_tool.pick == *g,
        Sub::Engine(_) => true,
    }
}

/// O pincel em uso é do grupo borracha (a borracha "com o pincel atual" não é ele).
fn preset_is_eraser(env: &Env) -> bool {
    env.ed.brushes.get(env.ed.opts.brush_preset).is_some_and(|p| preset_group(p) == "borracha")
}

// ------------------------------------------------------------------------------------------------ estado

/// Ajuste do usuário num pincel (tamanho, opacidade, cópia editada), lembrado ao trocar de sub-ferramenta.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Tuned {
    pub size: f32,
    pub opacity: f32,
    pub custom: Option<BrushPreset>,
}

#[derive(Default)]
pub(crate) struct ShelfState {
    /// Ferramenta da prateleira em uso (`None` até o primeiro quadro).
    pub slot: Option<Slot>,
    /// Aba aberta no painel de sub-ferramentas por ferramenta.
    pub tab: HashMap<Slot, &'static str>,
    /// Última sub-ferramenta usada em cada ferramenta.
    pub last: HashMap<Slot, Sub>,
    /// Ajustes por pincel (chave do pincel).
    pub tuned: HashMap<String, Tuned>,
}

/// Estado do lado esquerdo das áreas de desenho (um campo só no `Editor`).
#[derive(Default)]
pub(crate) struct DrawUi {
    pub shelf: ShelfState,
    /// Espelho de `prefs.drawing` (o app grava quando muda).
    pub prefs: crate::drawing_prefs::DrawingPrefs,
    /// Painel do catálogo a inserir no layout (um botão pediu um painel que não está nele: a Biblioteca).
    pub insert_panel: Option<crate::layout::PanelKind>,
    pub colors: super::panels::colors::ColorUi,
    pub detail: Option<super::panels::tool_detail::Detail>,
    pub sizes: super::panels::brush_size::SizeUi,
    pub props: super::panels::tool_props::PropsUi,
    /// Renomeando o pincel (índice, nome).
    pub rename: Option<(usize, String)>,
}

/// A aba aberta da ferramenta (a primeira, se nenhuma foi escolhida).
pub(crate) fn current_tab(env: &Env, slot: Slot) -> &'static Tab {
    let tabs = slot.tabs();
    let want = env.ed.draw_ui.shelf.tab.get(&slot).copied();
    want.and_then(|k| tabs.iter().find(|t| t.key == k)).unwrap_or(&tabs[0])
}

/// Sub-ferramenta em uso da ferramenta da prateleira ativa (a que está marcada na lista).
pub(crate) fn current_sub(env: &Env) -> Option<Sub> {
    let slot = env.ed.draw_ui.shelf.slot?;
    slot.tabs().iter().flat_map(|t| subs(env, slot, t)).find(|s| is_current(env, s))
}

/// Primeira sub-ferramenta da ferramenta neste workspace.
fn default_sub(env: &Env, slot: Slot) -> Option<Sub> {
    slot.tabs().iter().find_map(|t| subs(env, slot, t).into_iter().next())
}

/// Guarda o ajuste do pincel em uso (antes de trocar de pincel).
fn remember_tuning(env: &mut Env) {
    let o = &env.ed.opts;
    if let Some(p) = env.ed.brushes.get(o.brush_preset) {
        let t = Tuned { size: o.brush_size, opacity: o.brush_opacity, custom: o.brush_custom.clone() };
        let k = preset_key(p);
        env.ed.draw_ui.shelf.tuned.insert(k, t);
    }
}

/// Põe o pincel `i` em uso com o ajuste lembrado (ou o do preset).
fn use_preset(env: &mut Env, i: usize) {
    let Some(p) = env.ed.brushes.get(i).cloned() else { return };
    if env.ed.opts.brush_preset != i {
        remember_tuning(env);
    }
    let t = env.ed.draw_ui.shelf.tuned.get(&preset_key(&p)).cloned();
    let o = &mut env.ed.opts;
    o.brush_preset = i;
    match t {
        Some(t) => (o.brush_size, o.brush_opacity, o.brush_custom) = (t.size, t.opacity, t.custom),
        None => (o.brush_size, o.brush_opacity, o.brush_custom) = (p.size, p.opacity, None),
    }
}

/// Escolhe a sub-ferramenta `s` da ferramenta `slot` (põe a ferramenta do motor e as opções dela).
pub(crate) fn apply_sub(env: &mut Env, slot: Slot, s: &Sub) {
    match s {
        Sub::Preset(k) => {
            if let Some(i) = preset_index(env, k) {
                use_preset(env, i);
            }
        }
        Sub::PenTip(t) => (env.ed.opts.pen_tip, env.ed.opts.pen_shape) = (*t, PenShape::Freehand),
        Sub::PenShape(m) => env.ed.opts.pen_shape = *m,
        Sub::EraserRaster(i) => {
            let o = &mut env.ed.opts;
            (o.eraser_hardness, o.eraser_tip, o.eraser_use_brush) = (ERASERS[*i as usize % ERASERS.len()].1, EraserTip::Brush, false);
        }
        Sub::EraserTip(m) => (env.ed.opts.eraser_tip, env.ed.opts.eraser_use_brush) = (*m, false),
        Sub::EraserBrush => {
            // apaga com o pincel da última ferramenta de pincel (não com uma borracha da biblioteca)
            if preset_is_eraser(env) {
                if let Some(Sub::Preset(k)) = env.ed.draw_ui.shelf.last.get(&Slot::Brush).cloned() {
                    if let Some(i) = preset_index(env, &k) {
                        use_preset(env, i);
                    }
                }
            }
            (env.ed.opts.eraser_tip, env.ed.opts.eraser_use_brush) = (EraserTip::Brush, true);
        }
        Sub::EraserPreset(k) => {
            if let Some(i) = preset_index(env, k) {
                use_preset(env, i);
            }
            (env.ed.opts.eraser_tip, env.ed.opts.eraser_use_brush) = (EraserTip::Brush, true);
        }
        Sub::EraserVector(m) => env.ed.opts.eraser_mode = *m,
        Sub::Fill(i) => {
            let (_, gap, expand, left) = FILLS[*i as usize % FILLS.len()];
            let o = &mut env.ed.opts;
            (o.fill_gap, o.fill_expand, o.fill_close_leftovers) = (gap, expand, left);
        }
        Sub::Select(m) => env.ed.opts.select_mode = *m,
        Sub::Points(m) => (env.ed.opts.points_mode, env.ed.opts.fix.tool) = (*m, None),
        Sub::Fix(f) => {
            env.ed.opts.fix.tool = Some(*f);
            // o traço escolhido do Editar pontos não vale nas correções (pontos e alças somem do canvas)
            (env.ed.node_stroke, env.ed.node_sel) = (None, None);
        }
        Sub::Ruler(k) => env.ed.rulers.new_kind = *k,
        Sub::Gradient(g) => env.ed.grad_tool.pick = g.clone(),
        Sub::Engine(_) => {}
    }
    *env.tool = s.engine();
    let st = &mut env.ed.draw_ui.shelf;
    st.slot = Some(slot);
    st.last.insert(slot, s.clone());
}

/// Escolhe a ferramenta da prateleira: volta à última sub-ferramenta dela (ou à primeira).
pub(crate) fn activate(env: &mut Env, slot: Slot) {
    if !slot.available() {
        return;
    }
    let all: Vec<Sub> = slot.tabs().iter().flat_map(|t| subs(env, slot, t)).collect();
    let last = env.ed.draw_ui.shelf.last.get(&slot).cloned().filter(|s| all.contains(s));
    if let Some(s) = last.or_else(|| default_sub(env, slot)) {
        apply_sub(env, slot, &s);
    }
}

/// O que identifica a sub-ferramenta em uso para achar a ferramenta da prateleira dona dela.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Owner {
    /// Pincel raster de um grupo.
    Group(&'static str),
    PenTip(VectorTip),
    Shape,
    Wand,
    Other,
}

/// A ferramenta da prateleira dona da ferramenta do motor em uso: a atual, se ela ainda serve; senão a primeira que
/// serve. A ferramenta do motor muda por fora (bridge, atalho do zoom, Shift+M do objeto 3D) e a prateleira acompanha.
pub(crate) fn pick_slot(slots: &[Slot], current: Option<Slot>, tool: ToolId, owner: &Owner) -> Option<Slot> {
    let owns = |s: Slot| {
        s.engines().contains(&tool)
            && match (tool, owner) {
                (ToolId::Brush, Owner::Group(g)) => s.groups().contains(g),
                (ToolId::Pen, Owner::Shape) => s == Slot::Figure,
                (ToolId::Pen, Owner::PenTip(t)) => s != Slot::Figure && ((s == Slot::Pencil) == (*t == VectorTip::Pencil)),
                (ToolId::Lasso, Owner::Wand) => s == Slot::AutoSelect,
                (ToolId::Lasso, _) => s == Slot::Select,
                _ => true,
            }
    };
    if let Some(c) = current.filter(|c| slots.contains(c) && owns(*c)) {
        return Some(c);
    }
    slots
        .iter()
        .copied()
        .find(|s| owns(*s))
        .or_else(|| current.filter(|c| slots.contains(c) && c.engines().contains(&tool)))
        .or_else(|| slots.iter().copied().find(|s| s.engines().contains(&tool)))
}

/// Acompanha a ferramenta do motor (chamado a cada quadro antes de desenhar a prateleira).
pub(crate) fn sync(env: &mut Env) {
    let slots = slots_for(env.ws);
    if slots.is_empty() {
        return;
    }
    let o = &env.ed.opts;
    let owner = match *env.tool {
        ToolId::Brush => env.ed.brushes.get(o.brush_preset).map(|p| Owner::Group(preset_group(p))).unwrap_or(Owner::Other),
        ToolId::Pen if o.pen_shape != PenShape::Freehand => Owner::Shape,
        ToolId::Pen => Owner::PenTip(o.pen_tip),
        ToolId::Lasso if o.select_mode == SelTool::Wand => Owner::Wand,
        _ => Owner::Other,
    };
    let cur = env.ed.draw_ui.shelf.slot;
    env.ed.draw_ui.shelf.slot = pick_slot(&slots, cur, *env.tool, &owner);
}

/// Letras das ferramentas da prateleira (antes dos atalhos gerais, que só veem o que sobrou): a letra escolhe a
/// ferramenta ou passa para a próxima que divide a letra.
pub(crate) fn shortcuts(ctx: &egui::Context, env: &mut Env) {
    if ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) || super::text_tool::editing(env.ed) {
        return;
    }
    let slots = slots_for(env.ws);
    if slots.is_empty() {
        return;
    }
    let mut actions: Vec<Action> = Vec::new();
    for s in &slots {
        if !actions.contains(&s.action()) {
            actions.push(s.action());
        }
    }
    let keys = env.ed.keys.clone();
    let hit = ctx.input_mut(|i| actions.iter().copied().find(|a| keys.get(*a).is_some_and(|c| c.take(i))));
    if let Some(a) = hit {
        sync(env);
        if let Some(s) = cycle(&slots, env.ed.draw_ui.shelf.slot, a) {
            activate(env, s);
        }
    }
}

/// Dica da ferramenta: nome e atalho ("Caneta (P)"); a que não existe no motor diz "em breve".
pub(crate) fn tooltip(env: &Env, slot: Slot) -> String {
    let key = env.ed.keys.get(slot.action()).map(|c| c.label());
    let name = match key {
        Some(k) => format!("{} ({k})", tr(slot.title_key())),
        None => tr(slot.title_key()).to_owned(),
    };
    if !slot.available() {
        return format!("{name}\n{}", tr("shelf.soon"));
    }
    if slot == Slot::Hand {
        return format!("{name}\n{}", tr("tool.hand.tip"));
    }
    name
}

/// A prateleira: só ícones, em três blocos, com a dica de nome e atalho.
pub(crate) fn show(env: &mut Env, ui: &mut Ui) {
    sync(env);
    let slots = slots_for(env.ws);
    let t = env.t;
    let cur = env.ed.draw_ui.shelf.slot;
    let side = 32.0;
    let mut clicked: Option<Slot> = None;
    egui::ScrollArea::vertical().id_salt("shelf_scroll").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            let mut block = None;
            for slot in slots {
                if block.is_some_and(|b| b != slot.block()) {
                    // respiro entre os blocos: uma linha curta
                    let (r, _) = ui.allocate_exact_size(egui::vec2(side, 7.0), Sense::hover());
                    ui.painter().hline(r.x_range().shrink(6.0), r.center().y, Stroke::new(1.0, t.border));
                }
                block = Some(slot.block());
                let enabled = slot.available();
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), if enabled { Sense::click() } else { Sense::hover() });
                env.note(format!("tool:{}", slot.key()), rect);
                let selected = cur == Some(slot);
                let p = ui.painter();
                if selected {
                    p.rect(rect, 5.0, t.accent_dim, Stroke::new(1.0, t.accent), StrokeKind::Inside);
                } else if resp.hovered() && enabled {
                    p.rect_filled(rect, 5.0, t.bg_hover);
                }
                let ink = if !enabled {
                    t.text_dim.gamma_multiply(0.4)
                } else if selected || resp.hovered() {
                    t.text
                } else {
                    t.text_dim
                };
                env.icons.paint(p, slot.icon(), rect.shrink(6.5), ink);
                let tip = tooltip(env, slot);
                if resp.on_hover_text(tip).clicked() {
                    clicked = Some(slot);
                }
            }
        });
    });
    if let Some(s) = clicked {
        activate(env, s);
        // o painel das sub-ferramentas vem para a frente (se estiver numa aba atrás de outro)
        env.ed.want_panel = Some(crate::layout::PanelKind::Brushes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn selection_comes_first_and_drawing_shelves_follow_the_profiles() {
        for ws in WorkspaceId::ALL {
            let slots = slots_for(ws);
            let p = ws.profile();
            if !p.canvas || ws == WorkspaceId::Composicao {
                assert!(slots.is_empty(), "{ws:?} tem prateleira própria ou nenhuma");
                continue;
            }
            assert_eq!(slots.first(), Some(&Slot::Select), "{ws:?}: a Seleção abre a prateleira (decisão do usuário)");
            assert_eq!(slots.get(1), Some(&Slot::AutoSelect), "{ws:?}");
            let set: HashSet<_> = slots.iter().collect();
            assert_eq!(set.len(), slots.len(), "{ws:?} repete ferramenta");
            // toda ferramenta do motor da prateleira antiga continua alcançável
            for t in p.tools {
                assert!(slots.iter().any(|s| s.engines().contains(t)), "{ws:?}: {t:?} sumiu da prateleira");
            }
            // a ordem segue a de ALL (blocos contíguos)
            let idx: Vec<usize> = slots.iter().map(|s| Slot::ALL.iter().position(|a| a == s).unwrap()).collect();
            assert!(idx.windows(2).all(|w| w[0] < w[1]), "{ws:?}: {slots:?}");
        }
        // a Pintura não desenha linha; o Cenário não tem caneta
        let pint = slots_for(WorkspaceId::Pintura);
        assert!(!pint.contains(&Slot::Pen) && !pint.contains(&Slot::Pencil) && !pint.contains(&Slot::CorrectLine) && pint.contains(&Slot::Fill));
        assert!(pint.contains(&Slot::Blend) && pint.contains(&Slot::Airbrush) && pint.contains(&Slot::Gradient));
        assert!(!slots_for(WorkspaceId::Cenario).contains(&Slot::Pen) && slots_for(WorkspaceId::Cenario).contains(&Slot::Decoration));
        let anim = slots_for(WorkspaceId::Animacao);
        for s in [Slot::Pen, Slot::Pencil, Slot::Brush, Slot::Eraser, Slot::Fill, Slot::Figure, Slot::CorrectLine, Slot::Text] {
            assert!(anim.contains(&s), "Animação sem {s:?}");
        }
        assert!(!slots_for(WorkspaceId::Layout).contains(&Slot::Gradient), "sem balde, sem degradê");
    }

    #[test]
    fn the_same_letter_cycles_between_the_tools_that_share_it() {
        let slots = slots_for(WorkspaceId::Animacao);
        // P: Caneta → Lápis → Caneta
        assert_eq!(cycle(&slots, Some(Slot::Brush), Action::ToolPen), Some(Slot::Pen));
        assert_eq!(cycle(&slots, Some(Slot::Pen), Action::ToolPen), Some(Slot::Pencil));
        assert_eq!(cycle(&slots, Some(Slot::Pencil), Action::ToolPen), Some(Slot::Pen));
        // B: Pincel → Aerógrafo → Decoração → Pincel
        assert_eq!(cycle(&slots, None, Action::ToolBrush), Some(Slot::Brush));
        assert_eq!(cycle(&slots, Some(Slot::Brush), Action::ToolBrush), Some(Slot::Airbrush));
        assert_eq!(cycle(&slots, Some(Slot::Airbrush), Action::ToolBrush), Some(Slot::Decoration));
        assert_eq!(cycle(&slots, Some(Slot::Decoration), Action::ToolBrush), Some(Slot::Brush));
        // G: Balde → Degradê → Balde (S25: o degradê existe no motor)
        assert_eq!(cycle(&slots, Some(Slot::Fill), Action::ToolFill), Some(Slot::Gradient));
        assert_eq!(cycle(&slots, Some(Slot::Gradient), Action::ToolFill), Some(Slot::Fill));
        assert_eq!(cycle(&slots_for(WorkspaceId::Pintura), Some(Slot::Fill), Action::ToolFill), Some(Slot::Gradient));
        assert_eq!(pick_slot(&slots, Some(Slot::Fill), ToolId::Gradient, &Owner::Other), Some(Slot::Gradient));
        assert!(Slot::ALL.iter().all(|s| s.available()), "nenhuma ferramenta diz mais \"em breve\"");
        // U: Figura (a Animação não tem régua)
        assert_eq!(cycle(&slots, Some(Slot::Figure), Action::ToolRuler), Some(Slot::Figure));
        let lay = slots_for(WorkspaceId::Layout);
        assert_eq!(cycle(&lay, Some(Slot::Figure), Action::ToolRuler), Some(Slot::Ruler));
        assert_eq!(cycle(&lay, Some(Slot::Ruler), Action::ToolRuler), Some(Slot::Figure));
        // letra sem ferramenta neste workspace: nada
        assert_eq!(cycle(&slots_for(WorkspaceId::Pintura), None, Action::ToolPoints), None);
        // na Pintura o B passa só pelo que existe lá (sem Decoração)
        let p = slots_for(WorkspaceId::Pintura);
        assert_eq!(cycle(&p, Some(Slot::Airbrush), Action::ToolBrush), Some(Slot::Brush));
    }

    #[test]
    fn default_shortcuts_follow_the_brief() {
        let k = crate::keymap::Keymap::default();
        let letter = |s: Slot| k.get(s.action()).map(|c| c.label()).unwrap_or_default();
        for (s, l) in [
            (Slot::Select, "M"),
            (Slot::AutoSelect, "W"),
            (Slot::MoveLayer, "K"),
            (Slot::Hand, "H"),
            (Slot::Eyedropper, "I"),
            (Slot::Pen, "P"),
            (Slot::Pencil, "P"),
            (Slot::Brush, "B"),
            (Slot::Airbrush, "B"),
            (Slot::Decoration, "B"),
            (Slot::Eraser, "E"),
            (Slot::Blend, "J"),
            (Slot::Fill, "G"),
            (Slot::Gradient, "G"),
            (Slot::Figure, "U"),
            (Slot::Ruler, "U"),
            (Slot::Text, "T"),
            (Slot::CorrectLine, "Y"),
        ] {
            assert_eq!(letter(s), l, "{s:?}");
        }
    }

    #[test]
    fn slot_follows_the_engine_tool_changed_from_outside() {
        let slots = slots_for(WorkspaceId::Animacao);
        // pincel de aerógrafo escolhido pela bridge com a Caneta ativa: a prateleira vai para o Aerógrafo
        assert_eq!(pick_slot(&slots, Some(Slot::Pen), ToolId::Brush, &Owner::Group("aerografo")), Some(Slot::Airbrush));
        // pincel do grupo caneta: a Caneta continua
        assert_eq!(pick_slot(&slots, Some(Slot::Pen), ToolId::Brush, &Owner::Group("caneta")), Some(Slot::Pen));
        assert_eq!(pick_slot(&slots, Some(Slot::Brush), ToolId::Brush, &Owner::Group("marcador")), Some(Slot::Pen));
        // pena de lápis vetorial é do Lápis; forma é da Figura; varinha é da Seleção automática
        assert_eq!(pick_slot(&slots, Some(Slot::Pen), ToolId::Pen, &Owner::PenTip(VectorTip::Pencil)), Some(Slot::Pencil));
        assert_eq!(pick_slot(&slots, Some(Slot::Pen), ToolId::Pen, &Owner::Shape), Some(Slot::Figure));
        assert_eq!(pick_slot(&slots, Some(Slot::Figure), ToolId::Pen, &Owner::PenTip(VectorTip::GPen)), Some(Slot::Pen));
        assert_eq!(pick_slot(&slots, Some(Slot::Select), ToolId::Lasso, &Owner::Wand), Some(Slot::AutoSelect));
        assert_eq!(pick_slot(&slots, Some(Slot::AutoSelect), ToolId::Lasso, &Owner::Other), Some(Slot::Select));
        // zoom (Z) e objeto 3D (Shift+M) caem nas ferramentas que os agrupam
        assert_eq!(pick_slot(&slots, Some(Slot::Brush), ToolId::Zoom, &Owner::Other), Some(Slot::Hand));
        assert_eq!(pick_slot(&slots, None, ToolId::Model3d, &Owner::Other), Some(Slot::MoveLayer));
        // pincel de grupo que este workspace não tem (decoração na Pintura): fica numa ferramenta de pincel
        let p = slots_for(WorkspaceId::Pintura);
        assert_eq!(pick_slot(&p, Some(Slot::Brush), ToolId::Brush, &Owner::Group("decoracao")), Some(Slot::Brush));
    }

    #[test]
    fn factory_presets_fall_into_groups() {
        let lib = BrushPreset::defaults();
        let group = |name: &str| preset_group(lib.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{name}")));
        assert_eq!(group("Lápis de rascunho"), "lapis");
        assert_eq!(group("Redondo duro"), "pincel");
        assert_eq!(group("Aerógrafo"), "aerografo");
        assert_eq!(group("Borrador"), "mistura");
        assert_eq!(group("Folhagem"), "decoracao");
        for p in &lib {
            assert!(GROUPS.contains(&preset_group(p)), "{}", p.name);
        }
        let mut e = BrushPreset::round("x", 10.0, 0.5);
        e.mode = BrushMode::Erase;
        e.category = "meus".into();
        assert_eq!(preset_group(&e), "borracha");
        // toda aba com grupo usa um grupo conhecido; toda ferramenta de grupo tem aba
        for s in Slot::ALL {
            for t in s.tabs() {
                assert!(t.group.is_none_or(|g| GROUPS.contains(&g)), "{s:?}");
            }
        }
        for g in GROUPS {
            assert!(Slot::ALL.iter().any(|s| s.groups().contains(&g)), "grupo {g} sem ferramenta");
        }
    }

    #[test]
    fn subtools_have_the_right_engine_and_size_flag() {
        assert!(Sub::Preset("x".into()).has_size() && Sub::PenTip(VectorTip::GPen).has_size() && Sub::EraserRaster(0).has_size());
        for s in [Sub::Fill(0), Sub::Select(SelTool::Rect), Sub::Engine(ToolId::Hand), Sub::Engine(ToolId::Text), Sub::Points(PointsMode::Move)] {
            assert!(!s.has_size(), "{s:?} não tem tamanho: a paleta fica desabilitada");
        }
        // Corrigir linha: os pincéis de correção têm tamanho; puxar, redesenhar e poeira não
        use super::super::line_fix::FixTool;
        assert!(Sub::Fix(FixTool::Simplify).has_size() && Sub::Fix(FixTool::Width).has_size() && !Sub::Fix(FixTool::Pinch).has_size() && !Sub::Fix(FixTool::Dust).has_size());
        assert_eq!(fixed_subs(Slot::CorrectLine, "points").len(), 5 + FixTool::LINE.len());
        assert_eq!(fixed_subs(Slot::CorrectLine, "dust"), vec![Sub::Fix(FixTool::Dust), Sub::Fix(FixTool::SelectDust)]);
        for slot in Slot::ALL {
            for t in slot.tabs() {
                for s in fixed_subs(slot, t.key) {
                    assert!(slot.engines().contains(&s.engine()), "{slot:?}/{}: {s:?}", t.key);
                }
            }
            if slot.available() && slot.groups().is_empty() {
                assert!(slot.tabs().iter().any(|t| !fixed_subs(slot, t.key).is_empty()), "{slot:?} sem sub-ferramenta");
            }
        }
        // chaves das sub-ferramentas não colidem
        let keys: Vec<String> = Slot::ALL.iter().flat_map(|s| s.tabs().iter().flat_map(move |t| fixed_subs(*s, t.key))).map(|s| s.key()).collect();
        let set: HashSet<_> = keys.iter().collect();
        assert_eq!(set.len(), keys.len(), "{keys:?}");
        // as chaves da prateleira voltam (retângulos `tool:<chave>` da bridge)
        for s in Slot::ALL {
            assert_eq!(Slot::from_key(s.key()), Some(s));
        }
    }

    #[test]
    fn texts_exist_in_both_languages() {
        use crate::i18n::{Lang, tr_in};
        let mut keys: Vec<&str> = vec!["shelf.soon"];
        for s in Slot::ALL {
            keys.push(s.title_key());
            keys.extend(s.tabs().iter().map(|t| t.title_key));
        }
        for k in keys {
            for l in [Lang::Pt, Lang::En] {
                assert_ne!(tr_in(l, k), k, "{k} sem texto em {l:?}");
            }
        }
    }
}
