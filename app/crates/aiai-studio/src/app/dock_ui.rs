//! Dock: maximizar painel e janelas soltas (viewports nativos do eframe), a cola entre o `StudioApp`, o `egui_tiles` e
//! `dock_float` (dados guardados, monitores, máquina de estados do maximizar; as decisões estão no topo de lá).
//!
//! - Maximizar: o botão ao lado do X de cada painel (ou na barra de abas), o duplo clique na aba/cabeçalho, a tecla `
//!   (padrão do Premiere/After Effects: o painel sob o mouse; Preferências > Atalhos troca) ou Janela > Maximizar
//!   painel. O painel maximizado ocupa toda a área do dock; a árvore não muda (nada a gravar).
//! - Janela solta: "Destacar em janela" (botão, menu de contexto da aba/cabeçalho ou Janela > Destacar painel) tira o
//!   painel, ou o grupo de abas inteiro, da árvore principal e o põe numa janela do sistema com a sua própria árvore
//!   (viewport imediato do egui: o painel desenha com o mesmo `Env` do resto, mesmas texturas da GPU). Fechar a janela
//!   (ou Janela > Reencaixar todas as janelas) devolve os painéis ao lugar de onde saíram.
//! - Os retângulos dos widgets das janelas soltas vão para o `GET_UI_STATE` com o viewport (`float:N`) e o retângulo
//!   na tela; a entrada sintética da bridge (`INPUT`) continua indo só para a janela principal.

use super::{DockBehavior, StudioApp, ui_kit};
use crate::dock_float::{self, DockHost, FloatHome, IRect, Monitor, SavedFloat, SavedMonitor};
use crate::i18n::tr;
use crate::keymap::{Action, Chord};
use crate::layout::{self, PanelKind};
use crate::theme::{CARD_GAP, Tokens};
use aiai_bridge::protocol::BridgeError;
use egui::{Color32, Rect, Response, Sense, Stroke, Ui, UiBuilder};
use egui_tiles::{Behavior, Container, Linear, LinearDir, Tabs, Tile, TileId, Tiles, Tree};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Pedido feito durante o desenho do dock (botão, menu, duplo clique), aplicado depois, fora do empréstimo da árvore.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DockReq {
    /// Maximiza (ou restaura, se já está) o painel no dock dado.
    Maximize(DockHost, PanelKind),
    /// Tira o tile (painel ou grupo de abas) da árvore principal para uma janela.
    Detach(TileId),
    /// Fecha a janela solta e devolve os painéis à principal.
    Redock(u64),
}

/// O que o `DockBehavior` sabe e registra sobre o dock que está desenhando.
pub(crate) struct PaneDock {
    pub host: DockHost,
    pub maxed: Option<PanelKind>,
    pub reqs: Vec<DockReq>,
    /// Painel sob o ponteiro neste quadro (alvo da tecla de maximizar).
    pub hovered: Option<PanelKind>,
    /// Grupo de abas cuja barra está sendo desenhada: durante o desenho o egui_tiles tira o contêiner da lista de tiles,
    /// então a aba não acha o pai dela ("Destacar o grupo" do menu da aba usa este).
    cur_tabs: Option<TileId>,
}

impl PaneDock {
    pub(crate) fn new(host: DockHost, maxed: Option<PanelKind>) -> PaneDock {
        PaneDock { host, maxed, reqs: Vec::new(), hovered: None, cur_tabs: None }
    }

    /// Chamado no começo de cada painel: guarda o que está sob o ponteiro.
    pub(crate) fn note_hover(&mut self, ui: &Ui, rect: Rect, kind: PanelKind) {
        if ui.rect_contains_pointer(rect) {
            self.hovered = Some(kind);
        }
    }
}

/// Uma janela solta em uso.
pub(crate) struct FloatWin {
    pub id: u64,
    pub tree: Tree<PanelKind>,
    /// Canto externo e área cliente, pixels físicos (o que vai para o `layouts.json`).
    pub pos: Option<[i32; 2]>,
    pub size: [i32; 2],
    pub monitor: Option<SavedMonitor>,
    pub home: Option<FloatHome>,
    /// Posição/tamanho em pontos do egui dados ao `ViewportBuilder` na criação, sempre os mesmos depois: o egui só manda
    /// comando à janela quando o builder muda (passar a posição atual brigaria com o arraste do usuário).
    spawn: (Option<egui::Pos2>, egui::Vec2),
    /// Onde a janela tem de ficar (físico): conferido no 3º quadro, quando o sistema já pôs a janela no monitor dela
    /// (monitores com escalas diferentes convertem pontos de jeitos diferentes).
    target: Option<([i32; 2], [i32; 2])>,
    frames: u32,
    close: bool,
    focus: bool,
    /// Canto da área cliente (físico) e pixels por ponto deste quadro (GET_UI_STATE).
    origin: Option<egui::Pos2>,
    ppp: f32,
    focused: bool,
    title: String,
    /// Já é "possuída" pela janela principal (`dock_float::set_owner`).
    owned: bool,
}

impl FloatWin {
    fn viewport_id(&self) -> egui::ViewportId {
        egui::ViewportId::from_hash_of(("aiai_float", self.id))
    }

    fn saved(&self) -> Option<SavedFloat> {
        Some(SavedFloat { tree: layout::saved_from_tree(&self.tree)?, pos: self.pos, size: self.size, monitor: self.monitor.clone(), home: self.home.clone() })
    }

    fn panels(&self) -> Vec<PanelKind> {
        layout::panels(&self.tree)
    }
}

/// Estado do dock fora das árvores: maximizado, janelas soltas, monitores e os retângulos desenhados nas janelas.
#[derive(Default)]
pub(crate) struct DockState {
    pub max: dock_float::Maximized,
    floats: Vec<FloatWin>,
    /// Layout (`LayoutStore::slot`) cujas janelas estão carregadas.
    slot: Option<String>,
    next_id: u64,
    monitors: Vec<Monitor>,
    monitors_at: Option<Instant>,
    /// Pedidos feitos dentro das janelas soltas (aplicados depois que todas desenharam).
    pending: Vec<DockReq>,
    /// Retângulos dos widgets das janelas soltas: nome → (retângulo em pontos da janela, id da janela).
    rects_cur: BTreeMap<String, (Rect, u64)>,
    rects_prev: BTreeMap<String, (Rect, u64)>,
    /// Canto da área cliente da janela principal (físico) e pixels por ponto dela.
    main_origin: Option<egui::Pos2>,
    main_ppp: f32,
    icon: Option<Arc<egui::IconData>>,
}

impl DockState {
    fn refresh_monitors(&mut self, force: bool) {
        if force || self.monitors_at.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) {
            self.monitors = dock_float::monitors();
            self.monitors_at = Some(Instant::now());
        }
    }

    fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Cria a janela: escolhe monitor/posição (`dock_float::place`) e converte para os pontos que o egui quer.
    #[allow(clippy::too_many_arguments)]
    fn make(&mut self, tree: Tree<PanelKind>, id: u64, pos: Option<[i32; 2]>, size: [i32; 2], monitor: Option<SavedMonitor>, home: Option<FloatHome>, zoom: f32, fallback_scale: f32, whole: bool) -> FloatWin {
        let placed = dock_float::place(pos, size, monitor.as_ref(), &self.monitors, whole);
        let (pos, size, scale) = match &placed {
            Some(p) => (Some(p.pos), p.size, self.monitors[p.monitor].scale),
            None => (pos, size, fallback_scale),
        };
        if placed.as_ref().is_some_and(|p| p.moved) {
            tracing::info!("janela solta {id}: o monitor guardado não está disponível ou a janela estava fora da tela; trazida para {:?} {:?}", pos, size);
        }
        let ppp = (zoom * scale).max(0.1);
        let spawn = (pos.map(|p| egui::pos2(p[0] as f32 / ppp, p[1] as f32 / ppp)), egui::vec2(size[0] as f32 / ppp, size[1] as f32 / ppp));
        let monitor = placed.map(|p| SavedMonitor { name: self.monitors[p.monitor].name.clone(), work: self.monitors[p.monitor].work });
        FloatWin { id, tree, pos, size, monitor, home, spawn, target: pos.map(|p| (p, size)), frames: 0, close: false, focus: true, origin: None, ppp, focused: false, title: String::new(), owned: false }
    }
}

