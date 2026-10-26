//! Ferramentas interativas do canvas: o que cada gesto faz no documento. Cada gesto vira UM comando (ou um lote) pelo mesmo
//! caminho do agente: `InsertStrokes` (caneta vetorial), `paint_command` (pincel raster e borracha raster), `RemoveStrokes`
//! (borracha vetorial), `fill_command` (balde), `transform_command` (mover). Assim tudo entra no log, desfaz com Ctrl+Z e
//! o render que o usuário vê é o mesmo do `RENDER` da bridge.

use super::commands::{self as cmds, NodeId};
use super::editor::*;
use super::ui_kit::Env;
use crate::i18n::tr;
use crate::workspace::ToolId;
use aiai_core::command::IndexedStroke;
use aiai_core::*;
use aiai_paint::brush::{BrushMode, BrushPreset};
use aiai_tools::{fill as tfill, paint as tpaint, transform as ttransform};
use egui::Vec2;
use std::sync::OnceLock;
use std::time::Instant;

/// Presets de pincel do programa (a mesma lista de `BRUSH_PRESETS`).
pub(crate) fn presets() -> &'static [BrushPreset] {
    static P: OnceLock<Vec<BrushPreset>> = OnceLock::new();
    P.get_or_init(BrushPreset::defaults)
}

/// Preset da borracha raster pelos controles da barra (como a borracha do Photoshop): tamanho, dureza 0..1,
/// opacidade, fluxo, pressão → tamanho/opacidade e a ponta (Pincel macio, Lápis duro sem antisserrilhado, Bloco
/// quadrado). Com `brush` (usar o pincel atual), a ponta, a textura e a dinâmica do pincel apagam no lugar da redonda.
pub(crate) fn eraser_preset(o: &ToolOptions, brush: Option<BrushPreset>) -> BrushPreset {
    use aiai_paint::brush::{BrushTip, TipShape};
    let from_brush = brush.is_some();
    let mut p = brush.unwrap_or_else(BrushPreset::eraser);
    p.mode = BrushMode::Erase;
    p.size = o.eraser_size.max(0.5);
    p.opacity = o.eraser_opacity.clamp(0.01, 1.0);
    p.flow = if from_brush { p.flow * o.eraser_flow } else { o.eraser_flow }.clamp(0.01, 1.0);
    // pressão: o mínimo do pincel (ou o padrão) quando ligada; 1 = a pressão não muda nada
    let own = |v: f32, dflt: f32| if from_brush && v < 1.0 { v } else { dflt };
    p.size_pressure_min = if o.eraser_pressure_size { own(p.size_pressure_min, 0.2) } else { 1.0 };
    p.opacity_pressure_min = if o.eraser_pressure_opacity { own(p.opacity_pressure_min, 0.0) } else { 1.0 };
    // apagar não tem cor: nada de duas cores, mistura, borda molhada nem variação de cor
    p.two_tone = None;
    p.mix = 0.0;
    p.smudge_carry = 0.0;
    p.wet_edges = 0.0;
    (p.hue_jitter, p.saturation_jitter, p.value_jitter) = (0.0, 0.0, 0.0);
    match o.eraser_tip {
        EraserTip::Brush => {
            if !from_brush {
                p.tip = BrushTip::Round { hardness: o.eraser_hardness.clamp(0.0, 1.0), roundness: 1.0, angle: 0.0 };
                p.aliased = false;
            }
        }
        EraserTip::Pencil => {
            if !from_brush {
                p.tip = BrushTip::Round { hardness: 1.0, roundness: 1.0, angle: 0.0 };
            }
            p.aliased = true;
        }
        EraserTip::Block => {
            // o quadrado da forma cobre 90% do diâmetro: o lado do bloco fica igual ao tamanho escolhido
            p.tip = BrushTip::Shape { shape: TipShape::Square, aspect: 1.0 };
            p.size = o.eraser_size.max(0.5) / 0.9;
            p.aliased = true;
            p.size_pressure_min = 1.0;
            (p.scatter, p.size_jitter, p.angle_jitter, p.follow_direction, p.count, p.dual, p.texture) = (0.0, 0.0, 0.0, false, 1, None, None);
            p.spacing = p.spacing.min(0.1);
        }
    }
    p
}

/// Camada onde o gesto vai agir, já validada (tipo, visível, destravada).
pub(crate) struct Target {
    pub cut: NodeId,
    pub layer: NodeId,
    pub frame: u32,
    pub kind: LayerType,
}

pub(crate) fn target(env: &mut Env, allowed: &[LayerType], wrong_key: &str) -> Option<Target> {
    let cut = env.ed.active_cut(env.core)?;
    let Some(layer) = env.ed.active_layer(env.core) else {
        env.ed.notify(tr("err.no_layer"));
        return None;
    };
    let l = cmds::layer_data(&env.core.project, &layer)?.clone();
    if !allowed.contains(&l.kind) {
        env.ed.notify(tr(wrong_key));
        return None;
    }
    if l.locked {
        env.ed.notify(tr("err.layer_locked"));
        return None;
    }
    if !l.visible {
        env.ed.notify(tr("err.layer_hidden"));
        return None;
    }
    Some(Target { cut, layer, frame: env.ed.frame, kind: l.kind })
}

// ------------------------------------------------------------------------------------------------ traço

impl StrokeGesture {
    pub(crate) fn push(&mut self, p: Vec2, pressure: f32) {
        // régua com encaixe: o ponto vai para a reta escolhida (em px do canvas, antes do espaço do cel)
        let p = match self.snap.as_mut() {
            Some(s) => s.filter(p),
            None => p,
        };
        let p = match &self.inv {
            Some(a) => {
                let (x, y) = a.apply(p.x, p.y);
                Vec2::new(x, y)
            }
            None => p,
        };
        self.path.push(p);
        let t = self.t0.elapsed().as_secs_f32();
        let keep = if self.tool == ToolId::Eraser { Some(p) } else { self.stab.feed_at(p, t) };
        if let Some(q) = keep {
            if self.samples.last().is_none_or(|l| (l.p - q).length() * self.screen_scale >= 0.4) {
                self.samples.push(Sample { p: q, pressure: pressure.clamp(0.0, 1.0), t });
            }
        }
    }
}

/// Caneta numa camada que não é vetorial, ou pincel numa que não é raster: em vez de recusar o traço (o aviso na barra
/// de status passava despercebido e parecia que o programa não desenhava), cria a camada certa logo acima da ativa,
/// seleciona e avisa. Camada travada ou oculta continua sendo recusada (é escolha do usuário).
fn ensure_layer_for(env: &mut Env, tool: ToolId) {
    let want = match tool {
        ToolId::Pen => LayerType::Vector,
        ToolId::Brush => LayerType::Raster,
        _ => return,
    };
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let active = env.ed.active_layer(env.core);
    if active.as_ref().and_then(|l| cmds::layer_data(&env.core.project, l)).is_some_and(|l| l.kind == want) {
        return;
    }
    match cmds::add_layer(env.core, &cut, want, active.as_ref()) {
        Ok(id) => {
            let name = env.core.project.node(&id).map(|n| n.name.clone()).unwrap_or_default();
            env.ed.layer = Some(id);
            env.ed.cel_number = None;
            let key = if want == LayerType::Vector { "msg.auto_vector_layer" } else { "msg.auto_raster_layer" };
            env.ed.notify(format!("{} \u{201c}{name}\u{201d}", tr(key)));
        }
        Err(e) => env.ed.notify(e),
    }
}

pub(crate) fn begin_stroke(env: &mut Env, tool: ToolId, p: Vec2, pressure: f32, scale: f32) -> Gesture {
    ensure_layer_for(env, tool);
    let (allowed, wrong): (&[LayerType], &str) = match tool {
        ToolId::Pen => (&[LayerType::Vector], "err.need_vector"),
        ToolId::Brush => (&[LayerType::Raster], "err.need_raster"),
        _ => (&[LayerType::Vector, LayerType::Raster, LayerType::Fill], "err.eraser_layer"),
    };
    let Some(t) = target(env, allowed, wrong) else { return Gesture::None };
    let o = &env.ed.opts;
    let mode = if tool == ToolId::Eraser { StabMode::Off } else { o.stab_mode };
    // pincel com estabilização própria (canetas S25): a dele, no lugar da do painel
    let stab = match (tool == ToolId::Brush).then(|| brush_preset(env).stabilization).flatten() {
        Some(cfg) => Stabilizer::pen(cfg),
        None => Stabilizer::new(mode, o.stab_window as usize, o.stab_radius / scale.max(1e-3)),
    };
    let snap = super::rulers::stroke_snap(env, tool, p, scale);
    super::pen::clip_notice(env, &t.layer);
    // camada que se move neste quadro: o traço é gravado no espaço do cel para aparecer embaixo do cursor
    let fwd = cmds::layer_placement(&env.core.project, &t.cut, &t.layer, t.frame);
    let inv = fwd.and_then(|a| a.inverse());
    let fwd = inv.and(fwd);
    let scale = scale * fwd.map(|a| a.0[0].hypot(a.0[3])).unwrap_or(1.0);
    let mut g = StrokeGesture { tool, layer: t.layer, samples: Vec::new(), stab, t0: Instant::now(), hits: Vec::new(), path: Vec::new(), screen_scale: scale, fwd, inv, snap };
    g.push(p, pressure);
    Gesture::Stroke(Box::new(g))
}

