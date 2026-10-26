//! Balde do S25 (à moda do Clip Studio): os jeitos do balde no canvas.
//! - Outras camadas: o clique de tinta-e-pinta de sempre (`tools::do_fill`: camada de preenchimento + linhas de
//!   referência);
//! - Só a camada em edição: as cores da camada ativa delimitam e a cor entra nela (pixels na raster);
//! - Cercar e preencher: um laço; as regiões fechadas que ele cerca são pintadas;
//! - Pintar área não preenchida: um pincel; as regiões sem tinta que ele toca são pintadas.
//!
//! A conta mora em `aiai_tools::fill_area` (a mesma da ação `FILL_AREA` da bridge / `aiai_fill_area` do MCP).

use super::area_tools::FillMode;
use super::commands::{self as cmds};
use super::editor::*;
use super::ui_kit::Env;
use crate::i18n::tr;
use aiai_core::*;
use aiai_tools::fill_area::{FillAreaMode, FillAreaRequest, fill_area};
use egui::{Color32, Pos2, Shape, Stroke as EStroke, Vec2};
use std::time::Instant;

/// Começo do gesto do balde: o clique age ao soltar; cercar e preencher / área não preenchida desenham um caminho.
pub(crate) fn press(env: &mut Env, c: Vec2) -> Gesture {
    if env.ed.opts.area.fill_mode.is_path() { Gesture::FillPath { pts: vec![c] } } else { Gesture::FillClick }
}

/// Clique do balde (ao soltar).
pub(crate) fn click(env: &mut Env, c: Vec2, at: Instant) {
    match env.ed.opts.area.fill_mode {
        FillMode::Others => super::tools::do_fill(env, c, at),
        FillMode::Editing => fill_editing(env, c),
        // um clique sem arrasto: o pincel da área não preenchida ainda vale num ponto só
        FillMode::Unfilled => finish_path(env, vec![c], at),
        FillMode::Enclose => {}
    }
}

/// Leva um ponto do canvas para o espaço do cel da camada (o balde trabalha sem o movimento da camada).
fn to_cel(env: &mut Env, cut: &super::commands::NodeId, layer: &super::commands::NodeId, p: Vec2) -> Vec2 {
    match cmds::layer_placement(&env.core.project, cut, layer, env.ed.frame).and_then(|a| a.inverse()) {
        Some(a) => {
            let (x, y) = a.apply(p.x, p.y);
            Vec2::new(x, y)
        }
        None => p,
    }
}

/// A área da seleção (px do canvas) para o balde só pintar dentro dela.
fn clip(env: &Env, w: u32, h: u32) -> Option<aiai_paint::select::Selection> {
    let s = env.ed.selection.as_ref().filter(|s| !s.whole && s.strokes.is_empty())?;
    super::selection::area_cov(s, w, h)
}

fn base_request(env: &mut Env, layer: &super::commands::NodeId, cut: &super::commands::NodeId, mode: FillAreaMode) -> FillAreaRequest {
    let o = &env.ed.opts;
    let (w, h) = cmds::canvas_size(&env.core.project, cut);
    FillAreaRequest {
        cut: cut.clone(),
        frame: env.ed.frame,
        layer: layer.clone(),
        mode,
        color: env.ed.color.clone(),
        point: None,
        tolerance: o.area.fill_tolerance,
        refer_all: false,
        path: Vec::new(),
        radius: o.area.fill_brush.max(1.0) / 2.0,
        min_inside: 0.95,
        line_layers: Vec::new(),
        gap: o.fill_gap,
        expand: o.fill_expand,
        scale: 0.0,
        clip: clip(env, w, h),
    }
}

fn run(env: &mut Env, req: FillAreaRequest, label: &str) {
    let r = {
        let core = &mut *env.core;
        fill_area(&core.project, &mut core.blobs, &req).map_err(|e| e.to_string())
    };
    match r {
        Ok(plan) => {
            let cmd = Command::Batch { commands: vec![plan.command], label: Some(label.to_owned()) };
            if let Err(e) = cmds::run(env.core, cmd) {
                env.ed.notify(e);
            }
        }
        Err(e) => env.ed.notify(e),
    }
}

/// Só a camada em edição: as cores da camada ativa delimitam a região do clique e a cor entra nela.
fn fill_editing(env: &mut Env, c: Vec2) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let Some(layer) = env.ed.active_layer(env.core) else {
        env.ed.notify(tr("err.no_layer"));
        return;
    };
    let Some(l) = cmds::layer_data(&env.core.project, &layer).cloned() else { return };
    if !matches!(l.kind, LayerType::Raster | LayerType::Vector | LayerType::Fill) {
        env.ed.notify(tr("fillm.err_layer"));
        return;
    }
    if l.locked || !l.visible {
        env.ed.notify(tr(if l.locked { "err.layer_locked" } else { "err.layer_hidden" }));
        return;
    }
    let p = to_cel(env, &cut, &layer, c);
    let mut req = base_request(env, &layer, &cut, FillAreaMode::Editing);
    req.point = Some([p.x, p.y]);
    run(env, req, tr("fillm.editing"));
}

/// Fim do laço (cercar e preencher) ou do pincel (área não preenchida): pinta na camada de preenchimento do plano do
/// balde (a mesma do clique de tinta-e-pinta), com as linhas de referência dela.
pub(crate) fn finish_path(env: &mut Env, pts: Vec<Vec2>, _at: Instant) {
    let mode = env.ed.opts.area.fill_mode;
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let Some(active) = env.ed.active_layer(env.core) else {
        env.ed.notify(tr("err.no_layer"));
        return;
    };
    // camada que recebe: a de preenchimento do plano do balde (com as linhas dela); sem ela, a própria camada ativa
    let (target, lines) = match super::tools::fill_plan(&env.core.project, &cut, &active) {
        Some((fill, lines, _)) => (fill, lines),
        None => (active.clone(), Vec::new()),
    };
    if let Some(l) = cmds::layer_data(&env.core.project, &target) {
        if l.locked || !l.visible {
            env.ed.notify(tr(if l.locked { "err.layer_locked" } else { "err.layer_hidden" }));
            return;
        }
    }
    if mode == FillMode::Enclose && pts.len() < 3 {
        return;
    }
    let path: Vec<[f32; 2]> = pts.iter().map(|p| to_cel(env, &cut, &target, *p)).map(|p| [p.x, p.y]).collect();
    let mut req = base_request(env, &target, &cut, if mode == FillMode::Enclose { FillAreaMode::Enclose } else { FillAreaMode::Unfilled });
    req.path = path;
    req.line_layers = lines;
    run(env, req, tr(mode.label_key()));
}

/// Prévia do gesto: o laço tracejado do cercar e preencher, o rastro do pincel da área não preenchida.
pub(crate) fn paint_path(env: &Env, painter: &egui::Painter, xf: &Xf, pts: &[Vec2]) {
    let v: Vec<Pos2> = pts.iter().map(|p| xf.to_screen(*p)).collect();
    if env.ed.opts.area.fill_mode == FillMode::Enclose {
        let phase = (painter.ctx().input(|i| i.time) * 12.0 % 8.0) as f32;
        super::selection::ants(painter, &v, true, phase);
        return;
    }
    let wpx = (env.ed.opts.area.fill_brush * xf.scale).max(1.0);
    let col = env.t.accent.gamma_multiply(0.4);
    if v.len() == 1 {
        painter.circle_filled(v[0], wpx / 2.0, col);
    } else {
        painter.add(Shape::line(v, EStroke::new(wpx, col)));
    }
    let _ = Color32::WHITE;
}
