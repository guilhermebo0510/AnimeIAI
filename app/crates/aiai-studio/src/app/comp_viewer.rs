//! Viewer da Composição (S24 3.1): no workspace Composição o canvas não pinta. A barra troca as opções de pintura pelas
//! do viewer (resolução Completa/1/2/1/4, xadrez de transparência, canais RGB/Alfa, região de interesse) e as
//! ferramentas agem na camada e na câmera, não nos pixels:
//!
//! - Seleção `V`: arrasta a camada selecionada (Posição; com o cronômetro ligado grava a chave no quadro);
//! - Girar `W`: gira em volta da âncora (Shift = passos de 15°);
//! - Âncora `Y`: move o ponto de âncora sem mover a camada (compensa a Posição, como o "pan behind");
//! - Mão `H` e Zoom `Z`: os do canvas;
//! - Câmera `C`: a câmera do corte é 2D (centro, zoom, rotação, sem 2,5D): arrastar move o quadro da câmera, Shift +
//!   arrasto vertical faz zoom e Ctrl + arrasto horizontal gira; com chave na propriedade, grava a chave no quadro.
//!
//! Durante o arrasto só o contorno da camada (ou da câmera) anda; ao soltar grava um passo de desfazer e o quadro é
//! renderizado de novo. Soltar um efeito do painel Efeitos e predefinições no viewer aplica na camada selecionada.

use super::comp::{self, CompDrag, PropRef};
use super::commands::{self as cmds, NodeId};
use super::editor::Xf;
use super::ui_kit::Env;
use crate::i18n::tr;
use crate::workspace::{ToolId, WorkspaceId};
use aiai_core::{Command, NodeKind, Project};
use egui::{Color32, Pos2, Rect, Shape, Stroke, Ui, Vec2};

/// Ferramentas que agem por este módulo na Composição.
pub(crate) fn handles(env: &Env, tool: ToolId) -> bool {
    env.ws == WorkspaceId::Composicao && matches!(tool, ToolId::Move | ToolId::Rotate | ToolId::Anchor | ToolId::Camera)
}

/// Barra do viewer (no lugar das opções de pintura).
pub(crate) fn toolbar(env: &mut Env, ui: &mut Ui, has_cut: bool) {
    let st = &env.ed.comp;
    let (res, checker, alpha, roi) = (st.resolution, st.checker, st.alpha_view, st.roi.is_some() || st.roi_pick);
    let label = |r: u32| match r {
        1 => tr("comp.view.full").to_owned(),
        n => format!("1/{n}"),
    };
    let mut pick = None;
    let r = egui::ComboBox::from_id_salt("comp_view_res").width(78.0).selected_text(label(res)).show_ui(ui, |ui| {
        for n in [1u32, 2, 4] {
            if ui.selectable_label(res == n, label(n)).clicked() {
                pick = Some(n);
            }
        }
    });
    env.note("canvas:comp_resolution", r.response.rect);
    r.response.on_hover_text(tr("comp.view.resolution.tip"));
    if let Some(n) = pick {
        env.ed.comp.resolution = n;
    }
    let b = ui.add_enabled(has_cut, egui::Button::selectable(checker, tr("comp.view.checker"))).on_hover_text(tr("comp.view.checker.tip"));
    env.note("canvas:comp_checker", b.rect);
    if b.clicked() {
        env.ed.comp.checker = !checker;
    }
    for (on, key, name) in [(!alpha, "comp.view.rgb", "rgb"), (alpha, "comp.view.alpha", "alpha")] {
        let b = ui.add_enabled(has_cut, egui::Button::selectable(on, tr(key)));
        env.note(format!("canvas:comp_channel.{name}"), b.rect);
        if b.clicked() {
            env.ed.comp.alpha_view = name == "alpha";
        }
    }
    let b = ui.add_enabled(has_cut, egui::Button::selectable(roi, tr("comp.view.roi"))).on_hover_text(tr("comp.view.roi.tip"));
    env.note("canvas:comp_roi", b.rect);
    if b.clicked() {
        if roi {
            env.ed.comp.roi = None;
            env.ed.comp.roi_pick = false;
        } else {
            env.ed.comp.roi_pick = true;
        }
    }
    let (c, gd) = (env.ed.show_camera, env.ed.show_guides);
    if env.icon_button(ui, crate::icons::IconId::CameraFrame, "canvas.camera", c, has_cut, "canvas:camera").clicked() {
        env.ed.show_camera = !c;
    }
    if env.icon_button(ui, crate::icons::IconId::LayerGuide, "canvas.guides", gd, has_cut, "canvas:guides").clicked() {
        env.ed.show_guides = !gd;
    }
    let hint = if env.ed.comp.roi_pick {
        "comp.hint.roi"
    } else {
        match *env.tool {
            ToolId::Move => "comp.hint.move",
            ToolId::Rotate => "comp.hint.rotate",
            ToolId::Anchor => "comp.hint.anchor",
            ToolId::Camera => "comp.hint.camera",
            _ => "comp.hint.view",
        }
    };
    ui.label(egui::RichText::new(tr(hint)).size(11.5).color(env.t.text_dim));
}