fn float_tree_id(id: u64) -> egui::Id {
    egui::Id::new(("aiai_float_tree", id))
}

// ------------------------------------------------------------------------------------------ árvore: copiar, tirar, pôr

/// Copia o tile `id` de `src` (com tudo o que está dentro, proporções, aba ativa e escondidos) para `dst`, com ids novos.
fn graft(dst: &mut Tiles<PanelKind>, src: &Tiles<PanelKind>, id: TileId) -> Option<TileId> {
    let new = match src.get(id)? {
        Tile::Pane(k) => dst.insert_pane(*k),
        Tile::Container(c) => {
            let kids: Vec<(TileId, TileId)> = c.children().filter_map(|k| Some((*k, graft(dst, src, *k)?))).collect();
            if kids.is_empty() {
                return None;
            }
            let ids: Vec<TileId> = kids.iter().map(|k| k.1).collect();
            let map = |o: TileId| kids.iter().find(|(a, _)| *a == o).map(|(_, b)| *b);
            let nc = match c {
                Container::Tabs(t) => {
                    let mut n = Tabs::new(ids);
                    if let Some(a) = t.active.and_then(map) {
                        n.set_active(a);
                    }
                    Container::Tabs(n)
                }
                Container::Linear(l) => {
                    let mut n = Linear::new(l.dir, ids);
                    for (o, nw) in &kids {
                        n.shares.set_share(*nw, l.shares[*o]);
                    }
                    Container::Linear(n)
                }
                Container::Grid(_) => Container::new_grid(ids),
            };
            dst.insert_container(nc)
        }
    };
    if !src.is_visible(id) {
        dst.set_visible(new, false);
    }
    Some(new)
}

/// Tira um tile da árvore (e da lista do pai); a raiz deixa a árvore vazia.
fn remove_tile(tree: &mut Tree<PanelKind>, id: TileId) {
    if tree.root == Some(id) {
        tree.root = None;
    }
    tree.remove_recursively(id);
}

/// Primeiro painel dentro de um tile (em profundidade).
fn first_pane(tiles: &Tiles<PanelKind>, id: TileId) -> Option<(TileId, PanelKind)> {
    match tiles.get(id)? {
        Tile::Pane(k) => Some((id, *k)),
        Tile::Container(c) => c.children().find_map(|k| first_pane(tiles, *k)),
    }
}

/// Todos os painéis dentro de um tile.
fn panes_under(tiles: &Tiles<PanelKind>, id: TileId, out: &mut Vec<PanelKind>) {
    match tiles.get(id) {
        Some(Tile::Pane(k)) => out.push(*k),
        Some(Tile::Container(c)) => c.children().for_each(|k| panes_under(tiles, *k, out)),
        None => {}
    }
}

/// Onde o tile está, para voltar depois: aba junto de um vizinho, ou ao lado do bloco vizinho numa linha/coluna.
fn home_of(tree: &Tree<PanelKind>, id: TileId) -> Option<FloatHome> {
    let parent = tree.tiles.parent_of(id)?;
    match tree.tiles.get(parent)? {
        Tile::Container(Container::Tabs(t)) => {
            let mate = t.children.iter().copied().filter(|c| *c != id).find_map(|c| first_pane(&tree.tiles, c))?;
            let index = t.children.iter().position(|c| *c == id).unwrap_or(0);
            Some(FloatHome::Tab { anchor: mate.1.key().to_owned(), index })
        }
        Tile::Container(Container::Linear(l)) => {
            let i = l.children.iter().position(|c| *c == id)?;
            let total: f32 = l.children.iter().map(|c| l.shares[*c]).sum::<f32>().max(1e-6);
            let frac = (l.shares[id] / total).clamp(0.05, 0.9);
            let (nb, after) = if i > 0 { (l.children[i - 1], true) } else { (*l.children.get(i + 1)?, false) };
            let mut block = Vec::new();
            panes_under(&tree.tiles, nb, &mut block);
            if block.is_empty() {
                return None;
            }
            let block: Vec<String> = block.into_iter().map(|k| k.key().to_owned()).collect();
            Some(if l.dir == LinearDir::Horizontal { FloatHome::Row { block, after, frac } } else { FloatHome::Col { block, after, frac } })
        }
        _ => None,
    }
}

/// Põe `new` (já em `tree.tiles`, solto) de volta no lugar guardado. Devolve se conseguiu.
fn place_home(tree: &mut Tree<PanelKind>, new: TileId, home: Option<&FloatHome>) -> bool {
    let Some(home) = home else { return false };
    let (keys, dir, after, frac): (Vec<&String>, _, _, _) = match home {
        FloatHome::Tab { anchor, .. } => (vec![anchor], None, false, 0.0),
        FloatHome::Row { block, after, frac } => (block.iter().collect(), Some(LinearDir::Horizontal), *after, *frac),
        FloatHome::Col { block, after, frac } => (block.iter().collect(), Some(LinearDir::Vertical), *after, *frac),
    };
    // os painéis do bloco vizinho que ainda estão na árvore (o primeiro é o ponto de partida)
    let present: Vec<PanelKind> = keys.iter().filter_map(|k| PanelKind::from_key(k)).filter(|k| layout::find_pane(tree, *k).is_some()).collect();
    let Some(a) = present.first().and_then(|k| layout::find_pane(tree, *k)) else { return false };
    match dir {
        None => {
            // grupo de abas que volta para junto de outro grupo: as abas entram no grupo (sem abas dentro de abas)
            let kids: Vec<TileId> = match tree.tiles.get(new) {
                Some(Tile::Container(Container::Tabs(t))) => t.children.clone(),
                _ => vec![new],
            };
            let parent = tree.tiles.parent_of(a);
            let tabs_id = match parent {
                Some(p) if matches!(tree.tiles.get(p), Some(Tile::Container(Container::Tabs(_)))) => p,
                _ => {
                    let t = tree.tiles.insert_container(Container::Tabs(Tabs::new(vec![a])));
                    match parent {
                        Some(p) => {
                            if let Some(Tile::Container(c)) = tree.tiles.get_mut(p) {
                                let _ = c.replace_child(a, t);
                            }
                        }
                        None => tree.root = Some(t),
                    }
                    t
                }
            };
            if kids.len() > 1 || kids[0] != new {
                tree.tiles.remove(new);
            }
            let at = match home {
                FloatHome::Tab { index, .. } => *index,
                _ => usize::MAX,
            };
            if let Some(Tile::Container(Container::Tabs(t))) = tree.tiles.get_mut(tabs_id) {
                for (n, k) in kids.iter().enumerate() {
                    let i = at.saturating_add(n).min(t.children.len());
                    t.children.insert(i, *k);
                }
                t.set_active(kids[0]);
            }
            true
        }
        Some(dir) => {
            // o bloco vizinho = o menor pedaço da árvore com todos os painéis dele; filho de uma linha/coluna na mesma
            // direção: o painel entra ao lado dele; senão (a linha/coluna sumiu ao ficar com um filho só) o bloco é
            // embrulhado numa linha/coluna nova com o painel
            let contains_all = |tree: &Tree<PanelKind>, id: TileId| {
                let mut under = Vec::new();
                panes_under(&tree.tiles, id, &mut under);
                present.iter().all(|k| under.contains(k))
            };
            let mut cur = a;
            while !contains_all(tree, cur) {
                match tree.tiles.parent_of(cur) {
                    Some(p) => cur = p,
                    None => break,
                }
            }
            let frac = frac.clamp(0.05, 0.9);
            let parent = tree.tiles.parent_of(cur);
            if let Some(Tile::Container(Container::Linear(l))) = parent.and_then(|p| tree.tiles.get_mut(p)) {
                if l.dir == dir {
                    let total: f32 = l.children.iter().map(|c| l.shares[*c]).sum::<f32>().max(0.001);
                    let i = l.children.iter().position(|c| *c == cur).unwrap_or(0) + usize::from(after);
                    l.children.insert(i.min(l.children.len()), new);
                    l.shares.set_share(new, total * frac / (1.0 - frac));
                    return true;
                }
            }
            let kids = if after { vec![cur, new] } else { vec![new, cur] };
            let mut lin = Linear::new(dir, kids);
            lin.shares.set_share(new, frac);
            lin.shares.set_share(cur, 1.0 - frac);
            let wrap = tree.tiles.insert_container(Container::Linear(lin));
            match parent {
                Some(p) => {
                    if let Some(Tile::Container(c)) = tree.tiles.get_mut(p) {
                        let _ = c.replace_child(cur, wrap);
                    }
                }
                None => tree.root = Some(wrap),
            }
            true
        }
    }
}

