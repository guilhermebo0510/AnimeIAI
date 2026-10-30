//! Painéis, o catálogo deles (`PanelMeta`) e os layouts de fábrica de cada workspace (`egui_tiles`). A árvore em uso vira
//! `SavedNode` (painéis pela chave de texto, nunca pelo índice do enum) para ser guardada em `layouts.json`
//! (`layout_store`) e volta por `tree_from_saved`, que descarta chaves desconhecidas.

use crate::workspace::WorkspaceId;
use egui_tiles::{Container, Linear, LinearDir, Tabs, Tile, TileId, Tiles, Tree};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelKind {
    Canvas,
    Layers,
    Properties,
    Palette,
    Navigator,
    Timeline,
    Script,
    Outline,
    Characters,
    Sheet,
    Shots,
    Board,
    Preview,
    Media,
    /// Edição: monitor de origem (corte, som ou imagem aberto da Mídia; marcas I/O, inserir e sobrescrever).
    SourceMonitor,
    /// Edição: transições (lista com prévia; arrastar até um ponto de corte).
    Transitions,
    /// Edição: efeitos do clipe selecionado (opacidade, velocidade, filtros, rack de áudio, chaves).
    ClipEffects,
    /// Edição: legendas (várias trilhas, estilo, cps, gerar das falas, importar/exportar SRT/VTT).
    Captions,
    Mixer,
    /// Áudio: editor de forma de onda/espectrograma de um clipe (edição não destrutiva com histórico).
    Waveform,
    /// Áudio: rack de efeitos (até 16 posições) com prévia em tempo real e aplicar.
    EffectsRack,
    /// Áudio: medidores de loudness (LUFS momentâneo/curto/integrado, LRA, true peak) com alvos.
    Meters,
    /// Áudio: multipista do corte (cabeçalho de faixa, clipes com forma de onda, fades, rótulos).
    Multitrack,
    /// Áudio: voz sintética (motor, hardware, controles por fala, takes).
    Tts,
    Presets,
    Queue,
    Tasks,
    Episodes,
    /// Produção: gráficos (barras por etapa, rosca, matriz cena × etapa, burndown, fluxo cumulativo, vazão, tempos, retake).
    ProdCharts,
    /// Produção: gargalos (fila, vazão e espera por etapa, travados, atrasados; cada achado filtra a Lista).
    ProdBottleneck,
    /// Produção: análise por IA opcional (pacote de dados para o agente e notas com carimbo de hora).
    ProdAi,
    Project,
    /// Sub View: imagens de referência do disco com conta-gotas.
    SubView,
    /// Grupo de ferramentas (S25): abas de grupo e a lista das sub-ferramentas da ferramenta da prateleira.
    Brushes,
    /// Propriedades da ferramenta (S25): prévia do traço e os parâmetros com o olho aceso.
    ToolProps,
    /// Tamanho do pincel (S25): tamanhos prontos em bolinhas.
    BrushSize,
    /// Biblioteca de recursos: árvore de tipos e etiquetas, busca e grade de miniaturas (modelos 3D, materiais, texturas, paletas, poses, pincéis).
    Library,
    /// Prévia do recurso da Biblioteca: turntable 3D, teste de repetição 3×3, amostras da paleta; ações e metadados.
    LibraryPreview,
    /// Modelagem: viewport 3D (renderer cel com contorno, modos Objeto/Edição, operadores, navegação).
    Modeling,
    /// Modelagem: objetos do documento (adicionar primitivas, abrir, salvar, enviar ao Layout).
    ModelOutliner,
    /// Modelagem: propriedades do objeto ativo (transformação, cor toon, sombreamento, malha).
    ModelProps,
    /// Modelagem: pilha de modificadores (espelho, subdivisão, array).
    ModelModifiers,
    /// Editor de UV da Modelagem (ilhas 2D, costuras, desdobrar, empacotar).
    ModelUv,
    /// Texto: fonte, tamanho, cor, alinhamento, contorno e sombra da camada de texto.
    Text,
    /// Mesa de luz (papel cebola): desenhos vizinhos por baixo do atual no canvas.
    LightTable,
    /// Copiloto de traço: caixas, prompt, quanto mudar e aceitar (entra como coluna própria à direita quando o Copiloto liga).
    Copilot,
    /// Cena do corte: texto e falas do roteiro, painel do storyboard e tempo (só leitura).
    SceneInfo,
    /// Referências do personagem: folhas de modelo e paleta de settei dos personagens do corte.
    CharRef,
    /// Réguas: as réguas da camada de guia (perspectiva 1/2/3 pontos, paralelas, radial, simetria, grade) e as propriedades.
    Rulers,
    /// Personagens: a pasta do personagem (materiais por tipo, criar a partir de modelos, abrir para desenhar).
    CharFolder,
    /// Composição: linha do tempo estilo After Effects (camadas, transformação e efeitos com cronômetro e chaves).
    CompTimeline,
    /// Composição: efeitos e predefinições (árvore por categoria, busca, favoritos, miniaturas, predefinições do usuário).
    EffectsBrowser,
    /// Composição: controles de efeito da camada selecionada (parâmetros, ordem, ligar/desligar, cronômetros).
    EffectControls,
    /// Entrega: destinos (pasta, modelo de nome, versão automática, cópias, pacote de entrega).
    Destinations,
    /// Entrega: biblioteca do que já foi exportado (miniaturas, metadados, abrir, exportar de novo, lixeira).
    Deliverables,
    /// Entrega: pôster do entregável (quadro e título sobre a imagem).
    Poster,
    /// Entrega: verificação final antes de exportar (semáforo com link para corrigir).
    Preflight,
    /// Histórico (S25): os passos de desfazer com nomes legíveis; clicar leva o documento até o passo.
    History,
    /// Propriedades da camada (S25): a camada ativa, os filtros (efeitos) dela, papel, profundidade e deslocamento.
    LayerProps,
}

/// Categoria do painel (agrupa o Exibir > Painéis e decide junto de quem um painel novo entra como aba).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelCategory {
    General,
    PreProduction,
    Character,
    Drawing,
    Color,
    Reference,
    Time,
    SoundEdit,
    Delivery,
    Management,
    Resources,
}

impl PanelCategory {
    /// Ordem do menu.
    pub const ALL: [PanelCategory; 11] = [
        PanelCategory::General,
        PanelCategory::PreProduction,
        PanelCategory::Character,
        PanelCategory::Drawing,
        PanelCategory::Color,
        PanelCategory::Reference,
        PanelCategory::Time,
        PanelCategory::SoundEdit,
        PanelCategory::Delivery,
        PanelCategory::Management,
        PanelCategory::Resources,
    ];

    pub fn title_key(self) -> &'static str {
        match self {
            PanelCategory::General => "panel.cat.general",
            PanelCategory::PreProduction => "panel.cat.preprod",
            PanelCategory::Character => "panel.cat.character",
            PanelCategory::Drawing => "panel.cat.drawing",
            PanelCategory::Color => "panel.cat.color",
            PanelCategory::Reference => "panel.cat.reference",
            PanelCategory::Time => "panel.cat.time",
            PanelCategory::SoundEdit => "panel.cat.sound_edit",
            PanelCategory::Delivery => "panel.cat.delivery",
            PanelCategory::Management => "panel.cat.management",
            PanelCategory::Resources => "panel.cat.resources",
        }
    }
}

/// Onde um painel que não está na árvore entra quando o usuário o liga (se não houver painel da mesma categoria).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockHint {
    Left,
    Right,
    Bottom,
    Center,
}

/// Ficha do painel no catálogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelMeta {
    pub key: &'static str,
    pub title_key: &'static str,
    pub category: PanelCategory,
    pub dock_hint: DockHint,
    /// Workspaces que sugerem o painel (os de fábrica estão aqui; um workspace pode sugerir sem trazer de fábrica).
    pub workspaces: &'static [WorkspaceId],
    /// Só faz sentido onde há canvas (o próprio canvas, o navegador da visão).
    pub needs_canvas: bool,
}

use WorkspaceId as W;
const DRAWING_WS: &[WorkspaceId] = &[W::Personagens, W::Layout, W::Cenario, W::Animacao, W::Pintura, W::Composicao];
/// A Biblioteca vale em todo workspace (Exibir > Painéis); estes a sugerem.
const LIBRARY_WS: &[WorkspaceId] = &[W::Biblioteca, W::Modelagem, W::Personagens, W::Layout, W::Cenario, W::Animacao, W::Pintura, W::Composicao];

