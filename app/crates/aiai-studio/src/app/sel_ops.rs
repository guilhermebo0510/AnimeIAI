//! Operações com a seleção e a camada ativa (S25): limpar (dentro da seleção, ou a camada inteira sem seleção), limpar
//! fora, preencher a seleção com a cor atual, expandir/contrair a área, copiar para uma camada nova, transformar,
//! cancelar e inverter. Servem a barra de comandos (`workbar`), a tecla Delete sobre o canvas e o **lançador da
//! seleção**: a barrinha flutuante de ícones que aparece embaixo de uma seleção ativa (some enquanto se desenha).
//!
//! Camada raster: os pixels do cel exposto são lidos, mascarados pela cobertura da seleção (px do canvas, levada ao cel
//! pela colocação da camada) e gravados como tiles (`SetTiles`, um passo de desfazer). Camada vetorial: limpar tira os
//! traços dentro (ou fora) da área, os mesmos que o Mover levaria (`selection::area_stroke_ids`).

use super::commands::{self as cmds, NodeId};
use super::editor::Gesture;
use super::ui_kit::Env;
use super::workbar::{Cmd, CmdCtx};
use crate::i18n::tr;
use crate::workspace::ToolId;
use aiai_core::command::TileEdit;
use aiai_core::{CelData, Command, Id, LayerData, LayerType, NodeKind, TileEntry};
use aiai_paint::select::Selection as Cov;
use aiai_paint::{Affine, Image};
use egui::{Pos2, Rect, Vec2};

/// O que fazer com cada pixel do cel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PixOp {
    ClearInside,
    ClearOutside,
    /// Cor pré-multiplicada.
    Fill([f32; 4]),
}

/// Aplica `op` aos pixels (pré-multiplicados) de `img` (o cel, no espaço do cel). `cov` = cobertura da seleção em px do
/// canvas (`None` = tudo); `place` leva px do cel para px do canvas (camada com movimento).
pub(crate) fn mask_pixels(img: &mut Image, cov: Option<&Cov>, place: Option<&Affine>, op: PixOp) {
    let w = img.width as usize;
    for y in 0..img.height as usize {
        for x in 0..w {
            let k = match cov {
                None => 1.0,
                Some(c) => {
                    let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let (sx, sy) = place.map(|a| a.apply(cx, cy)).unwrap_or((cx, cy));
                    let (ix, iy) = (sx.floor() as i64, sy.floor() as i64);
                    if ix >= 0 && iy >= 0 && ix < c.width as i64 && iy < c.height as i64 { c.cov[(iy * c.width as i64 + ix) as usize].clamp(0.0, 1.0) } else { 0.0 }
                }
            };
            let px = &mut img.data[y * w + x];
            match op {
                PixOp::ClearInside => px.iter_mut().for_each(|v| *v *= 1.0 - k),
                PixOp::ClearOutside => px.iter_mut().for_each(|v| *v *= k),
                PixOp::Fill(c) => {
                    let a = c[3] * k;
                    if a > 0.0 {
                        for i in 0..4 {
                            px[i] = c[i] * k + px[i] * (1.0 - a);
                        }
                    }
                }
            }
        }
    }
}

/// Edições de tile que mudam algo: as que pintam e as que apagam um tile que existia.
fn changed_tiles(before: &CelData, edits: Vec<TileEdit>) -> Vec<TileEdit> {
    edits.into_iter().filter(|e| e.blob.is_some() || before.tiles.iter().any(|t| t.x == e.x && t.y == e.y)).collect()
}

