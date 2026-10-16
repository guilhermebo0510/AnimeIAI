//! aiai-metrics: metricas objetivas do protocolo de validacao (`sprints/07-PROTOCOLO-DE-VALIDACAO.md`).
//!
//! Tudo trabalha sobre quadros RGB8 sRGB codificados (`Rgb8`, ou `aiai_paint::Image` achatada sobre branco)
//! e e deterministico: somas em blocos de ordem fixa, k-means com semente fixa.
//!
//! | Funcao | Metrica | Limiar do protocolo |
//! |---|---|---|
//! | [`flip`] | FLIP (NVIDIA, LDR), media | <= 0,05 |
//! | [`ssim`] | SSIM luma, gaussiana 11x11 | >= 0,95 |
//! | [`mean_delta_e`] + [`flat_region_mask`] | ΔE2000 nas regioes chapadas | <= 5,0 |
//! | [`edge_map`] + [`edge_f1`] | F1 de bordas a 2 px | >= 0,95 |
//! | [`line_width_stats`] | mediana da espessura | +-5% |
//! | [`palette`] / [`match_palette`] | cores do settei | ΔE <= 3 |
//! | [`phase_correlation`] | erro de camera | <= 0,5% da largura |

mod color;
mod edges;
mod filt;
mod flip;
mod lines;
mod palette;
mod phase;
mod report;
mod rgb;
mod ssim;

pub use color::{delta_e2000, delta_e_map, image_to_lab, lab_to_srgb, mean_delta_e, rgb8_to_lab, srgb_to_lab, Lab};
pub use edges::{edge_f1, edge_map, edge_map_with, flat_region_mask, flat_region_mask_with, EdgeParams, EdgeScore};
pub use filt::{distance_transform, gaussian_blur};
pub use flip::{flip, flip_with_ppd, summarize_error_map, FlipResult, DEFAULT_PPD};
pub use lines::{dark_line_mask, line_width_samples, line_width_stats, line_width_stats_from_mask, skeletonize, LineWidthStats};
pub use palette::{match_palette, palette, palette_shift, PaletteMatch, Rgb};
pub use phase::phase_correlation;
pub use report::{
    compare, compare_with_artifacts, diff_visualization, edge_overlay, worst_regions, CameraShift, CompareArtifacts, CompareOptions, CompareReport,
    DeltaESummary, FlipSummary, LineWidthCompare, Thresholds, Verdict,
};
pub use rgb::{AsRgb8, Rgb8};
pub use ssim::{ssim, ssim_full, SsimResult};
