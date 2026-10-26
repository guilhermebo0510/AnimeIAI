//! Corrigir linha no canvas (S25, à moda do Clip Studio): as sub-ferramentas do Corrigir linha além dos pontos de controle
//! (que são o Editar pontos, `nodes.rs`) e a aba Remover poeira. A lógica mora em `aiai_tools::line_fix` (a mesma da
//! bridge, `LINE_FIX`); aqui ficam o gesto, a prévia, as opções e a miniatura de cada uma.
//!
//! - Puxar linha: arrastar a linha a puxa; a prévia é o motor redesenhando o traço (como o Editar pontos).
//! - Simplificar, Conectar, Ajustar largura, Redesenhar largura: pincel (tamanho na paleta Tamanho do pincel); a prévia
//!   desenha por cima o que vai ficar e soltar grava (um passo de desfazer).
//! - Redesenhar linha: desenhar de um lugar da linha a outro troca aquele trecho.
//! - Remover poeira / Selecionar poeira: clique = a camada inteira (ou dentro da seleção); arrastar = só na caixa.
//!
//! Tudo em px do cel (o movimento da camada é desfeito); tamanhos e distâncias das opções são px do canvas.

use super::commands::{self as cmds, NodeId};
use super::editor::Gesture;
use super::ui_kit::Env;
use crate::i18n::tr;
use aiai_core::*;
use aiai_paint::Affine;
use aiai_tools::line_fix as lf;
use egui::{Color32, Pos2, Rect, Stroke as EStroke, Ui, Vec2};
use std::time::Instant;

/// Sub-ferramenta do Corrigir linha (as que não são pontos de controle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FixTool {
    Pinch,
    Simplify,
    Connect,
    Width,
    Redraw,
    RedrawWidth,
    Dust,
    SelectDust,
}

impl FixTool {
    /// Aba Corrigir linha (depois dos pontos de controle), na ordem do Clip Studio.
    pub const LINE: [FixTool; 6] = [FixTool::Pinch, FixTool::Simplify, FixTool::Connect, FixTool::Width, FixTool::Redraw, FixTool::RedrawWidth];
    /// Aba Remover poeira.
    pub const DUST: [FixTool; 2] = [FixTool::Dust, FixTool::SelectDust];

    pub fn key(self) -> &'static str {
        match self {
            FixTool::Pinch => "pinch",
            FixTool::Simplify => "simplify",
            FixTool::Connect => "connect",
            FixTool::Width => "width",
            FixTool::Redraw => "redraw",
            FixTool::RedrawWidth => "redraw_width",
            FixTool::Dust => "dust",
            FixTool::SelectDust => "select_dust",
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            FixTool::Pinch => "fix.pinch",
            FixTool::Simplify => "fix.simplify",
            FixTool::Connect => "fix.connect",
            FixTool::Width => "fix.width",
            FixTool::Redraw => "fix.redraw",
            FixTool::RedrawWidth => "fix.redraw_width",
            FixTool::Dust => "fix.dust",
            FixTool::SelectDust => "fix.select_dust",
        }
    }

    pub fn tip_key(self) -> &'static str {
        match self {
            FixTool::Pinch => "fix.pinch.tip",
            FixTool::Simplify => "fix.simplify.tip",
            FixTool::Connect => "fix.connect.tip",
            FixTool::Width => "fix.width.tip",
            FixTool::Redraw => "fix.redraw.tip",
            FixTool::RedrawWidth => "fix.redraw_width.tip",
            FixTool::Dust => "fix.dust.tip",
            FixTool::SelectDust => "fix.select_dust.tip",
        }
    }

    /// Age como pincel (o tamanho é o da paleta Tamanho do pincel).
    pub fn has_size(self) -> bool {
        matches!(self, FixTool::Simplify | FixTool::Connect | FixTool::Width | FixTool::RedrawWidth)
    }

    pub fn dust(self) -> bool {
        matches!(self, FixTool::Dust | FixTool::SelectDust)
    }
}

/// Opções das sub-ferramentas (um campo só em `ToolOptions`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FixOptions {
    /// Sub-ferramenta em uso; `None` = pontos de controle (os modos do Editar pontos).
    pub tool: Option<FixTool>,
    /// Diâmetro do pincel (px do canvas).
    pub size: f32,
    pub pinch_range: f32,
    pub pinch_level: f32,
    pub pinch_fix_ends: bool,
    /// Força do Simplificar 0..100 (tolerância de `lf::simplify_tolerance`).
    pub simplify_strength: f32,
    pub connect_distance: f32,
    pub connect_smooth: bool,
    pub width_mode: lf::WidthMode,
    /// Engrossar/afinar: px; escalar: fator; largura fixa: px.
    pub width_px: f32,
    pub width_scale: f32,
    pub width_fixed: f32,
    pub width_whole: bool,
    /// Redesenhar linha: distância máxima das pontas do traço novo até a linha.
    pub redraw_tol: f32,
    pub dust_size: f32,
    pub dust_mode: lf::DustMode,
}