/// Divisor da resolução do render do viewer.
pub(crate) fn reduce(env_ws: WorkspaceId, st: &comp::CompState) -> u32 {
    if env_ws == WorkspaceId::Composicao { st.resolution.max(1) } else { 1 }
}

/// Camadas que o render do viewer mostra com solo ligado: as em solo, as pastas que as contêm (para chegar nelas) e
/// tudo dentro delas. Vazio = sem solo (tudo).
pub(crate) fn solo_ids(p: &Project, cut: &NodeId, solo: &std::collections::BTreeSet<NodeId>) -> Vec<NodeId> {
    let rows = cmds::layer_rows(p, cut);
    let in_cut: Vec<&NodeId> = solo.iter().filter(|s| rows.iter().any(|r| &r.id == *s)).collect();
    if in_cut.is_empty() {
        return Vec::new();
    }
    let mut out = std::collections::BTreeSet::new();
    for s in in_cut {
        out.insert(s.clone());
        // pastas acima
        let mut cur = p.node(s).and_then(|n| n.parent.clone());
        while let Some(c) = cur {
            if &c == cut {
                break;
            }
            out.insert(c.clone());
            cur = p.node(&c).and_then(|n| n.parent.clone());
        }
        // tudo dentro (pasta em solo)
        let mut stack = vec![s.clone()];
        while let Some(n) = stack.pop() {
            for ch in p.node(&n).map(|x| x.children.clone()).unwrap_or_default() {
                if cmds::layer_data(p, &ch).is_some() {
                    out.insert(ch.clone());
                    stack.push(ch);
                }
            }
        }
    }
    out.into_iter().collect()
}

/// Textura do canal alfa do quadro mostrado (cinza: branco = opaco), refeita quando o quadro muda.
pub(crate) fn alpha_texture(ed: &mut super::editor::Editor, ctx: &egui::Context) -> Option<egui::TextureId> {
    let shown = ed.shown.as_ref()?;
    let key = shown.rendered.key.clone();
    if let Some((k, t)) = &ed.comp.alpha_tex {
        if *k == key {
            return Some(t.id());
        }
    }
    let src = &shown.rendered.image;
    let pixels: Vec<Color32> = src.pixels.iter().map(|c| Color32::from_gray(c.a())).collect();
    let img = egui::ColorImage::new(src.size, pixels);
    let tex = ctx.load_texture("comp-alpha", img, egui::TextureOptions::LINEAR);
    let id = tex.id();
    ed.comp.alpha_tex = Some((key, tex));
    Some(id)
}

/// Valores da transformação da camada no quadro: (âncora, posição, escala, rotação).
fn transform_at(l: &aiai_core::LayerData, layer: &NodeId, frame: u32) -> ([f32; 2], [f32; 2], f32, f32) {
    let v = |n: &str| comp::values(l, &PropRef::layer(layer, n), frame);
    let (a, p, s, r) = (v("anchor"), v("position"), v("scale"), v("rotation"));
    ([a[0], a[1]], [p[0], p[1]], s[0], r[0])
}