impl PanelKind {
    pub const ALL: [PanelKind; 59] = [
        PanelKind::Canvas,
        PanelKind::Layers,
        PanelKind::Properties,
        PanelKind::Palette,
        PanelKind::Navigator,
        PanelKind::Timeline,
        PanelKind::Script,
        PanelKind::Outline,
        PanelKind::Characters,
        PanelKind::Sheet,
        PanelKind::Shots,
        PanelKind::Board,
        PanelKind::Preview,
        PanelKind::Media,
        PanelKind::SourceMonitor,
        PanelKind::Transitions,
        PanelKind::ClipEffects,
        PanelKind::Captions,
        PanelKind::Mixer,
        PanelKind::Waveform,
        PanelKind::EffectsRack,
        PanelKind::Meters,
        PanelKind::Multitrack,
        PanelKind::Tts,
        PanelKind::Presets,
        PanelKind::Queue,
        PanelKind::Tasks,
        PanelKind::Episodes,
        PanelKind::ProdCharts,
        PanelKind::ProdBottleneck,
        PanelKind::ProdAi,
        PanelKind::Project,
        PanelKind::SubView,
        PanelKind::Brushes,
        PanelKind::ToolProps,
        PanelKind::BrushSize,
        PanelKind::Library,
        PanelKind::LibraryPreview,
        PanelKind::Modeling,
        PanelKind::ModelOutliner,
        PanelKind::ModelProps,
        PanelKind::ModelModifiers,
        PanelKind::ModelUv,
        PanelKind::Text,
        PanelKind::LightTable,
        PanelKind::Copilot,
        PanelKind::SceneInfo,
        PanelKind::CharRef,
        PanelKind::Rulers,
        PanelKind::CharFolder,
        PanelKind::CompTimeline,
        PanelKind::EffectsBrowser,
        PanelKind::EffectControls,
            PanelKind::Destinations,
        PanelKind::Deliverables,
        PanelKind::Poster,
        PanelKind::Preflight,
        PanelKind::History,
        PanelKind::LayerProps,
    ];

    /// Identificador estável (bridge/UI_ACTION, `layouts.json`): `canvas`, `layers`, `timeline`...
    pub fn key(self) -> &'static str {
        self.title_key().split('.').nth(1).unwrap_or("")
    }

    pub fn from_key(key: &str) -> Option<PanelKind> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }

    /// Chave i18n do título do painel.
    pub fn title_key(self) -> &'static str {
        match self {
            PanelKind::Canvas => "panel.canvas.title",
            PanelKind::Layers => "panel.layers.title",
            PanelKind::Properties => "panel.properties.title",
            PanelKind::Palette => "panel.palette.title",
            PanelKind::Navigator => "panel.navigator.title",
            PanelKind::Timeline => "panel.timeline.title",
            PanelKind::Script => "panel.script.title",
            PanelKind::Outline => "panel.outline.title",
            PanelKind::Characters => "panel.characters.title",
            PanelKind::Sheet => "panel.sheet.title",
            PanelKind::Shots => "panel.shots.title",
            PanelKind::Board => "panel.board.title",
            PanelKind::Preview => "panel.preview.title",
            PanelKind::Media => "panel.media.title",
            PanelKind::SourceMonitor => "panel.source_monitor.title",
            PanelKind::Transitions => "panel.transitions.title",
            PanelKind::ClipEffects => "panel.clip_effects.title",
            PanelKind::Captions => "panel.captions.title",
            PanelKind::Mixer => "panel.mixer.title",
            PanelKind::Waveform => "panel.waveform.title",
            PanelKind::EffectsRack => "panel.rack.title",
            PanelKind::Meters => "panel.loudness.title",
            PanelKind::Multitrack => "panel.multitrack.title",
            PanelKind::Tts => "panel.tts.title",
            PanelKind::Presets => "panel.presets.title",
            PanelKind::Queue => "panel.queue.title",
            PanelKind::Tasks => "panel.tasks.title",
            PanelKind::Episodes => "panel.episodes.title",
            PanelKind::ProdCharts => "panel.prod_charts.title",
            PanelKind::ProdBottleneck => "panel.prod_bottleneck.title",
            PanelKind::ProdAi => "panel.prod_ai.title",
            PanelKind::Project => "panel.project.title",
            PanelKind::SubView => "panel.subview.title",
            PanelKind::Brushes => "panel.brushes.title",
            PanelKind::ToolProps => "panel.tool_props.title",
            PanelKind::BrushSize => "panel.brush_size.title",
            PanelKind::Library => "panel.library.title",
            PanelKind::LibraryPreview => "panel.library_preview.title",
            PanelKind::Modeling => "panel.modeling.title",
            PanelKind::ModelOutliner => "panel.model_outliner.title",
            PanelKind::ModelProps => "panel.model_props.title",
            PanelKind::ModelModifiers => "panel.model_modifiers.title",
            PanelKind::ModelUv => "panel.model_uv.title",
            PanelKind::Text => "panel.text.title",
            PanelKind::LightTable => "panel.light_table.title",
            PanelKind::Copilot => "panel.copilot.title",
            PanelKind::SceneInfo => "panel.scene_info.title",
            PanelKind::CharRef => "panel.char_ref.title",
            PanelKind::Rulers => "panel.rulers.title",
            PanelKind::CharFolder => "panel.char_folder.title",
            PanelKind::CompTimeline => "panel.comp_timeline.title",
            PanelKind::EffectsBrowser => "panel.effects_browser.title",
            PanelKind::EffectControls => "panel.effect_controls.title",
            PanelKind::Destinations => "panel.destinations.title",
            PanelKind::Deliverables => "panel.deliverables.title",
            PanelKind::Poster => "panel.poster.title",
            PanelKind::Preflight => "panel.preflight.title",
            PanelKind::History => "panel.history.title",
            PanelKind::LayerProps => "panel.layer_props.title",
        }
    }

    /// Chave i18n do texto de estado vazio.
    pub fn empty_key(self) -> &'static str {
        match self {
            PanelKind::Canvas => "panel.canvas.empty",
            PanelKind::Layers => "panel.layers.empty",
            PanelKind::Properties => "panel.properties.empty",
            PanelKind::Palette => "panel.palette.empty",
            PanelKind::Navigator => "panel.navigator.empty",
            PanelKind::Timeline => "panel.timeline.empty",
            PanelKind::Script => "panel.script.empty",
            PanelKind::Outline => "panel.outline.empty",
            PanelKind::Characters => "panel.characters.empty",
            PanelKind::Sheet => "panel.sheet.empty",
            PanelKind::Shots => "panel.shots.empty",
            PanelKind::Board => "panel.board.empty",
            PanelKind::Preview => "panel.preview.empty",
            PanelKind::Media => "panel.media.empty",
            PanelKind::SourceMonitor => "panel.source_monitor.empty",
            PanelKind::Transitions => "panel.transitions.empty",
            PanelKind::ClipEffects => "panel.clip_effects.empty",
            PanelKind::Captions => "panel.captions.empty",
            PanelKind::Mixer => "panel.mixer.empty",
            PanelKind::Waveform => "panel.waveform.empty",
            PanelKind::EffectsRack => "panel.rack.empty",
            PanelKind::Meters => "panel.loudness.empty",
            PanelKind::Multitrack => "panel.multitrack.empty",
            PanelKind::Tts => "panel.tts.empty",
            PanelKind::Presets => "panel.presets.empty",
            PanelKind::Queue => "panel.queue.empty",
            PanelKind::Tasks => "panel.tasks.empty",
            PanelKind::Episodes => "panel.episodes.empty",
            PanelKind::ProdCharts => "panel.prod_charts.empty",
            PanelKind::ProdBottleneck => "panel.prod_bottleneck.empty",
            PanelKind::ProdAi => "panel.prod_ai.empty",
            PanelKind::Project => "panel.project.empty",
            PanelKind::SubView => "panel.subview.empty",
            PanelKind::Brushes => "panel.brushes.empty",
            PanelKind::ToolProps => "panel.tool_props.empty",
            PanelKind::BrushSize => "panel.brush_size.empty",
            PanelKind::Library => "panel.library.empty",
            PanelKind::LibraryPreview => "panel.library_preview.empty",
            PanelKind::Modeling => "panel.modeling.empty",
            PanelKind::ModelOutliner => "panel.model_outliner.empty",
            PanelKind::ModelProps => "panel.model_props.empty",
            PanelKind::ModelModifiers => "panel.model_modifiers.empty",
            PanelKind::ModelUv => "panel.model_uv.empty",
            PanelKind::Text => "panel.text.empty",
            PanelKind::LightTable => "panel.light_table.empty",
            PanelKind::Copilot => "panel.copilot.empty",
            PanelKind::SceneInfo => "panel.scene_info.empty",
            PanelKind::CharRef => "panel.char_ref.empty",
            PanelKind::Rulers => "panel.rulers.empty",
            PanelKind::CharFolder => "panel.char_folder.empty",
            PanelKind::CompTimeline => "panel.comp_timeline.empty",
            PanelKind::EffectsBrowser => "panel.effects_browser.empty",
            PanelKind::EffectControls => "panel.effect_controls.empty",
            PanelKind::Destinations => "panel.destinations.empty",
            PanelKind::Deliverables => "panel.deliverables.empty",
            PanelKind::Poster => "panel.poster.empty",
            PanelKind::Preflight => "panel.preflight.empty",
            PanelKind::History => "panel.history.empty",
            PanelKind::LayerProps => "panel.layer_props.empty",
        }
    }

    /// Ficha do catálogo: categoria, onde docar, em quais workspaces faz sentido, se precisa de canvas.
    pub fn meta(self) -> PanelMeta {
        use DockHint::*;
        use PanelCategory as C;
        let (category, dock_hint, workspaces, needs_canvas): (PanelCategory, DockHint, &'static [WorkspaceId], bool) = match self {
            PanelKind::Canvas => (C::Drawing, Center, DRAWING_WS, true),
            PanelKind::Layers => (C::Drawing, Right, DRAWING_WS, false),
            PanelKind::Properties => (C::General, Right, &[W::Roteiro, W::Personagens, W::Storyboard, W::Layout, W::Cenario, W::Animacao, W::Pintura, W::Composicao, W::Edicao, W::Entrega, W::Producao], false),
            PanelKind::Palette => (C::Color, Right, &[W::Personagens, W::Layout, W::Cenario, W::Animacao, W::Pintura, W::Biblioteca], false),
            PanelKind::Navigator => (C::Drawing, Right, DRAWING_WS, true),
            PanelKind::Timeline => (C::Time, Bottom, &[W::Storyboard, W::Animacao, W::Pintura, W::Composicao, W::Audio, W::Edicao], false),
            PanelKind::Script => (C::PreProduction, Center, &[W::Roteiro], false),
            PanelKind::Outline => (C::PreProduction, Left, &[W::Roteiro], false),
            PanelKind::Characters => (C::Character, Left, &[W::Personagens, W::Modelagem], false),
            PanelKind::Sheet => (C::Character, Center, &[W::Personagens], false),
            PanelKind::Shots => (C::PreProduction, Left, &[W::Storyboard], false),
            PanelKind::Board => (C::PreProduction, Center, &[W::Storyboard], false),
            PanelKind::Preview => (C::SoundEdit, Center, &[W::Audio, W::Edicao], false),
            PanelKind::Media => (C::SoundEdit, Left, &[W::Audio, W::Edicao], false),
            PanelKind::SourceMonitor => (C::SoundEdit, Center, &[W::Edicao], false),
            PanelKind::Transitions => (C::SoundEdit, Left, &[W::Edicao], false),
            PanelKind::ClipEffects => (C::SoundEdit, Right, &[W::Edicao], false),
            PanelKind::Captions => (C::SoundEdit, Left, &[W::Edicao], false),
            PanelKind::Mixer => (C::SoundEdit, Right, &[W::Audio], false),
            PanelKind::Waveform => (C::SoundEdit, Center, &[W::Audio], false),
            PanelKind::EffectsRack => (C::SoundEdit, Right, &[W::Audio], false),
            PanelKind::Meters => (C::SoundEdit, Right, &[W::Audio], false),
            PanelKind::Multitrack => (C::SoundEdit, Bottom, &[W::Audio], false),
            PanelKind::Tts => (C::SoundEdit, Right, &[W::Audio, W::Roteiro], false),
            PanelKind::Presets => (C::Delivery, Left, &[W::Entrega], false),
            PanelKind::Queue => (C::Delivery, Center, &[W::Entrega], false),
            PanelKind::Tasks => (C::Management, Center, &[W::Producao], false),
            PanelKind::Episodes => (C::Management, Left, &[W::Producao], false),
            PanelKind::ProdCharts => (C::Management, Center, &[W::Producao], false),
            PanelKind::ProdBottleneck => (C::Management, Right, &[W::Producao], false),
            PanelKind::ProdAi => (C::Management, Right, &[W::Producao], false),
            PanelKind::Project => (C::General, Left, DRAWING_WS, false),
            PanelKind::SubView => (C::Reference, Left, &[W::Layout, W::Cenario, W::Animacao, W::Pintura, W::Biblioteca, W::Modelagem], false),
            PanelKind::Brushes => (C::Drawing, Left, &[W::Personagens, W::Layout, W::Cenario, W::Animacao, W::Pintura], false),
            PanelKind::ToolProps => (C::Drawing, Left, &[W::Personagens, W::Layout, W::Cenario, W::Animacao, W::Pintura], false),
            PanelKind::BrushSize => (C::Drawing, Left, &[W::Personagens, W::Layout, W::Cenario, W::Animacao, W::Pintura], false),
            PanelKind::Library => (C::Resources, Bottom, LIBRARY_WS, false),
            PanelKind::LibraryPreview => (C::Resources, Right, LIBRARY_WS, false),
            PanelKind::Modeling => (C::Resources, Center, &[W::Modelagem], false),
            PanelKind::ModelOutliner => (C::Resources, Left, &[W::Modelagem], false),
            PanelKind::ModelProps => (C::Resources, Right, &[W::Modelagem], false),
            PanelKind::ModelModifiers => (C::Resources, Right, &[W::Modelagem], false),
            PanelKind::ModelUv => (C::Resources, Right, &[W::Modelagem], false),
            PanelKind::Text => (C::Drawing, Right, &[W::Layout, W::Cenario, W::Animacao, W::Composicao, W::Edicao], false),
            PanelKind::LightTable => (C::Drawing, Right, &[W::Animacao, W::Pintura], false),
            PanelKind::Copilot => (C::Drawing, Right, DRAWING_WS, true),
            PanelKind::SceneInfo => (C::PreProduction, Left, &[W::Storyboard, W::Layout, W::Cenario, W::Animacao, W::Pintura], false),
            PanelKind::CharRef => (C::Character, Left, &[W::Storyboard, W::Layout, W::Animacao, W::Pintura], false),
            PanelKind::Rulers => (C::Drawing, Left, &[W::Layout, W::Cenario, W::Animacao], true),
            PanelKind::CharFolder => (C::Character, Left, &[W::Personagens], false),
            PanelKind::CompTimeline => (C::Time, Bottom, &[W::Composicao], false),
            PanelKind::EffectsBrowser => (C::Color, Right, &[W::Composicao], false),
            PanelKind::EffectControls => (C::Color, Right, &[W::Composicao], false),
            PanelKind::Destinations => (C::Delivery, Left, &[W::Entrega], false),
            PanelKind::Deliverables => (C::Delivery, Center, &[W::Entrega], false),
            PanelKind::Poster => (C::Delivery, Right, &[W::Entrega], false),
            PanelKind::Preflight => (C::Delivery, Right, &[W::Entrega], false),
            PanelKind::History => (C::Drawing, Right, DRAWING_WS, false),
            PanelKind::LayerProps => (C::Drawing, Right, DRAWING_WS, false),
        };
        PanelMeta { key: self.key(), title_key: self.title_key(), category, dock_hint, workspaces, needs_canvas }
    }

    /// Painel que ainda não tem conteúdo: só explica o que vem (Modelagem).
    pub fn is_planned(self) -> bool {
        // S24 3D (fase 12): a Modelagem ganhou o viewport; nenhum painel está em construção agora
        false
    }
}