/// Traços da camada vetorial no quadro atual que o caminho da borracha toca.
pub(crate) fn eraser_hits(core: &CoreSession, cut: &NodeId, layer: &NodeId, frame: u32, path: &[Vec2], radius: f32) -> Vec<NodeId> {
    let Some(cel) = cmds::exposed_cel(&core.project, cut, layer, frame) else { return Vec::new() };
    let Some(cd) = cmds::cel_data(&core.project, &cel) else { return Vec::new() };
    cd.strokes.iter().filter(|s| stroke_hit(s, path, radius)).map(|s| s.id.clone()).collect()
}

pub(crate) fn update_eraser_hits(env: &mut Env, g: &mut StrokeGesture) {
    if g.tool != ToolId::Eraser {
        return;
    }
    let Some(cut) = env.ed.cut.clone() else { return };
    let is_vector = cmds::layer_data(&env.core.project, &g.layer).is_some_and(|l| l.kind == LayerType::Vector);
    if is_vector {
        g.hits = eraser_hits(env.core, &cut, &g.layer, env.ed.frame, &g.path, env.ed.opts.eraser_size / 2.0);
    }
}

pub(crate) fn finish_stroke(env: &mut Env, mut g: Box<StrokeGesture>, released_at: Instant) {
    let tail = g.stab.finish();
    let pressure = g.samples.last().map(|s| s.pressure).unwrap_or(1.0);
    for q in tail {
        if g.samples.last().is_none_or(|l| (l.p - q).length() * g.screen_scale >= 0.4) {
            let t = g.t0.elapsed().as_secs_f32();
            g.samples.push(Sample { p: q, pressure, t });
        }
    }
    let Some(t) = target_for_commit(env, &g) else { return };
    match g.tool {
        ToolId::Pen => commit_pen(env, &t, &g, released_at),
        ToolId::Brush => commit_brush(env, &t, &g, released_at, false),
        ToolId::Eraser => match t.kind {
            LayerType::Vector => commit_erase_vector(env, &t, &g, released_at),
            LayerType::Raster => commit_brush(env, &t, &g, released_at, true),
            _ => commit_erase_fill(env, &t, &g, released_at),
        },
        _ => {}
    }
}

fn target_for_commit(env: &mut Env, g: &StrokeGesture) -> Option<Target> {
    let cut = env.ed.active_cut(env.core)?;
    let kind = cmds::layer_data(&env.core.project, &g.layer)?.kind;
    Some(Target { cut, layer: g.layer.clone(), frame: env.ed.frame, kind })
}

fn commit_pen(env: &mut Env, t: &Target, g: &StrokeGesture, released_at: Instant) {
    let mut points: Vec<StrokePoint> = g.samples.iter().map(|s| StrokePoint { x: s.p.x, y: s.p.y, pressure: s.pressure, tilt: None, t: Some(s.t), corner: false, width: 1.0, id: 0, handle: None }).collect();
    if points.is_empty() {
        return;
    }
    if points.len() == 1 {
        let q = points[0];
        points.push(StrokePoint { x: q.x + 0.01, ..q });
    }
    // seleção de área (auditoria 2.15): a caneta só desenha dentro dela; o traço sai em pedaços nas bordas
    let pieces: Vec<Vec<StrokePoint>> = match paint_clip_mask(env) {
        Some((cov, w, h)) => {
            let inside = |q: &StrokePoint| {
                let c = g.to_canvas(Vec2::new(q.x, q.y));
                let (x, y) = (c.x.floor() as i64, c.y.floor() as i64);
                x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h && cov[(y as u32 * w + x as u32) as usize] >= 0.5
            };
            let mut out = Vec::new();
            let mut cur = Vec::new();
            for q in &points {
                if inside(q) {
                    cur.push(*q);
                } else if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            if !cur.is_empty() {
                out.push(cur);
            }
            out.into_iter().filter(|r: &Vec<StrokePoint>| r.len() >= 2).collect()
        }
        None => vec![points.clone()],
    };
    if pieces.is_empty() {
        env.ed.notify(tr("pen.outside_selection"));
        return;
    }
    let clipped = pieces.len() != 1 || pieces[0].len() != points.len();
    let mut pieces = pieces.into_iter();
    let mut points = pieces.next().expect("há pedaço");
    let rest: Vec<Vec<StrokePoint>> = pieces.collect();
    let o = &env.ed.opts;
    // volta ao ponto de partida: fecha o traço (sem as pontas afinadas abrindo um vão que o balde não vê)
    let closed = !clipped && o.pen_autoclose && should_close(&points, o.pen_width, g.screen_scale);
    if closed {
        let (a, b) = (points[0], points[points.len() - 1]);
        if (a.x - b.x).hypot(a.y - b.y) < 1.0 {
            points.pop();
        }
    }
    let stroke = Stroke {
        id: Id(String::new()),
        points,
        geometry: StrokeGeometry::Freehand,
        tension: 0.0,
        tip: o.pen_tip,
        width: o.pen_width.max(0.1),
        profile: WidthProfile::default(),
        color: env.ed.color.clone(),
        opacity: 1.0,
        closed,
        aliased: o.pen_aliased,
        color_trace: false,
    };
    // régua de simetria: as cópias espelhadas entram no mesmo passo de undo
    let pts: Vec<Vec2> = stroke.points.iter().map(|q| Vec2::new(q.x, q.y)).collect();
    let copies: Vec<Stroke> = super::rulers::mirrored(g, &pts).into_iter().map(|c| {
        let mut s = stroke.clone();
        s.points.iter_mut().zip(c).for_each(|(q, p)| (q.x, q.y) = (p.x, p.y));
        s
    }).collect();
    let extra: Vec<Stroke> = rest.into_iter().map(|pts| Stroke { points: pts, ..stroke.clone() }).collect();
    let (cut, layer, frame, pref) = (t.cut.clone(), t.layer.clone(), t.frame, env.ed.cel_number);
    let width = stroke.width;
    // o fantasma é a mesma geometria do traço gravado (afinamento, fechamento), não a polilinha do arrasto
    let (ghost_pts, mut ghost_radii) = sampled(&stroke);
    let k = g.fwd.map(|a| a.0[0].hypot(a.0[3])).unwrap_or(1.0);
    ghost_radii.iter_mut().for_each(|r| *r *= k);
    let ghost_pts: Vec<Vec2> = ghost_pts.into_iter().map(|p| g.to_canvas(p)).collect();
    let planned = cmds::plan_with_prep(env.core, &cut, &layer, frame, pref, vec![], |p, _| {
        let cel = cmds::exposed_cel(p, &cut, &layer, frame).ok_or_else(|| tr("err.no_cel").to_owned())?;
        let index = cmds::cel_data(p, &cel).map(|c| c.strokes.len()).unwrap_or(0);
        let items = std::iter::once(stroke).chain(extra).chain(copies).enumerate().map(|(i, stroke)| IndexedStroke { index: index + i, stroke }).collect();
        Ok(Command::InsertStrokes { cel, items })
    });
    let color = ghost_color(env, 1.0);
    // camada com Recortar: sem fantasma (ele mostraria o traço inteiro até o render recortado chegar)
    let ghost = (!clipped && super::pen::clip_base(&env.core.project, &layer).is_none()).then_some((ghost_pts, ghost_radii, width, color));
    commit(env, planned, released_at, "pen", ghost);
}

/// Pontos (canvas) da forma da caneta de `a` até `b`; com `shift`, linha a cada 15°, quadrado, círculo.
pub(crate) fn shape_points(shape: PenShape, a: Vec2, b: Vec2, shift: bool) -> (Vec<Vec2>, StrokeGeometry, bool) {
    let mut b = b;
    match shape {
        PenShape::Line => {
            if shift {
                let d = b - a;
                let ang = (d.y.atan2(d.x) / (std::f32::consts::PI / 12.0)).round() * (std::f32::consts::PI / 12.0);
                b = a + Vec2::new(ang.cos(), ang.sin()) * d.length();
            }
            (vec![a, b], StrokeGeometry::Polyline, false)
        }
        PenShape::Rect | PenShape::Ellipse => {
            if shift {
                let d = b - a;
                let m = d.x.abs().max(d.y.abs());
                b = a + Vec2::new(m * d.x.signum(), m * d.y.signum());
            }
            if shape == PenShape::Rect {
                (vec![a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)], StrokeGeometry::Polyline, true)
            } else {
                let (c, r) = ((a + b) / 2.0, (b - a).abs() / 2.0);
                ((0..16).map(|i| {
                    let t = i as f32 / 16.0 * std::f32::consts::TAU;
                    c + Vec2::new(r.x * t.cos(), r.y * t.sin())
                }).collect(), StrokeGeometry::Spline, true)
            }
        }
        _ => (vec![a, b], StrokeGeometry::Spline, false),
    }
}