/// Onde a âncora da camada está no canvas (centro + âncora + posição).
fn anchor_canvas(l: &aiai_core::LayerData, layer: &NodeId, frame: u32, canvas: (u32, u32)) -> Vec2 {
    let (a, p, _, _) = transform_at(l, layer, frame);
    Vec2::new(canvas.0 as f32 / 2.0 + a[0] + p[0], canvas.1 as f32 / 2.0 + a[1] + p[1])
}

/// Botão apertado no viewer com uma ferramenta da Composição. Devolve se tratou.
pub(crate) fn press(env: &mut Env, xf: &Xf, pos: Pos2, mods: egui::Modifiers) -> bool {
    let c = xf.to_canvas(pos);
    if env.ed.comp.roi_pick {
        env.ed.comp.drag = Some(CompDrag::Roi { start: c, cur: c });
        return true;
    }
    let tool = *env.tool;
    if !handles(env, tool) {
        return false;
    }
    let Some(cut) = env.ed.active_cut(env.core) else { return false };
    let frame = env.ed.frame;
    // ponta do osso armada: o clique marca a ponta (na pose de repouso da camada)
    if let Some(layer) = env.ed.comp.tip_pick.take() {
        let canvas = cmds::canvas_size(&env.core.project, &cut);
        if let Some(l) = cmds::layer_data(&env.core.project, &layer).cloned() {
            let g = aiai_render::compose::parent_chain_affine(&env.core.project, &l, frame, canvas.0, canvas.1);
            let m = compose(&g, &aiai_render::compose::layer_motion_affine(&l, frame, canvas.0, canvas.1));
            if let Some(inv) = m.inverse() {
                let (x, y) = inv.apply(c.x, c.y);
                let r = crate::app::actions::run(env.core, "RIG", serde_json::json!({ "op": "bone_tip", "layer": layer, "tip": [x, y] }));
                if let Err(e) = r {
                    env.ed.notify(e);
                }
            }
        }
        return true;
    }
    // W na ponta do osso de uma camada com pai: IK
    if tool == ToolId::Rotate {
        if let Some(layer) = env.ed.active_layer(env.core) {
            if let Ok((_, _, tip)) = aiai_tools::rig::chain_points(&env.core.project, &cut, &layer, frame) {
                if (xf.to_screen(Vec2::new(tip[0], tip[1])) - pos).length() <= 10.0 {
                    env.ed.comp.drag = Some(CompDrag::Ik { layer, cut, cur: c });
                    return true;
                }
            }
        }
    }
    if tool == ToolId::Camera {
        let Some(cd) = cmds::cut_data(&env.core.project, &cut) else { return false };
        env.ed.comp.drag = Some(CompDrag::Camera { start: c, orig: cd.camera.clone() });
        env.ed.comp.cam_preview = Some(cd.camera.clone());
        return true;
    }
    let Some(layer) = env.ed.active_layer(env.core) else {
        env.ed.notify(tr("comp.select_layer_first").to_owned());
        return true;
    };
    let Some(l) = cmds::layer_data(&env.core.project, &layer).cloned() else { return false };
    if l.locked {
        env.ed.notify(tr("comp.layer_locked").to_owned());
        return true;
    }
    let canvas = cmds::canvas_size(&env.core.project, &cut);
    let _ = mods;
    env.ed.comp.drag = Some(match tool {
        ToolId::Rotate => {
            // o pivô no lugar em que está na tela: com pai, o movimento dele leva o pivô junto
            let g = aiai_render::compose::parent_chain_affine(&env.core.project, &l, frame, canvas.0, canvas.1);
            let a = anchor_canvas(&l, &layer, frame, canvas);
            let (px, py) = g.apply(a.x, a.y);
            let pivot = Vec2::new(px, py);
            let d = c - pivot;
            CompDrag::Rotate { layer: layer.clone(), pivot, start_angle: d.y.atan2(d.x), orig: l.clone() }
        }
        ToolId::Anchor => CompDrag::Anchor { layer: layer.clone(), start: c, orig: l.clone() },
        _ => CompDrag::Move { layer: layer.clone(), start: c, orig: l.clone() },
    });
    env.ed.comp.preview = Some((layer, l));
    true
}

