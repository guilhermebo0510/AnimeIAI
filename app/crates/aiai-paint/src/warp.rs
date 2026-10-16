//! Deformações geométricas para vetor (pontos) e raster (reamostragem): perspectiva (homografia de 4 pontos),
//! malha Bézier/bilinear (warp), liquify (empurrar, inchar, encolher, girar) e repetição ao longo de curva.

use crate::image::Image;
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- homografia

/// Matriz 3×3 de homografia (linha maior), mapeia (x, y) → (x', y').
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Homography(pub [f64; 9]);

impl Homography {
    /// Homografia que leva os 4 cantos `src` aos 4 cantos `dst` (ordem: sup-esq, sup-dir, inf-dir, inf-esq).
    pub fn from_quads(src: [[f32; 2]; 4], dst: [[f32; 2]; 4]) -> Option<Homography> {
        // resolve A h = b com h33 = 1 (8 incógnitas)
        let mut a = [[0.0f64; 9]; 8];
        for i in 0..4 {
            let (x, y) = (src[i][0] as f64, src[i][1] as f64);
            let (u, v) = (dst[i][0] as f64, dst[i][1] as f64);
            a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        // eliminação de Gauss com pivoteamento parcial
        for col in 0..8 {
            let piv = (col..8).max_by(|&i, &j| a[i][col].abs().partial_cmp(&a[j][col].abs()).unwrap_or(std::cmp::Ordering::Equal))?;
            if a[piv][col].abs() < 1e-12 {
                return None;
            }
            a.swap(col, piv);
            for r in 0..8 {
                if r != col {
                    let f = a[r][col] / a[col][col];
                    for c in col..9 {
                        a[r][c] -= f * a[col][c];
                    }
                }
            }
        }
        let mut h = [0.0f64; 9];
        for i in 0..8 {
            h[i] = a[i][8] / a[i][i];
        }
        h[8] = 1.0;
        Some(Homography(h))
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let m = &self.0;
        let (x, y) = (x as f64, y as f64);
        let w = m[6] * x + m[7] * y + m[8];
        (((m[0] * x + m[1] * y + m[2]) / w) as f32, ((m[3] * x + m[4] * y + m[5]) / w) as f32)
    }

    pub fn inverse(&self) -> Option<Homography> {
        let m = &self.0;
        let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6]) + m[2] * (m[3] * m[7] - m[4] * m[6]);
        if det.abs() < 1e-15 {
            return None;
        }
        let inv = [
            (m[4] * m[8] - m[5] * m[7]) / det,
            (m[2] * m[7] - m[1] * m[8]) / det,
            (m[1] * m[5] - m[2] * m[4]) / det,
            (m[5] * m[6] - m[3] * m[8]) / det,
            (m[0] * m[8] - m[2] * m[6]) / det,
            (m[2] * m[3] - m[0] * m[5]) / det,
            (m[3] * m[7] - m[4] * m[6]) / det,
            (m[1] * m[6] - m[0] * m[7]) / det,
            (m[0] * m[4] - m[1] * m[3]) / det,
        ];
        Some(Homography(inv))
    }
}

#[inline]
pub fn sample_bilinear(src: &Image, x: f32, y: f32) -> [f32; 4] {
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = fx.floor();
    let y0 = fy.floor();
    let (tx, ty) = (fx - x0, fy - y0);
    let get = |xi: i32, yi: i32| -> [f32; 4] {
        if xi < 0 || yi < 0 || xi >= src.width as i32 || yi >= src.height as i32 {
            [0.0; 4]
        } else {
            src.data[yi as usize * src.width as usize + xi as usize]
        }
    };
    let (xi, yi) = (x0 as i32, y0 as i32);
    let (a, b, c, d) = (get(xi, yi), get(xi + 1, yi), get(xi, yi + 1), get(xi + 1, yi + 1));
    let mut o = [0.0; 4];
    for k in 0..4 {
        let t = a[k] + (b[k] - a[k]) * tx;
        let u = c[k] + (d[k] - c[k]) * tx;
        o[k] = t + (u - t) * ty;
    }
    o
}

/// Reamostra a imagem inteira por uma função "destino → origem".
pub fn resample_inverse<F: Fn(f32, f32) -> Option<(f32, f32)> + Sync>(src: &Image, w: u32, h: u32, inv: F) -> Image {
    let mut out = Image::new(w, h);
    out.data.par_chunks_mut(w as usize).enumerate().for_each(|(y, row)| {
        for (x, px) in row.iter_mut().enumerate() {
            if let Some((sx, sy)) = inv(x as f32 + 0.5, y as f32 + 0.5) {
                *px = sample_bilinear(src, sx, sy);
            }
        }
    });
    out
}

