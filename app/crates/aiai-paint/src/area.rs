//! Áreas das ferramentas funcionais do S25 (à moda do Clip Studio), sem dependência de interface:
//! - varinha com "fechar intervalo" (as frestas da linha não deixam a seleção vazar) e "dimensionamento de área"
//!   (expande/contrai o resultado), além de só pixels conectados e tolerância;
//! - caneta de seleção e borracha de seleção (pintar/apagar a máscara com um pincel redondo);
//! - encolher seleção (o laço em volta do desenho encolhe até o contorno do que está pintado);
//! - laço magnético (os pontos do laço grudam na borda mais forte por perto);
//! - balde: regiões cercadas por um laço ("cercar e preencher") e regiões tocadas por um pincel ("pintar área não
//!   preenchida").
//!
//! Tudo em px do canvas; as máscaras são `Selection` (cobertura 0..1 do tamanho do canvas).

use crate::fill::{FillParams, Regions, label_regions};
use crate::image::Image;
use crate::select::Selection;

/// Opções da varinha (Seleção automática).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WandOptions {
    /// 0..255: a maior diferença por canal (RGBA pré-multiplicado) para a cor da semente.
    pub tolerance: f32,
    /// Só a região ligada ao clique (senão, todos os pixels parecidos da imagem).
    pub contiguous: bool,
    /// Fechar intervalo: largura (px) da fresta que ainda conta como fechada (0 = desligado; só vale com `contiguous`).
    pub gap: f32,
    /// Dimensionamento de área: px a mais (> 0) ou a menos (< 0) em volta do resultado.
    pub scale: f32,
    /// Franja suave de 1 px do lado de fora.
    pub antialias: bool,
}

impl Default for WandOptions {
    fn default() -> Self {
        WandOptions { tolerance: 32.0, contiguous: true, gap: 0.0, scale: 0.0, antialias: true }
    }
}

/// Varinha com fechar intervalo e dimensionamento de área. Com `gap` > 0, os pixels diferentes da semente são linha e
/// a região sai da rotulagem por competição do balde (`label_regions`): uma fresta mais estreita que `gap` não liga a
/// região da semente à vizinha (a fresta é dividida ao meio entre as duas, como no balde).
pub fn wand(img: &Image, seed: (u32, u32), o: &WandOptions) -> Selection {
    let (w, h) = (img.width, img.height);
    if !(o.contiguous && o.gap > 0.0) || seed.0 >= w || seed.1 >= h {
        let mut s = crate::select::magic_wand(img, seed, o.tolerance, o.contiguous, o.antialias && o.scale == 0.0);
        if o.scale != 0.0 {
            s.grow(o.scale);
        }
        return s;
    }
    let base = img.get(seed.0, seed.1);
    let tol = o.tolerance.clamp(0.0, 255.0) / 255.0 + 1e-5;
    let boundary: Vec<bool> = img.data.iter().map(|p| (0..4).any(|c| (p[c] - base[c]).abs() > tol)).collect();
    let regions = label_regions(&boundary, w as usize, h as usize, &FillParams { gap: o.gap, expand: 0, line_threshold: 0.5 });
    let label = regions.label_at(seed.0, seed.1);
    let mut s = Selection::none(w, h);
    if label != 0 {
        for (c, &l) in s.cov.iter_mut().zip(&regions.labels) {
            if l == label {
                *c = 1.0;
            }
        }
    }
    if o.scale != 0.0 {
        s.grow(o.scale);
    } else if o.antialias {
        fringe(&mut s);
    }
    s
}

/// Franja de 1 px do lado de fora (cobertura pela fração dos 8 vizinhos dentro).
pub fn fringe(s: &mut Selection) {
    let bin = s.cov.clone();
    let (w, h) = (s.width as i64, s.height as i64);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            if bin[i] >= 1.0 {
                continue;
            }
            let mut n = 0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (xx, yy) = (x + dx, y + dy);
                    if xx >= 0 && yy >= 0 && xx < w && yy < h && bin[(yy * w + xx) as usize] >= 1.0 {
                        n += 1;
                    }
                }
            }
            if n > 0 {
                s.cov[i] = s.cov[i].max(n as f32 / 9.0 * 0.5);
            }
        }
    }
}

