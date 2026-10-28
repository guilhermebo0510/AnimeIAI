//! Prévia de intercalação de um quadro na interface (R16 §7). A proposta mora na sessão (`CoreSession`), a mesma que o
//! MCP usa: uma prévia pedida pelo agente aparece aqui também (modo diretor), e aceitar por aqui passa pelas mesmas
//! barreiras. Só visão até o Aceitar: nada entra no projeto, no render nem na exportação.
//!
//! * Sobreposição: o candidato na cor da prévia, as chaves A e B finas e uma linha entre os centros de cada par, tudo
//!   projetado como o cel aceito vai aparecer (movimento da camada no quadro, transformação identidade).
//! * Barra: Aceitar (com o alcance escrito; desligado com recusa, prévia desatualizada ou camada travada, sempre com o
//!   motivo), Descartar, Atualizar, Corrigir pares e a lista de avisos/recusas. Sem atalho de teclado.
//! * Corrigir pares: clique num traço de A e depois num de B; clique num par manual e Delete desfaz.

use super::commands as cmds;
use super::editor::Xf;
use super::ui_kit::Env;
use crate::i18n::tr;
use aiai_core::{Id, LayerType, NodeKind, Stroke};
use aiai_tools::inbetween_preview::{self as ibp, InbetweenPreviewRequest, InbetweenProposal, Support};
use egui::{Color32, Pos2, Rect, RichText, Vec2};

/// Cor da prévia (a mesma do render com prévia da bridge; diferente do vermelho/verde da mesa de luz).
pub(crate) const PREVIEW: Color32 = Color32::from_rgb(224, 64, 250);
const KEY_A: Color32 = Color32::from_rgba_premultiplied(150, 40, 34, 150);
const KEY_B: Color32 = Color32::from_rgba_premultiplied(24, 112, 50, 150);
const REFUSE: Color32 = Color32::from_rgb(230, 60, 50);
const APPROX: Color32 = Color32::from_rgb(222, 160, 40);

/// Estado da interface da prévia (o que é só da tela; a proposta fica na sessão).
#[derive(Default)]
pub(crate) struct IbpUi {
    /// Cliques no canvas corrigem pares em vez de desenhar.
    pub pairs_mode: bool,
    /// Traço de A escolhido, esperando o de B.
    pub pick_a: Option<Id>,
    /// Par manual escolhido (Delete desfaz).
    pub selected: Option<(Id, Id)>,
    /// Lista de avisos aberta.
    pub show_notes: bool,
    /// Última proposta vista: uma prévia nova pedida pelo MCP leva a tela ao quadro dela (modo diretor).
    pub seen: Option<u64>,
    /// E1 (R16 §11): amostras brutas por quadro, só enquanto gravando (`UI_ACTION perf_samples`).
    pub perf: Perf,
    /// Geometria da proposta viva, amostrada uma vez.
    pub geom: Option<Geom>,
}

/// Amostras brutas de tempo (ms) por quadro: a interface inteira (CPU de montar o quadro) e o desenho da prévia.
#[derive(Default)]
pub(crate) struct Perf {
    pub on: bool,
    pub ui_ms: Vec<f32>,
    pub paint_ms: Vec<f32>,
}

impl Perf {
    const CAP: usize = 50_000;

    pub(crate) fn push_ui(&mut self, ms: f32) {
        if self.on && self.ui_ms.len() < Self::CAP {
            self.ui_ms.push(ms);
        }
    }

    pub(crate) fn push_paint(&mut self, ms: f32) {
        if self.on && self.paint_ms.len() < Self::CAP {
            self.paint_ms.push(ms);
        }
    }
}

/// Pedido de prévia para o quadro atual da camada ativa, se o quadro está entre dois blocos-chave (senão o motivo).
pub(crate) fn request_here(env: &mut Env) -> Result<InbetweenPreviewRequest, String> {
    let cut = env.ed.active_cut(env.core).ok_or_else(|| tr("err.no_cut").to_owned())?;
    let layer = env.ed.active_layer(env.core).ok_or_else(|| tr("ibp.need_layer").to_owned())?;
    if !cmds::layer_data(&env.core.project, &layer).is_some_and(|l| l.kind == LayerType::Vector) {
        return Err(tr("ibp.need_vector").to_owned());
    }
    let req = InbetweenPreviewRequest::new(cut, layer, env.ed.frame);
    ibp::resolve_frames(&env.core.project, &req).map_err(|e| e.to_string())?;
    Ok(req)
}

