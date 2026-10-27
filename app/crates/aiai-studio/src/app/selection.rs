//! Ferramenta Seleção (tecla S), do jeito do Photoshop / Clip Studio: retângulo, elipse, laço, laço poligonal, varinha
//! mágica e Selecionar traços (objetos da camada vetorial). Uma área é uma máscara do tamanho do canvas, em px do
//! canvas (com o movimento da camada já aplicado), e entra como nova / somar (Shift) / subtrair (Alt) / intersectar
//! (Shift+Alt), com borda suavizada opcional; Ctrl+Shift+I inverte, Ctrl+D tira, Ctrl+A pega tudo da camada.
//!
//! O Mover só move: a seleção (área ou traços escolhidos) ou, sem seleção, a camada inteira. Numa camada vetorial, os
//! traços que uma área leva são decididos aqui, uma vez, no espaço do canvas ([`area_stroke_ids`]); o commit, as
//! imagens do Mover ao vivo e a prévia de reserva usam essa mesma lista, então o que anda na tela durante o arrasto é
//! exatamente o que o commit grava. (Antes a prévia testava os pontos de controle crus, no espaço do cel, contra a
//! área no espaço do canvas e desenhava esses pontos sem o movimento da camada: numa camada com deslocamento/escala,
//! linhas "aleatórias" pelo canvas enquanto o botão ficava apertado.)

use super::commands::{self as cmds, NodeId};
use super::editor::*;
use super::ui_kit::Env;
use crate::i18n::tr;
use aiai_core::*;
use aiai_paint::Affine;
use aiai_paint::select::{SelectOp, Selection as Cov};
use egui::{Color32, Pos2, Shape, Stroke as EStroke, Vec2};
use std::sync::Arc;
use std::time::Instant;

/// Sub-ferramenta da Seleção (itens do painel Ferramenta).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SelTool {
    #[default]
    Rect,
    Ellipse,
    Lasso,
    /// Laço poligonal: cada clique põe um vértice; clique no primeiro, duplo clique, botão direito ou Enter fecha.
    Polygon,
    Wand,
    /// Selecionar traços (camada vetorial): clique escolhe o traço, arrasto no vazio = caixa dos traços que ela toca.
    Strokes,
    /// Caneta de seleção (S25): pinta a área com um pincel redondo (soma; com Alt/subtrair, apaga).
    Pen,
    /// Apagar seleção: o mesmo pincel tirando da área.
    Erase,
    /// Encolher seleção: um laço em volta do desenho encolhe até o contorno do que está pintado.
    Shrink,
    /// Laço magnético: o laço gruda na borda mais forte por perto.
    Magnetic,
}

impl SelTool {
    pub(crate) const ALL: [SelTool; 10] = [SelTool::Rect, SelTool::Ellipse, SelTool::Lasso, SelTool::Polygon, SelTool::Wand, SelTool::Strokes, SelTool::Pen, SelTool::Erase, SelTool::Shrink, SelTool::Magnetic];

    /// O gesto é um caminho à mão (laço, caneta/borracha de seleção, encolher, laço magnético): guarda cada ponto
    /// do arrasto (os outros guardam só o começo e o fim).
    pub(crate) fn is_path(self) -> bool {
        matches!(self, SelTool::Lasso | SelTool::Pen | SelTool::Erase | SelTool::Shrink | SelTool::Magnetic)
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            SelTool::Rect => "sel.rect",
            SelTool::Ellipse => "sel.ellipse",
            SelTool::Lasso => "sel.lasso",
            SelTool::Polygon => "sel.polygon",
            SelTool::Wand => "sel.wand",
            SelTool::Strokes => "sel.strokes",
            SelTool::Pen => "sel.pen",
            SelTool::Erase => "sel.erase",
            SelTool::Shrink => "sel.shrink",
            SelTool::Magnetic => "sel.magnetic",
        }
    }

    /// Nome estável (ui_state / bridge).
    pub(crate) fn id(self) -> &'static str {
        match self {
            SelTool::Rect => "rect",
            SelTool::Ellipse => "ellipse",
            SelTool::Lasso => "lasso",
            SelTool::Polygon => "polygon",
            SelTool::Wand => "wand",
            SelTool::Strokes => "strokes",
            SelTool::Pen => "pen",
            SelTool::Erase => "erase",
            SelTool::Shrink => "shrink",
            SelTool::Magnetic => "magnetic",
        }
    }
}

pub(crate) fn op_id(op: SelectOp) -> &'static str {
    match op {
        SelectOp::Replace => "replace",
        SelectOp::Add => "add",
        SelectOp::Subtract => "subtract",
        SelectOp::Intersect => "intersect",
    }
}

/// Área como máscara (varinha, somas, subtrações, borda suave), com a caixa e o contorno para as formigas.
#[derive(Debug)]
pub(crate) struct SelMask {
    pub w: u32,
    pub h: u32,
    pub cov: Vec<f32>,
    pub bbox: [f32; 4],
    pub outline: Vec<Vec<Vec2>>,
}

impl SelMask {
    pub(crate) fn from_cov(c: Cov) -> Option<SelMask> {
        if !c.cov.iter().any(|&v| v >= 0.5) {
            return None;
        }
        let b = c.bounds()?;
        let outline = c.outline().into_iter().map(|l| l.into_iter().map(|p| Vec2::new(p[0], p[1])).collect()).collect();
        Some(SelMask { w: c.width, h: c.height, bbox: [b[0] as f32, b[1] as f32, (b[2] - b[0]) as f32, (b[3] - b[1]) as f32], outline, cov: c.cov })
    }

    pub(crate) fn to_cov(&self) -> Cov {
        Cov { width: self.w, height: self.h, cov: self.cov.clone() }
    }

    /// A mesma máscara levada pela transformação `a` (px do canvas), reamostrada no vizinho mais próximo.
    pub(crate) fn transformed(&self, a: &Affine) -> Option<SelMask> {
        let inv = a.inverse()?;
        let (w, h) = (self.w as i64, self.h as i64);
        let mut c = Cov::none(self.w, self.h);
        for y in 0..h {
            for x in 0..w {
                let (sx, sy) = inv.apply(x as f32 + 0.5, y as f32 + 0.5);
                let (ix, iy) = (sx.floor() as i64, sy.floor() as i64);
                if ix >= 0 && iy >= 0 && ix < w && iy < h {
                    c.cov[(y * w + x) as usize] = self.cov[(iy * w + ix) as usize];
                }
            }
        }
        SelMask::from_cov(c)
    }
}

/// Último clique da varinha: refaz a seleção na hora quando a tolerância/opções mudam (a imagem de referência fica
/// guardada, então refazer é só a inundação).
pub(crate) struct WandClick {
    p: Vec2,
    op: SelectOp,
    base: Option<Arc<Cov>>,
    reference: Arc<aiai_paint::Image>,
    /// (documento, camada, quadro, camadas de referência) da imagem lida.
    sig: (u64, NodeId, u32, super::area_tools::WandRef),
    result: Option<Arc<SelMask>>,
}

#[derive(Default)]
pub(crate) struct SelState {
    pub wand: Option<WandClick>,
    /// Traços que a área leva (cache por documento, camada, quadro e área).
    area_ids: Option<(String, Arc<Vec<Id>>)>,
    /// Render da camada ativa (ou das de referência) e as bordas dela: conta-gotas da camada e laço magnético.
    pub layer_cache: super::area_tools::LayerCache,
}

/// Operação pelas teclas no momento do clique: Shift soma, Alt subtrai, Shift+Alt intersecta; sem tecla, a da barra.
pub(crate) fn op_for(mods: egui::Modifiers, bar: SelectOp) -> SelectOp {
    match (mods.shift, mods.alt) {
        (true, true) => SelectOp::Intersect,
        (true, false) => SelectOp::Add,
        (false, true) => SelectOp::Subtract,
        _ => bar,
    }
}

fn canvas_wh(env: &mut Env) -> Option<(u32, u32)> {
    let cut = env.ed.active_cut(env.core)?;
    Some(cmds::canvas_size(&env.core.project, &cut))
}

/// Cobertura (canvas `w`×`h`) da área escolhida, sem a transformação pendente. Traços escolhidos e "a camada
/// inteira" não são área.
pub(crate) fn area_cov(sel: &Selection, w: u32, h: u32) -> Option<Cov> {
    if sel.whole || !sel.strokes.is_empty() {
        return None;
    }
    if let Some(m) = &sel.mask {
        return (m.w == w && m.h == h).then(|| m.to_cov());
    }
    Cov::from_shape(&sel.shape()?, w, h, None).ok()
}

/// Seleção de área a partir de uma cobertura (vazia = sem seleção).
pub(crate) fn selection_from_cov(c: Cov) -> Option<Selection> {
    let m = SelMask::from_cov(c)?;
    let mut s = Selection::new(SelMode::Mask, Vec::new(), false);
    s.mask = Some(Arc::new(m));
    Some(s)
}

