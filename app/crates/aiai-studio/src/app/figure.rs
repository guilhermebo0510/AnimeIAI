//! Figura no canvas (S25, à moda do Clip Studio): as formas diretas da caneta (aba "Desenho direto": reta, curva de dois
//! passos, polilinha, curva contínua, Bézier, retângulo, elipse, polígono, laço de preenchimento) e as abas "Linhas de
//! velocidade" e "Linhas de foco".
//!
//! - Reta, retângulo, elipse, polígono e as linhas são de arrastar (`Gesture::Shape`): Shift = ângulo em passos (reta),
//!   quadrado/círculo (formas); Alt durante o arrasto = a partir do centro (as opções "Do centro" e "Proporção fixa"
//!   invertem o sentido das teclas, como o Clip Studio).
//! - Curva contínua e polilinha são de cliques (`pen.rs`, `Gesture::PolyCurve`).
//! - Curva de dois passos, Bézier e laço moram aqui (`Gesture::Figure`, estado em `FigureState::live`): a curva arrasta a
//!   reta e depois puxa até o clique; a Bézier põe um ponto por clique (arrastar = alça; clicar no primeiro fecha; Enter,
//!   duplo clique ou botão direito termina; Esc/Backspace tira o último); o laço é contorno à mão, preenchido ao soltar.
//!
//! O resultado sai de `aiai_tools::figure` (o mesmo de `aiai_figure` no MCP): numa camada vetorial a linha é um traço
//! editável e o preenchimento uma região do cel; numa raster vira pixels (dentro da seleção); numa de preenchimento vira
//! região. "Linha e preenchimento" pinta o preenchimento com a cor secundária, como o Clip Studio. As linhas de
//! velocidade/foco vão para uma camada vetorial nova com os parâmetros guardados nela: arrastar de novo com a camada de
//! linhas ativa REFAZ as linhas dela (novo centro, direção, quantidade); os traços continuam editáveis um a um.

use super::commands::{self as cmds, NodeId};
use super::editor::*;
use super::ui_kit::Env;
use crate::i18n::tr;
use aiai_core::command::ApplyCtx;
use aiai_core::{ColorRef, Command, LayerData, LayerType, NodeKind};
use aiai_tools::figure::{BezierAnchor, FigureMode, FigureRequest, FigureShape, LINES_PARAM, figure_command_masked, lines_layer_command, lines_params};
use aiai_tools::fx2d::{LinesKind, LinesRequest};
use egui::{Color32, Stroke, Ui, Vec2};
use std::time::Instant;

/// Quantidade, largura (px) e comprimento (% da largura do canvas) das linhas de velocidade/foco.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LineOpts {
    pub count: u32,
    pub width: [f32; 2],
    pub length: [f32; 2],
}

/// Opções da Figura (painel Propriedades da ferramenta).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FigOpts {
    /// Formas fechadas: linha, preenchimento ou os dois.
    pub mode: FigureMode,
    /// Raio dos cantos do retângulo (px).
    pub radius: f32,
    /// Desenha a partir do centro (Alt inverte durante o arrasto).
    pub from_center: bool,
    /// Proporção fixa: quadrado, círculo, polígono regular (Shift inverte).
    pub keep_aspect: bool,
    /// Shift na reta e na polilinha: passos deste ângulo (graus).
    pub angle_step: f32,
    /// Cantos do polígono.
    pub sides: u32,
    pub speed: LineOpts,
    pub focus: LineOpts,
}

impl Default for FigOpts {
    fn default() -> Self {
        FigOpts {
            mode: FigureMode::Line,
            radius: 0.0,
            from_center: false,
            keep_aspect: false,
            angle_step: 15.0,
            sides: 5,
            // os mesmos números do Filtro > Renderizar
            speed: LineOpts { count: 90, width: [1.5, 5.0], length: [15.0, 45.0] },
            focus: LineOpts { count: 160, width: [2.0, 7.0], length: [25.0, 60.0] },
        }
    }
}

/// Forma em vários passos em andamento.
#[derive(Clone, Debug)]
pub(crate) enum Live {
    /// Curva de dois passos: `phase` 0 = arrastando a reta; 1 = puxando a curva até `bend` (o clique grava).
    Curve2 { from: Vec2, to: Vec2, bend: Vec2, phase: u8 },
    /// Bézier: pontos com alça (`None` = canto); `dragging` = arrastando a alça do último.
    Bezier { anchors: Vec<(Vec2, Option<Vec2>)>, dragging: bool, last_click: Instant },
    Lasso { pts: Vec<Vec2> },
}

#[derive(Default)]
pub(crate) struct FigureState {
    pub opts: FigOpts,
    pub live: Option<Live>,
}

/// Formas da aba "Desenho direto" e das abas de linhas.
pub(crate) const DIRECT: [PenShape; 9] = [PenShape::Line, PenShape::TwoStepCurve, PenShape::Polyline, PenShape::Curve, PenShape::Bezier, PenShape::Rect, PenShape::Ellipse, PenShape::Polygon, PenShape::Lasso];

/// Nome da forma (sub-ferramenta).
pub(crate) fn label_key(s: PenShape) -> &'static str {
    match s {
        PenShape::Freehand => "pen.freehand",
        PenShape::Line => "fig.line",
        PenShape::Curve => "fig.continuous_curve",
        PenShape::TwoStepCurve => "fig.curve",
        PenShape::Polyline => "fig.polyline",
        PenShape::Bezier => "fig.bezier",
        PenShape::Rect => "fig.rect",
        PenShape::Ellipse => "fig.ellipse",
        PenShape::Polygon => "fig.polygon",
        PenShape::Lasso => "fig.lasso",
        PenShape::SpeedLines => "fig.speed",
        PenShape::FocusLines => "fig.focus",
    }
}

/// A forma pode fechar (as opções de preenchimento valem).
pub(crate) fn can_fill(s: PenShape) -> bool {
    matches!(s, PenShape::Rect | PenShape::Ellipse | PenShape::Polygon | PenShape::Curve | PenShape::Polyline | PenShape::Bezier)
}

// ------------------------------------------------------------------------------------------------ entrada

/// Aperto com uma forma da Figura escolhida (sem gesto aberto).
pub(crate) fn press(env: &mut Env, c: Vec2) {
    let shape = env.ed.opts.pen_shape;
    env.ed.gesture = match shape {
        PenShape::Curve => super::pen::begin(c, false),
        PenShape::Polyline => super::pen::begin(c, true),
        PenShape::TwoStepCurve => {
            env.ed.figure.live = Some(Live::Curve2 { from: c, to: c, bend: c, phase: 0 });
            Gesture::Figure
        }
        PenShape::Bezier => {
            env.ed.figure.live = Some(Live::Bezier { anchors: vec![(c, None)], dragging: true, last_click: Instant::now() });
            Gesture::Figure
        }
        PenShape::Lasso => {
            env.ed.figure.live = Some(Live::Lasso { pts: vec![c] });
            Gesture::Figure
        }
        PenShape::Freehand => Gesture::None,
        _ => Gesture::Shape { start: c, cur: c },
    };
}

