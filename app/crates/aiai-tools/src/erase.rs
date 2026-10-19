//! Borracha vetorial: traço inteiro, só o trecho que a borracha cobre, ou o trecho entre as interseções mais próximas
//! com outros traços (como a borracha "até a interseção" dos programas de linha). Os pedaços que sobram viram traços
//! novos à mão livre com a mesma ponta, largura e cor, nos mesmos lugares da pilha.

use crate::context::*;
use aiai_core::command::IndexedStroke;
use aiai_core::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EraseMode {
    /// Apaga o traço inteiro que a borracha toca.
    Whole,
    /// Apaga só o trecho coberto pela borracha (o traço se divide).
    #[default]
    Segment,
    /// Apaga, em cada traço tocado, o trecho entre as interseções com outros traços mais próximas do toque.
    Intersection,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EraseVectorRequest {
    pub cut: Id,
    #[serde(default)]
    pub frame: u32,
    /// Camada vetorial.
    pub layer: Id,
    /// Caminho da borracha (px do canvas, espaço do cel).
    pub path: Vec<[f32; 2]>,
    /// Raio da borracha (px).
    pub radius: f32,
    #[serde(default)]
    pub mode: EraseMode,
}

fn dist_to_path(x: f32, y: f32, path: &[[f32; 2]]) -> f32 {
    if path.len() == 1 {
        return ((x - path[0][0]).powi(2) + (y - path[0][1]).powi(2)).sqrt();
    }
    path.windows(2)
        .map(|w| {
            let ([ax, ay], [bx, by]) = (w[0], w[1]);
            let (dx, dy) = (bx - ax, by - ay);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 0.0 { (((x - ax) * dx + (y - ay) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
            ((x - ax - t * dx).powi(2) + (y - ay - t * dy).powi(2)).sqrt()
        })
        .fold(f32::MAX, f32::min)
}

/// Parâmetro (índice fracionário em `a`) onde o segmento a[i]→a[i+1] cruza algum segmento de `b`.
fn crossings(a: &[(f32, f32, f32)], b: &[(f32, f32, f32)], out: &mut Vec<f32>) {
    for i in 0..a.len().saturating_sub(1) {
        let (p, r) = ((a[i].0, a[i].1), (a[i + 1].0 - a[i].0, a[i + 1].1 - a[i].1));
        let (minx, maxx) = (a[i].0.min(a[i + 1].0), a[i].0.max(a[i + 1].0));
        let (miny, maxy) = (a[i].1.min(a[i + 1].1), a[i].1.max(a[i + 1].1));
        for j in 0..b.len().saturating_sub(1) {
            if b[j].0.max(b[j + 1].0) < minx || b[j].0.min(b[j + 1].0) > maxx || b[j].1.max(b[j + 1].1) < miny || b[j].1.min(b[j + 1].1) > maxy {
                continue;
            }
            let (q, s) = ((b[j].0, b[j].1), (b[j + 1].0 - b[j].0, b[j + 1].1 - b[j].1));
            let den = r.0 * s.1 - r.1 * s.0;
            if den.abs() < 1e-9 {
                continue;
            }
            let (qp0, qp1) = (q.0 - p.0, q.1 - p.1);
            let t = (qp0 * s.1 - qp1 * s.0) / den;
            let u = (qp0 * r.1 - qp1 * r.0) / den;
            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                out.push(i as f32 + t);
            }
        }
    }
}

/// Pedaço do traço como traço novo à mão livre (pontos a cada ~2 px).
fn piece(src: &Stroke, pts: &[(f32, f32, f32)]) -> Option<Stroke> {
    let mut keep: Vec<StrokePoint> = Vec::new();
    for (k, &(x, y, p)) in pts.iter().enumerate() {
        let last = k + 1 == pts.len();
        if keep.last().is_none_or(|l| ((l.x - x).powi(2) + (l.y - y).powi(2)).sqrt() >= 2.0) || last {
            keep.push(StrokePoint { x, y, pressure: p, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None });
        }
    }
    if keep.len() < 2 {
        return None;
    }
    let len: f32 = keep.windows(2).map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt()).sum();
    if len < src.width.max(1.0) {
        return None; // sobra de um ou dois pixels: some junto
    }
    let mut s = src.clone();
    s.id = Id(String::new());
    s.points = keep;
    s.geometry = StrokeGeometry::Freehand;
    s.closed = false;
    s.profile = WidthProfile::default();
    Some(s)
}

pub fn erase_vector_command(p: &Project, req: &EraseVectorRequest) -> ToolResult<Command> {
    let cut = cut_data(p, &req.cut)?;
    let layer = layer_data(p, &req.layer)?;
    if layer.kind != LayerType::Vector {
        return Err(ToolError::Invalid("borracha vetorial apaga em camada vetorial".into()));
    }
    if layer.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    if req.path.is_empty() {
        return Err(ToolError::Invalid("caminho da borracha vazio".into()));
    }
    let node = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {}", req.frame)))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    let r = req.radius.max(0.5);
    let flat: Vec<Vec<(f32, f32, f32)>> = cel.strokes.iter().map(aiai_paint::flatten_points).collect();
    let mut removed = Vec::new();
    let mut inserted = Vec::new();
    for (si, s) in cel.strokes.iter().enumerate() {
        let pts = &flat[si];
        // raio do toque inclui a meia largura do traço
        let hit_r = r + s.width * 0.5;
        let hit: Vec<bool> = pts.iter().map(|q| dist_to_path(q.0, q.1, &req.path) <= hit_r).collect();
        if !hit.iter().any(|&h| h) {
            continue;
        }
        removed.push(s.id.clone());
        let pieces: Vec<Vec<(f32, f32, f32)>> = match req.mode {
            EraseMode::Whole => Vec::new(),
            EraseMode::Segment => {
                let mut out: Vec<Vec<(f32, f32, f32)>> = Vec::new();
                let mut cur = Vec::new();
                for (q, &h) in pts.iter().zip(&hit) {
                    if h {
                        if !cur.is_empty() {
                            out.push(std::mem::take(&mut cur));
                        }
                    } else {
                        cur.push(*q);
                    }
                }
                if !cur.is_empty() {
                    out.push(cur);
                }
                // traço fechado: o primeiro e o último pedaço são o mesmo trecho passando pela emenda
                if s.closed && out.len() >= 2 && !hit[0] && !hit[hit.len() - 1] {
                    let first = out.remove(0);
                    out.last_mut().expect("tem").extend(first);
                }
                out
            }
            EraseMode::Intersection => {
                let mut xs = Vec::new();
                for (sj, o) in flat.iter().enumerate() {
                    if sj != si {
                        crossings(pts, o, &mut xs);
                    }
                }
                // ponto tocado mais "central" do toque
                let touched: Vec<usize> = hit.iter().enumerate().filter(|(_, h)| **h).map(|(i, _)| i).collect();
                let t_hit = touched[touched.len() / 2] as f32;
                let lo = xs.iter().copied().filter(|&t| t < t_hit).fold(f32::NEG_INFINITY, f32::max);
                let hi = xs.iter().copied().filter(|&t| t > t_hit).fold(f32::INFINITY, f32::min);
                let n = pts.len() as f32;
                let mut out = Vec::new();
                if lo.is_finite() {
                    let k = lo.floor() as usize;
                    let mut a: Vec<_> = pts[..=k].to_vec();
                    let f = lo - k as f32;
                    if k + 1 < pts.len() {
                        let (p0, p1) = (pts[k], pts[k + 1]);
                        a.push((p0.0 + (p1.0 - p0.0) * f, p0.1 + (p1.1 - p0.1) * f, p0.2));
                    }
                    out.push(a);
                }
                if hi.is_finite() && hi < n - 1.0 {
                    let k = hi.floor() as usize;
                    let f = hi - k as f32;
                    let (p0, p1) = (pts[k], pts[(k + 1).min(pts.len() - 1)]);
                    let mut b = vec![(p0.0 + (p1.0 - p0.0) * f, p0.1 + (p1.1 - p0.1) * f, p0.2)];
                    b.extend_from_slice(&pts[k + 1..]);
                    out.push(b);
                }
                out
            }
        };
        for pc in &pieces {
            if let Some(ns) = piece(s, pc) {
                inserted.push((si, ns));
            }
        }
    }
    if removed.is_empty() {
        return Err(ToolError::Invalid("a borracha não tocou nenhum traço".into()));
    }
    // inserções na ordem final: os pedaços ocupam o lugar do traço de origem na pilha já sem os removidos
    let removed_set: std::collections::HashSet<&Id> = removed.iter().collect();
    let mut final_index = 0usize;
    let mut items = Vec::new();
    for (si, s) in cel.strokes.iter().enumerate() {
        if removed_set.contains(&s.id) {
            for (_, ns) in inserted.iter().filter(|(o, _)| *o == si) {
                items.push(IndexedStroke { index: final_index, stroke: ns.clone() });
                final_index += 1;
            }
        } else {
            final_index += 1;
        }
    }
    let mut cmds = vec![Command::RemoveStrokes { cel: node.id.clone(), ids: removed }];
    if !items.is_empty() {
        cmds.push(Command::InsertStrokes { cel: node.id.clone(), items });
    }
    Ok(Command::Batch { commands: cmds, label: Some("Borracha vetorial".into()) })
}
