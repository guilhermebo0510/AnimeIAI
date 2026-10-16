//! Rasterização de traço por campo de distância: o traço é a união de "cápsulas desiguais" (dois círculos de raios
//! diferentes e o casco entre eles) entre amostras consecutivas. Cada cápsula grava a distância mínima só nos pixels
//! da sua caixa; a cobertura sai da distância (filtro de 1 px para antisserrilhado, ou limiar para linha de cel).
//! Junções e pontas redondas saem certas por construção, e o resultado é determinístico.

use crate::curve::StrokeSample;

/// Cobertura de um traço numa janela retangular do canvas.
pub struct StrokeCoverage {
    pub x0: i32,
    pub y0: i32,
    pub width: u32,
    pub height: u32,
    /// 0..1 por pixel
    pub cov: Vec<f32>,
}

/// Distância com sinal até a cápsula desigual (a, ra)–(b, rb). Fórmula de I. Quilez (sdUnevenCapsule), geral no plano.
#[inline]
pub fn sd_uneven_capsule(px: f32, py: f32, a: StrokeSample, b: StrokeSample) -> f32 {
    let (bax, bay) = (b.x - a.x, b.y - a.y);
    let h = (bax * bax + bay * bay).sqrt();
    let (qx, qy) = (px - a.x, py - a.y);
    if h < 1e-5 || (a.r - b.r).abs() >= h {
        // um círculo contém o outro (ou são o mesmo ponto)
        let da = (qx * qx + qy * qy).sqrt() - a.r;
        let db = ((px - b.x).powi(2) + (py - b.y).powi(2)).sqrt() - b.r;
        return da.min(db);
    }
    let (dx, dy) = (bax / h, bay / h);
    let y = qx * dx + qy * dy; // ao longo do eixo
    let x = (qx * dy - qy * dx).abs(); // distância ao eixo
    let k_b = (a.r - b.r) / h;
    let k_a = (1.0 - k_b * k_b).sqrt();
    let k = -k_b * x + k_a * y;
    if k < 0.0 {
        return (x * x + y * y).sqrt() - a.r;
    }
    if k > k_a * h {
        return (x * x + (y - h) * (y - h)).sqrt() - b.r;
    }
    x * k_a + y * k_b - a.r
}

/// Rasteriza as amostras, recortado em [0, clip_w) × [0, clip_h) do canvas. `aliased` = cobertura 0/1.
pub fn rasterize_stroke(samples: &[StrokeSample], aliased: bool, clip_w: u32, clip_h: u32) -> Option<StrokeCoverage> {
    if samples.is_empty() {
        return None;
    }
    let margin = 1.5;
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for s in samples {
        minx = minx.min(s.x - s.r);
        miny = miny.min(s.y - s.r);
        maxx = maxx.max(s.x + s.r);
        maxy = maxy.max(s.y + s.r);
    }
    let x0 = ((minx - margin).floor() as i32).max(0);
    let y0 = ((miny - margin).floor() as i32).max(0);
    let x1 = ((maxx + margin).ceil() as i32).min(clip_w as i32);
    let y1 = ((maxy + margin).ceil() as i32).min(clip_h as i32);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut dist = vec![f32::INFINITY; w * h];
    let pairs: Box<dyn Iterator<Item = (StrokeSample, StrokeSample)>> = if samples.len() == 1 {
        Box::new(std::iter::once((samples[0], samples[0])))
    } else {
        Box::new(samples.windows(2).map(|p| (p[0], p[1])))
    };
    for (a, b) in pairs {
        let r = a.r.max(b.r) + margin;
        let bx0 = ((a.x.min(b.x) - r).floor() as i32).max(x0);
        let by0 = ((a.y.min(b.y) - r).floor() as i32).max(y0);
        let bx1 = ((a.x.max(b.x) + r).ceil() as i32).min(x1);
        let by1 = ((a.y.max(b.y) + r).ceil() as i32).min(y1);
        for y in by0..by1 {
            let row = (y - y0) as usize * w;
            let py = y as f32 + 0.5;
            for x in bx0..bx1 {
                let d = sd_uneven_capsule(x as f32 + 0.5, py, a, b);
                let slot = &mut dist[row + (x - x0) as usize];
                if d < *slot {
                    *slot = d;
                }
            }
        }
    }
    let cov = dist
        .into_iter()
        .map(|d| if aliased { if d <= 0.0 { 1.0 } else { 0.0 } } else { (0.5 - d).clamp(0.0, 1.0) })
        .collect();
    Some(StrokeCoverage { x0, y0, width: w as u32, height: h as u32, cov })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: f32, y: f32, r: f32) -> StrokeSample {
        StrokeSample { x, y, r }
    }

    #[test]
    fn capsule_distance_basics() {
        let (a, b) = (s(0.0, 0.0, 2.0), s(10.0, 0.0, 2.0));
        assert!((sd_uneven_capsule(5.0, 0.0, a, b) + 2.0).abs() < 1e-5);
        assert!((sd_uneven_capsule(5.0, 5.0, a, b) - 3.0).abs() < 1e-5);
        assert!((sd_uneven_capsule(-5.0, 0.0, a, b) - 3.0).abs() < 1e-5);
        let (c, d) = (s(0.0, 0.0, 1.0), s(10.0, 0.0, 3.0));
        assert!(sd_uneven_capsule(10.0, 2.9, c, d) < 0.0, "dentro do círculo maior");
        assert!(sd_uneven_capsule(0.0, 1.1, c, d) > 0.0, "fora do círculo menor");
    }

    #[test]
    fn coverage_of_horizontal_line_has_expected_width() {
        let smp: Vec<StrokeSample> = (0..=100).map(|i| s(10.0 + i as f32, 20.0, 3.0)).collect();
        let c = rasterize_stroke(&smp, false, 200, 50).unwrap();
        let col = (60 - c.x0) as usize;
        let sum: f32 = (0..c.height as usize).map(|r| c.cov[r * c.width as usize + col]).sum();
        assert!((sum - 6.0).abs() < 0.05, "largura integrada ≈ 2r: {sum}");
        let ca = rasterize_stroke(&smp, true, 200, 50).unwrap();
        assert!(ca.cov.iter().all(|&v| v == 0.0 || v == 1.0), "linha de cel sem cinzas");
    }
}