/// Shift na reta: o ângulo em passos de `step` graus.
pub(crate) fn snap_angle(a: Vec2, b: Vec2, step: f32) -> Vec2 {
    let d = b - a;
    let st = step.clamp(1.0, 90.0).to_radians();
    let ang = (d.y.atan2(d.x) / st).round() * st;
    a + Vec2::new(ang.cos(), ang.sin()) * d.length()
}

/// Movimento com o gesto da Figura: atualiza a forma viva. `true` = era dela.
pub(crate) fn moved(env: &mut Env, c: Vec2) -> bool {
    if !matches!(env.ed.gesture, Gesture::Figure) {
        return false;
    }
    let (shift, step) = (env.ed.shift, env.ed.figure.opts.angle_step);
    match env.ed.figure.live.as_mut() {
        Some(Live::Curve2 { from, to, bend, phase }) => {
            if *phase == 0 {
                *to = if shift { snap_angle(*from, c, step) } else { c };
                *bend = (*from + *to) / 2.0;
            } else {
                *bend = c;
            }
        }
        Some(Live::Bezier { anchors, dragging: true, .. }) => {
            if let Some(last) = anchors.last_mut() {
                let h = c - last.0;
                last.1 = (h.length() > 2.0).then_some(h);
            }
        }
        Some(Live::Lasso { pts }) => {
            if pts.last().is_none_or(|l| (*l - c).length() >= 1.5) {
                pts.push(c);
            }
        }
        _ => {}
    }
    true
}

/// Soltar com o gesto da Figura (o gesto já foi tirado de `ed.gesture`).
pub(crate) fn release(env: &mut Env, c: Vec2, scale: f32) {
    let Some(live) = env.ed.figure.live.take() else { return };
    match live {
        Live::Curve2 { from, to, phase: 0, .. } => {
            if (to - from).length() * scale < super::pen::CLICK_PX {
                return;
            }
            // segundo passo: a curva segue o cursor até o clique
            env.ed.figure.live = Some(Live::Curve2 { from, to, bend: c, phase: 1 });
            env.ed.gesture = Gesture::Figure;
        }
        Live::Bezier { anchors, last_click, .. } => {
            env.ed.figure.live = Some(Live::Bezier { anchors, dragging: false, last_click });
            env.ed.gesture = Gesture::Figure;
        }
        Live::Lasso { mut pts } => {
            pts.push(c);
            pts.dedup_by(|a, b| (*a - *b).length() < 0.5);
            if pts.len() >= 3 {
                commit(env, FigureShape::Lasso { points: pts.iter().map(|p| [p.x, p.y]).collect() });
            }
        }
        l @ Live::Curve2 { .. } => {
            env.ed.figure.live = Some(l);
            env.ed.gesture = Gesture::Figure;
        }
    }
}

/// Aperto com a forma em vários passos aberta (curva no 2º passo, Bézier).
pub(crate) fn press_open(env: &mut Env, c: Vec2, scale: f32, button: egui::PointerButton) {
    let Some(live) = env.ed.figure.live.clone() else {
        env.ed.gesture = Gesture::None;
        return;
    };
    match (live, button) {
        (Live::Curve2 { phase: 1, .. }, egui::PointerButton::Secondary) => cancel(env),
        (Live::Curve2 { from, to, phase: 1, .. }, egui::PointerButton::Primary) => {
            cancel(env);
            commit(env, FigureShape::Curve { from: [from.x, from.y], to: [to.x, to.y], through: [c.x, c.y] });
        }
        (Live::Bezier { .. }, egui::PointerButton::Secondary) => finish_bezier(env, false),
        (Live::Bezier { anchors, last_click, .. }, egui::PointerButton::Primary) => {
            let pts: Vec<Vec2> = anchors.iter().map(|a| a.0).collect();
            if super::pen::closes(&pts, false, c, scale) {
                return finish_bezier(env, true);
            }
            if last_click.elapsed().as_millis() < 350 && pts.last().is_some_and(|l| (*l - c).length() * scale < 6.0) {
                return finish_bezier(env, false);
            }
            if let Some(Live::Bezier { anchors, dragging, last_click }) = env.ed.figure.live.as_mut() {
                anchors.push((c, None));
                *dragging = true;
                *last_click = Instant::now();
            }
            env.ed.gesture = Gesture::Figure;
        }
        _ => {}
    }
}

fn cancel(env: &mut Env) {
    env.ed.figure.live = None;
    env.ed.gesture = Gesture::None;
}

/// Grava a Bézier (aberta ou fechada) e encerra.
fn finish_bezier(env: &mut Env, closed: bool) {
    let Some(Live::Bezier { mut anchors, .. }) = env.ed.figure.live.take() else { return };
    env.ed.gesture = Gesture::None;
    anchors.dedup_by(|a, b| (a.0 - b.0).length() < 0.5 && a.1.is_none());
    if anchors.len() < 2 {
        return;
    }
    let anchors = anchors.iter().map(|(p, h)| BezierAnchor { at: [p.x, p.y], handle: h.map(|h| [h.x, h.y]) }).collect();
    commit(env, FigureShape::Bezier { anchors, closed });
}

