//! Comparação de um quadro renderizado com a referência pela régua do protocolo (07-PROTOCOLO-DE-VALIDACAO.md §5,
//! decisões 20 e 21): FLIP relativo ao piso de ruído do corte, SSIM, ΔE em regiões chapadas, F1 de bordas, espessura.

use crate::calib::load_reference;
use crate::context::*;
use aiai_core::*;
use aiai_metrics::{CompareOptions, CompareReport, compare_with_artifacts, diff_visualization, edge_overlay};
use aiai_render::{RenderOptions, render_frame};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompareRequest {
    pub cut: Id,
    pub frame: u32,
    /// PNG do quadro de referência.
    pub reference: String,
    /// Piso de ruído FLIP do corte (FLIP entre dois quadros idênticos da referência). Omitido = 0.
    #[serde(default)]
    pub noise_floor: f32,
    /// Pasta onde gravar render, mapa FLIP e sobreposição de bordas.
    #[serde(default)]
    pub out_dir: Option<String>,
    /// Fundo sob a transparência do render (sRGB).
    #[serde(default = "white")]
    pub background: [f32; 3],
}

fn white() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameVerdict {
    pub report: CompareReport,
    pub noise_floor: f32,
    pub flip_limit: f64,
    pub flip_hard_limit: f64,
    pub line_width_ok: bool,
    pub pass: bool,
    pub files: Vec<String>,
}

/// Regra de espessura (decisão 21): média dentro de ±5% ou diferença absoluta ≤ 0,3 px.
fn line_width_ok(r: &CompareReport) -> bool {
    let v = serde_json::to_value(&r.line_width).unwrap_or_default();
    let get = |side: &str, k: &str| v.get(side).and_then(|s| s.get(k)).and_then(|x| x.as_f64());
    match (get("reference", "mean"), get("test", "mean")) {
        (Some(a), Some(b)) if a > 0.0 => ((b - a).abs() / a <= 0.05) || (b - a).abs() <= 0.3,
        _ => r.verdict.line_width_ok,
    }
}

pub fn compare_frame(p: &Project, blobs: &BlobStore, req: &CompareRequest) -> ToolResult<FrameVerdict> {
    let reference = load_reference(&req.reference)?;
    let out = render_frame(p, blobs, &req.cut, req.frame, &RenderOptions { background: Some(req.background), ..Default::default() }).map_err(|e| ToolError::Render(e.to_string()))?;
    if (out.image.width, out.image.height) != (reference.width, reference.height) {
        return Err(ToolError::Invalid(format!("render {}x{} ≠ referência {}x{}", out.image.width, out.image.height, reference.width, reference.height)));
    }
    let mut opts = CompareOptions::default();
    let floor = req.noise_floor.max(0.0) as f64;
    opts.thresholds.flip_mean_max = floor + 0.05;
    opts.thresholds.flip_mean_hard_max = floor + 0.10;
    let (report, art) = compare_with_artifacts(&reference, &out.image, &opts).map_err(ToolError::Invalid)?;
    let lw_ok = line_width_ok(&report);
    let v = &report.verdict;
    let pass = v.flip_ok && v.ssim_ok && v.delta_e_flat_ok && v.edge_f1_ok && lw_ok && v.palette_ok && v.flip_hard_ok;
    let mut files = Vec::new();
    if let Some(dir) = &req.out_dir {
        let d = std::path::Path::new(dir);
        std::fs::create_dir_all(d).map_err(|e| ToolError::Invalid(e.to_string()))?;
        let stem = format!("q{:04}", req.frame);
        let save = |img: &aiai_paint::Image, name: &str, files: &mut Vec<String>| -> ToolResult<()> {
            let path = d.join(format!("{stem}_{name}.png"));
            img.save_png(&path).map_err(ToolError::Invalid)?;
            files.push(path.to_string_lossy().to_string());
            Ok(())
        };
        save(&out.image, "render", &mut files)?;
        save(&diff_visualization(&art.flip), "flip", &mut files)?;
        save(&edge_overlay(&reference, &art.ref_edges, &art.test_edges, opts.edge_tolerance_px), "bordas", &mut files)?;
        let rep_path = d.join(format!("{stem}_relatorio.json"));
        std::fs::write(&rep_path, serde_json::to_string_pretty(&report).unwrap_or_default()).map_err(|e| ToolError::Invalid(e.to_string()))?;
        files.push(rep_path.to_string_lossy().to_string());
    }
    Ok(FrameVerdict { flip_limit: floor + 0.05, flip_hard_limit: floor + 0.10, noise_floor: req.noise_floor, line_width_ok: lw_ok, pass, report, files })
}