/// Arrasto: atualiza a prévia (camada ou câmera).
pub(crate) fn drag(env: &mut Env, xf: &Xf, pos: Pos2, mods: egui::Modifiers) {
    let c = xf.to_canvas(pos);
    let frame = env.ed.frame;
    let Some(d) = env.ed.comp.drag.clone() else { return };
    match d {
        CompDrag::Roi { start, .. } => env.ed.comp.drag = Some(CompDrag::Roi { start, cur: c }),
        CompDrag::Ik { layer, cut, .. } => env.ed.comp.drag = Some(CompDrag::Ik { layer, cut, cur: c }),
        CompDrag::Move { layer, start, orig } => {
            let mut l = orig.clone();
            let (_, p, _, _) = transform_at(&orig, &layer, frame);
            let mut dv = c - start;
            if mods.shift {
                // Shift: só no eixo dominante
                if dv.x.abs() > dv.y.abs() { dv.y = 0.0 } else { dv.x = 0.0 }
            }
            if comp::set_values(&mut l, &PropRef::layer(&layer, "position"), frame, &[p[0] + dv.x, p[1] + dv.y]).is_ok() {
                env.ed.comp.preview = Some((layer, l));
            }
        }
        CompDrag::Rotate { layer, pivot, start_angle, orig } => {
            let mut l = orig.clone();
            let (_, _, _, r0) = transform_at(&orig, &layer, frame);
            let dv = c - pivot;
            let mut delta = (dv.y.atan2(dv.x) - start_angle).to_degrees();
            if delta > 180.0 {
                delta -= 360.0;
            } else if delta < -180.0 {
                delta += 360.0;
            }
            let mut r = r0 + delta;
            if mods.shift {
                r = (r / 15.0).round() * 15.0;
            }
            if comp::set_values(&mut l, &PropRef::layer(&layer, "rotation"), frame, &[r]).is_ok() {
                env.ed.comp.preview = Some((layer, l));
            }
        }
        CompDrag::Anchor { layer, start, orig } => {
            if let Some(l) = anchor_moved(&orig, &layer, frame, c - start) {
                env.ed.comp.preview = Some((layer, l));
            }
        }
        CompDrag::Camera { start, orig } => {
            let Some(cut) = env.ed.cut.clone() else { return };
            let canvas = cmds::canvas_size(&env.core.project, &cut);
            env.ed.comp.cam_preview = Some(camera_moved(&orig, frame, canvas, c - start, mods));
        }
    }
}

/// Âncora deslocada de `d` px do canvas (onde o marcador deve ir) sem mover a camada: com escala `s` e rotação `r`, a
/// âncora anda M⁻¹·d e a posição compensa (M − I)·Δâncora, então o desenho fica parado.
pub(crate) fn anchor_moved(orig: &aiai_core::LayerData, layer: &NodeId, frame: u32, d: Vec2) -> Option<aiai_core::LayerData> {
    let (a, p, s, r) = transform_at(orig, layer, frame);
    let s = s.max(1e-4);
    let (sn, cs) = r.to_radians().sin_cos();
    // M = s·R(r) com R = [[cos, −sin], [sin, cos]] (horário com y para baixo); M⁻¹·d = R(−r)·d / s
    let da = Vec2::new((cs * d.x + sn * d.y) / s, (-sn * d.x + cs * d.y) / s);
    let md = Vec2::new(s * (cs * da.x - sn * da.y), s * (sn * da.x + cs * da.y));
    let dp = md - da;
    let mut l = orig.clone();
    comp::set_values(&mut l, &PropRef::layer(layer, "anchor"), frame, &[a[0] + da.x, a[1] + da.y]).ok()?;
    comp::set_values(&mut l, &PropRef::layer(layer, "position"), frame, &[p[0] + dp.x, p[1] + dp.y]).ok()?;
    Some(l)
}