/// Grava um traço de forma (pontos no canvas) na camada vetorial ativa com a ponta/largura/cor da caneta; os pontos
/// vão para o espaço do cel (camada com movimento) como os da mão livre.
/// (S25: o app grava as formas pela Figura, `figure::commit`; esta fica para os testes da caneta com régua.)
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn commit_vector_shape(env: &mut Env, pts: Vec<Vec2>, geometry: StrokeGeometry, closed: bool) {
    let released_at = Instant::now();
    if pts.len() < 2 || pts.windows(2).all(|w| (w[1] - w[0]).length() < 0.5) {
        return;
    }
    let Some(t) = target(env, &[LayerType::Vector], "err.need_vector") else { return };
    let inv = cmds::layer_placement(&env.core.project, &t.cut, &t.layer, t.frame).and_then(|a| a.inverse());
    let local = |p: Vec2| match &inv {
        Some(a) => {
            let (x, y) = a.apply(p.x, p.y);
            Vec2::new(x, y)
        }
        None => p,
    };
    let o = &env.ed.opts;
    let stroke = Stroke {
        id: Id(String::new()),
        points: pts.iter().map(|p| {
            let q = local(*p);
            StrokePoint { x: q.x, y: q.y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }
        }).collect(),
        geometry,
        tension: 0.0,
        tip: o.pen_tip,
        width: o.pen_width.max(0.1),
        profile: WidthProfile::default(),
        color: env.ed.color.clone(),
        opacity: 1.0,
        closed,
        aliased: o.pen_aliased,
        color_trace: false,
    };
    let (cut, layer, frame, pref) = (t.cut.clone(), t.layer.clone(), t.frame, env.ed.cel_number);
    let width = stroke.width;
    let ghost: Vec<Vec2> = if closed { pts.iter().copied().chain(pts.first().copied()).collect() } else { pts.clone() };
    let planned = cmds::plan_with_prep(env.core, &cut, &layer, frame, pref, vec![], |p, _| {
        let cel = cmds::exposed_cel(p, &cut, &layer, frame).ok_or_else(|| tr("err.no_cel").to_owned())?;
        let index = cmds::cel_data(p, &cel).map(|c| c.strokes.len()).unwrap_or(0);
        Ok(Command::InsertStrokes { cel, items: vec![IndexedStroke { index, stroke }] })
    });
    let color = ghost_color(env, 1.0);
    commit(env, planned, released_at, "pen_shape", Some((ghost, Vec::new(), width, color)));
}

/// Amostras (posição, raio) de um traço vetorial como o motor desenha.
pub(crate) fn sampled(stroke: &Stroke) -> (Vec<Vec2>, Vec<f32>) {
    aiai_paint::sample_stroke(stroke).iter().map(|q| (Vec2::new(q.x, q.y), q.r)).unzip()
}

/// O traço que a caneta gravaria com estes pontos agora (para a prévia durante o arrasto).
pub(crate) fn pen_preview(env: &Env, g: &StrokeGesture) -> Option<Stroke> {
    let mut points: Vec<StrokePoint> = g.samples.iter().map(|s| StrokePoint { x: s.p.x, y: s.p.y, pressure: s.pressure, tilt: None, t: Some(s.t), corner: false, width: 1.0, id: 0, handle: None }).collect();
    if points.is_empty() {
        return None;
    }
    if points.len() == 1 {
        let q = points[0];
        points.push(StrokePoint { x: q.x + 0.01, ..q });
    }
    let o = &env.ed.opts;
    Some(Stroke {
        id: Id(String::new()),
        points,
        geometry: StrokeGeometry::Freehand,
        tension: 0.0,
        tip: o.pen_tip,
        width: o.pen_width.max(0.1),
        profile: WidthProfile::default(),
        color: env.ed.color.clone(),
        opacity: 1.0,
        closed: false,
        aliased: o.pen_aliased,
        color_trace: false,
    })
}

/// O traço termina perto de onde começou (a menos de 2 larguras ou 10 pontos da tela) depois de percorrer um caminho bem maior?
pub(crate) fn should_close(points: &[StrokePoint], width: f32, screen_scale: f32) -> bool {
    if points.len() < 8 {
        return false;
    }
    let (a, b) = (points[0], points[points.len() - 1]);
    let gap = (a.x - b.x).hypot(a.y - b.y);
    let len: f32 = points.windows(2).map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y)).sum();
    gap <= (width * 2.0).max(10.0 / screen_scale.max(1e-3)) && len > gap * 6.0 && len > width * 12.0
}

fn ghost_color(env: &Env, alpha: f32) -> egui::Color32 {
    let c = cmds::resolve_color(&env.core.project, &env.ed.color);
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    egui::Color32::from_rgba_unmultiplied(q(c.0[0]), q(c.0[1]), q(c.0[2]), q(alpha))
}

/// Pontos do pincel a partir das amostras (~1,5 px entre eles, sempre com a última), com o tempo de cada um
/// (`[x, y, pressão, t]`: o motor vê a velocidade). O commit, a prévia ao vivo e o calibrador usam esta mesma lista.
fn brush_points(samples: &[Sample]) -> Vec<Vec<f32>> {
    let raw: Vec<[f32; 4]> = samples.iter().map(|s| [s.p.x, s.p.y, s.pressure, s.t]).collect();
    tpaint::brush_points_from_samples(&raw)
}

/// Preset do traço (o do pincel com tamanho/opacidade da barra, ou o da borracha).
pub(crate) fn stroke_preset(env: &Env, erase: bool) -> BrushPreset {
    let o = &env.ed.opts;
    if erase {
        eraser_preset(o, o.eraser_use_brush.then(|| brush_preset(env)))
    } else {
        brush_preset(env)
    }
}

/// O pincel atual com o tamanho e a opacidade da barra.
fn brush_preset(env: &Env) -> BrushPreset {
    let o = &env.ed.opts;
    let mut p = o.brush_custom.clone().or_else(|| env.ed.brushes.get(o.brush_preset).cloned()).or_else(|| presets().first().cloned()).unwrap_or_else(|| BrushPreset::round("Pincel", 24.0, 0.9));
    p.size = o.brush_size.max(0.5);
    p.opacity = o.brush_opacity.clamp(0.0, 1.0);
    // degradê como cor (painel Cores): ao longo do traço
    p.color_ramp = super::panels::gradient_edit::brush_ramp(env);
    // cor transparente (painel Cores, S25): o pincel tira tinta em vez de pôr
    if env.ed.draw_ui.colors.transparent {
        p.mode = aiai_paint::brush::BrushMode::Erase;
    }
    p
}

/// Seleção que limita pincel e borracha raster: uma área de verdade (não "a camada inteira", não traços escolhidos),
/// sem transformação pendente, como cobertura em px do canvas `(cobertura, largura, altura)`.
pub(crate) fn paint_clip_mask(env: &Env) -> Option<(Vec<f32>, u32, u32)> {
    let sel = env.ed.selection.as_ref().filter(|s| s.is_identity())?;
    let (w, h) = cmds::canvas_size(&env.core.project, env.ed.cut.as_ref()?);
    super::selection::area_cov(sel, w, h).map(|c| (c.cov, w, h))
}

/// Semente do traço: a mesma no começo (prévia) e no commit, enquanto nada mais mudou no documento.
fn stroke_seed(env: &Env) -> u64 {
    env.core.change_counter.wrapping_mul(2654435761)
}

/// Liga a prévia ao vivo para o traço que acabou de começar (pincel em raster, borracha em raster ou vetor).
pub(crate) fn start_live(env: &mut Env, ctx: &egui::Context, g: &StrokeGesture, screen_scale: f32) {
    let Some(cut) = env.ed.cut.clone() else { return };
    let Some(l) = cmds::layer_data(&env.core.project, &g.layer).cloned() else { return };
    let mode = match (g.tool, l.kind) {
        (ToolId::Pen, LayerType::Vector) => super::live::Mode::Pen,
        (ToolId::Brush, LayerType::Raster) => super::live::Mode::Brush,
        (ToolId::Eraser, LayerType::Raster) => super::live::Mode::EraseRaster,
        (ToolId::Eraser, LayerType::Vector) => super::live::Mode::EraseVector,
        (ToolId::Eraser, LayerType::Fill) => super::live::Mode::EraseFill,
        _ => return,
    };
    let erase = g.tool == ToolId::Eraser;
    let sp = stroke_preset(env, erase);
    // pincel do usuário com ponta/textura de imagem: as imagens entram no projeto antes de pintar
    env.ed.brushes.ensure_blobs(&mut env.core.blobs, &sp);
    let preset = match aiai_tools::paint::load_tip_mask(&env.core.blobs, &sp) {
        Ok(p) => p,
        Err(_) => return,
    };
    let color = if erase { [0.0, 0.0, 0.0, 1.0] } else { cmds::resolve_color(&env.core.project, &env.ed.color).0 };
    let frame = env.ed.frame;
    let cel = cmds::exposed_cel(&env.core.project, &cut, &g.layer, frame).and_then(|c| cmds::cel_data(&env.core.project, &c).cloned());
    let canvas = cmds::canvas_size(&env.core.project, &cut);
    let spec = super::live::StartSpec {
        mode,
        project: Some(env.core.project.clone()),
        blobs: env.core.blobs.clone(),
        cut,
        layer: g.layer.clone(),
        frame,
        cel,
        canvas,
        preset,
        color,
        seed: stroke_seed(env),
        layer_opacity: l.opacity,
        fwd: g.fwd,
        guides: env.ed.show_guides,
        reference: env.ed.show_reference,
        checker: (super::canvas::checker_colors(env.ed.checker_dark), 8.0 / screen_scale.max(1e-3)),
        erase_radius: env.ed.opts.eraser_size / 2.0,
        erase_mode: eraser_vector_mode(env),
        hide: None,
        clip: match (paint_clip_mask(env), mode) {
            (Some(m), super::live::Mode::Brush | super::live::Mode::EraseRaster) => Some(std::sync::Arc::new(m)),
            _ => None,
        },
    };
    env.ed.live_stroke.start(ctx, spec, g.t0);
}