/// Teclas da forma aberta (antes dos atalhos gerais): Enter termina, Esc/Backspace tiram o último ponto (Bézier) ou
/// cancelam (curva). Trocar de ferramenta ou de forma no meio grava a Bézier e cancela a curva.
pub(crate) fn shortcuts(ctx: &egui::Context, env: &mut Env) {
    let Some(live) = env.ed.figure.live.as_ref() else { return };
    let want = match live {
        Live::Curve2 { .. } => PenShape::TwoStepCurve,
        Live::Bezier { .. } => PenShape::Bezier,
        Live::Lasso { .. } => PenShape::Lasso,
    };
    if *env.tool != crate::workspace::ToolId::Pen || env.ed.opts.pen_shape != want || !matches!(env.ed.gesture, Gesture::Figure) {
        return match want {
            PenShape::Bezier if matches!(env.ed.gesture, Gesture::Figure | Gesture::None) => finish_bezier(env, false),
            _ => {
                if matches!(env.ed.gesture, Gesture::Figure) {
                    env.ed.gesture = Gesture::None;
                }
                env.ed.figure.live = None;
            }
        };
    }
    use egui::{Key, Modifiers};
    let (enter, backs) = ctx.input_mut(|i| {
        let enter = i.consume_key(Modifiers::NONE, Key::Enter);
        let mut n = 0;
        while i.consume_key(Modifiers::NONE, Key::Escape) || i.consume_key(Modifiers::NONE, Key::Backspace) {
            n += 1;
        }
        (enter, n)
    });
    for _ in 0..backs {
        match env.ed.figure.live.as_mut() {
            Some(Live::Bezier { anchors, .. }) => {
                anchors.pop();
                if anchors.is_empty() {
                    cancel(env);
                }
            }
            _ => cancel(env),
        }
    }
    if enter {
        match env.ed.figure.live.clone() {
            Some(Live::Bezier { .. }) => finish_bezier(env, false),
            Some(Live::Curve2 { from, to, bend, phase: 1 }) => {
                cancel(env);
                commit(env, FigureShape::Curve { from: [from.x, from.y], to: [to.x, to.y], through: [bend.x, bend.y] });
            }
            _ => {}
        }
    }
}

/// Caixa [x, y, w, h] do arrasto de `a` a `b` com as opções e as teclas (Alt = do centro, Shift = proporção fixa).
pub(crate) fn drag_box(o: &FigOpts, shift: bool, alt: bool, a: Vec2, b: Vec2) -> [f32; 4] {
    let mut d = b - a;
    if o.keep_aspect != shift {
        let m = d.x.abs().max(d.y.abs());
        d = Vec2::new(m * if d.x < 0.0 { -1.0 } else { 1.0 }, m * if d.y < 0.0 { -1.0 } else { 1.0 });
    }
    if o.from_center != alt {
        [a.x - d.x.abs(), a.y - d.y.abs(), 2.0 * d.x.abs(), 2.0 * d.y.abs()]
    } else {
        [a.x, a.y, d.x, d.y]
    }
}

/// A forma de arrastar de `a` a `b` (sem as linhas de velocidade/foco).
pub(crate) fn drag_shape(env: &mut Env, a: Vec2, b: Vec2) -> Option<FigureShape> {
    let o = &env.ed.figure.opts;
    let (shift, alt) = (env.ed.shift, env.ed.alt);
    let rect = drag_box(o, shift, alt, a, b);
    Some(match env.ed.opts.pen_shape {
        PenShape::Line => {
            let b = if shift { snap_angle(a, b, o.angle_step) } else { super::rulers::snap_line_end(env, a, b) };
            FigureShape::Line { from: [a.x, a.y], to: [b.x, b.y] }
        }
        PenShape::Rect => FigureShape::Rect { rect, radius: o.radius, angle: 0.0 },
        PenShape::Ellipse => FigureShape::Ellipse { rect, angle: 0.0 },
        PenShape::Polygon => FigureShape::Polygon { center: [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0], radius: [rect[2].abs() / 2.0, rect[3].abs() / 2.0], sides: o.sides.clamp(3, 64), angle: 0.0 },
        _ => return None,
    })
}

/// Soltar um arrasto de forma (`Gesture::Shape`).
pub(crate) fn release_shape(env: &mut Env, start: Vec2, c: Vec2, scale: f32) {
    let shape = env.ed.opts.pen_shape;
    let tiny = (c - start).length() * scale < super::pen::CLICK_PX;
    match shape {
        // reta começada com um clique sem arrasto: vira polilinha (cliques põem os pontos)
        PenShape::Line if tiny => env.ed.gesture = super::pen::begin(start, true),
        PenShape::SpeedLines | PenShape::FocusLines => {
            if tiny {
                env.ed.notify(tr("fig.lines.drag"));
                return;
            }
            commit_lines(env, start, c, shape == PenShape::FocusLines);
        }
        _ if tiny => {}
        _ => {
            if let Some(s) = drag_shape(env, start, c) {
                commit(env, s);
            }
        }
    }
}

/// Polilinha/curva contínua de cliques (`pen::finish`).
pub(crate) fn commit_clicks(env: &mut Env, pts: Vec<Vec2>, line: bool, closed: bool) {
    let points: Vec<[f32; 2]> = pts.iter().map(|p| [p.x, p.y]).collect();
    commit(env, if line { FigureShape::Polyline { points, closed } } else { FigureShape::ContinuousCurve { points, closed } });
}

/// Cor principal e secundária do painel Cores (`ed.color` é sempre a de desenho, que pode ser a secundária).
pub(crate) fn main_sub(env: &Env) -> (ColorRef, ColorRef) {
    let other = env.ed.draw_ui.colors.other.clone();
    if env.ed.draw_ui.colors.sub_active { (other, env.ed.color.clone()) } else { (env.ed.color.clone(), other) }
}

/// Camada alvo da Figura: vetorial, raster ou de preenchimento; outra (texto, pasta, guia...) ganha uma vetorial nova
/// logo acima (como a caneta), com aviso.
fn figure_target(env: &mut Env) -> Option<super::tools::Target> {
    let ok = [LayerType::Vector, LayerType::Raster, LayerType::Fill];
    let cut = env.ed.active_cut(env.core)?;
    let active = env.ed.active_layer(env.core);
    if !active.as_ref().and_then(|l| cmds::layer_data(&env.core.project, l)).is_some_and(|l| ok.contains(&l.kind)) {
        match cmds::add_layer(env.core, &cut, LayerType::Vector, active.as_ref()) {
            Ok(id) => {
                let name = env.core.project.node(&id).map(|n| n.name.clone()).unwrap_or_default();
                env.ed.layer = Some(id);
                env.ed.cel_number = None;
                env.ed.notify(format!("{} \u{201c}{name}\u{201d}", tr("msg.auto_vector_layer")));
            }
            Err(e) => {
                env.ed.notify(e);
                return None;
            }
        }
    }
    super::tools::target(env, &ok, "fig.err.layer")
}

