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

fn eraser_preset(size: f32) -> BrushPreset {
    let mut p = presets().iter().find(|p| p.mode == BrushMode::Erase).or_else(|| presets().iter().find(|p| p.name == "Borracha")).cloned().unwrap_or_else(|| {
        let mut e = BrushPreset::round("Borracha", size, 0.9);
        e.mode = BrushMode::Erase;
        e
    });
    p.size = size;
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
        let p = match &self.inv {
            Some(a) => {
                let (x, y) = a.apply(p.x, p.y);
                Vec2::new(x, y)
            }
            None => p,
        };
        self.path.push(p);
        let keep = if self.tool == ToolId::Eraser { Some(p) } else { self.stab.feed(p) };
        if let Some(q) = keep {
            if self.samples.last().is_none_or(|l| (l.p - q).length() * self.screen_scale >= 0.4) {
                self.samples.push(Sample { p: q, pressure: pressure.clamp(0.0, 1.0), t: self.t0.elapsed().as_secs_f32() });
            }
        }
    }
}

pub(crate) fn begin_stroke(env: &mut Env, tool: ToolId, p: Vec2, pressure: f32, scale: f32) -> Gesture {
    let (allowed, wrong): (&[LayerType], &str) = match tool {
        ToolId::Pen => (&[LayerType::Vector], "err.need_vector"),
        ToolId::Brush => (&[LayerType::Raster], "err.need_raster"),
        _ => (&[LayerType::Vector, LayerType::Raster, LayerType::Fill], "err.eraser_layer"),
    };
    let Some(t) = target(env, allowed, wrong) else { return Gesture::None };
    let o = &env.ed.opts;
    let mode = if tool == ToolId::Eraser { StabMode::Off } else { o.stab_mode };
    let stab = Stabilizer::new(mode, o.stab_window as usize, o.stab_radius / scale.max(1e-3));
    // camada que se move neste quadro: o traço é gravado no espaço do cel para aparecer embaixo do cursor
    let fwd = cmds::layer_placement(&env.core.project, &t.cut, &t.layer, t.frame);
    let inv = fwd.and_then(|a| a.inverse());
    let fwd = inv.and(fwd);
    let scale = scale * fwd.map(|a| a.0[0].hypot(a.0[3])).unwrap_or(1.0);
    let mut g = StrokeGesture { tool, layer: t.layer, samples: Vec::new(), stab, t0: Instant::now(), hits: Vec::new(), path: Vec::new(), screen_scale: scale, fwd, inv };
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
    let mut points: Vec<StrokePoint> = g.samples.iter().map(|s| StrokePoint { x: s.p.x, y: s.p.y, pressure: s.pressure, tilt: None, t: Some(s.t) }).collect();
    if points.is_empty() {
        return;
    }
    if points.len() == 1 {
        let q = points[0];
        points.push(StrokePoint { x: q.x + 0.01, ..q });
    }
    let o = &env.ed.opts;
    // volta ao ponto de partida: fecha o traço (sem as pontas afinadas abrindo um vão que o balde não vê)
    let closed = o.pen_autoclose && should_close(&points, o.pen_width, g.screen_scale);
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
        Ok(Command::InsertStrokes { cel, items: vec![IndexedStroke { index, stroke }] })
    });
    let color = ghost_color(env, 1.0);
    commit(env, planned, released_at, "pen", Some((ghost_pts, ghost_radii, width, color)));
}

/// Amostras (posição, raio) de um traço vetorial como o motor desenha.
pub(crate) fn sampled(stroke: &Stroke) -> (Vec<Vec2>, Vec<f32>) {
    aiai_paint::sample_stroke(stroke).iter().map(|q| (Vec2::new(q.x, q.y), q.r)).unzip()
}

