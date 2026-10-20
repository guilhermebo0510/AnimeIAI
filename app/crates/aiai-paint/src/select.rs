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

    /// Contorno da seleção (pixels com cobertura >= 0,5) como laços fechados nos cantos dos pixels, com os trechos
    /// retos juntados (as "formigas" seguem a forma de verdade, não a caixa). Cada laço deixa a parte selecionada à
    /// direita (y para baixo); buracos viram laços próprios.
    pub fn outline(&self) -> Vec<Vec<[f32; 2]>> {
        let (w, h) = (self.width as i64, self.height as i64);
        let inside = |x: i64, y: i64| x >= 0 && y >= 0 && x < w && y < h && self.cov[(y * w + x) as usize] >= 0.5;
        // arestas dirigidas entre vértices da grade (canto sup-esq do pixel = (x, y))
        let mut edges: Vec<((i64, i64), (i64, i64))> = Vec::new();
        for y in 0..h {
            for x in 0..w {
                if !inside(x, y) {
                    continue;
                }
                if !inside(x, y - 1) {
                    edges.push(((x, y), (x + 1, y)));
                }
                if !inside(x + 1, y) {
                    edges.push(((x + 1, y), (x + 1, y + 1)));
                }
                if !inside(x, y + 1) {
                    edges.push(((x + 1, y + 1), (x, y + 1)));
                }
                if !inside(x - 1, y) {
                    edges.push(((x, y + 1), (x, y)));
                }
            }
        }
        let mut from: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::with_capacity(edges.len());
        for (i, e) in edges.iter().enumerate() {
            from.entry(e.0).or_default().push(i);
        }
        let mut used = vec![false; edges.len()];
        let mut loops = Vec::new();
        for start in 0..edges.len() {
            if used[start] {
                continue;
            }
            let mut pts: Vec<(i64, i64)> = vec![edges[start].0];
            let mut cur = start;
            loop {
                used[cur] = true;
                let (a, b) = edges[cur];
                pts.push(b);
                // no vértice em xadrez há duas saídas: vira para a direita (mantém as regiões diagonais separadas)
                let dir = (b.0 - a.0, b.1 - a.1);
                let next = from.get(&b).and_then(|v| {
                    let free: Vec<usize> = v.iter().copied().filter(|&i| !used[i]).collect();
                    let right = (-dir.1, dir.0);
                    free.iter().copied().find(|&i| (edges[i].1 .0 - b.0, edges[i].1 .1 - b.1) == right).or_else(|| free.first().copied())
                });
                match next {
                    Some(n) => cur = n,
                    None => break,
                }
            }
            // junta os pontos colineares (o fecho repete o primeiro: tira)
            pts.pop();
            let n = pts.len();
            let simplified: Vec<[f32; 2]> = (0..n)
                .filter(|&i| {
                    let (p, q, r) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
                    (q.0 - p.0) * (r.1 - q.1) != (q.1 - p.1) * (r.0 - q.0)
                })
                .map(|i| [pts[i].0 as f32, pts[i].1 as f32])
                .collect();
            if simplified.len() >= 3 {
                loops.push(simplified);
            }
        }
        loops
    }
}