/// Pede a prévia (substitui a anterior).
pub(crate) fn request_preview(env: &mut Env, req: &InbetweenPreviewRequest) {
    env.ed.ibp.pick_a = None;
    env.ed.ibp.selected = None;
    // a prévia aparece no quadro dela
    env.ed.frame = req.frame;
    match ibp::preview(env.core, req) {
        Ok(p) => {
            let msg = if p.refusals.is_empty() { format!("{} ({} {})", tr("ibp.ready"), p.approximations.len(), tr("ibp.warnings")) } else { format!("{}: {} {}", tr("ibp.ready"), p.refusals.len(), tr("ibp.refusals")) };
            env.ed.notify(msg);
        }
        Err(e) => env.ed.notify(format!("{}: {e}", tr("ibp.title"))),
    }
}

/// Proposta viva no corte mostrado.
/// (cópia leve: sem candidato nem pares; o aceite usa a proposta guardada na sessão)
fn current_for(env: &Env, cut: &Id) -> Option<InbetweenProposal> {
    ibp::current(env.core).filter(|p| &p.cut == cut).map(InbetweenProposal::light)
}

/// Há prévia viva neste corte?
pub(crate) fn current_here(env: &Env, cut: &Id) -> bool {
    ibp::current(env.core).is_some_and(|p| &p.cut == cut)
}

/// Uma vez por quadro: o projeto mudou depois da prévia → ela some com o motivo (sem recalcular).
pub(crate) fn tick(env: &mut Env) {
    if let Some(why) = ibp::invalidate_if_changed(env.core) {
        env.ed.ibp.pairs_mode = false;
        env.ed.ibp.pick_a = None;
        env.ed.notify(format!("{}: {why}", tr("ibp.invalidated")));
    }
    let cut = env.ed.active_cut(env.core);
    if let Some((id, frame, pcut)) = ibp::current(env.core).map(|p| (p.id, p.frame, p.cut.clone())) {
        if env.ed.ibp.seen != Some(id) {
            env.ed.ibp.seen = Some(id);
            if cut.as_ref() == Some(&pcut) && env.ed.frame != frame {
                env.ed.frame = frame;
            }
        }
    }
}

/// Cel → canvas no quadro alvo com a transformação identidade do candidato (R16 REQ-U02).
fn placement_of(project: &aiai_core::Project, p: &InbetweenProposal) -> Option<aiai_paint::Affine> {
    let l = cmds::layer_data(project, &p.layer)?;
    let (w, h) = cmds::canvas_size(project, &p.cut);
    Some(aiai_render::compose::layer_placement_in(project, l, [0.0, 0.0, 1.0, 0.0], p.frame, w, h))
}

/// Escala média da transformação (raio do traço no canvas).
fn scale_of(a: &aiai_paint::Affine) -> f32 {
    let m = a.0;
    (m[0] * m[4] - m[1] * m[3]).abs().sqrt()
}

fn to_canvas(a: &aiai_paint::Affine, x: f32, y: f32) -> Vec2 {
    let (cx, cy) = a.apply(x, y);
    Vec2::new(cx, cy)
}

/// Geometria da prévia já amostrada e levada ao canvas: não muda enquanto a proposta vive (qualquer mudança no projeto
/// derruba a proposta), então é feita uma vez por proposta em vez de a cada quadro (E1: reamostrar 380 curvas por quadro
/// custava ~11 ms no desenho no limite).
pub(crate) struct Geom {
    id: u64,
    a: Vec<(Id, Vec<Vec2>)>,
    b: Vec<(Id, Vec<Vec2>)>,
    cand: Vec<(Vec<Vec2>, Vec<f32>)>,
    links: Vec<(Vec2, Vec2, Support, bool)>,
}