/// Combina a área nova com a atual pela operação (`simple` = a forma exata, usada quando é só "nova" sem borda suave).
fn combine_area(env: &mut Env, mut new: Cov, op: SelectOp, simple: Option<Selection>) -> Option<Selection> {
    let feather = env.ed.opts.sel_feather.round().clamp(0.0, 250.0) as u32;
    if op == SelectOp::Replace && feather == 0 {
        if let Some(s) = simple {
            return Some(s);
        }
    }
    if feather > 0 {
        new.feather(feather);
    }
    let base = if op == SelectOp::Replace { None } else { env.ed.selection.as_ref().filter(|s| s.is_identity()).and_then(|s| area_cov(s, new.width, new.height)) };
    let out = match (base, op) {
        (Some(mut b), _) => {
            b.combine(&new, op);
            b
        }
        // somar ao nada = a nova; subtrair/intersectar com o nada = nada
        (None, SelectOp::Subtract | SelectOp::Intersect) => return None,
        (None, _) => new,
    };
    selection_from_cov(out)
}

/// Fim do arrasto do retângulo/elipse/laço (ou do laço poligonal fechado). Clique sem arrastar com "nova" tira a seleção.
pub(crate) fn finish_select(env: &mut Env, tool: SelTool, start: Vec2, pts: Vec<Vec2>, op: SelectOp, scale: f32) {
    let Some((w, h)) = canvas_wh(env) else { return };
    match tool {
        SelTool::Pen | SelTool::Erase => return finish_brush(env, tool, pts, op, w, h),
        SelTool::Shrink => return finish_shrink(env, pts, op, w, h, scale),
        SelTool::Magnetic => {
            let snapped = magnetic_pts(env, &pts);
            return finish_select(env, SelTool::Lasso, start, snapped, op, scale);
        }
        _ => {}
    }
    let end = pts.last().copied().unwrap_or(start);
    let sel = match tool {
        SelTool::Lasso | SelTool::Polygon => Selection::new(SelMode::Lasso, pts, false),
        SelTool::Ellipse => Selection::new(SelMode::Ellipse, vec![start, end], false),
        _ => Selection::new(SelMode::Rect, vec![start, end], false),
    };
    let tiny = sel.rect_of().is_none_or(|r| r[2] * scale < 3.0 || r[3] * scale < 3.0);
    let shape = sel.shape();
    let (Some(shape), false) = (shape, tiny) else {
        if op == SelectOp::Replace {
            env.ed.selection = None;
        }
        return;
    };
    let Ok(cov) = Cov::from_shape(&shape, w, h, None) else { return };
    env.ed.selection = combine_area(env, cov, op, Some(sel));
    env.ed.sel_state.wand = None;
}

/// Caneta de seleção / apagar seleção: o pincel redondo (diâmetro das opções) pinta ou tira da área atual. A caneta
/// soma; com "subtrair" (Alt) ela apaga, como a borracha de seleção.
fn finish_brush(env: &mut Env, tool: SelTool, pts: Vec<Vec2>, op: SelectOp, w: u32, h: u32) {
    let add = tool == SelTool::Pen && op != SelectOp::Subtract;
    // seleção com transformação pendente: grava a transformação antes de pintar por cima
    if let Some(s) = env.ed.selection.as_mut().filter(|s| !s.is_identity()) {
        s.bake_offset();
    }
    let mut base = env.ed.selection.as_ref().and_then(|s| area_cov(s, w, h)).unwrap_or_else(|| Cov::none(w, h));
    let path: Vec<[f32; 2]> = pts.iter().map(|p| [p.x, p.y]).collect();
    aiai_paint::area::paint_selection(&mut base, &path, env.ed.opts.area.sel_brush.max(1.0) / 2.0, add);
    env.ed.selection = selection_from_cov(base);
    env.ed.sel_state.wand = None;
}

/// Encolher seleção: o laço vira área, e a área encolhe até o desenho das camadas de referência (as mesmas da
/// varinha: a camada em edição, todas ou as de referência).
fn finish_shrink(env: &mut Env, pts: Vec<Vec2>, op: SelectOp, w: u32, h: u32, scale: f32) {
    let sel = Selection::new(SelMode::Lasso, pts, false);
    let tiny = sel.rect_of().is_none_or(|r| r[2] * scale < 3.0 || r[3] * scale < 3.0);
    let (Some(shape), false) = (sel.shape(), tiny) else {
        if op == SelectOp::Replace {
            env.ed.selection = None;
        }
        return;
    };
    let Ok(lasso) = Cov::from_shape(&shape, w, h, None) else { return };
    let img = match super::area_tools::cached_image(env, env.ed.opts.area.wand_ref) {
        Ok(i) => i,
        Err(e) => {
            env.ed.notify(e);
            return;
        }
    };
    let shrunk = aiai_paint::area::shrink_to_drawing(&lasso, &img, 0.1);
    if shrunk.is_empty() {
        env.ed.notify(tr("sel.shrink.empty"));
        if op == SelectOp::Replace {
            env.ed.selection = None;
        }
        return;
    }
    env.ed.selection = combine_area(env, shrunk, op, None);
    env.ed.sel_state.wand = None;
}

/// Pontos do laço magnético grudados na borda (as bordas ficam prontas no clique; sem elas, o laço como foi traçado).
fn magnetic_pts(env: &Env, pts: &[Vec2]) -> Vec<Vec2> {
    match super::area_tools::edges_ready(env) {
        Some((e, w, h)) => {
            let path: Vec<[f32; 2]> = pts.iter().map(|p| [p.x, p.y]).collect();
            aiai_paint::area::magnetic_path(&e, w, h, &path, env.ed.opts.area.sel_magnet).into_iter().map(|q| Vec2::new(q[0], q[1])).collect()
        }
        None => pts.to_vec(),
    }
}

/// Clique do laço poligonal com o gesto aberto: põe um vértice, ou fecha (clique perto do primeiro, duplo clique,
/// botão direito).
pub(crate) fn poly_click(env: &mut Env, c: Vec2, button: egui::PointerButton, scale: f32) {
    let Gesture::PolyLasso { pts, last_click, .. } = &mut env.ed.gesture else { return };
    let near_first = pts.len() >= 3 && (pts[0] - c).length() * scale < 8.0;
    let double = last_click.elapsed().as_millis() < 350 && pts.last().is_some_and(|l| (*l - c).length() * scale < 6.0);
    if button == egui::PointerButton::Secondary || near_first || double {
        finish_poly(env, scale);
        return;
    }
    if button == egui::PointerButton::Primary {
        pts.push(c);
        *last_click = Instant::now();
    }
}

pub(crate) fn finish_poly(env: &mut Env, scale: f32) {
    if let Gesture::PolyLasso { pts, op, .. } = std::mem::replace(&mut env.ed.gesture, Gesture::None) {
        if pts.len() >= 3 {
            finish_select(env, SelTool::Polygon, pts[0], pts, op, scale);
        }
    }
}

/// Clique da ferramenta Seleção no canvas (botão principal).
pub(crate) fn press(env: &mut Env, c: Vec2, mods: egui::Modifiers, scale: f32) {
    let op = op_for(mods, env.ed.opts.select_op);
    let tool = env.ed.opts.select_mode;
    match tool {
        SelTool::Wand => wand_click(env, c, op),
        SelTool::Polygon => env.ed.gesture = Gesture::PolyLasso { pts: vec![c], op, last_click: Instant::now() },
        SelTool::Magnetic => {
            // as bordas da imagem de referência ficam prontas no clique (a prévia gruda enquanto arrasta)
            if let Err(e) = super::area_tools::cached_edges(env, env.ed.opts.area.wand_ref) {
                env.ed.notify(e);
            }
            env.ed.gesture = Gesture::Select { start: c, pts: vec![c], tool, op };
        }
        SelTool::Strokes => {
            let vector = env.ed.active_layer(env.core).and_then(|l| cmds::layer_data(&env.core.project, &l).map(|d| d.kind)) == Some(LayerType::Vector);
            if vector {
                env.ed.gesture = Gesture::StrokePick { start: c, cur: c, op };
            } else {
                env.ed.notify(tr("err.need_vector"));
            }
        }
        _ => env.ed.gesture = Gesture::Select { start: c, pts: vec![c], tool, op },
    }
    let _ = scale;
}

// ------------------------------------------------------------------------------------------------ varinha

/// Imagem que a varinha lê, em px do canvas (com o movimento da camada): a camada ativa renderizada (vetor ou raster),
/// todas as camadas visíveis ou as camadas de referência (as sub-ferramentas da Seleção automática).
fn wand_reference(env: &Env, cut: &NodeId, layer: &NodeId, r: super::area_tools::WandRef) -> Result<aiai_paint::Image, String> {
    super::area_tools::reference_image(env, cut, layer, r)
}

