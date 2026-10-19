//! Pontos de controle de um traço vetorial pelo endereço (id do traço, índice do ponto): ler e editar em lote.
//!
//! O ponto não tem id gravado (traço à mão livre tem centenas): o endereço é o índice, protegido pela guarda
//! `stroke_rev` (impressão digital do traço) e/ou `point_count`. Edição com guarda que não bate é recusada
//! ([`ToolError::Stale`]), então um índice lido antes de outra mudança nunca edita o ponto errado.
//!
//! As operações do lote rodam em ordem e cada índice se refere ao traço como ficou depois das anteriores; o lote
//! inteiro vira um `ReplaceStrokes` (mesmo id, um passo de desfazer). O painel Editar pontos do app usa as mesmas
//! funções ([`nearest_on_curve`], [`insert_on_curve`], [`delete_point`]).

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
    StrokePoint { x, y, pressure: a.pressure + (b.pressure - a.pressure) * f, tilt: None, t: None, corner: false, width: a.width + (b.width - a.width) * f }
}

/// Põe um ponto em cima da curva, no lugar mais perto de (x, y) (até `tol`); devolve o índice novo.
pub fn insert_on_curve(s: &mut Stroke, x: f32, y: f32, tol: f32) -> Option<usize> {
    let hit = nearest_on_curve(s, x, y)?;
    if hit.dist > tol {
        return None;
    }
    let n = s.points.len();
    let q = between(s.points[hit.seg], s.points[(hit.seg + 1) % n], hit.frac, hit.x, hit.y);
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
    s.points.insert(after + 1, between(a, b, 0.5, x, y));
    unbezier(s);
    Ok(after + 1)
}

/// Apaga o ponto `i` (o traço fica com pelo menos 2).
pub fn delete_point(s: &mut Stroke, i: usize) -> bool {
    if s.points.len() <= 2 || i >= s.points.len() {
        return false;
    }
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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PointInfo {
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

/// Pontos do traço como a leitura devolve (cel e canvas).
pub fn describe(p: &Project, cel: &Id, s: &Stroke, place: &Affine) -> StrokePoints {
    let points = s
        .points
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let (cx, cy) = place.apply(q.x, q.y);
            PointInfo { index: i, x: q.x, y: q.y, canvas_x: cx, canvas_y: cy, pressure: q.pressure, corner: q.corner, width: q.width, width_px: aiai_paint::width_at_point(s, i) }
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
        points,
    }
}

/// Lê os pontos de um traço (pelo id ou pelo clique `at`).
pub fn read_points(p: &Project, req: &PointsRequest) -> ToolResult<StrokePoints> {
    let (node, cel) = vector_cel(p, &req.cut, &req.layer, req.frame)?;
    let place = cel_placement(p, &req.cut, &req.layer, req.frame)?;
    match (&req.stroke, req.at) {
        (Some(id), None) => Ok(describe(p, &node.id, find_stroke(cel, id)?, &place)),
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
            Ok(out)
        }
        (None, None) => Err(ToolError::Invalid("informe 'stroke' (id do traço) ou 'at' ([x, y] em px do canvas)".into())),
    }
}

// ----------------------------------------------------------------------------------------------------- edição

/// Espaço das coordenadas x/y das operações.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PointSpace {
    /// Coordenadas do cel (as `x`/`y` da leitura).
    #[default]
    Cel,
    /// Px do canvas do corte (as `canvas_x`/`canvas_y` da leitura; o movimento da camada é desfeito).
    Canvas,
}

/// Uma operação sobre os pontos. Os índices se referem ao traço como ficou depois das operações anteriores do lote.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointOp {
    /// Move o ponto `index` para (x, y).
    Move { index: usize, x: f32, y: f32 },
    /// Com `after`: põe o ponto (x, y) logo depois do índice `after`. Sem `after`: põe um ponto EM CIMA da curva, no
    /// lugar mais perto de (x, y), se ele estiver a até `tolerance` px (padrão 12) da curva.
    Insert {
        #[serde(default)]
        after: Option<usize>,
        x: f32,
        y: f32,
        #[serde(default)]
        tolerance: Option<f32>,
    },
    /// Apaga o ponto (o traço fica com pelo menos 2).
    Delete { index: usize },
    /// Canto (a curva quebra no ponto) ou suave. Vale para spline e mão livre.
    Corner { index: usize, on: bool },
    /// Largura em px naquele ponto (vira o multiplicador do ponto, 0..20× a largura da ponta).
    WidthPx { index: usize, px: f32 },
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
    /// Espaço de x/y em move/insert: cel (padrão) ou canvas.
    #[serde(default)]
    pub space: PointSpace,
    pub ops: Vec<PointOp>,
}