/// Distância de `p` ao segmento `a`–`b`.
fn seg_dist(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 1e-12 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let (qx, qy) = (a[0] + dx * t, a[1] + dy * t);
    (p[0] - qx).hypot(p[1] - qy)
}

/// Cobertura de um pincel redondo de raio `radius` arrastado por `pts` (cápsulas entre os pontos, borda suave de 1 px),
/// em (x0, y0, largura, altura, cobertura). `None` = fora do canvas.
pub fn brush_coverage(w: u32, h: u32, pts: &[[f32; 2]], radius: f32) -> Option<(u32, u32, u32, u32, Vec<f32>)> {
    let first = pts.first()?;
    let r = radius.max(0.5);
    let (mut x0, mut y0, mut x1, mut y1) = (first[0], first[1], first[0], first[1]);
    for p in pts {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let bx0 = (x0 - r - 1.0).floor().max(0.0) as u32;
    let by0 = (y0 - r - 1.0).floor().max(0.0) as u32;
    let bx1 = ((x1 + r + 1.0).ceil().max(0.0) as u32).min(w);
    let by1 = ((y1 + r + 1.0).ceil().max(0.0) as u32).min(h);
    if bx0 >= bx1 || by0 >= by1 {
        return None;
    }
    let (bw, bh) = (bx1 - bx0, by1 - by0);
    let mut cov = vec![0.0f32; (bw * bh) as usize];
    let segs: Vec<([f32; 2], [f32; 2])> = if pts.len() == 1 { vec![(pts[0], pts[0])] } else { pts.windows(2).map(|s| (s[0], s[1])).collect() };
    for (a, b) in segs {
        let sx0 = ((a[0].min(b[0]) - r - 1.0).floor().max(bx0 as f32)) as u32;
        let sy0 = ((a[1].min(b[1]) - r - 1.0).floor().max(by0 as f32)) as u32;
        let sx1 = ((a[0].max(b[0]) + r + 1.0).ceil() as u32).min(bx1);
        let sy1 = ((a[1].max(b[1]) + r + 1.0).ceil() as u32).min(by1);
        for y in sy0..sy1 {
            for x in sx0..sx1 {
                let d = seg_dist([x as f32 + 0.5, y as f32 + 0.5], a, b);
                let c = (r + 0.5 - d).clamp(0.0, 1.0);
                if c > 0.0 {
                    let i = ((y - by0) * bw + (x - bx0)) as usize;
                    cov[i] = cov[i].max(c);
                }
            }
        }
    }
    Some((bx0, by0, bw, bh, cov))
}

/// Caneta de seleção (`add`) e borracha de seleção: pinta ou apaga a máscara com um pincel redondo.
pub fn paint_selection(sel: &mut Selection, pts: &[[f32; 2]], radius: f32, add: bool) {
    let Some((x0, y0, bw, bh, cov)) = brush_coverage(sel.width, sel.height, pts, radius) else { return };
    for y in 0..bh {
        for x in 0..bw {
            let c = cov[(y * bw + x) as usize];
            if c <= 0.0 {
                continue;
            }
            let i = ((y0 + y) * sel.width + x0 + x) as usize;
            sel.cov[i] = if add { sel.cov[i].max(c) } else { sel.cov[i].min(1.0 - c) };
        }
    }
}

/// Encolher seleção: dentro da área `region` (o laço), tira o vazio transparente que encosta na borda do laço
/// (inundação pelos pixels transparentes a partir da borda), sobrando o desenho e os buracos cercados por ele — o laço
/// "encolhe" até o contorno do que está pintado. `alpha` = alfa mínimo para um pixel contar como desenho.
pub fn shrink_to_drawing(region: &Selection, img: &Image, alpha: f32) -> Selection {
    let (w, h) = (region.width as usize, region.height as usize);
    let inside: Vec<bool> = region.cov.iter().map(|&c| c >= 0.5).collect();
    let drawn = |i: usize| img.data.get(i).is_some_and(|p| p[3] >= alpha);
    let mut gone = vec![false; w * h];
    let mut stack = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !inside[i] || drawn(i) {
                continue;
            }
            // pixel transparente na borda do laço (ou do canvas)
            let edge = x == 0 || y == 0 || x + 1 == w || y + 1 == h || !inside[i - 1] || !inside[i + 1] || !inside[i - w] || !inside[i + w];
            if edge {
                gone[i] = true;
                stack.push(i);
            }
        }
    }
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        let neigh = [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)];
        for j in neigh.into_iter().flatten() {
            if inside[j] && !gone[j] && !drawn(j) {
                gone[j] = true;
                stack.push(j);
            }
        }
    }
    let mut out = Selection::none(region.width, region.height);
    for i in 0..w * h {
        if inside[i] && !gone[i] {
            out.cov[i] = 1.0;
        }
    }
    out
}