/// Borracha escolhida e nada em andamento: prepara a base da prévia ao vivo (o quadro abaixo e acima da camada
/// ativa) depois que o render da tela alcançou o documento, para o primeiro movimento já apagar como o commit.
pub(crate) fn erase_prefetch(env: &mut Env) {
    if *env.tool != ToolId::Eraser {
        env.ed.live_stroke.drop_prefetch();
        return;
    }
    if !matches!(env.ed.gesture, Gesture::None) {
        return;
    }
    let (Some(cut), Some(layer)) = (env.ed.cut.clone(), env.ed.layer.clone()) else { return };
    let Some(l) = cmds::layer_data(&env.core.project, &layer) else { return };
    if !matches!(l.kind, LayerType::Raster | LayerType::Vector | LayerType::Fill) || !l.visible || l.locked {
        return;
    }
    // o render da tela primeiro: os dois disputam a mesma CPU
    if env.ed.shown.as_ref().map(|s| s.rendered.key.counter) != Some(env.core.change_counter) {
        return;
    }
    let key = super::live::BaseKey { revision: env.core.project.revision, cut, layer, frame: env.ed.frame, guides: env.ed.show_guides, reference: env.ed.show_reference };
    let (p, b) = (&env.core.project, &env.core.blobs);
    env.ed.live_stroke.prefetch_erase(key, || (p.clone(), b.clone()));
}

/// Manda o traço até aqui para a prévia ao vivo.
pub(crate) fn feed_live(env: &mut Env, g: &StrokeGesture) {
    if !env.ed.live_stroke.active() {
        return;
    }
    let pen = env.ed.live_stroke.mode() == Some(super::live::Mode::Pen);
    let stroke = if pen {
        // a cor resolvida vai na prévia: a do traço é a mesma referência que o commit grava
        pen_preview(env, g)
    } else {
        None
    };
    let (pts, path) = if pen { (Vec::new(), Vec::new()) } else { (brush_points(&g.samples), g.path.iter().map(|p| [p.x, p.y]).collect()) };
    env.ed.live_stroke.feed(pts, path, stroke, g.samples.len() + g.path.len());
}

fn commit_brush(env: &mut Env, t: &Target, g: &StrokeGesture, released_at: Instant, erase: bool) {
    if g.samples.is_empty() {
        return;
    }
    let pts = brush_points(&g.samples);
    // régua de simetria: as cópias espelhadas no mesmo traço (um passo de undo)
    let mut specs = vec![tpaint::PaintStrokeSpec { points: pts.clone(), smooth: true }];
    for c in super::rulers::mirrored(g, &pts.iter().map(|q| Vec2::new(q[0], q[1])).collect::<Vec<_>>()) {
        specs.push(tpaint::PaintStrokeSpec { points: c.iter().zip(&pts).map(|(p, q)| vec![p.x, p.y, q[2]]).collect(), smooth: true });
    }
    let preset = stroke_preset(env, erase);
    env.ed.brushes.ensure_blobs(&mut env.core.blobs, &preset);
    let color = if erase { ColorRef::Direct { rgba: Rgba::BLACK } } else { env.ed.color.clone() };
    let size = preset.size;
    let opacity = preset.opacity;
    let seed = stroke_seed(env);
    let (cut, layer, frame, pref) = (t.cut.clone(), t.layer.clone(), t.frame, env.ed.cel_number);
    let clip = paint_clip_mask(env);
    let ghost_pts: Vec<Vec2> = g.samples.iter().map(|s| g.to_canvas(s.p)).collect();
    // borracha de várias camadas: apaga também nas outras camadas do quadro (visíveis, destravadas, com cel exposto)
    let others: Vec<(NodeId, LayerType)> = if erase && preset.all_layers {
        cmds::layer_rows(&env.core.project, &cut)
            .into_iter()
            .filter(|r| r.id != layer)
            .filter_map(|r| cmds::layer_data(&env.core.project, &r.id).filter(|l| l.visible && !l.locked && matches!(l.kind, LayerType::Raster | LayerType::Vector)).map(|l| (r.id.clone(), l.kind)))
            .filter(|(id, _)| cmds::exposed_cel(&env.core.project, &cut, id, frame).is_some())
            .collect()
    } else {
        Vec::new()
    };
    let vec_mode = eraser_vector_mode(env);
    let radius = env.ed.opts.eraser_size / 2.0;
    let path: Vec<[f32; 2]> = g.path.iter().map(|p| [p.x, p.y]).collect();
    let planned = cmds::plan_with_prep(env.core, &cut, &layer, frame, pref, vec![], |p, blobs| {
        let mut req = tpaint::PaintRequest { cut: cut.clone(), frame, layer: layer.clone(), preset, color, strokes: specs, seed };
        let main = tpaint::paint_command_masked(p, blobs, &req, clip.as_ref()).map_err(|e| e.to_string())?;
        if others.is_empty() {
            return Ok(main);
        }
        let mut all = vec![main];
        for (id, kind) in &others {
            let c = match kind {
                LayerType::Raster => {
                    req.layer = id.clone();
                    tpaint::paint_command_masked(p, blobs, &req, clip.as_ref()).ok()
                }
                _ => aiai_tools::erase::erase_vector_command(p, &aiai_tools::erase::EraseVectorRequest { cut: cut.clone(), frame, layer: id.clone(), path: path.clone(), radius, mode: vec_mode }).ok(),
            };
            all.extend(c);
        }
        Ok(Command::Batch { commands: all, label: Some(tr("brush.erase_all_layers").to_owned()) })
    });
    // com a prévia ao vivo na tela, ela mesma fica até o render chegar (o fantasma em polilinha "mudava" o traço)
    let ghost = if erase || env.ed.live_stroke.overlay.is_some() { None } else { Some((ghost_pts, Vec::new(), size, ghost_color(env, opacity.min(0.9)))) };
    commit(env, planned, released_at, if erase { "erase_raster" } else { "brush" }, ghost);
}

/// O que a borracha tira de um traço vetorial: o do pincel do grupo borracha em uso (Borracha vetorial = até as
/// interseções, áspera = traço inteiro...), senão o modo escolhido na ferramenta.
pub(crate) fn eraser_vector_mode(env: &Env) -> aiai_tools::erase::EraseMode {
    use aiai_paint::brush::VectorErase;
    let o = &env.ed.opts;
    let from_brush = o.eraser_use_brush.then(|| brush_preset(env)).and_then(|p| p.vector_erase);
    match from_brush {
        Some(VectorErase::Segment) => aiai_tools::erase::EraseMode::Segment,
        Some(VectorErase::Intersection) => aiai_tools::erase::EraseMode::Intersection,
        Some(VectorErase::Whole) => aiai_tools::erase::EraseMode::Whole,
        None => o.eraser_mode,
    }
}

fn commit_erase_vector(env: &mut Env, t: &Target, g: &StrokeGesture, released_at: Instant) {
    // trecho / até a interseção / traço inteiro (o caminho já está no espaço do cel)
    let req = aiai_tools::erase::EraseVectorRequest {
        cut: t.cut.clone(),
        frame: t.frame,
        layer: t.layer.clone(),
        path: g.path.iter().map(|p| [p.x, p.y]).collect(),
        radius: env.ed.opts.eraser_size / 2.0,
        mode: eraser_vector_mode(env),
    };
    // a prévia ao vivo usa este mesmo comando; o erro só vira aviso se a borracha passou por algum traço
    let planned = match aiai_tools::erase::erase_vector_command(&env.core.project, &req) {
        Ok(c) => Ok(c),
        Err(_) if eraser_hits(env.core, &t.cut, &t.layer, t.frame, &g.path, env.ed.opts.eraser_size / 2.0).is_empty() => return,
        Err(e) => Err(e.to_string()),
    };
    commit(env, planned, released_at, "erase_vector", None);
}

fn commit_erase_fill(env: &mut Env, t: &Target, g: &StrokeGesture, released_at: Instant) {
    let Some(cel) = cmds::exposed_cel(&env.core.project, &t.cut, &t.layer, t.frame) else { return };
    let Some(cd) = cmds::cel_data(&env.core.project, &cel) else { return };
    let covers = |f: &FillRegion, p: Vec2| -> bool {
        let [bx, by, bw, bh] = f.bounds;
        let (xi, yi) = (p.x.floor() as i32, p.y.floor() as i32);
        if xi < bx || yi < by || xi >= bx + bw || yi >= by + bh {
            return false;
        }
        let Some(h) = &f.mask else { return false };
        let Some(b) = env.core.blobs.get(h) else { return false };
        aiai_paint::Mask::decode(&b).map(|m| m.get((xi - bx) as u32, (yi - by) as u32) > 0.5).unwrap_or(false)
    };
    let fills: Vec<FillRegion> = cd.fills.iter().filter(|f| !g.path.iter().any(|p| covers(f, *p))).cloned().collect();
    if fills.len() == cd.fills.len() {
        return;
    }
    let planned = Ok(Command::ReplaceFills { cel, fills });
    commit(env, planned, released_at, "erase_fill", None);
}