/// Corte, camada (raster/vetor, destravada), cel exposto e tamanho do canvas; erro vira aviso.
fn target(env: &mut Env) -> Result<(NodeId, NodeId, LayerData, NodeId, (u32, u32)), String> {
    let cut = env.ed.active_cut(env.core).ok_or_else(|| tr("err.no_cut").to_owned())?;
    let layer = env.ed.active_layer(env.core).ok_or_else(|| tr("selop.no_layer").to_owned())?;
    let l = cmds::layer_data(&env.core.project, &layer).cloned().ok_or_else(|| tr("selop.no_layer").to_owned())?;
    if l.locked {
        return Err(tr("selop.locked").to_owned());
    }
    if !matches!(l.kind, LayerType::Raster | LayerType::Vector) {
        return Err(tr("selop.wrong_layer").to_owned());
    }
    let cel = cmds::exposed_cel(&env.core.project, &cut, &layer, env.ed.frame).ok_or_else(|| tr("selop.no_cel").to_owned())?;
    let wh = cmds::canvas_size(&env.core.project, &cut);
    Ok((cut, layer, l, cel, wh))
}

/// Cobertura da área escolhida (sem a transformação pendente), em px do canvas.
fn area(env: &Env, wh: (u32, u32)) -> Option<Cov> {
    let sel = env.ed.selection.as_ref().filter(|s| !s.whole)?;
    let mut s = sel.clone();
    if !s.is_identity() {
        s.bake_offset();
    }
    super::selection::area_cov(&s, wh.0, wh.1)
}

/// Ids dos traços da seleção na camada vetorial: os escolhidos, ou os que a área leva.
fn selected_strokes(env: &mut Env) -> Option<Vec<Id>> {
    let sel = env.ed.selection.clone().filter(|s| !s.whole)?;
    if !sel.strokes.is_empty() {
        return Some(sel.strokes.clone());
    }
    Some(super::selection::area_stroke_ids(env, &sel).to_vec())
}

fn run_labeled(env: &mut Env, cmd: Command, key: &str) -> bool {
    match cmds::run(env.core, Command::Batch { commands: vec![cmd], label: Some(tr(key).to_owned()) }) {
        Ok(_) => true,
        Err(e) => {
            env.ed.notify(e);
            false
        }
    }
}

/// Raster: lê o cel, mascara e grava os tiles que mudaram.
fn raster_op(env: &mut Env, cut: &NodeId, layer: &NodeId, cel: &NodeId, wh: (u32, u32), cov: Option<&Cov>, op: PixOp, key: &str) -> bool {
    let Some(cd) = cmds::cel_data(&env.core.project, cel).cloned() else { return false };
    // sem seleção e limpando: tira todos os tiles (também os que ficaram fora do canvas)
    if cov.is_none() && op == PixOp::ClearInside {
        let tiles: Vec<TileEdit> = cd.tiles.iter().map(|t| TileEdit { x: t.x, y: t.y, blob: None }).collect();
        if tiles.is_empty() {
            env.ed.notify(tr("selop.nothing"));
            return false;
        }
        return run_labeled(env, Command::SetTiles { cel: cel.clone(), tiles }, key);
    }
    let mut img = match aiai_tools::paint::cel_raster(&env.core.blobs, &cd, wh.0, wh.1) {
        Ok(i) => i,
        Err(e) => {
            env.ed.notify(e.to_string());
            return false;
        }
    };
    let place = aiai_tools::points::cel_placement(&env.core.project, cut, layer, env.ed.frame).ok();
    mask_pixels(&mut img, cov, place.as_ref(), op);
    let edits = aiai_tools::paint::tiles_for_region(&mut env.core.blobs, &img, [0, 0, wh.0 as i32, wh.1 as i32]);
    let tiles = changed_tiles(&cd, edits);
    if tiles.is_empty() {
        env.ed.notify(tr("selop.nothing"));
        return false;
    }
    run_labeled(env, Command::SetTiles { cel: cel.clone(), tiles }, key)
}

