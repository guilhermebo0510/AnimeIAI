//! FLIP (NVIDIA, LDR) via `nv-flip` (ligacao para a implementacao C++ oficial).
//!
//! Pixels por grau: o FLIP depende do angulo visual de um pixel. O padrao do projeto e 67 PPD
//! (valor padrao do FLIP: monitor 4K a 0,7 m). O protocolo fixa 67 PPD para 1920x1080; em um monitor
//! Full HD de ~24" a 0,7 m o valor fisico seria ~46 PPD (erros ficam mais visiveis, o FLIP sobe).
//! Usamos 67 porque e o numero do protocolo; `ppd` e configuravel em `CompareOptions`.

use crate::color::det_sum_f32;
use crate::rgb::{same_size, AsRgb8};
use rayon::prelude::*;

pub const DEFAULT_PPD: f32 = 67.0;

#[derive(Clone, Debug)]
pub struct FlipResult {
    pub mean: f64,
    pub p50: f32,
    pub p95: f32,
    pub max: f32,
    pub width: u32,
    pub height: u32,
    /// Erro por pixel em [0,1], linha a linha.
    pub error_map: Vec<f32>,
}

impl FlipResult {
    /// Janelas `size`x`size` de maior erro medio (ver [`crate::worst_regions`]).
    pub fn worst_regions(&self, n: usize, size: u32) -> Vec<[u32; 4]> {
        crate::report::worst_regions(&self.error_map, self.width, self.height, n, size)
    }
}

/// Resume um mapa de erro em media, percentis e maximo.
pub fn summarize_error_map(error_map: Vec<f32>, width: u32, height: u32) -> FlipResult {
    let mean = det_sum_f32(&error_map) / error_map.len().max(1) as f64;
    let mut sorted = error_map.clone();
    sorted.par_sort_unstable_by(|a, b| a.total_cmp(b));
    let pick = |q: f64| -> f32 {
        if sorted.is_empty() { 0.0 } else { sorted[((sorted.len() - 1) as f64 * q).round() as usize] }
    };
    FlipResult { mean, p50: pick(0.5), p95: pick(0.95), max: sorted.last().copied().unwrap_or(0.0), width, height, error_map }
}

pub fn flip(reference: &impl AsRgb8, test: &impl AsRgb8) -> Result<FlipResult, String> {
    flip_with_ppd(reference, test, DEFAULT_PPD)
}

pub fn flip_with_ppd(reference: &impl AsRgb8, test: &impl AsRgb8, ppd: f32) -> Result<FlipResult, String> {
    let (r, t) = (reference.as_rgb8(), test.as_rgb8());
    same_size(&r, &t)?;
    let a = nv_flip::FlipImageRgb8::with_data(r.width, r.height, &r.data);
    let b = nv_flip::FlipImageRgb8::with_data(t.width, t.height, &t.data);
    let map = nv_flip::flip(a, b, ppd);
    Ok(summarize_error_map(map.to_vec(), r.width, r.height))
}
