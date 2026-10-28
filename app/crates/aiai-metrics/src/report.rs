//! `compare`: todas as metricas + veredito contra a regua do protocolo, e visualizacoes.

use crate::color::{delta_e_map, mean_masked};
use crate::edges::{edge_f1, edge_map_with, flat_region_mask, EdgeParams, EdgeScore};
use crate::filt::distance_transform;
use crate::flip::{flip_with_ppd, FlipResult, DEFAULT_PPD};
use crate::lines::{line_width_stats, LineWidthStats};
use crate::palette::{palette_shift, PaletteMatch};
use crate::phase::phase_correlation;
use crate::rgb::{same_size, AsRgb8, Rgb8};
use crate::ssim::ssim_full;
use crate::steger::{centerline_compare, CenterlineCompare, StegerParams, WidthSummary};
use aiai_paint::Image;
use serde::{Deserialize, Serialize};

/// Limiares do protocolo de validacao (`sprints/07-PROTOCOLO-DE-VALIDACAO.md`, secao 5).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    pub flip_mean_max: f64,
    pub ssim_min: f64,
    pub delta_e_flat_max: f64,
    pub edge_f1_min: f64,
    pub line_width_median_tolerance: f64,
    /// Cada cor do settei deve casar com a da referencia ate este ΔE.
    pub palette_delta_e_max: f64,
    /// Nenhum quadro pode ter FLIP medio acima disto.
    pub flip_mean_hard_max: f64,
    /// Erro de camera maximo, fracao da largura do quadro.
    pub camera_error_max_frac: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            flip_mean_max: 0.05,
            ssim_min: 0.95,
            delta_e_flat_max: 5.0,
            edge_f1_min: 0.95,
            line_width_median_tolerance: 0.05,
            palette_delta_e_max: 3.0,
            flip_mean_hard_max: 0.10,
            camera_error_max_frac: 0.005,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompareOptions {
    pub ppd: f32,
    pub edge_tolerance_px: f32,
    pub palette_k: usize,
    pub worst_n: usize,
    pub worst_size: u32,
    pub thresholds: Thresholds,
}

