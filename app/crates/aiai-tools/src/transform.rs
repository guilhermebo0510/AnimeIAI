//! Transformar, deformar e repetir conteúdo de um cel (vetor sem perda; raster por reamostragem), opcionalmente só
//! dentro de uma seleção.

use crate::context::*;
use crate::paint::{cel_raster, tiles_for_region};
use aiai_core::command::IndexedStroke;
use aiai_core::*;
use aiai_paint::Image;
use aiai_paint::select::{SelectShape, Selection};
use aiai_paint::warp::{Homography, Liquify, MeshWarp, positions_along, resample_inverse};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Deform {
    /// Translação, escala (x, y), rotação (graus) e cisalhamento em torno de `pivot`.
    Affine {
        #[serde(default)]
        translate: [f32; 2],
        #[serde(default = "unit2")]
        scale: [f32; 2],
        #[serde(default)]
        rotate: f32,
        #[serde(default)]
        shear: [f32; 2],
        pivot: [f32; 2],
    },
    /// Leva o quadrilátero `from` (4 cantos: sup-esq, sup-dir, inf-dir, inf-esq) para `to`.
    Perspective { from: [[f32; 2]; 4], to: [[f32; 2]; 4] },
    Mesh(MeshWarp),
    Liquify(Liquify),
    /// Pincelada de liquify: os passos em ordem (o Liquefazer do Mover grava uma por pincelada).
    LiquifyPath { steps: Vec<Liquify> },
    /// Espelha no eixo vertical x = valor (horizontal = true) ou horizontal y = valor.
    Flip { horizontal: bool, axis: f32 },
}

fn unit2() -> [f32; 2] {
    [1.0, 1.0]
}

/// `Deform::Affine` (pivô na origem) que faz exatamente a matriz `m` = [a, b, tx, d, e, ty] (x' = a·x + b·y + tx):
/// decomposição QR da parte linear em rotação · cisalhamento · escala. Serve para levar uma transformação feita no
/// canvas para o espaço do cel de uma camada com movimento (P⁻¹·A·P não é só deslocar/escalar/girar).
pub fn affine_deform(m: [f32; 6]) -> Deform {
    let (a, b, d, e) = (m[0], m[1], m[3], m[4]);
    let sx = a.hypot(d);
    if sx < 1e-9 {
        return Deform::Affine { translate: [m[2], m[5]], scale: [0.0, 0.0], rotate: 0.0, shear: [0.0, 0.0], pivot: [0.0, 0.0] };
    }
    let (c, s) = (a / sx, d / sx);
    // Rᵀ·L = [[sx, u01], [0, sy]] com u01 = shear_x · sy
    let u01 = c * b + s * e;
    let sy = -s * b + c * e;
    let shx = if sy.abs() > 1e-9 { u01 / sy } else { 0.0 };
    Deform::Affine { translate: [m[2], m[5]], scale: [sx, sy], rotate: s.atan2(c).to_degrees(), shear: [shx, 0.0], pivot: [0.0, 0.0] }
}

impl Deform {
    fn affine_matrix(&self) -> Option<[f32; 6]> {
        match self {
            Deform::Affine { translate, scale, rotate, shear, pivot } => {
                let (s, c) = rotate.to_radians().sin_cos();
                // M = T(pivot + translate) · R · Sh · S · T(-pivot)
                let (a0, b0, d0, e0) = (scale[0], shear[0] * scale[1], shear[1] * scale[0], scale[1]);
                let (a, b, d, e) = (c * a0 - s * d0, c * b0 - s * e0, s * a0 + c * d0, s * b0 + c * e0);
                let tx = pivot[0] + translate[0] - (a * pivot[0] + b * pivot[1]);
                let ty = pivot[1] + translate[1] - (d * pivot[0] + e * pivot[1]);
                Some([a, b, tx, d, e, ty])
            }
            Deform::Flip { horizontal, axis } => Some(if *horizontal { [-1.0, 0.0, 2.0 * axis, 0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0, 0.0, -1.0, 2.0 * axis] }),
            _ => None,
        }
    }

