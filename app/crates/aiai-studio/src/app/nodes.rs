//! Ferramenta Editar pontos (traço vetorial): clique escolhe o traço e mostra os pontos de controle; o ponto clicado
//! fica escolhido (o painel mostra posição, largura e canto dele). Modos (itens do painel Ferramenta):
//! - Mover: arrastar move o ponto (a linha de verdade anda junto, pelo motor); Ctrl+arrastar muda a largura naquele
//!   ponto; Shift+clique no traço insere um ponto; botão direito num ponto apaga.
//! - Adicionar: clique no traço põe um ponto em cima da curva. Apagar: clique no ponto. Canto/suave: clique alterna.
//! - Largura: arrastar o ponto para cima engrossa, para baixo afina.
//!
//! Tudo vira `ReplaceStrokes` (mesmo id, um passo de desfazer), no espaço do cel (camada com movimento incluída).
//! O endereço de um ponto é (id do traço, índice): é o que o painel, o `ui_state` e os comandos usam.

use super::commands::{self as cmds, NodeId};
use super::editor::*;
use super::ui_kit::Env;
use aiai_core::*;
use aiai_tools::points::delete_point;
use egui::{Color32, Pos2, Stroke as EStroke, Vec2};

struct Ctx {
    cut: NodeId,
    layer: NodeId,
    cel: NodeId,
    fwd: Option<aiai_paint::Affine>,
    inv: Option<aiai_paint::Affine>,
}

fn ctx(env: &mut Env) -> Option<Ctx> {
    let cut = env.ed.active_cut(env.core)?;
    let layer = env.ed.active_layer(env.core)?;
    if cmds::layer_data(&env.core.project, &layer).is_none_or(|l| l.kind != LayerType::Vector || l.locked) {
        return None;
    }
    let cel = cmds::exposed_cel(&env.core.project, &cut, &layer, env.ed.frame)?;
    let fwd = cmds::layer_placement(&env.core.project, &cut, &layer, env.ed.frame);
    let inv = fwd.and_then(|a| a.inverse());
    Some(Ctx { cut, layer, cel, fwd: inv.and(fwd), inv })
}

fn apply(a: &Option<aiai_paint::Affine>, p: Vec2) -> Vec2 {
    match a {
        Some(a) => {
            let (x, y) = a.apply(p.x, p.y);
            Vec2::new(x, y)
        }
        None => p,
    }
}

fn picked(env: &Env, c: &Ctx) -> Option<Stroke> {
    let id = env.ed.node_stroke.as_ref()?;
    cmds::cel_data(&env.core.project, &c.cel)?.strokes.iter().find(|s| &s.id == id).cloned()
}

