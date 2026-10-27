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

    /// Deforma uma imagem (para raster), como a prévia na GPU: cada célula vira 16×16 quadradinhos levados pela malha e
    /// rasterizados em triângulos (coordenada de origem por baricêntricas). Não depende de inverter a malha, então
    /// células muito tortas não abrem buracos. O que estava dentro do retângulo sai do lugar; o resto fica.
    pub fn warp_image(&self, src: &Image) -> Image {
        const S: u32 = 16;
        let (w, h) = (src.width, src.height);
        let mut out = src.clone();
        if self.validate().is_err() {
            return out;
        }
        let r = self.rect;
        for y in 0..h {
            let cy = y as f32 + 0.5;
            if cy < r[1] || cy >= r[1] + r[3] {
                continue;
            }
            for x in 0..w {
                let cx = x as f32 + 0.5;
                if cx >= r[0] && cx < r[0] + r[2] {
                    out.data[(y * w + x) as usize] = [0.0; 4];
                }
            }
        }
        let (nu, nv) = (self.cols * S, self.rows * S);
        let src_pt = |i: u32, j: u32| [r[0] + r[2] * i as f32 / nu as f32, r[1] + r[3] * j as f32 / nv as f32];
        let mut dst = Vec::with_capacity(((nu + 1) * (nv + 1)) as usize);
        for j in 0..=nv {
            for i in 0..=nu {
                let p = src_pt(i, j);
                // o último vértice de cada lado fica exatamente no ponto da malha
                let q = self.edge_forward(i == nu, j == nv, p);
                dst.push([q.0, q.1]);
            }
        }
        let at = |i: u32, j: u32| dst[(j * (nu + 1) + i) as usize];
        for j in 0..nv {
            for i in 0..nu {
                let (s00, s10, s01, s11) = (src_pt(i, j), src_pt(i + 1, j), src_pt(i, j + 1), src_pt(i + 1, j + 1));
                let (d00, d10, d01, d11) = (at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1));
                raster_tri(src, &mut out, [d00, d10, d11], [s00, s10, s11]);
                raster_tri(src, &mut out, [d00, d11, d01], [s00, s11, s01]);
            }
        }
        out
    }

    /// `forward` dentro do retângulo, incluindo as bordas de baixo/direita (onde a busca de célula cairia fora).
    fn edge_forward(&self, right: bool, bottom: bool, p: [f32; 2]) -> (f32, f32) {
        let u = if right { self.cols as f32 } else { (p[0] - self.rect[0]) / self.rect[2] * self.cols as f32 };
        let v = if bottom { self.rows as f32 } else { (p[1] - self.rect[1]) / self.rect[3] * self.rows as f32 };
        let i = (u.floor() as u32).min(self.cols - 1);
        let j = (v.floor() as u32).min(self.rows - 1);
        let (fu, fv) = (u - i as f32, v - j as f32);
        let (a, b, c, d) = (self.vtx(i, j), self.vtx(i + 1, j), self.vtx(i, j + 1), self.vtx(i + 1, j + 1));
        let top = [a[0] + (b[0] - a[0]) * fu, a[1] + (b[1] - a[1]) * fu];
        let bot = [c[0] + (d[0] - c[0]) * fu, c[1] + (d[1] - c[1]) * fu];
        (top[0] + (bot[0] - top[0]) * fv, top[1] + (bot[1] - top[1]) * fv)
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

/// Pinta em `out` o triângulo de destino `d`, com a cor de `src` no ponto correspondente do triângulo `s`.
fn raster_tri(src: &Image, out: &mut Image, d: [[f32; 2]; 3], s: [[f32; 2]; 3]) {
    let area = (d[1][0] - d[0][0]) * (d[2][1] - d[0][1]) - (d[2][0] - d[0][0]) * (d[1][1] - d[0][1]);
    if area.abs() < 1e-6 {
        return;
    }
    let x0 = d.iter().map(|p| p[0]).fold(f32::MAX, f32::min).floor().max(0.0) as i64;
    let x1 = d.iter().map(|p| p[0]).fold(f32::MIN, f32::max).ceil().min(out.width as f32) as i64;
    let y0 = d.iter().map(|p| p[1]).fold(f32::MAX, f32::min).floor().max(0.0) as i64;
    let y1 = d.iter().map(|p| p[1]).fold(f32::MIN, f32::max).ceil().min(out.height as f32) as i64;
    const E: f32 = 1e-4;
    for y in y0..y1 {
        for x in x0..x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((d[1][0] - px) * (d[2][1] - py) - (d[2][0] - px) * (d[1][1] - py)) / area;
            let w1 = ((d[2][0] - px) * (d[0][1] - py) - (d[0][0] - px) * (d[2][1] - py)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < -E || w1 < -E || w2 < -E {
                continue;
            }
            let sx = w0 * s[0][0] + w1 * s[1][0] + w2 * s[2][0];
            let sy = w0 * s[0][1] + w1 * s[1][1] + w2 * s[2][1];
            out.data[(y as u32 * out.width + x as u32) as usize] = sample_bilinear(src, sx, sy);
        }
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

    /// O passo oposto (inverso aproximado, bom para passos pequenos como os de uma pincelada).
    pub fn opposite(&self) -> Liquify {
        let opposite = Liquify {
            strength: -self.strength,
            kind: match self.kind {
                LiquifyKind::Bulge => LiquifyKind::Pinch,
                LiquifyKind::Pinch => LiquifyKind::Bulge,
                k => k,
            },
            ..*self
        };
        if matches!(self.kind, LiquifyKind::Bulge | LiquifyKind::Pinch) { Liquify { strength: self.strength, ..opposite } } else { opposite }
    }

    /// O ponto cai na área de efeito?
    #[inline]
    fn touches(&self, x: f32, y: f32) -> bool {
        let (dx, dy) = (x - self.center[0], y - self.center[1]);
        dx * dx + dy * dy < self.radius * self.radius
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

/// Pincelada de liquify (Liquefazer do Mover): passos em ordem. Para um ponto, um passo depois do outro.
pub fn liquify_path_forward(steps: &[Liquify], x: f32, y: f32) -> (f32, f32) {
    steps.iter().fold((x, y), |(x, y), s| if s.touches(x, y) { s.forward(x, y) } else { (x, y) })
}

/// Pincelada de liquify numa imagem: os opostos na ordem contrária, numa reamostragem só (sem borrar a cada passo).
/// Fora da caixa de todos os passos a imagem fica igual.
pub fn liquify_path_image(steps: &[Liquify], src: &Image) -> Image {
    if steps.is_empty() {
        return src.clone();
    }
    let opp: Vec<Liquify> = steps.iter().rev().map(Liquify::opposite).collect();
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for s in steps {
        x0 = x0.min(s.center[0] - s.radius);
        y0 = y0.min(s.center[1] - s.radius);
        x1 = x1.max(s.center[0] + s.radius);
        y1 = y1.max(s.center[1] + s.radius);
    }
    resample_inverse(src, src.width, src.height, move |x, y| {
        if x < x0 || x > x1 || y < y0 || y > y1 {
            return Some((x, y));
        }
        Some(opp.iter().fold((x, y), |(x, y), s| if s.touches(x, y) { s.forward(x, y) } else { (x, y) }))
    })
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

/// Marionete (Puppet Warp, auditoria 4.7h): deformação rígida por mínimos quadrados móveis (MLS, Schaefer et al. 2006)
/// a partir dos pinos `(repouso, atual)`. Perto de um pino o desenho segue o pino; entre pinos, gira e anda sem esticar.
/// Um pino só = translação; nenhum = identidade.
pub fn mls_rigid(pins: &[([f32; 2], [f32; 2])], v: [f32; 2]) -> [f32; 2] {
    match pins.len() {
        0 => return v,
        1 => return [v[0] + pins[0].1[0] - pins[0].0[0], v[1] + pins[0].1[1] - pins[0].0[1]],
        _ => {}
    }
    let mut ws = Vec::with_capacity(pins.len());
    for (p, q) in pins {
        let d2 = (p[0] - v[0]).powi(2) + (p[1] - v[1]).powi(2);
        if d2 < 1e-8 {
            return *q;
        }
        ws.push(1.0 / d2 as f64);
    }
    let sw: f64 = ws.iter().sum();
    let (mut ps, mut qs) = ([0.0f64; 2], [0.0f64; 2]);
    for ((p, q), w) in pins.iter().zip(&ws) {
        ps[0] += w * p[0] as f64;
        ps[1] += w * p[1] as f64;
        qs[0] += w * q[0] as f64;
        qs[1] += w * q[1] as f64;
    }
    ps = [ps[0] / sw, ps[1] / sw];
    qs = [qs[0] / sw, qs[1] / sw];
    // rotação que melhor leva p̂ a q̂ (soma ponderada de conj(p̂)·q̂, em complexo)
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for ((p, q), w) in pins.iter().zip(&ws) {
        let (px, py) = (p[0] as f64 - ps[0], p[1] as f64 - ps[1]);
        let (qx, qy) = (q[0] as f64 - qs[0], q[1] as f64 - qs[1]);
        re += w * (px * qx + py * qy);
        im += w * (px * qy - py * qx);
    }
    let m = (re * re + im * im).sqrt();
    let (c, s) = if m > 1e-12 { (re / m, im / m) } else { (1.0, 0.0) };
    let (dx, dy) = (v[0] as f64 - ps[0], v[1] as f64 - ps[1]);
    [(dx * c - dy * s + qs[0]) as f32, (dx * s + dy * c + qs[1]) as f32]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puppet_pins_pull_and_rotate_rigidly() {
        assert_eq!(mls_rigid(&[], [3.0, 4.0]), [3.0, 4.0]);
        assert_eq!(mls_rigid(&[([0.0, 0.0], [10.0, 5.0])], [3.0, 4.0]), [13.0, 9.0]);
        // dois pinos parados = nada muda
        let still = [([0.0, 0.0], [0.0, 0.0]), ([100.0, 0.0], [100.0, 0.0])];
        let v = mls_rigid(&still, [50.0, 20.0]);
        assert!((v[0] - 50.0).abs() < 1e-3 && (v[1] - 20.0).abs() < 1e-3);
        // no pino, o ponto vai com ele
        let moved = [([0.0, 0.0], [0.0, 0.0]), ([100.0, 0.0], [100.0, 40.0])];
        assert_eq!(mls_rigid(&moved, [100.0, 0.0]), [100.0, 40.0]);
        // girar 90° os dois pinos em volta do meio gira o ponto do meio para cima/baixo sem esticar
        let rot = [([-50.0, 0.0], [0.0, -50.0]), ([50.0, 0.0], [0.0, 50.0])];
        let v = mls_rigid(&rot, [0.0, 10.0]);
        assert!((v[0] + 10.0).abs() < 1e-3 && v[1].abs() < 1e-3, "{v:?}");
    }

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