/// Sem lugar guardado: um painel entra pelo catálogo (`insert_panel`: aba da mesma categoria ou o lado do `dock_hint`);
/// um grupo entra numa coluna à direita.
fn place_fallback(tree: &mut Tree<PanelKind>, new: TileId) {
    match (tree.root, tree.tiles.get(new)) {
        (None, _) => tree.root = Some(new),
        (Some(_), Some(Tile::Pane(k))) => {
            let k = *k;
            let hidden = !tree.tiles.is_visible(new);
            tree.tiles.remove(new);
            let id = layout::insert_panel(tree, k);
            if hidden {
                tree.tiles.set_visible(id, false);
            }
        }
        (Some(root), _) => {
            let mut lin = Linear::new(LinearDir::Horizontal, vec![root, new]);
            lin.shares.set_share(root, 0.75);
            lin.shares.set_share(new, 0.25);
            tree.root = Some(tree.tiles.insert_container(Container::Linear(lin)));
        }
    }
}

/// O tile está visível de verdade (ele e todos os de cima)?
fn shown(tree: &Tree<PanelKind>, id: TileId) -> bool {
    let mut cur = Some(id);
    while let Some(c) = cur {
        if !tree.tiles.is_visible(c) {
            return false;
        }
        cur = tree.tiles.parent_of(c);
    }
    true
}

fn visible_panes(tree: &Tree<PanelKind>) -> Vec<(TileId, PanelKind)> {
    let mut out: Vec<(TileId, PanelKind)> = tree.tiles.iter().filter_map(|(id, t)| match t {
        Tile::Pane(k) if shown(tree, *id) => Some((*id, *k)),
        _ => None,
    }).collect();
    out.sort_by_key(|(_, k)| PanelKind::ALL.iter().position(|x| x == k));
    out
}

// ------------------------------------------------------------------------------------------ desenho

/// A árvore no `ui`, ou só o painel maximizado ocupando toda a área.
pub(crate) fn tree_ui(tree: &mut Tree<PanelKind>, b: &mut DockBehavior<'_>, ui: &mut Ui) {
    let maxed = b.dock.maxed.and_then(|k| layout::find_pane(tree, k).map(|id| (id, k))).filter(|(id, _)| shown(tree, *id));
    if let Some((id, kind)) = maxed {
        let rect = ui.available_rect_before_wrap();
        // o mesmo id que o egui_tiles daria ao painel no layout normal (raiz → … → painel): rolagens e estados internos
        // continuam ao maximizar e restaurar
        let mut path = vec![id];
        let mut cur = id;
        while let Some(p) = tree.tiles.parent_of(cur) {
            path.push(p);
            cur = p;
        }
        let mut uid = ui.id();
        for t in path.iter().rev() {
            uid = uid.with(*t);
        }
        let mut child = Ui::new(ui.ctx().clone(), uid, UiBuilder::new().layer_id(ui.layer_id()).max_rect(rect));
        b.in_tabs.remove(&id);
        let mut pane = kind;
        let _ = b.pane_ui(&mut child, id, &mut pane);
        ui.advance_cursor_after_rect(rect);
        return;
    }
    if visible_panes(tree).is_empty() {
        let r = ui.available_rect_before_wrap();
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, tr("panel.all_floating"), egui::FontId::proportional(13.0), b.t.text_dim);
    }
    tree.ui(b, ui);
}

#[derive(Clone, Copy)]
enum Glyph {
    Maximize,
    Restore,
    Detach,
    Redock,
}

fn paint_glyph(p: &egui::Painter, r: Rect, g: Glyph, col: Color32) {
    let s = Stroke::new(1.3, col);
    let c = r.center();
    let box_at = |min: egui::Pos2, size: f32| Rect::from_min_size(min, egui::vec2(size, size));
    match g {
        Glyph::Maximize => {
            p.rect_stroke(Rect::from_center_size(c, egui::vec2(9.0, 9.0)), 0.0, s, egui::StrokeKind::Middle);
        }
        Glyph::Restore => {
            let front = box_at(c + egui::vec2(-5.0, -2.0), 7.0);
            p.rect_stroke(front, 0.0, s, egui::StrokeKind::Middle);
            // a de trás só aparece onde a da frente não cobre
            let (x0, y0) = (c.x - 2.0, c.y - 5.0);
            p.line_segment([egui::pos2(x0, y0 + 3.0), egui::pos2(x0, y0)], s);
            p.line_segment([egui::pos2(x0, y0), egui::pos2(x0 + 7.0, y0)], s);
            p.line_segment([egui::pos2(x0 + 7.0, y0), egui::pos2(x0 + 7.0, y0 + 7.0)], s);
            p.line_segment([egui::pos2(x0 + 7.0, y0 + 7.0), egui::pos2(x0 + 4.0, y0 + 7.0)], s);
        }
        Glyph::Detach | Glyph::Redock => {
            // caixa embaixo à esquerda e seta para fora (destacar) ou para dentro (reencaixar)
            let bx = box_at(c + egui::vec2(-5.0, -2.0), 7.0);
            p.rect_stroke(bx, 0.0, s, egui::StrokeKind::Middle);
            let (a, b) = (c + egui::vec2(-0.5, 0.5), c + egui::vec2(5.0, -5.0));
            let (from, to) = if matches!(g, Glyph::Detach) { (a, b) } else { (b, a) };
            p.line_segment([from, to], s);
            let d = (to - from).normalized();
            let n = egui::vec2(-d.y, d.x);
            p.line_segment([to, to - d * 3.0 + n * 3.0], s);
            p.line_segment([to, to - d * 3.0 - n * 3.0], s);
        }
    }
}

/// Botão pequeno com desenho (mesmo estilo do X de fechar do cabeçalho).
fn glyph_button(ui: &mut Ui, t: &Tokens, rect: Rect, id: egui::Id, g: Glyph, accent: bool, tip: &str) -> Response {
    let r = ui.interact(rect, id, Sense::click());
    if r.hovered() {
        ui.painter().rect_filled(rect, 3.0, t.bg_hover);
    }
    let col = if accent { t.accent } else if r.hovered() { t.text } else { t.text_dim };
    paint_glyph(ui.painter(), rect, g, col);
    r.on_hover_text(tip)
}

fn maximize_tip(b: &DockBehavior<'_>, maxed: bool) -> String {
    let key = b.env.ed.keys.label(Action::PanelMaximize);
    let base = tr(if maxed { "panel.restore" } else { "panel.maximize" });
    if key.is_empty() { base.to_owned() } else { format!("{base} ({key})") }
}