/// Descrição declarativa de um layout.
enum Node {
    Pane(PanelKind),
    Tabs(&'static [PanelKind]),
    /// Filhos lado a lado com as proporções dadas.
    Row(Vec<(f32, Node)>),
    /// Filhos empilhados com as proporções dadas.
    Col(Vec<(f32, Node)>),
}

use Node::{Col, Pane, Row, Tabs as TabsOf};

/// Variante do layout de fábrica de um workspace. Só a Animação tem duas (a fase do corte escolhe): Genga (a base:
/// chaves, mesa de luz, referências e cena à vista) e Douga (intercalações com a mesa de luz grande). Cada variante
/// guarda o seu próprio layout personalizado em `layouts.json` (`animacao` e `animacao.douga`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LayoutVariant {
    #[default]
    Base,
    Douga,
}

impl LayoutVariant {
    /// Sufixo da chave em `layouts.json` (a base usa a chave do workspace sem nada).
    pub fn suffix(self) -> &'static str {
        match self {
            LayoutVariant::Base => "",
            LayoutVariant::Douga => ".douga",
        }
    }
}

/// Revisão do layout de fábrica de cada workspace. Mudou a fábrica (`default_node`), sobe a revisão: quem tem layout
/// próprio guardado vê o aviso "o padrão foi atualizado" e escolhe (nada é sobrescrito sem pedir). O teste
/// `factory_changes_bump_the_revision` lembra de subir.
pub fn factory_rev(ws: WorkspaceId) -> u32 {
    match ws {
        // S24: Animação com Cena, Referências do personagem e Mesa de luz no lugar de Projeto e Sub View, linha do tempo
        // mais baixa (compacta) e a variante Douga
        // S25: lado esquerdo como nos programas de pintura (grupo de ferramentas, propriedades, tamanho, cores embaixo)
        // S25 (auditoria): propriedades da ferramenta mais altas (as opções novas cabiam só rolando em 1080p)
        W::Animacao => 5,
        // S24: pasta do personagem (materiais por tipo) e canvas para desenhar neles
        // S25: coluna da ferramenta (grupo, propriedades, cores) ao lado da pasta
        W::Personagens => 3,
        // S24: fábricas novas (cada etapa de desenho com a sua cara) e os workspaces novos
        // S24: Réguas (perspectiva) em aba com os pincéis
        // S25: lado esquerdo novo (grupo de ferramentas, propriedades, tamanho do pincel, cores embaixo)
        // S25 (direita, a mesma revisão do lado esquerdo): Navegador no topo, propriedades da camada e Camadas |
        // Histórico em abas, nos workspaces de desenho
        W::Layout => 5,
        W::Cenario | W::Pintura => 4,
        // S24 3D (fase 12): viewport 3D no centro, Objetos e Biblioteca à esquerda, Propriedades do objeto e
        // Modificadores à direita
        W::Modelagem => 4,
        // S24 2.7–2.8: Biblioteca de verdade (árvore e grade, prévia e metadados) com as Cores para editar paletas
        W::Biblioteca => 3,
        // S24 3.1–3.5: viewer no centro, Linha do tempo da composição embaixo, Efeitos e predefinições e Controles de
        // efeito à direita
        // S25: Histórico em aba com o Projeto e as Camadas
        W::Composicao => 4,
        // S24: painel Texto (legendas) em aba com as Propriedades
        // S24 3.13–3.18: estilo Premiere (Mídia/Transições/Legendas, monitor de origem e do programa, Efeitos do clipe)
        W::Edicao => 3,
        // S24: Forma de onda, Efeitos, Medidores, Multipista e Voz sintética
        W::Audio => 2,
        // S24: Produção com Gráficos, Gargalos e Análise por IA ao lado do painel principal
        // S24 4.8–4.12: fila com progresso, Destinos, Entregáveis, Pôster e Verificação final
        W::Entrega => 2,
        W::Producao => 2,
        _ => 1,
    }
}

