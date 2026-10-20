//! Ferramenta Texto (T): clique cria texto de ponto, arrasto cria texto de caixa (quebra automática), clicar num texto
//! existente volta a editá-lo. A digitação é no próprio canvas: o texto vive em `params.text` de uma camada `Text` e o
//! compositor o desenha ao vivo; aqui ficam o cursor, a seleção por teclado/mouse, o IME e as molduras.
//!
//! Durante a edição cada mudança vai ao documento (para o canvas mostrar o resultado real, com as camadas de cima), e ao
//! terminar (Esc, clicar fora, trocar de ferramenta ou de camada) os passos da sessão viram UM passo de desfazer: desfaz
//! os intermediários e aplica o resultado final num comando só. Se algo mexeu no documento por fora no meio (undo do
//! menu, MCP), a sessão só termina e os passos ficam como estão.

use super::commands::{self as cmds, NodeId};
use super::editor::{Editor, Xf};
use super::ui_kit::Env;
use crate::i18n::tr;
use crate::workspace::ToolId;
use aiai_core::*;
use aiai_paint::Affine;
use aiai_paint::text_layer::{self as tl, TextLayout, TextParams};
use egui::{Color32, Event, Key, Pos2, Shape, Stroke, Vec2};
use serde_json::json;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Digitando no canvas.
    Typing,
    /// Arrastando a camada de texto com o Mover.
    Move,
}

/// Posição do cursor e da âncora da seleção (índices de char), com a posição guardada para subir/descer linhas.
#[derive(Debug, Clone, Default)]
pub(crate) struct Caret {
    pub caret: usize,
    pub anchor: usize,
    pub pref_flow: Option<f32>,
}

fn byte_of(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(s.len())
}

fn class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() && !tl_is_cjk(c) || c == '_' {
        1
    } else if tl_is_cjk(c) {
        3
    } else {
        2
    }
}

fn tl_is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xA000..=0xA4CF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

/// Começo da palavra anterior (Ctrl+←).
pub(crate) fn prev_word(s: &str, ci: usize) -> usize {
    let chars: Vec<char> = s.chars().collect();
    let mut i = ci.min(chars.len());
    while i > 0 && class(chars[i - 1]) == 0 {
        i -= 1;
    }
    if i > 0 {
        let k = class(chars[i - 1]);
        while i > 0 && class(chars[i - 1]) == k {
            i -= 1;
        }
    }
    i
}

/// Começo da próxima palavra (Ctrl+→).
pub(crate) fn next_word(s: &str, ci: usize) -> usize {
    let chars: Vec<char> = s.chars().collect();
    let mut i = ci.min(chars.len());
    if i < chars.len() {
        let k = class(chars[i]);
        if k != 0 {
            while i < chars.len() && class(chars[i]) == k {
                i += 1;
            }
        }
    }
    while i < chars.len() && class(chars[i]) == 0 && chars[i] != '\n' {
        i += 1;
    }
    i
}

/// A palavra (corrida de mesma classe) em volta do cursor, para o duplo clique.
pub(crate) fn word_at(s: &str, ci: usize) -> (usize, usize) {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let i = ci.min(chars.len() - 1);
    let k = class(chars[i]);
    let (mut a, mut b) = (i, i + 1);
    while a > 0 && class(chars[a - 1]) == k && chars[a - 1] != '\n' {
        a -= 1;
    }
    while b < chars.len() && class(chars[b]) == k && chars[b] != '\n' {
        b += 1;
    }
    (a, b)
}

impl Caret {
    pub(crate) fn range(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub(crate) fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }

    pub(crate) fn selected(&self, s: &str) -> String {
        let (a, b) = self.range();
        s.chars().skip(a).take(b - a).collect()
    }

    pub(crate) fn clamp(&mut self, s: &str) {
        let n = s.chars().count();
        self.caret = self.caret.min(n);
        self.anchor = self.anchor.min(n);
    }

    /// Troca a seleção (ou insere no cursor) por `text`.
    pub(crate) fn insert(&mut self, s: &mut String, text: &str) {
        let (a, b) = self.range();
        let (ba, bb) = (byte_of(s, a), byte_of(s, b));
        s.replace_range(ba..bb, text);
        self.caret = a + text.chars().count();
        self.anchor = self.caret;
        self.pref_flow = None;
    }

    pub(crate) fn backspace(&mut self, s: &mut String, word: bool) {
        if !self.has_selection() {
            let to = if word { prev_word(s, self.caret) } else { self.caret.saturating_sub(1) };
            self.anchor = to;
        }
        self.insert(s, "");
    }

