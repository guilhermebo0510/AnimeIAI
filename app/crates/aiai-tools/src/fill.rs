//! Balde de tinta-e-pinta e lint de regiões não pintadas.

use crate::context::*;
use aiai_core::*;
use aiai_paint::fill::{FillParams, Regions, boundary_from_alpha, label_regions};
use aiai_render::{RenderOptions, render_frame};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FillSeed {
    pub x: f32,
    pub y: f32,
    pub color: ColorRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FillRequest {
    pub cut: Id,
    pub frame: u32,
    /// Camada de preenchimento (tipo `fill`) cujo cel exposto recebe as regiões.
    pub layer: Id,
    pub seeds: Vec<FillSeed>,
    /// Camadas de linha que delimitam as regiões; vazio = `line_refs` da camada de preenchimento.
    #[serde(default)]
    pub line_layers: Vec<Id>,
    #[serde(default = "default_gap")]
    pub gap: f32,
    #[serde(default = "default_expand")]
    pub expand: u32,
    #[serde(default = "default_threshold")]
    pub line_threshold: f32,
}

fn default_gap() -> f32 {
    3.0
}
fn default_expand() -> u32 {
    2
}
fn default_threshold() -> f32 {
    0.35
}

/// Contexto calculado uma vez: as regiões delimitadas pelas linhas no quadro.
pub struct LineRegions {
    pub regions: Regions,
    pub width: u32,
    pub height: u32,
}

pub fn compute_regions(
    p: &Project,
    blobs: &BlobStore,
    cut_id: &Id,
    frame: u32,
    line_layers: &[Id],
    params: &FillParams,
) -> ToolResult<LineRegions> {
    if line_layers.is_empty() {
        return Err(ToolError::Invalid("nenhuma camada de linha para delimitar o balde (defina line_refs ou line_layers)".into()));
    }
    let opts = RenderOptions { only_layers: Some(line_layers.to_vec()), include_guides: false, include_reference: false, background: None };
    let out = render_frame(p, blobs, cut_id, frame, &opts).map_err(|e| ToolError::Render(e.to_string()))?;
    let (w, h) = (out.image.width, out.image.height);
    let boundary = boundary_from_alpha(&out.image, params.line_threshold);
    let regions = label_regions(&boundary, w as usize, h as usize, params);
    Ok(LineRegions { regions, width: w, height: h })
}

/// Gera o comando que pinta as regiões das sementes. Regiões já pintadas que contêm a semente são substituídas.
pub fn fill_command(p: &Project, blobs: &mut BlobStore, req: &FillRequest) -> ToolResult<Command> {
    let cut = cut_data(p, &req.cut)?;
    let layer = layer_data(p, &req.layer)?;
    if layer.kind != LayerType::Fill && layer.kind != LayerType::Raster && layer.kind != LayerType::Vector {
        return Err(ToolError::Invalid("o balde pinta em camada de preenchimento (fill), raster ou vetorial".into()));
    }
    let cel_node = exposed_cel(p, cut, &req.layer, req.frame)
        .ok_or_else(|| ToolError::Invalid(format!("a camada não expõe nenhum cel no quadro {} (crie o cel e/ou a célula do timesheet)", req.frame)))?;
    let NodeKind::Cel(cel) = &cel_node.data else { unreachable!() };
    let lines = if req.line_layers.is_empty() { layer.line_refs.clone() } else { req.line_layers.clone() };
    let params = FillParams { gap: req.gap, expand: req.expand, line_threshold: req.line_threshold };
    let lr = compute_regions(p, blobs, &req.cut, req.frame, &lines, &params)?;
    let mut fills = cel.fills.clone();
    for s in &req.seeds {
        if !(s.x >= 0.0 && s.y >= 0.0 && (s.x as u32) < lr.width && (s.y as u32) < lr.height) {
            return Err(ToolError::Invalid(format!("semente ({}, {}) fora do canvas {}x{}", s.x, s.y, lr.width, lr.height)));
        }
        let (sx, sy) = (s.x as u32, s.y as u32);
        let label = lr.regions.label_near(sx, sy);
        if label == 0 {
            return Err(ToolError::Invalid(format!("semente ({}, {}) caiu sobre a linha, sem região por perto", s.x, s.y)));
        }
        let (bounds, mask) = lr.regions.mask_of(label).expect("rótulo existe");
        // remove preenchimentos anteriores que cobrem a semente
        fills.retain(|f| !fill_covers(blobs, f, sx, sy));
        let hash = blobs.put(mask.encode());
        fills.push(FillRegion { id: Id(String::new()), seed: [s.x, s.y], color: s.color.clone(), mask: Some(hash), bounds });
    }
    Ok(Command::ReplaceFills { cel: cel_node.id.clone(), fills })
}

fn fill_covers(blobs: &BlobStore, f: &FillRegion, x: u32, y: u32) -> bool {
    let [bx, by, bw, bh] = f.bounds;
    let (xi, yi) = (x as i32, y as i32);
    if xi < bx || yi < by || xi >= bx + bw || yi >= by + bh {
        return false;
    }
    let Some(h) = &f.mask else { return false };
    let Some(b) = blobs.get(h) else { return false };
    match aiai_paint::Mask::decode(&b) {
        Ok(m) => m.get((xi - bx) as u32, (yi - by) as u32) > 0.5,
        Err(_) => false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LintRegion {
    pub label: u32,
    pub area: usize,
    /// Um ponto dentro da região (para usar como semente).
    pub point: [u32; 2],
    pub bounds: [i32; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LintReport {
    pub regions_total: u32,
    pub unpainted: Vec<LintRegion>,
    pub painted: usize,
}

/// Regiões fechadas (que não tocam a borda e têm área ≥ `min_area`) sem preenchimento no cel exposto.
pub fn lint(p: &Project, blobs: &BlobStore, cut_id: &Id, frame: u32, fill_layer: &Id, gap: f32, min_area: usize) -> ToolResult<LintReport> {
    let cut = cut_data(p, cut_id)?;
    let layer = layer_data(p, fill_layer)?;
    let params = FillParams { gap, expand: 0, line_threshold: 0.35 };
    let lr = compute_regions(p, blobs, cut_id, frame, &layer.line_refs, &params)?;
    let fills: Vec<FillRegion> = match exposed_cel(p, cut, fill_layer, frame).map(|n| &n.data) {
        Some(NodeKind::Cel(c)) => c.fills.clone(),
        _ => Vec::new(),
    };
    let areas = lr.regions.areas();
    let border = lr.regions.touches_border();
    let mut unpainted = Vec::new();
    let mut painted = 0;
    // um ponto representativo por rótulo: o pixel de maior distância seria ideal; usamos o primeiro pixel interior
    let w = lr.width as usize;
    let mut first = vec![None; lr.regions.count as usize + 1];
    for (i, &l) in lr.regions.labels.iter().enumerate() {
        if l != 0 && first[l as usize].is_none() {
            first[l as usize] = Some(((i % w) as u32, (i / w) as u32));
        }
    }
    for label in 1..=lr.regions.count {
        if border[label as usize] || areas[label as usize] < min_area {
            continue;
        }
        let Some((x, y)) = first[label as usize] else { continue };
        let covered = fills.iter().any(|f| fill_covers(blobs, f, x, y));
        if covered {
            painted += 1;
        } else {
            let (bounds, _) = lr.regions.mask_of(label).expect("existe");
            unpainted.push(LintRegion { label, area: areas[label as usize], point: [x, y], bounds });
        }
    }
    Ok(LintReport { regions_total: lr.regions.count, unpainted, painted })
}