/// Limpar dentro (`inside`) ou fora da seleção; sem seleção e `inside`, a camada inteira no quadro.
pub(crate) fn clear(env: &mut Env, inside: bool) {
    let (cut, layer, l, cel, wh) = match target(env) {
        Ok(t) => t,
        Err(e) => return env.ed.notify(e),
    };
    let has_sel = env.ed.selection.as_ref().is_some_and(|s| !s.whole);
    if !inside && !has_sel {
        return env.ed.notify(tr("selop.need_selection"));
    }
    let key = if !inside { "selop.clear_outside" } else if has_sel { "selop.clear_selection" } else { "selop.clear_layer" };
    match l.kind {
        LayerType::Raster => {
            let cov = if has_sel { area(env, wh) } else { None };
            if has_sel && cov.is_none() {
                return env.ed.notify(tr("selop.need_area"));
            }
            raster_op(env, &cut, &layer, &cel, wh, cov.as_ref(), if inside { PixOp::ClearInside } else { PixOp::ClearOutside }, key);
        }
        _ => {
            let all: Vec<Id> = cmds::cel_data(&env.core.project, &cel).map(|d| d.strokes.iter().map(|s| s.id.clone()).collect()).unwrap_or_default();
            let picked = if has_sel { selected_strokes(env).unwrap_or_default() } else { all.clone() };
            let ids: Vec<Id> = if inside { picked } else { all.into_iter().filter(|i| !picked.contains(i)).collect() };
            if ids.is_empty() {
                return env.ed.notify(tr("selop.nothing"));
            }
            let ok = run_labeled(env, Command::RemoveStrokes { cel, ids }, key);
            // os traços escolhidos saíram: a escolha também
            if ok && inside && env.ed.selection.as_ref().is_some_and(|s| !s.strokes.is_empty()) {
                env.ed.selection = None;
            }
        }
    }
}

/// Preenche a área com a cor atual (camada raster).
pub(crate) fn fill(env: &mut Env) {
    let (cut, layer, l, cel, wh) = match target(env) {
        Ok(t) => t,
        Err(e) => return env.ed.notify(e),
    };
    if l.kind != LayerType::Raster {
        return env.ed.notify(tr("selop.fill_raster"));
    }
    let Some(cov) = area(env, wh) else { return env.ed.notify(tr("selop.need_area")) };
    let c = cmds::resolve_color(&env.core.project, &env.ed.color).0;
    let pm = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]];
    raster_op(env, &cut, &layer, &cel, wh, Some(&cov), PixOp::Fill(pm), "selop.fill");
}

/// Expande (`px` > 0) ou contrai a área.
pub(crate) fn grow(env: &mut Env, px: f32) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let wh = cmds::canvas_size(&env.core.project, &cut);
    let Some(mut cov) = area(env, wh) else { return env.ed.notify(tr("selop.need_area")) };
    cov.grow(px);
    env.ed.sel_state.wand = None;
    match super::selection::selection_from_cov(cov) {
        Some(s) => env.ed.selection = Some(s),
        None => {
            env.ed.selection = None;
            env.ed.notify(tr("selop.shrunk_away"));
        }
    }
}