    pub(crate) fn delete(&mut self, s: &mut String, word: bool) {
        if !self.has_selection() {
            let n = s.chars().count();
            let to = if word { next_word(s, self.caret).max(self.caret + 1) } else { self.caret + 1 };
            self.anchor = to.min(n);
        }
        self.insert(s, "");
    }

    pub(crate) fn move_to(&mut self, to: usize, extend: bool, keep_pref: bool) {
        self.caret = to;
        if !extend {
            self.anchor = to;
        }
        if !keep_pref {
            self.pref_flow = None;
        }
    }
}

enum Drag {
    /// Criando texto: do clique até o cursor (clique sem arrasto = texto de ponto).
    NewBox { start: Vec2, cur: Vec2 },
    /// Selecionando texto com o mouse dentro da sessão.
    Select,
    /// Mover: arrastando a camada de texto.
    Move { last: Vec2 },
}

pub(crate) struct Session {
    pub kind: Kind,
    pub layer: Option<NodeId>,
    pub cut: NodeId,
    pub params: TextParams,
    original: Option<TextParams>,
    pub caret: Caret,
    /// Tamanho da pilha de desfazer antes da sessão e quantos passos a sessão gravou.
    base_len: usize,
    applied: usize,
    /// `change_counter` esperado: se mudar sem ser por nós, alguém mexeu no documento.
    counter: u64,
    /// O que já está no documento (para não regravar igual).
    pushed: Option<TextParams>,
    dirty: bool,
    lay: Option<(TextParams, TextLayout)>,
    blink0: Instant,
}

impl Session {
    fn lay(&mut self) -> &TextLayout {
        if self.lay.as_ref().is_none_or(|(p, _)| *p != self.params) {
            self.lay = Some((self.params.clone(), tl::layout(&self.params)));
        }
        &self.lay.as_ref().expect("layout").1
    }
}

#[derive(Default)]
pub(crate) struct TextState {
    /// Estilo do próximo texto novo (o último usado).
    pub defaults: TextParams,
    pub session: Option<Session>,
    drag: Option<Drag>,
    last_click: Option<(Instant, Vec2)>,
    warmed: bool,
    pub panel: super::panels::text::PanelState,
}

pub(crate) fn dragging(env: &Env) -> bool {
    env.ed.text.drag.is_some()
}

pub(crate) fn editing(ed: &Editor) -> bool {
    ed.text.session.as_ref().is_some_and(|s| s.kind == Kind::Typing)
}

fn run(env: &mut Env, cmd: Command) -> bool {
    match cmds::run(env.core, cmd) {
        Ok(_) => true,
        Err(e) => {
            env.ed.notify(e);
            false
        }
    }
}

fn history_len(env: &Env) -> usize {
    env.core.history().0.len()
}

/// Camada de texto (visível, destravada) mais alta sob `c` no corte, com o layout para o clique.
fn hit_layer(env: &Env, cut: &NodeId, c: Vec2) -> Option<NodeId> {
    let mut all = Vec::new();
    super::live::ordered_leaves(&env.core.project, cut, &mut all);
    for (id, l) in all.iter().rev() {
        if l.kind != LayerType::Text || !l.visible || l.locked {
            continue;
        }
        let Some(Ok(p)) = TextParams::from_layer_params(&l.params) else { continue };
        if p.content.is_empty() {
            continue;
        }
        let lay = tl::layout(&p);
        let aff = cmds::layer_placement(&env.core.project, cut, id, env.ed.frame);
        let local = to_local(&p, aff, c);
        let b = lay.bounds;
        let pad = 3.0;
        if local[0] >= b[0] - pad && local[0] <= b[2] + pad && local[1] >= b[1] - pad && local[1] <= b[3] + pad {
            return Some(id.clone());
        }
    }
    None
}

/// Ponto do canvas → espaço local do texto (desfaz o movimento da camada e a rotação em torno da âncora).
fn to_local(p: &TextParams, aff: Option<Affine>, c: Vec2) -> [f32; 2] {
    let c = match aff.and_then(|a| a.inverse()) {
        Some(inv) => {
            let (x, y) = inv.apply(c.x, c.y);
            [x, y]
        }
        None => [c.x, c.y],
    };
    p.canvas_to_local(c)
}

/// Ponto local do texto → canvas (com o movimento da camada).
fn to_canvas(p: &TextParams, aff: Option<Affine>, l: [f32; 2]) -> Vec2 {
    let c = p.local_to_canvas(l);
    match aff {
        Some(a) => {
            let (x, y) = a.apply(c[0], c[1]);
            Vec2::new(x, y)
        }
        None => Vec2::new(c[0], c[1]),
    }
}