/// Layout padrão de um workspace. Os de desenho seguem `S24-pesquisa/workspaces.md` (b), com os painéis que já existem:
/// fora da Animação, a Cena vira o Projeto (os cortes da cena), as Referências do personagem viram a Sub View e a fila
/// de pintura vira a linha do tempo (os quadros do corte).
fn default_node(ws: WorkspaceId) -> Node {
    use PanelKind as P;
    match ws {
        W::Roteiro => Row(vec![(0.22, Pane(P::Outline)), (0.52, Pane(P::Script)), (0.26, Pane(P::Properties))]),
        // pasta do personagem: a lista e a pasta (materiais por tipo) à esquerda, o canvas do material aberto no centro
        // (a folha com guias em aba) e à direita cores, camadas e a ficha em abas (ficha, paleta, consistência, voz, material)
        // S25: a coluna da ferramenta (grupo de ferramentas, propriedades e cores) entre a pasta e o canvas
        W::Personagens => Row(vec![
            (0.17, Col(vec![(0.30, Pane(P::Characters)), (0.70, Pane(P::CharFolder))])),
            (0.16, Col(vec![(0.32, Pane(P::Brushes)), (0.34, Pane(P::ToolProps)), (0.34, Pane(P::Palette))])),
            (0.42, TabsOf(&[P::Canvas, P::Sheet])),
            (0.25, Col(vec![(0.44, TabsOf(&[P::Layers, P::History])), (0.56, Pane(P::Properties))])),
        ]),
        W::Storyboard => Col(vec![
            (0.72, Row(vec![(0.20, Pane(P::Shots)), (0.56, Pane(P::Board)), (0.24, Pane(P::Properties))])),
            (0.28, Pane(P::Timeline)),
        ]),
        // enquadramento e construção: a cena (cortes) em cima à esquerda, réguas, pincéis e referências embaixo; à direita o
        // navegador (área de câmera), camadas e propriedades com a paleta em aba
        // S25: à esquerda o grupo de ferramentas (réguas em aba), as propriedades, o tamanho do pincel e as cores embaixo
        // (a cena e as referências em aba com elas)
        W::Layout => Row(vec![
            (0.19, Col(vec![(0.21, TabsOf(&[P::Brushes, P::Rulers])), (0.37, Pane(P::ToolProps)), (0.12, Pane(P::BrushSize)), (0.30, TabsOf(&[P::Palette, P::Project, P::SubView]))])),
            (0.53, Pane(P::Canvas)),
            (0.28, Col(vec![(0.28, Pane(P::Navigator)), (0.20, TabsOf(&[P::LayerProps, P::Text])), (0.52, TabsOf(&[P::Layers, P::History, P::Properties]))])),
        ]),
        // fundo: pincéis e referências grandes à esquerda (pintura de fundo vive de referência), paleta, camadas (planos)
        // e propriedades à direita
        // S25: à esquerda o grupo de ferramentas, as propriedades, o tamanho e as cores embaixo (referências em aba)
        W::Cenario => Row(vec![
            (0.18, Col(vec![(0.21, Pane(P::Brushes)), (0.37, Pane(P::ToolProps)), (0.12, Pane(P::BrushSize)), (0.30, TabsOf(&[P::Palette, P::SubView]))])),
            (0.53, Pane(P::Canvas)),
            (0.29, Col(vec![(0.28, Pane(P::Navigator)), (0.20, TabsOf(&[P::LayerProps, P::Text])), (0.52, TabsOf(&[P::Layers, P::History, P::Properties, P::Project]))])),
        ]),
        // genga: a cena do roteiro/storyboard e os pincéis em cima à esquerda, as referências do personagem embaixo; à
        // direita o navegador, a mesa de luz (com as cores pequenas em aba, para o balde de conferir linha) e camadas;
        // a linha do tempo compacta embaixo de tudo (22 %)
        // S25: à esquerda o grupo de ferramentas, as propriedades, o tamanho e as cores embaixo, com as referências do
        // personagem e a cena em aba; a mesa de luz fica sozinha à direita. As colunas dos lados vão na altura toda (como
        // nos programas de pintura) e a linha do tempo fica embaixo do canvas
        W::Animacao => Row(vec![
            (0.18, Col(vec![(0.21, Pane(P::Brushes)), (0.37, Pane(P::ToolProps)), (0.12, Pane(P::BrushSize)), (0.30, TabsOf(&[P::Palette, P::CharRef, P::SceneInfo]))])),
            (0.55, Col(vec![(0.78, Pane(P::Canvas)), (0.22, Pane(P::Timeline))])),
            (0.27, Col(vec![(0.25, Pane(P::Navigator)), (0.15, Pane(P::LayerProps)), (0.22, Pane(P::LightTable)), (0.38, TabsOf(&[P::Layers, P::History, P::Properties]))])),
        ]),
        // shiage: as cores (settei) em destaque à esquerda, referências à direita e os quadros do corte embaixo
        // S25: à esquerda o grupo de ferramentas, as propriedades, o tamanho e as cores (grandes) embaixo; as colunas dos
        // lados na altura toda e os quadros do corte embaixo do canvas
        W::Pintura => Row(vec![
            (0.18, Col(vec![(0.21, Pane(P::Brushes)), (0.37, Pane(P::ToolProps)), (0.12, Pane(P::BrushSize)), (0.30, TabsOf(&[P::Palette, P::Project]))])),
            (0.54, Col(vec![(0.80, Pane(P::Canvas)), (0.20, Pane(P::Timeline))])),
            (0.28, Col(vec![(0.28, Pane(P::Navigator)), (0.18, TabsOf(&[P::LayerProps, P::SubView])), (0.54, TabsOf(&[P::Layers, P::History, P::Properties]))])),
        ]),
        // satsuei como no After Effects (S24 3.1–3.5): projeto e camadas à esquerda, viewer no centro, Efeitos e
        // predefinições (com a câmera/cor e o texto em aba) em cima e Controles de efeito embaixo à direita; a Linha do
        // tempo da composição embaixo de tudo (a de exposição em aba)
        W::Composicao => Col(vec![
            (0.62, Row(vec![
                (0.17, TabsOf(&[P::Project, P::Layers, P::History])),
                (0.55, Pane(P::Canvas)),
                (0.28, Col(vec![(0.48, TabsOf(&[P::EffectsBrowser, P::Properties, P::Text])), (0.52, Pane(P::EffectControls))])),
            ])),
            (0.38, TabsOf(&[P::CompTimeline, P::Timeline])),
        ]),
        // biblioteca de recursos: a árvore e a grade grandes, a prévia com os metadados ao lado e as Cores (paletas) à direita
        W::Biblioteca => Row(vec![(0.50, Pane(P::Library)), (0.27, Pane(P::LibraryPreview)), (0.23, Pane(P::Palette))]),
        // modelagem de adereços (S24 3D, fase 12): objetos em cima à esquerda com as referências embaixo, o viewport 3D
        // grande no centro e, à direita, as propriedades do objeto sobre a pilha de modificadores
        W::Modelagem => Row(vec![
            (0.19, Col(vec![(0.58, Pane(P::ModelOutliner)), (0.42, Pane(P::SubView))])),
            (0.55, Pane(P::Modeling)),
            (0.26, Col(vec![(0.58, Pane(P::ModelProps)), (0.42, TabsOf(&[P::ModelModifiers, P::ModelUv]))])),
        ]),
        // nível Audition (S24 11): cortes, forma de onda grande, monitor (imagem do corte) sobre os medidores e o rack/voz
        // na altura toda à direita; multipista embaixo com o Mixer e a linha do tempo em abas
        W::Audio => Col(vec![
            (0.56, Row(vec![
                (0.14, Pane(P::Media)),
                (0.42, Pane(P::Waveform)),
                (0.19, Col(vec![(0.45, Pane(P::Preview)), (0.55, Pane(P::Meters))])),
                (0.25, TabsOf(&[P::EffectsRack, P::Tts])),
            ])),
            (0.44, TabsOf(&[P::Multitrack, P::Mixer, P::Timeline])),
        ]),
        // estilo Premiere "Editing" (S24 12): Mídia, Transições e Legendas em abas à esquerda, monitor de origem e do
        // programa lado a lado, Efeitos do clipe e Propriedades à direita; a linha do tempo embaixo
        W::Edicao => Col(vec![
            (0.55, Row(vec![
                (0.20, TabsOf(&[P::Media, P::Transitions, P::Captions])),
                (0.27, Pane(P::SourceMonitor)),
                (0.31, Pane(P::Preview)),
                (0.22, TabsOf(&[P::ClipEffects, P::Properties])),
            ])),
            (0.45, Pane(P::Timeline)),
        ]),
        // S24 4.8–4.12: formatos e destinos à esquerda, a fila em cima e os entregáveis embaixo no centro, e à direita o
        // pôster, a verificação final e o pedido
        W::Entrega => Row(vec![
            (0.20, TabsOf(&[P::Presets, P::Destinations])),
            (0.44, Col(vec![(0.58, Pane(P::Queue)), (0.42, Pane(P::Deliverables))])),
            (0.36, Col(vec![(0.34, Pane(P::Poster)), (0.30, Pane(P::Preflight)), (0.36, Pane(P::Properties))])),
        ]),
        // gestão (S24 14): episódios/cenas à esquerda; o painel principal (Lista, Miniaturas, Grande, Quadro) com os
        // Gráficos em aba grande ao lado; à direita o detalhe do corte e, embaixo, Gargalos e a Análise por IA (opcional)
        W::Producao => Row(vec![
            (0.17, Pane(P::Episodes)),
            (0.55, TabsOf(&[P::Tasks, P::ProdCharts])),
            (0.28, Col(vec![(0.45, Pane(P::Properties)), (0.55, TabsOf(&[P::ProdBottleneck, P::ProdAi]))])),
        ]),
    }
}