/// Aplica o comando planejado; erro vira aviso. Mede a latência e deixa o "fantasma" do traço no overlay até o render novo.
fn commit(env: &mut Env, planned: Result<Command, String>, released_at: Instant, label: &str, ghost: Option<(Vec<Vec2>, Vec<f32>, f32, egui::Color32)>) {
    let cmd = match planned {
        Ok(c) => c,
        Err(e) => {
            env.ed.notify(e);
            return;
        }
    };
    if env.ed.apply(env.core, cmd) {
        env.ed.begin_latency(released_at, label, env.core);
        if let Some((pts, radii, width, color)) = ghost {
            env.ed.ghosts.push(Ghost { pts, radii, width, color, until_counter: env.core.change_counter, born: Instant::now() });
        }
    }
}

// ------------------------------------------------------------------------------------------------ balde

/// Camada de preenchimento alvo e camadas de linha para o clique do balde. A camada ativa de preenchimento vale por si;
/// com uma camada de linha ativa, vale a primeira de preenchimento que a referencia, senão a mais próxima (acima, depois
/// abaixo). Sem `line_refs`, a linha é a camada vetorial vizinha (primeiro abaixo, depois acima) — e fica gravada.
pub(crate) fn fill_plan(p: &Project, cut: &NodeId, active: &NodeId) -> Option<(NodeId, Vec<NodeId>, Option<NodeId>)> {
    let rows = cmds::layer_rows(p, cut);
    let kind = |id: &NodeId| cmds::layer_data(p, id).map(|l| l.kind);
    let idx = rows.iter().position(|r| &r.id == active)?;
    let fill = if kind(active) == Some(LayerType::Fill) {
        active.clone()
    } else {
        let by_ref = rows.iter().find(|r| kind(&r.id) == Some(LayerType::Fill) && cmds::layer_data(p, &r.id).is_some_and(|l| l.line_refs.contains(active)));
        let near = (1..rows.len()).flat_map(|d| [idx.checked_sub(d), Some(idx + d)]).flatten().filter_map(|i| rows.get(i)).find(|r| kind(&r.id) == Some(LayerType::Fill));
        by_ref.or(near)?.id.clone()
    };
    let l = cmds::layer_data(p, &fill)?;
    let refs: Vec<NodeId> = l.line_refs.iter().filter(|r| p.node(r).is_some()).cloned().collect();
    if !refs.is_empty() {
        return Some((fill, refs, None));
    }
    let fi = rows.iter().position(|r| r.id == fill)?;
    let is_vec = |i: Option<usize>| i.and_then(|i| rows.get(i)).filter(|r| kind(&r.id) == Some(LayerType::Vector)).map(|r| r.id.clone());
    let near_vec = (1..rows.len()).flat_map(|d| [Some(fi + d), fi.checked_sub(d)]).find_map(is_vec);
    near_vec.map(|v| (fill.clone(), vec![v.clone()], Some(v)))
}

pub(crate) fn do_fill(env: &mut Env, p: Vec2, released_at: Instant) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let Some(active) = env.ed.active_layer(env.core) else {
        env.ed.notify(tr("err.no_layer"));
        return;
    };
    let Some((fill_layer, lines, persist)) = fill_plan(&env.core.project, &cut, &active) else {
        env.ed.notify(tr("err.no_fill_layer"));
        return;
    };
    let l = cmds::layer_data(&env.core.project, &fill_layer).cloned().expect("camada existe");
    if l.locked || !l.visible {
        env.ed.notify(tr(if l.locked { "err.layer_locked" } else { "err.layer_hidden" }));
        return;
    }
    let p = match cmds::layer_placement(&env.core.project, &cut, &fill_layer, env.ed.frame).and_then(|a| a.inverse()) {
        Some(a) => {
            let (x, y) = a.apply(p.x, p.y);
            Vec2::new(x, y)
        }
        None => p,
    };
    let (w, h) = cmds::canvas_size(&env.core.project, &cut);
    if p.x < 0.0 || p.y < 0.0 || p.x >= w as f32 || p.y >= h as f32 {
        return;
    }
    let extra = match persist {
        Some(v) => vec![Command::SetProps { id: fill_layer.clone(), name: None, data: serde_json::json!({ "line_refs": [v] }) }],
        None => vec![],
    };
    let (frame, pref) = (env.ed.frame, env.ed.cel_number);
    let o = &env.ed.opts;
    let (gap, expand) = (o.fill_gap, o.fill_expand);
    // frestas: até ~300 px num quadro 1080p (cantos de olho, entre dedos)
    let close = o.fill_close_leftovers.then_some(300u32);
    let color = env.ed.color.clone();
    let grad = super::panels::gradient_edit::fill_gradient(env);
    let planned = cmds::plan_with_prep(env.core, &cut, &fill_layer, frame, pref, extra, |pr, blobs| {
        let req = tfill::FillRequest { cut: cut.clone(), frame, layer: fill_layer.clone(), seeds: vec![tfill::FillSeed { x: p.x, y: p.y, color }], line_layers: lines, gap, expand, line_threshold: 0.35, close_leftovers: close, skip_seeds_on_line: false, only_unpainted: false };
        tfill::fill_command_paint(pr, blobs, &req, grad.as_ref()).map_err(|e| e.to_string())
    });
    commit(env, planned, released_at, "fill", None);
}

/// Conta as regiões fechadas ainda sem tinta na camada de preenchimento ativa (botão "Verificar").
pub(crate) fn run_lint(env: &mut Env) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let Some(active) = env.ed.active_layer(env.core) else { return };
    let Some((fill_layer, lines, persist)) = fill_plan(&env.core.project, &cut, &active) else {
        env.ed.lint_report = Some(tr("err.no_fill_layer").to_owned());
        return;
    };
    let _ = lines;
    // camada de cor ainda sem a linha ligada (nunca passou pelo balde): liga a linha mais perto, como o primeiro clique
    // do balde faria (senão o Verificar só funcionava depois de pintar uma vez)
    if let Some(v) = persist {
        if let Err(e) = cmds::run(env.core, Command::SetProps { id: fill_layer.clone(), name: None, data: serde_json::json!({ "line_refs": [v] }) }) {
            env.ed.lint_report = Some(e);
            return;
        }
    }
    let o = &env.ed.opts;
    let frame = env.ed.frame;
    let msg = match tfill::lint(&env.core.project, &env.core.blobs, &cut, frame, &fill_layer, o.fill_gap, 40) {
        Ok(r) => {
            // as regiões vêm na camada de preenchimento: para o desenho pela posição dela no quadro
            let place = cmds::layer_placement(&env.core.project, &cut, &fill_layer, frame);
            let to = |x: f32, y: f32| match &place {
                Some(a) => {
                    let (cx, cy) = a.apply(x, y);
                    Vec2::new(cx, cy)
                }
                None => Vec2::new(x, y),
            };
            let hole = |g: &tfill::LintRegion, border: bool| {
                // caixa da região: [x, y, largura, altura]
                let [x0, y0, bw, bh] = g.bounds.map(|v| v as f32);
                let (x1, y1) = (x0 + bw, y0 + bh);
                super::editor::LintHole {
                    point: to(g.point[0] as f32 + 0.5, g.point[1] as f32 + 0.5),
                    corners: [to(x0, y0), to(x1, y0), to(x1, y1), to(x0, y1)],
                    area: g.area,
                    border,
                    partial: g.coverage > 0.0,
                }
            };
            let mut holes: Vec<_> = r.unpainted.iter().map(|g| hole(g, false)).collect();
            holes.extend(r.border_unpainted.iter().map(|g| hole(g, true)));
            // ordem de leitura: de cima para baixo, da esquerda para a direita
            holes.sort_by(|a, b| (a.point.y / 64.0).floor().total_cmp(&(b.point.y / 64.0).floor()).then(a.point.x.total_cmp(&b.point.x)));
            let n_border = r.border_unpainted.len();
            env.ed.lint_marks = super::editor::LintMarks { cut: Some(cut.clone()), frame, holes, current: 0 };
            let mut m = tr("fill.lint_result").replace("{n}", &r.unpainted.len().to_string()).replace("{total}", &r.regions_total.to_string());
            if n_border > 0 {
                m.push_str(&tr("fill.lint_border").replace("{n}", &n_border.to_string()));
            }
            m
        }
        Err(e) => {
            env.ed.lint_marks = Default::default();
            e.to_string()
        }
    };
    env.ed.lint_report = Some(msg);
}

/// Vai para o buraco seguinte (`step` = 1) ou anterior (-1) do Verificar e centraliza a visão nele.
pub(crate) fn lint_go(env: &mut Env, step: i32) {
    let m = &mut env.ed.lint_marks;
    if m.holes.is_empty() {
        return;
    }
    let n = m.holes.len() as i32;
    m.current = (m.current as i32 + step).rem_euclid(n) as usize;
    let h = &m.holes[m.current];
    let c = h.corners.iter().fold(Vec2::ZERO, |a, p| a + *p) / 4.0;
    env.ed.view.center = c;
}

/// Pinta o buraco atual do Verificar com a cor ativa (o mesmo balde do clique) e confere de novo.
pub(crate) fn lint_fill_current(env: &mut Env) {
    let Some(h) = env.ed.lint_marks.holes.get(env.ed.lint_marks.current).cloned() else { return };
    let cur = env.ed.lint_marks.current;
    do_fill(env, h.point, Instant::now());
    run_lint(env);
    let n = env.ed.lint_marks.holes.len();
    if n > 0 {
        env.ed.lint_marks.current = cur.min(n - 1);
    }
}