pub(crate) fn wand_click(env: &mut Env, p: Vec2, op: SelectOp) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let Some(layer) = env.ed.active_layer(env.core) else {
        env.ed.notify(tr("err.no_layer"));
        return;
    };
    let (w, h) = cmds::canvas_size(&env.core.project, &cut);
    if p.x < 0.0 || p.y < 0.0 || p.x >= w as f32 || p.y >= h as f32 {
        if op == SelectOp::Replace {
            env.ed.selection = None;
        }
        return;
    }
    let all = env.ed.opts.area.wand_ref;
    let reference = match wand_reference(env, &cut, &layer, all) {
        Ok(r) => Arc::new(r),
        Err(e) => {
            env.ed.notify(e);
            return;
        }
    };
    let base = if op == SelectOp::Replace { None } else { env.ed.selection.as_ref().filter(|s| s.is_identity()).and_then(|s| area_cov(s, w, h)).map(Arc::new) };
    let mut click = WandClick { p, op, base, reference, sig: (env.core.change_counter, layer, env.ed.frame, all), result: None };
    run_wand(env, &mut click);
    env.ed.sel_state.wand = Some(click);
}

fn run_wand(env: &mut Env, click: &mut WandClick) {
    let o = &env.ed.opts;
    let seed = (click.p.x.max(0.0) as u32, click.p.y.max(0.0) as u32);
    let wo = aiai_paint::area::WandOptions { tolerance: o.wand_tolerance, contiguous: o.wand_contiguous, gap: o.area.wand_gap, scale: o.area.wand_scale, antialias: o.wand_antialias };
    let mut new = aiai_paint::area::wand(&click.reference, seed, &wo);
    let feather = o.sel_feather.round().clamp(0.0, 250.0) as u32;
    if feather > 0 {
        new.feather(feather);
    }
    let out = match (&click.base, click.op) {
        (Some(b), op) => {
            let mut b = (**b).clone();
            b.combine(&new, op);
            Some(b)
        }
        (None, SelectOp::Subtract | SelectOp::Intersect) => None,
        (None, _) => Some(new),
    };
    let sel = out.and_then(selection_from_cov);
    click.result = sel.as_ref().and_then(|s| s.mask.clone());
    env.ed.selection = sel;
}

/// Opção da varinha mudou na barra/painel: refaz o último clique se a seleção ainda é a dele (mesmo documento,
/// camada e quadro).
pub(crate) fn wand_refresh(env: &mut Env) {
    let Some(mut click) = env.ed.sel_state.wand.take() else { return };
    let same_sel = env.ed.selection.as_ref().and_then(|s| s.mask.as_ref()).zip(click.result.as_ref()).is_some_and(|(a, b)| Arc::ptr_eq(a, b)) || (env.ed.selection.is_none() && click.result.is_none());
    let layer = env.ed.active_layer(env.core);
    let same_doc = click.sig.0 == env.core.change_counter && Some(&click.sig.1) == layer.as_ref() && click.sig.2 == env.ed.frame;
    if !(same_sel && same_doc) {
        return;
    }
    let all = env.ed.opts.area.wand_ref;
    if click.sig.3 != all {
        let Some(cut) = env.ed.active_cut(env.core) else { return };
        match wand_reference(env, &cut, &click.sig.1, all) {
            Ok(r) => {
                click.reference = Arc::new(r);
                click.sig.3 = all;
            }
            Err(e) => {
                env.ed.notify(e);
                return;
            }
        }
    }
    run_wand(env, &mut click);
    env.ed.sel_state.wand = Some(click);
}

// ------------------------------------------------------------------------------------------------ traços

/// Traço da camada vetorial ativa sob o ponto `c` (px do canvas), com tolerância `tol` (px do canvas).
pub(crate) fn stroke_at(env: &mut Env, c: Vec2, tol: f32) -> Option<Id> {
    let cut = env.ed.active_cut(env.core)?;
    let layer = env.ed.active_layer(env.core)?;
    let l = cmds::layer_data(&env.core.project, &layer)?;
    if l.kind != LayerType::Vector || l.locked {
        return None;
    }
    let cel = cmds::exposed_cel(&env.core.project, &cut, &layer, env.ed.frame)?;
    let fwd = cmds::layer_placement(&env.core.project, &cut, &layer, env.ed.frame);
    let data = cmds::cel_data(&env.core.project, &cel)?;
    stroke_near(&data.strokes, fwd, c, tol)
}