/// Fábrica de uma variante: a do Douga só existe na Animação (nos outros workspaces a variante cai na base).
fn default_node_v(ws: WorkspaceId, v: LayoutVariant) -> Node {
    use PanelKind as P;
    match (ws, v) {
        // douga: limpar e intercalar; a mesa de luz ganha painel próprio e mais altura, as referências e a cena ficam
        // em aba (consulta) e os pincéis embaixo delas; a linha do tempo compacta embaixo
        // S25: o mesmo lado esquerdo do Genga, com as referências na frente das cores
        (W::Animacao, LayoutVariant::Douga) => Row(vec![
            (0.17, Col(vec![(0.21, Pane(P::Brushes)), (0.37, Pane(P::ToolProps)), (0.12, Pane(P::BrushSize)), (0.30, TabsOf(&[P::CharRef, P::SceneInfo, P::Palette]))])),
            (0.56, Col(vec![(0.80, Pane(P::Canvas)), (0.20, Pane(P::Timeline))])),
            (0.27, Col(vec![(0.24, Pane(P::Navigator)), (0.27, Pane(P::LightTable)), (0.13, Pane(P::LayerProps)), (0.36, TabsOf(&[P::Layers, P::History, P::Properties]))])),
        ]),
        _ => default_node(ws),
    }
}

fn build(tiles: &mut Tiles<PanelKind>, node: &Node) -> TileId {
    match node {
        Pane(k) => tiles.insert_pane(*k),
        TabsOf(kinds) => {
            let ids: Vec<TileId> = kinds.iter().map(|k| tiles.insert_pane(*k)).collect();
            tiles.insert_new(Tile::Container(Container::Tabs(Tabs::new(ids))))
        }
        Row(children) => linear(tiles, LinearDir::Horizontal, children),
        Col(children) => linear(tiles, LinearDir::Vertical, children),
    }
}

fn linear(tiles: &mut Tiles<PanelKind>, dir: LinearDir, children: &[(f32, Node)]) -> TileId {
    let built: Vec<(f32, TileId)> = children.iter().map(|(share, n)| (*share, build(tiles, n))).collect();
    let mut lin = Linear::new(dir, built.iter().map(|(_, id)| *id).collect());
    for (share, id) in &built {
        lin.shares.set_share(*id, *share);
    }
    tiles.insert_new(Tile::Container(Container::Linear(lin)))
}

fn tree_id(ws: WorkspaceId) -> egui::Id {
    egui::Id::new(("aiai_dock", ws.key()))
}

/// Árvore padrão do workspace.
pub fn default_tree(ws: WorkspaceId) -> Tree<PanelKind> {
    default_tree_v(ws, LayoutVariant::Base)
}

/// Árvore padrão de uma variante do workspace.
pub fn default_tree_v(ws: WorkspaceId, v: LayoutVariant) -> Tree<PanelKind> {
    let mut tiles = Tiles::default();
    let root = build(&mut tiles, &default_node_v(ws, v));
    Tree::new(tree_id(ws), root, tiles)
}

/// Painéis (folhas) de uma árvore.
pub fn panels(tree: &Tree<PanelKind>) -> Vec<PanelKind> {
    tree.tiles
        .iter()
        .filter_map(|(_, t)| match t {
            Tile::Pane(p) => Some(*p),
            _ => None,
        })
        .collect()
}

/// O tile do painel, se ele está na árvore (visível ou escondido).
pub fn find_pane(tree: &Tree<PanelKind>, kind: PanelKind) -> Option<TileId> {
    tree.tiles.iter().find_map(|(id, t)| matches!(t, Tile::Pane(k) if *k == kind).then_some(*id))
}

/// Painéis que já têm barra de abas (não desenham cabeçalho próprio).
pub fn panes_in_tabs(tree: &Tree<PanelKind>) -> std::collections::HashSet<TileId> {
    let mut out = std::collections::HashSet::new();
    for (_, t) in tree.tiles.iter() {
        if let Tile::Container(Container::Tabs(tabs)) = t {
            out.extend(tabs.children.iter().copied());
        }
    }
    out
}

// ------------------------------------------------------------------------------------------ inserir painel ausente

/// Tiles da árvore a partir da raiz, em profundidade (ordem estável, ao contrário de `tiles.iter()`).
fn dfs(tree: &Tree<PanelKind>) -> Vec<TileId> {
    let mut out = Vec::new();
    let mut stack: Vec<TileId> = tree.root.into_iter().collect();
    while let Some(id) = stack.pop() {
        out.push(id);
        if let Some(Tile::Container(c)) = tree.tiles.get(id) {
            let kids = c.children_vec();
            stack.extend(kids.into_iter().rev());
        }
    }
    out
}

fn contains(tree: &Tree<PanelKind>, ancestor: TileId, needle: TileId) -> bool {
    let mut cur = Some(needle);
    while let Some(id) = cur {
        if id == ancestor {
            return true;
        }
        cur = tree.tiles.parent_of(id);
    }
    false
}

/// A linha principal (horizontal) do layout: a que tem o canvas ou, sem canvas, a primeira linha horizontal.
fn main_row(tree: &Tree<PanelKind>) -> Option<TileId> {
    let rows: Vec<TileId> = dfs(tree).into_iter().filter(|id| matches!(tree.tiles.get(*id), Some(Tile::Container(Container::Linear(l))) if l.dir == LinearDir::Horizontal)).collect();
    let canvas = find_pane(tree, PanelKind::Canvas);
    canvas.and_then(|c| rows.iter().copied().find(|r| contains(tree, *r, c))).or_else(|| rows.first().copied())
}

/// Põe `new` na linear `lin_id` na posição `index` com `frac` do espaço (as proporções dos outros ficam).
fn insert_into_linear(tree: &mut Tree<PanelKind>, lin_id: TileId, index: usize, new: TileId, frac: f32) {
    if let Some(Tile::Container(Container::Linear(l))) = tree.tiles.get_mut(lin_id) {
        let total: f32 = l.children.iter().map(|c| l.shares[*c]).sum::<f32>().max(0.001);
        let share = total * frac / (1.0 - frac);
        let i = index.min(l.children.len());
        l.children.insert(i, new);
        l.shares.set_share(new, share);
    }
}

/// Embrulha a raiz numa linear nova com `new` antes ou depois.
fn wrap_root(tree: &mut Tree<PanelKind>, dir: LinearDir, new: TileId, new_first: bool, frac: f32) {
    let Some(root) = tree.root else {
        tree.root = Some(new);
        return;
    };
    let children = if new_first { [new, root] } else { [root, new] };
    let mut lin = Linear::new(dir, children.to_vec());
    lin.shares.set_share(new, frac);
    lin.shares.set_share(root, 1.0 - frac);
    tree.root = Some(tree.tiles.insert_container(Container::Linear(lin)));
}

/// Liga um painel que não está na árvore: entra como aba junto de um painel visível da mesma categoria; sem ele, segundo
/// `dock_hint` (coluna à esquerda/direita da linha principal, faixa embaixo, ou coluna ao lado do centro). Devolve o tile.
/// Painéis que não entram em nenhum layout de fábrica: aparecem sob demanda (o Copiloto entra quando o usuário o liga e
/// sai quando desliga). Pôr num layout de fábrica mudaria a assinatura dos padrões e mostraria a todos o aviso de
/// "o padrão foi atualizado".
pub const ON_DEMAND: &[PanelKind] = &[PanelKind::Copilot];