/// Copia o que está na seleção para uma camada nova logo acima (raster: os pixels visíveis da camada no canvas; vetor:
/// os traços escolhidos). Um passo de desfazer.
pub(crate) fn copy_to_layer(env: &mut Env) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let Some(layer) = env.ed.active_layer(env.core) else { return env.ed.notify(tr("selop.no_layer")) };
    let Some(l) = cmds::layer_data(&env.core.project, &layer).cloned() else { return };
    let wh = cmds::canvas_size(&env.core.project, &cut);
    let mut cel = CelData::new(1);
    match l.kind {
        LayerType::Raster => {
            let Some(cov) = area(env, wh) else { return env.ed.notify(tr("selop.need_area")) };
            let opts = aiai_render::RenderOptions { include_guides: false, include_reference: false, only_layers: Some(vec![layer.clone()]), background: None, camera: false, layer_motion: true };
            let mut img = match aiai_render::render_frame(&env.core.project, &env.core.blobs, &cut, env.ed.frame, &opts) {
                Ok(o) => o.image,
                Err(e) => return env.ed.notify(e.to_string()),
            };
            mask_pixels(&mut img, Some(&cov), None, PixOp::ClearOutside);
            let edits = aiai_tools::paint::tiles_for_region(&mut env.core.blobs, &img, [0, 0, wh.0 as i32, wh.1 as i32]);
            cel.tiles = edits.into_iter().filter_map(|e| e.blob.map(|blob| TileEntry { x: e.x, y: e.y, blob })).collect();
            if cel.tiles.is_empty() {
                return env.ed.notify(tr("selop.nothing"));
            }
        }
        LayerType::Vector => {
            let Some(src) = cmds::exposed_cel(&env.core.project, &cut, &layer, env.ed.frame) else { return env.ed.notify(tr("selop.no_cel")) };
            let ids = selected_strokes(env).unwrap_or_default();
            let strokes = cmds::cel_data(&env.core.project, &src).map(|d| d.strokes.iter().filter(|s| ids.contains(&s.id)).cloned().collect::<Vec<_>>()).unwrap_or_default();
            if strokes.is_empty() {
                return env.ed.notify(tr("selop.nothing"));
            }
            cel.strokes = strokes
                .into_iter()
                .map(|mut s| {
                    s.id = Id(String::new());
                    s
                })
                .collect();
        }
        _ => return env.ed.notify(tr("selop.wrong_layer")),
    }
    let p = &env.core.project;
    let (parent, index) = match p.node(&layer).and_then(|n| n.parent.clone()) {
        Some(par) => {
            let idx = p.node(&par).and_then(|pn| pn.children.iter().position(|c| *c == layer)).map(|i| i + 1);
            (par, idx)
        }
        None => (cut.clone(), None),
    };
    let copy = p.node(&layer).map(|n| crate::behavior_prefs::copy_name_tr(&n.name, env.ed.beh.prefs.copy_name));
    // o nome da cópia vale como está quando ninguém na pasta o usa (o `unique_layer_name` sempre acrescenta um número)
    let name = match copy {
        Some(c) if !p.node(&parent).is_some_and(|pn| pn.children.iter().filter_map(|k| p.node(k)).any(|k| k.name == c)) => c,
        Some(c) => cmds::unique_layer_name(p, &parent, &c),
        None => cmds::unique_layer_name(p, &parent, tr(cmds::layer_type_key(l.kind))),
    };
    let (id, cel_id) = (env.core.ids.next(), env.core.ids.next());
    let cmds_ = vec![
        cmds::create(id.clone(), Some(parent), index, name, NodeKind::Layer(LayerData::new(l.kind))),
        cmds::create(cel_id, Some(id.clone()), None, "1", NodeKind::Cel(cel)),
    ];
    match cmds::run(env.core, Command::Batch { commands: cmds_, label: Some(tr("selop.copy_to_layer").to_owned()) }) {
        Ok(_) => {
            env.ed.layer = Some(id);
            env.ed.cel_number = None;
            env.ed.selection = None;
        }
        Err(e) => env.ed.notify(e),
    }
}

/// Roda um comando de seleção/camada da barra ou do lançador.
pub(crate) fn run(env: &mut Env, c: Cmd) {
    match c {
        Cmd::Clear => clear(env, true),
        Cmd::ClearOutside => clear(env, false),
        Cmd::FillSelection => fill(env),
        Cmd::Transform => super::tools::free_transform(env),
        Cmd::Deselect => {
            env.ed.selection = None;
            env.ed.sel_state.wand = None;
        }
        Cmd::InvertSelection => super::selection::invert(env),
        Cmd::SelectionBorder => env.ed.workbar.sel_border = !env.ed.workbar.sel_border,
        Cmd::RulerSnap => env.ed.rulers.snap = !env.ed.rulers.snap,
        Cmd::Grow => {
            let px = env.ed.workbar.grow_px;
            grow(env, px)
        }
        Cmd::Shrink => {
            let px = env.ed.workbar.grow_px;
            grow(env, -px)
        }
        Cmd::CopyToLayer => copy_to_layer(env),
        Cmd::New | Cmd::Open | Cmd::Save | Cmd::Undo | Cmd::Redo | Cmd::Filter => {}
    }
}