/// Força da borda (Sobel do RGBA pré-multiplicado, somado nos canais), normalizada 0..1.
pub fn edge_strength(img: &Image) -> Vec<f32> {
    let (w, h) = (img.width as i64, img.height as i64);
    let at = |x: i64, y: i64| img.data[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize];
    let mut out = vec![0.0f32; (w * h) as usize];
    let mut max = 0.0f32;
    for y in 0..h {
        for x in 0..w {
            let mut m = 0.0f32;
            for c in 0..4 {
                let gx = at(x + 1, y - 1)[c] + 2.0 * at(x + 1, y)[c] + at(x + 1, y + 1)[c] - at(x - 1, y - 1)[c] - 2.0 * at(x - 1, y)[c] - at(x - 1, y + 1)[c];
                let gy = at(x - 1, y + 1)[c] + 2.0 * at(x, y + 1)[c] + at(x + 1, y + 1)[c] - at(x - 1, y - 1)[c] - 2.0 * at(x, y - 1)[c] - at(x + 1, y - 1)[c];
                m += gx.hypot(gy);
            }
            out[(y * w + x) as usize] = m;
            max = max.max(m);
        }
    }
    if max > 0.0 {
        out.iter_mut().for_each(|v| *v /= max);
    }
    out
}

/// Laço magnético: cada ponto do caminho (adensado a cada 2 px) vai para o pixel de borda mais forte num raio
/// `radius`, pesando a distância (perto ganha no empate); sem borda por perto (força < 0,08) o ponto fica onde está.
pub fn magnetic_path(edges: &[f32], w: u32, h: u32, pts: &[[f32; 2]], radius: f32) -> Vec<[f32; 2]> {
    let mut dense: Vec<[f32; 2]> = Vec::new();
    for (k, p) in pts.iter().enumerate() {
        if k > 0 {
            let a = pts[k - 1];
            let n = ((p[0] - a[0]).hypot(p[1] - a[1]) / 2.0).floor() as usize;
            for s in 1..n {
                let t = s as f32 / n as f32;
                dense.push([a[0] + (p[0] - a[0]) * t, a[1] + (p[1] - a[1]) * t]);
            }
        }
        dense.push(*p);
    }
    let r = radius.max(1.0);
    let ri = r.ceil() as i64;
    dense
        .into_iter()
        .map(|p| {
            let (cx, cy) = (p[0].floor() as i64, p[1].floor() as i64);
            let mut best: Option<(f32, [f32; 2])> = None;
            for dy in -ri..=ri {
                for dx in -ri..=ri {
                    let d = ((dx * dx + dy * dy) as f32).sqrt();
                    let (x, y) = (cx + dx, cy + dy);
                    if d > r || x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                        continue;
                    }
                    let e = edges[(y as u32 * w + x as u32) as usize];
                    if e < 0.08 {
                        continue;
                    }
                    let score = e - 0.25 * d / r;
                    if best.is_none_or(|(b, _)| score > b) {
                        best = Some((score, [x as f32 + 0.5, y as f32 + 0.5]));
                    }
                }
            }
            best.map(|(_, q)| q).unwrap_or(p)
        })
        .collect()
}