/// Insere `kind` como uma coluna própria, de altura inteira, na ponta direita da linha principal (`share` da largura):
/// para um painel comprido que precisa ficar à vista enquanto se desenha (o Copiloto). Já no layout: só reaparece.
pub fn insert_panel_column(tree: &mut Tree<PanelKind>, kind: PanelKind, share: f32) -> TileId {
    if let Some(id) = find_pane(tree, kind) {
        tree.tiles.set_visible(id, true);
        tree.make_active(|tid, _| tid == id);
        return id;
    }
    let new = tree.tiles.insert_pane(kind);
    match main_row(tree) {
        Some(r) => insert_into_linear(tree, r, usize::MAX, new, share),
        None => wrap_root(tree, LinearDir::Horizontal, new, false, share),
    }
    tree.make_active(|tid, _| tid == new);
    new
}

pub fn insert_panel(tree: &mut Tree<PanelKind>, kind: PanelKind) -> TileId {
    if let Some(id) = find_pane(tree, kind) {
        tree.tiles.set_visible(id, true);
        tree.make_active(|tid, _| tid == id);
        return id;
    }
    let meta = kind.meta();
    let order = dfs(tree);
    let new = tree.tiles.insert_pane(kind);
    // 1. aba ao lado de um painel da mesma categoria (nunca junto do canvas)
    let mate = order.iter().copied().find(|id| tree.tiles.is_visible(*id) && matches!(tree.tiles.get(*id), Some(Tile::Pane(k)) if *k != PanelKind::Canvas && k.meta().category == meta.category));
    if let Some(mate) = mate {
        let parent = tree.tiles.parent_of(mate);
        match parent.and_then(|p| tree.tiles.get_mut(p)) {
            Some(Tile::Container(Container::Tabs(t))) => {
                t.add_child(new);
                t.set_active(new);
            }
            _ => {
                // painel solto: vira uma barra de abas [mate, novo] no mesmo lugar
                let tabs = tree.tiles.insert_container(Container::Tabs(Tabs::new(vec![mate, new])));
                if let Some(p) = parent {
                    if let Some(Tile::Container(c)) = tree.tiles.get_mut(p) {
                        let _ = c.replace_child(mate, tabs);
                    }
                } else {
                    tree.root = Some(tabs);
                }
                if let Some(Tile::Container(Container::Tabs(t))) = tree.tiles.get_mut(tabs) {
                    t.set_active(new);
                }
            }
        }
    } else {
        let row = main_row(tree);
        match (meta.dock_hint, row) {
            (DockHint::Left, Some(r)) => insert_into_linear(tree, r, 0, new, 0.22),
            (DockHint::Right, Some(r)) => insert_into_linear(tree, r, usize::MAX, new, 0.25),
            (DockHint::Center, Some(r)) => {
                let canvas = find_pane(tree, PanelKind::Canvas);
                let idx = match tree.tiles.get(r) {
                    Some(Tile::Container(Container::Linear(l))) => {
                        canvas.and_then(|c| l.children.iter().position(|k| contains(tree, *k, c))).map(|i| i + 1).unwrap_or(l.children.len() / 2 + 1)
                    }
                    _ => 0,
                };
                insert_into_linear(tree, r, idx, new, 0.30);
            }
            (DockHint::Bottom, _) => match tree.root.and_then(|r| tree.tiles.get(r)) {
                Some(Tile::Container(Container::Linear(l))) if l.dir == LinearDir::Vertical => {
                    let r = tree.root.unwrap_or(new);
                    insert_into_linear(tree, r, usize::MAX, new, 0.25)
                }
                _ => wrap_root(tree, LinearDir::Vertical, new, false, 0.25),
            },
            (DockHint::Left, None) => wrap_root(tree, LinearDir::Horizontal, new, true, 0.22),
            (DockHint::Right | DockHint::Center, None) => wrap_root(tree, LinearDir::Horizontal, new, false, 0.25),
        }
    }
    tree.make_active(|tid, _| tid == new);
    new
}

// ------------------------------------------------------------------------------------------ árvore guardada

/// Um filho de linha/coluna com a sua proporção.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedChild {
    pub share: f32,
    pub node: SavedNode,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Árvore do dock em forma guardável: painéis pela chave (`PanelKind::key`), proporções e abas ativas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SavedNode {
    Pane {
        panel: String,
        #[serde(default, skip_serializing_if = "is_false")]
        hidden: bool,
    },
    Tabs {
        children: Vec<SavedNode>,
        #[serde(default)]
        active: Option<usize>,
        #[serde(default, skip_serializing_if = "is_false")]
        hidden: bool,
    },
    Row {
        children: Vec<SavedChild>,
        #[serde(default, skip_serializing_if = "is_false")]
        hidden: bool,
    },
    Col {
        children: Vec<SavedChild>,
        #[serde(default, skip_serializing_if = "is_false")]
        hidden: bool,
    },
    Grid {
        children: Vec<SavedNode>,
        #[serde(default, skip_serializing_if = "is_false")]
        hidden: bool,
    },
}

fn save_tile(tiles: &Tiles<PanelKind>, id: TileId) -> Option<SavedNode> {
    let hidden = !tiles.is_visible(id);
    Some(match tiles.get(id)? {
        Tile::Pane(k) => SavedNode::Pane { panel: k.key().to_owned(), hidden },
        Tile::Container(Container::Tabs(t)) => {
            let kids: Vec<(TileId, SavedNode)> = t.children.iter().filter_map(|c| Some((*c, save_tile(tiles, *c)?))).collect();
            let active = t.active.and_then(|a| kids.iter().position(|(c, _)| *c == a));
            SavedNode::Tabs { children: kids.into_iter().map(|(_, n)| n).collect(), active, hidden }
        }
        Tile::Container(Container::Linear(l)) => {
            let children = l.children.iter().filter_map(|c| Some(SavedChild { share: l.shares[*c], node: save_tile(tiles, *c)? })).collect();
            if l.dir == LinearDir::Horizontal { SavedNode::Row { children, hidden } } else { SavedNode::Col { children, hidden } }
        }
        Tile::Container(Container::Grid(g)) => SavedNode::Grid { children: g.children().filter_map(|c| save_tile(tiles, *c)).collect(), hidden },
    })
}

/// A árvore em uso em forma guardável (`None` = árvore vazia).
pub fn saved_from_tree(tree: &Tree<PanelKind>) -> Option<SavedNode> {
    save_tile(&tree.tiles, tree.root?)
}

/// Monta um nó guardado; chaves desconhecidas, painéis repetidos e o canvas onde não há canvas são descartados
/// (registrados no log). Contêiner que ficou vazio some.
fn load_node(tiles: &mut Tiles<PanelKind>, node: &SavedNode, ws: WorkspaceId, seen: &mut std::collections::HashSet<PanelKind>) -> Option<TileId> {
    let (id, hidden) = match node {
        SavedNode::Pane { panel, hidden } => {
            let Some(kind) = PanelKind::from_key(panel) else {
                tracing::info!("layout de {}: painel desconhecido '{panel}' descartado", ws.key());
                return None;
            };
            if (kind.meta().needs_canvas && !ws.profile().canvas) || !seen.insert(kind) {
                tracing::info!("layout de {}: painel '{panel}' descartado (repetido ou sem canvas)", ws.key());
                return None;
            }
            (tiles.insert_pane(kind), *hidden)
        }
        SavedNode::Tabs { children, active, hidden } => {
            let kids: Vec<(usize, TileId)> = children.iter().enumerate().filter_map(|(i, c)| Some((i, load_node(tiles, c, ws, seen)?))).collect();
            if kids.is_empty() {
                return None;
            }
            let mut t = Tabs::new(kids.iter().map(|(_, id)| *id).collect());
            if let Some(a) = active.and_then(|a| kids.iter().find(|(i, _)| *i == a)) {
                t.set_active(a.1);
            }
            (tiles.insert_container(Container::Tabs(t)), *hidden)
        }
        SavedNode::Row { children, hidden } | SavedNode::Col { children, hidden } => {
            let dir = if matches!(node, SavedNode::Row { .. }) { LinearDir::Horizontal } else { LinearDir::Vertical };
            let kids: Vec<(f32, TileId)> = children.iter().filter_map(|c| Some((c.share, load_node(tiles, &c.node, ws, seen)?))).collect();
            if kids.is_empty() {
                return None;
            }
            let mut lin = Linear::new(dir, kids.iter().map(|(_, id)| *id).collect());
            for (share, id) in &kids {
                lin.shares.set_share(*id, if share.is_finite() && *share > 0.0 { *share } else { 1.0 });
            }
            (tiles.insert_container(Container::Linear(lin)), *hidden)
        }
        SavedNode::Grid { children, hidden } => {
            let kids: Vec<TileId> = children.iter().filter_map(|c| load_node(tiles, c, ws, seen)).collect();
            if kids.is_empty() {
                return None;
            }
            (tiles.insert_container(Container::new_grid(kids)), *hidden)
        }
    };
    if hidden {
        tiles.set_visible(id, false);
    }
    Some(id)
}

/// A árvore de um layout guardado, ou `None` se não sobrou nenhum painel conhecido. Workspace com canvas sem o canvas na
/// árvore (arquivo editado à mão) ganha o canvas de volta.
pub fn tree_from_saved(ws: WorkspaceId, node: &SavedNode) -> Option<Tree<PanelKind>> {
    let mut tiles = Tiles::default();
    let mut seen = std::collections::HashSet::new();
    let root = load_node(&mut tiles, node, ws, &mut seen)?;
    let mut tree = Tree::new(tree_id(ws), root, tiles);
    if ws.profile().canvas && !seen.contains(&PanelKind::Canvas) {
        insert_panel(&mut tree, PanelKind::Canvas);
    }
    Some(tree)
}

