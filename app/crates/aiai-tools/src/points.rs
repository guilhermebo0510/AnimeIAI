//! Pontos de controle de um traço vetorial: ler e editar em lote, pelo id estável do ponto (`StrokePoint.id`, ver
//! `Stroke::point_id`) ou pelo índice.
//!
//! O id continua apontando para o mesmo ponto quando outros entram ou saem; o índice não, por isso a edição que usa
//! índice exige guarda: `stroke_rev` (impressão digital do traço) e/ou `point_count`. Guarda que não bate é recusada
//! ([`ToolError::Stale`]), então um índice lido antes de outra mudança nunca edita o ponto errado.
//!
//! As operações do lote rodam em ordem (os índices se referem ao traço como ficou depois das anteriores); o lote
//! inteiro vira um `ReplaceStrokes` (mesmo id, um passo de desfazer). O painel Editar pontos do app usa as mesmas
//! funções ([`nearest_on_curve`], [`insert_on_curve`], [`delete_point`]). Simplificar ([`simplify_plan`]) mede o
//! desvio na linha desenhada pelo motor, não só nos pontos de controle.

use crate::context::*;
use aiai_core::*;
use aiai_paint::Affine;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------------------------------- geometria do traço

/// Lugar na curva (desenhada pelo motor) mais perto de um ponto.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveHit {
    /// Distância do ponto até a curva.
    pub dist: f32,
    /// Segmento entre os pontos de controle `seg` e `seg + 1` (no fechado, o último volta ao 0).
    pub seg: usize,
    /// O lugar na curva.
    pub x: f32,
    pub y: f32,
    /// Fração (0..1) dentro do segmento, medida ao longo da curva achatada.
    pub frac: f32,
}

