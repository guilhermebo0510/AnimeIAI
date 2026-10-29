//! Prévia de intercalação de um quadro na interface (R16 §7). A proposta mora na sessão (`CoreSession`), a mesma que o
//! MCP usa: uma prévia pedida pelo agente aparece aqui também (modo diretor), e aceitar por aqui passa pelas mesmas
//! barreiras. Só visão até o Aceitar: nada entra no projeto, no render nem na exportação.
//!
//! * Sobreposição (só no quadro da proposta): o quadro esmaecido, as chaves A (vermelho) e B (verde) finas, o candidato
//!   na cor da prévia e uma linha entre os traços de cada correspondência, tudo projetado como o cel aceito vai aparecer
//!   (movimento da camada no quadro, transformação identidade).
//! * Barra: Aceitar (com o alcance escrito; desligado com o motivo), Atualizar, Corrigir pares, opções do motor,
//!   Descartar, e a lista inteira de recusas/avisos (clique localiza o traço). Sem atalho de teclado.
//! * Corrigir pares: clique num traço de A e depois num de B; clique no traço de A de um par manual e Delete desfaz.
//!   A tela mostra na hora o par novo (tracejado) e apaga a ligação que ele substitui.

use super::commands as cmds;
use super::editor::Xf;
use super::ui_kit::Env;
use crate::i18n::tr;
use aiai_anim::MotionMode;
use aiai_core::{Id, LayerType, NodeKind, Stroke};
use aiai_tools::inbetween_preview::{self as ibp, InbetweenPreviewRequest, InbetweenProposal, Notice, PairKind, Support};
use egui::{Color32, Pos2, Rect, RichText, Vec2};

/// Cor da prévia (a mesma do render com prévia da bridge; diferente do vermelho/verde da mesa de luz).
pub(crate) const PREVIEW: Color32 = Color32::from_rgb(224, 64, 250);
const KEY_A: Color32 = Color32::from_rgba_premultiplied(150, 40, 34, 150);
const KEY_B: Color32 = Color32::from_rgba_premultiplied(24, 112, 50, 150);
const REFUSE: Color32 = Color32::from_rgb(230, 60, 50);
const APPROX: Color32 = Color32::from_rgb(222, 160, 40);
const FOUND: Color32 = Color32::from_rgb(255, 200, 0);
/// Pontos que a sobreposição monta no máximo quando a proposta passou do teto (chaves simplificadas, com aviso).
const SIMPLIFIED_BUDGET: usize = 100_000;

/// Estado da interface da prévia (o que é só da tela; a proposta fica na sessão).
pub(crate) struct IbpUi {
    /// Cliques no canvas corrigem pares em vez de desenhar.
    pub pairs_mode: bool,
    /// Traço de A escolhido, esperando o de B.
    pub pick_a: Option<Id>,
    /// Par manual escolhido (Delete desfaz).
    pub selected: Option<(Id, Id)>,
    /// Lista de avisos aberta.
    pub show_notes: bool,
    /// Opções do motor abertas na barra.
    pub show_options: bool,
    /// Esmaecer o quadro por baixo da prévia (o desenho exposto hoje não compete com a proposta, A6-06).
    pub veil: bool,
    /// Traços apontados por um aviso clicado (destacados no canvas).
    pub highlight: Option<(Option<Id>, Option<Id>)>,
    /// Última proposta vista: uma prévia nova pedida pelo MCP leva a tela ao quadro dela (modo diretor).
    pub seen: Option<u64>,
    /// E1 (R16 §11): amostras brutas por quadro, só enquanto gravando (`UI_ACTION perf_samples`).
    pub perf: Perf,
    /// Geometria da proposta viva, amostrada uma vez.
    pub geom: Option<Geom>,
    /// E1 (CX12 C12-04): (proposta, quando o pedido foi atendido, já desenhada neste quadro). Fechado no fim do quadro
    /// em que a sobreposição dela foi desenhada pela primeira vez.
    pub present: Option<(u64, std::time::Instant, bool)>,
}

impl Default for IbpUi {
    fn default() -> Self {
        IbpUi { pairs_mode: false, pick_a: None, selected: None, show_notes: false, show_options: false, veil: true, highlight: None, seen: None, perf: Perf::default(), geom: None, present: None }
    }
}

impl IbpUi {
    /// Volta ao estado sem prévia, guardando o que é preferência ou medida.
    fn reset(&mut self) {
        let (perf, veil, seen, show_notes) = (std::mem::take(&mut self.perf), self.veil, self.seen, self.show_notes);
        *self = IbpUi { perf, veil, seen, show_notes, ..Default::default() };
    }

