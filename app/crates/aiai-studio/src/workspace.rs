//! Os 14 workspaces do AnimeIAI (ordem de produção: do roteiro à entrega), os grupos da barra e o perfil de cada um
//! (ferramentas da prateleira, ferramenta padrão, canvas).

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
    Cenario,
    Animacao,
    Pintura,
    Composicao,
    /// Acervo de materiais, modelos 3D, paletas e texturas. Era "Cor" (a grade do corte foi para a Composição):
    /// `prefs.json` antigo com `"cor"` abre aqui.
    #[serde(alias = "cor")]
    Biblioteca,
    /// Cena 3D simples e manequim como referência de desenho (esqueleto nesta sprint).
    Modelagem,
    Audio,
    Edicao,
    Entrega,
    Producao,
}

/// Grupos da barra de workspaces (separadores discretos entre eles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkspaceGroup {
    PreProducao,
    Arte,
    Finalizacao,
    Recursos,
    SomMontagem,
    Gestao,
}

impl WorkspaceGroup {
    pub fn title_key(self) -> &'static str {
        match self {
            WorkspaceGroup::PreProducao => "workspace.group.preprod",
            WorkspaceGroup::Arte => "workspace.group.art",
            WorkspaceGroup::Finalizacao => "workspace.group.finishing",
            WorkspaceGroup::Recursos => "workspace.group.resources",
            WorkspaceGroup::SomMontagem => "workspace.group.sound_edit",
            WorkspaceGroup::Gestao => "workspace.group.management",
        }
    }
}

/// O que um workspace oferece: as ferramentas da prateleira (na ordem em que aparecem), a ferramenta escolhida ao
/// entrar quando a atual não vale ali, se há canvas e se as teclas do editor (quadros, reprodução, ferramentas) valem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceProfile {
    pub tools: &'static [ToolId],
    pub default_tool: ToolId,
    pub canvas: bool,
    pub editor_keys: bool,
}

impl WorkspaceProfile {
    /// A ferramenta existe na prateleira deste workspace.
    pub fn allows(&self, tool: ToolId) -> bool {
        self.tools.contains(&tool)
    }
}

/// Sem canvas: nenhuma ferramenta de desenho.
const NO_TOOLS: &[ToolId] = &[];