/// Câmera arrastada: centro (arrasto), zoom (Shift, vertical) ou rotação (Ctrl, horizontal). Grava chave no quadro
/// onde a propriedade já tem chave; senão muda o valor fixo.
pub(crate) fn camera_moved(orig: &aiai_core::CameraData, frame: u32, canvas: (u32, u32), d: Vec2, mods: egui::Modifiers) -> aiai_core::CameraData {
    let mut cam = orig.clone();
    let f = frame as f32;
    let base = orig.center.unwrap_or([canvas.0 as f32 / 2.0, canvas.1 as f32 / 2.0]);
    let cur = |k: &str, dflt: f32| orig.keys.eval(k, f).unwrap_or(dflt);
    let put = |cam: &mut aiai_core::CameraData, k: &str, v: f32| {
        if cam.keys.has(k) {
            cam.keys.set_key(k, frame, v);
            true
        } else {
            false
        }
    };
    if mods.shift {
        let z = (cur("zoom", orig.zoom) * (-d.y * 0.004).exp()).clamp(0.05, 20.0);
        if !put(&mut cam, "zoom", z) {
            cam.zoom = z;
        }
    } else if mods.ctrl || mods.command {
        let r = cur("rotation", orig.rotation) + d.x * 0.25;
        if !put(&mut cam, "rotation", r) {
            cam.rotation = r;
        }
    } else {
        let (cx, cy) = (cur("center_x", base[0]) + d.x, cur("center_y", base[1]) + d.y);
        let kx = put(&mut cam, "center_x", cx);
        let ky = put(&mut cam, "center_y", cy);
        if !(kx && ky) {
            let mut c = cam.center.unwrap_or(base);
            if !kx {
                c[0] = cx;
            }
            if !ky {
                c[1] = cy;
            }
            cam.center = Some(c);
        }
    }
    cam
}

/// Soltou: grava (um passo de desfazer).
pub(crate) fn release(env: &mut Env) {
    let Some(d) = env.ed.comp.drag.take() else { return };
    let preview = env.ed.comp.preview.take();
    let cam = env.ed.comp.cam_preview.take();
    match d {
        CompDrag::Ik { layer, cut, cur } => {
            let frame = env.ed.frame;
            let r = crate::app::actions::run(env.core, "RIG", serde_json::json!({ "op": "ik", "cut": cut, "layer": layer, "target": [cur.x, cur.y], "frame": frame }));
            if let Err(e) = r {
                env.ed.notify(e);
            }
        }
        CompDrag::Roi { start, cur } => {
            env.ed.comp.roi_pick = false;
            let r = Rect::from_two_pos(start.to_pos2(), cur.to_pos2());
            env.ed.comp.roi = (r.width() > 4.0 && r.height() > 4.0).then_some([r.min.x, r.min.y, r.max.x, r.max.y]);
        }
        CompDrag::Camera { orig, .. } => {
            let (Some(cut), Some(cam)) = (env.ed.cut.clone(), cam) else { return };
            if cam == orig {
                return;
            }
            let Some(mut cd) = cmds::cut_data(&env.core.project, &cut).cloned() else { return };
            cd.camera = cam;
            let r = cmds::run(env.core, Command::Batch { commands: vec![Command::ReplaceData { id: cut, data: NodeKind::Cut(cd) }], label: Some(tr("comp.undo.camera").to_owned()) });
            if let Err(e) = r {
                env.ed.notify(e);
            }
        }
        CompDrag::Move { layer, orig, .. } | CompDrag::Rotate { layer, orig, .. } | CompDrag::Anchor { layer, orig, .. } => {
            let Some((pl, l)) = preview else { return };
            if pl != layer || l == orig {
                return;
            }
            let key = match *env.tool {
                ToolId::Rotate => "comp.undo.rotate",
                ToolId::Anchor => "comp.undo.anchor",
                _ => "comp.undo.move",
            };
            if let Err(e) = comp::commit(env.core, &layer, l, tr(key)) {
                env.ed.notify(e);
            }
        }
    }
}

