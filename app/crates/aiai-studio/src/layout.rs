//! Painéis e layouts padrão de cada workspace (`egui_tiles`). Os painéis são placeholders com título e texto de
//! estado vazio; o conteúdo real chega nas sprints seguintes (canvas na S03, projeto na S01...).

use crate::workspace::WorkspaceId;
use egui_tiles::{Container, Linear, LinearDir, Tabs, Tile, TileId, Tiles, Tree};

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
    Mixer,
    Presets,
    Queue,
    Tasks,
    Episodes,
}

impl PanelKind {
    pub const ALL: [PanelKind; 19] = [
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
        PanelKind::Mixer,
        PanelKind::Presets,
        PanelKind::Queue,
        PanelKind::Tasks,
        PanelKind::Episodes,
    ];

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
            PanelKind::Mixer => "panel.mixer.title",
            PanelKind::Presets => "panel.presets.title",
            PanelKind::Queue => "panel.queue.title",
            PanelKind::Tasks => "panel.tasks.title",
            PanelKind::Episodes => "panel.episodes.title",
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
            PanelKind::Mixer => "panel.mixer.empty",
            PanelKind::Presets => "panel.presets.empty",
            PanelKind::Queue => "panel.queue.empty",
            PanelKind::Tasks => "panel.tasks.empty",
            PanelKind::Episodes => "panel.episodes.empty",
        }
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

/// Layout padrão de um workspace.
fn default_node(ws: WorkspaceId) -> Node {
    use PanelKind as P;
    match ws {
        WorkspaceId::Roteiro => Row(vec![(0.22, Pane(P::Outline)), (0.52, Pane(P::Script)), (0.26, Pane(P::Properties))]),
        WorkspaceId::Personagens => Row(vec![
            (0.22, Pane(P::Characters)),
            (0.50, Pane(P::Sheet)),
            (0.28, Col(vec![(0.42, Pane(P::Palette)), (0.58, Pane(P::Properties))])),
        ]),
        WorkspaceId::Storyboard => Col(vec![
            (0.72, Row(vec![(0.20, Pane(P::Shots)), (0.56, Pane(P::Board)), (0.24, Pane(P::Properties))])),
            (0.28, Pane(P::Timeline)),
        ]),
        WorkspaceId::Layout => Row(vec![(0.72, Pane(P::Canvas)), (0.28, Col(vec![(0.30, Pane(P::Navigator)), (0.70, TabsOf(&[P::Layers, P::Properties]))]))]),
        WorkspaceId::Animacao => Col(vec![
            (0.70, Row(vec![(0.72, Pane(P::Canvas)), (0.28, Col(vec![(0.34, Pane(P::Palette)), (0.66, TabsOf(&[P::Layers, P::Properties]))]))])),
            (0.30, Pane(P::Timeline)),
        ]),
        WorkspaceId::Pintura => Row(vec![
            (0.70, Pane(P::Canvas)),
            (0.30, Col(vec![(0.22, Pane(P::Navigator)), (0.30, Pane(P::Palette)), (0.48, TabsOf(&[P::Layers, P::Properties]))])),
        ]),
        WorkspaceId::Cenario => Row(vec![
            (0.70, Pane(P::Canvas)),
            (0.30, Col(vec![(0.26, Pane(P::Navigator)), (0.22, Pane(P::Palette)), (0.52, TabsOf(&[P::Layers, P::Properties]))])),
        ]),
        WorkspaceId::Composicao => Row(vec![(0.72, Pane(P::Canvas)), (0.28, Col(vec![(0.26, Pane(P::Navigator)), (0.40, Pane(P::Layers)), (0.34, Pane(P::Properties))]))]),
        WorkspaceId::Cor => Row(vec![(0.28, Pane(P::Navigator)), (0.44, Pane(P::Palette)), (0.28, Pane(P::Properties))]),
        WorkspaceId::Audio => Col(vec![
            (0.55, Row(vec![(0.22, Pane(P::Media)), (0.50, Pane(P::Preview)), (0.28, Pane(P::Mixer))])),
            (0.45, Pane(P::Timeline)),
        ]),
        WorkspaceId::Edicao => Col(vec![
            (0.55, Row(vec![(0.22, Pane(P::Media)), (0.50, Pane(P::Preview)), (0.28, Pane(P::Properties))])),
            (0.45, Pane(P::Timeline)),
        ]),
        WorkspaceId::Entrega => Row(vec![(0.24, Pane(P::Presets)), (0.46, Pane(P::Queue)), (0.30, Pane(P::Properties))]),
        WorkspaceId::Producao => Row(vec![(0.22, Pane(P::Episodes)), (0.52, Pane(P::Tasks)), (0.26, Pane(P::Properties))]),
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

/// Árvore padrão do workspace.
pub fn default_tree(ws: WorkspaceId) -> Tree<PanelKind> {
    let mut tiles = Tiles::default();
    let root = build(&mut tiles, &default_node(ws));
    Tree::new(egui::Id::new(("aiai_dock", ws.key())), root, tiles)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{self, Lang};

    #[test]
    fn every_workspace_loads_a_layout_with_unique_panels() {
        for ws in WorkspaceId::ALL {
            let tree = default_tree(ws);
            assert!(tree.root().is_some(), "{ws:?} sem raiz");
            let p = panels(&tree);
            assert!(p.len() >= 3, "{ws:?} com poucos painéis: {p:?}");
            let set: std::collections::HashSet<_> = p.iter().collect();
            assert_eq!(set.len(), p.len(), "{ws:?} repete painel");
            assert!(tree.tiles.len() > p.len(), "{ws:?} sem containers");
        }
    }

    #[test]
    fn canvas_workspaces_have_a_canvas_and_the_others_do_not() {
        for ws in WorkspaceId::ALL {
            let has = panels(&default_tree(ws)).contains(&PanelKind::Canvas);
            assert_eq!(has, ws.has_canvas(), "{ws:?}");
        }
    }

    #[test]
    fn timeline_and_script_placement_follow_the_brief() {
        let has = |ws, k| panels(&default_tree(ws)).contains(&k);
        assert!(has(WorkspaceId::Animacao, PanelKind::Timeline));
        assert!(has(WorkspaceId::Audio, PanelKind::Timeline));
        assert!(has(WorkspaceId::Edicao, PanelKind::Timeline));
        assert!(has(WorkspaceId::Roteiro, PanelKind::Script));
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
    }
}