/// Monta (ou reaproveita) a geometria da proposta viva neste corte.
fn ensure_geom(env: &mut Env, cut: &Id) -> bool {
    let Some(p) = ibp::current(env.core).filter(|p| &p.cut == cut) else {
        env.ed.ibp.geom = None;
        return false;
    };
    if env.ed.ibp.geom.as_ref().is_some_and(|g| g.id == p.id) {
        return true;
    }
    let project = &env.core.project;
    let Some(a) = placement_of(project, p) else { return false };
    let k = scale_of(&a);
    let line = |s: &Stroke| -> Vec<Vec2> { aiai_paint::sample_stroke(s).iter().map(|q| to_canvas(&a, q.x, q.y)).collect() };
    let cel = |id: &Id| -> Vec<(Id, Vec<Vec2>)> {
        match project.node(id).map(|n| &n.data) {
            Some(NodeKind::Cel(c)) => c.strokes.iter().map(|s| (s.id.clone(), line(s))).collect(),
            _ => Vec::new(),
        }
    };
    let cand = p
        .candidate
        .iter()
        .map(|s| {
            let sm = aiai_paint::sample_stroke(s);
            (sm.iter().map(|q| to_canvas(&a, q.x, q.y)).collect(), sm.iter().map(|q| q.r * k).collect())
        })
        .collect();
    let links = p
        .pairs
        .iter()
        .filter_map(|pv| match (pv.a_center, pv.b_center) {
            (Some(ca), Some(cb)) => Some((to_canvas(&a, ca[0], ca[1]), to_canvas(&a, cb[0], cb[1]), pv.support, pv.manual)),
            _ => None,
        })
        .collect();
    let g = Geom { id: p.id, a: cel(&p.cel_a), b: cel(&p.cel_b), cand, links };
    env.ed.ibp.geom = Some(g);
    true
}

/// Traço de largura variável numa malha só (trapézios + leque nas juntas que dobram e nas pontas), tela em pontos.
fn mesh_line(mesh: &mut egui::Mesh, sp: &[Pos2], sr: &[f32], color: Color32) {
    let fan = |mesh: &mut egui::Mesh, c: Pos2, r: f32| {
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(c, color);
        const N: u32 = 10;
        for i in 0..=N {
            let t = i as f32 / N as f32 * std::f32::consts::TAU;
            mesh.colored_vertex(c + egui::vec2(t.cos(), t.sin()) * r, color);
        }
        for i in 0..N {
            mesh.add_triangle(base, base + 1 + i, base + 2 + i);
        }
    };
    for i in 0..sp.len().saturating_sub(1) {
        let (a, b) = (sp[i], sp[i + 1]);
        let d = b - a;
        let len = d.length();
        if len < 1e-3 {
            continue;
        }
        let n = egui::vec2(-d.y, d.x) / len;
        let base = mesh.vertices.len() as u32;
        for q in [a + n * sr[i], a - n * sr[i], b - n * sr[i + 1], b + n * sr[i + 1]] {
            mesh.colored_vertex(q, color);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    for (i, (p, r)) in sp.iter().zip(sr).enumerate() {
        let joint = i > 0 && i + 1 < sp.len() && {
            let (u, v) = (sp[i] - sp[i - 1], sp[i + 1] - sp[i]);
            u.length() > 1e-3 && v.length() > 1e-3 && u.normalized().dot(v.normalized()) < 0.94
        };
        if i == 0 || i + 1 == sp.len() || joint {
            fan(mesh, *p, *r);
        }
    }
}

/// Desenha a prévia por cima do canvas (só visão).
pub(crate) fn paint(env: &mut Env, painter: &egui::Painter, xf: &Xf, cut: &Id) {
    let Some((frame, stale)) = ibp::current(env.core).filter(|p| &p.cut == cut).map(|p| (p.frame, p.stale)) else {
        env.ed.ibp.geom = None;
        return;
    };
    if frame != env.ed.frame || !ensure_geom(env, cut) {
        return;
    }
    let ui = &env.ed.ibp;
    let Some(g) = ui.geom.as_ref() else { return };
    // as chaves, finas, para ver e escolher os pares
    let thin = |pts: &[Vec2], color: Color32, w: f32| {
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(pts.iter().map(|q| xf.to_screen(*q)).collect(), egui::Stroke::new(w, color)));
        }
    };
    for (id, pts) in &g.a {
        let picked = ui.pick_a.as_ref() == Some(id) || ui.selected.as_ref().is_some_and(|x| &x.0 == id);
        thin(pts, if picked { Color32::from_rgb(255, 120, 60) } else { KEY_A }, if picked { 3.0 } else { 1.2 });
    }
    for (id, pts) in &g.b {
        let picked = ui.selected.as_ref().is_some_and(|x| &x.1 == id);
        thin(pts, if picked { Color32::from_rgb(60, 220, 120) } else { KEY_B }, if picked { 3.0 } else { 1.2 });
    }
    // o candidato como a linha vai ficar (largura do traço), numa malha só
    let color = if stale { PREVIEW.gamma_multiply(0.45) } else { PREVIEW };
    let mut mesh = egui::Mesh::default();
    for (pts, radii) in &g.cand {
        let sp: Vec<Pos2> = pts.iter().map(|q| xf.to_screen(*q)).collect();
        let sr: Vec<f32> = radii.iter().map(|r| (r * xf.scale).max(0.5)).collect();
        mesh_line(&mut mesh, &sp, &sr, color);
    }
    painter.add(egui::Shape::mesh(mesh));
    // ligações dos pares
    for (ca, cb, support, manual) in &g.links {
        let (sa, sb) = (xf.to_screen(*ca), xf.to_screen(*cb));
        let c = match support {
            Support::Refuse => REFUSE,
            Support::Approximate => APPROX,
            Support::Preserve => Color32::from_gray(200),
        };
        painter.line_segment([sa, sb], egui::Stroke::new(if *manual { 2.2 } else { 1.0 }, c));
        painter.circle_filled(sa, 2.5, c);
        painter.circle_stroke(sb, 3.0, egui::Stroke::new(1.0, c));
    }
}

