//! Comparação de um quadro renderizado com a referência pela régua do protocolo (07-PROTOCOLO-DE-VALIDACAO.md §5,
//! decisões 20, 21, 23 e 24): FLIP, SSIM e F1 de bordas limitados pelo valor da própria referência sob a perturbação
//! P5T (5% de cor, 1 px de deslocamento e tremor suave de 1 px de decalque), ΔE em regiões chapadas, espessura.

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
    /// Piso de ruído FLIP do corte (FLIP entre dois quadros idênticos da referência), só registrado no relatório; os
    /// limites de FLIP vêm da perturbação P5 da própria referência (decisão 23). Omitido = 0.
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
    /// SSIM e F1 mínimos aplicados (decisão 25: F1 ≥ 0,90 a 4 px; SSIM ≥ min(P5T, 0,93)).
    pub ssim_limit: f64,
    pub edge_f1_limit: f64,
    pub line_width_ok: bool,
    pub pass: bool,
    pub files: Vec<String>,
}

/// Perturbação de 5% da referência (decisão 23): brilho e saturação ×`k` (saturação como no PIL: mistura com a luma
/// ITU-R 601) e deslocamento circular de (`dx`, `dy`) px. Valores sRGB.
pub fn perturb(img: &aiai_paint::Image, k: f32, dx: i32, dy: i32) -> aiai_paint::Image {
    let (w, h) = (img.width as i32, img.height as i32);
    let mut out = aiai_paint::Image::new(img.width, img.height);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = ((x - dx).rem_euclid(w), (y - dy).rem_euclid(h));
            let p = img.get(sx as u32, sy as u32);
            let a = if p[3] > 0.0 { p[3] } else { 1.0 };
            let c = [p[0] / a, p[1] / a, p[2] / a];
            let g = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
            let q = |v: f32| ((g + (v - g) * k) * k).clamp(0.0, 1.0);
            let i = out.idx(x as u32, y as u32);
            out.data[i] = [q(c[0]), q(c[1]), q(c[2]), 1.0];
        }
    }
    out
}

/// Deslocamento suave determinístico (decisão 24): campo aleatório filtrado por gaussiana (σ = 40 px) com desvio de
/// `amp` px em cada eixo — o tremor de um decalque cuidadoso na mesa de luz.
pub fn smooth_warp(img: &aiai_paint::Image, amp: f32, seed: u64) -> aiai_paint::Image {
    let (w, h) = (img.width as usize, img.height as usize);
    // ruído numa tela com margem de 3σ e recorte do miolo: o desfoque replica as bordas e, sem margem, o campo fica
    // forte nas beiradas e a normalização enfraquece o miolo pela metade
    let m = 120usize;
    let (pw, ph) = (w + 2 * m, h + 2 * m);
    let field = |s: u64| {
        let mut x = s;
        let noise: Vec<f32> = (0..pw * ph)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 11) as f32 / (1u64 << 53) as f32 - 0.5
            })
            .collect();
        let big = aiai_metrics::gaussian_blur(&noise, pw, ph, 40.0);
        let mut f = Vec::with_capacity(w * h);
        for y in 0..h {
            f.extend_from_slice(&big[(y + m) * pw + m..(y + m) * pw + m + w]);
        }
        let n = f.len() as f32;
        let mean = f.iter().sum::<f32>() / n;
        let sd = (f.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n).sqrt().max(1e-9);
        f.into_iter().map(|v| (v - mean) / sd * amp).collect::<Vec<f32>>()
    };
    let (ux, uy) = (field(seed), field(seed ^ 0x9E37_79B9_7F4A_7C15));
    let mut out = aiai_paint::Image::new(img.width, img.height);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            // centro do pixel em +0,5 (convenção de sample_bilinear); sem isso o tremor ainda borra meio pixel
            let (sx, sy) = ((x as f32 + 0.5 + ux[i]).clamp(0.5, w as f32 - 0.5), (y as f32 + 0.5 + uy[i]).clamp(0.5, h as f32 - 0.5));
            out.data[i] = aiai_paint::warp::sample_bilinear(img, sx, sy);
        }
    }
    out
}

/// Limites da decisão 24 para esta referência: a própria referência sob P5T = (brilho e saturação ×1,05 ou ×0,95,
/// 1 px de deslocamento e tremor suave de 1 px), nas duas direções; vale o mais exigente de cada métrica.
pub struct P5tLimits {
    pub flip_mean_max: f64,
    pub flip_hard_max: f64,
    pub ssim_min: f64,
    pub edge_f1_min: f64,
}

pub fn p5t_limits(reference: &aiai_paint::Image) -> Result<P5tLimits, String> {
    let opts = CompareOptions::default();
    // média de seis sorteios do tremor por direção (um campo só faz o F1 variar ±0,02)
    const SEEDS: [u64; 6] = [0x5EED_0001, 0x5EED_0002, 0x5EED_0003, 0x5EED_0004, 0x5EED_0005, 0x5EED_0006];
    let run = |k: f32| -> Result<(f64, f64, f64), String> {
        let n = SEEDS.len() as f64;
        let mut acc = (0.0, 0.0, 0.0);
        for seed in SEEDS {
            let r = aiai_metrics::compare(reference, &smooth_warp(&perturb(reference, k, 1, 0), 1.0, seed), &opts)?;
            acc = (acc.0 + r.flip.mean / n, acc.1 + r.ssim / n, acc.2 + r.edges.f1 / n);
        }
        Ok(acc)
    };
    let (up, down) = (run(1.05)?, run(0.95)?);
    Ok(P5tLimits { flip_mean_max: up.0.min(down.0), flip_hard_max: up.0.max(down.0), ssim_min: up.1.max(down.1), edge_f1_min: up.2.max(down.2) })
}