/// Menu de contexto de um painel (cabeçalho ou aba): maximizar/restaurar, destacar (o painel ou o grupo) ou reencaixar.
fn pane_menu(b: &mut DockBehavior<'_>, ui: &mut Ui, kind: PanelKind, tile: TileId, group: Option<TileId>) {
    let host = b.dock.host;
    let maxed = b.dock.maxed == Some(kind);
    let key = b.env.ed.keys.label(Action::PanelMaximize);
    let r = ui.add(egui::Button::new(tr(if maxed { "panel.restore" } else { "panel.maximize" })).shortcut_text(key));
    b.env.note("dock_menu:maximize", r.rect);
    if r.clicked() {
        b.dock.reqs.push(DockReq::Maximize(host, kind));
        ui.close();
    }
    match host {
        DockHost::Main => {
            let r = ui.button(tr("panel.detach"));
            b.env.note("dock_menu:detach", r.rect);
            if r.clicked() {
                b.dock.reqs.push(DockReq::Detach(tile));
                ui.close();
            }
            if let Some(g) = group {
                let r = ui.button(tr("panel.detach_group"));
                b.env.note("dock_menu:detach_group", r.rect);
                if r.clicked() {
                    b.dock.reqs.push(DockReq::Detach(g));
                    ui.close();
                }
            }
        }
        DockHost::Float(id) => {
            let r = ui.button(tr("panel.redock"));
            b.env.note("dock_menu:redock", r.rect);
            if r.clicked() {
                b.dock.reqs.push(DockReq::Redock(id));
                ui.close();
            }
        }
    }
}

/// Cabeçalho de painel sem abas: maximizar e destacar/reencaixar à esquerda do X; duplo clique maximiza; botão
/// direito abre o menu do painel.
pub(crate) fn header_extras(b: &mut DockBehavior<'_>, ui: &mut Ui, hdr: Rect, kind: PanelKind, tile_id: TileId, resp: &Response) {
    let t = b.t;
    let host = b.dock.host;
    let maxed = b.dock.maxed == Some(kind);
    // o X (quando existe) ocupa o canto: os botões vêm antes dele
    let mut x = hdr.right() - 12.0 - if kind != PanelKind::Canvas { 20.0 } else { 0.0 };
    let tip = maximize_tip(b, maxed);
    let r = Rect::from_center_size(egui::pos2(x, hdr.center().y), egui::vec2(18.0, 18.0));
    b.env.note(format!("panel_max:{}", kind.key()), r);
    let gid = ui.id().with(("dock_max", tile_id));
    if glyph_button(ui, &t, r, gid, if maxed { Glyph::Restore } else { Glyph::Maximize }, maxed, &tip).clicked() {
        b.dock.reqs.push(DockReq::Maximize(host, kind));
    }
    x -= 20.0;
    let r = Rect::from_center_size(egui::pos2(x, hdr.center().y), egui::vec2(18.0, 18.0));
    match host {
        DockHost::Main => {
            b.env.note(format!("panel_detach:{}", kind.key()), r);
            let gid = ui.id().with(("dock_detach", tile_id));
            if glyph_button(ui, &t, r, gid, Glyph::Detach, false, tr("panel.detach")).clicked() {
                b.dock.reqs.push(DockReq::Detach(tile_id));
            }
        }
        DockHost::Float(id) => {
            b.env.note(format!("panel_redock:{}", kind.key()), r);
            let gid = ui.id().with(("dock_redock", tile_id));
            if glyph_button(ui, &t, r, gid, Glyph::Redock, false, tr("panel.redock")).clicked() {
                b.dock.reqs.push(DockReq::Redock(id));
            }
        }
    }
    if resp.double_clicked() {
        b.dock.reqs.push(DockReq::Maximize(host, kind));
    }
    resp.context_menu(|ui| pane_menu(b, ui, kind, tile_id, None));
}

/// Canto direito da barra de abas: maximizar a aba ativa e destacar o grupo inteiro (ou reencaixar a janela).
pub(crate) fn tab_bar_buttons(b: &mut DockBehavior<'_>, tiles: &Tiles<PanelKind>, ui: &mut Ui, tile_id: TileId, tabs: &Tabs) {
    b.dock.cur_tabs = Some(tile_id);
    let Some((_, kind)) = tabs.active.and_then(|a| first_pane(tiles, a)) else { return };
    let t = b.t;
    let host = b.dock.host;
    let maxed = b.dock.maxed == Some(kind);
    ui.add_space(4.0);
    let tip = maximize_tip(b, maxed);
    let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
    b.env.note(format!("tabs_max:{}", kind.key()), r);
    let gid = ui.id().with(("dock_tabs_max", tile_id));
    if glyph_button(ui, &t, r, gid, if maxed { Glyph::Restore } else { Glyph::Maximize }, maxed, &tip).clicked() {
        b.dock.reqs.push(DockReq::Maximize(host, kind));
    }
    let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
    match host {
        DockHost::Main => {
            b.env.note(format!("tabs_detach:{}", kind.key()), r);
            let gid = ui.id().with(("dock_tabs_detach", tile_id));
            if glyph_button(ui, &t, r, gid, Glyph::Detach, false, tr("panel.detach_group")).clicked() {
                b.dock.reqs.push(DockReq::Detach(tile_id));
            }
        }
        DockHost::Float(id) => {
            b.env.note(format!("tabs_redock:{}", kind.key()), r);
            let gid = ui.id().with(("dock_tabs_redock", tile_id));
            if glyph_button(ui, &t, r, gid, Glyph::Redock, false, tr("panel.redock")).clicked() {
                b.dock.reqs.push(DockReq::Redock(id));
            }
        }
    }
}

/// Cada aba: duplo clique maximiza, botão direito abre o menu (com "destacar o grupo"), e o retângulo vai para o
/// `GET_UI_STATE` (`tab:<painel>`).
pub(crate) fn on_tab_button(b: &mut DockBehavior<'_>, tiles: &Tiles<PanelKind>, tile_id: TileId, resp: Response) -> Response {
    if let Some(Tile::Pane(kind)) = tiles.get(tile_id) {
        let kind = *kind;
        b.env.note(format!("tab:{}", kind.key()), resp.rect);
        if resp.double_clicked() {
            b.dock.reqs.push(DockReq::Maximize(b.dock.host, kind));
        }
        let group = tiles.parent_of(tile_id).or(b.dock.cur_tabs);
        resp.context_menu(|ui| pane_menu(b, ui, kind, tile_id, group));
    }
    resp
}

// ------------------------------------------------------------------------------------------ StudioApp

impl StudioApp {
    /// Chave do layout em uso do workspace atual.
    fn dock_slot(&self) -> String {
        self.layouts.slot(self.ws)
    }

    /// Começo do quadro do dock: carrega as janelas soltas do layout em uso (trocou de workspace ou de variante) e
    /// garante que um painel de janela solta não aparece também na principal.
    pub(crate) fn dock_sync(&mut self) {
        let ws = self.ws;
        let slot = self.dock_slot();
        if self.dock.slot.as_deref() != Some(slot.as_str()) {
            for f in std::mem::take(&mut self.dock.floats) {
                self.dock.max.forget(DockHost::Float(f.id));
            }
            self.dock.refresh_monitors(true);
            let saved = self.layouts.get(ws).map(|e| e.floating.clone()).unwrap_or_default();
            let mut taken = HashSet::new();
            let zoom = self.prefs.ui_zoom;
            for s in saved {
                let id = self.dock.new_id();
                let Some(tree) = layout::float_tree_from_saved(ws, &s.tree, float_tree_id(id), &taken) else {
                    tracing::info!("layout de {slot}: janela solta sem painel conhecido descartada");
                    continue;
                };
                taken.extend(layout::panels(&tree));
                let mut f = self.dock.make(tree, id, s.pos, s.size, s.monitor, s.home, zoom, 1.0, false);
                f.focus = false;
                self.dock.floats.push(f);
            }
            self.dock.slot = Some(slot);
        }
        self.ensure_tree(ws);
        if let Some(tree) = self.trees.get_mut(&ws) {
            for f in &self.dock.floats {
                for k in f.panels() {
                    if let Some(id) = layout::find_pane(tree, k) {
                        remove_tile(tree, id);
                    }
                }
            }
        }
    }

