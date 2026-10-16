//! Seleções como máscaras de cobertura do tamanho do canvas: retângulo, elipse, laço (polígono), varinha mágica
//! (região por cor ou por linhas, reaproveitando o balde), faixa de cor, e operações (somar, subtrair, intersectar,
//! expandir, contrair, suavizar).

use crate::fill::{FillParams, boundary_from_color, label_regions};
use crate::image::Image;
use crate::poly::fill_polygons;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub width: u32,
    pub height: u32,
    pub cov: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SelectShape {
    Rect { rect: [f32; 4] },
    Ellipse { rect: [f32; 4] },
    Lasso { points: Vec<[f32; 2]> },
    /// Varinha mágica: região contígua de cor parecida com a da semente.
    Wand { seed: [f32; 2], tolerance: f32, #[serde(default)] gap: f32 },
    /// Faixa de cor: todos os pixels (não contíguos) perto da cor.
    ColorRange { color: [f32; 3], tolerance: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectOp {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}

impl Selection {
    pub fn none(width: u32, height: u32) -> Self {
        Selection { width, height, cov: vec![0.0; (width * height) as usize] }
    }

    pub fn all(width: u32, height: u32) -> Self {
        Selection { width, height, cov: vec![1.0; (width * height) as usize] }
    }

    pub fn is_empty(&self) -> bool {
        self.cov.iter().all(|&v| v <= 0.0)
    }

    /// Caixa [x0, y0, x1, y1) do que está selecionado.
    pub fn bounds(&self) -> Option<[i32; 4]> {
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for (i, &v) in self.cov.iter().enumerate() {
            if v > 0.0 {
                let (x, y) = ((i % self.width as usize) as i32, (i / self.width as usize) as i32);
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
        (x0 <= x1 && x0 != i32::MAX).then_some([x0, y0, x1, y1])
    }

    /// Cria a seleção de uma forma; `reference` = imagem usada pela varinha/faixa de cor (camada ou composição).
    pub fn from_shape(shape: &SelectShape, width: u32, height: u32, reference: Option<&Image>) -> Result<Selection, String> {
        let mut s = Selection::none(width, height);
        match shape {
            SelectShape::Rect { rect } => {
                let poly = vec![[rect[0], rect[1]], [rect[0] + rect[2], rect[1]], [rect[0] + rect[2], rect[1] + rect[3]], [rect[0], rect[1] + rect[3]]];
                s.paint_poly(&[poly]);
            }
            SelectShape::Ellipse { rect } => {
                let (cx, cy, rx, ry) = (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0, rect[2] / 2.0, rect[3] / 2.0);
                let n = ((rx + ry) * 0.6).clamp(32.0, 720.0) as usize;
                let poly: Vec<[f32; 2]> = (0..n).map(|i| {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    [cx + rx * a.cos(), cy + ry * a.sin()]
                }).collect();
                s.paint_poly(&[poly]);
            }
            SelectShape::Lasso { points } => {
                if points.len() < 3 {
                    return Err("laço precisa de pelo menos 3 pontos".into());
                }
                s.paint_poly(&[points.clone()]);
            }
            SelectShape::Wand { seed, tolerance, gap } => {
                let img = reference.ok_or("varinha precisa de uma imagem de referência")?;
                let (sx, sy) = (seed[0] as u32, seed[1] as u32);
                if sx >= width || sy >= height {
                    return Err("semente fora do canvas".into());
                }
                let boundary = boundary_from_color(img, (sx, sy), *tolerance);
                let regions = label_regions(&boundary, width as usize, height as usize, &FillParams { gap: *gap, expand: 0, line_threshold: 0.5 });
                let l = regions.label_near(sx, sy);
                if l != 0 {
                    for (i, &v) in regions.labels.iter().enumerate() {
                        if v == l {
                            s.cov[i] = 1.0;
                        }
                    }
                }
            }
            SelectShape::ColorRange { color, tolerance } => {
                let img = reference.ok_or("faixa de cor precisa de uma imagem de referência")?;
                for (i, p) in img.data.iter().enumerate() {
                    if p[3] <= 0.0 {
                        continue;
                    }
                    let un = [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
                    let d = (0..3).map(|c| (un[c] - color[c]).abs()).fold(0.0f32, f32::max);
                    if d <= *tolerance {
                        s.cov[i] = 1.0;
                    }
                }
            }
        }
        Ok(s)
    }

    fn paint_poly(&mut self, polys: &[Vec<[f32; 2]>]) {
        if let Some(c) = fill_polygons(polys, false, self.width, self.height) {
            for y in 0..c.height {
                for x in 0..c.width {
                    let i = ((c.y0 as u32 + y) * self.width + c.x0 as u32 + x) as usize;
                    self.cov[i] = self.cov[i].max(c.cov[(y * c.width + x) as usize]);
                }
            }
        }
    }

    pub fn combine(&mut self, other: &Selection, op: SelectOp) {
        for (a, &b) in self.cov.iter_mut().zip(&other.cov) {
            *a = match op {
                SelectOp::Replace => b,
                SelectOp::Add => a.max(b),
                SelectOp::Subtract => (*a - b).max(0.0),
                SelectOp::Intersect => a.min(b),
            };
        }
    }

    /// Expande (`px` > 0) ou contrai (`px` < 0) por distância.
    pub fn grow(&mut self, px: f32) {
        let w = self.width as usize;
        let h = self.height as usize;
        let inside: Vec<bool> = self.cov.iter().map(|&v| v >= 0.5).collect();
        if px > 0.0 {
            let d = crate::fill::distance_to_boundary(&inside, w, h);
            for (i, c) in self.cov.iter_mut().enumerate() {
                *c = if inside[i] { 1.0 } else { (px + 0.5 - d[i]).clamp(0.0, 1.0) };
            }
        } else if px < 0.0 {
            let outside: Vec<bool> = inside.iter().map(|b| !b).collect();
            let d = crate::fill::distance_to_boundary(&outside, w, h);
            for (i, c) in self.cov.iter_mut().enumerate() {
                *c = if !inside[i] { 0.0 } else { (d[i] - (-px) + 0.5).clamp(0.0, 1.0) };
            }
        }
    }

    /// Suaviza a borda (desfoque de caixa 2 passes).
    pub fn feather(&mut self, radius: u32) {
        if radius == 0 {
            return;
        }
        let (w, h) = (self.width as usize, self.height as usize);
        let r = radius as i32;
        for _ in 0..2 {
            let mut tmp = vec![0.0f32; w * h];
            for y in 0..h {
                for x in 0..w {
                    let (a, b) = ((x as i32 - r).max(0) as usize, ((x as i32 + r) as usize).min(w - 1));
                    tmp[y * w + x] = self.cov[y * w + a..=y * w + b].iter().sum::<f32>() / (b - a + 1) as f32;
                }
            }
            for x in 0..w {
                for y in 0..h {
                    let (a, b) = ((y as i32 - r).max(0) as usize, ((y as i32 + r) as usize).min(h - 1));
                    let s: f32 = (a..=b).map(|yy| tmp[yy * w + x]).sum();
                    self.cov[y * w + x] = s / (b - a + 1) as f32;
                }
            }
        }
    }

    pub fn invert(&mut self) {
        for c in &mut self.cov {
            *c = 1.0 - *c;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_and_ops() {
        let r = Selection::from_shape(&SelectShape::Rect { rect: [10.0, 10.0, 20.0, 10.0] }, 64, 64, None).unwrap();
        assert!((r.cov.iter().sum::<f32>() - 200.0).abs() < 0.5);
        let mut e = Selection::from_shape(&SelectShape::Ellipse { rect: [0.0, 0.0, 40.0, 40.0] }, 64, 64, None).unwrap();
        let area = e.cov.iter().sum::<f32>();
        assert!((area - std::f32::consts::PI * 400.0).abs() < 15.0, "{area}");
        e.combine(&r, SelectOp::Subtract);
        assert!(e.cov.iter().sum::<f32>() < area);
        let mut g = r.clone();
        g.grow(2.0);
        assert!(g.cov.iter().sum::<f32>() > 250.0);
        assert_eq!(r.bounds(), Some([10, 10, 30, 20]));
    }

    #[test]
    fn wand_selects_contiguous_color() {
        let mut img = Image::filled(50, 50, [1.0, 1.0, 1.0, 1.0]);
        for y in 0..50 {
            let i = img.idx(25, y);
            img.data[i] = [0.0, 0.0, 0.0, 1.0];
        }
        let s = Selection::from_shape(&SelectShape::Wand { seed: [5.0, 5.0], tolerance: 0.1, gap: 0.0 }, 50, 50, Some(&img)).unwrap();
        let count = s.cov.iter().filter(|&&v| v > 0.0).count();
        assert_eq!(count, 25 * 50, "só o lado esquerdo da linha");
    }
}