/// Delete com o cursor no canvas e uma seleção: limpa o que está dentro (no Editar pontos, Delete é do ponto).
pub(crate) fn shortcuts(env: &mut Env, ctx: &egui::Context, canvas: Rect) {
    if *env.tool == ToolId::Points || ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) {
        return;
    }
    let has = env.ed.selection.as_ref().is_some_and(|s| !s.whole);
    let over = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| canvas.contains(p));
    if has && over && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
        clear(env, true);
    }
}

/// Ordem dos botões do lançador.
pub(crate) const LAUNCHER: [Cmd; 9] = [Cmd::Deselect, Cmd::InvertSelection, Cmd::Grow, Cmd::Shrink, Cmd::FillSelection, Cmd::Clear, Cmd::ClearOutside, Cmd::Transform, Cmd::CopyToLayer];

/// Onde o lançador fica: centrado embaixo da caixa da seleção na tela (`sel` = cantos na tela), dentro do canvas; sem
/// espaço embaixo, em cima.
pub(crate) fn launcher_pos(sel: Rect, canvas: Rect, size: Vec2) -> Pos2 {
    let gap = 10.0;
    let mut y = sel.max.y + gap;
    if y + size.y > canvas.max.y - 4.0 {
        y = sel.min.y - gap - size.y;
    }
    let y = y.clamp(canvas.min.y + 4.0, (canvas.max.y - size.y - 4.0).max(canvas.min.y + 4.0));
    let x = (sel.center().x - size.x / 2.0).clamp(canvas.min.x + 4.0, (canvas.max.x - size.x - 4.0).max(canvas.min.x + 4.0));
    Pos2::new(x, y)
}