impl WorkspaceId {
    /// Ordem canônica da barra de workspaces.
    pub const ALL: [WorkspaceId; 14] = [
        WorkspaceId::Roteiro,
        WorkspaceId::Personagens,
        WorkspaceId::Storyboard,
        WorkspaceId::Layout,
        WorkspaceId::Cenario,
        WorkspaceId::Animacao,
        WorkspaceId::Pintura,
        WorkspaceId::Composicao,
        WorkspaceId::Biblioteca,
        WorkspaceId::Modelagem,
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
            WorkspaceId::Cenario => "cenario",
            WorkspaceId::Animacao => "animacao",
            WorkspaceId::Pintura => "pintura",
            WorkspaceId::Composicao => "composicao",
            WorkspaceId::Biblioteca => "biblioteca",
            WorkspaceId::Modelagem => "modelagem",
            WorkspaceId::Audio => "audio",
            WorkspaceId::Edicao => "edicao",
            WorkspaceId::Entrega => "entrega",
            WorkspaceId::Producao => "producao",
        }
    }

    /// Aceita também as chaves antigas (`cor` = Biblioteca).
    pub fn from_key(key: &str) -> Option<WorkspaceId> {
        if key == "cor" {
            return Some(WorkspaceId::Biblioteca);
        }
        Self::ALL.into_iter().find(|w| w.key() == key)
    }

    /// Chave i18n do nome.
    pub fn title_key(self) -> &'static str {
        match self {
            WorkspaceId::Roteiro => "workspace.roteiro",
            WorkspaceId::Personagens => "workspace.personagens",
            WorkspaceId::Storyboard => "workspace.storyboard",
            WorkspaceId::Layout => "workspace.layout",
            WorkspaceId::Cenario => "workspace.cenario",
            WorkspaceId::Animacao => "workspace.animacao",
            WorkspaceId::Pintura => "workspace.pintura",
            WorkspaceId::Composicao => "workspace.composicao",
            WorkspaceId::Biblioteca => "workspace.biblioteca",
            WorkspaceId::Modelagem => "workspace.modelagem",
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
            WorkspaceId::Cenario => IconId::WsCenario,
            WorkspaceId::Animacao => IconId::WsAnimacao,
            WorkspaceId::Pintura => IconId::WsPintura,
            WorkspaceId::Composicao => IconId::WsComposicao,
            WorkspaceId::Biblioteca => IconId::WsBiblioteca,
            WorkspaceId::Modelagem => IconId::WsModelagem,
            WorkspaceId::Audio => IconId::WsAudio,
            WorkspaceId::Edicao => IconId::WsEdicao,
            WorkspaceId::Entrega => IconId::WsEntrega,
            WorkspaceId::Producao => IconId::WsProducao,
        }
    }

    /// Grupo da barra: pré-produção, arte, finalização, recursos, som e montagem, gestão.
    pub fn group(self) -> WorkspaceGroup {
        use WorkspaceId as W;
        match self {
            W::Roteiro | W::Personagens | W::Storyboard => WorkspaceGroup::PreProducao,
            W::Layout | W::Cenario | W::Animacao | W::Pintura => WorkspaceGroup::Arte,
            W::Composicao => WorkspaceGroup::Finalizacao,
            W::Biblioteca | W::Modelagem => WorkspaceGroup::Recursos,
            W::Audio | W::Edicao | W::Entrega => WorkspaceGroup::SomMontagem,
            W::Producao => WorkspaceGroup::Gestao,
        }
    }

    /// Perfil do workspace (ver `WorkspaceProfile`). Cada etapa de desenho tem a sua prateleira:
    /// - Layout: linha e construção (caneta, pontos), sem balde;
    /// - Cenário: pintura de fundo (pincel, balde), sem caneta vetorial nem pontos;
    /// - Animação: o que o animador usa no genga e no douga, e o balde para conferir o fechamento das linhas;
    /// - Pintura: balde primeiro, pincel e borracha para acertos; sem caneta nem pontos (não se desenha linha aqui);
    /// - Personagens: desenha nos materiais da pasta do personagem (caneta, pincel, balde para conferir o fechamento);
    /// - Composição: é um viewer: mover, conta-gotas, mão e zoom.
    pub fn profile(self) -> WorkspaceProfile {
        use ToolId as T;
        use WorkspaceId as W;
        match self {
            W::Layout => WorkspaceProfile {
                tools: &[T::Move, T::Pen, T::Brush, T::Points, T::Eraser, T::Lasso, T::Text, T::Eyedropper, T::Hand, T::Zoom],
                default_tool: T::Pen,
                canvas: true,
                editor_keys: true,
            },
            W::Cenario => WorkspaceProfile {
                tools: &[T::Move, T::Brush, T::Eraser, T::Fill, T::Lasso, T::Text, T::Eyedropper, T::Hand, T::Zoom],
                default_tool: T::Brush,
                canvas: true,
                editor_keys: true,
            },
            W::Animacao => WorkspaceProfile {
                tools: &[T::Move, T::Pen, T::Brush, T::Points, T::Eraser, T::Fill, T::Lasso, T::Text, T::Eyedropper, T::Hand, T::Zoom],
                default_tool: T::Pen,
                canvas: true,
                editor_keys: true,
            },
            W::Pintura => WorkspaceProfile {
                tools: &[T::Fill, T::Brush, T::Eraser, T::Lasso, T::Move, T::Eyedropper, T::Hand, T::Zoom],
                default_tool: T::Fill,
                canvas: true,
                editor_keys: true,
            },
            // pasta do personagem: desenha-se nos materiais (folha de modelo, expressões, boca...): linha, cor e conferência
            W::Personagens => WorkspaceProfile {
                tools: &[T::Move, T::Pen, T::Brush, T::Eraser, T::Fill, T::Lasso, T::Text, T::Eyedropper, T::Hand, T::Zoom],
                default_tool: T::Pen,
                canvas: true,
                editor_keys: true,
            },
            W::Composicao => WorkspaceProfile { tools: &[T::Move, T::Text, T::Eyedropper, T::Hand, T::Zoom], default_tool: T::Move, canvas: true, editor_keys: true },
            // sem canvas, mas com linha do tempo: quadros e reprodução pelas teclas do editor
            W::Storyboard | W::Audio | W::Edicao => WorkspaceProfile { tools: NO_TOOLS, default_tool: T::Move, canvas: false, editor_keys: true },
            W::Roteiro | W::Biblioteca | W::Modelagem | W::Entrega | W::Producao => {
                WorkspaceProfile { tools: NO_TOOLS, default_tool: T::Move, canvas: false, editor_keys: false }
            }
        }
    }
}