/// A camada ativa, se for de texto (destravada ou não): seus parâmetros.
fn active_text_layer(env: &mut Env) -> Option<(NodeId, TextParams)> {
    let id = env.ed.active_layer(env.core)?;
    let l = cmds::layer_data(&env.core.project, &id)?;
    if l.kind != LayerType::Text {
        return None;
    }
    TextParams::from_layer_params(&l.params)?.ok().map(|p| (id, p))
}

/// Parâmetros que o painel mostra: os da sessão, os da camada de texto ativa, ou o estilo padrão do próximo texto.
pub(crate) fn current_params(env: &mut Env) -> (Option<NodeId>, TextParams, bool) {
    if let Some(s) = &env.ed.text.session {
        return (s.layer.clone(), s.params.clone(), true);
    }
    if let Some((id, p)) = active_text_layer(env) {
        return (Some(id), p, true);
    }
    (None, env.ed.text.defaults.clone(), false)
}

/// Aplica uma mudança de estilo: na sessão em curso (vai ao documento no quadro), na camada de texto ativa (um passo de
/// desfazer) ou, sem nenhuma das duas, no estilo padrão do próximo texto. O estilo também vira o padrão dali em diante.
pub(crate) fn edit(env: &mut Env, label_key: &str, f: impl FnOnce(&mut TextParams)) {
    if let Some(s) = env.ed.text.session.as_mut() {
        f(&mut s.params);
        let n = s.params.content.clone();
        s.caret.clamp(&n);
        s.dirty = true;
        remember_defaults(env);
        return;
    }
    if let Some((layer, old)) = active_text_layer(env) {
        let mut p = old.clone();
        f(&mut p);
        if p != old {
            let patch = p.to_layer_patch();
            let cmd = Command::Batch { commands: vec![Command::SetProps { id: layer, name: None, data: patch }], label: Some(tr(label_key).to_owned()) };
            if run(env, cmd) {
                env.ed.text.defaults = style_of(&p, &env.ed.text.defaults);
            }
        }
        return;
    }
    f(&mut env.ed.text.defaults);
}

/// O estilo de `p` sem o que é específico do texto (conteúdo, posição, caixa, rotação).
fn style_of(p: &TextParams, base: &TextParams) -> TextParams {
    TextParams { content: base.content.clone(), pos: base.pos, box_size: base.box_size, rotation: base.rotation, ..p.clone() }
}

fn remember_defaults(env: &mut Env) {
    if let Some(s) = &env.ed.text.session {
        env.ed.text.defaults = style_of(&s.params, &env.ed.text.defaults);
    }
}

// ------------------------------------------------------------------------------------------------ sessão

fn new_session(env: &mut Env, kind: Kind, layer: Option<NodeId>, cut: NodeId, params: TextParams, original: Option<TextParams>) -> Session {
    Session {
        kind,
        layer,
        cut,
        params,
        original,
        caret: Caret::default(),
        base_len: history_len(env),
        applied: 0,
        counter: env.core.change_counter,
        pushed: None,
        dirty: false,
        lay: None,
        blink0: Instant::now(),
    }
}

/// Começa a digitar um texto novo em `pos` (ponto) ou na caixa dada. A camada só nasce quando houver o primeiro caractere.
fn begin_new(env: &mut Env, cut: NodeId, at: Vec2, box_size: Option<[f32; 2]>) {
    let mut p = env.ed.text.defaults.clone();
    p.content = String::new();
    p.box_size = box_size;
    p.rotation = 0.0;
    p.color = env.ed.color.clone();
    p.pos = [at.x, at.y];
    if box_size.is_none() {
        // o clique marca a linha de base (como no Photoshop): sobe a âncora pela distância do topo da linha até ela
        let probe = tl::layout(&TextParams { content: "A".into(), ..p.clone() });
        if !p.vertical {
            p.pos[1] -= probe.lines.first().map(|l| l.baseline).unwrap_or(0.0);
        }
    }
    let s = new_session(env, Kind::Typing, None, cut, p, None);
    env.ed.text.session = Some(s);
}

/// Volta a editar a camada de texto `layer`, com o cursor onde o clique caiu (`at`, em px do canvas).
fn begin_edit(env: &mut Env, layer: NodeId, at: Option<Vec2>) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let Some(l) = cmds::layer_data(&env.core.project, &layer) else { return };
    let Some(Ok(p)) = TextParams::from_layer_params(&l.params) else { return };
    let mut s = new_session(env, Kind::Typing, Some(layer.clone()), cut.clone(), p.clone(), Some(p.clone()));
    s.pushed = Some(p.clone());
    let n = p.content.chars().count();
    s.caret = Caret { caret: n, anchor: n, pref_flow: None };
    if let Some(c) = at {
        let aff = cmds::layer_placement(&env.core.project, &cut, &layer, env.ed.frame);
        let lay = s.lay().clone();
        let h = lay.hit(to_local(&p, aff, c));
        s.caret = Caret { caret: h, anchor: h, pref_flow: None };
    }
    env.ed.layer = Some(layer);
    env.ed.text.session = Some(s);
}