/// Resultado da edição: o comando a aplicar, o traço como fica e os índices dos pontos inseridos (no traço final).
#[derive(Debug, Clone)]
pub struct PointsEdit {
    pub command: Command,
    pub stroke: Stroke,
    pub cel: Id,
    pub inserted: Vec<usize>,
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

/// Aplica o lote ao traço (sem guarda).
pub fn apply_ops(s: &mut Stroke, ops: &[PointOp], to_cel: impl Fn(f32, f32) -> (f32, f32), canvas_scale: f32) -> Result<Vec<usize>, String> {
    let mut inserted: Vec<usize> = Vec::new();
    for (k, op) in ops.iter().enumerate() {
        let n = s.points.len();
        let check = |i: usize| if i < n { Ok(i) } else { Err(format!("ops[{k}]: índice {i} fora do traço (pontos 0..{})", n.saturating_sub(1))) };
        let finite = |v: f32, name: &str| if v.is_finite() { Ok(v) } else { Err(format!("ops[{k}]: '{name}' deve ser um número finito")) };
        match *op {
            PointOp::Move { index, x, y } => {
                let i = check(index)?;
                let (cx, cy) = to_cel(finite(x, "x")?, finite(y, "y")?);
                s.points[i].x = cx;
                s.points[i].y = cy;
            }
            PointOp::Insert { after, x, y, tolerance } => {
                let (cx, cy) = to_cel(finite(x, "x")?, finite(y, "y")?);
                let at = match after {
                    Some(a) => insert_after(s, a, cx, cy).map_err(|e| format!("ops[{k}]: {e}"))?,
                    None => {
                        let tol = tolerance.unwrap_or(12.0) * canvas_scale;
                        insert_on_curve(s, cx, cy, tol).ok_or_else(|| {
                            let d = nearest_on_curve(s, cx, cy).map(|h| format!("{:.1}", h.dist)).unwrap_or_else(|| "?".into());
                            format!("ops[{k}]: ({x}, {y}) está a {d} px da curva, mais que a tolerância {}; aumente 'tolerance' ou use 'after'", tolerance.unwrap_or(12.0))
                        })?
                    }
                };
                for j in &mut inserted {
                    if *j >= at {
                        *j += 1;
                    }
                }
                inserted.push(at);
            }
            PointOp::Delete { index } => {
                let i = check(index)?;
                if !delete_point(s, i) {
                    return Err(format!("ops[{k}]: o traço precisa de pelo menos 2 pontos (tem {n})"));
                }
                inserted.retain(|&j| j != i);
                for j in &mut inserted {
                    if *j > i {
                        *j -= 1;
                    }
                }
            }
            PointOp::Corner { index, on } => {
                let i = check(index)?;
                s.points[i].corner = on;
            }
            PointOp::WidthPx { index, px } => {
                let i = check(index)?;
                let px = finite(px, "px")?;
                if px < 0.0 {
                    return Err(format!("ops[{k}]: 'px' deve ser >= 0"));
                }
                s.points[i].width = aiai_paint::point_width_for(s, i, px);
            }
        }
    }
    Ok(inserted)
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
    let (to_cel, scale): (Box<dyn Fn(f32, f32) -> (f32, f32)>, f32) = match req.space {
        PointSpace::Cel => (Box::new(|x, y| (x, y)), 1.0),
        PointSpace::Canvas => {
            let inv = cel_placement(p, &req.cut, &req.layer, req.frame)?.inverse().ok_or_else(|| ToolError::Invalid("o movimento da camada neste quadro não tem inversa (escala 0)".into()))?;
            let (x0, y0) = inv.apply(0.0, 0.0);
            let (x1, y1) = inv.apply(1.0, 0.0);
            (Box::new(move |x, y| inv.apply(x, y)), (x1 - x0).hypot(y1 - y0))
        }
    };
    let mut ns = s.clone();
    let inserted = apply_ops(&mut ns, &req.ops, to_cel, scale).map_err(ToolError::Invalid)?;
    ns.validate().map_err(|e| ToolError::Invalid(format!("o traço editado ficaria inválido: {e}")))?;
    Ok(PointsEdit { command: Command::ReplaceStrokes { cel: node.id.clone(), strokes: vec![ns.clone()] }, stroke: ns, cel: node.id.clone(), inserted })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> StrokePoint {
        StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0 }
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

    #[test]
    fn apply_ops_runs_in_order_and_tracks_inserted_indices() {
        let mut s = line(StrokeGeometry::Spline);
        let ops: Vec<PointOp> = serde_json::from_value(serde_json::json!([
            {"op": "insert", "after": 0, "x": 50, "y": 10},
            {"op": "move", "index": 3, "x": 210, "y": 20},
            {"op": "corner", "index": 2, "on": true},
            {"op": "width_px", "index": 0, "px": 8},
            {"op": "insert", "x": 100, "y": 3},
            {"op": "delete", "index": 0}
        ]))
        .unwrap();
        let inserted = apply_ops(&mut s, &ops, |x, y| (x, y), 1.0).unwrap();
        // depois de apagar o 0, o primeiro inserido (1) vira 0
        assert_eq!(s.points.len(), 4);
        assert_eq!(inserted.len(), 2);
        assert_eq!((s.points[0].x, s.points[0].y), (50.0, 10.0));
        assert!(s.points.iter().any(|q| q.corner && q.x == 100.0));
        assert_eq!((s.points[3].x, s.points[3].y), (210.0, 20.0));
        assert!(apply_ops(&mut s, &serde_json::from_value::<Vec<PointOp>>(serde_json::json!([{"op": "move", "index": 9, "x": 0, "y": 0}])).unwrap(), |x, y| (x, y), 1.0).unwrap_err().contains("fora do traço"));
        assert!(serde_json::from_value::<PointOp>(serde_json::json!({"op": "move", "index": 0, "x": 0, "y": 0, "z": 1})).is_err(), "chave desconhecida na operação");
    }

    #[test]
    fn width_px_round_trips_through_width_at_point() {
        let mut s = line(StrokeGeometry::Spline);
        s.tip = VectorTip::GPen;
        s.points[1].pressure = 0.5;
        apply_ops(&mut s, &[PointOp::WidthPx { index: 1, px: 6.0 }], |x, y| (x, y), 1.0).unwrap();
        assert!((aiai_paint::width_at_point(&s, 1) - 6.0).abs() < 1e-3);
    }
}