/// [`p5t_limits`] guardado ao lado da referência (`<quadro>.p5t.json`, chave = tamanho + data do arquivo): 12 comparações
/// por quadro de referência só na primeira vez.
pub fn cached_p5t_limits(path: &str, reference: &aiai_paint::Image) -> Result<P5tLimits, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let key = format!("{}-{:?}-v1", meta.len(), meta.modified().ok());
    let cache = format!("{path}.p5t.json");
    if let Ok(txt) = std::fs::read_to_string(&cache) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            if v.get("key").and_then(|k| k.as_str()) == Some(key.as_str()) {
                let g = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(f64::NAN);
                let lim = P5tLimits { flip_mean_max: g("flip_mean_max"), flip_hard_max: g("flip_hard_max"), ssim_min: g("ssim_min"), edge_f1_min: g("edge_f1_min") };
                if [lim.flip_mean_max, lim.flip_hard_max, lim.ssim_min, lim.edge_f1_min].iter().all(|x| x.is_finite()) {
                    return Ok(lim);
                }
            }
        }
    }
    let lim = p5t_limits(reference)?;
    let v = serde_json::json!({"key": key, "flip_mean_max": lim.flip_mean_max, "flip_hard_max": lim.flip_hard_max,
                               "ssim_min": lim.ssim_min, "edge_f1_min": lim.edge_f1_min});
    let _ = std::fs::write(&cache, serde_json::to_string_pretty(&v).unwrap_or_default());
    Ok(lim)
}

/// Limites de FLIP da decisão 23 para esta referência: (médio, máximo) = (FLIP sob P5, FLIP sob P5').
pub fn p5_flip_limits(reference: &aiai_paint::Image) -> Result<(f64, f64), String> {
    let up = aiai_metrics::flip(reference, &perturb(reference, 1.05, 1, 0))?.mean;
    let down = aiai_metrics::flip(reference, &perturb(reference, 0.95, 1, 0))?.mean;
    Ok((up.min(down), up.max(down)))
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

/// Régua perceptual (decisão 25): tolerância de bordas, F1 mínimo e teto do SSIM exigido. Vale para o quadro inteiro
/// redesenhado contra a referência (4 px ≈ 0,4% da altura, invisível em movimento). A medida fina de linha é a linha
/// central subpixel (Steger) que vai junto no relatório (`report.centerline`: desvio da linha central e R² de w(s)), e é
/// ela que aprova as canetas na bancada de calibração (`tools/csp/calib_compare.py`, auditoria 4.9h).
pub const EDGE_TOLERANCE_PX: f32 = 4.0;
pub const EDGE_F1_MIN_PERCEPTUAL: f64 = 0.90;
pub const SSIM_MIN_PERCEPTUAL: f64 = 0.93;

pub fn compare_frame(p: &Project, blobs: &BlobStore, req: &CompareRequest) -> ToolResult<FrameVerdict> {
    let reference = load_reference(&req.reference)?;
    let out = render_frame(p, blobs, &req.cut, req.frame, &RenderOptions { background: Some(req.background), ..Default::default() }).map_err(|e| ToolError::Render(e.to_string()))?;
    if (out.image.width, out.image.height) != (reference.width, reference.height) {
        return Err(ToolError::Invalid(format!("render {}x{} ≠ referência {}x{}", out.image.width, out.image.height, reference.width, reference.height)));
    }
    let mut opts = CompareOptions::default();
    let lim = cached_p5t_limits(&req.reference, &reference).map_err(ToolError::Invalid)?;
    let (flip_limit, flip_hard_limit) = (lim.flip_mean_max, lim.flip_hard_max);
    opts.thresholds.flip_mean_max = flip_limit;
    opts.thresholds.flip_mean_hard_max = flip_hard_limit;
    // decisão 25 (régua perceptual): bordas com tolerância de 4 px (≈0,4 % da altura, invisível em movimento) e F1 ≥ 0,90;
    // SSIM ≥ min(limite P5T, 0,93); FLIP continua no limite P5T da própria referência
    opts.edge_tolerance_px = EDGE_TOLERANCE_PX;
    let ssim_min = lim.ssim_min.min(SSIM_MIN_PERCEPTUAL);
    opts.thresholds.ssim_min = ssim_min;
    opts.thresholds.edge_f1_min = EDGE_F1_MIN_PERCEPTUAL;
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
        // valores crus do FLIP (cinza 0..1) para medir onde está o erro por região
        let mut raw = aiai_paint::Image::new(art.flip.width, art.flip.height);
        for (px, &e) in raw.data.iter_mut().zip(&art.flip.error_map) {
            *px = [e, e, e, 1.0];
        }
        save(&raw, "flip_bruto", &mut files)?;
        save(&edge_overlay(&reference, &art.ref_edges, &art.test_edges, opts.edge_tolerance_px), "bordas", &mut files)?;
        let rep_path = d.join(format!("{stem}_relatorio.json"));
        std::fs::write(&rep_path, serde_json::to_string_pretty(&report).unwrap_or_default()).map_err(|e| ToolError::Invalid(e.to_string()))?;
        files.push(rep_path.to_string_lossy().to_string());
    }
    Ok(FrameVerdict { flip_limit, flip_hard_limit, ssim_limit: ssim_min, edge_f1_limit: EDGE_F1_MIN_PERCEPTUAL, noise_floor: req.noise_floor,
                      line_width_ok: lw_ok, pass, report, files })
}