/// Cria a camada de texto com `params` num passo só (acima de `above`, na pasta se for pasta; sem ela, no topo do corte).
pub(crate) fn create_text_layer(core: &mut CoreSession, cut: &NodeId, above: Option<&NodeId>, params: &TextParams, label: &str) -> Result<NodeId, String> {
    let p = &core.project;
    let (parent, index) = match above.and_then(|a| p.node(a).map(|n| (a, n))) {
        Some((a, n)) if matches!(&n.data, NodeKind::Layer(l) if l.kind == LayerType::Group) => (a.clone(), None),
        Some((a, n)) => match &n.parent {
            Some(par) => (par.clone(), p.node(par).and_then(|pn| pn.children.iter().position(|c| c == a)).map(|i| i + 1)),
            None => (cut.clone(), None),
        },
        None => (cut.clone(), None),
    };
    let first_line: String = params.content.lines().next().unwrap_or("").chars().take(28).collect();
    let name = if first_line.trim().is_empty() { cmds::unique_layer_name(p, &parent, tr(cmds::layer_type_key(LayerType::Text))) } else { first_line.trim().to_owned() };
    let mut data = LayerData::new(LayerType::Text);
    data.params = json!({ TextParams::KEY: params.to_value() });
    let id = core.ids.next();
    let cmd = Command::Batch { commands: vec![cmds::create(id.clone(), Some(parent), index, name, NodeKind::Layer(data))], label: Some(label.to_owned()) };
    cmds::run(core, cmd)?;
    Ok(id)
}

/// A camada de texto padrão do "+" do painel Camadas: texto de ponto no meio do canvas.
pub(crate) fn default_layer_params(canvas: (u32, u32), color: ColorRef) -> TextParams {
    let size = (canvas.1 as f32 * 0.08).clamp(24.0, 160.0);
    TextParams { content: tr("text.placeholder").to_owned(), size, color, pos: [canvas.0 as f32 / 2.0, canvas.1 as f32 / 2.0], align: tl::TextAlign::Center, ..Default::default() }
}

/// Grava no documento o que a sessão tem de novo (um passo provisório; `finish` junta todos).
fn push(env: &mut Env) {
    let Some(mut s) = env.ed.text.session.take() else { return };
    if s.dirty && s.pushed.as_ref() != Some(&s.params) {
        match s.layer.clone() {
            Some(id) => {
                let patch = s.params.to_layer_patch();
                if run(env, Command::SetProps { id, name: None, data: patch }) {
                    s.applied += 1;
                }
            }
            None if !s.params.content.is_empty() => {
                let above = env.ed.layer.clone();
                match create_text_layer(env.core, &s.cut, above.as_ref(), &s.params, tr("text.cmd_edit")) {
                    Ok(id) => {
                        env.ed.layer = Some(id.clone());
                        s.layer = Some(id);
                        s.applied += 1;
                    }
                    Err(e) => env.ed.notify(e),
                }
            }
            None => {}
        }
        s.pushed = Some(s.params.clone());
        s.counter = env.core.change_counter;
    }
    s.dirty = false;
    env.ed.text.session = Some(s);
}

/// Termina a sessão: grava o pendente e junta os passos num só (ver o cabeçalho do módulo).
pub(crate) fn finish(env: &mut Env) {
    push(env);
    env.ed.text.drag = None;
    let Some(s) = env.ed.text.session.take() else { return };
    if s.applied == 0 {
        return;
    }
    let intact = env.core.change_counter == s.counter && history_len(env) == s.base_len + s.applied;
    if !intact {
        return;
    }
    let label = tr(if s.kind == Kind::Move { "text.cmd_move" } else { "text.cmd_edit" });
    let empty = s.params.content.chars().all(char::is_whitespace);
    for _ in 0..s.applied {
        if let Err(e) = env.core.undo() {
            env.ed.notify(e.to_string());
            return;
        }
    }
    match (s.original.as_ref(), s.layer.as_ref()) {
        // texto novo: a camada nasce de novo, de uma vez, com o texto final
        (None, _) if !empty => {
            let above = env.ed.layer.clone().filter(|l| env.core.project.node(l).is_some());
            match create_text_layer(env.core, &s.cut, above.as_ref(), &s.params, label) {
                Ok(id) => env.ed.layer = Some(id),
                Err(e) => env.ed.notify(e),
            }
        }
        (None, _) => {}
        // texto existente esvaziado: a camada some (como no Photoshop)
        (Some(_), Some(layer)) if empty => {
            if let Ok(_o) = cmds::run(env.core, Command::Batch { commands: vec![Command::DeleteNode { id: layer.clone() }], label: Some(label.to_owned()) }) {
                env.ed.layer = cmds::layer_rows(&env.core.project, &s.cut).first().map(|r| r.id.clone());
            }
        }
        (Some(orig), Some(layer)) => {
            if *orig != s.params {
                let cmd = Command::Batch { commands: vec![Command::SetProps { id: layer.clone(), name: None, data: s.params.to_layer_patch() }], label: Some(label.to_owned()) };
                run(env, cmd);
            }
        }
        (Some(_), None) => {}
    }
}