/// O lançador da seleção: barrinha de ícones embaixo da seleção ativa. Some durante o desenho (gesto em curso ou botão
/// apertado), com a transformação pendente e no Editar pontos.
pub(crate) fn launcher(env: &mut Env, ctx: &egui::Context, xf: &super::editor::Xf, canvas: Rect) {
    let Some(sel) = env.ed.selection.clone().filter(|s| !s.whole) else { return };
    // botão apertado fora do lançador = desenhando (o clique num botão dele não pode sumir com ele no meio)
    let last = env.ed.workbar.launcher_rect.take();
    let pressing_elsewhere = ctx.input(|i| i.pointer.any_down() && i.pointer.press_origin().is_none_or(|p| !last.is_some_and(|r| r.contains(p))));
    if !matches!(env.ed.gesture, Gesture::None) || pressing_elsewhere || *env.tool == ToolId::Points || !sel.is_identity() {
        return;
    }
    let Some(r) = sel.rect_of() else { return };
    let corners = [Vec2::new(r[0], r[1]), Vec2::new(r[0] + r[2], r[1]), Vec2::new(r[0] + r[2], r[1] + r[3]), Vec2::new(r[0], r[1] + r[3])].map(|c| xf.to_screen(c));
    let bb = Rect::from_points(&corners);
    if !bb.intersects(canvas) {
        return;
    }
    let btn = 24.0;
    let size = Vec2::new(LAUNCHER.len() as f32 * (btn + 2.0) + 10.0, btn + 10.0);
    let pos = launcher_pos(bb, canvas, size);
    let c = CmdCtx::of(env.core, env.ed);
    let t = env.t;
    let mut run_cmd = None;
    let area = egui::Area::new(egui::Id::new("aiai_sel_launcher")).order(egui::Order::Foreground).fixed_pos(pos).constrain(false).show(ctx, |ui| {
        egui::Frame::NONE.fill(t.bg_panel).stroke(egui::Stroke::new(1.0, t.border)).corner_radius(6.0).inner_margin(egui::Margin::same(5)).shadow(egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: egui::Color32::from_black_alpha(70) }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let keys = env.ed.keys.clone();
                for cmd in LAUNCHER {
                    let r = super::workbar::bar_button(env.icons, ui, &t, cmd.icon(), &cmd.tip(&keys), false, cmd.available(&c), btn);
                    env.notes.push((format!("launcher:{}", cmd.id()), r.rect));
                    if r.clicked() {
                        run_cmd = Some(cmd);
                    }
                }
            });
        });
    });
    env.note("launcher", area.response.rect);
    env.ed.workbar.launcher_rect = Some(area.response.rect);
    if let Some(cmd) = run_cmd {
        run(env, cmd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32, c: [f32; 4]) -> Image {
        Image { width: w, height: h, data: vec![c; (w * h) as usize] }
    }

    fn left_half(w: u32, h: u32) -> Cov {
        let mut c = Cov::none(w, h);
        for y in 0..h {
            for x in 0..w / 2 {
                c.cov[(y * w + x) as usize] = 1.0;
            }
        }
        c
    }

    #[test]
    fn clearing_and_filling_follow_the_selection_coverage() {
        let red = [1.0, 0.0, 0.0, 1.0];
        let cov = left_half(8, 2);
        let mut a = img(8, 2, red);
        mask_pixels(&mut a, Some(&cov), None, PixOp::ClearInside);
        assert_eq!(a.data[0], [0.0; 4]);
        assert_eq!(a.data[7], red);
        let mut b = img(8, 2, red);
        mask_pixels(&mut b, Some(&cov), None, PixOp::ClearOutside);
        assert_eq!((b.data[0], b.data[7]), (red, [0.0; 4]));
        // preencher: cor por cima dentro, nada fora; meia opacidade mistura (pré-multiplicada)
        let mut f = img(8, 2, [0.0; 4]);
        mask_pixels(&mut f, Some(&cov), None, PixOp::Fill([0.0, 0.5, 0.0, 0.5]));
        assert_eq!((f.data[0], f.data[7]), ([0.0, 0.5, 0.0, 0.5], [0.0; 4]));
        let mut g = img(8, 2, red);
        mask_pixels(&mut g, Some(&cov), None, PixOp::Fill([0.0, 0.5, 0.0, 0.5]));
        assert_eq!(g.data[0], [0.5, 0.5, 0.0, 1.0]);
        // sem seleção: tudo
        let mut h = img(8, 2, red);
        mask_pixels(&mut h, None, None, PixOp::ClearInside);
        assert!(h.data.iter().all(|p| *p == [0.0; 4]));
    }

    #[test]
    fn the_mask_goes_through_the_layer_placement() {
        // camada deslocada 4 px para a direita: o pixel 0 do cel está no px 4 do canvas (fora da metade esquerda)
        let cov = left_half(8, 1);
        let place = Affine::from_trs(4.0, 0.0, 1.0, 0.0, [0.0, 0.0]);
        let mut a = img(8, 1, [1.0; 4]);
        mask_pixels(&mut a, Some(&cov), Some(&place), PixOp::ClearInside);
        assert_eq!(a.data[0], [1.0; 4], "cel 0 → canvas 4: fora da área");
        assert_eq!(a.data[7], [1.0; 4], "cel 7 → canvas 11: fora do canvas");
        let mut b = img(8, 1, [1.0; 4]);
        mask_pixels(&mut b, Some(&cov), Some(&Affine::from_trs(-4.0, 0.0, 1.0, 0.0, [0.0, 0.0])), PixOp::ClearInside);
        assert_eq!(b.data[4], [0.0; 4], "cel 4 → canvas 0: dentro");
    }

    #[test]
    fn launcher_sits_under_the_selection_and_stays_inside_the_canvas() {
        let canvas = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(800.0, 600.0));
        let size = Vec2::new(240.0, 34.0);
        let p = launcher_pos(Rect::from_min_max(Pos2::new(300.0, 100.0), Pos2::new(500.0, 300.0)), canvas, size);
        assert_eq!(p, Pos2::new(280.0, 310.0));
        // seleção encostada embaixo: vai para cima
        let p = launcher_pos(Rect::from_min_max(Pos2::new(300.0, 400.0), Pos2::new(500.0, 590.0)), canvas, size);
        assert!(p.y + size.y <= 400.0, "{p:?}");
        // encostada na esquerda: não sai do canvas
        let p = launcher_pos(Rect::from_min_max(Pos2::new(-50.0, 100.0), Pos2::new(40.0, 200.0)), canvas, size);
        assert!(p.x >= 4.0);
    }

    #[test]
    fn fill_clear_grow_and_copy_work_on_a_real_document() {
        use crate::app::editor::{SelMode, Selection};
        let mut core = aiai_core::CoreSession::for_tests("selops");
        let (cut, _) = cmds::quick_start_with_layers(&mut core, 6, Some([128, 96])).unwrap();
        let raster = cmds::add_layer(&mut core, &cut, LayerType::Raster, None).unwrap();
        let mut ed = crate::app::editor::Editor::default();
        ed.cut = Some(cut.clone());
        ed.layer = Some(raster.clone());
        ed.color = aiai_core::ColorRef::Direct { rgba: aiai_core::Rgba([1.0, 0.0, 0.0, 1.0]) };
        let (mut icons, mut notes, mut tool) = (crate::icons::Icons::default(), Vec::new(), ToolId::Lasso);
        let mut env = Env { t: crate::theme::Tokens::dark(), core: &mut core, ed: &mut ed, icons: &mut icons, notes: &mut notes, tool: &mut tool, ws: crate::workspace::WorkspaceId::Pintura };
        let pixels = |env: &Env| {
            let c = cmds::exposed_cel(&env.core.project, &cut, &raster, 0).unwrap();
            let d = cmds::cel_data(&env.core.project, &c).unwrap().clone();
            aiai_tools::paint::cel_raster(&env.core.blobs, &d, 128, 96).unwrap()
        };
        env.ed.selection = Some(Selection::new(SelMode::Rect, vec![Vec2::new(16.0, 16.0), Vec2::new(48.0, 48.0)], false));
        run(&mut env, Cmd::FillSelection);
        let img = pixels(&env);
        assert_eq!(img.get(20, 20)[3], 1.0, "dentro pintado");
        assert_eq!(img.get(60, 20)[3], 0.0, "fora não");
        // expandir 4 px: o pixel 50 entra na área; limpar dentro apaga até ele
        run(&mut env, Cmd::Grow);
        let r = env.ed.selection.as_ref().and_then(|s| s.rect_of()).unwrap();
        assert!(r[0] <= 12.5 && r[2] >= 39.5, "{r:?}");
        // copiar para camada nova: a cópia tem o vermelho e fica ativa; o nome segue Preferências > Nome das cópias
        env.ed.beh.prefs.copy_name = crate::behavior_prefs::CopyName::Number;
        let orig = env.core.project.node(&raster).unwrap().name.clone();
        run(&mut env, Cmd::CopyToLayer);
        let copy = env.ed.layer.clone().unwrap();
        assert_ne!(copy, raster);
        assert_eq!(env.core.project.node(&copy).unwrap().name, format!("{orig} 2"), "estilo numerado, sem sufixo extra");
        assert!(env.ed.selection.is_none());
        let cc = cmds::exposed_cel(&env.core.project, &cut, &copy, 0).unwrap();
        assert!(!cmds::cel_data(&env.core.project, &cc).unwrap().tiles.is_empty());
        // limpar a camada inteira (sem seleção) na original
        env.ed.layer = Some(raster.clone());
        run(&mut env, Cmd::Clear);
        assert!(pixels(&env).data.iter().all(|p| p[3] == 0.0), "camada limpa");
        // limpar fora sem seleção: recusa com aviso
        run(&mut env, Cmd::ClearOutside);
        assert!(env.ed.toast.is_some());
    }

    #[test]
    fn only_changed_tiles_are_written() {
        let blob = aiai_core::BlobHash::of(b"x");
        let mut cel = CelData::new(1);
        cel.tiles.push(TileEntry { x: 0, y: 0, blob: blob.clone() });
        let edits = vec![TileEdit { x: 0, y: 0, blob: None }, TileEdit { x: 1, y: 0, blob: None }, TileEdit { x: 2, y: 0, blob: Some(blob) }];
        let kept = changed_tiles(&cel, edits);
        assert_eq!(kept.iter().map(|e| e.x).collect::<Vec<_>>(), vec![0, 2]);
    }
}