pub fn perspective_image(src: &Image, h: &Homography) -> Image {
    let Some(inv) = h.inverse() else { return Image::new(src.width, src.height) };
    resample_inverse(src, src.width, src.height, |x, y| Some(inv.apply(x, y)))
}

// ---------------------------------------------------------------- malha

/// Malha de deformação: grade regular `cols × rows` de células sobre o retângulo `rect` [x, y, w, h]; `points` são os
/// (cols+1)×(rows+1) vértices deslocados (linha maior). Interpolação bilinear por célula.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MeshWarp {
    pub rect: [f32; 4],
    pub cols: u32,
    pub rows: u32,
    pub points: Vec<[f32; 2]>,
}

impl MeshWarp {
    pub fn identity(rect: [f32; 4], cols: u32, rows: u32) -> Self {
        let mut points = Vec::new();
        for j in 0..=rows {
            for i in 0..=cols {
                points.push([rect[0] + rect[2] * i as f32 / cols as f32, rect[1] + rect[3] * j as f32 / rows as f32]);
            }
        }
        MeshWarp { rect, cols, rows, points }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.cols == 0 || self.rows == 0 || self.cols > 64 || self.rows > 64 {
            return Err("malha precisa de 1..64 colunas e linhas".into());
        }
        if self.points.len() != ((self.cols + 1) * (self.rows + 1)) as usize {
            return Err(format!("malha {}x{} precisa de {} pontos", self.cols, self.rows, (self.cols + 1) * (self.rows + 1)));
        }
        if self.rect[2] <= 0.0 || self.rect[3] <= 0.0 {
            return Err("retângulo da malha vazio".into());
        }
        Ok(())
    }

    fn vtx(&self, i: u32, j: u32) -> [f32; 2] {
        self.points[(j * (self.cols + 1) + i) as usize]
    }

    /// Ponto (x, y) do espaço original → deformado. Fora do retângulo: inalterado.
    pub fn forward(&self, x: f32, y: f32) -> (f32, f32) {
        let u = (x - self.rect[0]) / self.rect[2] * self.cols as f32;
        let v = (y - self.rect[1]) / self.rect[3] * self.rows as f32;
        if u < 0.0 || v < 0.0 || u > self.cols as f32 || v > self.rows as f32 {
            return (x, y);
        }
        let i = (u.floor() as u32).min(self.cols - 1);
        let j = (v.floor() as u32).min(self.rows - 1);
        let (fu, fv) = (u - i as f32, v - j as f32);
        let (a, b, c, d) = (self.vtx(i, j), self.vtx(i + 1, j), self.vtx(i, j + 1), self.vtx(i + 1, j + 1));
        let top = [a[0] + (b[0] - a[0]) * fu, a[1] + (b[1] - a[1]) * fu];
        let bot = [c[0] + (d[0] - c[0]) * fu, c[1] + (d[1] - c[1]) * fu];
        (top[0] + (bot[0] - top[0]) * fv, top[1] + (bot[1] - top[1]) * fv)
    }

    /// Deforma uma imagem (para raster): inversão por busca local de Newton a partir da posição sem deformação.
    pub fn warp_image(&self, src: &Image) -> Image {
        let m = self.clone();
        resample_inverse(src, src.width, src.height, move |x, y| m.inverse_point(x, y))
    }

    /// Inverte `forward` por Newton (com jacobiano numérico); `None` se não convergir (fora da malha deformada).
    pub fn inverse_point(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        // chute: o ponto da malha original no mesmo lugar
        let (mut px, mut py) = (x, y);
        for _ in 0..12 {
            let (fx, fy) = self.forward(px, py);
            let (ex, ey) = (fx - x, fy - y);
            if ex.abs() < 0.01 && ey.abs() < 0.01 {
                return Some((px, py));
            }
            let e = 0.5;
            let (ax, ay) = self.forward(px + e, py);
            let (bx, by) = self.forward(px, py + e);
            let (j00, j10, j01, j11) = ((ax - fx) / e, (ay - fy) / e, (bx - fx) / e, (by - fy) / e);
            let det = j00 * j11 - j01 * j10;
            if det.abs() < 1e-6 {
                return None;
            }
            px -= (j11 * ex - j01 * ey) / det;
            py -= (-j10 * ex + j00 * ey) / det;
        }
        let (fx, fy) = self.forward(px, py);
        ((fx - x).abs() < 0.5 && (fy - y).abs() < 0.5).then_some((px, py))
    }
}