// ------------------------------------------------------------------------------------------------ entrada

/// Clique do mouse com a ferramenta Texto.
pub(crate) fn press(env: &mut Env, c: Vec2, scale: f32, mods: egui::Modifiers) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let now = Instant::now();
    let double = env.ed.text.last_click.is_some_and(|(t, p)| t.elapsed().as_millis() < 400 && (p - c).length() * scale < 6.0);
    env.ed.text.last_click = Some((now, c));
    if editing(env.ed) {
        let inside = {
            let aff = env.ed.text.session.as_ref().and_then(|s| s.layer.clone()).and_then(|l| cmds::layer_placement(&env.core.project, &cut, &l, env.ed.frame));
            let s = env.ed.text.session.as_mut().expect("sessão");
            let local = to_local(&s.params, aff, c);
            let b = s.lay().bounds;
            let pad = 4.0 / scale.max(1e-3);
            (local[0] >= b[0] - pad && local[0] <= b[2] + pad && local[1] >= b[1] - pad && local[1] <= b[3] + pad).then_some((local, aff))
        };
        if let Some((local, _)) = inside {
            let s = env.ed.text.session.as_mut().expect("sessão");
            let h = s.lay().hit(local);
            let content = s.params.content.clone();
            if double {
                let (a, b) = word_at(&content, h);
                s.caret = Caret { caret: b, anchor: a, pref_flow: None };
            } else {
                let anchor = if mods.shift { s.caret.anchor } else { h };
                s.caret = Caret { caret: h, anchor, pref_flow: None };
            }
            s.blink0 = Instant::now();
            env.ed.text.drag = Some(Drag::Select);
            return;
        }
        // fora do texto: confirma. Clicar noutro texto passa a editá-lo; no vazio só confirma
        finish(env);
        if let Some(l) = hit_layer(env, &cut, c) {
            begin_edit(env, l, Some(c));
            env.ed.text.drag = Some(Drag::Select);
        }
        return;
    }
    if let Some(l) = hit_layer(env, &cut, c) {
        begin_edit(env, l, Some(c));
        env.ed.text.drag = Some(Drag::Select);
        return;
    }
    env.ed.text.drag = Some(Drag::NewBox { start: c, cur: c });
}

/// Clique com o Mover: se a camada ativa é de texto, o arrasto a leva (a posição do texto muda). `true` = tratado.
pub(crate) fn move_press(env: &mut Env, c: Vec2) -> bool {
    if editing(env.ed) {
        finish(env);
    }
    let Some((layer, p)) = active_text_layer(env) else { return false };
    let Some(cut) = env.ed.active_cut(env.core) else { return false };
    if cmds::layer_data(&env.core.project, &layer).is_some_and(|l| l.locked || !l.visible) {
        env.ed.notify(tr("err.move_layer"));
        return true;
    }
    let mut s = new_session(env, Kind::Move, Some(layer), cut, p.clone(), Some(p.clone()));
    s.pushed = Some(p);
    env.ed.text.session = Some(s);
    env.ed.text.drag = Some(Drag::Move { last: c });
    true
}

pub(crate) fn drag(env: &mut Env, c: Vec2) {
    match env.ed.text.drag.take() {
        Some(Drag::NewBox { start, .. }) => env.ed.text.drag = Some(Drag::NewBox { start, cur: c }),
        Some(Drag::Select) => {
            let cut = env.ed.cut.clone();
            if let (Some(cut), Some(s)) = (cut, env.ed.text.session.as_mut()) {
                let aff = s.layer.clone().and_then(|l| cmds::layer_placement(&env.core.project, &cut, &l, env.ed.frame));
                let local = to_local(&s.params, aff, c);
                let h = s.lay().hit(local);
                s.caret.caret = h;
                s.caret.pref_flow = None;
            }
            env.ed.text.drag = Some(Drag::Select);
        }
        Some(Drag::Move { last }) => {
            if let Some(s) = env.ed.text.session.as_mut() {
                let d = c - last;
                s.params.pos = [s.params.pos[0] + d.x, s.params.pos[1] + d.y];
                s.dirty = true;
            }
            env.ed.text.drag = Some(Drag::Move { last: c });
        }
        None => {}
    }
}