    /// O layout do workspace foi trocado por inteiro (fábrica, layout com nome): as janelas soltas de agora somem e as
    /// do layout novo (as guardadas no `LayoutStore`) entram no próximo quadro.
    pub(crate) fn dock_reload(&mut self, ws: crate::workspace::WorkspaceId) {
        if ws == self.ws {
            for f in std::mem::take(&mut self.dock.floats) {
                self.dock.max.forget(DockHost::Float(f.id));
            }
            self.dock.slot = None;
        }
    }

    /// Layout com nome salvo agora: leva as janelas soltas junto.
    pub(crate) fn dock_save_named(&mut self, name: &str) {
        let ws = self.ws;
        let fl = self.layouts.get(ws).map(|e| e.floating.clone()).unwrap_or_default();
        if let Some(n) = self.layouts.entry(ws).named.iter_mut().find(|n| n.name == name) {
            n.floating = fl;
        }
    }

    /// Layout com nome aplicado agora: as janelas soltas dele passam a ser as do layout em uso.
    pub(crate) fn dock_apply_named(&mut self, name: &str) {
        let ws = self.ws;
        let fl = self.layouts.get(ws).and_then(|e| e.named.iter().find(|n| n.name == name)).map(|n| n.floating.clone()).unwrap_or_default();
        self.layouts.entry(ws).floating = fl;
        self.dock_reload(ws);
    }

    /// O que o `DockBehavior` da janela principal precisa (dock, painel maximizado válido).
    pub(crate) fn dock_main_pane(&mut self) -> PaneDock {
        let slot = self.dock_slot();
        if let Some(tree) = self.trees.get(&self.ws) {
            self.dock.max.validate(&slot, DockHost::Main, |k| layout::find_pane(tree, k).is_some_and(|id| shown(tree, id)));
        }
        PaneDock::new(DockHost::Main, self.dock.max.get(&slot, DockHost::Main))
    }

    /// Depois de desenhar a árvore principal: canto da janela (para posicionar janelas novas), pedidos e a tecla de
    /// maximizar.
    pub(crate) fn dock_main_after(&mut self, ctx: &egui::Context, pd: PaneDock) {
        let ppp = ctx.pixels_per_point();
        self.dock.main_ppp = ppp;
        self.dock.main_origin = ctx.input(|i| i.viewport().inner_rect).map(|r| egui::pos2(r.min.x * ppp, r.min.y * ppp));
        let PaneDock { host, reqs, hovered, .. } = pd;
        for r in reqs {
            self.dock_apply(r);
        }
        if self.dock_key_pressed(ctx) {
            self.dock_toggle_key(host, hovered);
        }
    }