// ------------------------------------------------------------------------------------------------ conta-gotas

pub(crate) fn do_eyedrop(env: &mut Env, p: Vec2) {
    // Conta-gotas > cor da camada (S25): lê só a camada ativa (o Alt das outras ferramentas lê o que aparece)
    if *env.tool == ToolId::Eyedropper && env.ed.opts.area.eyedrop_layer {
        super::area_tools::eyedrop_layer(env, p);
        return;
    }
    let Some(shown) = &env.ed.shown else { return };
    if let Some(c) = shown.rendered.sample(p.x.floor() as i32, p.y.floor() as i32) {
        env.ed.set_color_direct(Rgba([c[0], c[1], c[2], 1.0]));
    }
}

// ------------------------------------------------------------------------------------------------ seleção e mover
// (a ferramenta Seleção, a varinha, Selecionar traços e o que uma área leva ficam em `selection.rs`)

/// Prepara o gesto de mover: sem seleção, move o conteúdo todo da camada.
pub(crate) fn begin_move(env: &mut Env) -> bool {
    let Some(t) = target(env, &[LayerType::Vector, LayerType::Raster], "err.move_layer") else { return false };
    if env.ed.selection.is_none() {
        let (w, h) = cmds::canvas_size(&env.core.project, &t.cut);
        env.ed.selection = Some(Selection::new(SelMode::Rect, vec![Vec2::ZERO, Vec2::new(w as f32, h as f32)], true));
    }
    true
}

/// Chave das imagens do Mover ao vivo para o estado atual (camada ativa movível), ou `None`.
pub(crate) fn move_key(env: &mut Env) -> Option<super::move_live::MoveKey> {
    let cut = env.ed.active_cut(env.core)?;
    let layer = env.ed.active_layer(env.core)?;
    let l = cmds::layer_data(&env.core.project, &layer)?;
    if !matches!(l.kind, LayerType::Vector | LayerType::Raster) || l.locked || !l.visible {
        return None;
    }
    let sel = super::selection::move_sel_key(env.ed.selection.as_ref());
    Some(super::move_live::MoveKey { counter: env.core.change_counter, cut, frame: env.ed.frame, layer, sel, guides: env.ed.show_guides, reference: env.ed.show_reference })
}

/// Com o Mover ativo e parado, prepara as imagens do arrasto em segundo plano (o arrasto começa sem esperar). Numa
/// camada vetorial vão os ids dos traços que andam (os mesmos do commit); numa raster, a máscara da área.
pub(crate) fn move_prefetch(env: &mut Env, ctx: &egui::Context) {
    if !matches!(env.ed.gesture, Gesture::None) {
        return;
    }
    let Some(key) = move_key(env) else { return };
    if !env.ed.move_live.wants(&key) {
        return;
    }
    let sel = env.ed.selection.clone().filter(|s| !s.whole);
    let vector = cmds::layer_data(&env.core.project, &key.layer).is_some_and(|l| l.kind == LayerType::Vector);
    let (w, h) = cmds::canvas_size(&env.core.project, &key.cut);
    let (strokes, mask) = match &sel {
        None => (None, None),
        Some(s) if vector => (Some(if s.strokes.is_empty() { super::selection::area_stroke_ids(env, s).to_vec() } else { s.strokes.clone() }), None),
        Some(s) => (None, super::selection::area_cov(s, w, h).map(std::sync::Arc::new)),
    };
    let (project, blobs) = (&env.core.project, &env.core.blobs);
    env.ed.move_live.want(ctx, || super::move_live::Request { key: key.clone(), project: project.clone(), blobs: blobs.clone(), mask, strokes }, &key);
}

pub(crate) fn commit_move(env: &mut Env) {
    let released_at = Instant::now();
    let Some(mut sel) = env.ed.selection.clone() else { return };
    if sel.is_identity() {
        return;
    }
    let plain = sel.plain_scale() && sel.rotate.abs() < 1e-3;
    let Some(t) = target(env, &[LayerType::Vector, LayerType::Raster], "err.move_layer") else { return };
    if t.kind == LayerType::Raster && plain {
        sel.offset = Vec2::new(sel.offset.x.round(), sel.offset.y.round());
        if sel.offset == Vec2::ZERO {
            env.ed.selection.as_mut().map(|s| s.offset = Vec2::ZERO);
            return;
        }
    }
    let off = sel.offset;
    // o mesmo plano que o Mover ao vivo mostrou: traços (vetor) ou máscara (raster), levados ao espaço do cel
    let planned = super::selection::plan_move(env, &sel, &t.cut, &t.layer, t.frame, t.kind).and_then(|plan| {
        let req = ttransform::TransformRequest { cut: t.cut.clone(), frame: t.frame, layer: t.layer.clone(), deform: plan.deform, selection: None, scale_width: true, strokes: plan.strokes };
        ttransform::transform_command_masked(&env.core.project, &mut env.core.blobs, &req, plan.mask.as_ref()).map_err(|e| e.to_string())
    });
    let ok = planned.is_ok();
    commit(env, planned, released_at, "move", None);
    if ok {
        if !sel.strokes.is_empty() {
            // os mesmos traços continuam escolhidos, com a caixa refeita no lugar novo
            super::selection::select_strokes(env, sel.strokes.clone());
        } else if let Some(s) = env.ed.selection.as_mut() {
            if s.whole {
                env.ed.selection = None;
            } else {
                s.offset = off;
                s.bake_offset();
            }
        }
    } else {
        // nada para mover (área vazia na camada vetorial, erro): a seleção volta ao lugar
        if let Some(s) = env.ed.selection.as_mut() {
            s.offset = Vec2::ZERO;
            s.scale = Vec2::splat(1.0);
            s.rotate = 0.0;
        }
        if env.ed.selection.as_ref().is_some_and(|s| s.whole) {
            env.ed.selection = None;
        }
    }
}

/// Ctrl+T: transformação livre no Mover: sobre a escolha atual, ou sobre tudo da camada quando não há escolha.
pub(crate) fn free_transform(env: &mut Env) {
    *env.tool = ToolId::Move;
    if env.ed.selection.as_ref().is_none_or(|s| s.whole) {
        env.ed.selection = None;
        super::selection::select_all(env);
    }
}

pub(crate) fn cancel_move(ed: &mut Editor) {
    if let Some(s) = ed.selection.as_mut() {
        if !s.is_identity() {
            s.offset = Vec2::ZERO;
            s.scale = Vec2::splat(1.0);
            s.rotate = 0.0;
            if s.whole {
                ed.selection = None;
            }
            return;
        }
    }
    ed.selection = None;
}

// ------------------------------------------------------------------------------------------------ atalhos