pub(crate) fn release(env: &mut Env, c: Vec2, scale: f32) {
    match env.ed.text.drag.take() {
        Some(Drag::NewBox { start, .. }) => {
            let Some(cut) = env.ed.active_cut(env.core) else { return };
            let d = c - start;
            if d.length() * scale < 6.0 {
                begin_new(env, cut, start, None);
            } else {
                let (x0, y0) = (start.x.min(c.x), start.y.min(c.y));
                begin_new(env, cut, Vec2::new(x0, y0), Some([d.x.abs().max(8.0), d.y.abs().max(8.0)]));
            }
        }
        Some(Drag::Select) => {}
        Some(Drag::Move { .. }) => finish(env),
        None => {}
    }
}

fn is_line_axis_swapped(s: &Session) -> bool {
    s.params.vertical
}

/// Teclado e ciclo de vida da sessão: roda uma vez por quadro, antes do tratamento do mouse.
pub(crate) fn frame(env: &mut Env, ctx: &egui::Context) {
    if *env.tool == ToolId::Text && !env.ed.text.warmed {
        env.ed.text.warmed = true;
        aiai_paint::fonts::warm_up();
    }
    // soltou o mouse fora da janela sem o evento: termina o arrasto
    if env.ed.text.drag.is_some() && !ctx.input(|i| i.pointer.any_down()) {
        let p = env.ed.cursor.unwrap_or_default();
        if let Some(rect) = env.ed.canvas_rect {
            let xf = env.ed.view.xf(rect, ctx.pixels_per_point());
            let c = xf.to_canvas(p);
            release(env, c, xf.scale);
        } else {
            env.ed.text.drag = None;
        }
    }
    let Some(s) = env.ed.text.session.as_ref() else { return };
    // fim automático: outra ferramenta, outra camada/corte, ou o documento mudou por fora
    let tool_ok = match s.kind {
        Kind::Typing => *env.tool == ToolId::Text,
        Kind::Move => *env.tool == ToolId::Move,
    };
    let layer_ok = s.layer.as_ref().is_none_or(|l| env.ed.layer.as_ref() == Some(l));
    let cut_ok = env.ed.cut.as_ref() == Some(&s.cut);
    if env.core.change_counter != s.counter {
        env.ed.text.session = None;
        env.ed.text.drag = None;
        return;
    }
    if !tool_ok || !layer_ok || !cut_ok {
        finish(env);
        return;
    }
    if s.kind == Kind::Move {
        push(env);
        return;
    }
    // ---- teclado (só se nenhum campo de texto da interface tem o foco)
    if !ctx.egui_wants_keyboard_input() {
        let events: Vec<Event> = ctx.input(|i| i.events.clone());
        let mut commit = false;
        for ev in &events {
            let Some(s) = env.ed.text.session.as_mut() else { break };
            match ev {
                Event::Text(t) => type_text(s, t),
                Event::Ime(egui::ImeEvent::Commit(t)) => type_text(s, t),
                Event::Paste(t) => type_text(s, &t.replace("\r\n", "\n").replace('\r', "\n")),
                Event::Copy => {
                    if s.caret.has_selection() {
                        ctx.copy_text(s.caret.selected(&s.params.content));
                    }
                }
                Event::Cut => {
                    if s.caret.has_selection() {
                        ctx.copy_text(s.caret.selected(&s.params.content));
                        let mut c = std::mem::take(&mut s.params.content);
                        s.caret.insert(&mut c, "");
                        s.params.content = c;
                        s.dirty = true;
                    }
                }
                Event::Key { key, pressed: true, modifiers, .. } => {
                    if *key == Key::Escape || (*key == Key::Enter && modifiers.command) {
                        commit = true;
                        break;
                    }
                    key_press(s, *key, *modifiers);
                }
                _ => {}
            }
        }
        if commit {
            finish(env);
            return;
        }
    }
    if env.ed.text.session.as_ref().is_some_and(|s| s.dirty) {
        if let Some(s) = env.ed.text.session.as_mut() {
            s.blink0 = Instant::now();
        }
        push(env);
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
}

fn type_text(s: &mut Session, text: &str) {
    let clean: String = text.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect();
    if clean.is_empty() {
        return;
    }
    let mut c = std::mem::take(&mut s.params.content);
    s.caret.insert(&mut c, &clean);
    s.params.content = c;
    s.dirty = true;
}

fn key_press(s: &mut Session, key: Key, m: egui::Modifiers) {
    let (shift, word) = (m.shift, m.command);
    let swapped = is_line_axis_swapped(s);
    let content = s.params.content.clone();
    let n = content.chars().count();
    match key {
        Key::Enter => type_text(s, "\n"),
        Key::Backspace => {
            let mut c = std::mem::take(&mut s.params.content);
            s.caret.backspace(&mut c, word);
            s.params.content = c;
            s.dirty = true;
        }
        Key::Delete => {
            let mut c = std::mem::take(&mut s.params.content);
            s.caret.delete(&mut c, word);
            s.params.content = c;
            s.dirty = true;
        }
        Key::A if m.command => s.caret = Caret { caret: n, anchor: 0, pref_flow: None },
        Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
            // no tategaki, ↑/↓ andam pela coluna (como ←/→ no horizontal) e ←/→ trocam de coluna
            let along = match (key, swapped) {
                (Key::ArrowLeft, false) | (Key::ArrowUp, true) => Some(-1),
                (Key::ArrowRight, false) | (Key::ArrowDown, true) => Some(1),
                _ => None,
            };
            if let Some(dir) = along {
                let to = if !shift && s.caret.has_selection() && !word {
                    let (a, b) = s.caret.range();
                    if dir < 0 { a } else { b }
                } else if word {
                    if dir < 0 { prev_word(&content, s.caret.caret) } else { next_word(&content, s.caret.caret) }
                } else if dir < 0 {
                    s.caret.caret.saturating_sub(1)
                } else {
                    (s.caret.caret + 1).min(n)
                };
                s.caret.move_to(to, shift, false);
            } else {
                // linha acima/abaixo (no vertical: coluna à direita/esquerda), guardando a posição no fluxo
                let dir = match (key, swapped) {
                    (Key::ArrowUp, false) | (Key::ArrowRight, true) => -1,
                    _ => 1,
                };
                let lay = s.lay().clone();
                let flow = s.caret.pref_flow.unwrap_or_else(|| lay.flow_of(s.caret.caret));
                let to = lay.move_line(s.caret.caret, dir, flow);
                s.caret.move_to(to, shift, true);
                s.caret.pref_flow = Some(flow);
            }
        }
        Key::Home | Key::End => {
            let lay = s.lay().clone();
            let li = lay.line_of(s.caret.caret);
            let to = if word {
                if key == Key::Home { 0 } else { n }
            } else {
                match lay.lines.get(li) {
                    Some(l) if key == Key::Home => l.start,
                    Some(l) => {
                        let wrapped = lay.lines.get(li + 1).is_some_and(|nl| nl.start == l.end);
                        if wrapped { l.end.saturating_sub(1).max(l.start) } else { l.end }
                    }
                    None => 0,
                }
            };
            s.caret.move_to(to, shift, false);
        }
        _ => {}
    }
    s.blink0 = Instant::now();
}