/// Traço escolhido e o ponto escolhido (válido), para o painel.
pub(crate) fn selection(env: &mut Env) -> Option<(Stroke, Option<usize>)> {
    let c = ctx(env)?;
    let s = picked(env, &c)?;
    let sel = env.ed.node_sel.filter(|&i| i < s.points.len());
    Some((s, sel))
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let d = b - a;
    let l2 = d.length_sq();
    let t = if l2 > 0.0 { ((p - a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
    ((p - (a + d * t)).length(), t)
}

fn replace(env: &mut Env, cel: &NodeId, s: Stroke, label: &str) -> bool {
    if let Err(e) = s.validate() {
        env.ed.notify(format!("{label}: {e}"));
        return false;
    }
    match cmds::run(env.core, Command::ReplaceStrokes { cel: cel.clone(), strokes: vec![s] }) {
        Ok(_) => true,
        Err(e) => {
            env.ed.notify(format!("{label}: {e}"));
            false
        }
    }
}

/// Grava uma mudança no traço escolhido (painel). `f` devolve `false` para não gravar.
pub(crate) fn edit(env: &mut Env, label: &str, f: impl FnOnce(&mut Stroke) -> bool) {
    let Some(c) = ctx(env) else { return };
    let Some(mut s) = picked(env, &c) else { return };
    if f(&mut s) {
        replace(env, &c.cel, s, label);
    }
}

/// Apaga o ponto escolhido (o traço fica com pelo menos 2).
pub(crate) fn delete_selected(env: &mut Env) {
    let Some(i) = env.ed.node_sel else { return };
    edit(env, "apagar ponto", |s| delete_point(s, i));
    env.ed.node_sel = None;
}

/// Põe um ponto na curva perto de `p` (até `tol`); devolve o índice novo. A lógica é a de `aiai_tools::points`, a
/// mesma que a bridge usa (EDIT_POINTS).
fn insert_at(s: &mut Stroke, p: Vec2, tol: f32) -> Option<usize> {
    aiai_tools::points::insert_on_curve(s, p.x, p.y, tol)
}

/// Clique no canvas com Editar pontos. `pix` = tamanho de 1 px de tela em px do cel.
pub(crate) fn press(env: &mut Env, c_canvas: Vec2, pix: f32, button: egui::PointerButton, mods: egui::Modifiers) {
    let Some(c) = ctx(env) else {
        env.ed.notify(crate::i18n::tr("err.need_vector"));
        return;
    };
    let p = apply(&c.inv, c_canvas);
    let tol = 9.0 * pix;
    let mode = env.ed.opts.points_mode;
    let t0 = std::time::Instant::now();
    if let Some(s) = picked(env, &c) {
        let near = s.points.iter().enumerate().map(|(i, q)| (i, (Vec2::new(q.x, q.y) - p).length())).filter(|(_, d)| *d <= tol).min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = near {
            if button == egui::PointerButton::Secondary || (mode == PointsMode::Delete && button == egui::PointerButton::Primary) {
                let mut ns = s.clone();
                if delete_point(&mut ns, i) {
                    replace(env, &c.cel, ns, "apagar ponto");
                }
                env.ed.node_sel = None;
                return;
            }
            if button != egui::PointerButton::Primary {
                return;
            }
            env.ed.node_sel = Some(i);
            if mode == PointsMode::Corner {
                let mut ns = s.clone();
                ns.points[i].corner = !ns.points[i].corner;
                replace(env, &c.cel, ns, "canto/suave");
                return;
            }
            let width = mode == PointsMode::Width || mods.ctrl || mods.command;
            env.ed.gesture = Gesture::DragNode { idx: i, start: p, cur: p, width, t0 };
            return;
        }
        if button == egui::PointerButton::Primary && (mode == PointsMode::Add || (mode == PointsMode::Move && mods.shift)) {
            let mut ns = s.clone();
            if let Some(i) = insert_at(&mut ns, p, tol * 1.5) {
                if replace(env, &c.cel, ns, "inserir ponto") {
                    env.ed.node_sel = Some(i);
                    // já dá para arrastar o ponto novo no mesmo gesto
                    env.ed.gesture = Gesture::DragNode { idx: i, start: p, cur: p, width: false, t0 };
                }
                return;
            }
        }
    }
    if button != egui::PointerButton::Primary {
        return;
    }
    // escolhe o traço mais perto (pelas amostras do motor)
    let strokes = cmds::cel_data(&env.core.project, &c.cel).map(|d| d.strokes.clone()).unwrap_or_default();
    let best = strokes.iter().map(|s| {
        let d = aiai_paint::sample_stroke(s).iter().map(|q| ((Vec2::new(q.x, q.y) - p).length() - q.r).max(0.0)).fold(f32::MAX, f32::min);
        (s.id.clone(), d)
    }).min_by(|a, b| a.1.total_cmp(&b.1));
    let new = best.filter(|b| b.1 <= tol * 1.5).map(|b| b.0);
    if new != env.ed.node_stroke {
        env.ed.node_sel = None;
        env.ed.node_field = None;
    }
    env.ed.node_stroke = new;
}

pub(crate) fn drag(env: &mut Env, c_canvas: Vec2) {
    let Some(c) = ctx(env) else { return };
    let p = apply(&c.inv, c_canvas);
    if let Gesture::DragNode { cur, .. } = &mut env.ed.gesture {
        *cur = p;
    }
}

/// O traço escolhido com o arrasto em andamento aplicado.
fn dragged(s: &Stroke, idx: usize, start: Vec2, cur: Vec2, width: bool) -> Stroke {
    let mut ns = s.clone();
    if let Some(q) = ns.points.get_mut(idx) {
        if width {
            // para cima engrossa, para baixo afina (dobra a cada 60 px do cel)
            q.width = (q.width * 2f32.powf((start.y - cur.y) / 60.0)).clamp(0.02, 20.0);
        } else {
            q.x = cur.x;
            q.y = cur.y;
        }
    }
    ns
}

pub(crate) fn release(env: &mut Env, g: Gesture) {
    let Gesture::DragNode { idx, start, cur, width, .. } = g else { return };
    let Some(c) = ctx(env) else { return };
    let Some(s) = picked(env, &c) else { return };
    if (cur - start).length() < 0.25 {
        return;
    }
    let ns = dragged(&s, idx, start, cur, width);
    replace(env, &c.cel, ns, if width { "largura no ponto" } else { "mover ponto" });
}

/// Liga a prévia ao vivo do arrasto: o motor redesenha o traço editado com o original escondido.
pub(crate) fn start_live(env: &mut Env, ctx_ui: &egui::Context, t0: std::time::Instant, screen_scale: f32) {
    let Some(c) = ctx(env) else { return };
    let Some(s) = picked(env, &c) else { return };
    let Some(l) = cmds::layer_data(&env.core.project, &c.layer).cloned() else { return };
    let color = cmds::resolve_color(&env.core.project, &s.color).0;
    let spec = super::live::StartSpec {
        mode: super::live::Mode::EditStroke,
        project: Some(env.core.project.clone()),
        blobs: env.core.blobs.clone(),
        cut: c.cut.clone(),
        layer: c.layer.clone(),
        frame: env.ed.frame,
        cel: cmds::cel_data(&env.core.project, &c.cel).cloned(),
        canvas: cmds::canvas_size(&env.core.project, &c.cut),
        preset: aiai_paint::brush::BrushPreset::round("-", 4.0, 0.9),
        color,
        seed: 0,
        layer_opacity: l.opacity,
        fwd: c.fwd,
        guides: env.ed.show_guides,
        reference: env.ed.show_reference,
        checker: (super::canvas::checker_colors(env.ed.checker_dark), 8.0 / screen_scale.max(1e-3)),
        erase_radius: 0.0,
        erase_mode: env.ed.opts.eraser_mode,
        hide: Some((c.cel.clone(), s.id.clone())),
    };
    env.ed.live_stroke.start(ctx_ui, spec, t0);
}

/// Manda o traço com o arrasto atual para a prévia.
pub(crate) fn feed_live(env: &mut Env) {
    let Gesture::DragNode { idx, start, cur, width, .. } = env.ed.gesture else { return };
    let Some(c) = ctx(env) else { return };
    let Some(s) = picked(env, &c) else { return };
    let ns = dragged(&s, idx, start, cur, width);
    // contagem = posição do arrasto (manda de novo só quando ela muda)
    let count = ((cur.x * 64.0) as i64 as usize).wrapping_mul(1_000_003) ^ ((cur.y * 64.0) as i64 as usize) ^ (width as usize) ^ 1;
    env.ed.live_stroke.feed(Vec::new(), Vec::new(), Some(ns), count);
}

/// Simplifica o traço escolhido (Ramer–Douglas–Peucker, tolerância em px) e o deixa como spline (poucos pontos
/// editáveis).
pub(crate) fn simplify(env: &mut Env, tolerance: f32) {
    let Some(c) = ctx(env) else { return };
    let Some(s) = picked(env, &c) else { return };
    let pts: Vec<Vec2> = s.points.iter().map(|q| Vec2::new(q.x, q.y)).collect();
    let mut keep = vec![false; pts.len()];
    rdp(&pts, 0, pts.len().saturating_sub(1), tolerance.max(0.2), &mut keep);
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    // cantos marcados ficam
    for (k, q) in keep.iter_mut().zip(&s.points) {
        *k |= q.corner;
    }
    let mut ns = s.clone();
    ns.materialize_point_ids(); // os pontos que ficam mantêm os ids
    ns.points = ns.points.iter().zip(&keep).filter(|(_, k)| **k).map(|(q, _)| *q).collect();
    if ns.points.len() >= 2 && ns.points.len() < s.points.len() {
        if ns.geometry == StrokeGeometry::Freehand {
            ns.geometry = StrokeGeometry::Spline;
        }
        env.ed.node_sel = None;
        replace(env, &c.cel, ns, "simplificar");
    }
}

fn rdp(p: &[Vec2], a: usize, b: usize, tol: f32, keep: &mut [bool]) {
    if b <= a + 1 {
        return;
    }
    let (mut best, mut idx) = (0.0f32, a);
    for i in a + 1..b {
        let (d, _) = seg_dist(p[i], p[a], p[b]);
        if d > best {
            best = d;
            idx = i;
        }
    }
    if best > tol {
        keep[idx] = true;
        rdp(p, a, idx, tol, keep);
        rdp(p, idx, b, tol, keep);
    }
}

/// Pontos de controle do traço escolhido: suave = círculo, canto = quadrado; o escolhido cheio na cor de destaque.
/// Durante o arrasto, enquanto a prévia do motor não chega, a linha vai em contorno.
pub(crate) fn paint(env: &mut Env, painter: &egui::Painter, to_screen: impl Fn(Vec2) -> Pos2, accent: Color32) {
    let Some(c) = ctx(env) else { return };
    let Some(mut s) = picked(env, &c) else { return };
    let live = env.ed.live_stroke.overlay.is_some();
    if let Gesture::DragNode { idx, start, cur, width, .. } = &env.ed.gesture {
        s = dragged(&s, *idx, *start, *cur, *width);
        if !live {
            let sp: Vec<Pos2> = aiai_paint::sample_stroke(&s).iter().map(|q| to_screen(apply(&c.fwd, Vec2::new(q.x, q.y)))).collect();
            if sp.len() >= 2 {
                painter.add(egui::Shape::line(sp, EStroke::new(1.5, accent)));
            }
        }
    }
    let pts: Vec<Pos2> = s.points.iter().map(|q| to_screen(apply(&c.fwd, Vec2::new(q.x, q.y)))).collect();
    if pts.len() >= 2 {
        painter.add(egui::Shape::line(pts.clone(), EStroke::new(1.0, accent.gamma_multiply(0.45))));
    }
    let sel = env.ed.node_sel;
    // traço denso (mão livre): pontos menores para a linha continuar visível
    let spacing = if pts.len() >= 2 { pts.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f32>() / (pts.len() - 1) as f32 } else { 10.0 };
    let r0 = (spacing * 0.3).clamp(1.6, 3.5);
    for (i, p) in pts.iter().enumerate() {
        let q = &s.points[i];
        let r = if sel == Some(i) { 3.5 } else { r0 };
        let (fill, line) = if sel == Some(i) { (accent, Color32::WHITE) } else { (Color32::WHITE, accent) };
        // endereço do ponto na tela (ui_state: "point:<índice>"), para quem opera o app pela bridge
        env.note(format!("point:{i}"), egui::Rect::from_center_size(*p, egui::vec2(r * 2.0, r * 2.0)));
        if q.corner {
            painter.rect(egui::Rect::from_center_size(*p, egui::vec2(r * 2.0, r * 2.0)), 0.0, fill, EStroke::new(1.3, line), egui::StrokeKind::Inside);
        } else {
            painter.circle(*p, r, fill, EStroke::new(1.3, line));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdp_keeps_corners_and_drops_collinear_points() {
        let p: Vec<Vec2> = (0..=10).map(|i| Vec2::new(i as f32 * 10.0, 0.0)).chain((1..=10).map(|i| Vec2::new(100.0, i as f32 * 10.0))).collect();
        let mut keep = vec![false; p.len()];
        rdp(&p, 0, p.len() - 1, 1.0, &mut keep);
        keep[0] = true;
        *keep.last_mut().unwrap() = true;
        let kept: Vec<Vec2> = p.iter().zip(&keep).filter(|(_, k)| **k).map(|(q, _)| *q).collect();
        assert_eq!(kept, vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)]);
    }

    fn stroke(pts: &[(f32, f32)]) -> Stroke {
        Stroke {
            id: Id::from("s"),
            points: pts.iter().map(|&(x, y)| StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
            geometry: StrokeGeometry::Spline,
            tension: 0.0,
            tip: VectorTip::Marker,
            width: 4.0,
            profile: Default::default(),
            color: ColorRef::default(),
            opacity: 1.0,
            closed: false,
            aliased: false,
            color_trace: false,
        }
    }

    #[test]
    fn insert_lands_on_the_curve_in_the_right_segment() {
        let mut s = stroke(&[(0.0, 0.0), (100.0, 0.0), (200.0, 50.0)]);
        let i = insert_at(&mut s, Vec2::new(150.0, 30.0), 40.0).expect("perto da curva");
        assert_eq!(i, 2, "entre o segundo e o terceiro ponto");
        assert_eq!(s.points.len(), 4);
        let q = s.points[2];
        assert!((q.x - 150.0).abs() < 20.0, "x {}", q.x);
        assert!(insert_at(&mut s, Vec2::new(100.0, 300.0), 10.0).is_none(), "longe demais");
    }

    #[test]
    fn corner_changes_the_curve_and_delete_keeps_two_points() {
        let s = stroke(&[(0.0, 0.0), (100.0, 80.0), (200.0, 0.0)]);
        let mut c = s.clone();
        c.points[1].corner = true;
        let (a, b) = (aiai_paint::flatten_points(&s), aiai_paint::flatten_points(&c));
        let y_at = |f: &[(f32, f32, f32)], x: f32| f.iter().min_by(|p, q| (p.0 - x).abs().total_cmp(&(q.0 - x).abs())).unwrap().1;
        assert!((y_at(&a, 80.0) - y_at(&b, 80.0)).abs() > 0.5, "o canto muda a curva");
        let mut d = stroke(&[(0.0, 0.0), (10.0, 0.0)]);
        assert!(!delete_point(&mut d, 0), "traço de 2 pontos não perde ponto");
    }
}
