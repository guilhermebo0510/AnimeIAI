//! Edição de pontos de traço vetorial (modo "Editar pontos" do Mover): clique escolhe o traço e mostra os pontos de
//! controle; arrastar move o ponto; Ctrl+arrastar muda a largura naquele ponto (pressão); Shift+clique no traço
//! insere um ponto; botão direito num ponto apaga; "Simplificar" reduz um traço denso de mão livre a poucos pontos.
//! Tudo vira `ReplaceStrokes` (mesmo id, um passo de desfazer), no espaço do cel (camada com movimento incluída).

use super::commands::{self as cmds, NodeId};
use super::editor::*;
use super::ui_kit::Env;
use aiai_core::*;
use egui::{Color32, Pos2, Stroke as EStroke, Vec2};

struct Ctx {
    cut: NodeId,
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
    Some(Ctx { cut, cel, fwd: inv.and(fwd), inv })
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

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let d = b - a;
    let l2 = d.length_sq();
    let t = if l2 > 0.0 { ((p - a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
    ((p - (a + d * t)).length(), t)
}

fn replace(env: &mut Env, cel: &NodeId, s: Stroke, label: &str) {
    let r = cmds::run(env.core, Command::ReplaceStrokes { cel: cel.clone(), strokes: vec![s] });
    if let Err(e) = r {
        env.ed.notify(format!("{label}: {e}"));
    }
}

/// Clique no canvas com o Mover em "Editar pontos". `pix` = tamanho de 1 px de tela em px do cel.
pub(crate) fn press(env: &mut Env, c_canvas: Vec2, pix: f32, button: egui::PointerButton, mods: egui::Modifiers) {
    let Some(c) = ctx(env) else {
        env.ed.notify(crate::i18n::tr("err.need_vector"));
        return;
    };
    let p = apply(&c.inv, c_canvas);
    let tol = 9.0 * pix;
    if let Some(s) = picked(env, &c) {
        let near = s.points.iter().enumerate().map(|(i, q)| (i, (Vec2::new(q.x, q.y) - p).length())).filter(|(_, d)| *d <= tol).min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = near {
            if button == egui::PointerButton::Secondary {
                if s.points.len() > 2 {
                    let mut ns = s.clone();
                    ns.points.remove(i);
                    replace(env, &c.cel, ns, "apagar ponto");
                }
                return;
            }
            env.ed.gesture = Gesture::DragNode { idx: i, start: p, cur: p, width: mods.ctrl || mods.command };
            return;
        }
        if mods.shift && button == egui::PointerButton::Primary {
            // insere um ponto no segmento mais perto
            let best = s.points.windows(2).enumerate().map(|(i, w)| {
                let (d, t) = seg_dist(p, Vec2::new(w[0].x, w[0].y), Vec2::new(w[1].x, w[1].y));
                (i, d, t)
            }).min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, d, t)) = best.filter(|b| b.1 <= tol * 1.5) {
                let (a, b) = (s.points[i], s.points[i + 1]);
                let q = StrokePoint { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, pressure: a.pressure + (b.pressure - a.pressure) * t, tilt: None, t: None };
                let mut ns = s.clone();
                ns.points.insert(i + 1, q);
                let _ = d;
                replace(env, &c.cel, ns, "inserir ponto");
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
    env.ed.node_stroke = best.filter(|b| b.1 <= tol * 1.5).map(|b| b.0);
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
            q.pressure = (q.pressure + (start.y - cur.y) / 80.0).clamp(0.05, 1.0);
        } else {
            q.x = cur.x;
            q.y = cur.y;
        }
    }
    ns
}

pub(crate) fn release(env: &mut Env, g: Gesture) {
    let Gesture::DragNode { idx, start, cur, width } = g else { return };
    let Some(c) = ctx(env) else { return };
    let Some(s) = picked(env, &c) else { return };
    if (cur - start).length() < 0.25 {
        return;
    }
    let ns = dragged(&s, idx, start, cur, width);
    replace(env, &c.cel, ns, "mover ponto");
    let _ = c.cut;
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
    let mut ns = s.clone();
    ns.points = s.points.iter().zip(&keep).filter(|(_, k)| **k).map(|(q, _)| *q).collect();
    if ns.points.len() >= 2 && ns.points.len() < s.points.len() {
        if ns.geometry == StrokeGeometry::Freehand {
            ns.geometry = StrokeGeometry::Spline;
        }
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

/// Pontos de controle do traço escolhido (e a prévia do arrasto).
pub(crate) fn paint(env: &mut Env, painter: &egui::Painter, to_screen: impl Fn(Vec2) -> Pos2, accent: Color32) {
    let Some(c) = ctx(env) else { return };
    let Some(mut s) = picked(env, &c) else { return };
    if let Gesture::DragNode { idx, start, cur, width } = &env.ed.gesture {
        s = dragged(&s, *idx, *start, *cur, *width);
        let sp: Vec<Pos2> = aiai_paint::sample_stroke(&s).iter().map(|q| to_screen(apply(&c.fwd, Vec2::new(q.x, q.y)))).collect();
        if sp.len() >= 2 {
            painter.add(egui::Shape::line(sp, EStroke::new(1.5, accent)));
        }
    }
    let pts: Vec<Pos2> = s.points.iter().map(|q| to_screen(apply(&c.fwd, Vec2::new(q.x, q.y)))).collect();
    if pts.len() >= 2 {
        painter.add(egui::Shape::line(pts.clone(), EStroke::new(1.0, accent.gamma_multiply(0.6))));
    }
    for (i, p) in pts.iter().enumerate() {
        let r = 2.5 + 2.5 * s.points[i].pressure.clamp(0.0, 1.0);
        painter.rect(egui::Rect::from_center_size(*p, egui::vec2(r * 2.0, r * 2.0)), 1.0, Color32::WHITE, EStroke::new(1.3, accent), egui::StrokeKind::Inside);
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
}