/// Grava a forma na camada ativa (um passo de desfazer).
pub(crate) fn commit(env: &mut Env, shape: FigureShape) {
    let Some(t) = figure_target(env) else { return };
    // linha com a cor de desenho; em "linha e preenchimento" o preenchimento vai com a outra (a secundária)
    let mode = env.ed.figure.opts.mode;
    let fill_color = match mode {
        FigureMode::LineFill => Some(env.ed.draw_ui.colors.other.clone()),
        _ => None,
    };
    let req = FigureRequest {
        cut: t.cut.clone(),
        frame: t.frame,
        layer: t.layer.clone(),
        shape,
        mode,
        width: env.ed.opts.pen_width.max(0.1),
        color: env.ed.color.clone(),
        fill_color,
        tip: aiai_core::VectorTip::Marker,
        opacity: 1.0,
        aliased: env.ed.opts.pen_aliased,
        selection: None,
    };
    let mask = if t.kind == LayerType::Raster { super::tools::paint_clip_mask(env) } else { None };
    let released = Instant::now();
    let planned = cmds::plan_with_prep(env.core, &t.cut, &t.layer, t.frame, env.ed.cel_number, vec![], |p, blobs| figure_command_masked(p, blobs, &req, mask.as_ref()).map_err(|e| e.to_string()));
    match planned {
        Ok(c) => {
            if env.ed.apply(env.core, c) {
                env.ed.begin_latency(released, "figure", env.core);
            }
        }
        Err(e) => env.ed.notify(e),
    }
}

// ------------------------------------------------------------------------------------------------ linhas

/// Onde entra uma camada nova acima de `active` (dentro dela, se for pasta; no topo do corte sem ativa).
fn place_above(p: &aiai_core::Project, cut: &NodeId, active: Option<&NodeId>) -> (NodeId, Option<usize>) {
    match active.and_then(|a| p.node(a).map(|n| (a, n))) {
        Some((a, n)) if matches!(&n.data, NodeKind::Layer(l) if l.kind == LayerType::Group) => (a.clone(), None),
        Some((a, n)) => match &n.parent {
            Some(par) => (par.clone(), p.node(par).and_then(|pn| pn.children.iter().position(|c| c == a)).map(|i| i + 1)),
            None => (cut.clone(), None),
        },
        None => (cut.clone(), None),
    }
}

/// Pedido das linhas do arrasto de `a` a `b` (sem a camada).
pub(crate) fn lines_request(env: &mut Env, cut: &NodeId, a: Vec2, b: Vec2, focus: bool) -> LinesRequest {
    let (w, h) = cmds::canvas_size(&env.core.project, cut);
    let d = b - a;
    let o = if focus { env.ed.figure.opts.focus } else { env.ed.figure.opts.speed };
    let kind = if focus {
        // Shift = área livre redonda
        let (rx, ry) = if env.ed.shift { (d.length(), d.length()) } else { (d.x.abs().max(8.0), d.y.abs().max(8.0)) };
        LinesKind::Focus { center: [a.x, a.y], clear: [rx, ry] }
    } else {
        LinesKind::Speed { angle: d.y.atan2(d.x).to_degrees(), area: [0.0, 0.0, w as f32, h as f32] }
    };
    let k = w as f32 / 100.0;
    LinesRequest {
        cut: cut.clone(),
        frame: env.ed.frame,
        layer: cut.clone(),
        kind,
        count: o.count.clamp(1, 2000),
        length: [o.length[0].min(o.length[1]) * k, o.length[1].max(o.length[0]) * k],
        width: [o.width[0].min(o.width[1]), o.width[1].max(o.width[0])],
        color: env.ed.color.clone(),
        // semente nova a cada gesto (o contador muda a cada comando), a mesma na prévia e no gravado
        seed: env.core.change_counter.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(h as u64),
    }
}

/// Linhas de velocidade/foco: refaz a camada de linhas ativa da Figura, ou cria uma vetorial nova acima da ativa (um
/// passo de desfazer).
pub(crate) fn commit_lines(env: &mut Env, a: Vec2, b: Vec2, focus: bool) {
    let Some(cut) = env.ed.active_cut(env.core) else { return env.ed.notify(tr("err.no_cut")) };
    let mut req = lines_request(env, &cut, a, b, focus);
    let active = env.ed.active_layer(env.core);
    let lines_layer = active.clone().filter(|l| cmds::layer_data(&env.core.project, l).is_some_and(|d| d.kind == LayerType::Vector && lines_params(d).is_some()));
    if let Some(layer) = lines_layer {
        if cmds::layer_data(&env.core.project, &layer).is_some_and(|l| l.locked) {
            return env.ed.notify(tr("err.layer_locked"));
        }
        req.layer = layer.clone();
        let planned = cmds::plan_with_prep(env.core, &cut, &layer, req.frame, env.ed.cel_number, vec![], |p, _| lines_layer_command(p, &req, true).map_err(|e| e.to_string()));
        match planned {
            Ok(c) => {
                env.ed.apply(env.core, c);
            }
            Err(e) => env.ed.notify(e),
        }
        return;
    }
    let p = &env.core.project;
    let (parent, index) = place_above(p, &cut, active.as_ref());
    let name = cmds::unique_layer_name(p, &parent, tr(if focus { "filter.focus_lines" } else { "filter.speed_lines" }));
    let (layer, cel) = (env.core.ids.next(), env.core.ids.next());
    let mut cmds_v = vec![
        Command::CreateNode { id: Some(layer.clone()), parent: Some(parent), index, name, data: NodeKind::Layer(LayerData::new(LayerType::Vector)) },
        Command::CreateNode { id: Some(cel.clone()), parent: Some(layer.clone()), index: None, name: "1".into(), data: NodeKind::Cel(aiai_core::CelData::new(1)) },
    ];
    // as linhas são calculadas sobre o documento com a camada nova já criada
    let mut tmp = env.core.project.clone();
    let ctx = ApplyCtx { blobs: &env.core.blobs, author: &env.core.author, revision: tmp.revision + 1 };
    for c in &cmds_v {
        if let Err(e) = c.apply(&mut tmp, &ctx) {
            return env.ed.notify(e.to_string());
        }
    }
    req.layer = layer.clone();
    match lines_layer_command(&tmp, &req, false) {
        Ok(c) => cmds_v.push(c),
        Err(e) => return env.ed.notify(e.to_string()),
    }
    let label = tr(if focus { "filter.focus_lines" } else { "filter.speed_lines" }).to_owned();
    if env.ed.apply(env.core, Command::Batch { commands: cmds_v, label: Some(label) }) {
        env.ed.layer = Some(layer);
        env.ed.cel_number = None;
    }
}

// ------------------------------------------------------------------------------------------------ prévia

fn ink(env: &Env) -> Color32 {
    let c = cmds::resolve_color(&env.core.project, &env.ed.color);
    super::ui_kit::rgba_to_color32(aiai_core::Rgba([c.0[0], c.0[1], c.0[2], 1.0]))
}

/// Contorno (px do canvas) da forma, como o motor desenha.
fn outline_pts(shape: &FigureShape) -> Vec<Vec2> {
    match shape.outline() {
        Ok(o) => aiai_tools::figure::outline_polygon(&o).into_iter().map(|p| Vec2::new(p[0], p[1])).chain(if o.closed { o.points.first().map(|q| Vec2::new(q.x, q.y)) } else { None }).collect(),
        Err(_) => Vec::new(),
    }
}