/// Teclas do editor. Ficam desligadas enquanto um campo de texto tem o foco ou um menu/popup está aberto (roda antes da
/// barra de menus no quadro: consumir Esc/setas aqui impedia o menu de fechar com Esc).
pub(crate) fn shortcuts(ctx: &egui::Context, env: &mut Env) {
    // digitando num texto no canvas: as teclas são do texto (a ferramenta Texto trata o teclado em `text_tool::frame`)
    if ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) || super::text_tool::editing(env.ed) {
        return;
    }
    use crate::keymap::Action as A;
    use egui::{Key, Modifiers};
    // curva/linha por cliques: Enter termina, Esc/Backspace tiram o último ponto (antes dos atalhos gerais)
    super::pen::shortcuts(ctx, env);
    // Figura em vários passos (Bézier, curva de dois passos) e degradê livre: Enter/Esc/Backspace/Delete deles
    super::figure::shortcuts(ctx, env);
    super::gradient_tool::shortcuts(ctx, env);
    let none = Modifiers::NONE;
    let keys = env.ed.keys.clone();
    // Editar pontos com um ponto escolhido: Delete apaga, . e , passam de ponto, C alterna canto/suave,
    // Shift+C volta a alça para a automática, setas movem (Shift = 10 px) e Esc solta o ponto. Vem antes dos atalhos
    // gerais (as setas trocam de quadro quando não há ponto escolhido).
    if *env.tool == ToolId::Points && env.ed.node_stroke.is_some() {
        let has_point = env.ed.node_sel.is_some();
        let (del, next, prev, corner, auto, arrows, esc) = ctx.input_mut(|i| {
            let hit = |a: A, i: &mut egui::InputState| keys.get(a).is_some_and(|c| c.take(i));
            let (next, prev) = (hit(A::PointNext, i), hit(A::PointPrev, i));
            let (del, corner, auto) = if has_point {
                let d = hit(A::PointDelete, i) || i.consume_key(Modifiers::NONE, Key::Backspace);
                (d, hit(A::PointCorner, i), hit(A::PointAutoHandle, i))
            } else {
                (false, false, false)
            };
            let mut arrows = Vec2::ZERO;
            if has_point {
                // Shift primeiro: sem modificador, o egui também casa a tecla com Shift apertado
                for (m, step) in [(Modifiers::SHIFT, 10.0), (Modifiers::NONE, 1.0)] {
                    for (k, d) in [(Key::ArrowLeft, Vec2::new(-1.0, 0.0)), (Key::ArrowRight, Vec2::new(1.0, 0.0)), (Key::ArrowUp, Vec2::new(0.0, -1.0)), (Key::ArrowDown, Vec2::new(0.0, 1.0))] {
                        if i.consume_key(m, k) {
                            arrows += d * step;
                        }
                    }
                }
            }
            (del, next, prev, corner, auto, arrows, i.consume_key(Modifiers::NONE, Key::Escape))
        });
        if del {
            super::nodes::delete_selected(env);
        }
        if next || prev {
            super::nodes::select_step(env, if next { 1 } else { -1 });
        }
        if corner {
            super::nodes::toggle_corner(env);
        }
        if auto {
            super::nodes::auto_handle(env);
        }
        if arrows != Vec2::ZERO {
            super::nodes::nudge(env, arrows.x, arrows.y);
        }
        if esc {
            // Esc solta o ponto; com nenhum ponto, solta o traço
            if env.ed.node_sel.take().is_none() {
                env.ed.node_stroke = None;
            }
        }
    }
    // Ctrl+T transformação livre, Ctrl+A tudo, Ctrl+D desfaz a seleção (como no Photoshop)
    let (ct, ca, cd) = ctx.input_mut(|i| (i.consume_key(Modifiers::COMMAND, Key::T), i.consume_key(Modifiers::COMMAND, Key::A), i.consume_key(Modifiers::COMMAND, Key::D)));
    if ct {
        free_transform(env);
    }
    if ca {
        env.ed.selection = None;
        super::selection::select_all(env);
    }
    if cd {
        env.ed.selection = None;
        env.ed.sel_state.wand = None;
    }
    // Ctrl+Shift+I inverte; laço poligonal: Enter fecha, Backspace tira o último vértice
    super::selection::shortcuts(ctx, env);
    super::rulers::shortcuts(ctx, env);
    super::model3d::shortcuts(ctx, env);
    let (tool_key, size_dir, enter, esc, step, home_end, fit, one) = ctx.input_mut(|i| {
        // atalho configurável (Preferências > Atalhos): só dispara com os modificadores exatos
        let mut hit = |a: A| keys.get(a).is_some_and(|c| c.take(i));
        let mut tk: Option<ToolId> = None;
        for (a, t) in [
            (A::ToolMove, ToolId::Move),
            (A::ToolBrush, ToolId::Brush),
            (A::ToolPen, ToolId::Pen),
            (A::ToolPoints, ToolId::Points),
            (A::ToolEraser, ToolId::Eraser),
            (A::ToolFill, ToolId::Fill),
            (A::ToolLasso, ToolId::Lasso),
            (A::ToolEyedropper, ToolId::Eyedropper),
            (A::ToolHand, ToolId::Hand),
            (A::ToolZoom, ToolId::Zoom),
            (A::ToolText, ToolId::Text),
            (A::ToolRuler, ToolId::Ruler),
            (A::ToolModel3d, ToolId::Model3d),
        ] {
            if hit(a) {
                tk = Some(t);
            }
        }
        let sd = hit(A::SizeUp) as i32 - hit(A::SizeDown) as i32;
        let st = hit(A::FrameNext) as i32 - hit(A::FramePrev) as i32;
        let he = if hit(A::FrameFirst) {
            -1
        } else if hit(A::FrameLast) {
            1
        } else {
            0
        };
        let (fit, one) = (hit(A::ViewFit), hit(A::ViewActual));
        (tk, sd, i.consume_key(none, Key::Enter), i.consume_key(none, Key::Escape), st, he, fit, one)
    });
    if let Some(t) = tool_key {
        *env.tool = t;
    }
    if size_dir != 0 {
        let k = if size_dir > 0 { 1.15f32 } else { 1.0 / 1.15 };
        let o = &mut env.ed.opts;
        match *env.tool {
            ToolId::Pen => o.pen_width = (o.pen_width * k).clamp(0.5, 200.0),
            ToolId::Brush => o.brush_size = (o.brush_size * k).clamp(1.0, 600.0),
            ToolId::Eraser => o.eraser_size = (o.eraser_size * k).clamp(1.0, 600.0),
            // Editar pontos: [ e ] afinam/engrossam o traço no ponto escolhido
            ToolId::Points => super::nodes::width_step(env, k),
            _ => {}
        }
    }
    if enter {
        if matches!(env.ed.gesture, Gesture::PolyCurve { .. }) {
            super::pen::finish(env, false);
        } else {
            commit_move(env);
        }
    }
    if esc {
        if !matches!(env.ed.gesture, Gesture::None) {
            env.ed.gesture = Gesture::None;
        } else {
            cancel_move(env.ed);
        }
    }
    if step != 0 || home_end != 0 {
        env.ed.playback.playing = false;
        let dur = env.ed.cut.as_ref().and_then(|c| cmds::cut_data(&env.core.project, c)).map(|c| c.duration).unwrap_or(1);
        env.ed.frame = match home_end {
            -1 => 0,
            1 => dur.saturating_sub(1),
            _ => (env.ed.frame as i64 + step as i64).clamp(0, dur.saturating_sub(1) as i64) as u32,
        };
    }
    if fit {
        env.ed.fit_pending = true;
    }
    if one {
        if let Some(rect) = env.ed.canvas_rect {
            let ppp = ctx.pixels_per_point();
            env.ed.view.set_zoom_at(rect, ppp, rect.center(), 1.0);
            env.ed.auto_fit = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::commands::{add_layer, quick_start};

    /// Quanto um carimbo da borracha (raio 20) tira de uma camada opaca, do centro para a borda (px 0..32).
    fn erased_profile(o: &ToolOptions, brush: Option<BrushPreset>) -> Vec<f32> {
        let p = eraser_preset(o, brush);
        let mut img = aiai_paint::Image::filled(64, 64, [0.2, 0.3, 0.4, 1.0]);
        aiai_paint::brush::paint_stroke(&mut img, &p, &[aiai_paint::brush::BrushPoint { x: 32.0, y: 32.0, pressure: 1.0 }], [0.0, 0.0, 0.0, 1.0], 1);
        (0..32).map(|i| 1.0 - img.get(32 + i, 32)[3]).collect()
    }

    /// Dureza contínua 0..1 (o controle de 0–100%): miolo apagado por inteiro até a fração `h` do raio e queda em
    /// cosseno até a borda; mais dura apaga sempre pelo menos tanto quanto mais macia.
    #[test]
    fn eraser_hardness_curve() {
        let prof = |h: f32| erased_profile(&ToolOptions { eraser_size: 40.0, eraser_hardness: h, ..Default::default() }, None);
        let (hard, mid, soft) = (prof(1.0), prof(0.5), prof(0.0));
        for (name, p) in [("dura", &hard), ("média", &mid), ("macia", &soft)] {
            assert!(p.windows(2).all(|w| w[1] <= w[0] + 1e-4), "{name}: o quanto sai só cai para a borda: {p:?}");
            assert!(p[0] > 0.99, "{name}: o centro sai inteiro: {}", p[0]);
            assert!(p[25] < 1e-3, "{name}: fora do raio nada sai");
        }
        assert!(hard[18] > 0.99, "dura: até a borda");
        // h = 0,5: inteiro até meio raio (d = 9,5/20), metade da queda perto de 3/4 do raio, quase nada na borda
        assert!(mid[9] > 0.99 && (0.35..0.75).contains(&mid[14]) && mid[19] < 0.1, "média: {mid:?}");
        // macia: cai desde o centro
        assert!(soft[10] < 0.6 && soft[10] > 0.3, "macia: {soft:?}");
        for i in 0..32 {
            assert!(hard[i] + 1e-4 >= mid[i] && mid[i] + 1e-4 >= soft[i], "px {i}: dura ≥ média ≥ macia");
        }
        // opacidade limita o quanto sai
        let half = erased_profile(&ToolOptions { eraser_size: 40.0, eraser_opacity: 0.5, ..Default::default() }, None);
        assert!((half[0] - 0.5).abs() < 0.02, "opacidade 50%: {}", half[0]);
    }

    /// Lápis: borda dura sem antisserrilhado (só tudo ou nada); Bloco: quadrado do tamanho escolhido; pincel atual:
    /// a ponta do pincel apagando.
    #[test]
    fn eraser_tips_and_brush_as_eraser() {
        let pencil = erased_profile(&ToolOptions { eraser_size: 40.0, eraser_tip: EraserTip::Pencil, ..Default::default() }, None);
        assert!(pencil.iter().all(|v| *v < 1e-4 || *v > 0.999), "lápis sem meio-termo: {pencil:?}");
        let block = ToolOptions { eraser_size: 20.0, eraser_tip: EraserTip::Block, ..Default::default() };
        let p = eraser_preset(&block, None);
        let mut img = aiai_paint::Image::filled(64, 64, [0.2, 0.3, 0.4, 1.0]);
        aiai_paint::brush::paint_stroke(&mut img, &p, &[aiai_paint::brush::BrushPoint { x: 32.0, y: 32.0, pressure: 1.0 }], [0.0, 0.0, 0.0, 1.0], 1);
        assert!(img.get(40, 40)[3] < 1e-3, "o canto do quadrado sai (a redonda deixaria)");
        assert!(img.get(43, 32)[3] > 0.999 && img.get(32, 43)[3] > 0.999, "nada além do lado de 20 px");
        let grass = presets().iter().find(|b| matches!(b.tip, aiai_paint::brush::BrushTip::Shape { shape: aiai_paint::brush::TipShape::GrassBlade, .. })).cloned().expect("pincel de grama");
        let o = ToolOptions { eraser_size: 33.0, eraser_use_brush: true, ..Default::default() };
        let e = eraser_preset(&o, Some(grass.clone()));
        assert_eq!((e.mode, e.size, e.tip.clone(), e.two_tone.is_none()), (BrushMode::Erase, 33.0, grass.tip, true));
        let pressure_off = eraser_preset(&ToolOptions { eraser_pressure_size: false, ..Default::default() }, None);
        assert_eq!(pressure_off.size_pressure_min, 1.0, "sem pressão no tamanho");
    }

    fn doc() -> (CoreSession, NodeId, NodeId, NodeId, NodeId) {
        let mut core = CoreSession::for_tests("t");
        core.project.settings.width = 64;
        core.project.settings.height = 36;
        let cut = quick_start(&mut core, 12, None).unwrap();
        let line = add_layer(&mut core, &cut, LayerType::Vector, None).unwrap();
        let fill = add_layer(&mut core, &cut, LayerType::Fill, Some(&line)).unwrap();
        let ras = add_layer(&mut core, &cut, LayerType::Raster, Some(&fill)).unwrap();
        (core, cut, line, fill, ras)
    }

    #[test]
    fn fill_plan_follows_line_refs_and_neighbours() {
        let (core, cut, line, fill, ras) = doc();
        let p = &core.project;
        // ativa = camada de preenchimento ligada à linha
        assert_eq!(fill_plan(p, &cut, &fill), Some((fill.clone(), vec![line.clone()], None)));
        // ativa = a linha: acha o balde que a referencia
        assert_eq!(fill_plan(p, &cut, &line), Some((fill.clone(), vec![line.clone()], None)));
        // ativa = raster: a camada de preenchimento mais próxima
        assert_eq!(fill_plan(p, &cut, &ras).map(|x| x.0), Some(fill.clone()));
        // sem line_refs: usa o vetor vizinho e pede para gravar
        let mut core2 = core;
        cmds::set_layer_props(&mut core2, &fill, serde_json::json!({"line_refs": []})).unwrap();
        assert_eq!(fill_plan(&core2.project, &cut, &fill), Some((fill.clone(), vec![line.clone()], Some(line.clone()))));
        // sem camada de preenchimento nenhuma: nada
        let mut c3 = CoreSession::for_tests("x");
        let cut3 = quick_start(&mut c3, 4, None).unwrap();
        let v = add_layer(&mut c3, &cut3, LayerType::Vector, None).unwrap();
        assert_eq!(fill_plan(&c3.project, &cut3, &v), None);
    }

    /// Quadrado desenhado como a caneta faz (amostras a cada 4 px, pressão 0,7, estabilizador): o balde precisa enxergar uma
    /// região fechada, e a camada de preenchimento fica ABAIXO da linha.
    #[test]
    fn the_bucket_fills_a_square_drawn_like_the_pen_does() {
        let (mut core, cut, line, fill, _) = doc();
        core.project.settings.width = 320;
        core.project.settings.height = 180;
        let corners = [(80.0f32, 40.0f32), (240.0, 40.0), (240.0, 140.0), (80.0, 140.0), (80.0, 40.0)];
        let mut stab = Stabilizer::new(StabMode::Average, 4, 0.0);
        let mut pts: Vec<StrokePoint> = Vec::new();
        let push = |p: Vec2, pr: f32, pts: &mut Vec<StrokePoint>| pts.push(StrokePoint { x: p.x, y: p.y, pressure: pr, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None });
        let mut raw = vec![];
        for w in corners.windows(2) {
            let (a, b) = (Vec2::new(w[0].0, w[0].1), Vec2::new(w[1].0, w[1].1));
            let n = ((b - a).length() / 4.0).ceil() as usize;
            for i in 0..=n {
                raw.push(a + (b - a) * (i as f32 / n as f32));
            }
        }
        for (i, p) in raw.iter().enumerate() {
            if let Some(q) = stab.feed(*p) {
                push(q, if i == 0 { 1.0 } else { 0.7 }, &mut pts);
            }
        }
        for q in stab.finish() {
            push(q, 0.7, &mut pts);
        }
        let closed = should_close(&pts, 3.0, 1.0);
        if closed {
            pts.pop();
        }
        let stroke = Stroke {
            id: Id(String::new()),
            points: pts,
            geometry: StrokeGeometry::Freehand,
            tension: 0.0,
            tip: VectorTip::GPen,
            width: 3.0,
            profile: WidthProfile::default(),
            color: ColorRef::default(),
            opacity: 1.0,
            closed,
            aliased: false,
            color_trace: false,
        };
        let cel = cmds::exposed_cel(&core.project, &cut, &line, 0).unwrap();
        cmds::run(&mut core, Command::InsertStrokes { cel, items: vec![IndexedStroke { index: 0, stroke }] }).unwrap();
        let (fill_layer, lines, _) = fill_plan(&core.project, &cut, &line).unwrap();
        assert_eq!((fill_layer.clone(), lines), (fill.clone(), vec![line.clone()]));
        let mut blobs = core.blobs.clone();
        let req = tfill::FillRequest { cut: cut.clone(), frame: 0, layer: fill.clone(), seeds: vec![tfill::FillSeed { x: 160.0, y: 90.0, color: ColorRef::Direct { rgba: Rgba::rgb8(255, 0, 0) } }], line_layers: vec![], gap: 3.0, expand: 2, line_threshold: 0.35, close_leftovers: None, skip_seeds_on_line: false, only_unpainted: false };
        let cmd = tfill::fill_command(&core.project, &mut blobs, &req).unwrap();
        core.blobs = blobs;
        cmds::run(&mut core, cmd).unwrap();
        let fcel = cmds::exposed_cel(&core.project, &cut, &fill, 0).unwrap();
        let region = &cmds::cel_data(&core.project, &fcel).unwrap().fills[0];
        assert!(region.bounds[2] < 200 && region.bounds[3] < 130, "a região é só o interior do quadrado, não o canvas todo: {:?}", region.bounds);
    }

    fn sp(x: f32, y: f32) -> StrokePoint {
        StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }
    }

    #[test]
    fn closing_pen_strokes() {
        let ring: Vec<StrokePoint> = (0..=40)
            .map(|i| {
                let a = i as f32 / 40.0 * std::f32::consts::TAU;
                sp(100.0 + 50.0 * a.cos(), 100.0 + 50.0 * a.sin())
            })
            .collect();
        assert!(should_close(&ring, 3.0, 1.0), "um círculo que volta ao começo fecha");
        let open: Vec<StrokePoint> = (0..=40).map(|i| sp(i as f32 * 5.0, 0.0)).collect();
        assert!(!should_close(&open, 3.0, 1.0), "uma reta não fecha");
        assert!(!should_close(&ring[..6], 3.0, 1.0), "poucos pontos não fecham");
    }

    #[test]
    fn eraser_hits_only_strokes_on_the_path() {
        let (mut core, cut, line, _, _) = doc();
        let mk = |y: f32| Stroke {
            id: Id(String::new()),
            points: vec![StrokePoint { x: 5.0, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }, StrokePoint { x: 55.0, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }],
            geometry: StrokeGeometry::Polyline,
            tension: 0.0,
            tip: VectorTip::Marker,
            width: 2.0,
            profile: WidthProfile::default(),
            color: ColorRef::default(),
            opacity: 1.0,
            closed: false,
            aliased: false,
            color_trace: false,
        };
        let cel = cmds::exposed_cel(&core.project, &cut, &line, 0).unwrap();
        cmds::run(&mut core, Command::InsertStrokes { cel, items: vec![IndexedStroke { index: 0, stroke: mk(10.0) }, IndexedStroke { index: 1, stroke: mk(25.0) }] }).unwrap();
        let hits = eraser_hits(&core, &cut, &line, 0, &[Vec2::new(30.0, 24.0), Vec2::new(32.0, 26.0)], 3.0);
        assert_eq!(hits.len(), 1);
        let none = eraser_hits(&core, &cut, &line, 0, &[Vec2::new(30.0, 17.0)], 3.0);
        assert!(none.is_empty());
        let both = eraser_hits(&core, &cut, &line, 0, &[Vec2::new(30.0, 0.0), Vec2::new(30.0, 35.0)], 3.0);
        assert_eq!(both.len(), 2, "o caminho vertical cruza os dois");
    }

    #[test]
    fn pen_shapes_snap_with_shift() {
        let (a, b) = (Vec2::new(0.0, 0.0), Vec2::new(100.0, 8.0));
        let (pts, geo, closed) = shape_points(PenShape::Line, a, b, true);
        assert_eq!((geo, closed), (StrokeGeometry::Polyline, false));
        assert!(pts[1].y.abs() < 1e-3 && (pts[1].x - 100.3).abs() < 0.5, "linha presa no horizontal: {:?}", pts[1]);
        let (sq, _, closed) = shape_points(PenShape::Rect, a, Vec2::new(50.0, 20.0), true);
        assert!(closed && sq.len() == 4 && (sq[2].x - 50.0).abs() < 1e-3 && (sq[2].y - 50.0).abs() < 1e-3, "quadrado: {sq:?}");
        let (el, geo, closed) = shape_points(PenShape::Ellipse, a, Vec2::new(40.0, 20.0), false);
        assert_eq!((geo, closed, el.len()), (StrokeGeometry::Spline, true, 16));
        assert!((el[0].x - 40.0).abs() < 1e-3 && (el[0].y - 10.0).abs() < 1e-3);
    }
}