    /// Fim do quadro: se a sobreposição da proposta medida foi desenhada nele, registra pedido → imagem (ms).
    pub(crate) fn end_frame(&mut self) {
        if let Some((_, t0, true)) = self.present {
            self.perf.push_present(t0.elapsed().as_secs_f32() * 1000.0);
            self.present = None;
        }
    }
}

/// Amostras brutas de tempo (ms) por quadro: a interface inteira (CPU de montar o quadro) e o desenho da prévia.
#[derive(Default)]
pub(crate) struct Perf {
    pub on: bool,
    pub ui_ms: Vec<f32>,
    pub paint_ms: Vec<f32>,
    /// Pedido atendido → fim do quadro (CPU) que desenhou a sobreposição da proposta pela primeira vez.
    pub present_ms: Vec<f32>,
}

impl Perf {
    const CAP: usize = 50_000;

    pub(crate) fn push_ui(&mut self, ms: f32) {
        if self.on && self.ui_ms.len() < Self::CAP {
            self.ui_ms.push(ms);
        }
    }

    pub(crate) fn push_present(&mut self, ms: f32) {
        if self.on && self.present_ms.len() < Self::CAP {
            self.present_ms.push(ms);
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
    request_at(env, &cut, &layer, env.ed.frame)
}

/// Pedido para uma célula (camada e quadro): o motivo quando não dá.
pub(crate) fn request_at(env: &Env, cut: &Id, layer: &Id, frame: u32) -> Result<InbetweenPreviewRequest, String> {
    if !cmds::layer_data(&env.core.project, layer).is_some_and(|l| l.kind == LayerType::Vector) {
        return Err(tr("ibp.need_vector").to_owned());
    }
    let req = InbetweenPreviewRequest::new(cut.clone(), layer.clone(), frame);
    ibp::resolve_frames(&env.core.project, &req).map_err(|e| e.to_string())?;
    Ok(req)
}

/// Pede a prévia (substitui a anterior); a tela vai ao quadro e à camada dela.
pub(crate) fn request_preview(env: &mut Env, req: &InbetweenPreviewRequest) {
    env.ed.ibp.pick_a = None;
    env.ed.ibp.selected = None;
    env.ed.ibp.highlight = None;
    env.ed.frame = req.frame;
    env.ed.layer = Some(req.layer.clone());
    match ibp::preview(env.core, req) {
        Ok(p) => {
            env.ed.ibp.seen = Some(p.id);
            let msg = if p.refusals.is_empty() { format!("{} ({} {})", tr("ibp.ready"), p.approximations.len(), tr("ibp.warnings")) } else { format!("{}: {} {}", tr("ibp.ready"), p.refusals.len(), tr("ibp.refusals")) };
            env.ed.notify(msg);
        }
        Err(e) => env.ed.notify(format!("{}: {e}", tr("ibp.title"))),
    }
}

/// Proposta viva no corte mostrado (cópia leve: sem candidato nem pares; o aceite usa a proposta guardada na sessão).
fn current_for(env: &Env, cut: &Id) -> Option<InbetweenProposal> {
    ibp::current(env.core).filter(|p| &p.cut == cut).map(InbetweenProposal::light)
}

/// Há prévia viva neste corte?
pub(crate) fn current_here(env: &Env, cut: &Id) -> bool {
    ibp::current(env.core).is_some_and(|p| &p.cut == cut)
}

/// Uma vez por quadro: o projeto mudou depois da prévia → ela some com o motivo (sem recalcular). Uma prévia nova vinda
/// do MCP leva a tela ao quadro e à camada dela.
pub(crate) fn tick(env: &mut Env) {
    if let Some(why) = ibp::invalidate_if_changed(env.core) {
        env.ed.ibp.pairs_mode = false;
        env.ed.ibp.pick_a = None;
        env.ed.notify(format!("{}: {why}", tr("ibp.invalidated")));
    }
    let cut = env.ed.active_cut(env.core);
    if let Some((id, frame, pcut, layer)) = ibp::current(env.core).map(|p| (p.id, p.frame, p.cut.clone(), p.layer.clone())) {
        if env.ed.ibp.seen != Some(id) {
            env.ed.ibp.seen = Some(id);
            // pedida com a tela em outro corte (modo diretor pelo MCP): a tela vai até o corte dela; sem isso o pedido
            // dava certo e nada aparecia (CX16 C16-09)
            if cut.as_ref() != Some(&pcut) {
                env.ed.set_cut(pcut);
            }
            env.ed.frame = frame;
            env.ed.layer = Some(layer);
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

/// Traço de uma chave na sobreposição: pontos no canvas e centro (média dos pontos de controle, a mesma conta da
/// proposta: a ponta da ligação não salta ao Atualizar).
pub(crate) struct GLine {
    id: Id,
    pts: Vec<Vec2>,
    center: Vec2,
}

/// Ligação de uma correspondência calculada.
struct Link {
    a: Option<Id>,
    b: Option<Id>,
    ca: Vec2,
    cb: Vec2,
    support: Support,
    manual: bool,
    kind: PairKind,
}

/// Geometria da prévia já amostrada e levada ao canvas: não muda enquanto a proposta vive (qualquer mudança no projeto
/// derruba a proposta), então é feita uma vez por proposta em vez de a cada quadro (E1: reamostrar 380 curvas por quadro
/// custava ~11 ms no desenho no limite). Com orçamento de amostras: traço que passaria dele entra pelos pontos de controle.
pub(crate) struct Geom {
    id: u64,
    a: Vec<GLine>,
    b: Vec<GLine>,
    cand: Vec<(Vec<Vec2>, Vec<f32>)>,
    links: Vec<Link>,
    /// As chaves passaram do teto de amostras (a proposta foi recusada por tamanho e não tem candidato): aparecem pelos
    /// pontos de controle, e a barra diz isso.
    pub simplified: bool,
}

/// Amostras reais de um traço no canvas (a mesma geometria do render).
fn sampled(s: &Stroke, a: &aiai_paint::Affine, k: f32) -> (Vec<Vec2>, Vec<f32>) {
    let sm = aiai_paint::sample_stroke(s);
    (sm.iter().map(|q| to_canvas(a, q.x, q.y)).collect(), sm.iter().map(|q| q.r * k).collect())
}

/// Só para chaves de proposta acima do teto: os pontos de controle, no máximo 2 000 por traço, dentro de `budget`.
fn simplified(s: &Stroke, a: &aiai_paint::Affine, budget: &mut usize) -> Vec<Vec2> {
    let step = s.points.len().div_ceil(2_000).max(1);
    let n = s.points.len().div_ceil(step);
    if n > *budget {
        return Vec::new();
    }
    *budget -= n;
    s.points.iter().step_by(step).map(|q| to_canvas(a, q.x, q.y)).collect()
}

fn cel_center(s: &Stroke, a: &aiai_paint::Affine) -> Vec2 {
    let n = s.points.len().max(1) as f32;
    let (x, y) = s.points.iter().fold((0.0, 0.0), |acc, q| (acc.0 + q.x, acc.1 + q.y));
    to_canvas(a, x / n, y / n)
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
    let strokes = |id: &Id| -> &[Stroke] {
        match project.node(id).map(|n| &n.data) {
            Some(NodeKind::Cel(c)) => &c.strokes,
            _ => &[],
        }
    };
    let (sa, sb) = (strokes(&p.cel_a), strokes(&p.cel_b));
    // a prévia garante que chaves + candidato cabem no teto (senão recusa e não há candidato): dentro dele tudo é
    // desenhado com as amostras reais; o candidato nunca é trocado por outra geometria (CX11 C11-02)
    let total = sa.iter().chain(sb).chain(&p.candidate).map(aiai_paint::sample_count).fold(0usize, usize::saturating_add);
    let simple = total > ibp::MAX_PREVIEW_SAMPLES;
    let mut budget = SIMPLIFIED_BUDGET;
    let mut lines = |list: &[Stroke]| -> Vec<GLine> {
        list.iter()
            .map(|s| GLine { id: s.id.clone(), pts: if simple { simplified(s, &a, &mut budget) } else { sampled(s, &a, k).0 }, center: cel_center(s, &a) })
            .collect()
    };
    let (ga, gb) = (lines(sa), lines(sb));
    let cand = if simple { Vec::new() } else { p.candidate.iter().map(|s| sampled(s, &a, k)).collect() };
    let links = p
        .pairs
        .iter()
        .filter_map(|pv| match (pv.a_center, pv.b_center) {
            (Some(ca), Some(cb)) => Some(Link { a: pv.a.clone(), b: pv.b.clone(), ca: to_canvas(&a, ca[0], ca[1]), cb: to_canvas(&a, cb[0], cb[1]), support: pv.support, manual: pv.manual, kind: pv.kind }),
            _ => None,
        })
        .collect();
    env.ed.ibp.geom = Some(Geom { id: p.id, a: ga, b: gb, cand, links, simplified: simple });
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

/// A ligação calculada ainda vale com os pares manuais pendentes? (antes de Atualizar: um par novo substitui a ligação
/// dos dois traços dele; um par manual solto deixa de valer)
fn link_superseded(l: &Link, pending: &[(Id, Id)]) -> bool {
    if l.kind != PairKind::Pair {
        return false;
    }
    let (Some(a), Some(b)) = (&l.a, &l.b) else { return false };
    let mine = pending.iter().any(|(x, y)| x == a && y == b);
    let touched = pending.iter().any(|(x, y)| x == a || y == b);
    (touched && !mine) || (l.manual && !mine)
}

/// Desenha a prévia por cima do canvas (só visão, só no quadro dela).
pub(crate) fn paint(env: &mut Env, painter: &egui::Painter, xf: &Xf, cut: &Id) {
    let Some((frame, stale, pending)) = ibp::current(env.core).filter(|p| &p.cut == cut).map(|p| (p.frame, p.stale, p.request.manual_pairs.clone())) else {
        env.ed.ibp.geom = None;
        return;
    };
    if frame != env.ed.frame || !ensure_geom(env, cut) {
        return;
    }
    let (cw, ch) = cmds::canvas_size(&env.core.project, cut);
    if let Some((id, _, drawn)) = env.ed.ibp.present.as_mut() {
        if env.ed.ibp.geom.as_ref().is_some_and(|g| g.id == *id) {
            *drawn = true;
        }
    }
    let ui = &env.ed.ibp;
    let Some(g) = ui.geom.as_ref() else { return };
    // o quadro de hoje esmaecido: o desenho exposto não compete com a proposta
    if ui.veil {
        let quad: Vec<Pos2> = [Vec2::new(0.0, 0.0), Vec2::new(cw as f32, 0.0), Vec2::new(cw as f32, ch as f32), Vec2::new(0.0, ch as f32)].iter().map(|v| xf.to_screen(*v)).collect();
        painter.add(egui::Shape::convex_polygon(quad, Color32::from_rgba_unmultiplied(250, 250, 250, 170), egui::Stroke::NONE));
    }
    let hl = ui.highlight.clone().unwrap_or((None, None));
    let thin = |pts: &[Vec2], color: Color32, w: f32| {
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(pts.iter().map(|q| xf.to_screen(*q)).collect(), egui::Stroke::new(w, color)));
        }
    };
    for l in &g.a {
        let picked = ui.pick_a.as_ref() == Some(&l.id) || ui.selected.as_ref().is_some_and(|x| x.0 == l.id);
        let found = hl.0.as_ref() == Some(&l.id);
        thin(&l.pts, if found { FOUND } else if picked { Color32::from_rgb(255, 120, 60) } else { KEY_A }, if picked || found { 3.0 } else { 1.2 });
    }
    for l in &g.b {
        let picked = ui.selected.as_ref().is_some_and(|x| x.1 == l.id);
        let found = hl.1.as_ref() == Some(&l.id);
        thin(&l.pts, if found { FOUND } else if picked { Color32::from_rgb(60, 220, 120) } else { KEY_B }, if picked || found { 3.0 } else { 1.2 });
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
    // ligações calculadas (a que um par pendente substitui fica apagada, tracejada)
    for l in &g.links {
        let (sa, sb) = (xf.to_screen(l.ca), xf.to_screen(l.cb));
        let c = match l.support {
            Support::Refuse => REFUSE,
            Support::Approximate => APPROX,
            Support::Preserve => Color32::from_gray(200),
        };
        if stale && link_superseded(l, &pending) {
            painter.extend(egui::Shape::dashed_line(&[sa, sb], egui::Stroke::new(1.0, c.gamma_multiply(0.35)), 3.0, 4.0));
            continue;
        }
        if l.kind == PairKind::GroupMorph {
            painter.extend(egui::Shape::dotted_line(&[sa, sb], c, 6.0, 1.3));
        } else {
            painter.line_segment([sa, sb], egui::Stroke::new(if l.manual { 2.2 } else { 1.0 }, c));
        }
        painter.circle_filled(sa, 2.5, c);
        painter.circle_stroke(sb, 3.0, egui::Stroke::new(1.0, c));
    }
    // pares manuais ainda não calculados: tracejado grosso, para ver o que acabou de ser escolhido
    if stale {
        let center = |lines: &[GLine], id: &Id| lines.iter().find(|l| &l.id == id).map(|l| l.center);
        for (ia, ib) in &pending {
            if let (Some(ca), Some(cb)) = (center(&g.a, ia), center(&g.b, ib)) {
                let (sa, sb) = (xf.to_screen(ca), xf.to_screen(cb));
                let sel = ui.selected.as_ref() == Some(&(ia.clone(), ib.clone()));
                painter.extend(egui::Shape::dashed_line(&[sa, sb], egui::Stroke::new(if sel { 3.5 } else { 2.5 }, PREVIEW), 7.0, 5.0));
                painter.circle_filled(sa, 4.0, PREVIEW);
                painter.circle_filled(sb, 4.0, PREVIEW);
            }
        }
    }
}

/// Traço mais perto de `q` (canvas) dentro de `tol`, com a distância.
fn nearest_in(lines: &[GLine], q: Vec2, tol: f32) -> Option<(Id, f32)> {
    lines
        .iter()
        .filter(|l| !l.id.0.is_empty())
        .map(|l| (l.pts.iter().map(|p| (*p - q).length()).fold(f32::INFINITY, f32::min), &l.id))
        .filter(|(d, _)| *d <= tol)
        .min_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(d, id)| (id.clone(), d))
}

/// Clique no canvas com "Corrigir pares" ligado, no quadro da prévia (A7-02):
/// * sem traço de A escolhido: clicar num traço de A escolhe (e, se ele tem par manual, escolhe o par: Delete solta);
/// * com A escolhido: o traço mais perto decide — de B forma o par; de A troca a escolha; no vazio cancela.
///
/// `true` = o clique foi da prévia (não desenha). Fora do quadro da prévia nada é interceptado (CX10 C10-04 / A6-04).
pub(crate) fn pick(env: &mut Env, canvas_pos: Vec2, scale: f32) -> bool {
    if !env.ed.ibp.pairs_mode {
        return false;
    }
    let Some(cut) = env.ed.active_cut(env.core) else { return false };
    let Some(p) = current_for(env, &cut) else {
        env.ed.ibp.pairs_mode = false;
        return false;
    };
    if p.frame != env.ed.frame || !ensure_geom(env, &cut) {
        return false;
    }
    let tol = 12.0 / scale.max(1e-3);
    let (near_a, near_b) = {
        let g = env.ed.ibp.geom.as_ref().expect("montada");
        (nearest_in(&g.a, canvas_pos, tol), nearest_in(&g.b, canvas_pos, tol))
    };
    let choose_a = |env: &mut Env, sa: Id| {
        env.ed.ibp.selected = p.request.manual_pairs.iter().find(|m| m.0 == sa).cloned();
        env.ed.ibp.pick_a = Some(sa);
        env.ed.notify(tr("ibp.pick_b").to_owned());
    };
    match env.ed.ibp.pick_a.clone() {
        None => match near_a {
            Some((sa, _)) => choose_a(env, sa),
            None => {
                env.ed.ibp.selected = None;
                env.ed.notify(tr("ibp.pick_a").to_owned());
            }
        },
        Some(sa) => {
            let b_wins = match (&near_a, &near_b) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some((_, da)), Some((_, db))) => db <= da,
            };
            if b_wins {
                let sb = near_b.expect("b").0;
                let mut pairs: Vec<(Id, Id)> = p.request.manual_pairs.iter().filter(|m| m.0 != sa && m.1 != sb).cloned().collect();
                pairs.push((sa.clone(), sb.clone()));
                env.ed.ibp.pick_a = None;
                match ibp::set_manual_pairs(env.core, p.id, pairs) {
                    Ok(()) => env.ed.ibp.selected = Some((sa, sb)),
                    // par recusado na hora (fechado com aberto, linha de cor...): os pares de antes ficam
                    Err(e) => {
                        env.ed.ibp.selected = None;
                        env.ed.notify(format!("{}: {e}", tr("ibp.pair_refused")));
                    }
                }
            } else if let Some((other, _)) = near_a {
                choose_a(env, other);
            } else {
                env.ed.ibp.pick_a = None;
                env.ed.ibp.selected = None;
                env.ed.notify(tr("ibp.pick_a").to_owned());
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

/// Esc com uma escolha de par em andamento: cancela a escolha (nunca descarta a prévia). Chamado antes dos atalhos
/// gerais do editor, que também usam Esc. `true` = a tecla era da prévia.
pub(crate) fn escape_pick(env: &mut Env, ctx: &egui::Context) -> bool {
    if !(env.ed.ibp.pairs_mode && (env.ed.ibp.pick_a.is_some() || env.ed.ibp.selected.is_some())) || ctx.egui_wants_keyboard_input() {
        return false;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        env.ed.ibp.pick_a = None;
        env.ed.ibp.selected = None;
        return true;
    }
    false
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
        // mesmo quadro e mesmas opções; os pares manuais voltam só se os traços ainda existem (senão o erro diz)
        Some("again") => request_preview(env, &req),
        Some("close") => ibp::clear_invalidated(env.core),
        _ => {}
    }
}

/// Lista inteira de recusas, avisos e observações (virtualizada); clicar numa linha destaca os traços dela (C10-05).
fn notices_list(env: &mut Env, ui: &mut egui::Ui, p: &InbetweenProposal) {
    let t = env.t;
    let rows: Vec<(Color32, &Notice)> = p
        .refusals
        .iter()
        .map(|n| (REFUSE, n))
        .chain(p.approximations.iter().map(|n| (APPROX, n)))
        .chain(p.notes.iter().map(|n| (t.text_dim, n)))
        .collect();
    let h = 16.0;
    egui::ScrollArea::vertical().id_salt("ibp_notes").max_height(150.0).auto_shrink([false, true]).show_rows(ui, h, rows.len(), |ui, range| {
        for i in range {
            let (c, n) = rows[i];
            let target = (n.stroke_a.clone(), n.stroke_b.clone());
            let on = env.ed.ibp.highlight.as_ref() == Some(&target);
            let r = ui.selectable_label(on, RichText::new(format!("• {}", n.text)).size(11.0).color(c)).on_hover_text(tr("ibp.locate_tip"));
            if i < 3 {
                env.note(format!("ibp:notice.{i}"), r.rect);
            }
            if r.clicked() {
                env.ed.ibp.highlight = if on || (target.0.is_none() && target.1.is_none()) { None } else { Some(target) };
            }
        }
    });
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
    // a mesma regra do MCP e do aceite (trava da camada ou de uma pasta acima, recusa, desatualizada)
    let blocked = ibp::blocked_reason(&env.core.project, full);
    let label = ibp::accept_scope(&env.core.project, full).map(|s| s.button_text(&layer_name)).unwrap_or_else(|e| e.to_string());
    let p = full.light();
    let on_frame = p.frame == env.ed.frame;
    if !on_frame {
        // fora do quadro da prévia não se corrige par (o que não está à mostra não é escolhido)
        env.ed.ibp.pairs_mode = false;
        env.ed.ibp.pick_a = None;
    }
    let (mut motion, mut bend, mut max_cost) = (p.request.motion, p.request.bend, p.request.max_cost);
    // só uma edição do usuário muda as opções (abrir a área não corrige valores que vieram do MCP, CX11 C11-05)
    let mut edited = false;
    let mut act: Option<&'static str> = None;
    // como o Aceitar foi acionado (CX11 C11-01): clique do ponteiro, Enter/Espaço com o botão em foco, ou acessibilidade
    let mut accept_src = aiai_core::InputSource::UnknownLegacy;
    let (ptr_src, key_src) = env.ed.input_src;
    let simplified = env.ed.ibp.geom.as_ref().is_some_and(|g| g.id == p.id && g.simplified);
    let w = (rect.width() - 24.0).clamp(240.0, 820.0);
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
                    if !on_frame {
                        ui.label(RichText::new(format!("· {} {}", tr("ibp.off_frame"), env.ed.frame + 1)).size(11.5).color(APPROX));
                        let r = ui.button(format!("{} {}", tr("ibp.go_frame"), p.frame + 1));
                        env.note("ibp:go", r.rect);
                        if r.clicked() {
                            act = Some("go");
                        }
                    }
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
                        accept_src = if r.clicked_by(egui::PointerButton::Primary) {
                            ptr_src
                        } else if ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space)) {
                            // só os eventos Enter/Espaço deste quadro decidem; os dois lados juntos = desconhecida
                            key_src
                        } else {
                            aiai_core::InputSource::UnknownLegacy
                        };
                    }
                    let r = ui.add_enabled(p.stale, egui::Button::new(tr("ibp.refresh"))).on_hover_text(tr("ibp.refresh_tip"));
                    env.note("ibp:refresh", r.rect);
                    if r.clicked() {
                        act = Some("refresh");
                    }
                    let r = ui.add_enabled(on_frame, egui::Button::selectable(env.ed.ibp.pairs_mode, tr("ibp.pairs"))).on_hover_text(tr("ibp.pairs_tip"));
                    env.note("ibp:pairs", r.rect);
                    if r.clicked() {
                        act = Some("pairs");
                    }
                    let r = ui.selectable_label(env.ed.ibp.show_options, tr("ibp.options")).on_hover_text(tr("ibp.options_tip"));
                    env.note("ibp:options", r.rect);
                    if r.clicked() {
                        env.ed.ibp.show_options = !env.ed.ibp.show_options;
                    }
                    let r = ui.checkbox(&mut env.ed.ibp.veil, tr("ibp.veil")).on_hover_text(tr("ibp.veil_tip"));
                    env.note("ibp:veil", r.rect);
                    let r = ui.button(tr("ibp.discard"));
                    env.note("ibp:discard", r.rect);
                    if r.clicked() {
                        act = Some("discard");
                    }
                });
                if env.ed.ibp.show_options {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(tr("ibp.motion")).size(11.5));
                        for (m, k, name) in [(MotionMode::Global, "ibp.motion.global", "global"), (MotionMode::PerStroke, "ibp.motion.per_stroke", "per_stroke"), (MotionMode::Linear, "ibp.motion.linear", "linear")] {
                            let r = ui.selectable_label(motion == m, tr(k));
                            env.note(format!("ibp:motion.{name}"), r.rect);
                            if r.clicked() && motion != m {
                                motion = m;
                                edited = true;
                            }
                        }
                        ui.label(RichText::new(tr("ibp.bend")).size(11.5));
                        // mesmo domínio do núcleo (−2…2; custo ≥ 0), sem prender valores existentes
                        let r = ui.add(egui::Slider::new(&mut bend, -2.0..=2.0).step_by(0.05).max_decimals(2).clamping(egui::SliderClamping::Never)).on_hover_text(tr("ibp.bend_tip"));
                        edited |= r.changed();
                        env.note("ibp:bend", r.rect);
                        ui.label(RichText::new(tr("ibp.max_cost")).size(11.5));
                        let r = ui.add(egui::DragValue::new(&mut max_cost).range(0.0..=100_000.0).clamp_existing_to_range(false).speed(2.0).suffix(" px")).on_hover_text(tr("ibp.max_cost_tip"));
                        edited |= r.changed();
                        env.note("ibp:max_cost", r.rect);
                    });
                }
                if let Some(why) = &blocked {
                    ui.label(RichText::new(why).size(11.0).color(if p.refusals.is_empty() { APPROX } else { REFUSE }));
                }
                if simplified {
                    ui.label(RichText::new(tr("ibp.simplified")).size(11.0).color(APPROX));
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
                        notices_list(env, ui, &p);
                    }
                }
                if env.ed.ibp.pairs_mode {
                    let msg = if env.ed.ibp.pick_a.is_some() { tr("ibp.pick_b") } else { tr("ibp.pick_a") };
                    ui.label(RichText::new(format!("{msg} · {}", tr("ibp.delete_hint"))).size(11.0).color(t.text_dim));
                }
            });
        });
    env.note("ibp:bar", area.response.rect);
    if edited && (motion, bend, max_cost) != (p.request.motion, p.request.bend, p.request.max_cost) {
        if let Err(e) = ibp::set_options(env.core, p.id, motion, bend, max_cost) {
            env.ed.notify(e.to_string());
        }
    }
    // consome a tecla: senão o Delete da seleção do canvas (que roda depois) apagaria o conteúdo selecionado
    if env.ed.ibp.pairs_mode
        && env.ed.ibp.selected.is_some()
        && !ctx.egui_wants_keyboard_input()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete) || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace))
    {
        remove_selected(env, &p);
    }
    match act {
        Some("accept") => {
            // a fonte do que acionou o botão, não a do quadro inteiro (CX10 C10-01, CX11 C11-01)
            let prev = std::mem::replace(&mut env.core.source, accept_src);
            let r = ibp::accept(env.core, p.id, super::doc::AUTHOR);
            env.core.source = prev;
            match r {
                Ok(a) => {
                    env.ed.ibp.reset();
                    env.ed.notify(a.outcome.description);
                }
                Err(e) => env.ed.notify(e.to_string()),
            }
        }
        Some("refresh") => {
            env.ed.ibp.highlight = None;
            if let Err(e) = ibp::refresh(env.core, p.id) {
                env.ed.notify(e.to_string());
            }
        }
        Some("pairs") => {
            env.ed.ibp.pairs_mode = !env.ed.ibp.pairs_mode;
            env.ed.ibp.pick_a = None;
        }
        Some("go") => {
            env.ed.frame = p.frame;
            env.ed.layer = Some(p.layer.clone());
        }
        Some("discard") => {
            let _ = ibp::discard(env.core, p.id);
            env.ed.ibp.reset();
        }
        _ => {}
    }
}