/// Traço mais perto de `q` (canvas) dentro de `tol`.
fn nearest_in(lines: &[(Id, Vec<Vec2>)], q: Vec2, tol: f32) -> Option<Id> {
    lines
        .iter()
        .filter(|(id, _)| !id.0.is_empty())
        .map(|(id, pts)| (pts.iter().map(|p| (*p - q).length()).fold(f32::INFINITY, f32::min), id))
        .filter(|(d, _)| *d <= tol)
        .min_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|x| x.1.clone())
}

/// Clique no canvas com "Corrigir pares" ligado: A e depois B = par manual; clique num traço de A que já tem par manual
/// escolhe o par (Delete desfaz). `true` = o clique foi da prévia (não desenha).
pub(crate) fn pick(env: &mut Env, canvas_pos: Vec2, scale: f32) -> bool {
    if !env.ed.ibp.pairs_mode {
        return false;
    }
    let Some(cut) = env.ed.active_cut(env.core) else { return false };
    let Some(p) = current_for(env, &cut) else {
        env.ed.ibp.pairs_mode = false;
        return false;
    };
    if !ensure_geom(env, &cut) {
        return true;
    }
    let tol = 12.0 / scale.max(1e-3);
    let (near_a, near_b) = {
        let g = env.ed.ibp.geom.as_ref().expect("montada");
        (nearest_in(&g.a, canvas_pos, tol), nearest_in(&g.b, canvas_pos, tol))
    };
    match env.ed.ibp.pick_a.clone() {
        None => {
            let Some(sa) = near_a else {
                env.ed.ibp.selected = None;
                env.ed.notify(tr("ibp.pick_a").to_owned());
                return true;
            };
            env.ed.ibp.selected = p.request.manual_pairs.iter().find(|m| m.0 == sa).cloned();
            env.ed.ibp.pick_a = Some(sa);
            env.ed.notify(tr("ibp.pick_b").to_owned());
        }
        Some(sa) => {
            let Some(sb) = near_b else {
                env.ed.notify(tr("ibp.pick_b").to_owned());
                return true;
            };
            let mut pairs: Vec<(Id, Id)> = p.request.manual_pairs.iter().filter(|m| m.0 != sa && m.1 != sb).cloned().collect();
            pairs.push((sa.clone(), sb.clone()));
            env.ed.ibp.pick_a = None;
            env.ed.ibp.selected = Some((sa, sb));
            if let Err(e) = ibp::set_manual_pairs(env.core, p.id, pairs) {
                env.ed.notify(e.to_string());
            }
        }
    }
    true
}

fn remove_selected(env: &mut Env, p: &InbetweenProposal) {
    let Some(sel) = env.ed.ibp.selected.take() else { return };
    let pairs: Vec<(Id, Id)> = p.request.manual_pairs.iter().filter(|m| **m != sel).cloned().collect();
    env.ed.ibp.pick_a = None;
    if let Err(e) = ibp::set_manual_pairs(env.core, p.id, pairs) {
        env.ed.notify(e.to_string());
    }
}

fn notice_rows(ui: &mut egui::Ui, items: &[ibp::Notice], color: Color32, max: usize) {
    for n in items.iter().take(max) {
        ui.label(RichText::new(format!("• {}", n.text)).size(11.0).color(color));
    }
    if items.len() > max {
        ui.label(RichText::new(format!("… +{}", items.len() - max)).size(11.0).color(color));
    }
}