/// O traço que a caneta gravaria com estes pontos agora (para a prévia durante o arrasto).
pub(crate) fn pen_preview(env: &Env, g: &StrokeGesture) -> Option<Stroke> {
    let mut points: Vec<StrokePoint> = g.samples.iter().map(|s| StrokePoint { x: s.p.x, y: s.p.y, pressure: s.pressure, tilt: None, t: Some(s.t) }).collect();
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

fn commit_brush(env: &mut Env, t: &Target, g: &StrokeGesture, released_at: Instant, erase: bool) {
    if g.samples.is_empty() {
        return;
    }
    // espaça as amostras (~1,5 px) e sempre guarda a última: o motor de dab cuida do espaçamento fino
    let mut pts: Vec<Vec<f32>> = Vec::new();
    let mut last: Option<Vec2> = None;
    for s in &g.samples {
        if last.is_none_or(|l| (l - s.p).length() >= 1.5) {
            pts.push(vec![s.p.x, s.p.y, s.pressure]);
            last = Some(s.p);
        }
    }
    if let Some(s) = g.samples.last() {
        if last != Some(s.p) {
            pts.push(vec![s.p.x, s.p.y, s.pressure]);
        }
    }
    if pts.len() == 1 {
        let q = pts[0].clone();
        pts.push(vec![q[0] + 0.01, q[1], q[2]]);
    }
    let o = &env.ed.opts;
    let preset = if erase {
        eraser_preset(o.eraser_size)
    } else {
        let mut p = o.brush_custom.clone().or_else(|| presets().get(o.brush_preset).cloned()).or_else(|| presets().first().cloned()).unwrap_or_else(|| BrushPreset::round("Pincel", 24.0, 0.9));
        p.size = o.brush_size.max(0.5);
        p.opacity = o.brush_opacity.clamp(0.0, 1.0);
        p
    };
    let color = if erase { ColorRef::Direct { rgba: Rgba::BLACK } } else { env.ed.color.clone() };
    let size = preset.size;
    let opacity = preset.opacity;
    let seed = env.core.change_counter.wrapping_mul(2654435761);
    let (cut, layer, frame, pref) = (t.cut.clone(), t.layer.clone(), t.frame, env.ed.cel_number);
    let ghost_pts: Vec<Vec2> = g.samples.iter().map(|s| g.to_canvas(s.p)).collect();
    let planned = cmds::plan_with_prep(env.core, &cut, &layer, frame, pref, vec![], |p, blobs| {
        let req = tpaint::PaintRequest { cut: cut.clone(), frame, layer: layer.clone(), preset, color, strokes: vec![tpaint::PaintStrokeSpec { points: pts, smooth: true }], seed };
        tpaint::paint_command(p, blobs, &req).map_err(|e| e.to_string())
    });
    let ghost = if erase { None } else { Some((ghost_pts, Vec::new(), size, ghost_color(env, opacity.min(0.9)))) };
    commit(env, planned, released_at, if erase { "erase_raster" } else { "brush" }, ghost);
}

fn commit_erase_vector(env: &mut Env, t: &Target, g: &StrokeGesture, released_at: Instant) {
    let ids = eraser_hits(env.core, &t.cut, &t.layer, t.frame, &g.path, env.ed.opts.eraser_size / 2.0);
    if ids.is_empty() {
        return;
    }
    let Some(cel) = cmds::exposed_cel(&env.core.project, &t.cut, &t.layer, t.frame) else { return };
    let planned = Ok(Command::RemoveStrokes { cel, ids });
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
    let planned = cmds::plan_with_prep(env.core, &cut, &fill_layer, frame, pref, extra, |pr, blobs| {
        let req = tfill::FillRequest { cut: cut.clone(), frame, layer: fill_layer.clone(), seeds: vec![tfill::FillSeed { x: p.x, y: p.y, color }], line_layers: lines, gap, expand, line_threshold: 0.35, close_leftovers: close };
        tfill::fill_command(pr, blobs, &req).map_err(|e| e.to_string())
    });
    commit(env, planned, released_at, "fill", None);
}

/// Conta as regiões fechadas ainda sem tinta na camada de preenchimento ativa (botão "Verificar").
pub(crate) fn run_lint(env: &mut Env) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let Some(active) = env.ed.active_layer(env.core) else { return };
    let Some((fill_layer, lines, _)) = fill_plan(&env.core.project, &cut, &active) else {
        env.ed.lint_report = Some(tr("err.no_fill_layer").to_owned());
        return;
    };
    let _ = lines;
    let o = &env.ed.opts;
    let msg = match tfill::lint(&env.core.project, &env.core.blobs, &cut, env.ed.frame, &fill_layer, o.fill_gap, 40) {
        Ok(r) => tr("fill.lint_result").replace("{n}", &r.unpainted.len().to_string()).replace("{total}", &r.regions_total.to_string()),
        Err(e) => e.to_string(),
    };
    env.ed.lint_report = Some(msg);
}

// ------------------------------------------------------------------------------------------------ conta-gotas

pub(crate) fn do_eyedrop(env: &mut Env, p: Vec2) {
    let Some(shown) = &env.ed.shown else { return };
    if let Some(c) = shown.rendered.sample(p.x.floor() as i32, p.y.floor() as i32) {
        env.ed.set_color_direct(Rgba([c[0], c[1], c[2], 1.0]));
    }
}

// ------------------------------------------------------------------------------------------------ seleção e mover

pub(crate) fn finish_select(env: &mut Env, mode: SelMode, start: Vec2, pts: Vec<Vec2>) {
    let pts = match mode {
        SelMode::Lasso => pts,
        _ => {
            let end = pts.last().copied().unwrap_or(start);
            vec![start, end]
        }
    };
    let sel = Selection { mode, pts, offset: Vec2::ZERO, whole: false };
    // um clique sem arrastar limpa a seleção
    env.ed.selection = sel.shape().filter(|_| sel.rect_of().is_some_and(|r| r[2] >= 2.0 && r[3] >= 2.0)).map(|_| sel);
}