/// Varinha mágica como a do Photoshop. `tolerance` 0..255: a maior diferença por canal (RGBA pré-multiplicado,
/// escala 0..255) para a cor do pixel da semente; `contiguous` = só a região ligada à semente (4 vizinhos, não vaza
/// pelas diagonais de uma linha), senão todos os pixels parecidos da imagem; `antialias` = franja suave de 1 px do
/// lado de fora (o que está dentro continua com cobertura 1).
pub fn magic_wand(img: &Image, seed: (u32, u32), tolerance: f32, contiguous: bool, antialias: bool) -> Selection {
    let (w, h) = (img.width, img.height);
    let mut s = Selection::none(w, h);
    if seed.0 >= w || seed.1 >= h {
        return s;
    }
    let base = img.get(seed.0, seed.1);
    let tol = tolerance.clamp(0.0, 255.0) / 255.0 + 1e-5;
    let near = |p: &[f32; 4]| (0..4).all(|c| (p[c] - base[c]).abs() <= tol);
    if contiguous {
        let mut stack = vec![(seed.0, seed.1)];
        s.cov[(seed.1 * w + seed.0) as usize] = 1.0;
        while let Some((x, y)) = stack.pop() {
            let mut visit = |nx: u32, ny: u32, stack: &mut Vec<(u32, u32)>| {
                let i = (ny * w + nx) as usize;
                if s.cov[i] == 0.0 && near(&img.data[i]) {
                    s.cov[i] = 1.0;
                    stack.push((nx, ny));
                }
            };
            if x > 0 {
                visit(x - 1, y, &mut stack);
            }
            if x + 1 < w {
                visit(x + 1, y, &mut stack);
            }
            if y > 0 {
                visit(x, y - 1, &mut stack);
            }
            if y + 1 < h {
                visit(x, y + 1, &mut stack);
            }
        }
    } else {
        for (c, p) in s.cov.iter_mut().zip(&img.data) {
            if near(p) {
                *c = 1.0;
            }
        }
    }
    if antialias {
        let bin = s.cov.clone();
        let (wi, hi) = (w as i64, h as i64);
        for y in 0..hi {
            for x in 0..wi {
                let i = (y * wi + x) as usize;
                if bin[i] >= 1.0 {
                    continue;
                }
                let mut n = 0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (xx, yy) = (x + dx, y + dy);
                        if xx >= 0 && yy >= 0 && xx < wi && yy < hi && bin[(yy * wi + xx) as usize] >= 1.0 {
                            n += 1;
                        }
                    }
                }
                // no máximo 5/9 de vizinhos dentro: 0,42 < 0,5 (a franja nunca entra na seleção "dura")
                s.cov[i] = n as f32 / 9.0 * 0.75;
            }
        }
    }
    s
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

    /// Imagem sintética: fundo branco, um quadrado cinza claro (230) em 10..20 e outro cinza escuro (100) em 30..40,
    /// separados; uma linha preta vertical em x = 25.
    fn synthetic() -> Image {
        let mut img = Image::filled(50, 50, [1.0, 1.0, 1.0, 1.0]);
        for y in 10..20 {
            for x in 10..20 {
                let i = img.idx(x, y);
                img.data[i] = [230.0 / 255.0, 230.0 / 255.0, 230.0 / 255.0, 1.0];
            }
        }
        for y in 30..40 {
            for x in 30..40 {
                let i = img.idx(x, y);
                img.data[i] = [100.0 / 255.0, 100.0 / 255.0, 100.0 / 255.0, 1.0];
            }
        }
        for y in 0..50 {
            let i = img.idx(25, y);
            img.data[i] = [0.0, 0.0, 0.0, 1.0];
        }
        img
    }

    fn count(s: &Selection) -> usize {
        s.cov.iter().filter(|&&v| v >= 0.5).count()
    }

    #[test]
    fn wand_tolerance_controls_what_joins() {
        let img = synthetic();
        // tolerância 0: só o branco ligado à semente (lado esquerdo da linha, menos o quadrado claro)
        let tight = magic_wand(&img, (2, 2), 0.0, true, false);
        assert_eq!(count(&tight), 25 * 50 - 100);
        // 30 já engole o cinza 230 (diferença 25), não o 100 nem a linha
        let loose = magic_wand(&img, (2, 2), 30.0, true, false);
        assert_eq!(count(&loose), 25 * 50);
        // 255: tudo, inclusive a linha e o outro lado
        assert_eq!(count(&magic_wand(&img, (2, 2), 255.0, true, false)), 50 * 50);
        // semente no quadrado escuro: só ele
        assert_eq!(count(&magic_wand(&img, (35, 35), 10.0, true, false)), 100);
    }

    #[test]
    fn wand_contiguous_or_not() {
        let img = synthetic();
        // contíguo: a linha separa os dois lados
        let c = magic_wand(&img, (2, 2), 0.0, true, false);
        assert!(c.cov[img.idx(45, 5)] == 0.0, "o outro lado da linha não entra");
        // não contíguo: todo branco da imagem, dos dois lados
        let n = magic_wand(&img, (2, 2), 0.0, false, false);
        assert_eq!(count(&n), 50 * 50 - 50 - 100 - 100);
        assert!(n.cov[img.idx(45, 5)] == 1.0);
    }

    #[test]
    fn wand_on_transparent_layer_selects_the_hole_not_everything() {
        // camada vetorial renderizada: transparente com um anel preto; clicar dentro pega só o miolo
        let mut img = Image::filled(40, 40, [0.0, 0.0, 0.0, 0.0]);
        for y in 0..40u32 {
            for x in 0..40u32 {
                let d = ((x as f32 - 20.0).powi(2) + (y as f32 - 20.0).powi(2)).sqrt();
                if (9.0..12.0).contains(&d) {
                    let i = img.idx(x, y);
                    img.data[i] = [0.0, 0.0, 0.0, 1.0];
                }
            }
        }
        let inner = magic_wand(&img, (20, 20), 32.0, true, false);
        let n = count(&inner);
        assert!(n > 200 && n < 300, "só o miolo do anel (~πr²): {n}");
        assert_eq!(inner.bounds().map(|b| b[2] - b[0] <= 20), Some(true));
    }

    #[test]
    fn wand_antialias_softens_only_outside() {
        let img = synthetic();
        let hard = magic_wand(&img, (35, 35), 10.0, true, false);
        let soft = magic_wand(&img, (35, 35), 10.0, true, true);
        assert_eq!(count(&hard), count(&soft), "a franja não entra no >= 0,5");
        assert!(soft.cov[img.idx(29, 35)] > 0.0 && soft.cov[img.idx(29, 35)] < 0.5, "franja suave do lado de fora");
        assert_eq!(soft.cov[img.idx(35, 35)], 1.0);
    }

    #[test]
    fn ops_add_subtract_intersect_on_masks() {
        let a = Selection::from_shape(&SelectShape::Rect { rect: [0.0, 0.0, 20.0, 20.0] }, 40, 40, None).unwrap();
        let b = Selection::from_shape(&SelectShape::Rect { rect: [10.0, 10.0, 20.0, 20.0] }, 40, 40, None).unwrap();
        let mut add = a.clone();
        add.combine(&b, SelectOp::Add);
        assert_eq!(count(&add), 400 + 400 - 100);
        let mut sub = a.clone();
        sub.combine(&b, SelectOp::Subtract);
        assert_eq!(count(&sub), 300);
        assert_eq!(sub.cov[15 * 40 + 15], 0.0);
        let mut int = a.clone();
        int.combine(&b, SelectOp::Intersect);
        assert_eq!(count(&int), 100);
        assert_eq!(int.bounds(), Some([10, 10, 20, 20]));
        let mut inv = a.clone();
        inv.invert();
        assert_eq!(count(&inv), 1600 - 400);
    }

    #[test]
    fn outline_follows_the_shape() {
        // um L e um buraco: o contorno tem os cantos do L (6) e o buraco vira laço próprio
        let mut s = Selection::none(20, 20);
        for y in 2..12 {
            for x in 2..6 {
                s.cov[y * 20 + x] = 1.0;
            }
        }
        for y in 8..12 {
            for x in 6..12 {
                s.cov[y * 20 + x] = 1.0;
            }
        }
        let loops = s.outline();
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].len(), 6, "{:?}", loops[0]);
        let mut ring = Selection::from_shape(&SelectShape::Rect { rect: [2.0, 2.0, 10.0, 10.0] }, 20, 20, None).unwrap();
        let hole = Selection::from_shape(&SelectShape::Rect { rect: [5.0, 5.0, 3.0, 3.0] }, 20, 20, None).unwrap();
        ring.combine(&hole, SelectOp::Subtract);
        let loops = ring.outline();
        assert_eq!(loops.len(), 2, "fora e buraco");
        assert!(loops.iter().all(|l| l.len() == 4));
        assert!(Selection::none(5, 5).outline().is_empty());
    }
}