/// A árvore de uma janela solta (`dock_float`): o mesmo carregamento, com o id da janela e sem repor o canvas (ele
/// fica na janela principal ou em outra janela). Painéis em `taken` (já em outro lugar do layout) ficam de fora.
pub fn float_tree_from_saved(ws: WorkspaceId, node: &SavedNode, id: egui::Id, taken: &std::collections::HashSet<PanelKind>) -> Option<Tree<PanelKind>> {
    let mut tiles = Tiles::default();
    let mut seen = taken.clone();
    let root = load_node(&mut tiles, node, ws, &mut seen)?;
    Some(Tree::new(id, root, tiles))
}

/// Mesmo layout: mesma estrutura, mesmos painéis e o mesmo escondido, proporções iguais a menos de 0,5 % (a aba ativa
/// não conta: trocar de aba não personaliza o layout).
pub fn same_layout(a: &SavedNode, b: &SavedNode) -> bool {
    fn shares_close(a: &[SavedChild], b: &[SavedChild]) -> bool {
        let (ta, tb) = (a.iter().map(|c| c.share).sum::<f32>().max(1e-6), b.iter().map(|c| c.share).sum::<f32>().max(1e-6));
        a.iter().zip(b).all(|(x, y)| (x.share / ta - y.share / tb).abs() < 0.005 && same_layout(&x.node, &y.node))
    }
    match (a, b) {
        (SavedNode::Pane { panel: p, hidden: h }, SavedNode::Pane { panel: q, hidden: g }) => p == q && h == g,
        (SavedNode::Tabs { children: x, hidden: h, .. }, SavedNode::Tabs { children: y, hidden: g, .. })
        | (SavedNode::Grid { children: x, hidden: h }, SavedNode::Grid { children: y, hidden: g }) => {
            h == g && x.len() == y.len() && x.iter().zip(y).all(|(m, n)| same_layout(m, n))
        }
        (SavedNode::Row { children: x, hidden: h }, SavedNode::Row { children: y, hidden: g })
        | (SavedNode::Col { children: x, hidden: h }, SavedNode::Col { children: y, hidden: g }) => h == g && x.len() == y.len() && shares_close(x, y),
        _ => false,
    }
}

/// O layout de fábrica em forma guardável.
pub fn factory_saved(ws: WorkspaceId) -> SavedNode {
    factory_saved_v(ws, LayoutVariant::Base)
}