/// Folga da barra até o pé do canvas (a leitura de coordenadas fica à mostra).
const BAR_GAP: f32 = 34.0;

/// A prévia caiu (o projeto mudou): o motivo e "Gerar de novo" (sem recalcular sozinho, R10 H-3).
fn dropped_bar(env: &mut Env, ctx: &egui::Context, rect: Rect, cut: &Id) {
    let Some((why, req)) = ibp::invalidated(env.core).filter(|(_, r)| &r.cut == cut).cloned() else { return };
    let t = env.t;
    let mut act: Option<&'static str> = None;
    let w = (rect.width() - 24.0).clamp(240.0, 760.0);
    let area = egui::Area::new(egui::Id::new("ibp_dropped"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(rect.center().x - w / 2.0, rect.bottom() - BAR_GAP))
        .pivot(egui::Align2::LEFT_BOTTOM)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(t.bg_panel).show(ui, |ui| {
                ui.set_width(w);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tr("ibp.title")).strong().color(PREVIEW));
                    ui.label(RichText::new(format!("{}: {why}", tr("ibp.dropped"))).size(11.5).color(t.text_dim));
                    let r = ui.button(tr("ibp.again"));
                    env.note("ibp:again", r.rect);
                    if r.clicked() {
                        act = Some("again");
                    }
                    let r = ui.button(tr("ibp.close"));
                    env.note("ibp:close", r.rect);
                    if r.clicked() {
                        act = Some("close");
                    }
                });
            });
        });
    env.note("ibp:dropped", area.response.rect);
    match act {
        Some("again") => {
            // mesmo quadro e mesmas opções; os pares manuais voltam só se os traços ainda existem (senão o erro diz)
            request_preview(env, &req);
        }
        Some("close") => ibp::clear_invalidated(env.core),
        _ => {}
    }
}