    /// Ponto original → deformado.
    pub fn forward(&self, x: f32, y: f32) -> ToolResult<(f32, f32)> {
        if let Some(m) = self.affine_matrix() {
            return Ok((m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5]));
        }
        match self {
            Deform::Perspective { from, to } => {
                let h = Homography::from_quads(*from, *to).ok_or_else(|| ToolError::Invalid("quadriláteros degenerados".into()))?;
                Ok(h.apply(x, y))
            }
            Deform::Mesh(m) => {
                m.validate().map_err(ToolError::Invalid)?;
                Ok(m.forward(x, y))
            }
            Deform::Liquify(l) => Ok(l.forward(x, y)),
            Deform::LiquifyPath { steps } => Ok(aiai_paint::warp::liquify_path_forward(steps, x, y)),
            _ => unreachable!(),
        }
    }

    /// Fator médio de escala (para a largura dos traços).
    fn width_scale(&self) -> f32 {
        match self.affine_matrix() {
            Some(m) => ((m[0] * m[4] - m[1] * m[3]).abs()).sqrt(),
            None => 1.0,
        }
    }

    fn image(&self, src: &Image) -> ToolResult<Image> {
        let (w, h) = (src.width, src.height);
        if let Some(m) = self.affine_matrix() {
            return Ok(aiai_paint::transform_image(src, aiai_paint::Affine(m), w, h));
        }
        match self {
            Deform::Perspective { from, to } => {
                let hm = Homography::from_quads(*from, *to).ok_or_else(|| ToolError::Invalid("quadriláteros degenerados".into()))?;
                Ok(aiai_paint::warp::perspective_image(src, &hm))
            }
            Deform::Mesh(mw) => {
                mw.validate().map_err(ToolError::Invalid)?;
                Ok(mw.warp_image(src))
            }
            Deform::Liquify(l) => Ok(l.liquify_image(src)),
            Deform::LiquifyPath { steps } => Ok(aiai_paint::warp::liquify_path_image(steps, src)),
            _ => Ok(resample_inverse(src, w, h, |x, y| Some((x, y)))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TransformRequest {
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    pub deform: Deform,
    /// Só o que está dentro da seleção (vetor: traços cujo centro da caixa está dentro).
    #[serde(default)]
    pub selection: Option<SelectShape>,
    /// Vetor: escala a largura dos traços junto (padrão: sim).
    #[serde(default = "yes")]
    pub scale_width: bool,
    /// Vetor: só estes traços (ids); vazio = todos (ou os da seleção).
    #[serde(default)]
    pub strokes: Vec<Id>,
}

fn yes() -> bool {
    true
}

/// Um traço vai junto com a seleção quando a maioria dos seus pontos está dentro dela (o mesmo critério na
/// prévia da interface e no comando; o centro da caixa puxava traços grandes que mal tocavam a seleção).
pub fn stroke_in_selection(s: &Stroke, inside: impl Fn(f32, f32) -> bool) -> bool {
    if s.points.is_empty() {
        return false;
    }
    let n = s.points.iter().filter(|p| inside(p.x, p.y)).count();
    n * 2 >= s.points.len()
}

/// Imagem que a varinha mágica e a faixa de cor leem: os pixels do cel raster, ou a camada vetorial renderizada
/// (o cel vetorial não tem tiles).
pub fn selection_reference(p: &Project, blobs: &BlobStore, cut_id: &Id, layer_id: &Id, frame: u32) -> ToolResult<Image> {
    let cut = cut_data(p, cut_id)?;
    let layer = layer_data(p, layer_id)?;
    let (w, h) = canvas_size(p, cut);
    if layer.kind == LayerType::Vector {
        let opts = aiai_render::RenderOptions { only_layers: Some(vec![layer_id.clone()]), camera: false, layer_motion: false, ..Default::default() };
        return Ok(aiai_render::render_frame(p, blobs, cut_id, frame, &opts).map_err(|e| ToolError::Render(e.to_string()))?.image);
    }
    let node = exposed_cel(p, cut, layer_id, frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {frame}")))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    cel_raster(blobs, cel, w, h)
}

pub fn transform_command(p: &Project, blobs: &mut BlobStore, req: &TransformRequest) -> ToolResult<Command> {
    let cut = cut_data(p, &req.cut)?;
    let (w, h) = canvas_size(p, cut);
    let sel = match &req.selection {
        Some(shape) => {
            let reference = if matches!(shape, SelectShape::Wand { .. } | SelectShape::ColorRange { .. }) { Some(selection_reference(p, blobs, &req.cut, &req.layer, req.frame)?) } else { None };
            Some(Selection::from_shape(shape, w, h, reference.as_ref()).map_err(ToolError::Invalid)?)
        }
        None => None,
    };
    transform_command_masked(p, blobs, req, sel.as_ref())
}

/// Como [`transform_command`], com a seleção já pronta como máscara de cobertura do tamanho do canvas, no espaço do
/// cel (somar/subtrair/intersectar, varinha e suavizar não cabem numa `SelectShape`); `req.selection` é ignorada.
pub fn transform_command_masked(p: &Project, blobs: &mut BlobStore, req: &TransformRequest, sel: Option<&Selection>) -> ToolResult<Command> {
    let cut = cut_data(p, &req.cut)?;
    let layer = layer_data(p, &req.layer)?;
    if layer.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    let node = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {}", req.frame)))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    let (w, h) = canvas_size(p, cut);
    if sel.is_some_and(|s| s.width != w || s.height != h) {
        return Err(ToolError::Invalid("a máscara da seleção não tem o tamanho do canvas".into()));
    }
    match layer.kind {
        LayerType::Vector => {
            let ws = if req.scale_width { req.deform.width_scale() } else { 1.0 };
            let mut changed = Vec::new();
            for s in &cel.strokes {
                if !req.strokes.is_empty() && !req.strokes.contains(&s.id) {
                    continue;
                }
                if let Some(sel) = sel {
                    let inside = |x: f32, y: f32| {
                        let (ix, iy) = (x.floor() as i64, y.floor() as i64);
                        ix >= 0 && iy >= 0 && ix < w as i64 && iy < h as i64 && sel.cov[(iy as u32 * w + ix as u32) as usize] >= 0.5
                    };
                    if !stroke_in_selection(s, inside) {
                        continue;
                    }
                }
                let mut ns = s.clone();
                for pt in &mut ns.points {
                    let (x, y) = req.deform.forward(pt.x, pt.y)?;
                    pt.x = x;
                    pt.y = y;
                }
                ns.width = (ns.width * ws).clamp(0.05, 2000.0);
                changed.push(ns);
            }
            if changed.is_empty() {
                return Err(ToolError::Invalid("nenhum traço selecionado para transformar".into()));
            }
            Ok(Command::ReplaceStrokes { cel: node.id.clone(), strokes: changed })
        }
        LayerType::Raster => {
            let src = cel_raster(blobs, cel, w, h)?;
            let result = match sel {
                None => req.deform.image(&src)?,
                Some(sel) => {
                    // flutuante = parte selecionada; base = o resto
                    let mut floating = src.clone();
                    let mut base = src.clone();
                    for (i, &c) in sel.cov.iter().enumerate() {
                        for k in 0..4 {
                            floating.data[i][k] *= c;
                            base.data[i][k] *= 1.0 - c;
                        }
                    }
                    let moved = req.deform.image(&floating)?;
                    base.draw_over(&moved, 0, 0, 1.0);
                    base
                }
            };
            Ok(Command::SetTiles { cel: node.id.clone(), tiles: tiles_for_region(blobs, &result, [0, 0, w as i32, h as i32]) })
        }
        other => Err(ToolError::Invalid(format!("transformar ainda não suporta camada {other:?}"))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RepeatRequest {
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    /// Traços a repetir (o "motivo"), centrados em `origin`.
    pub strokes: Vec<Id>,
    pub origin: [f32; 2],
    /// Caminho (polilinha) ao longo do qual repetir.
    pub path: Vec<[f32; 2]>,
    pub spacing: f32,
    #[serde(default)]
    pub offset: f32,
    /// Gira o motivo seguindo a tangente do caminho.
    #[serde(default)]
    pub follow: bool,
    /// Escala no início e no fim do caminho (perspectiva simples: diminui ao afastar).
    #[serde(default = "unit2")]
    pub scale_range: [f32; 2],
}

/// Repetição ao longo de caminho (colunas, balaústres, janelas, babados) para camadas vetoriais.
pub fn repeat_command(p: &Project, req: &RepeatRequest) -> ToolResult<Command> {
    let cut = cut_data(p, &req.cut)?;
    let node = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid("a camada não expõe cel nesse quadro".into()))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    let motif: Vec<&Stroke> = cel.strokes.iter().filter(|s| req.strokes.contains(&s.id)).collect();
    if motif.is_empty() {
        return Err(ToolError::Invalid("nenhum traço do motivo encontrado".into()));
    }
    let pos = positions_along(&req.path, req.spacing, req.offset);
    let n = pos.len().max(1);
    let mut items = Vec::new();
    let mut idx = cel.strokes.len();
    for (k, &(x, y, ang)) in pos.iter().enumerate() {
        let t = if n > 1 { k as f32 / (n - 1) as f32 } else { 0.0 };
        let sc = req.scale_range[0] + (req.scale_range[1] - req.scale_range[0]) * t;
        let a = if req.follow { ang.to_radians() } else { 0.0 };
        let (sn, cs) = a.sin_cos();
        for s in &motif {
            let mut ns = (*s).clone();
            ns.id = Id(String::new());
            for pt in &mut ns.points {
                let (dx, dy) = ((pt.x - req.origin[0]) * sc, (pt.y - req.origin[1]) * sc);
                pt.x = x + dx * cs - dy * sn;
                pt.y = y + dx * sn + dy * cs;
            }
            ns.width = (ns.width * sc).max(0.05);
            items.push(IndexedStroke { index: idx, stroke: ns });
            idx += 1;
        }
    }
    Ok(Command::InsertStrokes { cel: node.id.clone(), items })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affine_deform_reproduces_any_matrix() {
        let cases = [[1.0, 0.0, 5.0, 0.0, 1.0, -3.0], [2.0, 0.5, 1.0, -0.3, 0.7, 2.0], [-1.0, 0.0, 10.0, 0.0, 1.0, 0.0], [0.0, -1.0, 0.0, 1.0, 0.0, 0.0], [0.5, 0.0, 3.0, 0.0, -2.0, 1.0]];
        for m in cases {
            let d = affine_deform(m);
            for (x, y) in [(0.0f32, 0.0f32), (10.0, 3.0), (-4.0, 7.0)] {
                let (fx, fy) = d.forward(x, y).unwrap();
                let (ex, ey) = (m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5]);
                assert!((fx - ex).abs() < 1e-3 && (fy - ey).abs() < 1e-3, "{m:?} em ({x}, {y}): {fx}, {fy} != {ex}, {ey}");
            }
        }
    }
}
