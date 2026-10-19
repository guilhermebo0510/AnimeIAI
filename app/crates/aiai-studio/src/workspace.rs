//! Os 13 workspaces do AnimeIAI (ordem de produção: do roteiro à entrega).

use crate::icons::IconId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceId {
    #[default]
    Roteiro,
    Personagens,
    Storyboard,
    Layout,
    Animacao,
    Pintura,
    Cenario,
    Composicao,
    Cor,
    Audio,
    Edicao,
    Entrega,
    Producao,
}

impl WorkspaceId {
    /// Ordem canônica da barra de workspaces.
    pub const ALL: [WorkspaceId; 13] = [
        WorkspaceId::Roteiro,
        WorkspaceId::Personagens,
        WorkspaceId::Storyboard,
        WorkspaceId::Layout,
        WorkspaceId::Animacao,
        WorkspaceId::Pintura,
        WorkspaceId::Cenario,
        WorkspaceId::Composicao,
        WorkspaceId::Cor,
        WorkspaceId::Audio,
        WorkspaceId::Edicao,
        WorkspaceId::Entrega,
        WorkspaceId::Producao,
    ];

    /// Identificador estável (CLI, preferências): `roteiro`, `animacao`...
    pub fn key(self) -> &'static str {
        match self {
            WorkspaceId::Roteiro => "roteiro",
            WorkspaceId::Personagens => "personagens",
            WorkspaceId::Storyboard => "storyboard",
            WorkspaceId::Layout => "layout",
            WorkspaceId::Animacao => "animacao",
            WorkspaceId::Pintura => "pintura",
            WorkspaceId::Cenario => "cenario",
            WorkspaceId::Composicao => "composicao",
            WorkspaceId::Cor => "cor",
            WorkspaceId::Audio => "audio",
            WorkspaceId::Edicao => "edicao",
            WorkspaceId::Entrega => "entrega",
            WorkspaceId::Producao => "producao",
        }
    }

    pub fn from_key(key: &str) -> Option<WorkspaceId> {
        Self::ALL.into_iter().find(|w| w.key() == key)
    }

    /// Chave i18n do nome.
    pub fn title_key(self) -> &'static str {
        match self {
            WorkspaceId::Roteiro => "workspace.roteiro",
            WorkspaceId::Personagens => "workspace.personagens",
            WorkspaceId::Storyboard => "workspace.storyboard",
            WorkspaceId::Layout => "workspace.layout",
            WorkspaceId::Animacao => "workspace.animacao",
            WorkspaceId::Pintura => "workspace.pintura",
            WorkspaceId::Cenario => "workspace.cenario",
            WorkspaceId::Composicao => "workspace.composicao",
            WorkspaceId::Cor => "workspace.cor",
            WorkspaceId::Audio => "workspace.audio",
            WorkspaceId::Edicao => "workspace.edicao",
            WorkspaceId::Entrega => "workspace.entrega",
            WorkspaceId::Producao => "workspace.producao",
        }
    }

    pub fn icon(self) -> IconId {
        match self {
            WorkspaceId::Roteiro => IconId::WsRoteiro,
            WorkspaceId::Personagens => IconId::WsPersonagens,
            WorkspaceId::Storyboard => IconId::WsStoryboard,
            WorkspaceId::Layout => IconId::WsLayout,
            WorkspaceId::Animacao => IconId::WsAnimacao,
            WorkspaceId::Pintura => IconId::WsPintura,
            WorkspaceId::Cenario => IconId::WsCenario,
            WorkspaceId::Composicao => IconId::WsComposicao,
            WorkspaceId::Cor => IconId::WsCor,
            WorkspaceId::Audio => IconId::WsAudio,
            WorkspaceId::Edicao => IconId::WsEdicao,
            WorkspaceId::Entrega => IconId::WsEntrega,
            WorkspaceId::Producao => IconId::WsProducao,
        }
    }

    /// Workspaces que desenham no canvas (ganham a prateleira de ferramentas).
    pub fn has_canvas(self) -> bool {
        matches!(self, WorkspaceId::Layout | WorkspaceId::Animacao | WorkspaceId::Pintura | WorkspaceId::Cenario | WorkspaceId::Composicao)
    }
}

/// Ferramentas básicas da prateleira (só seleção nesta sprint; o comportamento entra na S03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ToolId {
    Move,
    #[default]
    Brush,
    Pen,
    /// Editar pontos de traço vetorial (mover ponto, largura, inserir, apagar, simplificar).
    Points,
    Eraser,
    Fill,
    Lasso,
    Eyedropper,
    Hand,
    Zoom,
}

impl ToolId {
    pub const ALL: [ToolId; 10] = [
        ToolId::Move,
        ToolId::Brush,
        ToolId::Pen,
        ToolId::Points,
        ToolId::Eraser,
        ToolId::Fill,
        ToolId::Lasso,
        ToolId::Eyedropper,
        ToolId::Hand,
        ToolId::Zoom,
    ];

    /// Identificador estável (bridge/UI_ACTION): `move`, `brush`, `pen`...
    pub fn key(self) -> &'static str {
        self.title_key().split('.').nth(1).unwrap_or("")
    }

    pub fn from_key(key: &str) -> Option<ToolId> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }

    pub fn title_key(self) -> &'static str {
        match self {
            ToolId::Move => "tool.move",
            ToolId::Brush => "tool.brush",
            ToolId::Pen => "tool.pen",
            ToolId::Points => "tool.points",
            ToolId::Eraser => "tool.eraser",
            ToolId::Fill => "tool.fill",
            ToolId::Lasso => "tool.lasso",
            ToolId::Eyedropper => "tool.eyedropper",
            ToolId::Hand => "tool.hand",
            ToolId::Zoom => "tool.zoom",
        }
    }

    pub fn icon(self) -> IconId {
        match self {
            ToolId::Move => IconId::ToolMove,
            ToolId::Brush => IconId::ToolBrush,
            ToolId::Pen => IconId::ToolPen,
            ToolId::Points => IconId::ToolPoints,
            ToolId::Eraser => IconId::ToolEraser,
            ToolId::Fill => IconId::ToolFill,
            ToolId::Lasso => IconId::ToolLasso,
            ToolId::Eyedropper => IconId::ToolEyedropper,
            ToolId::Hand => IconId::ToolHand,
            ToolId::Zoom => IconId::ToolZoom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for w in WorkspaceId::ALL {
            assert_eq!(WorkspaceId::from_key(w.key()), Some(w));
            assert!(seen.insert(w.key()));
        }
        assert_eq!(seen.len(), 13);
    }

    #[test]
    fn tool_keys_round_trip() {
        for t in ToolId::ALL {
            assert_eq!(ToolId::from_key(t.key()), Some(t));
        }
        assert_eq!(ToolId::from_key("pen"), Some(ToolId::Pen));
        assert_eq!(ToolId::from_key("tool.pen"), None);
    }

    #[test]
    fn serde_uses_the_same_keys() {
        for w in WorkspaceId::ALL {
            let j = serde_json::to_string(&w).unwrap();
            assert_eq!(j, format!("\"{}\"", w.key()));
        }
    }
}