/// O layout de fábrica de uma variante em forma guardável.
pub fn factory_saved_v(ws: WorkspaceId, v: LayoutVariant) -> SavedNode {
    saved_from_tree(&default_tree_v(ws, v)).expect("fábrica sem raiz")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{self, Lang};
    use std::collections::HashSet;

    #[test]
    fn panel_keys_round_trip_and_are_unique() {
        let mut seen = HashSet::new();
        for p in PanelKind::ALL {
            assert_eq!(PanelKind::from_key(p.key()), Some(p));
            assert!(seen.insert(p.key()), "{}", p.key());
            assert_eq!(p.meta().key, p.key());
        }
        assert_eq!(PanelKind::Canvas.key(), "canvas");
    }

    #[test]
    fn every_workspace_loads_a_layout_with_unique_panels() {
        for ws in WorkspaceId::ALL {
            let tree = default_tree(ws);
            assert!(tree.root().is_some(), "{ws:?} sem raiz");
            let p = panels(&tree);
            assert!(p.len() >= 3, "{ws:?} com poucos painéis: {p:?}");
            let set: HashSet<_> = p.iter().collect();
            assert_eq!(set.len(), p.len(), "{ws:?} repete painel");
            assert!(tree.tiles.len() > p.len(), "{ws:?} sem containers");
        }
    }

    #[test]
    fn canvas_workspaces_have_a_canvas_and_the_others_do_not() {
        for ws in WorkspaceId::ALL {
            let has = panels(&default_tree(ws)).contains(&PanelKind::Canvas);
            assert_eq!(has, ws.profile().canvas, "{ws:?}");
        }
    }

    #[test]
    fn timeline_and_script_placement_follow_the_brief() {
        let has = |ws, k| panels(&default_tree(ws)).contains(&k);
        assert!(has(WorkspaceId::Animacao, PanelKind::Timeline));
        assert!(has(WorkspaceId::Audio, PanelKind::Timeline));
        assert!(has(WorkspaceId::Edicao, PanelKind::Timeline));
        assert!(has(WorkspaceId::Composicao, PanelKind::Timeline));
        assert!(has(WorkspaceId::Roteiro, PanelKind::Script));
        assert!(has(WorkspaceId::Biblioteca, PanelKind::Library));
        assert!(has(WorkspaceId::Modelagem, PanelKind::Modeling));
        // a Composição é um viewer: sem pincéis nem paleta de pintura
        assert!(!has(WorkspaceId::Composicao, PanelKind::Brushes) && !has(WorkspaceId::Composicao, PanelKind::Palette));
    }

    #[test]
    fn drawing_workspaces_no_longer_look_the_same() {
        let ws = [WorkspaceId::Layout, WorkspaceId::Cenario, WorkspaceId::Animacao, WorkspaceId::Pintura, WorkspaceId::Composicao];
        for (i, a) in ws.iter().enumerate() {
            for b in &ws[i + 1..] {
                assert!(!same_layout(&factory_saved(*a), &factory_saved(*b)), "{a:?} e {b:?} têm o mesmo layout de fábrica");
            }
        }
    }

    #[test]
    fn catalog_is_consistent_with_the_factories() {
        let mut used: HashSet<PanelKind> = HashSet::new();
        for ws in WorkspaceId::ALL {
            for p in panels(&default_tree(ws)) {
                assert!(p.meta().workspaces.contains(&ws), "{p:?} está na fábrica de {ws:?} mas o catálogo não sugere");
                assert!(!p.meta().needs_canvas || ws.profile().canvas, "{p:?} precisa de canvas e está em {ws:?}");
                used.insert(p);
            }
        }
        for p in PanelKind::ALL {
            assert!(used.contains(&p) || ON_DEMAND.contains(&p), "{p:?} não está em nenhum workspace (nem é sob demanda)");
            assert!(!(used.contains(&p) && ON_DEMAND.contains(&p)), "{p:?} é sob demanda mas está numa fábrica");
            assert!(!p.meta().workspaces.is_empty(), "{p:?}");
        }
        let cats: HashSet<_> = PanelKind::ALL.iter().map(|p| p.meta().category).collect();
        for c in cats {
            assert!(PanelCategory::ALL.contains(&c));
        }
    }

    #[test]
    fn panel_texts_exist_in_both_languages_and_are_not_filler() {
        for k in PanelKind::ALL {
            for lang in [Lang::Pt, Lang::En] {
                for key in [k.title_key(), k.empty_key()] {
                    let s = i18n::tr_in(lang, key);
                    assert_ne!(s, key, "chave ausente: {key}");
                    assert!(!s.to_lowercase().contains("lorem"), "{key}");
                }
            }
        }
        for c in PanelCategory::ALL {
            for lang in [Lang::Pt, Lang::En] {
                assert_ne!(i18n::tr_in(lang, c.title_key()), c.title_key());
            }
        }
    }

    #[test]
    fn saved_tree_round_trips_through_json() {
        for ws in WorkspaceId::ALL {
            let mut tree = default_tree(ws);
            // personaliza: esconde um painel e muda uma proporção
            let p = panels(&tree).into_iter().find(|p| *p != PanelKind::Canvas).unwrap();
            let id = find_pane(&tree, p).unwrap();
            tree.tiles.set_visible(id, false);
            let saved = saved_from_tree(&tree).unwrap();
            assert!(!same_layout(&saved, &factory_saved(ws)), "{ws:?}: esconder um painel personaliza");
            let json = serde_json::to_string(&saved).unwrap();
            assert!(json.contains(&format!("\"panel\":\"{}\"", p.key())), "chave de texto, não índice: {json}");
            let back: SavedNode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, saved);
            let rebuilt = tree_from_saved(ws, &back).unwrap();
            assert_eq!(saved_from_tree(&rebuilt).unwrap(), saved, "{ws:?}");
            assert!(!rebuilt.tiles.is_visible(find_pane(&rebuilt, p).unwrap()));
            assert!(same_layout(&factory_saved(ws), &saved_from_tree(&default_tree(ws)).unwrap()));
        }
    }

    #[test]
    fn unknown_and_repeated_keys_are_dropped() {
        let json = r#"{"type":"row","children":[
            {"share":0.3,"node":{"type":"pane","panel":"painel_que_nao_existe"}},
            {"share":0.4,"node":{"type":"tabs","children":[{"type":"pane","panel":"layers"},{"type":"pane","panel":"layers"},{"type":"pane","panel":"nada"}],"active":2}},
            {"share":0.3,"node":{"type":"col","children":[{"share":1.0,"node":{"type":"pane","panel":"zzz"}}]}}]}"#;
        let node: SavedNode = serde_json::from_str(json).unwrap();
        // Pintura tem canvas: volta mesmo sem estar no arquivo
        let tree = tree_from_saved(WorkspaceId::Pintura, &node).unwrap();
        let mut p = panels(&tree);
        p.sort_by_key(|k| k.key());
        assert_eq!(p, vec![PanelKind::Canvas, PanelKind::Layers]);
        // só chaves desconhecidas: nada a carregar (o app usa a fábrica)
        let none: SavedNode = serde_json::from_str(r#"{"type":"tabs","children":[{"type":"pane","panel":"x"}]}"#).unwrap();
        assert!(tree_from_saved(WorkspaceId::Roteiro, &none).is_none());
        // canvas onde não há canvas é descartado
        let c: SavedNode = serde_json::from_str(r#"{"type":"row","children":[{"share":1,"node":{"type":"pane","panel":"canvas"}},{"share":1,"node":{"type":"pane","panel":"script"}}]}"#).unwrap();
        assert_eq!(panels(&tree_from_saved(WorkspaceId::Roteiro, &c).unwrap()), vec![PanelKind::Script]);
    }

    #[test]
    fn showing_a_missing_panel_inserts_it_by_category_or_dock_hint() {
        // mesma categoria: Projeto (Geral) no Roteiro entra como aba junto das Propriedades (Geral)
        let mut t = default_tree(WorkspaceId::Roteiro);
        let id = insert_panel(&mut t, PanelKind::Project);
        let parent = t.tiles.parent_of(id).unwrap();
        match t.tiles.get(parent) {
            Some(Tile::Container(Container::Tabs(tabs))) => {
                assert!(tabs.is_active(id));
                assert!(tabs.children.iter().any(|c| matches!(t.tiles.get(*c), Some(Tile::Pane(PanelKind::Properties)))));
            }
            other => panic!("esperava abas: {other:?}"),
        }
        // embaixo: Linha do tempo no Roteiro (sem par de categoria) vira faixa embaixo de tudo
        let mut t = default_tree(WorkspaceId::Roteiro);
        let id = insert_panel(&mut t, PanelKind::Timeline);
        let root = t.root.unwrap();
        match t.tiles.get(root) {
            Some(Tile::Container(Container::Linear(l))) => {
                assert_eq!(l.dir, LinearDir::Vertical);
                assert_eq!(l.children.last(), Some(&id));
            }
            other => panic!("{other:?}"),
        }
        // esquerda: Mídia (Som e montagem) no Layout entra como primeira coluna da linha do canvas
        let mut t = default_tree(WorkspaceId::Layout);
        let id = insert_panel(&mut t, PanelKind::Media);
        let row = main_row(&t).unwrap();
        match t.tiles.get(row) {
            Some(Tile::Container(Container::Linear(l))) => assert_eq!(l.children.first(), Some(&id)),
            other => panic!("{other:?}"),
        }
        // centro: Roteiro (Script) na Pintura entra logo depois do canvas
        let mut t = default_tree(WorkspaceId::Pintura);
        let id = insert_panel(&mut t, PanelKind::Script);
        let row = main_row(&t).unwrap();
        let canvas = find_pane(&t, PanelKind::Canvas).unwrap();
        match t.tiles.get(row) {
            Some(Tile::Container(Container::Linear(l))) => {
                let ci = l.children.iter().position(|c| contains(&t, *c, canvas)).unwrap();
                assert_eq!(l.children.get(ci + 1), Some(&id));
            }
            other => panic!("{other:?}"),
        }
        // painel escondido: só volta a aparecer (não duplica)
        let mut t = default_tree(WorkspaceId::Animacao);
        let tl = find_pane(&t, PanelKind::Timeline).unwrap();
        t.tiles.set_visible(tl, false);
        assert_eq!(insert_panel(&mut t, PanelKind::Timeline), tl);
        assert!(t.tiles.is_visible(tl));
        assert_eq!(panels(&t).iter().filter(|p| **p == PanelKind::Timeline).count(), 1);
        // a árvore com o painel novo continua guardável e volta igual
        let saved = saved_from_tree(&t).unwrap();
        assert_eq!(saved_from_tree(&tree_from_saved(WorkspaceId::Animacao, &saved).unwrap()).unwrap(), saved);
    }

    #[test]
    fn animation_factories_bring_the_animation_panels_in_two_variants() {
        for v in [LayoutVariant::Base, LayoutVariant::Douga] {
            let p = panels(&default_tree_v(WorkspaceId::Animacao, v));
            for k in [PanelKind::Canvas, PanelKind::Timeline, PanelKind::LightTable, PanelKind::SceneInfo, PanelKind::CharRef, PanelKind::Layers] {
                assert!(p.contains(&k), "{v:?} sem {k:?}");
            }
            // o usuário pediu: Projeto e Sub View não servem na Animação; as cores ficam (pequenas, em aba) por causa do balde
            assert!(!p.contains(&PanelKind::Project) && !p.contains(&PanelKind::SubView), "{v:?}: {p:?}");
            assert!(p.contains(&PanelKind::Palette));
            let set: HashSet<_> = p.iter().collect();
            assert_eq!(set.len(), p.len());
            for k in &p {
                assert!(k.meta().workspaces.contains(&WorkspaceId::Animacao), "{k:?}");
            }
            // ida e volta pelo JSON guardado
            let saved = factory_saved_v(WorkspaceId::Animacao, v);
            assert_eq!(saved_from_tree(&tree_from_saved(WorkspaceId::Animacao, &saved).unwrap()).unwrap(), saved);
        }
        assert!(!same_layout(&factory_saved_v(WorkspaceId::Animacao, LayoutVariant::Douga), &factory_saved(WorkspaceId::Animacao)), "Genga e Douga iguais");
        // a mesa de luz tem painel próprio no Douga (no Genga divide aba com as cores)
        let douga = default_tree_v(WorkspaceId::Animacao, LayoutVariant::Douga);
        let lt = find_pane(&douga, PanelKind::LightTable).unwrap();
        assert!(!panes_in_tabs(&douga).contains(&lt));
        // variante sem fábrica própria cai na base
        assert!(same_layout(&factory_saved_v(WorkspaceId::Pintura, LayoutVariant::Douga), &factory_saved(WorkspaceId::Pintura)));
    }

    /// Assinatura da fábrica (FNV-1a do JSON guardável): mudar `default_node` muda a assinatura e obriga a rever a tabela
    /// abaixo junto com `factory_rev` (quem tem layout próprio precisa ser avisado do padrão novo).
    fn signature(ws: WorkspaceId) -> u64 {
        signature_v(ws, LayoutVariant::Base)
    }

    fn signature_v(ws: WorkspaceId, v: LayoutVariant) -> u64 {
        let json = serde_json::to_string(&factory_saved_v(ws, v)).unwrap();
        json.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
    }

    /// A fábrica do Douga divide a revisão com a da Animação: mudou uma, sobe `factory_rev(Animacao)`.
    #[test]
    fn douga_factory_changes_bump_the_animation_revision() {
        let (rev, sig) = (factory_rev(W::Animacao), signature_v(W::Animacao, LayoutVariant::Douga));
        assert_eq!((rev, sig), (5, 0x44ab306d6fb2cb6e), "a fábrica do Douga mudou; suba `factory_rev(Animacao)` e atualize: ({rev}, 0x{sig:016x})");
    }

    #[test]
    fn factory_changes_bump_the_revision() {
        let expected: &[(WorkspaceId, u32, u64)] = &[
            (W::Roteiro, 1, 0x815a21560ccb6c0e),
            (W::Personagens, 3, 0x17b9ea49d212f47f),
            (W::Storyboard, 1, 0x14d8a62efb1d9b23),
            (W::Layout, 5, 0x922099a74aacb19c),
            (W::Cenario, 4, 0x1e71bf899774b5ca),
            (W::Animacao, 5, 0xa91e62d884157881),
            (W::Pintura, 4, 0x92c9dca60add5222),
            (W::Composicao, 4, 0x097fc0aca4a379a6),
            (W::Biblioteca, 3, 0x1a430bf82b451dc2),
            (W::Modelagem, 4, 0xf934e709501b458f),
            (W::Audio, 2, 0xb370ba9cf04cb8ac),
            (W::Edicao, 3, 0x3d752f0083dbd76d),
            (W::Entrega, 2, 0x5ef237c098c7824b),
            (W::Producao, 2, 0x9b17b578f72810dc),
        ];
        let mut report = String::new();
        let mut changed = Vec::new();
        for ws in WorkspaceId::ALL {
            let sig = signature(ws);
            report.push_str(&format!("(W::{ws:?}, {}, 0x{sig:016x}),\n", factory_rev(ws)));
            if expected.iter().find(|(w, _, _)| *w == ws).is_some_and(|(_, rev, s)| (*rev, *s) != (factory_rev(ws), sig)) {
                changed.push(ws);
            }
        }
        assert!(changed.is_empty(), "a fábrica de {changed:?} mudou; suba `factory_rev` e atualize a tabela:\n{report}");
        assert_eq!(expected.len(), WorkspaceId::ALL.len(), "tabela incompleta; atual:\n{report}");
    }
}