/// Prepara o gesto de mover: sem seleção, move o conteúdo todo da camada.
pub(crate) fn begin_move(env: &mut Env) -> bool {
    let Some(t) = target(env, &[LayerType::Vector, LayerType::Raster], "err.move_layer") else { return false };
    if env.ed.selection.is_none() {
        let (w, h) = cmds::canvas_size(&env.core.project, &t.cut);
        env.ed.selection = Some(Selection { mode: SelMode::Rect, pts: vec![Vec2::ZERO, Vec2::new(w as f32, h as f32)], offset: Vec2::ZERO, whole: true });
    }
    true
}

pub(crate) fn has_pending_move(ed: &Editor) -> bool {
    ed.selection.as_ref().is_some_and(|s| s.offset != Vec2::ZERO)
}

pub(crate) fn commit_move(env: &mut Env) {
    let released_at = Instant::now();
    let Some(sel) = env.ed.selection.clone() else { return };
    if sel.offset == Vec2::ZERO {
        return;
    }
    let Some(t) = target(env, &[LayerType::Vector, LayerType::Raster], "err.move_layer") else { return };
    let mut off = sel.offset;
    if t.kind == LayerType::Raster {
        off = Vec2::new(off.x.round(), off.y.round());
        if off == Vec2::ZERO {
            env.ed.selection.as_mut().map(|s| s.offset = Vec2::ZERO);
            return;
        }
    }
    let req = ttransform::TransformRequest {
        cut: t.cut.clone(),
        frame: t.frame,
        layer: t.layer.clone(),
        deform: ttransform::Deform::Affine { translate: [off.x, off.y], scale: [1.0, 1.0], rotate: 0.0, shear: [0.0, 0.0], pivot: [0.0, 0.0] },
        selection: if sel.whole { None } else { sel.shape() },
        scale_width: true,
        strokes: Vec::new(),
    };
    let planned = ttransform::transform_command(&env.core.project, &mut env.core.blobs, &req).map_err(|e| e.to_string());
    let ok = planned.is_ok();
    commit(env, planned, released_at, "move", None);
    if ok {
        if let Some(s) = env.ed.selection.as_mut() {
            if s.whole {
                env.ed.selection = None;
            } else {
                s.offset = off;
                s.bake_offset();
            }
        }
    }
}

pub(crate) fn cancel_move(ed: &mut Editor) {
    if let Some(s) = ed.selection.as_mut() {
        if s.offset != Vec2::ZERO {
            s.offset = Vec2::ZERO;
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
    if ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) {
        return;
    }
    use crate::keymap::Action as A;
    use egui::{Key, Modifiers};
    let none = Modifiers::NONE;
    let keys = env.ed.keys.clone();
    let (tool_key, size_dir, enter, esc, step, home_end, fit, one) = ctx.input_mut(|i| {
        // atalho configurável (Preferências > Atalhos): só dispara com os modificadores exatos
        let mut hit = |a: A| keys.get(a).is_some_and(|c| c.take(i));
        let mut tk: Option<ToolId> = None;
        for (a, t) in [
            (A::ToolMove, ToolId::Move),
            (A::ToolBrush, ToolId::Brush),
            (A::ToolPen, ToolId::Pen),
            (A::ToolEraser, ToolId::Eraser),
            (A::ToolFill, ToolId::Fill),
            (A::ToolLasso, ToolId::Lasso),
            (A::ToolEyedropper, ToolId::Eyedropper),
            (A::ToolHand, ToolId::Hand),
            (A::ToolZoom, ToolId::Zoom),
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
            _ => {}
        }
    }
    if enter {
        commit_move(env);
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
        let push = |p: Vec2, pr: f32, pts: &mut Vec<StrokePoint>| pts.push(StrokePoint { x: p.x, y: p.y, pressure: pr, tilt: None, t: None });
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
        let req = tfill::FillRequest { cut: cut.clone(), frame: 0, layer: fill.clone(), seeds: vec![tfill::FillSeed { x: 160.0, y: 90.0, color: ColorRef::Direct { rgba: Rgba::rgb8(255, 0, 0) } }], line_layers: vec![], gap: 3.0, expand: 2, line_threshold: 0.35, close_leftovers: None };
        let cmd = tfill::fill_command(&core.project, &mut blobs, &req).unwrap();
        core.blobs = blobs;
        cmds::run(&mut core, cmd).unwrap();
        let fcel = cmds::exposed_cel(&core.project, &cut, &fill, 0).unwrap();
        let region = &cmds::cel_data(&core.project, &fcel).unwrap().fills[0];
        assert!(region.bounds[2] < 200 && region.bounds[3] < 130, "a região é só o interior do quadrado, não o canvas todo: {:?}", region.bounds);
    }

    fn sp(x: f32, y: f32) -> StrokePoint {
        StrokePoint { x, y, pressure: 1.0, tilt: None, t: None }
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
            points: vec![StrokePoint { x: 5.0, y, pressure: 1.0, tilt: None, t: None }, StrokePoint { x: 55.0, y, pressure: 1.0, tilt: None, t: None }],
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
}