impl Default for FixOptions {
    fn default() -> Self {
        FixOptions {
            tool: None,
            size: 30.0,
            pinch_range: 80.0,
            pinch_level: 0.5,
            pinch_fix_ends: false,
            simplify_strength: 25.0,
            connect_distance: 24.0,
            connect_smooth: true,
            width_mode: lf::WidthMode::Widen,
            width_px: 1.0,
            width_scale: 1.25,
            width_fixed: 3.0,
            width_whole: false,
            redraw_tol: 12.0,
            dust_size: 6.0,
            dust_mode: lf::DustMode::Opaque,
        }
    }
}

/// Uma sub-ferramenta do Corrigir linha está em uso (com Editar pontos ativo).
pub(crate) fn active(env: &Env) -> bool {
    *env.tool == crate::workspace::ToolId::Points && env.ed.opts.fix.tool.is_some()
}

// ------------------------------------------------------------------------------------------------ gesto

/// Gesto em andamento (no espaço do cel da camada ativa).
pub(crate) struct FixGesture {
    tool: FixTool,
    cut: NodeId,
    layer: NodeId,
    cel: NodeId,
    fwd: Option<Affine>,
    inv: Option<Affine>,
    /// Caminho (x, y, pressão) no cel.
    path: Vec<[f32; 3]>,
    /// Começo e posição atual no canvas (caixa da poeira).
    start: Vec2,
    cur: Vec2,
    /// Puxar linha: o traço agarrado como estava.
    grabbed: Option<Stroke>,
    /// Prévia: os traços como vão ficar e os que somem.
    preview: Vec<Stroke>,
    removed: Vec<Id>,
    dirty: bool,
    t0: Instant,
    /// Px do canvas por px da tela.
    pix: f32,
}

struct Ctx {
    cut: NodeId,
    layer: NodeId,
    cel: NodeId,
    kind: LayerType,
    fwd: Option<Affine>,
    inv: Option<Affine>,
}

fn ctx(env: &mut Env) -> Option<Ctx> {
    let cut = env.ed.active_cut(env.core)?;
    let layer = env.ed.active_layer(env.core)?;
    let l = cmds::layer_data(&env.core.project, &layer)?;
    if l.locked {
        return None;
    }
    let kind = l.kind;
    let cel = cmds::exposed_cel(&env.core.project, &cut, &layer, env.ed.frame)?;
    let fwd = cmds::layer_placement(&env.core.project, &cut, &layer, env.ed.frame);
    let inv = fwd.and_then(|a| a.inverse());
    Some(Ctx { cut, layer, cel, kind, fwd: inv.and(fwd), inv })
}

fn map(a: &Option<Affine>, p: Vec2) -> Vec2 {
    match a {
        Some(a) => {
            let (x, y) = a.apply(p.x, p.y);
            Vec2::new(x, y)
        }
        None => p,
    }
}

/// Px do cel por px do canvas.
fn cel_scale(inv: &Option<Affine>) -> f32 {
    (map(inv, Vec2::new(1.0, 0.0)) - map(inv, Vec2::ZERO)).length().max(1e-6)
}

fn strokes_of(env: &Env, cel: &NodeId) -> Vec<Stroke> {
    cmds::cel_data(&env.core.project, cel).map(|d| d.strokes.clone()).unwrap_or_default()
}