/// Soltar um efeito arrastado do navegador no viewer: vai para a camada selecionada.
pub(crate) fn drop_target(env: &mut Env, ui: &Ui, resp: &egui::Response) {
    if env.ws != WorkspaceId::Composicao {
        return;
    }
    if resp.dnd_hover_payload::<super::panels::effects_browser::FxPayload>().is_some() {
        ui.painter().rect_stroke(resp.rect.shrink(2.0), 4.0, Stroke::new(2.0, env.t.accent), egui::StrokeKind::Inside);
    }
    if let Some(p) = resp.dnd_release_payload::<super::panels::effects_browser::FxPayload>() {
        match env.ed.active_layer(env.core) {
            Some(l) => super::panels::effects_browser::apply(env, &l, &p),
            None => env.ed.notify(tr("comp.select_layer_first").to_owned()),
        }
    }
}

/// Desenho por cima do quadro: contorno e âncora da camada selecionada (com as ferramentas de camada), a câmera em
/// arrasto e a região de interesse.
/// m2 ∘ m1 (aplica m1, depois m2).
fn compose(m2: &aiai_paint::Affine, m1: &aiai_paint::Affine) -> aiai_paint::Affine {
    let (a, b) = (m2.0, m1.0);
    aiai_paint::Affine([
        a[0] * b[0] + a[1] * b[3],
        a[0] * b[1] + a[1] * b[4],
        a[0] * b[2] + a[1] * b[5] + a[2],
        a[3] * b[0] + a[4] * b[3],
        a[3] * b[1] + a[4] * b[4],
        a[3] * b[2] + a[4] * b[5] + a[5],
    ])
}

/// Ossos da camada ativa (W, rigging): ombro → cotovelo → ponta, com a ponta para arrastar (IK); no arrasto, a reta
/// até o cursor.
fn paint_bones(env: &Env, painter: &egui::Painter, xf: &Xf, cut: &NodeId) {
    if *env.tool != ToolId::Rotate {
        return;
    }
    let Some(layer) = env.ed.layer.clone() else { return };
    let Ok((root, elbow, tip)) = aiai_tools::rig::chain_points(&env.core.project, cut, &layer, env.ed.frame) else { return };
    let s = |p: [f32; 2]| xf.to_screen(Vec2::new(p[0], p[1]));
    let col = Color32::from_rgb(120, 200, 255);
    painter.line_segment([s(root), s(elbow)], Stroke::new(3.0, col.gamma_multiply(0.8)));
    painter.line_segment([s(elbow), s(tip)], Stroke::new(3.0, col.gamma_multiply(0.8)));
    for p in [root, elbow] {
        painter.circle_filled(s(p), 4.0, col);
    }
    painter.circle_stroke(s(tip), 7.0, Stroke::new(2.0, env.t.accent));
    if let Some(CompDrag::Ik { cur, .. }) = &env.ed.comp.drag {
        let c = xf.to_screen(*cur);
        painter.extend(Shape::dashed_line(&[s(root), c], Stroke::new(1.0, env.t.accent), 5.0, 3.0));
        painter.circle_stroke(c, 7.0, Stroke::new(2.0, env.t.accent));
    }
}