impl Default for CompareOptions {
    fn default() -> Self {
        CompareOptions { ppd: DEFAULT_PPD, edge_tolerance_px: 2.0, palette_k: 6, worst_n: 4, worst_size: 128, thresholds: Thresholds::default() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlipSummary {
    pub mean: f64,
    pub p50: f32,
    pub p95: f32,
    pub max: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeltaESummary {
    /// ΔE2000 medio nas regioes chapadas (mascara calculada na referencia).
    pub flat_mean: f64,
    /// Fracao da imagem que a mascara chapada cobre.
    pub flat_fraction: f64,
    pub overall_mean: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineWidthCompare {
    pub reference: LineWidthStats,
    pub test: LineWidthStats,
    /// (mediana_teste - mediana_ref) / mediana_ref: a medida do protocolo (+-5%)
    pub median_error: f64,
    /// (media_teste - media_ref) / media_ref: complementar, nao discreta (ver `LineWidthStats::mean`)
    pub mean_error: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraShift {
    pub dx: f64,
    pub dy: f64,
    pub peak: f64,
    /// |dx| / largura do quadro
    pub error_frac_of_width: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    pub flip_ok: bool,
    pub ssim_ok: bool,
    pub delta_e_flat_ok: bool,
    pub edge_f1_ok: bool,
    pub line_width_ok: bool,
    pub palette_ok: bool,
    /// FLIP medio abaixo do teto duro do corte (0,10).
    pub flip_hard_ok: bool,
    /// Passa no quadro: as cinco metricas por quadro do protocolo (FLIP, SSIM, ΔE chapado, F1, espessura)
    /// e a paleta (cada cor do settei ate ΔE 3).
    pub pass: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompareReport {
    pub width: u32,
    pub height: u32,
    pub flip: FlipSummary,
    pub ssim: f64,
    pub delta_e: DeltaESummary,
    pub edges: EdgeScore,
    pub line_width: LineWidthCompare,
    pub palette: Vec<PaletteMatch>,
    pub camera: CameraShift,
    pub thresholds: Thresholds,
    pub verdict: Verdict,
    pub ppd: f32,
    pub edge_tolerance_px: f32,
    pub worst_regions: Vec<[u32; 4]>,
    /// Linha central subpixel (Steger) e largura pareada ponto a ponto (auditoria 4.9h): desvio da linha central e R²
    /// do perfil de espessura w(s). Informativo no veredito do protocolo; é o critério da bancada das canetas.
    #[serde(default)]
    pub centerline: CenterlineCompare,
    /// Largura pela linha central (fora de junções e pontas), referência e teste.
    #[serde(default)]
    pub centerline_width: [WidthSummary; 2],
}

/// Mapas intermediarios para gerar as imagens do relatorio.
pub struct CompareArtifacts {
    pub flip: FlipResult,
    pub ref_edges: Vec<bool>,
    pub test_edges: Vec<bool>,
    pub flat_mask: Vec<bool>,
}

pub fn compare(reference: &impl AsRgb8, test: &impl AsRgb8, opts: &CompareOptions) -> Result<CompareReport, String> {
    Ok(compare_with_artifacts(reference, test, opts)?.0)
}

pub fn compare_with_artifacts(reference: &impl AsRgb8, test: &impl AsRgb8, opts: &CompareOptions) -> Result<(CompareReport, CompareArtifacts), String> {
    let (r, t) = (reference.as_rgb8(), test.as_rgb8());
    same_size(&r, &t)?;
    let (w, h) = (r.width, r.height);
    let th = opts.thresholds;

    let fl = flip_with_ppd(&*r, &*t, opts.ppd)?;
    let ss = ssim_full(&*r, &*t)?;

    let flat = flat_region_mask(&*r);
    let de_map = delta_e_map(&*r, &*t)?;
    let flat_n = flat.iter().filter(|b| **b).count();
    let de = DeltaESummary { flat_mean: mean_masked(&de_map, Some(&flat)), flat_fraction: flat_n as f64 / flat.len().max(1) as f64, overall_mean: mean_masked(&de_map, None) };

    let params = EdgeParams::auto(&*r);
    let ref_edges = edge_map_with(&*r, &params);
    let test_edges = edge_map_with(&*t, &params);
    let es = edge_f1(&ref_edges, &test_edges, w, h, opts.edge_tolerance_px);

    let lw_r = line_width_stats(&*r);
    let lw_t = line_width_stats(&*t);
    let lw_err = if lw_r.median > 0.0 { (lw_t.median as f64 - lw_r.median as f64) / lw_r.median as f64 } else if lw_t.median > 0.0 { 1.0 } else { 0.0 };

    let mean_err = if lw_r.mean > 0.0 { (lw_t.mean as f64 - lw_r.mean as f64) / lw_r.mean as f64 } else { 0.0 };
    let mask = if flat_n > 0 { Some(flat.as_slice()) } else { None };
    let pal = palette_shift(&*r, &*t, opts.palette_k, mask)?;

    let (dx, dy, peak) = phase_correlation(&*r, &*t)?;
    let cam = CameraShift { dx, dy, peak, error_frac_of_width: dx.hypot(dy) / w as f64 };

    let flip_ok = fl.mean <= th.flip_mean_max;
    let ssim_ok = ss.mean >= th.ssim_min;
    let de_ok = de.flat_mean <= th.delta_e_flat_max;
    let f1_ok = es.f1 >= th.edge_f1_min;
    let lw_ok = lw_err.abs() <= th.line_width_median_tolerance;
    let pal_ok = pal.iter().all(|m| m.delta_e <= th.palette_delta_e_max);
    let hard_ok = fl.mean <= th.flip_mean_hard_max;
    let verdict = Verdict { flip_ok, ssim_ok, delta_e_flat_ok: de_ok, edge_f1_ok: f1_ok, line_width_ok: lw_ok, palette_ok: pal_ok, flip_hard_ok: hard_ok, pass: flip_ok && ssim_ok && de_ok && f1_ok && lw_ok && pal_ok };

    let (cr, ct, cl) = centerline_compare(&*r, &*t, &StegerParams::default(), 3.0);
    let centerline_width = [cr.width_stats(), ct.width_stats()];

    let worst = fl.worst_regions(opts.worst_n, opts.worst_size);
    let report = CompareReport {
        width: w,
        height: h,
        flip: FlipSummary { mean: fl.mean, p50: fl.p50, p95: fl.p95, max: fl.max },
        ssim: ss.mean,
        delta_e: de,
        edges: es,
        line_width: LineWidthCompare { reference: lw_r, test: lw_t, median_error: lw_err, mean_error: mean_err },
        palette: pal,
        camera: cam,
        thresholds: th,
        verdict,
        ppd: opts.ppd,
        edge_tolerance_px: opts.edge_tolerance_px,
        worst_regions: worst,
        centerline: cl,
        centerline_width,
    };
    Ok((report, CompareArtifacts { flip: fl, ref_edges, test_edges, flat_mask: flat }))
}

/// Mapa de calor FLIP (LUT magma do FLIP oficial) como `Image` opaca.
pub fn diff_visualization(flip: &FlipResult) -> Image {
    let map = nv_flip::FlipImageFloat::with_data(flip.width, flip.height, &flip.error_map);
    let colored = map.apply_color_lut(&nv_flip::magma_lut());
    let rgb = Rgb8 { width: flip.width, height: flip.height, data: colored.to_vec() };
    rgb.to_image()
}

/// Sobreposicao de bordas sobre a referencia esmaecida: verde = so referencia, vermelho = so teste,
/// amarelo = ambos (a menos de `tolerance_px` um do outro).
pub fn edge_overlay(reference: &impl AsRgb8, ref_edges: &[bool], test_edges: &[bool], tolerance_px: f32) -> Image {
    let r = reference.as_rgb8();
    let (w, h) = (r.width as usize, r.height as usize);
    let dr = distance_transform(ref_edges, w, h);
    let dt = distance_transform(test_edges, w, h);
    let tol = tolerance_px + 1e-4;
    let mut data = Vec::with_capacity(w * h * 3);
    for i in 0..w * h {
        let p = r.px(i);
        // fundo: referencia clareada (25% de opacidade sobre branco)
        let mut c = [0u8; 3];
        for k in 0..3 {
            c[k] = (255.0 - (255.0 - p[k] as f32) * 0.25 + 0.5) as u8;
        }
        let (re, te) = (ref_edges[i], test_edges[i]);
        if re && te {
            c = [255, 220, 0];
        } else if re {
            c = if dt[i] <= tol { [255, 220, 0] } else { [0, 200, 0] };
        } else if te {
            c = if dr[i] <= tol { [255, 220, 0] } else { [230, 0, 0] };
        }
        data.extend_from_slice(&c);
    }
    Rgb8 { width: r.width, height: r.height, data }.to_image()
}

/// Ate `n` janelas `size`x`size` sem sobreposicao, de maior erro medio, como `[x, y, w, h]`.
/// Busca em passos de `size/2` com imagem integral; guloso do pior para o melhor.
pub fn worst_regions(error_map: &[f32], width: u32, height: u32, n: usize, size: u32) -> Vec<[u32; 4]> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || n == 0 || size == 0 {
        return vec![];
    }
    let (sw, sh) = ((size as usize).min(w), (size as usize).min(h));
    let mut integ = vec![0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0.0;
        for x in 0..w {
            row += error_map[y * w + x] as f64;
            integ[(y + 1) * (w + 1) + x + 1] = integ[y * (w + 1) + x + 1] + row;
        }
    }
    let sum = |x0: usize, y0: usize| -> f64 {
        let (x1, y1) = (x0 + sw, y0 + sh);
        integ[y1 * (w + 1) + x1] - integ[y0 * (w + 1) + x1] - integ[y1 * (w + 1) + x0] + integ[y0 * (w + 1) + x0]
    };
    let step = (size as usize / 2).max(1);
    let mut cands: Vec<(f64, usize, usize)> = Vec::new();
    let mut y = 0;
    loop {
        let yy = y.min(h - sh);
        let mut x = 0;
        loop {
            let xx = x.min(w - sw);
            cands.push((sum(xx, yy), xx, yy));
            if xx == w - sw {
                break;
            }
            x += step;
        }
        if yy == h - sh {
            break;
        }
        y += step;
    }
    cands.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.2.cmp(&b.2)).then(a.1.cmp(&b.1)));
    let mut out: Vec<[u32; 4]> = Vec::new();
    for (_, x, y) in cands {
        if out.len() >= n {
            break;
        }
        let overlaps = out.iter().any(|r| {
            let (rx, ry) = (r[0] as usize, r[1] as usize);
            x < rx + sw && rx < x + sw && y < ry + sh && ry < y + sh
        });
        if !overlaps {
            out.push([x as u32, y as u32, sw as u32, sh as u32]);
        }
    }
    out
}