/// Clique no canvas com uma sub-ferramenta do Corrigir linha. `pix` = 1 px de tela em px do canvas.
pub(crate) fn press(env: &mut Env, c_canvas: Vec2, pix: f32, button: egui::PointerButton) {
    if button != egui::PointerButton::Primary {
        return;
    }
    let Some(tool) = env.ed.opts.fix.tool else { return };
    let Some(c) = ctx(env) else {
        env.ed.notify(tr(if tool.dust() { "fix.msg.need_layer" } else { "err.need_vector" }));
        return;
    };
    if tool.dust() && !matches!(c.kind, LayerType::Vector | LayerType::Raster) {
        env.ed.notify(tr("fix.msg.need_layer"));
        return;
    }
    if !tool.dust() && c.kind != LayerType::Vector {
        env.ed.notify(tr("err.need_vector"));
        return;
    }
    let p = map(&c.inv, c_canvas);
    let mut g = FixGesture {
        tool,
        cut: c.cut,
        layer: c.layer,
        cel: c.cel,
        fwd: c.fwd,
        inv: c.inv,
        path: vec![[p.x, p.y, env.ed.pressure]],
        start: c_canvas,
        cur: c_canvas,
        grabbed: None,
        preview: Vec::new(),
        removed: Vec::new(),
        dirty: true,
        t0: Instant::now(),
        pix,
    };
    if tool == FixTool::Pinch {
        // o traço mais perto (pela borda desenhada), como o Editar pontos escolhe
        let tol = 12.0 * pix * cel_scale(&g.inv);
        let best = strokes_of(env, &g.cel)
            .into_iter()
            .map(|s| {
                let d = aiai_paint::sample_stroke(&s).iter().map(|q| ((Vec2::new(q.x, q.y) - p).length() - q.r).max(0.0)).fold(f32::MAX, f32::min);
                (s, d)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((s, d)) if d <= tol => g.grabbed = Some(s),
            _ => {
                env.ed.notify(tr("fix.msg.no_line"));
                return;
            }
        }
    }
    env.ed.gesture = Gesture::LineFix(Box::new(g));
}

pub(crate) fn drag(env: &mut Env, c_canvas: Vec2) {
    let pr = env.ed.pressure;
    let Gesture::LineFix(g) = &mut env.ed.gesture else { return };
    let p = map(&g.inv, c_canvas);
    g.cur = c_canvas;
    let step = 0.75 * g.pix * cel_scale(&g.inv);
    if g.path.last().is_none_or(|l| (l[0] - p.x).hypot(l[1] - p.y) >= step) {
        g.path.push([p.x, p.y, pr]);
        g.dirty = true;
    }
}

/// O que o gesto faz com os traços do cel: (traços que mudam, traços que somem) ou o erro a mostrar.
fn compute(o: &super::editor::ToolOptions, g: &FixGesture, strokes: &[Stroke]) -> Result<(Vec<Stroke>, Vec<Id>), String> {
    let f = &o.fix;
    let k = cel_scale(&g.inv);
    let radius = (f.size * 0.5).max(0.5) * k;
    let path2: Vec<[f32; 2]> = g.path.iter().map(|q| [q[0], q[1]]).collect();
    let mut out: Vec<Stroke> = Vec::new();
    let mut removed = Vec::new();
    match g.tool {
        FixTool::Pinch => {
            let Some(s) = &g.grabbed else { return Ok((out, removed)) };
            let (a, b) = (g.path[0], *g.path.last().expect("caminho com o clique"));
            if (b[0] - a[0]).hypot(b[1] - a[1]) < 0.25 * k {
                return Ok((out, removed));
            }
            let p = lf::PinchParams { range: f.pinch_range.max(2.0) * k, level: f.pinch_level, fix_ends: f.pinch_fix_ends };
            out.push(lf::pinch(s, [a[0], a[1]], [b[0], b[1]], &p)?);
        }
        FixTool::Simplify => {
            let tol = lf::simplify_tolerance(f.simplify_strength) * k;
            for s in strokes {
                if let Some((ns, _)) = lf::simplify_area(s, &path2, radius, tol)? {
                    out.push(ns);
                }
            }
        }
        FixTool::Connect => {
            let r = lf::connect(strokes, &path2, radius, &lf::ConnectParams { distance: f.connect_distance.max(0.5) * k, smooth: f.connect_smooth });
            out = r.changed;
            removed = r.removed;
        }
        FixTool::Width => {
            let amount = match f.width_mode {
                lf::WidthMode::Widen | lf::WidthMode::Narrow => f.width_px * k,
                lf::WidthMode::Scale => f.width_scale,
                lf::WidthMode::Fixed => f.width_fixed * k,
            };
            let p = lf::WidthParams { mode: f.width_mode, amount, whole_line: f.width_whole };
            out = strokes.iter().filter_map(|s| lf::adjust_width(s, &path2, radius, &p)).collect();
        }
        FixTool::RedrawWidth => {
            out = strokes.iter().filter_map(|s| lf::redraw_width(s, &g.path, radius)).collect();
        }
        FixTool::Redraw => {
            let tol = f.redraw_tol.max(1.0) * k;
            if g.path.len() < 2 {
                return Ok((out, removed));
            }
            let Some(i) = lf::redraw_pick(strokes, &g.path, tol) else { return Err(tr("fix.msg.redraw_fail").to_owned()) };
            out.push(lf::redraw(&strokes[i], &g.path, tol)?);
        }
        FixTool::Dust | FixTool::SelectDust => {}
    }
    for s in &out {
        s.validate()?;
    }
    Ok((out, removed))
}

/// Recalcula a prévia dos pincéis (uma vez por quadro, quando o caminho andou).
fn refresh(env: &mut Env) {
    let Gesture::LineFix(g) = &env.ed.gesture else { return };
    if !g.dirty || matches!(g.tool, FixTool::Redraw | FixTool::Dust | FixTool::SelectDust) {
        return;
    }
    let strokes = strokes_of(env, &g.cel);
    let res = compute(&env.ed.opts, g, &strokes);
    if let Gesture::LineFix(g) = &mut env.ed.gesture {
        g.dirty = false;
        (g.preview, g.removed) = res.unwrap_or_default();
    }
}

pub(crate) fn release(env: &mut Env, g: FixGesture) {
    if g.tool.dust() {
        return dust_release(env, &g);
    }
    let strokes = strokes_of(env, &g.cel);
    let (changed, removed) = match compute(&env.ed.opts, &g, &strokes) {
        Ok(v) => v,
        Err(e) => {
            env.ed.notify(format!("{}: {e}", tr(g.tool.label_key())));
            return;
        }
    };
    if changed.is_empty() && removed.is_empty() {
        if g.tool == FixTool::Connect {
            env.ed.notify(tr("fix.msg.connect_none"));
        }
        return;
    }
    if let Err(e) = cmds::run(env.core, lf::fix_command(&g.cel, changed, removed)) {
        env.ed.notify(format!("{}: {e}", tr(g.tool.label_key())));
    }
}

/// Remover / selecionar poeira: a camada inteira (ou a seleção), ou a caixa arrastada.
fn dust_release(env: &mut Env, g: &FixGesture) {
    let (w, h) = cmds::canvas_size(&env.core.project, &g.cut);
    let dragged = (g.cur - g.start).length() > 4.0 * g.pix;
    let mut within: Option<Vec<f32>> = env.ed.selection.as_ref().and_then(|s| super::selection::area_cov(s, w, h)).map(|c| c.cov);
    if dragged {
        let (a, b) = (g.start.min(g.cur), g.start.max(g.cur));
        let m = within.get_or_insert_with(|| vec![1.0; (w * h) as usize]);
        for (i, v) in m.iter_mut().enumerate() {
            let (x, y) = ((i as u32 % w) as f32 + 0.5, (i as u32 / w) as f32 + 0.5);
            if x < a.x || y < a.y || x >= b.x || y >= b.y {
                *v = 0.0;
            }
        }
    }
    let o = &env.ed.opts.fix;
    let size = o.dust_size.max(0.5) * cel_scale(&g.inv);
    let scan = match lf::dust_scan(&env.core.project, &env.core.blobs, &g.cut, &g.layer, env.ed.frame, size, o.dust_mode, within.as_deref()) {
        Ok(s) => s,
        Err(e) => {
            env.ed.notify(format!("{}: {e}", tr(g.tool.label_key())));
            return;
        }
    };
    let n = scan.count();
    if n == 0 {
        env.ed.notify(tr("fix.msg.dust_none"));
        return;
    }
    if g.tool == FixTool::SelectDust {
        let cov = aiai_paint::select::Selection { width: w, height: h, cov: scan.canvas_coverage() };
        env.ed.selection = super::selection::selection_from_cov(cov);
        env.ed.notify(format!("{} {n}", tr("fix.msg.dust_selected")));
        return;
    }
    let Some(cmd) = lf::dust_command(&mut env.core.blobs, scan) else { return };
    match cmds::run(env.core, cmd) {
        Ok(_) => env.ed.notify(format!("{} {n}", tr("fix.msg.dust_removed"))),
        Err(e) => env.ed.notify(format!("{}: {e}", tr(g.tool.label_key()))),
    }
}

/// Puxar linha: a linha de verdade anda junto (o motor redesenha o traço com o original escondido).
pub(crate) fn live(env: &mut Env, ctx_ui: &egui::Context, screen_scale: f32) {
    let Gesture::LineFix(g) = &env.ed.gesture else { return };
    if g.tool != FixTool::Pinch {
        return;
    }
    let Some(s) = g.grabbed.clone() else { return };
    let (t0, cut, layer, cel, fwd) = (g.t0, g.cut.clone(), g.layer.clone(), g.cel.clone(), g.fwd);
    if env.ed.live_stroke.started_for() != Some(t0) {
        super::nodes::start_live_for(env, ctx_ui, t0, screen_scale, &cut, &layer, &cel, fwd, &s);
    }
    let Gesture::LineFix(g) = &env.ed.gesture else { return };
    let strokes = strokes_of(env, &g.cel);
    let Ok((ns, _)) = compute(&env.ed.opts, g, &strokes) else { return };
    let ns = ns.into_iter().next().unwrap_or(s);
    let last = g.path.last().copied().unwrap_or([0.0; 3]);
    // contagem = posição do arrasto (manda de novo só quando ela muda)
    let count = ((last[0] * 64.0) as i64 as usize).wrapping_mul(1_000_003) ^ ((last[1] * 64.0) as i64 as usize) ^ (7 << 40);
    env.ed.live_stroke.feed(Vec::new(), Vec::new(), Some(ns), count);
}

// ------------------------------------------------------------------------------------------------ desenho

/// Faixa (malha) do traço desenhado com a largura de verdade.
fn stroke_mesh(s: &Stroke, to: &dyn Fn(Vec2) -> Pos2, scale: f32, color: Color32) -> egui::Shape {
    let smp = aiai_paint::sample_stroke(s);
    let mut m = egui::Mesh::default();
    let step = (smp.len() / 1500).max(1);
    let pts: Vec<&aiai_paint::StrokeSample> = smp.iter().step_by(step).chain(smp.last()).collect();
    for (k, q) in pts.iter().enumerate() {
        let a = pts[k.saturating_sub(1)];
        let b = pts[(k + 1).min(pts.len() - 1)];
        let d = Vec2::new(b.x - a.x, b.y - a.y);
        let n = if d.length() > 1e-6 { Vec2::new(-d.y, d.x).normalized() } else { Vec2::new(0.0, 1.0) };
        let c = Vec2::new(q.x, q.y);
        let r = q.r.max(0.5 / scale.max(1e-3));
        m.colored_vertex(to(c + n * r), color);
        m.colored_vertex(to(c - n * r), color);
        if k > 0 {
            let i = (k as u32) * 2;
            m.add_triangle(i - 2, i - 1, i);
            m.add_triangle(i - 1, i + 1, i);
        }
    }
    egui::Shape::mesh(m)
}

/// Prévia, contorno do pincel e pontas (Conectar) por cima do canvas.
pub(crate) fn paint(env: &mut Env, painter: &egui::Painter, to_screen: &dyn Fn(Vec2) -> Pos2, accent: Color32) {
    let Some(tool) = env.ed.opts.fix.tool else { return };
    refresh(env);
    let scale = (to_screen(Vec2::new(100.0, 0.0)) - to_screen(Vec2::ZERO)).length() / 100.0;
    let Some(c) = ctx(env) else { return };
    let to = |p: Vec2| to_screen(map(&c.fwd, p));
    let size = env.ed.opts.fix.size;
    // pontas soltas, para mirar o Conectar
    if tool == FixTool::Connect && c.kind == LayerType::Vector {
        for s in strokes_of(env, &c.cel).iter().filter(|s| !s.closed && s.points.len() >= 2) {
            for q in [s.points[0], s.points[s.points.len() - 1]] {
                painter.circle_stroke(to(Vec2::new(q.x, q.y)), 3.5, EStroke::new(1.3, accent));
            }
        }
    }
    if let Gesture::LineFix(g) = &env.ed.gesture {
        let path: Vec<Pos2> = g.path.iter().map(|q| to(Vec2::new(q[0], q[1]))).collect();
        match g.tool {
            FixTool::Pinch => {
                if env.ed.live_stroke.overlay.is_none() {
                    for s in &g.preview {
                        let sp: Vec<Pos2> = aiai_paint::sample_stroke(s).iter().map(|q| to(Vec2::new(q.x, q.y))).collect();
                        painter.add(egui::Shape::line(sp, EStroke::new(1.5, accent)));
                    }
                }
            }
            FixTool::Redraw => {
                painter.add(egui::Shape::line(path, EStroke::new(2.0, accent)));
            }
            FixTool::Dust | FixTool::SelectDust => {
                if (g.cur - g.start).length() > 4.0 * g.pix {
                    let r = Rect::from_two_pos(to_screen(g.start), to_screen(g.cur));
                    painter.rect_stroke(r, 0.0, EStroke::new(1.0, accent), egui::StrokeKind::Middle);
                }
            }
            _ => {
                // rastro do pincel e os traços como vão ficar
                if path.len() >= 2 {
                    painter.add(egui::Shape::line(path, EStroke::new((size * scale).max(1.0), accent.gamma_multiply(0.18))));
                }
                let k = cel_scale(&g.inv);
                let to_dyn = |p: Vec2| to(p);
                for s in &g.preview {
                    painter.add(stroke_mesh(s, &to_dyn, scale / k, accent.gamma_multiply(0.75)));
                    if g.tool == FixTool::Simplify {
                        for q in &s.points {
                            painter.circle_filled(to(Vec2::new(q.x, q.y)), 2.2, Color32::WHITE);
                        }
                    }
                }
            }
        }
    }
    // contorno do pincel no cursor
    if tool.has_size() {
        if let Some(p) = env.ed.cursor {
            let r = (size * 0.5 * scale).max(2.0);
            painter.circle_stroke(p, r + 0.8, EStroke::new(1.0, Color32::from_black_alpha(160)));
            painter.circle_stroke(p, r, EStroke::new(1.0, Color32::WHITE));
        }
    }
}

// ------------------------------------------------------------------------------------------------ opções

/// Número com barra (painel) ou campo (barra do canvas).
#[allow(clippy::too_many_arguments)]
fn num(env: &mut Env, ui: &mut Ui, compact: bool, key: &'static str, v: &mut f32, lo: f32, hi: f32, log: bool, decimals: usize, suffix: &str) {
    if compact {
        let t = env.t;
        let mut x = *v;
        let mut rect = None;
        super::panels::properties::row(ui, &t, true, key, |ui| {
            rect = Some(ui.add(egui::DragValue::new(&mut x).range(lo..=hi).speed(((hi - lo) / 300.0).max(0.01)).max_decimals(decimals).suffix(suffix)).rect);
        });
        if let Some(r) = rect {
            env.note(format!("fix:{key}"), r);
        }
        *v = x;
        return;
    }
    let mut dummy = aiai_paint::brush::BrushPreset::round("", 1.0, 1.0);
    let t = env.t;
    let mut e = super::panels::tool_detail::Edit { p: &mut dummy, o: None, t, icons: env.icons, notes: env.notes, compact: true, salt: "fix" };
    super::panels::tool_detail::plain_bar(ui, &mut e, tr(key), v, lo, hi, log, decimals, suffix);
}

fn check(env: &mut Env, ui: &mut Ui, key: &'static str, v: &mut bool) {
    let r = ui.checkbox(v, tr(key));
    env.note(format!("fix:{key}"), r.rect);
    r.on_hover_text(tr(&format!("{key}.tip")));
}

/// Opções da sub-ferramenta em uso: no painel Propriedades da ferramenta (`compact` = falso) e na barra do canvas.
pub(crate) fn options(env: &mut Env, ui: &mut Ui, compact: bool) {
    let Some(tool) = env.ed.opts.fix.tool else { return };
    let mut f = env.ed.opts.fix.clone();
    if tool.has_size() {
        num(env, ui, compact, "fix.opt.size", &mut f.size, 1.0, 500.0, true, 0, " px");
    }
    match tool {
        FixTool::Pinch => {
            num(env, ui, compact, "fix.opt.range", &mut f.pinch_range, 4.0, 2000.0, true, 0, " px");
            let mut lv = f.pinch_level * 100.0;
            num(env, ui, compact, "fix.opt.level", &mut lv, 0.0, 100.0, false, 0, "");
            f.pinch_level = (lv / 100.0).clamp(0.0, 1.0);
            check(env, ui, "fix.opt.fix_ends", &mut f.pinch_fix_ends);
        }
        FixTool::Simplify => num(env, ui, compact, "fix.opt.strength", &mut f.simplify_strength, 0.0, 100.0, false, 0, ""),
        FixTool::Connect => {
            num(env, ui, compact, "fix.opt.distance", &mut f.connect_distance, 1.0, 400.0, true, 0, " px");
            check(env, ui, "fix.opt.smooth", &mut f.connect_smooth);
        }
        FixTool::Width => {
            let modes = [(lf::WidthMode::Widen, "fix.mode.widen"), (lf::WidthMode::Narrow, "fix.mode.narrow"), (lf::WidthMode::Scale, "fix.mode.scale"), (lf::WidthMode::Fixed, "fix.mode.fixed")];
            let t = env.t;
            let mut rect = None;
            super::panels::properties::row(ui, &t, compact, "fix.opt.mode", |ui| {
                let cur = modes.iter().find(|m| m.0 == f.width_mode).map(|m| m.1).unwrap_or("fix.mode.widen");
                rect = Some(egui::ComboBox::from_id_salt(("fix_width_mode", compact)).selected_text(tr(cur)).show_ui(ui, |ui| {
                    for (m, k) in modes {
                        ui.selectable_value(&mut f.width_mode, m, tr(k));
                    }
                }).response.rect);
            });
            if let Some(r) = rect {
                env.note("fix:fix.opt.mode", r);
            }
            match f.width_mode {
                lf::WidthMode::Widen | lf::WidthMode::Narrow => num(env, ui, compact, "fix.opt.amount", &mut f.width_px, 0.1, 50.0, true, 1, " px"),
                lf::WidthMode::Scale => num(env, ui, compact, "fix.opt.factor", &mut f.width_scale, 0.1, 5.0, false, 2, "×"),
                lf::WidthMode::Fixed => num(env, ui, compact, "fix.opt.target", &mut f.width_fixed, 0.1, 100.0, true, 1, " px"),
            }
            check(env, ui, "fix.opt.whole", &mut f.width_whole);
        }
        FixTool::Redraw => num(env, ui, compact, "fix.opt.redraw_tol", &mut f.redraw_tol, 1.0, 100.0, true, 0, " px"),
        FixTool::RedrawWidth => {}
        FixTool::Dust | FixTool::SelectDust => {
            num(env, ui, compact, "fix.opt.dust_size", &mut f.dust_size, 1.0, 200.0, true, 0, " px");
            let modes = [(lf::DustMode::Opaque, "fix.dust.opaque"), (lf::DustMode::Holes, "fix.dust.holes"), (lf::DustMode::Both, "fix.dust.both")];
            let t = env.t;
            let mut rect = None;
            super::panels::properties::row(ui, &t, compact, "fix.opt.dust_mode", |ui| {
                let cur = modes.iter().find(|m| m.0 == f.dust_mode).map(|m| m.1).unwrap_or("fix.dust.opaque");
                rect = Some(egui::ComboBox::from_id_salt(("fix_dust_mode", compact)).selected_text(tr(cur)).show_ui(ui, |ui| {
                    for (m, k) in modes {
                        ui.selectable_value(&mut f.dust_mode, m, tr(k));
                    }
                }).response.rect);
            });
            if let Some(r) = rect {
                env.note("fix:fix.opt.dust_mode", r);
            }
        }
    }
    env.ed.opts.fix = f;
    if tool.dust() {
        // aplicar sem clicar no canvas: a camada inteira (ou a seleção)
        let b = ui.button(tr(if tool == FixTool::Dust { "fix.dust.apply" } else { "fix.select_dust.apply" }));
        env.note("fix:apply", b.rect);
        if b.clicked() {
            apply_dust_now(env, tool);
        }
    }
    if !compact {
        ui.label(egui::RichText::new(tr(tool.tip_key())).size(11.5).color(env.t.text_dim));
    }
}

/// O botão Aplicar do Remover/Selecionar poeira: como um clique no canvas.
fn apply_dust_now(env: &mut Env, tool: FixTool) {
    let Some(c) = ctx(env) else {
        env.ed.notify(tr("fix.msg.need_layer"));
        return;
    };
    let g = FixGesture { tool, cut: c.cut, layer: c.layer, cel: c.cel, fwd: c.fwd, inv: c.inv, path: Vec::new(), start: Vec2::ZERO, cur: Vec2::ZERO, grabbed: None, preview: Vec::new(), removed: Vec::new(), dirty: false, t0: Instant::now(), pix: 1.0 };
    dust_release(env, &g);
}

// ------------------------------------------------------------------------------------------------ miniatura

/// Miniatura da sub-ferramenta na lista (curva clara; em destaque o que a ferramenta faz).
pub(crate) fn paint_thumb(p: &egui::Painter, r: Rect, f: FixTool, t: &crate::theme::Tokens) {
    let at = |fx: f32, fy: f32| Pos2::new(r.left() + r.width() * fx, r.top() + r.height() * fy);
    let ink = EStroke::new(1.6, t.text);
    let acc = EStroke::new(1.6, t.accent);
    let wave = |y0: f32, amp: f32, x0: f32, x1: f32| -> Vec<Pos2> { (0..=24).map(|k| { let u = k as f32 / 24.0; at(x0 + (x1 - x0) * u, y0 + amp * (u * std::f32::consts::TAU).sin()) }).collect() };
    match f {
        FixTool::Pinch => {
            // linha puxada para cima no meio, seta do puxão
            let pts: Vec<Pos2> = (0..=24).map(|k| { let u = k as f32 / 24.0; at(0.08 + 0.84 * u, 0.72 - 0.42 * (-(u - 0.5).powi(2) / 0.02).exp()) }).collect();
            p.add(egui::Shape::line(pts, ink));
            p.arrow(at(0.5, 0.5), Vec2::new(0.0, -r.height() * 0.32), acc);
        }
        FixTool::Simplify => {
            let pts = wave(0.5, 0.22, 0.08, 0.92);
            p.add(egui::Shape::line(pts.clone(), ink));
            for (k, q) in pts.iter().enumerate().step_by(6) {
                p.circle(*q, 2.3, if k % 12 == 0 { t.accent } else { Color32::TRANSPARENT }, EStroke::new(1.0, t.accent));
            }
        }
        FixTool::Connect => {
            p.add(egui::Shape::line(wave(0.5, 0.18, 0.06, 0.44), ink));
            p.add(egui::Shape::line(wave(0.5, 0.18, 0.56, 0.94), ink));
            p.circle_stroke(at(0.5, 0.5), r.height() * 0.24, acc);
        }
        FixTool::Width => {
            for k in 0..=24 {
                let u = k as f32 / 24.0;
                let w = 0.8 + 3.4 * (-(u - 0.5).powi(2) / 0.03).exp();
                p.circle_filled(at(0.08 + 0.84 * u, 0.5), w, if (u - 0.5).abs() < 0.18 { t.accent } else { t.text });
            }
        }
        FixTool::Redraw => {
            p.add(egui::Shape::line(wave(0.6, 0.0, 0.06, 0.94), EStroke::new(1.6, t.text_dim)));
            let pts: Vec<Pos2> = (0..=16).map(|k| { let u = k as f32 / 16.0; at(0.3 + 0.4 * u, 0.6 - 0.4 * (u * std::f32::consts::PI).sin()) }).collect();
            p.add(egui::Shape::line(pts, acc));
        }
        FixTool::RedrawWidth => {
            for k in 0..=24 {
                let u = k as f32 / 24.0;
                p.circle_filled(at(0.08 + 0.84 * u, 0.5), 0.8 + 2.6 * (u * std::f32::consts::PI).sin(), t.text);
            }
            p.add(egui::Shape::line(wave(0.5, 0.0, 0.3, 0.7), EStroke::new(1.0, t.accent)));
        }
        FixTool::Dust | FixTool::SelectDust => {
            p.add(egui::Shape::line(wave(0.45, 0.15, 0.06, 0.94), ink));
            for (x, y) in [(0.2, 0.8), (0.35, 0.18), (0.62, 0.82), (0.8, 0.2), (0.5, 0.86)] {
                p.circle_filled(at(x, y), 1.4, t.text);
            }
            if f == FixTool::SelectDust {
                p.rect_stroke(Rect::from_center_size(at(0.62, 0.82), egui::vec2(7.0, 7.0)), 0.0, EStroke::new(1.0, t.accent), egui::StrokeKind::Middle);
                p.rect_stroke(Rect::from_center_size(at(0.2, 0.8), egui::vec2(7.0, 7.0)), 0.0, EStroke::new(1.0, t.accent), egui::StrokeKind::Middle);
            } else {
                p.line_segment([at(0.55, 0.92), at(0.75, 0.7)], acc);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_exist_in_both_languages() {
        use crate::i18n::{Lang, tr_in};
        let src = include_str!("line_fix.rs");
        // toda chave "fix.*" citada aqui (e as dicas das caixas de marcar) existe nas duas línguas
        let mut keys: Vec<String> = Vec::new();
        let quote = char::from(34u8);
        let open = format!("{quote}fix.");
        for (i, _) in src.match_indices(open.as_str()) {
            let rest = &src[i + 1..];
            let key = &rest[..rest.find(quote).unwrap_or(0)];
            if key.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_') && !key.ends_with('.') {
                keys.push(key.to_owned());
            }
        }
        for k in ["fix.opt.fix_ends", "fix.opt.smooth", "fix.opt.whole"] {
            keys.push(format!("{k}.tip"));
        }
        for f in FixTool::LINE.iter().chain(&FixTool::DUST) {
            keys.push(f.label_key().into());
            keys.push(f.tip_key().into());
        }
        keys.push("err.need_vector".into());
        assert!(keys.len() > 30, "{keys:?}");
        for k in &keys {
            for l in [Lang::Pt, Lang::En] {
                assert_ne!(tr_in(l, k), k.as_str(), "{k} sem texto em {l:?}");
            }
        }
    }

    #[test]
    fn brush_tools_have_size_and_keys_are_unique() {
        let all: Vec<FixTool> = FixTool::LINE.iter().chain(&FixTool::DUST).copied().collect();
        let keys: std::collections::HashSet<&str> = all.iter().map(|f| f.key()).collect();
        assert_eq!(keys.len(), all.len());
        assert!(FixTool::Simplify.has_size() && FixTool::Width.has_size() && !FixTool::Pinch.has_size() && !FixTool::Dust.has_size());
    }
}