/// Preenchimento translúcido da prévia (polígono em leque a partir do centro: certo para formas convexas, o bastante
/// para a ideia nas outras).
fn fill_preview(painter: &egui::Painter, xf: &Xf, pts: &[Vec2], color: Color32) {
    if pts.len() < 3 {
        return;
    }
    let screen: Vec<egui::Pos2> = pts.iter().map(|p| xf.to_screen(*p)).collect();
    let mut mesh = egui::Mesh::default();
    let c = screen.iter().fold(egui::Vec2::ZERO, |a, p| a + p.to_vec2()) / screen.len() as f32;
    mesh.colored_vertex(c.to_pos2(), color);
    for p in &screen {
        mesh.colored_vertex(*p, color);
    }
    let n = screen.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn paint_outline(env: &Env, painter: &egui::Painter, xf: &Xf, shape: &FigureShape) {
    let pts = outline_pts(shape);
    let lasso = matches!(shape, FigureShape::Lasso { .. });
    let mode = if lasso { FigureMode::Fill } else { env.ed.figure.opts.mode };
    if shape.closed() && mode.fill() {
        let fill = if mode == FigureMode::LineFill { env.ed.draw_ui.colors.other.clone() } else { env.ed.color.clone() };
        let c = cmds::resolve_color(&env.core.project, &fill);
        fill_preview(painter, xf, &pts, super::ui_kit::rgba_to_color32(aiai_core::Rgba([c.0[0], c.0[1], c.0[2], 0.45])));
    }
    if mode.line() || !shape.closed() {
        super::canvas::polyline(painter, &pts, env.ed.opts.pen_width.max(0.5), ink(env), xf);
    } else {
        super::canvas::polyline(painter, &pts, 1.0 / xf.scale.max(1e-3), env.t.accent, xf);
    }
}

/// Prévia do arrasto de forma (`Gesture::Shape`).
pub(crate) fn paint_shape(env: &mut Env, painter: &egui::Painter, xf: &Xf, start: Vec2, cur: Vec2) {
    match env.ed.opts.pen_shape {
        PenShape::SpeedLines | PenShape::FocusLines => paint_lines_guide(env, painter, xf, start, cur),
        _ => {
            if let Some(s) = drag_shape(env, start, cur) {
                paint_outline(env, painter, xf, &s);
            }
        }
    }
}

/// Guia das linhas: a elipse livre e raios (foco), ou a direção com algumas paralelas (velocidade).
fn paint_lines_guide(env: &mut Env, painter: &egui::Painter, xf: &Xf, a: Vec2, b: Vec2) {
    let accent = env.t.accent;
    let st = Stroke::new(1.5, accent);
    let focus = env.ed.opts.pen_shape == PenShape::FocusLines;
    let Some(cut) = env.ed.cut.clone() else { return };
    let req = lines_request(env, &cut, a, b, focus);
    match req.kind {
        LinesKind::Focus { center, clear } => {
            let c = Vec2::new(center[0], center[1]);
            let ring: Vec<egui::Pos2> = (0..=48).map(|k| {
                let t = k as f32 / 48.0 * std::f32::consts::TAU;
                xf.to_screen(c + Vec2::new(clear[0] * t.cos(), clear[1] * t.sin()))
            }).collect();
            painter.add(egui::Shape::line(ring, st));
            for k in 0..24 {
                let t = k as f32 / 24.0 * std::f32::consts::TAU;
                let dir = Vec2::new(t.cos(), t.sin());
                let r_in = 1.0 / ((dir.x / clear[0]).powi(2) + (dir.y / clear[1]).powi(2)).sqrt();
                painter.line_segment([xf.to_screen(c + dir * r_in), xf.to_screen(c + dir * (r_in + req.length[1] * 0.5))], Stroke::new(1.0, accent.gamma_multiply(0.7)));
            }
        }
        LinesKind::Speed { angle, .. } => {
            let dir = Vec2::new(angle.to_radians().cos(), angle.to_radians().sin());
            let n = Vec2::new(-dir.y, dir.x);
            for k in -3..=3 {
                let o = n * (k as f32 * 18.0 / xf.scale.max(1e-3));
                painter.line_segment([xf.to_screen(a + o), xf.to_screen(b + o)], Stroke::new(if k == 0 { 2.0 } else { 1.0 }, accent.gamma_multiply(if k == 0 { 1.0 } else { 0.6 })));
            }
            painter.circle_filled(xf.to_screen(b), 4.0, accent);
        }
    }
}

/// Prévia da forma em vários passos (`Gesture::Figure`).
pub(crate) fn paint_live(env: &mut Env, painter: &egui::Painter, xf: &Xf) {
    let Some(live) = env.ed.figure.live.clone() else { return };
    let accent = env.t.accent;
    let dot = |p: Vec2, big: bool| painter.circle(xf.to_screen(p), if big { 4.5 } else { 3.5 }, Color32::WHITE, Stroke::new(1.4, accent));
    match live {
        Live::Curve2 { from, to, bend, phase } => {
            let shape = if phase == 0 { FigureShape::Line { from: [from.x, from.y], to: [to.x, to.y] } } else { FigureShape::Curve { from: [from.x, from.y], to: [to.x, to.y], through: [bend.x, bend.y] } };
            paint_outline(env, painter, xf, &shape);
            dot(from, false);
            dot(to, false);
        }
        Live::Bezier { anchors, dragging, .. } => {
            let mut a: Vec<BezierAnchor> = anchors.iter().map(|(p, h)| BezierAnchor { at: [p.x, p.y], handle: h.map(|h| [h.x, h.y]) }).collect();
            let cursor = env.ed.cursor.map(|q| xf.to_canvas(q));
            let pts: Vec<Vec2> = anchors.iter().map(|x| x.0).collect();
            let closing = !dragging && cursor.is_some_and(|q| super::pen::closes(&pts, false, q, xf.scale));
            if let (Some(q), false, false) = (cursor, dragging, closing) {
                a.push(BezierAnchor { at: [q.x, q.y], handle: None });
            }
            if a.len() >= 2 {
                paint_outline(env, painter, xf, &FigureShape::Bezier { anchors: a, closed: closing });
            }
            for (i, (p, h)) in anchors.iter().enumerate() {
                if let Some(h) = h {
                    let (s0, s1) = (xf.to_screen(*p - *h), xf.to_screen(*p + *h));
                    painter.line_segment([s0, s1], Stroke::new(1.0, accent));
                    painter.circle_filled(s0, 2.5, accent);
                    painter.circle_filled(s1, 2.5, accent);
                }
                dot(*p, i == 0);
            }
        }
        Live::Lasso { pts } => {
            let mut path = pts.clone();
            if let Some(f) = pts.first() {
                path.push(*f);
            }
            fill_preview(painter, xf, &path, {
                let c = cmds::resolve_color(&env.core.project, &env.ed.color);
                super::ui_kit::rgba_to_color32(aiai_core::Rgba([c.0[0], c.0[1], c.0[2], 0.35]))
            });
            super::canvas::polyline(painter, &path, 1.2 / xf.scale.max(1e-3), accent, xf);
        }
    }
}

// ------------------------------------------------------------------------------------------------ propriedades

fn small(ui: &mut Ui, env: &Env, key: &str) {
    ui.label(egui::RichText::new(tr(key)).size(12.0).color(env.t.text));
}

/// Linhas do painel Propriedades da ferramenta para a forma `shape`.
pub(crate) fn prop_rows(env: &mut Env, ui: &mut Ui, shape: PenShape) {
    let lines = matches!(shape, PenShape::SpeedLines | PenShape::FocusLines);
    if !lines && shape != PenShape::Lasso {
        ui.horizontal(|ui| {
            small(ui, env, "fig.opt.width");
            let r = ui.add(egui::DragValue::new(&mut env.ed.opts.pen_width).range(0.5..=200.0).speed(0.1).max_decimals(1).suffix(" px"));
            env.note("fig:width", r.rect);
        });
    }
    if can_fill(shape) {
        ui.horizontal(|ui| {
            small(ui, env, "fig.opt.mode");
            for (m, key) in [(FigureMode::Line, "fig.mode.line"), (FigureMode::Fill, "fig.mode.fill"), (FigureMode::LineFill, "fig.mode.line_fill")] {
                let on = env.ed.figure.opts.mode == m;
                let r = ui.selectable_label(on, tr(key));
                env.note(format!("fig:mode:{key}"), r.rect);
                if r.on_hover_text(tr("fig.mode.tip")).clicked() {
                    env.ed.figure.opts.mode = m;
                }
            }
        });
    }
    let o = &mut env.ed.figure.opts;
    match shape {
        PenShape::Rect | PenShape::Ellipse | PenShape::Polygon => {
            if shape == PenShape::Rect {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tr("fig.opt.radius")).size(12.0));
                    ui.add(egui::DragValue::new(&mut o.radius).range(0.0..=2000.0).speed(0.5).max_decimals(0).suffix(" px"));
                });
            }
            if shape == PenShape::Polygon {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tr("fig.opt.sides")).size(12.0));
                    ui.add(egui::DragValue::new(&mut o.sides).range(3..=64).speed(0.1));
                });
            }
            ui.checkbox(&mut o.from_center, tr("fig.opt.from_center")).on_hover_text(tr("fig.opt.from_center.tip"));
            ui.checkbox(&mut o.keep_aspect, tr("fig.opt.keep_aspect")).on_hover_text(tr("fig.opt.keep_aspect.tip"));
        }
        PenShape::Line | PenShape::Polyline | PenShape::TwoStepCurve => {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tr("fig.opt.angle_step")).size(12.0)).on_hover_text(tr("fig.opt.angle_step.tip"));
                ui.add(egui::DragValue::new(&mut o.angle_step).range(1.0..=90.0).speed(0.2).max_decimals(0).suffix("°"));
            });
        }
        PenShape::SpeedLines | PenShape::FocusLines => {
            let lo = if shape == PenShape::FocusLines { &mut o.focus } else { &mut o.speed };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tr("fig.opt.count")).size(12.0));
                ui.add(egui::DragValue::new(&mut lo.count).range(1..=2000).speed(1.0));
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tr("fig.opt.line_width")).size(12.0));
                ui.add(egui::DragValue::new(&mut lo.width[0]).range(0.2..=100.0).speed(0.1).max_decimals(1));
                ui.label("–");
                ui.add(egui::DragValue::new(&mut lo.width[1]).range(0.2..=100.0).speed(0.1).max_decimals(1).suffix(" px"));
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tr("fig.opt.length")).size(12.0)).on_hover_text(tr("fig.opt.length.tip"));
                ui.add(egui::DragValue::new(&mut lo.length[0]).range(1.0..=200.0).speed(0.5).max_decimals(0));
                ui.label("–");
                ui.add(egui::DragValue::new(&mut lo.length[1]).range(1.0..=200.0).speed(0.5).max_decimals(0).suffix("%"));
            });
        }
        _ => {}
    }
    if !lines {
        let r = ui.checkbox(&mut env.ed.opts.pen_aliased, tr("opt.aliased"));
        env.note("fig:aliased", r.rect);
    }
    let hint = match shape {
        PenShape::SpeedLines => "fig.hint.speed",
        PenShape::FocusLines => "fig.hint.focus",
        PenShape::TwoStepCurve => "fig.hint.curve",
        PenShape::Bezier => "fig.hint.bezier",
        PenShape::Curve | PenShape::Polyline => "fig.hint.clicks",
        PenShape::Lasso => "fig.hint.lasso",
        _ => "fig.hint.drag",
    };
    // camada de linhas ativa: o arrasto refaz as linhas dela
    let lines_active = lines
        && env.ed.layer.as_ref().and_then(|l| cmds::layer_data(&env.core.project, l)).is_some_and(|d| d.params.get(LINES_PARAM).is_some());
    ui.label(egui::RichText::new(tr(if lines_active { "fig.hint.lines_redo" } else { hint })).size(11.5).color(env.t.text_dim));
}