// ------------------------------------------------------------------------------------------------ pintura

fn quad_screen(p: &TextParams, aff: Option<Affine>, xf: &Xf, r: [f32; 4]) -> [Pos2; 4] {
    [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]].map(|q| xf.to_screen(to_canvas(p, aff, q)))
}

/// Moldura, seleção e cursor da sessão; prévia da caixa que está sendo arrastada; destaque do texto sob o mouse.
pub(crate) fn paint(env: &mut Env, painter: &egui::Painter, xf: &Xf, cut: &NodeId) {
    let accent = env.t.accent;
    let ctx = painter.ctx().clone();
    if let Some(Drag::NewBox { start, cur }) = &env.ed.text.drag {
        let r = egui::Rect::from_two_pos(xf.to_screen(*start), xf.to_screen(*cur));
        painter.rect_filled(r, 0.0, accent.gamma_multiply(0.10));
        painter.add(Shape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], Stroke::new(1.0, accent), 5.0, 4.0));
    }
    let frame_no = env.ed.frame;
    // texto sob o mouse (ferramenta Texto, sem edição): contorno fino
    if *env.tool == ToolId::Text && env.ed.text.session.is_none() && env.ed.text.drag.is_none() {
        if let Some(pos) = env.ed.cursor {
            let c = xf.to_canvas(pos);
            if env.ed.canvas_rect.is_some_and(|r| r.contains(pos)) {
                if let Some(l) = hit_layer(env, cut, c) {
                    if let Some(Ok(p)) = cmds::layer_data(&env.core.project, &l).and_then(|ld| TextParams::from_layer_params(&ld.params)) {
                        let aff = cmds::layer_placement(&env.core.project, cut, &l, frame_no);
                        let lay = tl::layout(&p);
                        let q = quad_screen(&p, aff, xf, lay.bounds);
                        painter.add(Shape::closed_line(q.to_vec(), Stroke::new(1.0, accent.gamma_multiply(0.7))));
                    }
                }
            }
        }
    }
    let Some(s) = env.ed.text.session.as_mut() else { return };
    if s.kind != Kind::Typing {
        return;
    }
    let aff = s.layer.clone().and_then(|l| cmds::layer_placement(&env.core.project, &s.cut, &l, frame_no));
    let params = s.params.clone();
    let lay = s.lay().clone();
    // moldura: tracejada na caixa de parágrafo, fina no texto de ponto
    let frame_q = quad_screen(&params, aff, xf, lay.bounds);
    if params.box_size.is_some() {
        let mut pts = frame_q.to_vec();
        pts.push(frame_q[0]);
        painter.add(Shape::dashed_line(&pts, Stroke::new(1.0, accent), 5.0, 4.0));
    } else {
        painter.add(Shape::closed_line(frame_q.to_vec(), Stroke::new(1.0, accent.gamma_multiply(0.55))));
    }
    // seleção
    let (a, b) = s.caret.range();
    for r in lay.selection_rects(a, b) {
        painter.add(Shape::convex_polygon(quad_screen(&params, aff, xf, r).to_vec(), accent.gamma_multiply(0.30), Stroke::NONE));
    }
    // cursor
    let cr = lay.caret_rect(s.caret.caret);
    let (p0, p1) = (xf.to_screen(to_canvas(&params, aff, [cr[0], cr[1]])), xf.to_screen(to_canvas(&params, aff, [cr[2], cr[3]])));
    let on = s.blink0.elapsed().as_secs_f32() % 1.0 < 0.6;
    if on {
        let ink = caret_color(env.t.text);
        painter.line_segment([p0, p1], Stroke::new(3.0, Color32::from_black_alpha(90)));
        painter.line_segment([p0, p1], Stroke::new(1.5, ink));
    }
    // IME: onde a janela de candidatos aparece
    let rect = egui::Rect::from_two_pos(frame_q[0], frame_q[2]);
    let cursor_rect = egui::Rect::from_two_pos(p0, p1).expand2(Vec2::new(1.0, 0.0));
    ctx.output_mut(|o| o.ime = Some(egui::output::IMEOutput { purpose: egui::IMEPurpose::Normal, rect, cursor_rect, should_interrupt_composition: false }));
}