pub(crate) fn paint_overlay(env: &mut Env, painter: &egui::Painter, rect: Rect, xf: &Xf, cut: &NodeId, canvas: (u32, u32)) {
    let t = env.t;
    let frame = env.ed.frame;
    paint_bones(env, painter, xf, cut);
    let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
    // região de interesse: escurece fora
    let roi = match &env.ed.comp.drag {
        Some(CompDrag::Roi { start, cur }) => Some([start.x.min(cur.x), start.y.min(cur.y), start.x.max(cur.x), start.y.max(cur.y)]),
        _ => env.ed.comp.roi,
    };
    if let Some([x0, y0, x1, y1]) = roi {
        let q: Vec<Pos2> = [Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)].iter().map(|v| xf.to_screen(*v)).collect();
        if xf_axis_aligned(xf) {
            let r = Rect::from_points(&q);
            let dim = Color32::from_black_alpha(150);
            for part in [
                Rect::from_min_max(rect.min, egui::pos2(rect.right(), r.top())),
                Rect::from_min_max(egui::pos2(rect.left(), r.bottom()), rect.max),
                Rect::from_min_max(egui::pos2(rect.left(), r.top()), egui::pos2(r.left(), r.bottom())),
                Rect::from_min_max(egui::pos2(r.right(), r.top()), egui::pos2(rect.right(), r.bottom())),
            ] {
                if part.is_positive() {
                    painter.rect_filled(part.intersect(rect), 0.0, dim);
                }
            }
        }
        let mut closed = q.clone();
        closed.push(q[0]);
        painter.extend(Shape::dashed_line(&closed, Stroke::new(1.0, t.accent), 5.0, 3.0));
    }
    // camada selecionada (ferramentas de camada)
    if matches!(*env.tool, ToolId::Move | ToolId::Rotate | ToolId::Anchor) {
        if let Some(layer) = env.ed.layer.clone() {
            let l = match &env.ed.comp.preview {
                Some((id, l)) if *id == layer => Some(l.clone()),
                _ => cmds::layer_data(&env.core.project, &layer).cloned(),
            };
            if let Some(l) = l {
                let ct = cmds::exposed_cel(&env.core.project, cut, &layer, frame).and_then(|c| cmds::cel_data(&env.core.project, &c).map(|d| d.transform)).unwrap_or([0.0, 0.0, 1.0, 0.0]);
                let a = aiai_render::compose::layer_placement_in(&env.core.project, &l, ct, frame, canvas.0, canvas.1);
                let pts: Vec<Pos2> = [Vec2::new(0.0, 0.0), Vec2::new(cw, 0.0), Vec2::new(cw, ch), Vec2::new(0.0, ch)]
                    .iter()
                    .map(|v| {
                        let (x, y) = a.apply(v.x, v.y);
                        xf.to_screen(Vec2::new(x, y))
                    })
                    .collect();
                painter.add(Shape::closed_line(pts.clone(), Stroke::new(1.0, t.accent.gamma_multiply(0.85))));
                for p in &pts {
                    painter.rect_filled(Rect::from_center_size(*p, egui::vec2(5.0, 5.0)), 0.0, t.accent);
                }
                // com pai, a âncora anda com ele
                let g = aiai_render::compose::parent_chain_affine(&env.core.project, &l, frame, canvas.0, canvas.1);
                let a0 = anchor_canvas(&l, &layer, frame, canvas);
                let (ax, ay) = g.apply(a0.x, a0.y);
                let ac = xf.to_screen(Vec2::new(ax, ay));
                painter.circle_stroke(ac, 6.0, Stroke::new(2.5, Color32::from_black_alpha(160)));
                painter.circle_stroke(ac, 6.0, Stroke::new(1.2, Color32::WHITE));
                for dv in [egui::vec2(10.0, 0.0), egui::vec2(-10.0, 0.0), egui::vec2(0.0, 10.0), egui::vec2(0.0, -10.0)] {
                    painter.line_segment([ac + dv * 0.6, ac + dv], Stroke::new(1.2, Color32::WHITE));
                }
            }
        }
    }
    // câmera em arrasto
    if let Some(cam) = &env.ed.comp.cam_preview {
        let s = &env.core.project.settings;
        let st = aiai_render::compose::CameraState::eval(cam, frame, canvas);
        let m = st.affine(0.0, s.width, s.height).0;
        let det = m[0] * m[4] - m[1] * m[3];
        if det.abs() > 1e-9 {
            let inv = |p: Vec2| {
                let (x, y) = (p.x - m[2], p.y - m[5]);
                Vec2::new((m[4] * x - m[1] * y) / det, (-m[3] * x + m[0] * y) / det)
            };
            let (ow, oh) = (s.width as f32, s.height as f32);
            let pts: Vec<Pos2> = [Vec2::new(0.0, 0.0), Vec2::new(ow, 0.0), Vec2::new(ow, oh), Vec2::new(0.0, oh)].iter().map(|v| xf.to_screen(inv(*v))).collect();
            painter.add(Shape::closed_line(pts, Stroke::new(2.0, t.accent)));
        }
    }
}