    /// A tecla de maximizar (`Action::PanelMaximize`, padrão `). Com o padrão, vale também a tecla física do ` do teclado
    /// americano (à esquerda do 1): no ABNT2 o ` é tecla morta e ali fica o ', e é a posição que o Premiere usa.
    fn dock_key_pressed(&self, ctx: &egui::Context) -> bool {
        if self.key_capture.is_some() || ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) {
            return false;
        }
        let Some(c) = self.ed.keys.get(Action::PanelMaximize) else { return false };
        ctx.input_mut(|i| {
            if c.take(i) {
                return true;
            }
            if c == Chord::plain(egui::Key::Backtick) && i.modifiers.is_none() {
                let hit = i.events.iter().position(|e| matches!(e, egui::Event::Key { physical_key: Some(egui::Key::Backtick), pressed: true, repeat: false, .. }));
                if let Some(ix) = hit {
                    i.events.remove(ix);
                    return true;
                }
            }
            false
        })
    }

    /// A tecla: o painel sob o mouse maximiza (ou restaura, se já é ele); fora de qualquer painel, restaura.
    fn dock_toggle_key(&mut self, host: DockHost, hovered: Option<PanelKind>) {
        let slot = self.dock_slot();
        match hovered.or(self.dock.max.get(&slot, host)) {
            Some(k) => {
                self.dock.max.toggle(&slot, host, k);
            }
            None => {
                self.dock.max.restore(&slot, host);
            }
        }
    }

    fn dock_apply(&mut self, r: DockReq) {
        match r {
            DockReq::Maximize(host, kind) => {
                let slot = self.dock_slot();
                self.dock.max.toggle(&slot, host, kind);
            }
            DockReq::Detach(tile) => {
                self.dock_detach(tile);
            }
            DockReq::Redock(id) => self.dock_redock(id),
        }
    }

    /// Tira o tile (painel ou grupo de abas) da árvore principal para uma janela nova, no lugar onde ele estava na tela.
    pub(crate) fn dock_detach(&mut self, tile: TileId) -> Option<u64> {
        let ws = self.ws;
        self.ensure_tree(ws);
        let zoom = self.prefs.ui_zoom;
        let (origin, mppp) = (self.dock.main_origin, self.dock.main_ppp.max(0.1));
        let tree = self.trees.get_mut(&ws)?;
        tree.tiles.get(tile)?;
        let home = home_of(tree, tile);
        let rect = tree.tiles.rect(tile);
        let id = self.dock.new_id();
        let mut tiles = Tiles::default();
        let root = graft(&mut tiles, &tree.tiles, tile)?;
        let ftree = Tree::new(float_tree_id(id), root, tiles);
        remove_tile(tree, tile);
        // a janela nasce sobre o lugar do painel, um pouco deslocada (mostra que saiu); tamanho do painel na tela
        let native = mppp / zoom.max(0.1);
        let (pos, size) = match (origin, rect) {
            (Some(o), Some(r)) => (
                Some([(o.x + r.min.x * mppp + 24.0 * native) as i32, (o.y + r.min.y * mppp + 24.0 * native) as i32]),
                [(r.width() * mppp).max(480.0 * native) as i32, (r.height() * mppp).max(320.0 * native) as i32],
            ),
            _ => (None, [(900.0 * native) as i32, (560.0 * native) as i32]),
        };
        self.dock.refresh_monitors(true);
        let f = self.dock.make(ftree, id, pos, size, None, home, zoom, native, true);
        let names: Vec<&str> = f.panels().into_iter().map(|k| tr(k.title_key())).collect();
        self.notice(format!("{} {}", tr("msg.dock_detached"), names.join(", ")));
        self.dock.floats.push(f);
        Some(id)
    }

    /// Fecha a janela solta e devolve a árvore dela à principal, no lugar de onde saiu.
    pub(crate) fn dock_redock(&mut self, id: u64) {
        let Some(ix) = self.dock.floats.iter().position(|f| f.id == id) else { return };
        let f = self.dock.floats.remove(ix);
        self.dock.max.forget(DockHost::Float(id));
        let ws = self.ws;
        self.ensure_tree(ws);
        let Some(tree) = self.trees.get_mut(&ws) else { return };
        let Some(root) = f.tree.root else { return };
        for k in f.panels() {
            if let Some(old) = layout::find_pane(tree, k) {
                remove_tile(tree, old);
            }
        }
        let Some(new) = graft(&mut tree.tiles, &f.tree.tiles, root) else { return };
        let first = first_pane(&tree.tiles, new).map(|(_, k)| k);
        if !place_home(tree, new, f.home.as_ref()) {
            place_fallback(tree, new);
        }
        if let Some(k) = first {
            tree.make_active(|_, t| matches!(t, Tile::Pane(p) if *p == k));
        }
    }

    /// Janela > Reencaixar todas as janelas.
    pub(crate) fn dock_redock_all(&mut self) -> usize {
        let ids: Vec<u64> = self.dock.floats.iter().map(|f| f.id).collect();
        for id in &ids {
            self.dock_redock(*id);
        }
        ids.len()
    }

    /// Janela solta que tem o painel.
    fn dock_float_of(&self, kind: PanelKind) -> Option<u64> {
        self.dock.floats.iter().find(|f| layout::find_pane(&f.tree, kind).is_some()).map(|f| f.id)
    }

    /// O painel está numa janela solta (visível nela)?
    pub(crate) fn dock_floating(&self, kind: PanelKind) -> bool {
        self.dock.floats.iter().any(|f| layout::find_pane(&f.tree, kind).is_some_and(|id| shown(&f.tree, id)))
    }

    /// Pedido de painel (aba da frente) que está numa janela solta: a aba fica ativa lá e a janela vem para a frente.
    pub(crate) fn dock_focus_floating(&mut self, kind: PanelKind) -> bool {
        let Some(f) = self.dock.floats.iter_mut().find(|f| layout::find_pane(&f.tree, kind).is_some()) else { return false };
        f.tree.make_active(|_, t| matches!(t, Tile::Pane(p) if *p == kind));
        f.focus = true;
        true
    }

    /// Exibir > Painéis com um painel de janela solta: mostrar = a janela vem para a frente; esconder = o painel volta
    /// escondido para a principal (a janela fecha se ficar vazia). `None` = o painel não está em janela solta.
    pub(crate) fn dock_set_visible(&mut self, kind: PanelKind, on: bool) -> Option<Result<bool, String>> {
        let fid = self.dock_float_of(kind)?;
        if on {
            if let Some(f) = self.dock.floats.iter_mut().find(|f| f.id == fid) {
                if let Some(id) = layout::find_pane(&f.tree, kind) {
                    f.tree.tiles.set_visible(id, true);
                }
            }
            self.dock_focus_floating(kind);
            return Some(Ok(true));
        }
        let ws = self.ws;
        let f = self.dock.floats.iter_mut().find(|f| f.id == fid)?;
        if let Some(id) = layout::find_pane(&f.tree, kind) {
            remove_tile(&mut f.tree, id);
        }
        let empty = layout::panels(&f.tree).is_empty();
        if empty {
            self.dock.floats.retain(|f| f.id != fid);
            self.dock.max.forget(DockHost::Float(fid));
        }
        self.ensure_tree(ws);
        if let Some(tree) = self.trees.get_mut(&ws) {
            let id = layout::insert_panel(tree, kind);
            tree.tiles.set_visible(id, false);
        }
        Some(Ok(false))
    }

    /// Desenha as janelas soltas (uma por viewport imediato), aplica os pedidos feitos nelas, reencaixa as que o usuário
    /// fechou e espelha posição/tamanho/monitor no `LayoutStore` (gravado no `layouts.json` como o resto do layout).
    pub(crate) fn dock_floats_ui(&mut self, ctx: &egui::Context) {
        self.dock.rects_prev = std::mem::take(&mut self.dock.rects_cur);
        if !self.dock.floats.is_empty() {
            self.dock.refresh_monitors(false);
        }
        let icon = self.dock.icon.get_or_insert_with(|| Arc::new(crate::brand::window_icon())).clone();
        let mut floats = std::mem::take(&mut self.dock.floats);
        for f in floats.iter_mut() {
            if f.close {
                continue;
            }
            let names: Vec<&str> = f.panels().into_iter().filter(|k| layout::find_pane(&f.tree, *k).is_some_and(|id| shown(&f.tree, id))).map(|k| tr(k.title_key())).collect();
            f.title = format!("{} - {}", names.join(" · "), crate::brand::APP_NAME);
            let min = egui::vec2(dock_float::MIN_W as f32, dock_float::MIN_H as f32);
            let mut b = egui::ViewportBuilder::default().with_title(f.title.clone()).with_inner_size(f.spawn.1).with_min_inner_size(min).with_icon(icon.clone());
            if let Some(p) = f.spawn.0 {
                b = b.with_position(p);
            }
            let vid = f.viewport_id();
            ctx.show_viewport_immediate(vid, b, |ui, _class| self.dock_float_frame(ui, f));
            if std::mem::take(&mut f.focus) {
                ctx.send_viewport_cmd_to(vid, egui::ViewportCommand::Focus);
            }
        }
        let closing: Vec<u64> = floats.iter().filter(|f| f.close).map(|f| f.id).collect();
        // pedidos feitos com as janelas fora de `self.dock` entram agora (as janelas novas criadas por eles ficam)
        floats.append(&mut self.dock.floats);
        self.dock.floats = floats;
        for id in closing {
            self.dock_redock(id);
        }
        for r in std::mem::take(&mut self.dock.pending) {
            self.dock_apply(r);
        }
        self.dock_mirror();
    }

    /// Um quadro de uma janela solta.
    fn dock_float_frame(&mut self, ui: &mut Ui, f: &mut FloatWin) {
        let ctx = ui.ctx().clone();
        let t = self.tokens();
        let (info, ppp) = ctx.input(|i| (i.viewport().clone(), i.pixels_per_point()));
        if info.close_requested() {
            f.close = true;
        }
        f.ppp = ppp;
        f.focused = info.focused == Some(true);
        if let Some(o) = info.outer_rect {
            f.pos = Some([(o.min.x * ppp).round() as i32, (o.min.y * ppp).round() as i32]);
        }
        if let Some(r) = info.inner_rect {
            f.size = [(r.width() * ppp).round() as i32, (r.height() * ppp).round() as i32];
            f.origin = Some(egui::pos2(r.min.x * ppp, r.min.y * ppp));
        }
        if let Some(p) = f.pos {
            let r = IRect::new(p[0], p[1], f.size[0], f.size[1]);
            if let Some(mi) = dock_float::monitor_of(&r, &self.dock.monitors) {
                let m = &self.dock.monitors[mi];
                f.monitor = Some(SavedMonitor { name: m.name.clone(), work: m.work });
            }
        }
        // 3º quadro: a janela já está no monitor dela; se a conversão de pontos (escala de outro monitor) a pôs em outro
        // lugar, corrige com a escala de agora
        f.frames += 1;
        if f.frames == 3 {
            if let (Some((tp, ts)), Some(p)) = (f.target.take(), f.pos) {
                if (p[0] - tp[0]).abs() > 3 || (p[1] - tp[1]).abs() > 3 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(tp[0] as f32 / ppp, tp[1] as f32 / ppp)));
                }
                if (f.size[0] - ts[0]).abs() > 3 || (f.size[1] - ts[1]).abs() > 3 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(ts[0] as f32 / ppp, ts[1] as f32 / ppp)));
                }
            }
        }
        // janela solta sempre na frente da principal (tenta nos primeiros quadros, até o sistema ter as duas janelas)
        if !f.owned && f.frames >= 2 && f.frames < 120 && !self.title.is_empty() {
            f.owned = dock_float::set_owner(&f.title, &self.title);
        }
        // atalhos do editor e do documento valem com a janela solta em foco (o canvas destacado continua com B, E, Ctrl+Z…)
        if f.focused {
            self.editor_shortcuts(&ctx);
            self.doc_shortcuts(&ctx);
        }
        let slot = self.dock_slot();
        let host = DockHost::Float(f.id);
        self.dock.max.validate(&slot, host, |k| layout::find_pane(&f.tree, k).is_some_and(|id| shown(&f.tree, id)));
        let maxed = self.dock.max.get(&slot, host);
        let ws = self.ws;
        let mut notes: Vec<(String, Rect)> = Vec::new();
        let env = ui_kit::Env { t, core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut notes, tool: &mut self.tool, ws };
        let mut behavior = DockBehavior { t, in_tabs: layout::panes_in_tabs(&f.tree), env, close_req: None, dock: PaneDock::new(host, maxed) };
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.bg_frame).inner_margin(egui::Margin::same(CARD_GAP as i8))).show(ui, |ui| {
            tree_ui(&mut f.tree, &mut behavior, ui);
        });
        if let Some(id) = behavior.close_req.take() {
            f.tree.tiles.set_visible(id, false);
        }
        let PaneDock { reqs, hovered, .. } = behavior.dock;
        for (name, rect) in notes {
            self.dock.rects_cur.insert(name, (rect, f.id));
        }
        // nenhum painel visível na janela: ela fecha e os escondidos voltam (escondidos) para a principal
        if visible_panes(&f.tree).is_empty() {
            f.close = true;
        }
        self.dock.pending.extend(reqs);
        if self.dock_key_pressed(&ctx) {
            self.dock_toggle_key(host, hovered);
        }
        if let Some(msg) = self.ed.toast.take() {
            self.notice(msg);
        }
    }

    /// Janelas soltas de agora → `LayoutStore` (só quando mudou; o arquivo é gravado como o resto do layout).
    fn dock_mirror(&mut self) {
        let ws = self.ws;
        if self.dock.slot.as_deref() != Some(self.dock_slot().as_str()) {
            return;
        }
        let saved: Vec<SavedFloat> = self.dock.floats.iter().filter(|f| !f.close).filter_map(FloatWin::saved).collect();
        let same = match self.layouts.get(ws) {
            Some(e) => e.floating == saved,
            None => saved.is_empty(),
        };
        if !same {
            self.layouts.entry(ws).floating = saved;
            self.layouts_changed();
        }
    }

    // -------------------------------------------------------------------------------------- menus

    /// Janela > Maximizar painel, Destacar painel, Reencaixar todas as janelas.
    pub(crate) fn menu_dock(&mut self, ui: &mut Ui) {
        let ws = self.ws;
        self.ensure_tree(ws);
        let panes: Vec<(TileId, PanelKind)> = self.trees.get(&ws).map(visible_panes).unwrap_or_default();
        let slot = self.dock_slot();
        let maxed = self.dock.max.get(&slot, DockHost::Main);
        ui.separator();
        let key = self.ed.keys.label(Action::PanelMaximize);
        let r = ui.menu_button(tr("menu.window.maximize"), |ui| {
            if maxed.is_some() {
                let r = ui.add(egui::Button::new(tr("menu.window.restore")).shortcut_text(key.clone()));
                self.rects.note("menu_item:window.restore", r.rect);
                if r.clicked() {
                    self.dock.max.restore(&slot, DockHost::Main);
                    ui.close();
                }
                ui.separator();
            }
            for (_, k) in &panes {
                let r = ui.radio(maxed == Some(*k), tr(k.title_key()));
                self.rects.note(format!("menu_item:window.maximize.{}", k.key()), r.rect);
                if r.clicked() {
                    self.dock.max.toggle(&slot, DockHost::Main, *k);
                    ui.close();
                }
            }
        });
        self.rects.note("menu_item:window.maximize", r.response.rect);
        let r = ui.menu_button(tr("menu.window.detach"), |ui| {
            for (id, k) in &panes {
                let r = ui.button(tr(k.title_key()));
                self.rects.note(format!("menu_item:window.detach.{}", k.key()), r.rect);
                if r.clicked() {
                    self.dock_detach(*id);
                    ui.close();
                }
            }
            if panes.is_empty() {
                ui.add_enabled(false, egui::Button::new(tr("menu.window.layout.none")));
            }
        });
        self.rects.note("menu_item:window.detach", r.response.rect);
        let n = self.dock.floats.len();
        let r = ui.add_enabled(n > 0, egui::Button::new(format!("{} ({n})", tr("menu.window.redock_all"))));
        self.rects.note("menu_item:window.redock_all", r.rect);
        if r.clicked() {
            self.dock_redock_all();
            ui.close();
        }
    }

    // -------------------------------------------------------------------------------------- GET_UI_STATE / UI_ACTION

    /// Viewports (janela principal e janelas soltas) para o `GET_UI_STATE`.
    pub(crate) fn dock_viewports_json(&self) -> Value {
        let r4 = |r: [f32; 4]| json!([r[0].round(), r[1].round(), r[2].round(), r[3].round()]);
        let mut out = vec![json!({ "id": "root", "main": true, "maximized_panel": self.dock.max.get(&self.dock_slot(), DockHost::Main).map(|k| k.key()) })];
        for f in &self.dock.floats {
            out.push(json!({
                "id": DockHost::Float(f.id).key(),
                "main": false,
                "title": f.title,
                "panels": f.panels().iter().map(|k| k.key()).collect::<Vec<_>>(),
                "outer_pos_physical": f.pos,
                "inner_size_physical": f.size,
                "client_origin_screen_physical": f.origin.map(|o| json!([o.x.round(), o.y.round()])),
                "rect_screen": f.origin.map(|o| r4([o.x, o.y, f.size[0] as f32, f.size[1] as f32])),
                "pixels_per_point": f.ppp,
                "monitor": f.monitor.as_ref().map(|m| json!({ "name": m.name, "work": [m.work.x, m.work.y, m.work.w, m.work.h] })),
                "focused": f.focused,
                "maximized_panel": self.dock.max.get(&self.dock_slot(), DockHost::Float(f.id)).map(|k| k.key()),
            }));
        }
        Value::Array(out)
    }

    fn dock_entry(&self, r: Rect, fid: u64) -> Value {
        let round = |v: f32| (v as f64 * 100.0).round() / 100.0;
        let f = self.dock.floats.iter().find(|f| f.id == fid);
        let ppp = f.map(|f| f.ppp).unwrap_or(1.0);
        let mut v = json!({
            "viewport": DockHost::Float(fid).key(),
            "rect_logical": [round(r.min.x), round(r.min.y), round(r.width()), round(r.height())],
            "rect_physical": [round(r.min.x * ppp), round(r.min.y * ppp), round(r.width() * ppp), round(r.height() * ppp)],
        });
        if let Some(o) = f.and_then(|f| f.origin) {
            v["rect_screen"] = json!([round(o.x + r.min.x * ppp), round(o.y + r.min.y * ppp), round(r.width() * ppp), round(r.height() * ppp)]);
            v["center_screen"] = json!([round(o.x + r.center().x * ppp), round(o.y + r.center().y * ppp)]);
        }
        v
    }

    /// Widgets desenhados nas janelas soltas (com `viewport` e o retângulo na tela daquela janela).
    pub(crate) fn dock_widget_entries(&self) -> Vec<(String, Value)> {
        self.dock.rects_prev.iter().filter(|(k, _)| !k.starts_with("panel:")).map(|(k, (r, fid))| (k.clone(), self.dock_entry(*r, *fid))).collect()
    }

    /// Painéis das janelas soltas para a lista `panels` do `GET_UI_STATE`.
    pub(crate) fn dock_panel_entries(&self) -> Vec<Value> {
        let mut out = Vec::new();
        for f in &self.dock.floats {
            for (id, t) in f.tree.tiles.iter() {
                if let Tile::Pane(k) = t {
                    let visible = shown(&f.tree, *id);
                    let mut v = json!({ "name": k.key(), "title": tr(k.title_key()), "visible": visible, "viewport": DockHost::Float(f.id).key() });
                    if let (true, Some((r, fid))) = (visible, self.dock.rects_prev.get(&format!("panel:{}", k.key()))) {
                        let e = self.dock_entry(*r, *fid);
                        for key in ["rect_logical", "rect_physical", "rect_screen"] {
                            if !e[key].is_null() {
                                v[key] = e[key].clone();
                            }
                        }
                    }
                    out.push(v);
                }
            }
        }
        out
    }

    /// Painel maximizado na janela principal (os outros não estão na tela).
    pub(crate) fn dock_main_maximized(&self) -> Option<PanelKind> {
        self.dock.max.get(&self.dock_slot(), DockHost::Main)
    }

    /// Em que viewport o canvas foi desenhado (`root` ou `float:N`).
    pub(crate) fn dock_canvas_viewport(&self) -> String {
        match self.dock.rects_prev.get("panel:canvas") {
            Some((_, fid)) => DockHost::Float(*fid).key(),
            None => "root".to_owned(),
        }
    }

    /// `UI_ACTION {action: "dock", op, panel?, group?}`: op = list | maximize | restore | detach | redock | redock_all.
    /// `detach` com `group: true` destaca o grupo de abas inteiro do painel.
    pub(crate) fn bridge_dock(&mut self, params: &Value) -> Result<Value, BridgeError> {
        let op = params.get("op").and_then(Value::as_str).ok_or_else(|| BridgeError::invalid_params("dock exige 'op' (list|maximize|restore|detach|redock|redock_all)"))?.to_owned();
        let panel = params.get("panel").and_then(Value::as_str);
        let kind = || -> Result<PanelKind, BridgeError> {
            let key = panel.ok_or_else(|| BridgeError::invalid_params(format!("dock {op} exige 'panel'")))?;
            PanelKind::from_key(key).ok_or_else(|| BridgeError::invalid_params(format!("painel desconhecido: {key}")))
        };
        let ws = self.ws;
        self.dock_sync();
        let slot = self.dock_slot();
        match op.as_str() {
            "list" => {}
            "maximize" => {
                let k = kind()?;
                let host = match self.dock_float_of(k) {
                    Some(id) => DockHost::Float(id),
                    None => {
                        let shown_main = self.trees.get(&ws).and_then(|t| layout::find_pane(t, k).map(|id| shown(t, id))).unwrap_or(false);
                        if !shown_main {
                            return Err(BridgeError::invalid_params(format!("o painel '{}' não está visível no workspace {}", k.key(), ws.key())));
                        }
                        DockHost::Main
                    }
                };
                self.dock.max.toggle(&slot, host, k);
            }
            "restore" => {
                self.dock.max.restore(&slot, DockHost::Main);
                for f in &self.dock.floats {
                    self.dock.max.restore(&slot, DockHost::Float(f.id));
                }
            }
            "detach" => {
                let k = kind()?;
                let group = params.get("group").and_then(Value::as_bool).unwrap_or(false);
                let tile = self.trees.get(&ws).and_then(|t| {
                    let id = layout::find_pane(t, k).filter(|id| shown(t, *id))?;
                    let parent = t.tiles.parent_of(id).filter(|p| matches!(t.tiles.get(*p), Some(Tile::Container(Container::Tabs(_)))));
                    Some(if group { parent.unwrap_or(id) } else { id })
                });
                let tile = tile.ok_or_else(|| BridgeError::invalid_params(format!("o painel '{}' não está visível na janela principal", k.key())))?;
                self.dock_detach(tile).ok_or_else(|| BridgeError::invalid_params("não foi possível destacar o painel"))?;
            }
            "redock" => {
                let k = kind()?;
                let id = self.dock_float_of(k).ok_or_else(|| BridgeError::invalid_params(format!("o painel '{}' não está numa janela solta", k.key())))?;
                self.dock_redock(id);
            }
            "redock_all" => {
                self.dock_redock_all();
            }
            _ => return Err(BridgeError::invalid_params(format!("dock: op desconhecida: {op}"))),
        }
        self.dock_mirror();
        Ok(json!({ "op": op, "workspace": ws.key(), "viewports": self.dock_viewports_json() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::WorkspaceId;

    /// Destaca o painel (como o botão faz) e reencaixa (como fechar a janela faz): o layout volta igual.
    fn round_trip(ws: WorkspaceId, kind: PanelKind, group: bool) {
        let mut tree = layout::default_tree(ws);
        let before = layout::saved_from_tree(&tree).unwrap();
        let pane = layout::find_pane(&tree, kind).unwrap();
        let tile = if group { tree.tiles.parent_of(pane).unwrap() } else { pane };
        let home = home_of(&tree, tile);
        assert!(home.is_some(), "{kind:?}: sem lugar guardado");
        let mut tiles = Tiles::default();
        let root = graft(&mut tiles, &tree.tiles, tile).unwrap();
        let float = Tree::new(float_tree_id(1), root, tiles);
        remove_tile(&mut tree, tile);
        assert!(layout::find_pane(&tree, kind).is_none());
        assert!(layout::find_pane(&float, kind).is_some());
        // a árvore principal se simplifica como no quadro seguinte do egui_tiles
        tree.simplify(&egui_tiles::SimplificationOptions::default());
        // a janela guardada passa pelo arquivo
        let saved = SavedFloat { tree: layout::saved_from_tree(&float).unwrap(), pos: None, size: [800, 400], monitor: None, home: home.clone() };
        let back: SavedFloat = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        let float = layout::float_tree_from_saved(ws, &back.tree, float_tree_id(2), &HashSet::new()).unwrap();
        let new = graft(&mut tree.tiles, &float.tiles, float.root.unwrap()).unwrap();
        assert!(place_home(&mut tree, new, back.home.as_ref()), "{kind:?}: não voltou ao lugar");
        tree.simplify(&egui_tiles::SimplificationOptions::default());
        let after = layout::saved_from_tree(&tree).unwrap();
        assert!(layout::same_layout(&before, &after), "{kind:?} group={group}:
antes {before:?}
depois {after:?}");
    }

    #[test]
    fn detach_and_redock_restores_the_layout() {
        // a linha do tempo da Animação (faixa de baixo da coluna principal), uma aba e um grupo de abas inteiro
        round_trip(WorkspaceId::Animacao, PanelKind::Timeline, false);
        round_trip(WorkspaceId::Animacao, PanelKind::Layers, false);
        round_trip(WorkspaceId::Animacao, PanelKind::Layers, true);
        round_trip(WorkspaceId::Animacao, PanelKind::Canvas, false);
        round_trip(WorkspaceId::Composicao, PanelKind::CompTimeline, false);
        round_trip(WorkspaceId::Edicao, PanelKind::Timeline, false);
        round_trip(WorkspaceId::Audio, PanelKind::Multitrack, false);
    }

    #[test]
    fn a_missing_home_falls_back_to_the_catalog() {
        let ws = WorkspaceId::Animacao;
        let mut tree = layout::default_tree(ws);
        let id = layout::find_pane(&tree, PanelKind::Timeline).unwrap();
        let mut tiles = Tiles::default();
        let root = graft(&mut tiles, &tree.tiles, id).unwrap();
        remove_tile(&mut tree, id);
        let new = graft(&mut tree.tiles, &tiles, root).unwrap();
        let home = FloatHome::Col { block: vec!["nao_existe".into()], after: true, frac: 0.3 };
        assert!(!place_home(&mut tree, new, Some(&home)));
        place_fallback(&mut tree, new);
        assert!(layout::find_pane(&tree, PanelKind::Timeline).is_some_and(|t| shown(&tree, t)));
        // árvore vazia (tudo em janelas): o que volta vira a raiz
        let mut empty = layout::default_tree(ws);
        let r = empty.root.unwrap();
        let mut t2 = Tiles::default();
        let saved_root = graft(&mut t2, &empty.tiles, r).unwrap();
        remove_tile(&mut empty, r);
        assert!(empty.root.is_none());
        let n = graft(&mut empty.tiles, &t2, saved_root).unwrap();
        place_fallback(&mut empty, n);
        assert_eq!(empty.root, Some(n));
    }
}