// ------------------------------------------------------------------------------------------------ miniaturas

/// Miniatura da forma na lista de sub-ferramentas (desenho próprio, no estilo das outras).
pub(crate) fn paint_thumb(env: &Env, p: &egui::Painter, r: egui::Rect, shape: PenShape) {
    let t = env.t;
    let ink = Stroke::new(1.6, t.text);
    let at = |fx: f32, fy: f32| egui::pos2(r.left() + r.width() * fx, r.top() + r.height() * fy);
    let dot = |q: egui::Pos2| p.rect_filled(egui::Rect::from_center_size(q, egui::vec2(4.0, 4.0)), 0.0, t.accent);
    let curve = |f: &dyn Fn(f32) -> egui::Pos2| p.add(egui::Shape::line((0..=32).map(|k| f(k as f32 / 32.0)).collect(), ink));
    match shape {
        PenShape::Line => {
            p.line_segment([at(0.1, 0.8), at(0.9, 0.2)], ink);
        }
        PenShape::TwoStepCurve => {
            curve(&|f| at(0.1 + 0.8 * f, 0.8 - 0.6 * (f * std::f32::consts::PI).sin()));
            p.line_segment([at(0.1, 0.8), at(0.9, 0.8)], Stroke::new(1.0, t.text_dim));
        }
        PenShape::Polyline => {
            let pts = vec![at(0.08, 0.75), at(0.32, 0.25), at(0.55, 0.7), at(0.92, 0.3)];
            p.add(egui::Shape::line(pts.clone(), ink));
            for q in pts {
                dot(q);
            }
        }
        PenShape::Curve => {
            curve(&|f| at(0.08 + 0.84 * f, 0.5 + 0.3 * (f * std::f32::consts::TAU).sin()));
            for f in [0.0, 0.25, 0.75, 1.0] {
                dot(at(0.08 + 0.84 * f, 0.5 + 0.3 * (f * std::f32::consts::TAU).sin()));
            }
        }
        PenShape::Bezier => {
            curve(&|f| {
                let u = 1.0 - f;
                let (x0, x1, x2, x3) = (0.1, 0.25, 0.75, 0.9);
                let (y0, y1, y2, y3) = (0.75, 0.05, 0.95, 0.25);
                at(u * u * u * x0 + 3.0 * u * u * f * x1 + 3.0 * u * f * f * x2 + f * f * f * x3, u * u * u * y0 + 3.0 * u * u * f * y1 + 3.0 * u * f * f * y2 + f * f * f * y3)
            });
            p.line_segment([at(0.1, 0.75), at(0.25, 0.05)], Stroke::new(1.0, t.accent));
            dot(at(0.1, 0.75));
            dot(at(0.9, 0.25));
            p.circle_filled(at(0.25, 0.05), 2.0, t.accent);
        }
        PenShape::Rect => {
            p.rect_stroke(egui::Rect::from_min_max(at(0.3, 0.18), at(0.7, 0.82)), 3.0, ink, egui::StrokeKind::Inside);
        }
        PenShape::Ellipse => {
            curve(&|f| {
                let a = f * std::f32::consts::TAU;
                at(0.5 + 0.22 * a.cos(), 0.5 + 0.34 * a.sin())
            });
        }
        PenShape::Polygon => {
            let pts: Vec<egui::Pos2> = (0..=5).map(|k| {
                let a = (-90.0 + 72.0 * k as f32).to_radians();
                at(0.5 + 0.2 * a.cos(), 0.55 + 0.36 * a.sin())
            }).collect();
            p.add(egui::Shape::line(pts, ink));
        }
        PenShape::Lasso => {
            let pts: Vec<egui::Pos2> = (0..=40).map(|k| {
                let a = k as f32 / 40.0 * std::f32::consts::TAU;
                let r = 1.0 + 0.18 * (3.0 * a).sin();
                at(0.5 + 0.2 * r * a.cos(), 0.5 + 0.32 * r * a.sin())
            }).collect();
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(at(0.5, 0.5), t.accent.gamma_multiply(0.55));
            for q in &pts {
                mesh.colored_vertex(*q, t.accent.gamma_multiply(0.55));
            }
            for i in 0..pts.len() as u32 - 1 {
                mesh.add_triangle(0, i + 1, i + 2);
            }
            p.add(egui::Shape::mesh(mesh));
            p.add(egui::Shape::line(pts, ink));
        }
        PenShape::SpeedLines => {
            for (k, (y, x0, x1)) in [(0.2, 0.15, 0.85), (0.38, 0.3, 0.95), (0.55, 0.05, 0.7), (0.72, 0.25, 0.9), (0.88, 0.4, 0.8)].into_iter().enumerate() {
                p.line_segment([at(x0, y), at(x1, y)], Stroke::new(if k % 2 == 0 { 1.6 } else { 1.0 }, t.text));
            }
        }
        PenShape::FocusLines => {
            let c = at(0.5, 0.5);
            for k in 0..16 {
                let a = k as f32 / 16.0 * std::f32::consts::TAU;
                let d = egui::vec2(a.cos() * r.width() * 0.5, a.sin() * r.height() * 0.5);
                p.line_segment([c + d * 0.45, c + d], Stroke::new(if k % 2 == 0 { 1.4 } else { 0.9 }, t.text));
            }
        }
        PenShape::Freehand => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_box_follows_the_options_and_keys() {
        let o = FigOpts::default();
        let (a, b) = (Vec2::new(100.0, 100.0), Vec2::new(160.0, 130.0));
        assert_eq!(drag_box(&o, false, false, a, b), [100.0, 100.0, 60.0, 30.0]);
        assert_eq!(drag_box(&o, true, false, a, b), [100.0, 100.0, 60.0, 60.0], "Shift: quadrado");
        assert_eq!(drag_box(&o, false, true, a, b), [40.0, 70.0, 120.0, 60.0], "Alt: do centro");
        let inv = FigOpts { from_center: true, keep_aspect: true, ..FigOpts::default() };
        assert_eq!(drag_box(&inv, true, true, a, b), [100.0, 100.0, 60.0, 30.0], "as teclas invertem as opções");
        // para cima/esquerda: a caixa fica do outro lado
        assert_eq!(drag_box(&o, true, false, a, Vec2::new(80.0, 40.0)), [100.0, 100.0, -60.0, -60.0]);
    }

    #[test]
    fn angle_snap_uses_the_step() {
        let a = Vec2::ZERO;
        let b = snap_angle(a, Vec2::new(100.0, 40.0), 45.0);
        assert!((b.x - b.y).abs() < 1e-3 || b.y.abs() < 1e-3);
        let b = snap_angle(a, Vec2::new(100.0, 20.0), 15.0);
        assert!((b.y.atan2(b.x).to_degrees() - 15.0).abs() < 1e-3);
    }

    use super::super::commands::{add_layer, quick_start};
    use crate::workspace::{ToolId, WorkspaceId};
    use aiai_core::{CoreSession, StrokeGeometry};

    struct Doc {
        core: CoreSession,
        ed: Editor,
        icons: crate::icons::Icons,
        notes: Vec<(String, egui::Rect)>,
        tool: ToolId,
        cut: NodeId,
    }

    impl Doc {
        fn new(kind: LayerType) -> (Doc, NodeId) {
            let mut core = CoreSession::for_tests("fig");
            core.project.settings.width = 400;
            core.project.settings.height = 300;
            let cut = quick_start(&mut core, 4, None).unwrap();
            let l = add_layer(&mut core, &cut, kind, None).unwrap();
            let mut ed = Editor::default();
            ed.cut = Some(cut.clone());
            ed.layer = Some(l.clone());
            (Doc { core, ed, icons: Default::default(), notes: Vec::new(), tool: ToolId::Pen, cut }, l)
        }
        fn env(&mut self) -> Env<'_> {
            Env { t: crate::theme::Tokens::dark(), core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut self.notes, tool: &mut self.tool, ws: WorkspaceId::Animacao }
        }
        fn cel(&self, layer: &NodeId) -> aiai_core::CelData {
            let c = cmds::exposed_cel(&self.core.project, &self.cut, layer, 0).unwrap();
            cmds::cel_data(&self.core.project, &c).unwrap().clone()
        }
    }

    /// Bézier pelo caminho do canvas: clique (canto), clique e arrasto (alça), clique, Enter → um traço editável com a alça.
    #[test]
    fn bezier_by_clicks_and_drags_commits_a_spline_with_handles() {
        let (mut d, l) = Doc::new(LayerType::Vector);
        d.ed.opts.pen_shape = PenShape::Bezier;
        let mut env = d.env();
        press(&mut env, Vec2::new(50.0, 200.0));
        env.ed.gesture = Gesture::None;
        release(&mut env, Vec2::new(50.0, 200.0), 1.0);
        assert!(matches!(env.ed.gesture, Gesture::Figure));
        press_open(&mut env, Vec2::new(200.0, 100.0), 1.0, egui::PointerButton::Primary);
        assert!(moved(&mut env, Vec2::new(260.0, 100.0)));
        env.ed.gesture = Gesture::None;
        release(&mut env, Vec2::new(260.0, 100.0), 1.0);
        press_open(&mut env, Vec2::new(350.0, 200.0), 1.0, egui::PointerButton::Primary);
        env.ed.gesture = Gesture::None;
        release(&mut env, Vec2::new(350.0, 200.0), 1.0);
        press_open(&mut env, Vec2::new(380.0, 250.0), 1.0, egui::PointerButton::Secondary);
        assert!(env.ed.figure.live.is_none() && matches!(env.ed.gesture, Gesture::None));
        let cel = d.cel(&l);
        assert_eq!(cel.strokes.len(), 1);
        let s = &cel.strokes[0];
        assert_eq!((s.geometry, s.points.len()), (StrokeGeometry::Spline, 3));
        assert!(s.points[0].corner && s.points[1].handle == Some([60.0, 0.0]) && s.points[2].corner);
    }

    /// Curva de dois passos: arrasta a reta, solta, o clique grava a curva passando por ele.
    #[test]
    fn two_step_curve_commits_on_the_second_click() {
        let (mut d, l) = Doc::new(LayerType::Vector);
        d.ed.opts.pen_shape = PenShape::TwoStepCurve;
        let mut env = d.env();
        press(&mut env, Vec2::new(50.0, 200.0));
        moved(&mut env, Vec2::new(350.0, 200.0));
        env.ed.gesture = Gesture::None;
        release(&mut env, Vec2::new(350.0, 200.0), 1.0);
        assert!(matches!(env.ed.figure.live, Some(Live::Curve2 { phase: 1, .. })));
        press_open(&mut env, Vec2::new(200.0, 80.0), 1.0, egui::PointerButton::Primary);
        let s = &d.cel(&l).strokes[0];
        let samp = aiai_paint::sample_stroke(s);
        assert!(samp.iter().any(|q| (q.x - 200.0).abs() < 1.5 && (q.y - 80.0).abs() < 1.5), "passa pelo clique");
    }

    /// Retângulo em "linha e preenchimento" numa raster: pixels com a cor secundária dentro e a de desenho na borda.
    #[test]
    fn rect_line_and_fill_on_raster_uses_the_sub_colour() {
        let (mut d, l) = Doc::new(LayerType::Raster);
        d.ed.opts.pen_shape = PenShape::Rect;
        d.ed.opts.pen_width = 6.0;
        d.ed.figure.opts.mode = FigureMode::LineFill;
        d.ed.color = ColorRef::Direct { rgba: aiai_core::Rgba([1.0, 0.0, 0.0, 1.0]) };
        d.ed.draw_ui.colors.other = ColorRef::Direct { rgba: aiai_core::Rgba([0.0, 0.0, 1.0, 1.0]) };
        let mut env = d.env();
        release_shape(&mut env, Vec2::new(100.0, 100.0), Vec2::new(300.0, 200.0), 1.0);
        assert!(!d.cel(&l).tiles.is_empty());
        let img = aiai_render::render_frame(&d.core.project, &d.core.blobs, &d.cut, 0, &aiai_render::RenderOptions { camera: false, ..Default::default() }).unwrap().image;
        assert!(img.get(200, 150)[2] > 0.9, "dentro: azul");
        assert!(img.get(100, 150)[0] > 0.9, "borda: vermelho");
    }

    /// Linhas de foco: a primeira vez cria a camada; com ela ativa, o arrasto refaz (a quantidade de traços segue a opção).
    #[test]
    fn focus_lines_create_a_layer_then_redo_it() {
        let (mut d, _) = Doc::new(LayerType::Vector);
        d.ed.opts.pen_shape = PenShape::FocusLines;
        d.ed.figure.opts.focus.count = 30;
        let n0 = cmds::layer_rows(&d.core.project, &d.cut).len();
        let mut env = d.env();
        release_shape(&mut env, Vec2::new(200.0, 150.0), Vec2::new(260.0, 190.0), 1.0);
        let layer = env.ed.layer.clone().unwrap();
        assert_eq!(cmds::layer_rows(&d.core.project, &d.cut).len(), n0 + 1);
        assert_eq!(d.cel(&layer).strokes.len(), 30);
        d.ed.figure.opts.focus.count = 12;
        let mut env = d.env();
        release_shape(&mut env, Vec2::new(100.0, 100.0), Vec2::new(150.0, 130.0), 1.0);
        assert_eq!(cmds::layer_rows(&d.core.project, &d.cut).len(), n0 + 1, "refez na mesma camada");
        assert_eq!(d.cel(&layer).strokes.len(), 12);
        d.core.undo().unwrap();
        assert_eq!(d.cel(&layer).strokes.len(), 30, "um passo de desfazer");
    }

    #[test]
    fn every_shape_has_a_label_in_both_languages() {
        use crate::i18n::{Lang, tr_in};
        for s in DIRECT.into_iter().chain([PenShape::SpeedLines, PenShape::FocusLines]) {
            for l in [Lang::Pt, Lang::En] {
                assert_ne!(tr_in(l, label_key(s)), label_key(s), "{s:?} sem nome em {l:?}");
            }
        }
    }
}