/// O traço mais perto de `c` (px do canvas) até `tol`, medido na curva desenhada levada ao canvas; o de cima ganha no
/// empate (o último desenhado fica na frente).
pub(crate) fn stroke_near(strokes: &[Stroke], fwd: Option<Affine>, c: Vec2, tol: f32) -> Option<Id> {
    let k = fwd.map(|a| a.0[0].hypot(a.0[3])).unwrap_or(1.0);
    strokes
        .iter()
        .rev()
        .map(|s| (s.id.clone(), aiai_paint::sample_stroke(s).iter().map(|q| ((to_canvas(fwd, q.x, q.y) - c).length() - q.r * k).max(0.0)).fold(f32::MAX, f32::min)))
        .filter(|(_, d)| *d <= tol)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

fn to_canvas(fwd: Option<Affine>, x: f32, y: f32) -> Vec2 {
    match fwd {
        Some(a) => {
            let (x, y) = a.apply(x, y);
            Vec2::new(x, y)
        }
        None => Vec2::new(x, y),
    }
}

/// Escolhe estes traços da camada ativa (seleção de objetos); lista vazia = sem seleção.
pub(crate) fn select_strokes(env: &mut Env, ids: Vec<Id>) {
    if ids.is_empty() {
        env.ed.selection = None;
        return;
    }
    let (Some(cut), Some(layer)) = (env.ed.active_cut(env.core), env.ed.active_layer(env.core)) else { return };
    let Some(cel) = cmds::exposed_cel(&env.core.project, &cut, &layer, env.ed.frame) else { return };
    let fwd = cmds::layer_placement(&env.core.project, &cut, &layer, env.ed.frame);
    let Some(data) = cmds::cel_data(&env.core.project, &cel) else { return };
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for s in data.strokes.iter().filter(|s| ids.contains(&s.id)) {
        for q in aiai_paint::sample_stroke(s) {
            for (dx, dy) in [(-q.r, -q.r), (q.r, q.r), (-q.r, q.r), (q.r, -q.r)] {
                let p = to_canvas(fwd, q.x + dx, q.y + dy);
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        env.ed.selection = None;
        return;
    }
    let mut sel = Selection::new(SelMode::Rect, vec![Vec2::new(x0, y0), Vec2::new(x1.max(x0 + 1.0), y1.max(y0 + 1.0))], false);
    sel.strokes = ids;
    sel.strokes_layer = Some(layer);
    env.ed.selection = Some(sel);
}

/// Traços que a caixa `a`–`b` (px do canvas) toca.
pub(crate) fn box_hits(env: &mut Env, a: Vec2, b: Vec2) -> Vec<Id> {
    let (Some(cut), Some(layer)) = (env.ed.active_cut(env.core), env.ed.active_layer(env.core)) else { return Vec::new() };
    let Some(cel) = cmds::exposed_cel(&env.core.project, &cut, &layer, env.ed.frame) else { return Vec::new() };
    let fwd = cmds::layer_placement(&env.core.project, &cut, &layer, env.ed.frame);
    let Some(data) = cmds::cel_data(&env.core.project, &cel) else { return Vec::new() };
    strokes_touching_box(&data.strokes, fwd, a, b)
}

pub(crate) fn strokes_touching_box(strokes: &[Stroke], fwd: Option<Affine>, a: Vec2, b: Vec2) -> Vec<Id> {
    let (x0, y0, x1, y1) = (a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y));
    strokes
        .iter()
        .filter(|s| {
            aiai_paint::sample_stroke(s).iter().any(|q| {
                let p = to_canvas(fwd, q.x, q.y);
                p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1
            })
        })
        .map(|s| s.id.clone())
        .collect()
}

/// Combina a lista de traços atual com `hit` pela operação.
pub(crate) fn combine_ids(cur: &[Id], hit: &[Id], op: SelectOp) -> Vec<Id> {
    match op {
        SelectOp::Replace => hit.to_vec(),
        SelectOp::Add => cur.iter().chain(hit.iter().filter(|h| !cur.contains(h))).cloned().collect(),
        SelectOp::Subtract => cur.iter().filter(|c| !hit.contains(c)).cloned().collect(),
        SelectOp::Intersect => cur.iter().filter(|c| hit.contains(c)).cloned().collect(),
    }
}

fn current_ids(env: &mut Env) -> Vec<Id> {
    let layer = env.ed.active_layer(env.core);
    env.ed.selection.as_ref().filter(|s| !s.strokes.is_empty() && s.strokes_layer == layer).map(|s| s.strokes.clone()).unwrap_or_default()
}

/// Fim do gesto de Selecionar traços: clique = o traço sob o cursor (no vazio, com "nova", tira a seleção);
/// arrasto = os traços que a caixa toca.
pub(crate) fn strokes_release(env: &mut Env, start: Vec2, end: Vec2, op: SelectOp, scale: f32) {
    let hit = if (end - start).length() * scale < 4.0 {
        match stroke_at(env, start, 6.0 / scale.max(1e-3)) {
            Some(id) => vec![id],
            None => {
                if op == SelectOp::Replace {
                    env.ed.selection = None;
                }
                return;
            }
        }
    } else {
        box_hits(env, start, end)
    };
    let cur = current_ids(env);
    let next = combine_ids(&cur, &hit, op);
    select_strokes(env, next);
    env.ed.sel_state.wand = None;
}

/// Mover com "Seleção automática" (desligada por padrão): o clique num traço da camada vetorial escolhe só ele antes de
/// arrastar (se já não fazia parte da seleção); fora de qualquer traço e fora da seleção, tira a seleção (o arrasto
/// leva a camada inteira).
pub(crate) fn auto_pick(env: &mut Env, c: Vec2, tol: f32) {
    let vector = env.ed.active_layer(env.core).and_then(|l| cmds::layer_data(&env.core.project, &l).map(|d| d.kind)) == Some(LayerType::Vector);
    if !vector {
        return;
    }
    match stroke_at(env, c, tol) {
        Some(id) => {
            if !current_ids(env).contains(&id) {
                select_strokes(env, vec![id]);
            }
        }
        None => {
            if !env.ed.selection.as_ref().is_some_and(|s| !s.whole && s.contains_mapped(c)) {
                env.ed.selection = None;
            }
        }
    }
}

/// Ctrl+A: tudo da camada ativa (vetor: todos os traços; raster: a caixa do conteúdo).
pub(crate) fn select_all(env: &mut Env) {
    let (Some(cut), Some(layer)) = (env.ed.active_cut(env.core), env.ed.active_layer(env.core)) else { return };
    let Some(l) = cmds::layer_data(&env.core.project, &layer).cloned() else { return };
    env.ed.sel_state.wand = None;
    match l.kind {
        LayerType::Vector => {
            let ids = cmds::exposed_cel(&env.core.project, &cut, &layer, env.ed.frame).and_then(|c| cmds::cel_data(&env.core.project, &c)).map(|d| d.strokes.iter().map(|s| s.id.clone()).collect()).unwrap_or_default();
            select_strokes(env, ids);
        }
        LayerType::Raster => {
            if let Some(r) = raster_content_box(env, &cut, &layer) {
                env.ed.selection = Some(Selection::new(SelMode::Rect, vec![Vec2::new(r[0], r[1]), Vec2::new(r[0] + r[2], r[1] + r[3])], false));
            }
        }
        _ => {}
    }
}

/// Caixa (x, y, w, h) do que a camada raster tem pintado no quadro, em px do canvas.
fn raster_content_box(env: &Env, cut: &NodeId, layer: &NodeId) -> Option<[f32; 4]> {
    let opts = aiai_render::RenderOptions { include_guides: false, include_reference: false, only_layers: Some(vec![layer.clone()]), background: None, camera: false, layer_motion: true };
    let img = aiai_render::render_frame(&env.core.project, &env.core.blobs, cut, env.ed.frame, &opts).ok()?.image;
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..img.height {
        for x in 0..img.width {
            if img.data[(y * img.width + x) as usize][3] > 0.004 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    (x0 < x1).then(|| [x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32])
}

/// Selecionar a cor atual (menu Selecionar, auditoria 2.12): todos os pixels da referência da varinha (a camada ou o
/// que aparece, como na varinha) com a cor ativa, dentro da tolerância da varinha (a faixa de cor do Clip Studio).
pub(crate) fn select_color(env: &mut Env) {
    let (Some(cut), Some(layer)) = (env.ed.active_cut(env.core), env.ed.active_layer(env.core)) else { return };
    let (w, h) = cmds::canvas_size(&env.core.project, &cut);
    let reference = match wand_reference(env, &cut, &layer, env.ed.opts.area.wand_ref) {
        Ok(r) => r,
        Err(e) => {
            env.ed.notify(e);
            return;
        }
    };
    let c = cmds::resolve_color(&env.core.project, &env.ed.color);
    let shape = aiai_paint::select::SelectShape::ColorRange { color: [c.0[0], c.0[1], c.0[2]], tolerance: (env.ed.opts.wand_tolerance / 255.0).clamp(0.0, 1.0) };
    env.ed.sel_state.wand = None;
    match aiai_paint::select::Selection::from_shape(&shape, w, h, Some(&reference)) {
        Ok(cov) => match selection_from_cov(cov) {
            Some(sel) => env.ed.selection = Some(sel),
            None => {
                env.ed.selection = None;
                env.ed.notify(tr("sel.color_none"));
            }
        },
        Err(e) => env.ed.notify(e),
    }
}

/// Ctrl+Shift+I: área = o resto do canvas; traços = os outros traços da camada.
pub(crate) fn invert(env: &mut Env) {
    let Some((w, h)) = canvas_wh(env) else { return };
    let Some(sel) = env.ed.selection.clone() else { return };
    env.ed.sel_state.wand = None;
    if !sel.strokes.is_empty() {
        let (Some(cut), Some(layer)) = (env.ed.active_cut(env.core), env.ed.active_layer(env.core)) else { return };
        let all: Vec<Id> = cmds::exposed_cel(&env.core.project, &cut, &layer, env.ed.frame).and_then(|c| cmds::cel_data(&env.core.project, &c)).map(|d| d.strokes.iter().map(|s| s.id.clone()).collect()).unwrap_or_default();
        let rest = combine_ids(&all, &sel.strokes, SelectOp::Subtract);
        select_strokes(env, rest);
        return;
    }
    let mut s = sel;
    if !s.is_identity() {
        s.bake_offset();
    }
    let Some(mut c) = area_cov(&s, w, h) else { return };
    c.invert();
    env.ed.selection = selection_from_cov(c);
}

// ------------------------------------------------------------------------------------------------ mover

fn sel_key(sel: &Selection) -> String {
    format!("{:?}{:?}{:?}{:?}", sel.mode, sel.pts, sel.mask.as_ref().map(Arc::as_ptr), sel.strokes)
}

/// Chave da seleção para as imagens do Mover ao vivo (sem o deslocamento pendente).
pub(crate) fn move_sel_key(sel: Option<&Selection>) -> String {
    match sel.filter(|s| !s.whole) {
        Some(s) => sel_key(s),
        None => "whole".to_owned(),
    }
}

/// Traços que a área leva (camada vetorial ativa): os que têm a maior parte da curva desenhada dentro da área, medida
/// no canvas (com o movimento da camada). Mesma lista no commit, no Mover ao vivo e na prévia.
pub(crate) fn area_stroke_ids(env: &mut Env, sel: &Selection) -> Arc<Vec<Id>> {
    let empty = || Arc::new(Vec::new());
    let (Some(cut), Some(layer)) = (env.ed.active_cut(env.core), env.ed.active_layer(env.core)) else { return empty() };
    let key = format!("{}|{}|{}|{}", env.core.change_counter, layer.0, env.ed.frame, sel_key(sel));
    if let Some((k, ids)) = &env.ed.sel_state.area_ids {
        if *k == key {
            return ids.clone();
        }
    }
    let (w, h) = cmds::canvas_size(&env.core.project, &cut);
    let fwd = cmds::layer_placement(&env.core.project, &cut, &layer, env.ed.frame);
    let ids = match (cmds::exposed_cel(&env.core.project, &cut, &layer, env.ed.frame).and_then(|c| cmds::cel_data(&env.core.project, &c).cloned()), area_cov(sel, w, h)) {
        (Some(data), Some(cov)) => Arc::new(strokes_in_area(&data.strokes, &cov, fwd)),
        _ => empty(),
    };
    env.ed.sel_state.area_ids = Some((key, ids.clone()));
    ids
}

/// Traços cuja curva desenhada (amostras do motor, levadas ao canvas por `fwd`) fica na maior parte dentro de `cov`.
pub(crate) fn strokes_in_area(strokes: &[Stroke], cov: &Cov, fwd: Option<Affine>) -> Vec<Id> {
    let (w, h) = (cov.width as i64, cov.height as i64);
    let inside = |p: Vec2| {
        let (ix, iy) = (p.x.floor() as i64, p.y.floor() as i64);
        ix >= 0 && iy >= 0 && ix < w && iy < h && cov.cov[(iy * w + ix) as usize] >= 0.5
    };
    strokes
        .iter()
        .filter(|s| {
            let q = aiai_paint::sample_stroke(s);
            !q.is_empty() && q.iter().filter(|q| inside(to_canvas(fwd, q.x, q.y))).count() * 2 >= q.len()
        })
        .map(|s| s.id.clone())
        .collect()
}

/// a∘b (aplica `b`, depois `a`).
fn mul(a: &Affine, b: &Affine) -> Affine {
    let (a, b) = (a.0, b.0);
    Affine([a[0] * b[0] + a[1] * b[3], a[0] * b[1] + a[1] * b[4], a[0] * b[2] + a[1] * b[5] + a[2], a[3] * b[0] + a[4] * b[3], a[3] * b[1] + a[4] * b[4], a[3] * b[2] + a[4] * b[5] + a[5]])
}

/// Cobertura no espaço do cel: o pixel (x, y) do cel cai em `place(x, y)` no canvas.
fn cov_to_cel(c: &Cov, place: &Affine) -> Cov {
    let (w, h) = (c.width as i64, c.height as i64);
    let mut out = Cov::none(c.width, c.height);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = place.apply(x as f32 + 0.5, y as f32 + 0.5);
            let (ix, iy) = (sx.floor() as i64, sy.floor() as i64);
            if ix >= 0 && iy >= 0 && ix < w && iy < h {
                out.cov[(y * w + x) as usize] = c.cov[(iy * w + ix) as usize];
            }
        }
    }
    out
}

/// O que o Mover vai gravar para `sel` (com a transformação pendente) na camada: traços (vetor) ou pixels (raster),
/// tudo levado do canvas para o espaço do cel da camada.
pub(crate) struct MovePlan {
    pub deform: aiai_tools::transform::Deform,
    pub strokes: Vec<Id>,
    pub mask: Option<Cov>,
}

pub(crate) fn plan_move(env: &mut Env, sel: &Selection, cut: &NodeId, layer: &NodeId, frame: u32, kind: LayerType) -> Result<MovePlan, String> {
    let (w, h) = cmds::canvas_size(&env.core.project, cut);
    let place = cmds::layer_placement(&env.core.project, cut, layer, frame);
    // distorção livre: a caixa vai para os cantos novos (homografia), no espaço do cel da camada
    let distort = match (sel.corners, sel.box_corners()) {
        (Some(to), Some(from)) => {
            let to_cel = |p: Vec2| -> Result<[f32; 2], String> {
                match &place {
                    None => Ok([p.x, p.y]),
                    Some(pl) => {
                        let inv = pl.inverse().ok_or_else(|| tr("err.move_layer").to_owned())?;
                        let (x, y) = inv.apply(p.x, p.y);
                        Ok([x, y])
                    }
                }
            };
            Some(aiai_tools::transform::Deform::Perspective {
                from: [to_cel(from[0])?, to_cel(from[1])?, to_cel(from[2])?, to_cel(from[3])?],
                to: [to_cel(to[0])?, to_cel(to[1])?, to_cel(to[2])?, to_cel(to[3])?],
            })
        }
        _ => None,
    };
    // malha: a grade da caixa vai para os vértices arrastados, levada ao espaço do cel
    let mesh = match sel.mesh_warp() {
        Some(m) => {
            let to_cel = |x: f32, y: f32| -> Result<[f32; 2], String> {
                match &place {
                    None => Ok([x, y]),
                    Some(pl) => {
                        let inv = pl.inverse().ok_or_else(|| tr("err.move_layer").to_owned())?;
                        let (a, b) = inv.apply(x, y);
                        Ok([a, b])
                    }
                }
            };
            let a = to_cel(m.rect[0], m.rect[1])?;
            let b = to_cel(m.rect[0] + m.rect[2], m.rect[1] + m.rect[3])?;
            let rect = [a[0].min(b[0]), a[1].min(b[1]), (b[0] - a[0]).abs(), (b[1] - a[1]).abs()];
            let points = m.points.iter().map(|p| to_cel(p[0], p[1])).collect::<Result<Vec<_>, _>>()?;
            Some(aiai_tools::transform::Deform::Mesh(aiai_paint::warp::MeshWarp { rect, cols: m.cols, rows: m.rows, points }))
        }
        None => None,
    };
    // liquify: os passos levados ao espaço do cel (centro, direção e tamanho pela inversa do lugar da camada)
    let liquify = if sel.liquify.is_empty() {
        None
    } else {
        let mut steps = sel.liquify.clone();
        if let Some(pl) = &place {
            let inv = pl.inverse().ok_or_else(|| tr("err.move_layer").to_owned())?;
            for s in &mut steps {
                let (cx, cy) = inv.apply(s.center[0], s.center[1]);
                let (ux, uy) = inv.apply(s.center[0] + 1.0, s.center[1]);
                let k = ((ux - cx).powi(2) + (uy - cy).powi(2)).sqrt();
                let (dx, dy) = inv.apply(s.center[0] + s.direction[0], s.center[1] + s.direction[1]);
                s.center = [cx, cy];
                s.direction = [dx - cx, dy - cy];
                s.radius *= k;
                if s.kind == aiai_paint::warp::LiquifyKind::Push {
                    s.strength *= k;
                }
            }
        }
        Some(aiai_tools::transform::Deform::LiquifyPath { steps })
    };
    let deform = match &place {
        _ if liquify.is_some() => liquify.expect("liquify"),
        _ if mesh.is_some() => mesh.expect("malha"),
        _ if distort.is_some() => distort.expect("distorção"),
        None => {
            let c = sel.center();
            aiai_tools::transform::Deform::Affine { translate: [sel.offset.x, sel.offset.y], scale: [sel.scale.x, sel.scale.y], rotate: sel.rotate, shear: [0.0, 0.0], pivot: [c.x, c.y] }
        }
        Some(pl) => {
            let inv = pl.inverse().ok_or_else(|| tr("err.move_layer").to_owned())?;
            aiai_tools::transform::affine_deform(mul(&mul(&inv, &sel.affine()), pl).0)
        }
    };
    match kind {
        LayerType::Vector => {
            let strokes = if !sel.strokes.is_empty() {
                sel.strokes.clone()
            } else if sel.whole {
                Vec::new()
            } else {
                let ids = area_stroke_ids(env, sel);
                if ids.is_empty() {
                    return Err(tr("msg.nothing_to_move").to_owned());
                }
                ids.to_vec()
            };
            Ok(MovePlan { deform, strokes, mask: None })
        }
        _ => {
            let mask = if sel.whole { None } else { area_cov(sel, w, h).map(|c| place.as_ref().map(|pl| cov_to_cel(&c, pl)).unwrap_or(c)) };
            Ok(MovePlan { deform, strokes: Vec::new(), mask })
        }
    }
}

// ------------------------------------------------------------------------------------------------ desenho

/// Contornos da seleção (px do canvas, antes da transformação pendente).
fn outlines(sel: &Selection) -> Vec<Vec<Vec2>> {
    if let Some(m) = &sel.mask {
        return m.outline.clone();
    }
    let Some(r) = sel.rect_of() else { return Vec::new() };
    match sel.mode {
        SelMode::Lasso => vec![sel.pts.clone()],
        SelMode::Ellipse => {
            let (cx, cy, rx, ry) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0, r[2] / 2.0, r[3] / 2.0);
            vec![(0..96)
                .map(|i| {
                    let a = i as f32 / 96.0 * std::f32::consts::TAU;
                    Vec2::new(cx + rx * a.cos(), cy + ry * a.sin())
                })
                .collect()]
        }
        _ => vec![vec![Vec2::new(r[0], r[1]), Vec2::new(r[0] + r[2], r[1]), Vec2::new(r[0] + r[2], r[1] + r[3]), Vec2::new(r[0], r[1] + r[3])]],
    }
}

/// Formigas: contorno branco (ou da cor das Preferências) com traços pretos que andam (como o Photoshop).
pub(crate) fn ants(painter: &egui::Painter, pts: &[Pos2], closed: bool, phase: f32) {
    if pts.len() < 2 {
        return;
    }
    let mut v = pts.to_vec();
    if closed {
        v.push(pts[0]);
    }
    // Preferências > Camada e seleção: cor das formigas (padrão: branco com tracejado preto)
    let base = crate::behavior_prefs::current(painter.ctx()).sel_color.map(crate::behavior_prefs::rgb).unwrap_or(Color32::WHITE);
    painter.add(Shape::line(v.clone(), EStroke::new(1.3, base)));
    painter.extend(Shape::dashed_line_with_offset(&v, EStroke::new(1.3, Color32::BLACK), &[4.0], &[4.0], phase));
}

/// Formigas da seleção atual (seguem a forma de verdade: máscara, laço, elipse, retângulo) com a transformação pendente.
pub(crate) fn paint_outline(painter: &egui::Painter, xf: &Xf, sel: &Selection) {
    let phase = (painter.ctx().input(|i| i.time) * 12.0 % 8.0) as f32;
    let mut total = 0usize;
    for l in outlines(sel) {
        let pts: Vec<Pos2> = l.iter().map(|p| xf.to_screen(sel.map(*p))).collect();
        // laço menor que 2 px na tela (zoom longe): não pesa o quadro
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in &pts {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        if x1 - x0 < 2.0 && y1 - y0 < 2.0 {
            continue;
        }
        total += pts.len();
        if total > 200_000 {
            painter.add(Shape::closed_line(pts, EStroke::new(1.0, Color32::WHITE)));
        } else {
            ants(painter, &pts, true, phase);
        }
    }
    painter.ctx().request_repaint_after(std::time::Duration::from_millis(90));
}

/// Destaque dos traços escolhidos e, durante o arrasto do Mover antes das imagens ao vivo ficarem prontas, a prévia
/// dos traços que andam: a curva desenhada (não os pontos de controle) no lugar do canvas, transformada.
pub(crate) fn paint_strokes(env: &mut Env, painter: &egui::Painter, xf: &Xf, cut: &NodeId, sel: &Selection, moving_preview: bool) {
    let Some(layer) = env.ed.active_layer(env.core) else { return };
    if cmds::layer_data(&env.core.project, &layer).is_none_or(|l| l.kind != LayerType::Vector) {
        return;
    }
    let ids: Vec<Id> = if !sel.strokes.is_empty() {
        sel.strokes.clone()
    } else if moving_preview && !sel.whole {
        area_stroke_ids(env, sel).to_vec()
    } else if moving_preview {
        Vec::new()
    } else {
        return;
    };
    let all = sel.whole && moving_preview;
    let Some(cel) = cmds::exposed_cel(&env.core.project, cut, &layer, env.ed.frame) else { return };
    let fwd = cmds::layer_placement(&env.core.project, cut, &layer, env.ed.frame);
    let Some(data) = cmds::cel_data(&env.core.project, &cel) else { return };
    let accent = env.t.accent;
    let k = fwd.map(|a| a.0[0].hypot(a.0[3])).unwrap_or(1.0) * sel.scale.x.abs().max(sel.scale.y.abs());
    for s in data.strokes.iter().filter(|s| all || ids.contains(&s.id)) {
        let q = aiai_paint::sample_stroke(s);
        let pts: Vec<Pos2> = q.iter().step_by(2).chain(q.last()).map(|q| xf.to_screen(sel.map(to_canvas(fwd, q.x, q.y)))).collect();
        if pts.len() < 2 {
            continue;
        }
        if moving_preview {
            let r = q.iter().map(|q| q.r).sum::<f32>() / q.len() as f32;
            painter.add(Shape::line(pts, EStroke::new((2.0 * r * k * xf.scale).max(1.0), accent.gamma_multiply(0.75))));
        } else {
            painter.add(Shape::line(pts, EStroke::new(1.5, accent)));
        }
    }
}

/// Gestos da Seleção em andamento (forma sendo arrastada, laço poligonal, caixa de traços) e o sinal da operação.
pub(crate) fn paint_gesture(env: &Env, painter: &egui::Painter, xf: &Xf) {
    let phase = (painter.ctx().input(|i| i.time) * 12.0 % 8.0) as f32;
    let cursor = env.ed.cursor;
    let op = match &env.ed.gesture {
        Gesture::Select { start, pts, tool, op } => {
            let end = pts.last().copied().unwrap_or(*start);
            if matches!(tool, SelTool::Pen | SelTool::Erase) {
                // o rastro do pincel com o diâmetro de verdade
                let v: Vec<Pos2> = pts.iter().map(|p| xf.to_screen(*p)).collect();
                let wpx = (env.ed.opts.area.sel_brush * xf.scale).max(1.0);
                let col = if *tool == SelTool::Erase || *op == SelectOp::Subtract { Color32::from_rgba_unmultiplied(230, 70, 70, 90) } else { env.t.accent.gamma_multiply(0.45) };
                if v.len() == 1 {
                    painter.circle_filled(v[0], wpx / 2.0, col);
                } else {
                    painter.add(Shape::line(v, EStroke::new(wpx, col)));
                }
                return;
            }
            let path = if *tool == SelTool::Magnetic { magnetic_pts(env, pts) } else { pts.clone() };
            let s = match tool {
                SelTool::Lasso | SelTool::Shrink | SelTool::Magnetic => Selection::new(SelMode::Lasso, path, false),
                SelTool::Ellipse => Selection::new(SelMode::Ellipse, vec![*start, end], false),
                _ => Selection::new(SelMode::Rect, vec![*start, end], false),
            };
            for l in outlines(&s) {
                let v: Vec<Pos2> = l.iter().map(|p| xf.to_screen(*p)).collect();
                ants(painter, &v, true, phase);
            }
            Some(*op)
        }
        Gesture::PolyLasso { pts, op, .. } => {
            let mut v: Vec<Pos2> = pts.iter().map(|p| xf.to_screen(*p)).collect();
            if let Some(c) = cursor {
                v.push(c);
            }
            ants(painter, &v, false, phase);
            for (i, p) in pts.iter().enumerate() {
                let q = xf.to_screen(*p);
                let near = i == 0 && pts.len() >= 3 && cursor.is_some_and(|c| (c - q).length() < 8.0);
                painter.circle(q, if near { 5.0 } else { 2.5 }, Color32::WHITE, EStroke::new(1.0, env.t.accent));
            }
            Some(*op)
        }
        Gesture::StrokePick { start, cur, op } => {
            let r = egui::Rect::from_two_pos(xf.to_screen(*start), xf.to_screen(*cur));
            painter.rect_filled(r, 0.0, env.t.accent.gamma_multiply(0.12));
            ants(painter, &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], true, phase);
            Some(*op)
        }
        _ => None,
    };
    // sinal da operação perto do cursor (+ soma, − subtrai, × intersecta), também antes de apertar, com as teclas
    let op = op.or_else(|| {
        let mods = painter.ctx().input(|i| i.modifiers);
        (matches!(env.ed.gesture, Gesture::None)).then(|| op_for(mods, env.ed.opts.select_op))
    });
    if let (Some(op), Some(c)) = (op, cursor) {
        if op != SelectOp::Replace {
            let p = c + egui::vec2(12.0, 12.0);
            let st = EStroke::new(1.6, Color32::WHITE);
            let sh = EStroke::new(3.2, Color32::from_black_alpha(160));
            let segs: Vec<[Pos2; 2]> = match op {
                SelectOp::Add => vec![[p - egui::vec2(4.0, 0.0), p + egui::vec2(4.0, 0.0)], [p - egui::vec2(0.0, 4.0), p + egui::vec2(0.0, 4.0)]],
                SelectOp::Subtract => vec![[p - egui::vec2(4.0, 0.0), p + egui::vec2(4.0, 0.0)]],
                _ => vec![[p - egui::vec2(3.5, 3.5), p + egui::vec2(3.5, 3.5)], [p + egui::vec2(-3.5, 3.5), p + egui::vec2(3.5, -3.5)]],
            };
            for s in &segs {
                painter.line_segment(*s, sh);
            }
            for s in segs {
                painter.line_segment(s, st);
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------ teclas e opções

/// Teclas da seleção (antes das gerais): Ctrl+Shift+I inverte; com o laço poligonal aberto, Enter fecha e Backspace
/// tira o último vértice.
pub(crate) fn shortcuts(ctx: &egui::Context, env: &mut Env) {
    use egui::{Key, Modifiers};
    let poly = matches!(env.ed.gesture, Gesture::PolyLasso { .. });
    let (inv, enter, back) = ctx.input_mut(|i| (i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::I), poly && i.consume_key(Modifiers::NONE, Key::Enter), poly && i.consume_key(Modifiers::NONE, Key::Backspace)));
    if inv {
        invert(env);
    }
    if back {
        if let Gesture::PolyLasso { pts, .. } = &mut env.ed.gesture {
            if pts.len() > 1 {
                pts.pop();
            }
        }
    }
    if enter {
        finish_poly(env, 1.0);
    }
}

/// Botão de operação (nova / somar / subtrair / intersectar) com o desenho clássico de dois quadrados.
fn op_button(ui: &mut egui::Ui, op: SelectOp, on: bool, t: &crate::theme::Tokens) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 22.0), egui::Sense::click());
    let p = ui.painter_at(rect);
    let bg = if on { t.accent.gamma_multiply(0.35) } else if resp.hovered() { t.bg_widget } else { Color32::TRANSPARENT };
    p.rect_filled(rect, 3.0, bg);
    if on {
        p.rect_stroke(rect, 3.0, EStroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    }
    let ink = t.text;
    let fill = t.text.gamma_multiply(0.75);
    let a = egui::Rect::from_min_size(rect.min + egui::vec2(5.0, 4.0), egui::vec2(10.0, 10.0));
    let b = a.translate(egui::vec2(6.0, 4.0));
    let st = EStroke::new(1.0, ink);
    match op {
        SelectOp::Replace => {
            let c = egui::Rect::from_center_size(rect.center(), egui::vec2(12.0, 12.0));
            p.rect_filled(c, 0.0, fill);
        }
        SelectOp::Add => {
            p.rect_filled(a, 0.0, fill);
            p.rect_filled(b, 0.0, fill);
        }
        SelectOp::Subtract => {
            p.rect_filled(a, 0.0, fill);
            p.rect_filled(b, 0.0, if on { t.accent.gamma_multiply(0.35) } else { t.bg_panel });
            p.rect_stroke(b, 0.0, st, egui::StrokeKind::Inside);
        }
        SelectOp::Intersect => {
            p.rect_stroke(a, 0.0, st, egui::StrokeKind::Inside);
            p.rect_stroke(b, 0.0, st, egui::StrokeKind::Inside);
            p.rect_filled(a.intersect(b), 0.0, fill);
        }
    }
    resp
}

/// Opções da Seleção na barra do canvas (`compact`) e no painel (`panel`: a forma vira item da lista do painel
/// Ferramenta).
pub(crate) fn tool_options(env: &mut Env, ui: &mut egui::Ui, compact: bool, panel: bool) {
    let t = env.t;
    if !panel {
        super::panels::properties::row(ui, &t, compact, "opt.select_mode", |ui| {
            let o = &mut env.ed.opts;
            egui::ComboBox::from_id_salt(("sel_mode", compact)).width(130.0).selected_text(tr(o.select_mode.label_key())).show_ui(ui, |ui| {
                for m in SelTool::ALL {
                    ui.selectable_value(&mut o.select_mode, m, tr(m.label_key()));
                }
            });
        });
    }
    let tool = env.ed.opts.select_mode;
    if tool != SelTool::Strokes {
        // operação: nova / somar (Shift) / subtrair (Alt) / intersectar (Shift+Alt)
        let mut notes = Vec::new();
        super::panels::properties::row(ui, &t, compact, "opt.sel_op", |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (op, key) in [(SelectOp::Replace, "sel.op.replace"), (SelectOp::Add, "sel.op.add"), (SelectOp::Subtract, "sel.op.subtract"), (SelectOp::Intersect, "sel.op.intersect")] {
                let r = op_button(ui, op, env.ed.opts.select_op == op, &t);
                notes.push((format!("opt:sel_op:{}", op_id(op)), r.rect));
                if r.on_hover_text(tr(key)).clicked() {
                    env.ed.opts.select_op = op;
                }
            }
        });
        for (k, r) in notes {
            env.note(k, r);
        }
        super::panels::properties::row(ui, &t, compact, "opt.feather", |ui| {
            ui.add(egui::DragValue::new(&mut env.ed.opts.sel_feather).range(0.0..=250.0).speed(0.25).max_decimals(0).suffix(" px")).on_hover_text(tr("opt.feather.tip"));
        });
    }
    if matches!(tool, SelTool::Pen | SelTool::Erase) {
        let mut note = None;
        super::panels::properties::row(ui, &t, compact, "opt.sel_brush", |ui| {
            note = Some(ui.add(egui::DragValue::new(&mut env.ed.opts.area.sel_brush).range(1.0..=1000.0).speed(0.5).max_decimals(0).suffix(" px")).on_hover_text(tr("opt.sel_brush.tip")).rect);
        });
        if let Some(r) = note {
            env.note("opt:sel_brush", r);
        }
    }
    if tool == SelTool::Magnetic {
        let mut note = None;
        super::panels::properties::row(ui, &t, compact, "opt.sel_magnet", |ui| {
            note = Some(ui.add(egui::DragValue::new(&mut env.ed.opts.area.sel_magnet).range(2.0..=60.0).speed(0.2).max_decimals(0).suffix(" px")).on_hover_text(tr("opt.sel_magnet.tip")).rect);
        });
        if let Some(r) = note {
            env.note("opt:sel_magnet", r);
        }
    }
    if matches!(tool, SelTool::Wand | SelTool::Shrink | SelTool::Magnetic) {
        // a que camadas a varinha / o encolher / o ímã se referem (na lista do painel são as sub-ferramentas da varinha)
        let before = env.ed.opts.area.wand_ref;
        let mut notes = Vec::new();
        super::panels::properties::row(ui, &t, compact, "opt.wand_ref", |ui| {
            for r in super::area_tools::WandRef::ALL {
                let b = ui.selectable_label(env.ed.opts.area.wand_ref == r, tr(r.label_key()));
                notes.push((format!("opt:wand_ref:{}", r.id()), b.rect));
                if b.clicked() {
                    env.ed.opts.area.wand_ref = r;
                }
            }
        });
        for (k, r) in notes {
            env.note(k, r);
        }
        if before != env.ed.opts.area.wand_ref && tool == SelTool::Wand {
            wand_refresh(env);
        }
    }
    if tool == SelTool::Wand {
        let before = (env.ed.opts.wand_tolerance, env.ed.opts.wand_contiguous, env.ed.opts.area.wand_gap, env.ed.opts.area.wand_scale, env.ed.opts.wand_antialias, env.ed.opts.sel_feather);
        let mut note = None;
        super::panels::properties::row(ui, &t, compact, "opt.wand_tolerance", |ui| {
            let r = ui.add(egui::Slider::new(&mut env.ed.opts.wand_tolerance, 0.0..=255.0).step_by(1.0).max_decimals(0)).on_hover_text(tr("opt.wand_tolerance.tip"));
            note = Some(r.rect);
        });
        if let Some(r) = note {
            env.note("opt:wand_tolerance", r);
        }
        // fechar intervalo (a fresta da linha que a seleção não atravessa) e dimensionamento de área
        let mut gap_on = env.ed.opts.area.wand_gap > 0.0;
        let mut rects = Vec::new();
        super::panels::properties::row(ui, &t, compact, "opt.wand_gap", |ui| {
            let r = ui.checkbox(&mut gap_on, "").on_hover_text(tr("opt.wand_gap.tip"));
            rects.push(("opt:wand_gap_on", r.rect));
            if r.changed() {
                env.ed.opts.area.wand_gap = if gap_on { 4.0 } else { 0.0 };
            }
            let r = ui.add_enabled(gap_on, egui::DragValue::new(&mut env.ed.opts.area.wand_gap).range(0.0..=30.0).speed(0.1).max_decimals(1).suffix(" px")).on_hover_text(tr("opt.wand_gap.tip"));
            rects.push(("opt:wand_gap", r.rect));
        });
        super::panels::properties::row(ui, &t, compact, "opt.wand_scale", |ui| {
            let r = ui.add(egui::DragValue::new(&mut env.ed.opts.area.wand_scale).range(-50.0..=50.0).speed(0.1).max_decimals(1).suffix(" px")).on_hover_text(tr("opt.wand_scale.tip"));
            rects.push(("opt:wand_scale", r.rect));
        });
        for (k, r) in rects {
            env.note(k, r);
        }
        for (key, tip, id) in [("opt.wand_contiguous", "opt.wand_contiguous.tip", 0), ("opt.wand_antialias", "opt.wand_antialias.tip", 2)] {
            let o = &mut env.ed.opts;
            let v = match id {
                0 => &mut o.wand_contiguous,
                _ => &mut o.wand_antialias,
            };
            let r = ui.checkbox(v, tr(key));
            env.note(format!("opt:{}", key.trim_start_matches("opt.")), r.rect);
            r.on_hover_text(tr(tip));
        }
        let after = (env.ed.opts.wand_tolerance, env.ed.opts.wand_contiguous, env.ed.opts.area.wand_gap, env.ed.opts.area.wand_scale, env.ed.opts.wand_antialias, env.ed.opts.sel_feather);
        if before != after {
            wand_refresh(env);
        }
    }
    let has = env.ed.selection.as_ref().is_some_and(|s| !s.whole);
    for (key, tip, act) in [("opt.sel_invert", "opt.sel_invert.tip", 0), ("opt.sel_all", "opt.sel_all.tip", 1), ("opt.move_deselect", "opt.move_deselect.tip", 2)] {
        if act != 1 && !has {
            continue;
        }
        let b = ui.button(tr(key));
        env.note(format!("opt:{}", key.trim_start_matches("opt.")), b.rect);
        if b.on_hover_text(tr(tip)).clicked() {
            match act {
                0 => invert(env),
                1 => {
                    env.ed.selection = None;
                    select_all(env);
                }
                _ => {
                    env.ed.selection = None;
                    env.ed.sel_state.wand = None;
                }
            }
        }
    }
    if let Some(n) = env.ed.selection.as_ref().map(|s| s.strokes.len()).filter(|n| *n > 0) {
        if !compact {
            super::ui_kit::dim_label(ui, &t, &format!("{} {n}", tr("opt.move_picked")));
        }
        stroke_group_row(env, ui);
    }
}

/// Grupo de intercalação dos traços escolhidos (auditoria 1.5a): o Intercalar só casa traços do mesmo grupo, e o traço a
/// mais de um grupo nasce do par do grupo (1 para N). Vazio = sem grupo.
pub(crate) fn stroke_group_row(env: &mut Env, ui: &mut egui::Ui) {
    let t = env.t;
    let Some(sel) = env.ed.selection.as_ref().filter(|s| !s.strokes.is_empty()) else { return };
    let ids = sel.strokes.clone();
    let Some(layer) = sel.strokes_layer.clone().or_else(|| env.ed.layer.clone()) else { return };
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let Some(cel) = super::commands::exposed_cel(&env.core.project, &cut, &layer, env.ed.frame) else { return };
    let Some(cd) = super::commands::cel_data(&env.core.project, &cel) else { return };
    let chosen: Vec<Stroke> = cd.strokes.iter().filter(|s| ids.contains(&s.id)).cloned().collect();
    let Some(first) = chosen.first() else { return };
    let current = if chosen.iter().all(|s| s.group == first.group) { first.group.clone() } else { String::new() };
    let key = egui::Id::new(("stroke_group", &cel.0, &ids));
    let mut text: String = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| current.clone());
    let mut apply = false;
    super::ui_kit::dim_label(ui, &t, tr("opt.stroke_group"));
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 70.0).clamp(60.0, 180.0);
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(w).hint_text(tr("opt.stroke_group.none"))).on_hover_text(tr("opt.stroke_group.tip"));
        env.note("opt:stroke_group", r.rect);
        apply |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let b = ui.add_enabled(text.trim() != current, egui::Button::new(tr("opt.stroke_group.apply")));
        env.note("opt:stroke_group_apply", b.rect);
        apply |= b.clicked();
    });
    if apply && text.trim() != current {
        let g = text.trim().to_owned();
        let strokes: Vec<Stroke> = chosen.into_iter().map(|mut s| {
            s.group = g.clone();
            s
        }).collect();
        if let Err(e) = super::commands::run(env.core, Command::ReplaceStrokes { cel, strokes }) {
            env.ed.notify(e);
        }
        ui.data_mut(|d| d.remove::<String>(key));
    } else {
        ui.data_mut(|d| d.insert_temp(key, text));
    }
}