fn caret_color(text: Color32) -> Color32 {
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_inserts_deletes_and_selects_by_char_not_byte() {
        let mut s = String::from("olá mundo");
        let mut c = Caret { caret: 3, anchor: 3, pref_flow: None };
        c.insert(&mut s, "!"); // depois do "á"
        assert_eq!(s, "olá! mundo");
        assert_eq!(c.caret, 4);
        c.backspace(&mut s, false);
        c.backspace(&mut s, false);
        assert_eq!(s, "ol mundo");
        assert_eq!(c.caret, 2);
        c.delete(&mut s, false);
        assert_eq!(s, "olmundo");
        // seleção substituída pelo texto digitado
        c.caret = 5;
        c.anchor = 2;
        assert_eq!(c.selected(&s), "mun");
        c.insert(&mut s, "XY");
        assert_eq!(s, "olXYdo");
        assert_eq!((c.caret, c.anchor), (4, 4));
    }

    #[test]
    fn word_motion_and_double_click_selection() {
        let s = "hello  big world";
        assert_eq!(next_word(s, 0), 7, "pula a palavra e os espaços");
        assert_eq!(next_word(s, 7), 11);
        assert_eq!(prev_word(s, 11), 7);
        assert_eq!(prev_word(s, 7), 0);
        assert_eq!(word_at(s, 8), (7, 10));
        let jp = "日本語 テスト";
        assert_eq!(word_at(jp, 1), (0, 3), "ideogramas juntos são uma palavra");
        let mut t = String::from("one two three");
        let mut c = Caret { caret: 13, anchor: 13, pref_flow: None };
        c.backspace(&mut t, true);
        assert_eq!(t, "one two ");
    }

    #[test]
    fn utf8_safe_with_emoji_and_cjk() {
        let mut s = String::from("あい😀う");
        let mut c = Caret { caret: 3, anchor: 3, pref_flow: None };
        c.backspace(&mut s, false);
        assert_eq!(s, "あいう");
        c.insert(&mut s, "え");
        assert_eq!(s, "あいえう");
        c.caret = 99;
        c.clamp(&s);
        assert_eq!(c.caret, 4);
    }
}