/// Cercar e preencher: rótulos das regiões que o laço `lasso` cerca (pelo menos `min_frac` dos pixels da região
/// dentro do laço; a região de fora, que só entra em parte, fica de fora).
pub fn enclosed_labels(regions: &Regions, lasso: &Selection, min_frac: f32) -> Vec<u32> {
    let n = regions.count as usize + 1;
    let (mut total, mut inside) = (vec![0usize; n], vec![0usize; n]);
    for (i, &l) in regions.labels.iter().enumerate() {
        if l == 0 {
            continue;
        }
        total[l as usize] += 1;
        if lasso.cov.get(i).is_some_and(|&c| c >= 0.5) {
            inside[l as usize] += 1;
        }
    }
    (1..n).filter(|&l| inside[l] > 0 && inside[l] as f32 >= min_frac * total[l] as f32).map(|l| l as u32).collect()
}

/// Pintar área não preenchida: rótulos das regiões que o pincel (`pts`, raio `radius`) toca.
pub fn touched_labels(regions: &Regions, pts: &[[f32; 2]], radius: f32) -> Vec<u32> {
    let (w, h) = (regions.width as u32, regions.height as u32);
    let Some((x0, y0, bw, bh, cov)) = brush_coverage(w, h, pts, radius) else { return Vec::new() };
    let mut seen = vec![false; regions.count as usize + 1];
    for y in 0..bh {
        for x in 0..bw {
            if cov[(y * bw + x) as usize] >= 0.5 {
                seen[regions.label_at(x0 + x, y0 + y) as usize] = true;
            }
        }
    }
    (1..seen.len()).filter(|&l| seen[l]).map(|l| l as u32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::boundary_from_alpha;

    /// Quadrado de linha 2 px (de 10 a 30) com uma fresta de `gap` px no lado direito.
    fn square_with_gap(gap: u32) -> Image {
        let mut img = Image::filled(48, 40, [1.0, 1.0, 1.0, 1.0]);
        let ink = [0.0, 0.0, 0.0, 1.0];
        for y in 10..31u32 {
            for x in 10..31u32 {
                let border = x < 12 || x > 28 || y < 12 || y > 28;
                let in_gap = x > 28 && (19..19 + gap).contains(&y);
                if border && !in_gap {
                    let i = img.idx(x, y);
                    img.data[i] = ink;
                }
            }
        }
        img
    }

    fn count(s: &Selection) -> usize {
        s.cov.iter().filter(|&&c| c >= 0.5).count()
    }

    #[test]
    fn wand_close_gap_stops_the_leak() {
        let img = square_with_gap(3);
        let open = WandOptions { antialias: false, ..Default::default() };
        let leak = wand(&img, (20, 20), &open);
        // sem fechar intervalo a seleção vaza pela fresta e pega o fundo
        assert!(leak.cov[(2 * 48 + 2) as usize] >= 0.5);
        let closed = wand(&img, (20, 20), &WandOptions { gap: 5.0, ..open });
        assert!(closed.cov[(2 * 48 + 2) as usize] < 0.5, "a fresta de 3 px fecha com intervalo 5");
        let n = count(&closed);
        // o miolo 17×17 (de 12 a 28), mais a metade da fresta no máximo
        assert!((17 * 17..=17 * 17 + 6).contains(&n), "{n}");
    }

    #[test]
    fn wand_area_scaling_grows_and_shrinks() {
        let img = square_with_gap(0);
        let o = WandOptions { antialias: false, ..Default::default() };
        let base = count(&wand(&img, (20, 20), &o));
        assert_eq!(base, 17 * 17);
        let grown = count(&wand(&img, (20, 20), &WandOptions { scale: 2.0, ..o }));
        // a contração deixa meia cobertura no anel da borda (o `grow` suaviza): conta só os pixels inteiros
        let shrunk = wand(&img, (20, 20), &WandOptions { scale: -2.0, ..o }).cov.iter().filter(|&&c| c > 0.99).count();
        assert!(grown >= 21 * 21 - 16 && grown <= 21 * 21, "{grown}");
        assert_eq!(shrunk, 13 * 13);
    }

    #[test]
    fn selection_pen_paints_and_erases() {
        let mut s = Selection::none(40, 40);
        paint_selection(&mut s, &[[5.0, 20.0], [35.0, 20.0]], 3.0, true);
        assert!(s.cov[(20 * 40 + 20) as usize] >= 1.0 && s.cov[(20 * 40) as usize] < 0.5);
        assert!(s.cov[(27 * 40 + 20) as usize] == 0.0);
        paint_selection(&mut s, &[[20.0, 10.0], [20.0, 30.0]], 2.0, false);
        assert_eq!(s.cov[(20 * 40 + 20) as usize], 0.0, "a borracha abre um corte");
        assert!(s.cov[(20 * 40 + 10) as usize] >= 1.0);
    }

    #[test]
    fn shrink_selection_hugs_the_drawing() {
        // um disco pintado com um buraco transparente no meio, e um laço bem maior em volta
        let mut img = Image::new(60, 60);
        for y in 0..60u32 {
            for x in 0..60u32 {
                let d = ((x as f32 - 30.0).powi(2) + (y as f32 - 30.0).powi(2)).sqrt();
                if d < 12.0 && d >= 4.0 {
                    let i = img.idx(x, y);
                    img.data[i] = [0.5, 0.0, 0.0, 1.0];
                }
            }
        }
        let lasso = Selection::from_shape(&crate::select::SelectShape::Rect { rect: [5.0, 5.0, 50.0, 50.0] }, 60, 60, None).unwrap();
        let s = shrink_to_drawing(&lasso, &img, 0.5);
        assert!(s.cov[(30 * 60 + 30) as usize] >= 0.5, "o buraco cercado pelo desenho fica");
        assert!(s.cov[(30 * 60 + 20) as usize] >= 0.5, "o desenho fica");
        assert!(s.cov[(8 * 60 + 8) as usize] < 0.5, "o vazio em volta sai");
        let b = s.bounds().unwrap();
        assert!(b[0] >= 18 && b[2] <= 43, "{b:?}");
    }

    #[test]
    fn magnetic_lasso_sticks_to_the_edge() {
        // borda vertical em x = 20: os pontos traçados em x = 23 grudam nela
        let mut img = Image::filled(40, 40, [1.0, 1.0, 1.0, 1.0]);
        for y in 0..40u32 {
            for x in 0..20u32 {
                let i = img.idx(x, y);
                img.data[i] = [0.0, 0.0, 0.0, 1.0];
            }
        }
        let e = edge_strength(&img);
        let path = magnetic_path(&e, 40, 40, &[[23.0, 5.0], [23.0, 35.0]], 5.0);
        assert!(path.len() > 10);
        for p in &path {
            assert!((p[0] - 20.0).abs() <= 1.0, "{p:?}");
        }
        // sem borda por perto o ponto não anda
        let far = magnetic_path(&e, 40, 40, &[[35.0, 20.0]], 5.0);
        assert_eq!(far, vec![[35.0, 20.0]]);
    }

    #[test]
    fn enclose_and_paint_unfilled_pick_regions() {
        // dois quadrados de linha lado a lado
        let mut img = Image::new(80, 40);
        for y in 5..35u32 {
            for x in 5..75u32 {
                let border = x < 7 || x > 72 || y < 7 || y > 32 || (38..42).contains(&x);
                if border {
                    let i = img.idx(x, y);
                    img.data[i] = [0.0, 0.0, 0.0, 1.0];
                }
            }
        }
        let b = boundary_from_alpha(&img, 0.5);
        let r = label_regions(&b, 80, 40, &FillParams { gap: 0.0, expand: 0, line_threshold: 0.5 });
        let left = r.label_at(20, 20);
        let right = r.label_at(60, 20);
        let outside = r.label_at(1, 1);
        // laço em volta só do quadrado da esquerda
        let lasso = Selection::from_shape(&crate::select::SelectShape::Rect { rect: [2.0, 2.0, 38.0, 36.0] }, 80, 40, None).unwrap();
        let got = enclosed_labels(&r, &lasso, 0.95);
        assert_eq!(got, vec![left]);
        assert!(!got.contains(&outside) && !got.contains(&right));
        // pincel passando pelos dois miolos
        let mut t = touched_labels(&r, &[[20.0, 20.0], [60.0, 20.0]], 2.0);
        t.sort();
        let mut want = vec![left, right];
        want.sort();
        assert_eq!(t, want);
    }
}