/// Estado da Seleção para o `ui_state` da bridge.
pub(crate) fn state_json(ed: &Editor) -> serde_json::Value {
    let o = &ed.opts;
    serde_json::json!({
        "tool": o.select_mode.id(),
        "op": op_id(o.select_op),
        "feather": o.sel_feather,
        "wand": { "tolerance": o.wand_tolerance, "contiguous": o.wand_contiguous, "refer": o.area.wand_ref.id(), "sample_all": o.area.wand_ref == super::area_tools::WandRef::All, "gap": o.area.wand_gap, "scale": o.area.wand_scale, "antialias": o.wand_antialias },
        "brush": o.area.sel_brush,
        "magnet": o.area.sel_magnet,
        "move_auto_select": o.move_auto,
        "covered_px": ed.selection.as_ref().and_then(|s| s.mask.as_ref()).map(|m| m.cov.iter().filter(|&&v| v >= 0.5).count()),
        "outline_loops": ed.selection.as_ref().and_then(|s| s.mask.as_ref()).map(|m| m.outline.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_paint::select::SelectShape;

    fn line(id: &str, a: (f32, f32), b: (f32, f32)) -> Stroke {
        let p = |x: f32, y: f32| StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None };
        Stroke {
            id: Id(id.into()),
            points: vec![p(a.0, a.1), p(b.0, b.1)],
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
            group: String::new(),
        }
    }

    fn ids(v: &[Id]) -> Vec<&str> {
        v.iter().map(|i| i.0.as_str()).collect()
    }

    #[test]
    fn op_from_modifiers() {
        let m = |shift, alt| egui::Modifiers { shift, alt, ..Default::default() };
        assert_eq!(op_for(m(false, false), SelectOp::Replace), SelectOp::Replace);
        assert_eq!(op_for(m(false, false), SelectOp::Add), SelectOp::Add, "sem tecla vale a barra");
        assert_eq!(op_for(m(true, false), SelectOp::Replace), SelectOp::Add);
        assert_eq!(op_for(m(false, true), SelectOp::Replace), SelectOp::Subtract);
        assert_eq!(op_for(m(true, true), SelectOp::Replace), SelectOp::Intersect);
    }

    #[test]
    fn picking_and_box_select_strokes() {
        let s = vec![line("a", (10.0, 10.0), (50.0, 10.0)), line("b", (10.0, 30.0), (50.0, 30.0)), line("c", (100.0, 100.0), (120.0, 100.0))];
        assert_eq!(stroke_near(&s, None, Vec2::new(30.0, 12.0), 4.0).map(|i| i.0), Some("a".into()));
        assert_eq!(stroke_near(&s, None, Vec2::new(30.0, 20.0), 4.0), None, "no vazio entre os dois");
        assert_eq!(ids(&strokes_touching_box(&s, None, Vec2::new(0.0, 0.0), Vec2::new(60.0, 35.0))), ["a", "b"]);
        assert_eq!(ids(&strokes_touching_box(&s, None, Vec2::new(40.0, 25.0), Vec2::new(45.0, 40.0))), ["b"], "a caixa que só toca um pedaço pega o traço");
        // camada com movimento: o cel (0..) aparece deslocado +200 em x no canvas
        let fwd = Some(Affine([1.0, 0.0, 200.0, 0.0, 1.0, 0.0]));
        assert_eq!(stroke_near(&s, fwd, Vec2::new(30.0, 10.0), 4.0), None, "no lugar do cel não há nada");
        assert_eq!(stroke_near(&s, fwd, Vec2::new(230.0, 10.0), 4.0).map(|i| i.0), Some("a".into()));
        assert_eq!(ids(&strokes_touching_box(&s, fwd, Vec2::new(300.0, 90.0), Vec2::new(330.0, 110.0))), ["c"]);
        // somar / subtrair / intersectar listas de traços
        let (a, b, c) = (Id("a".into()), Id("b".into()), Id("c".into()));
        assert_eq!(ids(&combine_ids(&[a.clone()], &[b.clone()], SelectOp::Add)), ["a", "b"]);
        assert_eq!(ids(&combine_ids(&[a.clone(), b.clone()], &[b.clone()], SelectOp::Subtract)), ["a"]);
        assert_eq!(ids(&combine_ids(&[a.clone(), b.clone()], &[b.clone(), c.clone()], SelectOp::Intersect)), ["b"]);
        assert_eq!(ids(&combine_ids(&[a], &[c], SelectOp::Replace)), ["c"]);
    }

    /// Regressão do bug do Mover em camada vetorial: a área (px do canvas) tem de pegar os traços pelo lugar onde eles
    /// aparecem (cel levado ao canvas pelo movimento da camada), e pela curva desenhada, não pelos pontos crus no espaço
    /// do cel. Com o deslocamento de +200 px, a área em volta de onde o traço "a" aparece pega só ele; a mesma área no
    /// lugar do cel não pega nada.
    #[test]
    fn area_takes_strokes_where_they_are_drawn() {
        let s = vec![line("a", (10.0, 10.0), (50.0, 10.0)), line("b", (10.0, 30.0), (50.0, 30.0))];
        let fwd = Some(Affine([1.0, 0.0, 200.0, 0.0, 1.0, 0.0]));
        let at_canvas = Cov::from_shape(&SelectShape::Rect { rect: [200.0, 0.0, 60.0, 20.0] }, 400, 100, None).unwrap();
        assert_eq!(ids(&strokes_in_area(&s, &at_canvas, fwd)), ["a"]);
        let at_cel = Cov::from_shape(&SelectShape::Rect { rect: [0.0, 0.0, 60.0, 20.0] }, 400, 100, None).unwrap();
        assert!(strokes_in_area(&s, &at_cel, fwd).is_empty(), "a área onde o cel estaria não leva nada");
        // maioria da curva: uma área que pega só a ponta do traço não o leva
        let tip = Cov::from_shape(&SelectShape::Rect { rect: [200.0, 0.0, 15.0, 20.0] }, 400, 100, None).unwrap();
        assert!(strokes_in_area(&s, &tip, fwd).is_empty());
    }

    #[test]
    fn layer_space_move_matches_canvas_move() {
        // camada escalada 2× e deslocada: mover 10 px no canvas = 5 px no cel; o ponto (20, 10) do cel aparece em (40+100, 20)
        let place = Affine([2.0, 0.0, 100.0, 0.0, 2.0, 0.0]);
        let a = Affine([1.0, 0.0, 10.0, 0.0, 1.0, -4.0]);
        let inv = place.inverse().unwrap();
        let d = aiai_tools::transform::affine_deform(mul(&mul(&inv, &a), &place).0);
        let (x, y) = d.forward(20.0, 10.0).unwrap();
        let (cx, cy) = place.apply(x, y);
        assert!((cx - 150.0).abs() < 1e-3 && (cy - 16.0).abs() < 1e-3, "o traço vai para onde o arrasto mostrou: ({cx}, {cy})");
        // máscara levada ao cel: o pixel do cel (5, 5) aparece em (110, 10) no canvas
        let c = Cov::from_shape(&SelectShape::Rect { rect: [108.0, 8.0, 4.0, 4.0] }, 200, 40, None).unwrap();
        let cel = cov_to_cel(&c, &place);
        assert_eq!(cel.cov[5 * 200 + 4], 1.0);
        assert_eq!(cel.cov[5 * 200 + 50], 0.0);
    }

    #[test]
    fn mask_moves_with_the_selection() {
        let mut c = Cov::none(40, 40);
        for y in 5..10 {
            for x in 5..10 {
                c.cov[y * 40 + x] = 1.0;
            }
        }
        let m = SelMask::from_cov(c).unwrap();
        assert_eq!(m.bbox, [5.0, 5.0, 5.0, 5.0]);
        assert_eq!(m.outline.len(), 1);
        let t = m.transformed(&Affine([1.0, 0.0, 10.0, 0.0, 1.0, 3.0])).unwrap();
        assert_eq!(t.bbox, [15.0, 8.0, 5.0, 5.0]);
        assert!(SelMask::from_cov(Cov::none(4, 4)).is_none());
    }
}