fn xf_axis_aligned(xf: &Xf) -> bool {
    let a = xf.to_screen(Vec2::new(0.0, 0.0));
    let b = xf.to_screen(Vec2::new(100.0, 0.0));
    (a.y - b.y).abs() < 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_core::{LayerData, LayerType};

    #[test]
    fn the_anchor_tool_keeps_the_layer_still() {
        let id = NodeId::from("L");
        let mut l = LayerData::new(LayerType::Raster);
        l.scale = 0.5;
        l.rotation = 30.0;
        l.offset = [12.0, -7.0];
        let before = aiai_render::compose::layer_placement(&l, [0.0, 0.0, 1.0, 0.0], 0, 200, 100);
        let moved = anchor_moved(&l, &id, 0, Vec2::new(40.0, 25.0)).unwrap();
        let after = aiai_render::compose::layer_placement(&moved, [0.0, 0.0, 1.0, 0.0], 0, 200, 100);
        for (x, y) in [(0.0, 0.0), (200.0, 100.0), (37.0, 81.0)] {
            let (a, b) = (before.apply(x, y), after.apply(x, y));
            assert!((a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3, "{a:?} vs {b:?}");
        }
        // o marcador da âncora foi para onde o cursor levou
        let m0 = anchor_canvas(&l, &id, 0, (200, 100));
        let m1 = anchor_canvas(&moved, &id, 0, (200, 100));
        assert!(((m1 - m0) - Vec2::new(40.0, 25.0)).length() < 1e-3, "{m0:?} → {m1:?}");
    }

    #[test]
    fn camera_drag_writes_static_values_or_keys() {
        let cam = aiai_core::CameraData::default();
        let moved = camera_moved(&cam, 3, (200, 100), Vec2::new(10.0, -4.0), egui::Modifiers::NONE);
        assert_eq!(moved.center, Some([110.0, 46.0]));
        assert!(moved.keys.is_empty());
        let mut keyed = cam.clone();
        keyed.keys.set_key("zoom", 0, 1.0);
        keyed.keys.set_key("zoom", 10, 2.0);
        let z = camera_moved(&keyed, 5, (200, 100), Vec2::new(0.0, -100.0), egui::Modifiers::SHIFT);
        assert_eq!(z.keys.frames("zoom"), vec![0, 5, 10]);
        assert!(z.keys.eval("zoom", 5.0).unwrap() > 1.5);
        assert_eq!(z.zoom, 1.0, "o fixo não muda quando há chave");
    }

    #[test]
    fn solo_keeps_the_folders_on_the_way() {
        let mut core = aiai_core::CoreSession::for_tests("solo");
        let cut = cmds::quick_start(&mut core, 4, None).unwrap();
        let g = cmds::add_layer(&mut core, &cut, LayerType::Group, None).unwrap();
        let a = cmds::add_layer(&mut core, &cut, LayerType::Raster, None).unwrap();
        cmds::move_layer_to(&mut core, &a, &g).unwrap();
        let b = cmds::add_layer(&mut core, &cut, LayerType::Raster, None).unwrap();
        let ids = solo_ids(&core.project, &cut, &[a.clone()].into_iter().collect());
        assert!(ids.contains(&a) && ids.contains(&g) && !ids.contains(&b), "{ids:?}");
        assert!(solo_ids(&core.project, &cut, &Default::default()).is_empty());
    }
}
