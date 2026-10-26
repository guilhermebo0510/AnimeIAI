//! Degradê livre (S25, como o "Degradê de forma livre" do Clip Studio): pontos de cor soltos no canvas e um campo de cor
//! contínuo entre eles. Cada pixel mistura as cores dos pontos com peso pela distância inversa (Shepard) amaciada:
//! `w = 1 / (d² + ε²)^(p/2)`, p = 3 — perto de um ponto vale a cor dele, entre dois pontos a passagem é suave e
//! longe de todos a cor tende à média ponderada dos mais próximos, sem as "bolhas" chapadas do p = 2. A mistura é no
//! OKLab pré-multiplicado pelo alfa (passagens perceptualmente uniformes, sem a faixa escura do sRGB; ponto
//! transparente não escurece os vizinhos), a mesma escolha do degradê `oklab` do programa.
//!
//! Um ponto só pinta a cor dele no canvas inteiro.

use aiai_core::paint::{oklab_to_srgb, srgb_to_oklab};
use aiai_core::{BlendMode, ColorRef};
use aiai_paint::Image;
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Ponto de cor do degradê livre (px do canvas).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FreeformPoint {
    pub at: [f32; 2],
    pub color: ColorRef,
}

/// Expoente da distância inversa.
pub const POWER: f32 = 3.0;
/// Raio de amaciamento como fração da diagonal do canvas (tira o bico no próprio ponto).
pub const SOFT: f32 = 0.02;

/// Campo de cor pronto para amostrar.
#[derive(Debug, Clone)]
pub struct FreeField {
    /// (posição, OKLab × alfa, alfa)
    pts: Vec<([f32; 2], [f32; 3], f32)>,
    eps2: f32,
}

impl FreeField {
    /// `points`: (posição, cor sRGB não pré-multiplicada); `diag`: diagonal do canvas (px).
    pub fn new(points: &[([f32; 2], [f32; 4])], diag: f32) -> FreeField {
        let pts = points
            .iter()
            .map(|(p, c)| {
                let a = c[3].clamp(0.0, 1.0);
                let lab = srgb_to_oklab([c[0], c[1], c[2]]);
                (*p, [lab[0] * a, lab[1] * a, lab[2] * a], a)
            })
            .collect();
        let e = (diag * SOFT).max(1.0);
        FreeField { pts, eps2: e * e }
    }

    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }

    /// Cor sRGB não pré-multiplicada em `p`.
    pub fn sample(&self, p: [f32; 2]) -> [f32; 4] {
        if self.pts.is_empty() {
            return [0.0; 4];
        }
        let (mut acc, mut aa, mut wsum) = ([0.0f32; 3], 0.0f32, 0.0f32);
        for (q, lab, a) in &self.pts {
            let d2 = (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + self.eps2;
            let w = 1.0 / (d2 * d2.sqrt());
            for k in 0..3 {
                acc[k] += lab[k] * w;
            }
            aa += a * w;
            wsum += w;
        }
        let alpha = aa / wsum;
        if alpha <= 1e-6 {
            return [0.0; 4];
        }
        let rgb = oklab_to_srgb([acc[0] / aa, acc[1] / aa, acc[2] / aa]);
        [rgb[0], rgb[1], rgb[2], alpha.clamp(0.0, 1.0)]
    }
}

/// Pinta o campo na imagem inteira (`blend`, `opacity`).
pub fn paint(img: &mut Image, field: &FreeField, blend: BlendMode, opacity: f32) {
    if field.is_empty() {
        return;
    }
    let w = img.width as usize;
    img.data.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, px) in row.iter_mut().enumerate() {
            let c = field.sample([x as f32 + 0.5, y as f32 + 0.5]);
            aiai_paint::blend::blend_pixel(px, [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]], blend, opacity);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 4], b: [f32; 4], tol: f32) -> bool {
        (0..4).all(|k| (a[k] - b[k]).abs() <= tol)
    }

    #[test]
    fn a_point_holds_its_color_and_one_point_fills_everything() {
        let red = [1.0, 0.0, 0.0, 1.0];
        let blue = [0.0, 0.0, 1.0, 1.0];
        let f = FreeField::new(&[([100.0, 100.0], red), ([900.0, 100.0], blue)], 1000.0);
        assert!(close(f.sample([100.0, 100.0]), red, 0.03), "{:?}", f.sample([100.0, 100.0]));
        assert!(close(f.sample([900.0, 100.0]), blue, 0.03));
        // no meio: mistura dos dois, sem escurecer (OKLab): luminância perto das pontas
        let mid = f.sample([500.0, 100.0]);
        assert!(mid[0] > 0.3 && mid[2] > 0.3, "{mid:?}");
        let one = FreeField::new(&[([10.0, 10.0], [0.2, 0.6, 0.4, 1.0])], 500.0);
        assert!(close(one.sample([400.0, 300.0]), [0.2, 0.6, 0.4, 1.0], 2e-3));
    }

    #[test]
    fn the_field_is_smooth_and_monotonic_between_two_points() {
        let f = FreeField::new(&[([0.0, 0.0], [0.0, 0.0, 0.0, 1.0]), ([1000.0, 0.0], [1.0, 1.0, 1.0, 1.0])], 1000.0);
        let v: Vec<f32> = (0..=100).map(|i| f.sample([i as f32 * 10.0, 0.0])[0]).collect();
        assert!(v.windows(2).all(|w| w[1] >= w[0] - 1e-4), "a passagem volta: {v:?}");
        // sem degraus: nenhum passo de 10 px pula mais de 6%
        assert!(v.windows(2).all(|w| (w[1] - w[0]).abs() < 0.06));
    }

    #[test]
    fn transparent_points_fade_alpha_without_darkening() {
        let f = FreeField::new(&[([0.0, 0.0], [1.0, 0.0, 0.0, 1.0]), ([100.0, 0.0], [0.0, 0.0, 0.0, 0.0])], 100.0);
        let m = f.sample([50.0, 0.0]);
        assert!(m[3] > 0.3 && m[3] < 0.7 && m[0] > 0.95, "vermelho que some, não escurece: {m:?}");
    }

    #[test]
    fn paint_covers_the_image() {
        let mut img = Image::new(40, 20);
        let f = FreeField::new(&[([0.0, 0.0], [1.0, 0.0, 0.0, 1.0]), ([40.0, 20.0], [0.0, 1.0, 0.0, 1.0])], 45.0);
        paint(&mut img, &f, BlendMode::Normal, 1.0);
        assert!(img.data.iter().all(|p| (p[3] - 1.0).abs() < 1e-4));
        assert!(img.get(0, 0)[0] > 0.9 && img.get(39, 19)[1] > 0.9);
    }
}