// ---------------------------------------------------------------- liquify

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LiquifyKind {
    /// Empurra na direção (dx, dy).
    Push,
    /// Afasta do centro.
    Bulge,
    /// Puxa para o centro.
    Pinch,
    /// Gira em torno do centro (graus).
    Twirl,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Liquify {
    pub kind: LiquifyKind,
    pub center: [f32; 2],
    pub radius: f32,
    /// Push: deslocamento em px; Bulge/Pinch: 0..1; Twirl: graus.
    pub strength: f32,
    #[serde(default)]
    pub direction: [f32; 2],
}

impl Liquify {
    #[inline]
    fn falloff(&self, d: f32) -> f32 {
        let t = (1.0 - d / self.radius.max(1e-3)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Deslocamento aplicado ao ponto (para vetor).
    pub fn forward(&self, x: f32, y: f32) -> (f32, f32) {
        let (dx, dy) = (x - self.center[0], y - self.center[1]);
        let d = (dx * dx + dy * dy).sqrt();
        let f = self.falloff(d);
        if f <= 0.0 {
            return (x, y);
        }
        match self.kind {
            LiquifyKind::Push => {
                let n = (self.direction[0].powi(2) + self.direction[1].powi(2)).sqrt().max(1e-6);
                (x + self.direction[0] / n * self.strength * f, y + self.direction[1] / n * self.strength * f)
            }
            LiquifyKind::Bulge | LiquifyKind::Pinch => {
                let s = if self.kind == LiquifyKind::Bulge { 1.0 } else { -1.0 } * self.strength.clamp(0.0, 1.0) * f * 0.5;
                (x + dx * s, y + dy * s)
            }
            LiquifyKind::Twirl => {
                let a = (self.strength * f).to_radians();
                let (sn, cs) = a.sin_cos();
                (self.center[0] + dx * cs - dy * sn, self.center[1] + dx * sn + dy * cs)
            }
        }
    }

    pub fn liquify_image(&self, src: &Image) -> Image {
        // inverso aproximado: aplica o oposto (bom para intensidades moderadas, como no Photoshop por pinceladas)
        let opposite = Liquify {
            strength: -self.strength,
            kind: match self.kind {
                LiquifyKind::Bulge => LiquifyKind::Pinch,
                LiquifyKind::Pinch => LiquifyKind::Bulge,
                k => k,
            },
            ..*self
        };
        let opp = if matches!(self.kind, LiquifyKind::Bulge | LiquifyKind::Pinch) { Liquify { strength: self.strength, ..opposite } } else { opposite };
        resample_inverse(src, src.width, src.height, move |x, y| Some(opp.forward(x, y)))
    }
}

// ---------------------------------------------------------------- repetição

/// Posições ao longo de uma polilinha a cada `spacing` px: (x, y, ângulo em graus da tangente).
pub fn positions_along(path: &[[f32; 2]], spacing: f32, offset: f32) -> Vec<(f32, f32, f32)> {
    let mut out = Vec::new();
    if path.len() < 2 || spacing <= 0.0 {
        return out;
    }
    let mut next = offset.max(0.0);
    let mut acc = 0.0f32;
    for seg in path.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 1e-6 {
            continue;
        }
        let ang = dy.atan2(dx).to_degrees();
        while next <= acc + len + 1e-4 {
            let t = (next - acc) / len;
            out.push((a[0] + dx * t, a[1] + dy * t, ang));
            next += spacing;
        }
        acc += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homography_maps_corners() {
        let src = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        let dst = [[10.0, 20.0], [90.0, 5.0], [120.0, 110.0], [0.0, 95.0]];
        let h = Homography::from_quads(src, dst).unwrap();
        for i in 0..4 {
            let (x, y) = h.apply(src[i][0], src[i][1]);
            assert!((x - dst[i][0]).abs() < 1e-3 && (y - dst[i][1]).abs() < 1e-3);
        }
        let inv = h.inverse().unwrap();
        let (x, y) = inv.apply(dst[2][0], dst[2][1]);
        assert!((x - 100.0).abs() < 1e-2 && (y - 100.0).abs() < 1e-2);
    }

    #[test]
    fn mesh_identity_and_inverse() {
        let mut m = MeshWarp::identity([0.0, 0.0, 100.0, 100.0], 2, 2);
        let (ax, ay) = m.forward(30.0, 70.0);
        assert!((ax - 30.0).abs() < 1e-4 && (ay - 70.0).abs() < 1e-4);
        m.points[4] = [60.0, 55.0]; // centro deslocado
        let (fx, fy) = m.forward(50.0, 50.0);
        assert!((fx - 60.0).abs() < 1e-4 && (fy - 55.0).abs() < 1e-4);
        let (ix, iy) = m.inverse_point(fx, fy).unwrap();
        assert!((ix - 50.0).abs() < 0.05 && (iy - 50.0).abs() < 0.05);
    }

    #[test]
    fn repetition_spacing() {
        let p = positions_along(&[[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]], 25.0, 0.0);
        assert_eq!(p.len(), 7);
        assert!((p[4].0 - 100.0).abs() < 1e-3 && p[5].2 == 90.0);
    }
}