/// Botão do comando no quadro atual da camada ativa (painel da mesa de luz).
pub(crate) fn command_button(env: &mut Env, ui: &mut egui::Ui, name: &str) -> bool {
    let here = request_here(env);
    // com o botão desligado, o motivo e o caminho ficam escritos (antes só apareciam com o mouse parado em cima)
    let why = here.as_ref().err().cloned();
    let clicked = command_for(env, ui, name, here);
    if let Some(why) = why {
        let t = env.t;
        let r = ui.label(RichText::new(format!("{} {why}", tr("ibp.now"))).size(11.5).color(t.text_dim));
        env.note(format!("{name}.why"), r.rect);
        ui.label(RichText::new(tr("ibp.how")).size(11.5).color(t.text_dim));
    }
    clicked
}

/// Botão do comando: ligado só entre dois blocos-chave da camada (senão desligado, com o motivo na dica).
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


#[cfg(test)]
mod tests {
    use super::*;
    use aiai_core::command::CellEdit;
    use aiai_core::*;

    fn line(id: &str, dx: f32) -> Stroke {
        let pt = |x: f32, y: f32| StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None };
        Stroke {
            id: Id::from(id),
            points: vec![pt(100.0 + dx, 100.0), pt(140.0 + dx, 90.0), pt(180.0 + dx, 100.0)],
            geometry: StrokeGeometry::Spline,
            tension: 0.0,
            tip: VectorTip::GPen,
            width: 3.0,
            profile: Default::default(),
            color: ColorRef::Direct { rgba: Rgba::BLACK },
            opacity: 1.0,
            closed: false,
            aliased: false,
            color_trace: false,
            group: String::new(),
        }
    }

    /// CX16 C16-09: a tela está no corte 1 e o MCP pede uma prévia no corte 2 — a tela vai até o corte, o quadro e a
    /// camada da prévia.
    #[test]
    fn a_proposal_requested_for_another_cut_brings_the_screen_to_it() {
        let mut core = CoreSession::for_tests("t");
        let mk = |s: &mut CoreSession, parent: Option<Id>, name: &str, data: NodeKind| s.apply(Command::CreateNode { id: None, parent, index: None, name: name.into(), data }).unwrap().created[0].clone();
        let ep = mk(&mut core, None, "EP", NodeKind::Episode(EpisodeData::default()));
        let sc = mk(&mut core, Some(ep), "S", NodeKind::Scene(SceneData::default()));
        let mut made = Vec::new();
        for k in 1..=2 {
            let cut = mk(&mut core, Some(sc.clone()), &format!("C00{k}"), NodeKind::Cut(CutData::new(24)));
            let layer = mk(&mut core, Some(cut.clone()), "A", NodeKind::Layer(LayerData::new(LayerType::Vector)));
            for (n, dx) in [(1u32, 0.0f32), (2, 80.0)] {
                let mut c = CelData::new(n);
                c.role = CelRole::Key;
                c.strokes = vec![line(&format!("k{k}{n}"), dx)];
                mk(&mut core, Some(layer.clone()), &format!("A{n}"), NodeKind::Cel(c));
            }
            let track = Id(format!("trk-{k}"));
            core.apply(Command::AddTrack { cut: cut.clone(), index: None, track: Track::new(track.clone(), "A", TrackKind::Drawing, Some(layer.clone())) }).unwrap();
            core.apply(Command::SetCells { cut: cut.clone(), track, cells: vec![CellEdit { frame: 0, cell: Some(CellRef::Cel { number: 1 }) }, CellEdit { frame: 8, cell: Some(CellRef::Cel { number: 2 }) }] }).unwrap();
            made.push((cut, layer));
        }
        let mut ed = super::super::editor::Editor::default();
        ed.cut = Some(made[0].0.clone());
        ed.layer = Some(made[0].1.clone());
        ed.frame = 0;
        ibp::preview(&mut core, &InbetweenPreviewRequest::new(made[1].0.clone(), made[1].1.clone(), 4)).unwrap();
        let mut icons = crate::icons::Icons::default();
        let mut notes = Vec::new();
        let mut tool = crate::workspace::ToolId::Pen;
        let mut env = Env { t: crate::theme::Tokens::dark(), core: &mut core, ed: &mut ed, icons: &mut icons, notes: &mut notes, tool: &mut tool, ws: crate::workspace::WorkspaceId::Animacao };
        tick(&mut env);
        assert_eq!((ed.cut.clone(), ed.frame, ed.layer.clone()), (Some(made[1].0.clone()), 4, Some(made[1].1.clone())));
    }
}