/// Barra da prévia no rodapé do canvas (some sem prévia).
pub(crate) fn bar(env: &mut Env, ctx: &egui::Context, rect: Rect, cut: &Id) {
    let Some(full) = ibp::current(env.core).filter(|p| &p.cut == cut) else {
        env.ed.ibp.pairs_mode = false;
        dropped_bar(env, ctx, rect, cut);
        return;
    };
    let t = env.t;
    let layer_name = env.core.project.node(&full.layer).map(|n| n.name.clone()).unwrap_or_default();
    let locked = cmds::layer_data(&env.core.project, &full.layer).is_some_and(|l| l.locked);
    let blocked: Option<String> = match full.acceptable() {
        Err(e) => Some(e),
        Ok(()) if locked => Some(tr("ibp.locked").to_owned()),
        Ok(()) => None,
    };
    let label = ibp::accept_scope(&env.core.project, full).map(|s| s.button_text(&layer_name)).unwrap_or_else(|e| e.to_string());
    let p = full.light();
    let mut act: Option<&'static str> = None;
    let w = (rect.width() - 24.0).clamp(240.0, 760.0);
    let area = egui::Area::new(egui::Id::new("ibp_bar"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(rect.center().x - w / 2.0, rect.bottom() - BAR_GAP))
        .pivot(egui::Align2::LEFT_BOTTOM)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(t.bg_panel).show(ui, |ui| {
                ui.set_width(w);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tr("ibp.title")).strong().color(PREVIEW));
                    ui.label(
                        RichText::new(format!("{} {} · {} {}{} → {}{} · t = {:.2}", tr("ibp.frame"), p.frame + 1, tr("ibp.between"), layer_name, p.key_a, layer_name, p.key_b, p.t))
                            .size(11.5)
                            .color(t.text_dim),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    let r = ui.add_enabled(blocked.is_none(), egui::Button::new(RichText::new(&label).strong()));
                    let r = match &blocked {
                        Some(why) => r.on_disabled_hover_text(why.as_str()),
                        None => r,
                    };
                    env.note("ibp:accept", r.rect);
                    if r.clicked() {
                        act = Some("accept");
                    }
                    let r = ui.add_enabled(p.stale, egui::Button::new(tr("ibp.refresh"))).on_hover_text(tr("ibp.refresh_tip"));
                    env.note("ibp:refresh", r.rect);
                    if r.clicked() {
                        act = Some("refresh");
                    }
                    let r = ui.selectable_label(env.ed.ibp.pairs_mode, tr("ibp.pairs")).on_hover_text(tr("ibp.pairs_tip"));
                    env.note("ibp:pairs", r.rect);
                    if r.clicked() {
                        act = Some("pairs");
                    }
                    let r = ui.button(tr("ibp.discard"));
                    env.note("ibp:discard", r.rect);
                    if r.clicked() {
                        act = Some("discard");
                    }
                });
                if let Some(why) = &blocked {
                    ui.label(RichText::new(why).size(11.0).color(if p.refusals.is_empty() { APPROX } else { REFUSE }));
                }
                let (nr, na, nn) = (p.refusals.len(), p.approximations.len(), p.notes.len());
                if nr + na + nn > 0 {
                    let head = format!("{nr} {} · {na} {} · {nn} {}", tr("ibp.refusals"), tr("ibp.warnings"), tr("ibp.notes"));
                    let r = ui.selectable_label(env.ed.ibp.show_notes, RichText::new(head).size(11.0));
                    env.note("ibp:notes", r.rect);
                    if r.clicked() {
                        env.ed.ibp.show_notes = !env.ed.ibp.show_notes;
                    }
                    if env.ed.ibp.show_notes || nr > 0 {
                        egui::ScrollArea::vertical().id_salt("ibp_notes").max_height(120.0).show(ui, |ui| {
                            notice_rows(ui, &p.refusals, REFUSE, 12);
                            if env.ed.ibp.show_notes {
                                notice_rows(ui, &p.approximations, APPROX, 12);
                                notice_rows(ui, &p.notes, t.text_dim, 6);
                            }
                        });
                    }
                }
                if env.ed.ibp.pairs_mode {
                    let msg = if env.ed.ibp.pick_a.is_some() { tr("ibp.pick_b") } else { tr("ibp.pick_a") };
                    ui.label(RichText::new(msg).size(11.0).color(t.text_dim));
                }
            });
        });
    env.note("ibp:bar", area.response.rect);
    if env.ed.ibp.pairs_mode && env.ed.ibp.selected.is_some() && ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
        remove_selected(env, &p);
    }
    match act {
        Some("accept") => match ibp::accept(env.core, p.id, super::doc::AUTHOR) {
            Ok(a) => {
                let perf = std::mem::take(&mut env.ed.ibp.perf);
                env.ed.ibp = IbpUi { perf, ..Default::default() };
                env.ed.notify(a.outcome.description);
            }
            Err(e) => env.ed.notify(e.to_string()),
        },
        Some("refresh") => {
            if let Err(e) = ibp::refresh(env.core, p.id) {
                env.ed.notify(e.to_string());
            }
        }
        Some("pairs") => {
            env.ed.ibp.pairs_mode = !env.ed.ibp.pairs_mode;
            env.ed.ibp.pick_a = None;
        }
        Some("discard") => {
            let _ = ibp::discard(env.core, p.id);
            let perf = std::mem::take(&mut env.ed.ibp.perf);
            env.ed.ibp = IbpUi { perf, ..Default::default() };
        }
        _ => {}
    }
}

/// Botão do comando no quadro atual da camada ativa (painel da mesa de luz).
pub(crate) fn command_button(env: &mut Env, ui: &mut egui::Ui, name: &str) -> bool {
    let here = request_here(env);
    command_for(env, ui, name, here)
}

/// Pedido para uma célula da linha do tempo (camada e quadro clicados).
pub(crate) fn request_at(env: &Env, cut: &Id, layer: &Id, frame: u32) -> Result<InbetweenPreviewRequest, String> {
    if !cmds::layer_data(&env.core.project, layer).is_some_and(|l| l.kind == LayerType::Vector) {
        return Err(tr("ibp.need_vector").to_owned());
    }
    let req = InbetweenPreviewRequest::new(cut.clone(), layer.clone(), frame);
    ibp::resolve_frames(&env.core.project, &req).map_err(|e| e.to_string())?;
    Ok(req)
}

/// Botão do comando: ligado só entre dois blocos-chave da camada (senão mostra o motivo).
pub(crate) fn command_for(env: &mut Env, ui: &mut egui::Ui, name: &str, here: Result<InbetweenPreviewRequest, String>) -> bool {
    let r = ui.add_enabled(here.is_ok(), egui::Button::new(tr("ibp.command")));
    let r = match &here {
        Ok(_) => r.on_hover_text(tr("ibp.command_tip")),
        Err(why) => r.on_disabled_hover_text(why.as_str()),
    };
    env.note(name, r.rect);
    if r.clicked() {
        if let Ok(req) = here {
            request_preview(env, &req);
        }
        return true;
    }
    false
}