fn seg_dist(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((px - a.0) * dx + (py - a.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    ((px - (a.0 + dx * t)).hypot(py - (a.1 + dy * t)), t)
}

/// Lugar na curva mais perto de (x, y), com o segmento de controle em que ele cai.
pub fn nearest_on_curve(s: &Stroke, x: f32, y: f32) -> Option<CurveHit> {
    let flat = aiai_paint::flatten_points(s);
    if flat.len() < 2 || s.points.len() < 2 {
        return None;
    }
    // onde cada ponto de controle cai na curva achatada (a curva passa por eles, em ordem)
    let mut at = Vec::with_capacity(s.points.len());
    let mut from = 0usize;
    for q in &s.points {
        let mut best = (f32::MAX, from);
        for (k, f) in flat.iter().enumerate().skip(from) {
            let d = (f.0 - q.x).hypot(f.1 - q.y);
            if d < best.0 {
                best = (d, k);
            }
            if d < 1e-3 {
                break;
            }
        }
        at.push(best.1);
        from = best.1;
    }
    let (mut bd, mut bk, mut bt) = (f32::MAX, 0usize, 0.0f32);
    for k in 0..flat.len() - 1 {
        let (d, t) = seg_dist(x, y, (flat[k].0, flat[k].1), (flat[k + 1].0, flat[k + 1].1));
        if d < bd {
            (bd, bk, bt) = (d, k, t);
        }
    }
    let on = (flat[bk].0 + (flat[bk + 1].0 - flat[bk].0) * bt, flat[bk].1 + (flat[bk + 1].1 - flat[bk].1) * bt);
    let n = s.points.len();
    let segs = if s.closed { n } else { n - 1 };
    let mut seg = 0usize;
    for (i, &a) in at.iter().enumerate().take(segs) {
        if a <= bk {
            seg = i;
        }
    }
    let (a, b) = (at[seg], if seg + 1 < n { at[seg + 1] } else { flat.len() - 1 });
    let frac = if b > a { ((bk as f32 + bt) - a as f32) / (b - a) as f32 } else { 0.5 };
    Some(CurveHit { dist: bd, seg, x: on.0, y: on.1, frac: frac.clamp(0.0, 1.0) })
}

/// Bézier tem 3n+1 pontos: ao inserir/apagar vira spline (passa pelos mesmos pontos de passagem).
fn unbezier(s: &mut Stroke) {
    if s.geometry == StrokeGeometry::Bezier {
        s.geometry = StrokeGeometry::Spline;
    }
}

/// Ponto novo entre `a` e `b` (pressão e largura interpoladas em `f`).
fn between(a: StrokePoint, b: StrokePoint, f: f32, x: f32, y: f32) -> StrokePoint {
    StrokePoint { x, y, pressure: a.pressure + (b.pressure - a.pressure) * f, tilt: None, t: None, corner: false, width: a.width + (b.width - a.width) * f, id: 0, handle: None }
}

/// Põe um ponto em cima da curva, no lugar mais perto de (x, y) (até `tol`); devolve o índice novo.
pub fn insert_on_curve(s: &mut Stroke, x: f32, y: f32, tol: f32) -> Option<usize> {
    let hit = nearest_on_curve(s, x, y)?;
    if hit.dist > tol {
        return None;
    }
    let n = s.points.len();
    let mut q = between(s.points[hit.seg], s.points[(hit.seg + 1) % n], hit.frac, hit.x, hit.y);
    s.materialize_point_ids();
    q.id = s.next_point_id();
    s.points.insert(hit.seg + 1, q);
    unbezier(s);
    Some(hit.seg + 1)
}

/// Põe o ponto (x, y) logo depois do índice `after`; devolve o índice novo (`after + 1`).
pub fn insert_after(s: &mut Stroke, after: usize, x: f32, y: f32) -> Result<usize, String> {
    let n = s.points.len();
    if after >= n {
        return Err(format!("'after' {after} fora do traço (pontos 0..{})", n.saturating_sub(1)));
    }
    let a = s.points[after];
    let b = if after + 1 < n { s.points[after + 1] } else if s.closed { s.points[0] } else { a };
    let mut q = between(a, b, 0.5, x, y);
    s.materialize_point_ids();
    q.id = s.next_point_id();
    s.points.insert(after + 1, q);
    unbezier(s);
    Ok(after + 1)
}

/// Apaga o ponto `i` (o traço fica com pelo menos 2).
pub fn delete_point(s: &mut Stroke, i: usize) -> bool {
    if s.points.len() <= 2 || i >= s.points.len() {
        return false;
    }
    // os ids dos outros pontos não podem mudar com a remoção
    s.materialize_point_ids();
    s.points.remove(i);
    unbezier(s);
    true
}

/// Impressão digital do traço (FNV-1a de 64 bits do JSON dele, em hex): muda a cada edição do traço, igual em
/// qualquer processo. É a revisão que a leitura devolve e a edição confere.
pub fn stroke_rev(s: &Stroke) -> String {
    let bytes = serde_json::to_vec(s).unwrap_or_default();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Transformação cel → canvas da camada no quadro (transformação do cel exposto + movimento da camada, sem câmera).
pub fn cel_placement(p: &Project, cut_id: &Id, layer_id: &Id, frame: u32) -> ToolResult<Affine> {
    let cut = cut_data(p, cut_id)?;
    let layer = layer_data(p, layer_id)?;
    let ct = match exposed_cel(p, cut, layer_id, frame).map(|n| &n.data) {
        Some(NodeKind::Cel(c)) => c.transform,
        _ => [0.0, 0.0, 1.0, 0.0],
    };
    let (w, h) = canvas_size(p, cut);
    Ok(aiai_render::compose::layer_placement(layer, ct, frame, w, h))
}

// ------------------------------------------------------------------------------------------------ simplificar

/// O que simplificar tira (ou tiraria, no ensaio).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SimplifyReport {
    /// Tolerância usada, em px do cel.
    pub tolerance: f32,
    /// Ids dos pontos tirados.
    pub removed: Vec<u32>,
    /// Pontos que ficam.
    pub kept: usize,
    /// Maior distância (px do cel) entre a linha de antes e a de depois; sempre <= `tolerance`.
    pub max_deviation: f32,
}

/// Distância do ponto até a polilinha.
fn polyline_dist(x: f32, y: f32, line: &[(f32, f32, f32)]) -> f32 {
    if line.len() == 1 {
        return (line[0].0 - x).hypot(line[0].1 - y);
    }
    line.windows(2).map(|w| seg_dist(x, y, (w[0].0, w[0].1), (w[1].0, w[1].1)).0).fold(f32::MAX, f32::min)
}

/// Maior distância entre as duas linhas desenhadas (nos dois sentidos, pelas curvas achatadas do motor).
pub fn curve_deviation(a: &Stroke, b: &Stroke) -> f32 {
    let (fa, fb) = (aiai_paint::flatten_points(a), aiai_paint::flatten_points(b));
    if fa.is_empty() || fb.is_empty() {
        return 0.0;
    }
    let one_way = |from: &[(f32, f32, f32)], to: &[(f32, f32, f32)]| {
        // no máximo ~1500 amostras de um lado (traços longos de mão livre)
        let step = (from.len() / 1500).max(1);
        from.iter().step_by(step).chain(from.last()).map(|q| polyline_dist(q.0, q.1, to)).fold(0.0f32, f32::max)
    };
    one_way(&fa, &fb).max(one_way(&fb, &fa))
}

/// Douglas-Peucker nos pontos de controle (x, y e meia largura): marca em `keep` os que seguram a forma.
fn rdp(p: &[[f32; 3]], a: usize, b: usize, tol: f32, keep: &mut [bool]) {
    if b <= a + 1 {
        return;
    }
    let d3 = |q: [f32; 3], u: [f32; 3], v: [f32; 3]| {
        let d = [v[0] - u[0], v[1] - u[1], v[2] - u[2]];
        let l2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        let t = if l2 > 0.0 { (((q[0] - u[0]) * d[0] + (q[1] - u[1]) * d[1] + (q[2] - u[2]) * d[2]) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let e = [q[0] - u[0] - d[0] * t, q[1] - u[1] - d[1] * t, q[2] - u[2] - d[2] * t];
        (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt()
    };
    let (mut best, mut idx) = (0.0f32, a);
    for i in a + 1..b {
        let d = d3(p[i], p[a], p[b]);
        if d > best {
            (best, idx) = (d, i);
        }
    }
    if best > tol {
        keep[idx] = true;
        rdp(p, a, idx, tol, keep);
        rdp(p, idx, b, tol, keep);
    }
}

/// Pontos que podem sair sem a linha desenhada se afastar mais que `tol` px (do cel) da de agora. Ficam sempre as
/// pontas, os cantos e os pontos com alça explícita. Devolve quais ficam e o desvio real medido.
pub fn simplify_plan(s: &Stroke, tol: f32) -> Result<(Vec<bool>, f32), String> {
    if s.geometry == StrokeGeometry::Bezier {
        return Err("simplificar não vale para Bézier (controles 3n+1); converta para spline antes".into());
    }
    let n = s.points.len();
    let locked: Vec<bool> = s.points.iter().enumerate().map(|(i, q)| i == 0 || i + 1 == n || q.corner || q.handle.is_some()).collect();
    let pts: Vec<[f32; 3]> = (0..n).map(|i| [s.points[i].x, s.points[i].y, aiai_paint::width_at_point(s, i) * 0.5]).collect();
    let mut t = tol.max(0.0);
    for _ in 0..12 {
        let mut keep = locked.clone();
        let anchors: Vec<usize> = (0..n).filter(|&i| locked[i]).collect();
        for w in anchors.windows(2) {
            rdp(&pts, w[0], w[1], t, &mut keep);
        }
        if keep.iter().all(|k| *k) {
            return Ok((keep, 0.0));
        }
        let cand = keep_only(s, &keep);
        let dev = curve_deviation(s, &cand);
        if dev <= tol {
            return Ok((keep, dev));
        }
        t *= 0.5;
    }
    Ok((vec![true; n], 0.0))
}

/// O traço só com os pontos marcados (ids gravados antes, para os que ficam manterem os seus).
fn keep_only(s: &Stroke, keep: &[bool]) -> Stroke {
    let mut ns = s.clone();
    ns.materialize_point_ids();
    ns.points = ns.points.iter().zip(keep).filter(|(_, k)| **k).map(|(q, _)| *q).collect();
    if ns.geometry == StrokeGeometry::Freehand {
        ns.geometry = StrokeGeometry::Spline;
    }
    ns
}

// ---------------------------------------------------------------------------------------------------- leitura

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PointsRequest {
    pub cut: Id,
    pub frame: u32,
    /// Camada VETORIAL; os pontos são do cel que ela expõe no quadro.
    pub layer: Id,
    /// Id do traço. Sem ele, `at` escolhe o traço mais perto.
    #[serde(default)]
    pub stroke: Option<Id>,
    /// Ponto clicado em px do canvas (o movimento da camada é desfeito antes de procurar).
    #[serde(default)]
    pub at: Option<[f32; 2]>,
    /// Distância máxima (px do canvas) de `at` até a borda do traço. Padrão 12.
    #[serde(default)]
    pub tolerance: Option<f32>,
    /// Prévia de simplificar com esta tolerância (px do cel): quais ids sairiam e o desvio. Não muda nada.
    #[serde(default)]
    pub simplify_preview: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PointInfo {
    /// Id estável do ponto no traço: continua o mesmo quando outros pontos entram ou saem.
    pub id: u32,
    pub index: usize,
    /// Posição no cel (as coordenadas gravadas no traço).
    pub x: f32,
    pub y: f32,
    /// Posição no canvas do corte (com a transformação do cel e o movimento da camada no quadro).
    pub canvas_x: f32,
    pub canvas_y: f32,
    pub pressure: f32,
    pub corner: bool,
    /// Multiplicador da largura neste ponto (1 = a do traço).
    pub width: f32,
    /// Largura em px neste ponto (largura × ponta pela pressão × multiplicador).
    pub width_px: f32,
    /// Alça (tangente) efetiva em px do cel: a curva sai por +handle e chega por -handle. Só spline/mão livre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<[f32; 2]>,
    /// A mesma alça no canvas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas_handle: Option<[f32; 2]>,
    /// A alça foi dada (set_handle); falso = automática da spline.
    pub handle_explicit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StrokePoints {
    pub cel: Id,
    pub stroke: Id,
    /// Revisão do traço: passe-a em `stroke_rev` ao editar.
    pub stroke_rev: String,
    pub point_count: usize,
    /// Revisão do projeto quando lido.
    pub revision: u64,
    pub geometry: StrokeGeometry,
    pub closed: bool,
    pub tip: VectorTip,
    /// Largura base do traço (px).
    pub width: f32,
    /// Com `at`: distância (px do canvas) até a borda do traço e o ponto de controle mais perto do clique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_distance: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nearest_point: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nearest_point_id: Option<u32>,
    /// Com `simplify_preview`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simplify: Option<SimplifyReport>,
    pub points: Vec<PointInfo>,
}

/// Cel exposto de uma camada vetorial no quadro.
fn vector_cel<'a>(p: &'a Project, cut_id: &Id, layer_id: &Id, frame: u32) -> ToolResult<(&'a Node, &'a CelData)> {
    let cut = cut_data(p, cut_id)?;
    let layer = layer_data(p, layer_id)?;
    if layer.kind != LayerType::Vector {
        return Err(ToolError::Invalid(format!("a camada {layer_id} é {:?}; pontos só existem em camada vetorial", layer.kind)));
    }
    let node = exposed_cel(p, cut, layer_id, frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {frame}")))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    Ok((node, cel))
}

fn find_stroke<'a>(cel: &'a CelData, id: &Id) -> ToolResult<&'a Stroke> {
    cel.strokes.iter().find(|s| &s.id == id).ok_or_else(|| ToolError::NotFound(format!("traço {id} no cel exposto")))
}

/// Vetor (sem a translação) pela parte linear da transformação.
fn apply_vec(a: &Affine, v: [f32; 2]) -> [f32; 2] {
    let m = a.0;
    [m[0] * v[0] + m[1] * v[1], m[3] * v[0] + m[4] * v[1]]
}

/// Pontos do traço como a leitura devolve (cel e canvas).
pub fn describe(p: &Project, cel: &Id, s: &Stroke, place: &Affine) -> StrokePoints {
    let points = s
        .points
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let (cx, cy) = place.apply(q.x, q.y);
            let handle = aiai_paint::point_handle(s, i);
            PointInfo {
                id: s.point_id(i),
                index: i,
                x: q.x,
                y: q.y,
                canvas_x: cx,
                canvas_y: cy,
                pressure: q.pressure,
                corner: q.corner,
                width: q.width,
                width_px: aiai_paint::width_at_point(s, i),
                handle,
                canvas_handle: handle.map(|h| apply_vec(place, h)),
                handle_explicit: q.handle.is_some(),
            }
        })
        .collect();
    StrokePoints {
        cel: cel.clone(),
        stroke: s.id.clone(),
        stroke_rev: stroke_rev(s),
        point_count: s.points.len(),
        revision: p.revision,
        geometry: s.geometry,
        closed: s.closed,
        tip: s.tip,
        width: s.width,
        hit_distance: None,
        nearest_point: None,
        nearest_point_id: None,
        simplify: None,
        points,
    }
}

/// Relatório de simplificar `s` com `tol` px do cel.
pub fn simplify_report(s: &Stroke, tol: f32) -> Result<SimplifyReport, String> {
    if !(tol.is_finite() && tol >= 0.0) {
        return Err("a tolerância de simplificar deve ser um número >= 0".into());
    }
    let (keep, dev) = simplify_plan(s, tol)?;
    let removed = (0..s.points.len()).filter(|&i| !keep[i]).map(|i| s.point_id(i)).collect();
    Ok(SimplifyReport { tolerance: tol, removed, kept: keep.iter().filter(|k| **k).count(), max_deviation: dev })
}

/// Lê os pontos de um traço (pelo id ou pelo clique `at`).
pub fn read_points(p: &Project, req: &PointsRequest) -> ToolResult<StrokePoints> {
    let (node, cel) = vector_cel(p, &req.cut, &req.layer, req.frame)?;
    let place = cel_placement(p, &req.cut, &req.layer, req.frame)?;
    let (s, mut out) = match (&req.stroke, req.at) {
        (Some(id), None) => {
            let s = find_stroke(cel, id)?;
            (s, describe(p, &node.id, s, &place))
        }
        (stroke, Some([ax, ay])) => {
            let inv = place.inverse().ok_or_else(|| ToolError::Invalid("o movimento da camada neste quadro não tem inversa (escala 0)".into()))?;
            let (x, y) = inv.apply(ax, ay);
            // px do cel por px do canvas (a tolerância é dada no canvas)
            let scale = { let (x1, y1) = inv.apply(ax + 1.0, ay); (x1 - x).hypot(y1 - y).max(1e-6) };
            let edge = |s: &Stroke| aiai_paint::sample_stroke(s).iter().map(|q| ((q.x - x).hypot(q.y - y) - q.r).max(0.0)).fold(f32::MAX, f32::min);
            let (s, d) = match stroke {
                Some(id) => {
                    let s = find_stroke(cel, id)?;
                    (s, edge(s))
                }
                None => cel.strokes.iter().map(|s| (s, edge(s))).min_by(|a, b| a.1.total_cmp(&b.1)).ok_or_else(|| ToolError::NotFound("o cel exposto não tem traços".into()))?,
            };
            let d = d / scale;
            let tol = req.tolerance.unwrap_or(12.0);
            if stroke.is_none() && d > tol {
                return Err(ToolError::NotFound(format!("nenhum traço a até {tol} px de ({ax}, {ay}) (o mais perto, {}, está a {d:.1} px)", s.id)));
            }
            let mut out = describe(p, &node.id, s, &place);
            out.hit_distance = Some(d);
            out.nearest_point = s.points.iter().enumerate().map(|(i, q)| (i, (q.x - x).hypot(q.y - y))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i);
            out.nearest_point_id = out.nearest_point.map(|i| s.point_id(i));
            (s, out)
        }
        (None, None) => return Err(ToolError::Invalid("informe 'stroke' (id do traço) ou 'at' ([x, y] em px do canvas)".into())),
    };
    if let Some(tol) = req.simplify_preview {
        out.simplify = Some(simplify_report(s, tol).map_err(ToolError::Invalid)?);
    }
    Ok(out)
}

// ----------------------------------------------------------------------------------------------------- edição

/// Espaço das coordenadas x/y (e das alças dx/dy) das operações.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PointSpace {
    /// Coordenadas do cel (as `x`/`y`/`handle` da leitura).
    #[default]
    Cel,
    /// Px do canvas do corte (as `canvas_x`/`canvas_y`/`canvas_handle` da leitura; o movimento da camada é desfeito).
    Canvas,
}

/// Uma operação sobre os pontos. O ponto é dado por `id` (estável; preferido) OU por `index` (posição no traço como
/// ficou depois das operações anteriores do lote; exige guarda).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointOp {
    /// Move o ponto para (x, y).
    Move {
        #[serde(default)]
        id: Option<u32>,
        #[serde(default)]
        index: Option<usize>,
        x: f32,
        y: f32,
    },
    /// Com `after_id`/`after` (índice): põe o ponto (x, y) logo depois desse ponto. Sem eles: põe um ponto EM CIMA da
    /// curva, no lugar mais perto de (x, y), se ele estiver a até `tolerance` px (padrão 12) da curva.
    Insert {
        #[serde(default)]
        after_id: Option<u32>,
        #[serde(default)]
        after: Option<usize>,
        x: f32,
        y: f32,
        #[serde(default)]
        tolerance: Option<f32>,
    },
    /// Apaga o ponto (o traço fica com pelo menos 2).
    Delete {
        #[serde(default)]
        id: Option<u32>,
        #[serde(default)]
        index: Option<usize>,
    },
    /// Canto (a curva quebra no ponto) ou suave. Vale para spline e mão livre.
    Corner {
        #[serde(default)]
        id: Option<u32>,
        #[serde(default)]
        index: Option<usize>,
        on: bool,
    },
    /// Largura em px naquele ponto (vira o multiplicador do ponto, 0..20× a largura da ponta).
    WidthPx {
        #[serde(default)]
        id: Option<u32>,
        #[serde(default)]
        index: Option<usize>,
        px: f32,
    },
    /// Alça explícita (dx, dy): a curva sai do ponto na direção +(dx, dy) e chega por -(dx, dy); mais comprida = curva
    /// mais aberta ali. Só spline/mão livre e ponto que não é canto.
    SetHandle {
        #[serde(default)]
        id: Option<u32>,
        #[serde(default)]
        index: Option<usize>,
        dx: f32,
        dy: f32,
    },
    /// Volta a alça para a automática da spline.
    ClearHandle {
        #[serde(default)]
        id: Option<u32>,
        #[serde(default)]
        index: Option<usize>,
    },
    /// Tira os pontos que não seguram a forma: a linha desenhada não se afasta mais que `tolerance` px da de antes.
    /// Pontas, cantos e pontos com alça explícita ficam.
    Simplify { tolerance: f32 },
}

impl PointOp {
    /// A operação endereça um ponto por índice (e então a edição precisa de guarda)?
    pub fn uses_index(&self) -> bool {
        match self {
            PointOp::Move { index, .. } | PointOp::Delete { index, .. } | PointOp::Corner { index, .. } | PointOp::WidthPx { index, .. } | PointOp::SetHandle { index, .. } | PointOp::ClearHandle { index, .. } => index.is_some(),
            PointOp::Insert { after, .. } => after.is_some(),
            PointOp::Simplify { .. } => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PointsEditRequest {
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    pub stroke: Id,
    /// Guarda: a `stroke_rev` da última leitura. Se o traço mudou desde então, nada é editado.
    #[serde(default)]
    pub stroke_rev: Option<String>,
    /// Guarda: a quantidade de pontos que você viu. Se não bater, nada é editado.
    #[serde(default)]
    pub point_count: Option<usize>,
    /// Espaço de x/y, dx/dy e das tolerâncias: cel (padrão) ou canvas.
    #[serde(default)]
    pub space: PointSpace,
    /// Ensaio: devolve o traço como ficaria (e o que simplify tiraria) sem mudar o documento.
    #[serde(default)]
    pub dry_run: bool,
    pub ops: Vec<PointOp>,
}

/// O que o lote fez, além do traço novo.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EditReport {
    /// Ids dos pontos inseridos (os que ainda existem no fim).
    pub inserted: Vec<u32>,
    /// Ids dos pontos tirados (delete e simplify).
    pub removed: Vec<u32>,
    /// O último simplify do lote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simplify: Option<SimplifyReport>,
}

/// Resultado da edição: o comando a aplicar, o traço como fica e o relatório.
#[derive(Debug, Clone)]
pub struct PointsEdit {
    pub command: Command,
    pub stroke: Stroke,
    pub cel: Id,
    pub report: EditReport,
}

fn guard(s: &Stroke, req: &PointsEditRequest) -> ToolResult<()> {
    if let Some(rev) = &req.stroke_rev {
        let now = stroke_rev(s);
        if &now != rev {
            return Err(ToolError::Stale(format!("o traço {} mudou desde a leitura (stroke_rev {rev}, agora {now}, {} pontos): leia de novo os pontos antes de editar", s.id, s.points.len())));
        }
    }
    if let Some(n) = req.point_count {
        if n != s.points.len() {
            return Err(ToolError::Stale(format!("o traço {} tem {} pontos, não {n}: leia de novo os pontos antes de editar", s.id, s.points.len())));
        }
    }
    Ok(())
}

/// Índice do ponto dado por `id` ou `index` (um dos dois).
fn pick(s: &Stroke, k: usize, id: Option<u32>, index: Option<usize>) -> Result<usize, String> {
    match (id, index) {
        (Some(_), Some(_)) => Err(format!("ops[{k}]: dê 'id' OU 'index', não os dois")),
        (None, None) => Err(format!("ops[{k}]: falta o ponto: 'id' (estável) ou 'index'")),
        (Some(id), None) => s.point_index(id).ok_or_else(|| format!("ops[{k}]: não há ponto com id {id} no traço (ids: {:?})", (0..s.points.len()).map(|i| s.point_id(i)).collect::<Vec<_>>())),
        (None, Some(i)) if i < s.points.len() => Ok(i),
        (None, Some(i)) => Err(format!("ops[{k}]: índice {i} fora do traço (pontos 0..{})", s.points.len().saturating_sub(1))),
    }
}

/// Aplica o lote ao traço (sem guarda). `inv` = canvas → cel quando as coordenadas vêm do canvas.
pub fn apply_ops(s: &mut Stroke, ops: &[PointOp], inv: Option<&Affine>) -> Result<EditReport, String> {
    let to_cel = |x: f32, y: f32| inv.map(|a| a.apply(x, y)).unwrap_or((x, y));
    let to_cel_vec = |v: [f32; 2]| inv.map(|a| apply_vec(a, v)).unwrap_or(v);
    // px do cel por px do request (para as tolerâncias)
    let scale = inv.map(|a| { let v = apply_vec(a, [1.0, 0.0]); v[0].hypot(v[1]) }).unwrap_or(1.0);
    let mut rep = EditReport::default();
    for (k, op) in ops.iter().enumerate() {
        let n = s.points.len();
        let finite = |v: f32, name: &str| if v.is_finite() { Ok(v) } else { Err(format!("ops[{k}]: '{name}' deve ser um número finito")) };
        match *op {
            PointOp::Move { id, index, x, y } => {
                let i = pick(s, k, id, index)?;
                let (cx, cy) = to_cel(finite(x, "x")?, finite(y, "y")?);
                s.points[i].x = cx;
                s.points[i].y = cy;
            }
            PointOp::Insert { after_id, after, x, y, tolerance } => {
                let (cx, cy) = to_cel(finite(x, "x")?, finite(y, "y")?);
                let at = match (after_id, after) {
                    (None, None) => {
                        let tol = tolerance.unwrap_or(12.0) * scale;
                        insert_on_curve(s, cx, cy, tol).ok_or_else(|| {
                            let d = nearest_on_curve(s, cx, cy).map(|h| format!("{:.1}", h.dist / scale)).unwrap_or_else(|| "?".into());
                            format!("ops[{k}]: ({x}, {y}) está a {d} px da curva, mais que a tolerância {}; aumente 'tolerance' ou use 'after_id'", tolerance.unwrap_or(12.0))
                        })?
                    }
                    (id, index) => {
                        let a = pick(s, k, id, index).map_err(|e| e.replace("'id'", "'after_id'").replace("'index'", "'after'"))?;
                        insert_after(s, a, cx, cy).map_err(|e| format!("ops[{k}]: {e}"))?
                    }
                };
                rep.inserted.push(s.point_id(at));
            }
            PointOp::Delete { id, index } => {
                let i = pick(s, k, id, index)?;
                s.materialize_point_ids();
                let gone = s.point_id(i);
                if !delete_point(s, i) {
                    return Err(format!("ops[{k}]: o traço precisa de pelo menos 2 pontos (tem {n})"));
                }
                rep.inserted.retain(|&j| j != gone);
                rep.removed.push(gone);
            }
            PointOp::Corner { id, index, on } => {
                let i = pick(s, k, id, index)?;
                s.points[i].corner = on;
            }
            PointOp::WidthPx { id, index, px } => {
                let i = pick(s, k, id, index)?;
                let px = finite(px, "px")?;
                if px < 0.0 {
                    return Err(format!("ops[{k}]: 'px' deve ser >= 0"));
                }
                s.points[i].width = aiai_paint::point_width_for(s, i, px * scale);
            }
            PointOp::SetHandle { id, index, dx, dy } => {
                let i = pick(s, k, id, index)?;
                if !matches!(s.geometry, StrokeGeometry::Spline | StrokeGeometry::Freehand) {
                    return Err(format!("ops[{k}]: alça só vale em spline ou mão livre (o traço é {:?})", s.geometry));
                }
                if s.points[i].corner {
                    return Err(format!("ops[{k}]: o ponto {} é canto e canto ignora a alça; tire o canto antes (corner on:false)", s.point_id(i)));
                }
                s.points[i].handle = Some(to_cel_vec([finite(dx, "dx")?, finite(dy, "dy")?]));
            }
            PointOp::ClearHandle { id, index } => {
                let i = pick(s, k, id, index)?;
                s.points[i].handle = None;
            }
            PointOp::Simplify { tolerance } => {
                let tol = finite(tolerance, "tolerance")? * scale;
                let r = simplify_report(s, tol).map_err(|e| format!("ops[{k}]: {e}"))?;
                if !r.removed.is_empty() {
                    let keep: Vec<bool> = (0..s.points.len()).map(|i| !r.removed.contains(&s.point_id(i))).collect();
                    *s = keep_only(s, &keep);
                }
                rep.inserted.retain(|j| !r.removed.contains(j));
                rep.removed.extend(r.removed.iter().copied());
                rep.simplify = Some(r);
            }
        }
    }
    Ok(rep)
}

/// Edita os pontos de um traço em lote: um `ReplaceStrokes` (um passo de desfazer).
pub fn edit_points(p: &Project, req: &PointsEditRequest) -> ToolResult<PointsEdit> {
    let (node, cel) = vector_cel(p, &req.cut, &req.layer, req.frame)?;
    if layer_data(p, &req.layer)?.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    let s = find_stroke(cel, &req.stroke)?;
    guard(s, req)?;
    if req.ops.is_empty() {
        return Err(ToolError::Invalid("'ops' vazio: nada a editar".into()));
    }
    let inv = match req.space {
        PointSpace::Cel => None,
        PointSpace::Canvas => Some(cel_placement(p, &req.cut, &req.layer, req.frame)?.inverse().ok_or_else(|| ToolError::Invalid("o movimento da camada neste quadro não tem inversa (escala 0)".into()))?),
    };
    let mut ns = s.clone();
    let report = apply_ops(&mut ns, &req.ops, inv.as_ref()).map_err(ToolError::Invalid)?;
    ns.validate().map_err(|e| ToolError::Invalid(format!("o traço editado ficaria inválido: {e}")))?;
    Ok(PointsEdit { command: Command::ReplaceStrokes { cel: node.id.clone(), strokes: vec![ns.clone()] }, stroke: ns, cel: node.id.clone(), report })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> StrokePoint {
        StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }
    }

    fn line(geometry: StrokeGeometry) -> Stroke {
        let mut s: Stroke = serde_json::from_value(serde_json::json!({"id": "s1", "points": [], "width": 4.0, "tip": "marker", "color": {"type": "direct", "rgba": [0, 0, 0, 1]}})).unwrap();
        s.points = vec![pt(0.0, 0.0), pt(100.0, 0.0), pt(200.0, 0.0)];
        s.geometry = geometry;
        s
    }

    #[test]
    fn nearest_on_curve_finds_segment_and_fraction() {
        let s = line(StrokeGeometry::Polyline);
        let h = nearest_on_curve(&s, 150.0, 5.0).unwrap();
        assert_eq!(h.seg, 1);
        assert!((h.x - 150.0).abs() < 0.5 && h.y.abs() < 0.5 && (h.dist - 5.0).abs() < 0.5, "{h:?}");
        assert!((h.frac - 0.5).abs() < 0.02, "{h:?}");
    }

    #[test]
    fn insert_on_curve_respects_tolerance_and_interpolates() {
        let mut s = line(StrokeGeometry::Polyline);
        s.points[0].width = 1.0;
        s.points[1].width = 3.0;
        assert_eq!(insert_on_curve(&mut s, 50.0, 30.0, 12.0), None, "longe demais");
        assert_eq!(insert_on_curve(&mut s, 50.0, 4.0, 12.0), Some(1));
        assert_eq!(s.points.len(), 4);
        assert!((s.points[1].x - 50.0).abs() < 0.5 && s.points[1].y.abs() < 0.5, "na curva, não no clique");
        assert!((s.points[1].width - 2.0).abs() < 0.05, "largura interpolada: {}", s.points[1].width);
    }

    #[test]
    fn insert_after_and_delete_keep_at_least_two_and_drop_bezier() {
        let mut s = line(StrokeGeometry::Spline);
        assert_eq!(insert_after(&mut s, 2, 300.0, 0.0), Ok(3));
        assert!(insert_after(&mut s, 9, 0.0, 0.0).is_err());
        let mut b = line(StrokeGeometry::Bezier);
        b.points.push(pt(300.0, 0.0));
        assert!(delete_point(&mut b, 1));
        assert_eq!(b.geometry, StrokeGeometry::Spline, "Bézier sem 3n+1 pontos vira spline");
        assert!(delete_point(&mut b, 0));
        assert!(!delete_point(&mut b, 0), "fica com 2");
        assert_eq!(b.points.len(), 2);
    }

    #[test]
    fn stroke_rev_changes_with_any_edit_and_is_stable() {
        let s = line(StrokeGeometry::Spline);
        assert_eq!(stroke_rev(&s), stroke_rev(&s.clone()));
        let mut t = s.clone();
        t.points[1].corner = true;
        assert_ne!(stroke_rev(&s), stroke_rev(&t));
        assert_eq!(stroke_rev(&s).len(), 16);
    }

    fn ops(v: serde_json::Value) -> Vec<PointOp> {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn apply_ops_addresses_points_by_stable_id_or_index() {
        let mut s = line(StrokeGeometry::Spline);
        assert_eq!((0..3).map(|i| s.point_id(i)).collect::<Vec<_>>(), vec![1, 2, 3], "sem ids gravados: índice+1");
        let rep = apply_ops(&mut s, &ops(serde_json::json!([
            {"op": "insert", "after_id": 1, "x": 50, "y": 10},
            {"op": "move", "id": 3, "x": 210, "y": 20},
            {"op": "corner", "id": 2, "on": true},
            {"op": "width_px", "index": 0, "px": 8},
            {"op": "insert", "x": 100, "y": 3},
            {"op": "delete", "id": 1}
        ])), None)
        .unwrap();
        assert_eq!((rep.inserted.clone(), rep.removed.clone()), (vec![4, 5], vec![1]));
        assert_eq!(s.points.len(), 4);
        let at = |id: u32| s.points[s.point_index(id).unwrap()];
        assert_eq!((at(4).x, at(4).y, s.point_index(4)), (50.0, 10.0, Some(0)), "o inserido virou o primeiro e manteve o id");
        assert!(at(2).corner && at(2).x == 100.0);
        assert_eq!((at(3).x, at(3).y), (210.0, 20.0));
        let err = |v: serde_json::Value| apply_ops(&mut s.clone(), &ops(v), None).unwrap_err();
        assert!(err(serde_json::json!([{"op": "move", "id": 1, "x": 0, "y": 0}])).contains("não há ponto com id 1"), "id apagado não existe mais");
        assert!(err(serde_json::json!([{"op": "move", "index": 9, "x": 0, "y": 0}])).contains("fora do traço"));
        assert!(err(serde_json::json!([{"op": "corner", "id": 2, "index": 1, "on": true}])).contains("não os dois"));
        assert!(err(serde_json::json!([{"op": "delete"}])).contains("falta o ponto"));
        assert!(serde_json::from_value::<PointOp>(serde_json::json!({"op": "move", "index": 0, "x": 0, "y": 0, "z": 1})).is_err(), "chave desconhecida na operação");
        assert!(ops(serde_json::json!([{"op": "insert", "after": 0, "x": 1, "y": 1}]))[0].uses_index());
        assert!(!ops(serde_json::json!([{"op": "insert", "after_id": 1, "x": 1, "y": 1}, {"op": "simplify", "tolerance": 1}])).iter().any(PointOp::uses_index));
    }

    #[test]
    fn width_px_round_trips_through_width_at_point() {
        let mut s = line(StrokeGeometry::Spline);
        s.tip = VectorTip::GPen;
        s.points[1].pressure = 0.5;
        apply_ops(&mut s, &[PointOp::WidthPx { id: Some(2), index: None, px: 6.0 }], None).unwrap();
        assert!((aiai_paint::width_at_point(&s, 1) - 6.0).abs() < 1e-3);
    }

    #[test]
    fn handles_are_set_in_the_request_space_and_refused_on_corners() {
        let mut s = line(StrokeGeometry::Spline);
        s.points[1].y = 40.0;
        let before = aiai_paint::point_handle(&s, 1).unwrap();
        // canvas com o dobro do tamanho do cel: (20, 0) no canvas = (10, 0) no cel
        let half = Affine([0.5, 0.0, 0.0, 0.0, 0.5, 0.0]);
        apply_ops(&mut s, &ops(serde_json::json!([{"op": "set_handle", "id": 2, "dx": 20, "dy": 0}])), Some(&half)).unwrap();
        assert_eq!(s.points[1].handle, Some([10.0, 0.0]));
        assert_eq!(aiai_paint::point_handle(&s, 1), Some([10.0, 0.0]));
        apply_ops(&mut s, &ops(serde_json::json!([{"op": "clear_handle", "index": 1}])), None).unwrap();
        assert_eq!((s.points[1].handle, aiai_paint::point_handle(&s, 1)), (None, Some(before)), "volta à automática");
        s.points[1].corner = true;
        assert!(apply_ops(&mut s, &ops(serde_json::json!([{"op": "set_handle", "id": 2, "dx": 5, "dy": 0}])), None).unwrap_err().contains("canto"));
        let mut poly = line(StrokeGeometry::Polyline);
        assert!(apply_ops(&mut poly, &ops(serde_json::json!([{"op": "set_handle", "id": 2, "dx": 5, "dy": 0}])), None).unwrap_err().contains("spline"));
    }

    #[test]
    fn simplify_keeps_ends_corners_and_handles_and_stays_within_tolerance() {
        // reta com 11 pontos: só sobram as pontas, o canto (índice 5) e o da alça (índice 8)
        let mut s = line(StrokeGeometry::Freehand);
        s.points = (0..11).map(|i| pt(i as f32 * 20.0, 0.0)).collect();
        s.points[5].corner = true;
        s.points[8].handle = Some([6.0, 0.0]);
        let r = simplify_report(&s, 0.5).unwrap();
        assert_eq!(r.removed, vec![2, 3, 4, 5, 7, 8, 10], "{r:?}");
        assert_eq!(r.kept, 4);
        assert!(r.max_deviation <= 0.5);
        let rep = apply_ops(&mut s, &ops(serde_json::json!([{"op": "simplify", "tolerance": 0.5}])), None).unwrap();
        assert_eq!(rep.removed, r.removed);
        assert_eq!((0..s.points.len()).map(|i| s.point_id(i)).collect::<Vec<_>>(), vec![1, 6, 9, 11], "os que ficam mantêm os ids");
        assert_eq!(s.geometry, StrokeGeometry::Spline);

        // onda: tira pontos, mas a linha desenhada não sai mais que a tolerância da original
        let mut w = line(StrokeGeometry::Spline);
        w.points = (0..41).map(|i| { let x = i as f32 * 10.0; pt(x, 30.0 * (x / 400.0 * std::f32::consts::TAU).sin()) }).collect();
        for tol in [0.3f32, 1.0, 3.0] {
            let r = simplify_report(&w, tol).unwrap();
            assert!(!r.removed.is_empty() && r.max_deviation <= tol, "tol {tol}: {r:?}");
            let mut t = w.clone();
            apply_ops(&mut t, &[PointOp::Simplify { tolerance: tol }], None).unwrap();
            assert!(curve_deviation(&w, &t) <= tol + 1e-3, "tol {tol}");
        }
        let mut b = line(StrokeGeometry::Bezier);
        b.points.push(pt(300.0, 0.0));
        assert!(simplify_report(&b, 1.0).unwrap_err().contains("Bézier"));
    }
}