// Ferramentas básicas da prateleira (só seleção nesta sprint; o comportamento entra na S03).
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
    /// Texto: clique cria texto de ponto, arrasto cria caixa; edita no canvas (camada de texto editável).
    Text,
}

impl ToolId {
    pub const ALL: [ToolId; 11] = [
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
        ToolId::Text,
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
            ToolId::Text => "tool.text",
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
            ToolId::Text => IconId::ToolText,
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
        assert_eq!(seen.len(), 14);
    }

    #[test]
    fn old_color_key_opens_the_library() {
        assert_eq!(WorkspaceId::from_key("cor"), Some(WorkspaceId::Biblioteca));
        assert_eq!(serde_json::from_str::<WorkspaceId>("\"cor\"").unwrap(), WorkspaceId::Biblioteca);
        assert_eq!(serde_json::from_str::<WorkspaceId>("\"biblioteca\"").unwrap(), WorkspaceId::Biblioteca);
    }

    #[test]
    fn order_and_groups_follow_the_pipeline() {
        let keys: Vec<&str> = WorkspaceId::ALL.iter().map(|w| w.key()).collect();
        assert_eq!(
            keys,
            ["roteiro", "personagens", "storyboard", "layout", "cenario", "animacao", "pintura", "composicao", "biblioteca", "modelagem", "audio", "edicao", "entrega", "producao"]
        );
        // cada grupo é um trecho contíguo da barra
        let mut seen: Vec<WorkspaceGroup> = Vec::new();
        for w in WorkspaceId::ALL {
            if seen.last() != Some(&w.group()) {
                assert!(!seen.contains(&w.group()), "{w:?} parte o grupo {:?}", w.group());
                seen.push(w.group());
            }
        }
        assert_eq!(seen.len(), 6);
    }

    #[test]
    fn profiles_give_each_drawing_stage_its_own_shelf() {
        for w in WorkspaceId::ALL {
            let p = w.profile();
            assert_eq!(p.canvas, !p.tools.is_empty(), "{w:?}: canvas sem ferramentas ou ferramentas sem canvas");
            if p.canvas {
                assert!(p.allows(p.default_tool), "{w:?}: a ferramenta padrão não está na prateleira");
                assert!(p.editor_keys, "{w:?}");
                let set: std::collections::HashSet<_> = p.tools.iter().collect();
                assert_eq!(set.len(), p.tools.len(), "{w:?} repete ferramenta");
            }
        }
        // sem pintura onde não se pinta
        for w in [WorkspaceId::Composicao, WorkspaceId::Audio, WorkspaceId::Edicao, WorkspaceId::Entrega, WorkspaceId::Producao] {
            for t in [ToolId::Brush, ToolId::Pen, ToolId::Points, ToolId::Eraser, ToolId::Fill] {
                assert!(!w.profile().allows(t), "{w:?} mostra {t:?}");
            }
        }
        assert!(!WorkspaceId::Pintura.profile().allows(ToolId::Pen) && WorkspaceId::Pintura.profile().default_tool == ToolId::Fill);
        assert!(!WorkspaceId::Layout.profile().allows(ToolId::Fill));
        // as prateleiras dos workspaces de desenho não são iguais entre si
        let shelves: Vec<&[ToolId]> = [WorkspaceId::Layout, WorkspaceId::Cenario, WorkspaceId::Animacao, WorkspaceId::Pintura, WorkspaceId::Composicao].iter().map(|w| w.profile().tools).collect();
        for (i, a) in shelves.iter().enumerate() {
            for b in &shelves[i + 1..] {
                assert_ne!(a, b);
            }
        }
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
