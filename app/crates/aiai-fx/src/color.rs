//! Correção primária (Grade) sobre cor NÃO pré-multiplicada, re-pré-multiplicada na saída.
//!
//! Ordem das operações (por pixel, em sRGB codificado 0..1):
//! 1. balanço de branco: ganhos R/B por `temperature` (±0,3 por unidade) e G por `tint` (positivo = magenta);
//! 2. ASC-CDL-like por canal: `out = (gain · (in + lift · (1 − in)))^(1/gamma)` (base limitada em 0);
//! 3. contraste em torno do pivô: `(v − pivot) · contrast + pivot`;
//! 4. saturação com luma Rec.709;
//! 5. limite em 0..1 (`apply_rgb`; a versão `apply_rgb_raw` não limita).

use crate::util::*;
use aiai_paint::Image;
use rayon::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradeParams {
    pub lift: [f32; 3],
    pub gamma: [f32; 3],
    pub gain: [f32; 3],
    pub saturation: f32,
    pub temperature: f32,
    pub tint: f32,
    pub contrast: f32,
    pub pivot: f32,
}

impl Default for GradeParams {
    fn default() -> Self {
        GradeParams { lift: [0.0; 3], gamma: [1.0; 3], gain: [1.0; 3], saturation: 1.0, temperature: 0.0, tint: 0.0, contrast: 1.0, pivot: 0.5 }
    }
}

impl GradeParams {
    pub fn to_vec(&self) -> Vec<f32> {
        let mut v = Vec::with_capacity(14);
        v.extend_from_slice(&self.lift);
        v.extend_from_slice(&self.gamma);
        v.extend_from_slice(&self.gain);
        v.extend_from_slice(&[self.saturation, self.temperature, self.tint, self.contrast, self.pivot]);
        v
    }

    pub fn is_identity(&self) -> bool {
        *self == GradeParams::default()
    }

    /// Cor reta (0..1) → cor reta graduada, SEM o limite final em 0..1 (usado ao cozinhar LUTs: sem quinas de
    /// clamp a interpolação tetraédrica reproduz a curva; `Lut::apply_image` limita depois de interpolar).
    pub fn apply_rgb_raw(&self, c: [f32; 3]) -> [f32; 3] {
        let wb = [1.0 + 0.3 * self.temperature, 1.0 - 0.2 * self.tint, 1.0 - 0.3 * self.temperature];
        let mut v = [0.0f32; 3];
        for k in 0..3 {
            let x = c[k] * wb[k];
            let base = (self.gain[k] * (x + self.lift[k] * (1.0 - x))).max(0.0);
            let g = self.gamma[k].max(1e-3);
            let y = if (g - 1.0).abs() < 1e-6 { base } else { base.powf(1.0 / g) };
            v[k] = (y - self.pivot) * self.contrast + self.pivot;
        }
        let l = luma(v);
        for k in 0..3 {
            v[k] = l + (v[k] - l) * self.saturation;
        }
        v
    }

    /// Cor reta (0..1) → cor reta graduada e limitada a 0..1.
    pub fn apply_rgb(&self, c: [f32; 3]) -> [f32; 3] {
        let v = self.apply_rgb_raw(c);
        [v[0].max(0.0).min(1.0), v[1].max(0.0).min(1.0), v[2].max(0.0).min(1.0)]
    }

    pub fn apply_image(&self, img: &mut Image) {
        if self.is_identity() {
            return;
        }
        img.data.par_iter_mut().for_each(|p| {
            if p[3] <= 0.0 {
                return;
            }
            let un = unpremul(*p);
            let o = self.apply_rgb(un);
            p[0] = o[0] * p[3];
            p[1] = o[1] * p[3];
            p[2] = o[2] * p[3];
            fix(p);
        });
    }
}
