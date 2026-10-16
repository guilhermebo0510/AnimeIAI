//! Preenchimento de polígonos com cobertura de área exata (acumulação de área com sinal, à maneira do font-rs).
//! Regra não-zero aproximada por |soma| limitada a 1 — suficiente para formas preenchidas de ferramentas e balões.

pub struct PolyCoverage {
    pub x0: i32,
    pub y0: i32,
    pub width: u32,
    pub height: u32,
    pub cov: Vec<f32>,
}

/// `contours`: listas de pontos fechados implicitamente, em pixels do canvas. `aliased` = limiar 0,5.
pub fn fill_polygons(contours: &[Vec<[f32; 2]>], aliased: bool, clip_w: u32, clip_h: u32) -> Option<PolyCoverage> {
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for c in contours {
        for p in c {
            minx = minx.min(p[0]);
            miny = miny.min(p[1]);
            maxx = maxx.max(p[0]);
            maxy = maxy.max(p[1]);
        }
    }
    if minx > maxx {
        return None;
    }
    let x0 = (minx.floor() as i32).max(0);
    let y0 = (miny.floor() as i32).max(0);
    let x1 = (maxx.ceil() as i32 + 1).min(clip_w as i32);
    let y1 = (maxy.ceil() as i32 + 1).min(clip_h as i32);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    // +1 coluna para a acumulação
    let mut acc = vec![0.0f32; (w + 2) * h];
    let stride = w + 2;
    for c in contours {
        if c.len() < 3 {
            continue;
        }
        for i in 0..c.len() {
            let a = c[i];
            let b = c[(i + 1) % c.len()];
            draw_line(&mut acc, stride, h, [a[0] - x0 as f32, a[1] - y0 as f32], [b[0] - x0 as f32, b[1] - y0 as f32]);
        }
    }
    let mut cov = vec![0.0f32; w * h];
    for y in 0..h {
        let mut sum = 0.0f32;
        for x in 0..w {
            sum += acc[y * stride + x];
            let v = sum.abs().min(1.0);
            cov[y * w + x] = if aliased { if v >= 0.5 { 1.0 } else { 0.0 } } else { v };
        }
    }
    Some(PolyCoverage { x0, y0, width: w as u32, height: h as u32, cov })
}

/// Acumula área com sinal da aresta p0→p1 (coordenadas locais).
fn draw_line(acc: &mut [f32], stride: usize, h: usize, p0: [f32; 2], p1: [f32; 2]) {
    if (p0[1] - p1[1]).abs() < f32::EPSILON {
        return;
    }
    let (dir, p0, p1) = if p0[1] < p1[1] { (1.0f32, p0, p1) } else { (-1.0f32, p1, p0) };
    let dxdy = (p1[0] - p0[0]) / (p1[1] - p0[1]);
    let mut x = p0[0];
    let y0 = p0[1].max(0.0);
    if p0[1] < 0.0 {
        x -= p0[1] * dxdy;
    }
    let y_end = p1[1].min(h as f32);
    let mut y = y0;
    while y < y_end {
        let yi = y.floor() as usize;
        let ynext = ((yi + 1) as f32).min(y_end);
        let dy = ynext - y;
        let xnext = x + dxdy * dy;
        let d = dy * dir;
        let (xl, xr) = if x < xnext { (x, xnext) } else { (xnext, x) };
        let row = yi * stride;
        let xl_c = xl.max(0.0);
        let x0i = xl_c.floor() as usize;
        let x0f = xl_c - x0i as f32;
        let xr_c = xr.max(0.0);
        if x0i >= stride - 1 {
            // aresta além da direita da janela: não contribui (fica fora do recorte)
        } else if xr_c.ceil() as usize <= x0i + 1 {
            // cabe numa coluna
            let xmf = 0.5 * (xl_c + xr_c) - x0i as f32;
            acc[row + x0i] += d - d * xmf;
            acc[row + x0i + 1] += d * xmf;
        } else {
            let s = 1.0 / (xr - xl);
            let x0p = (x0f - 1.0).min(0.0).abs(); // parte da 1ª coluna
            let a0 = 0.5 * s * x0p * x0p;
            let x1i = (xr_c.ceil() as usize).min(stride - 1);
            let x1f = xr_c - x1i as f32 + 1.0;
            acc[row + x0i] += d * a0;
            if x1i == x0i + 2 {
                acc[row + x0i + 1] += d * (1.0 - a0 - 0.5 * s * x1f * x1f);
            } else {
                let a1 = s * (1.5 - x0f);
                acc[row + x0i + 1] += d * (a1 - a0);
                for xi in x0i + 2..x1i.saturating_sub(1) {
                    acc[row + xi] += d * s;
                }
                let a2 = a1 + (x1i as f32 - x0i as f32 - 3.0) * s;
                if x1i >= 1 {
                    acc[row + x1i - 1] += d * (1.0 - a2 - 0.5 * s * x1f * x1f);
                }
            }
            if x1i < stride {
                acc[row + x1i] += d * 0.5 * s * x1f * x1f;
            }
        }
        x = xnext;
        y = ynext;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_area_is_exact() {
        let sq = vec![vec![[10.25, 10.0], [30.25, 10.0], [30.25, 30.0], [10.25, 30.0]]];
        let c = fill_polygons(&sq, false, 100, 100).unwrap();
        let area: f32 = c.cov.iter().sum();
        assert!((area - 400.0).abs() < 0.5, "área {area}");
    }

    #[test]
    fn triangle_area_and_aliased() {
        let tri = vec![vec![[0.0, 0.0], [40.0, 0.0], [0.0, 40.0]]];
        let c = fill_polygons(&tri, false, 100, 100).unwrap();
        let area: f32 = c.cov.iter().sum();
        assert!((area - 800.0).abs() < 1.0, "área {area}");
        let ca = fill_polygons(&tri, true, 100, 100).unwrap();
        assert!(ca.cov.iter().all(|&v| v == 0.0 || v == 1.0));
    }
}
