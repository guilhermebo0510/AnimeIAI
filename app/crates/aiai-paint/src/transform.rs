//! Reamostragem afim (bilinear, pré-multiplicada) — imagens importadas, cels transformados, deslocamentos fracionários.

use crate::image::Image;
use rayon::prelude::*;

/// Matriz afim 2×3: [a, b, c, d, e, f] mapeia (x, y) → (a x + b y + c, d x + e y + f).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine(pub [f32; 6]);

impl Affine {
    pub const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);

    /// Escala e rotação (graus) em torno de `pivot`, depois translação.
    pub fn from_trs(tx: f32, ty: f32, scale: f32, rot_deg: f32, pivot: [f32; 2]) -> Affine {
        let (s, c) = rot_deg.to_radians().sin_cos();
        let (a, b, d, e) = (c * scale, -s * scale, s * scale, c * scale);
        let cx = pivot[0] - (a * pivot[0] + b * pivot[1]) + tx;
        let cy = pivot[1] - (d * pivot[0] + e * pivot[1]) + ty;
        Affine([a, b, cx, d, e, cy])
    }

    pub fn is_identity(&self) -> bool {
        self.0.iter().zip(Affine::IDENTITY.0).all(|(a, b)| (a - b).abs() < 1e-6)
    }

    pub fn is_integer_translation(&self) -> Option<(i32, i32)> {
        let m = self.0;
        let lin = (m[0] - 1.0).abs() < 1e-6 && m[1].abs() < 1e-6 && m[3].abs() < 1e-6 && (m[4] - 1.0).abs() < 1e-6;
        let ti = (m[2] - m[2].round()).abs() < 1e-4 && (m[5] - m[5].round()).abs() < 1e-4;
        (lin && ti).then(|| (m[2].round() as i32, m[5].round() as i32))
    }

    pub fn inverse(&self) -> Option<Affine> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * e - b * d;
        if det.abs() < 1e-12 {
            return None;
        }
        let ia = e / det;
        let ib = -b / det;
        let id = -d / det;
        let ie = a / det;
        Some(Affine([ia, ib, -(ia * c + ib * f), id, ie, -(id * c + ie * f)]))
    }

    #[inline]
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let m = self.0;
        (m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5])
    }
}

#[inline]
fn sample_bilinear(src: &Image, x: f32, y: f32) -> [f32; 4] {
    // centro do pixel em +0,5
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = fx.floor();
    let y0 = fy.floor();
    let tx = fx - x0;
    let ty = fy - y0;
    let get = |xi: i32, yi: i32| -> [f32; 4] {
        if xi < 0 || yi < 0 || xi >= src.width as i32 || yi >= src.height as i32 {
            [0.0; 4]
        } else {
            src.data[yi as usize * src.width as usize + xi as usize]
        }
    };
    let (xi, yi) = (x0 as i32, y0 as i32);
    let (p00, p10, p01, p11) = (get(xi, yi), get(xi + 1, yi), get(xi, yi + 1), get(xi + 1, yi + 1));
    let mut out = [0.0; 4];
    for c in 0..4 {
        let top = p00[c] + (p10[c] - p00[c]) * tx;
        let bot = p01[c] + (p11[c] - p01[c]) * tx;
        out[c] = top + (bot - top) * ty;
    }
    out
}

/// Desenha `src` transformada por `m` (coordenadas de `src` → destino) numa imagem nova `w × h`.
pub fn transform_image(src: &Image, m: Affine, w: u32, h: u32) -> Image {
    if let Some((dx, dy)) = m.is_integer_translation() {
        let mut out = Image::new(w, h);
        out.draw_over(src, dx, dy, 1.0);
        return out;
    }
    let inv = match m.inverse() {
        Some(i) => i,
        None => return Image::new(w, h),
    };
    let mut out = Image::new(w, h);
    out.data.par_chunks_mut(w as usize).enumerate().for_each(|(y, row)| {
        for (x, px) in row.iter_mut().enumerate() {
            let (sx, sy) = inv.apply(x as f32 + 0.5, y as f32 + 0.5);
            *px = sample_bilinear(src, sx, sy);
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_roundtrip_and_integer_shortcut() {
        let m = Affine::from_trs(10.0, -3.0, 1.5, 30.0, [50.0, 20.0]);
        let inv = m.inverse().unwrap();
        let (x, y) = m.apply(7.0, 9.0);
        let (bx, by) = inv.apply(x, y);
        assert!((bx - 7.0).abs() < 1e-3 && (by - 9.0).abs() < 1e-3);
        assert_eq!(Affine::from_trs(3.0, 4.0, 1.0, 0.0, [0.0, 0.0]).is_integer_translation(), Some((3, 4)));
    }
}
