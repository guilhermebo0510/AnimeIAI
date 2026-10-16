//! Texto como contornos (onomatopeias, títulos, créditos, legendas queimadas): glifos de uma fonte OpenType/TrueType
//! (ab_glyph) achatados em polígonos no canvas. Horizontal ou vertical (tategaki simples: um glifo por linha).

use ab_glyph::{Font, FontRef, OutlineCurve, PxScale, ScaleFont};

#[derive(Debug, Clone, PartialEq)]
pub struct TextOutline {
    /// Contornos fechados (regra não-zero: buracos têm orientação oposta).
    pub contours: Vec<Vec<[f32; 2]>>,
    /// Caixa [x0, y0, x1, y1].
    pub bounds: [f32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextOptions {
    pub size: f32,
    pub letter_spacing: f32,
    pub vertical: bool,
    /// Centro do bloco de texto no canvas.
    pub center: [f32; 2],
    /// Rotação (graus, horário) e cisalhamento horizontal (itálico) em torno do centro.
    pub rotation: f32,
    pub skew: f32,
    /// Escala não uniforme (largura, altura).
    pub scale: [f32; 2],
}

/// Bordas irregulares de letreiro feito à mão: desloca os pontos do contorno ao longo da normal por ruído
/// (amplitude em px, comprimento de onda em px). Reamostra antes para a irregularidade não depender dos vértices da fonte.
pub fn roughen(outline: &mut TextOutline, amplitude: f32, wavelength: f32, seed: u32) {
    roughen_contours(&mut outline.contours, amplitude, wavelength, seed);
}

/// Igual a `roughen`, para quaisquer contornos fechados.
pub fn roughen_contours(contours: &mut [Vec<[f32; 2]>], amplitude: f32, wavelength: f32, seed: u32) {
    if amplitude <= 0.0 {
        return;
    }
    let step = (wavelength / 6.0).clamp(0.75, 6.0);
    for c in contours.iter_mut() {
        // reamostra o contorno fechado a cada `step` px
        let n = c.len();
        let mut res = Vec::new();
        for i in 0..n {
            let (a, b) = (c[i], c[(i + 1) % n]);
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let k = (len / step).ceil().max(1.0) as usize;
            for j in 0..k {
                let t = j as f32 / k as f32;
                res.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            }
        }
        let m = res.len();
        if m < 3 {
            continue;
        }
        let mut out = Vec::with_capacity(m);
        let mut s_acc = 0.0f32;
        for i in 0..m {
            let (p0, p1, p2) = (res[(i + m - 1) % m], res[i], res[(i + 1) % m]);
            let (tx, ty) = (p2[0] - p0[0], p2[1] - p0[1]);
            let tl = (tx * tx + ty * ty).sqrt().max(1e-6);
            let (nx, ny) = (-ty / tl, tx / tl);
            if i > 0 {
                s_acc += ((p1[0] - res[i - 1][0]).powi(2) + (p1[1] - res[i - 1][1]).powi(2)).sqrt();
            }
            let u = s_acc / wavelength.max(1.0);
            let d = amplitude * (0.65 * crate::noise::perlin(u, 0.5, seed) + 0.35 * crate::noise::perlin(u * 3.1, 1.7, seed ^ 0x77));
            out.push([p1[0] + nx * d, p1[1] + ny * d]);
        }
        *c = out;
    }
}

fn flatten_quad(out: &mut Vec<[f32; 2]>, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], n: usize) {
    for k in 1..=n {
        let t = k as f32 / n as f32;
        let u = 1.0 - t;
        out.push([u * u * p0[0] + 2.0 * u * t * p1[0] + t * t * p2[0], u * u * p0[1] + 2.0 * u * t * p1[1] + t * t * p2[1]]);
    }
}

fn flatten_cubic(out: &mut Vec<[f32; 2]>, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], n: usize) {
    for k in 1..=n {
        let t = k as f32 / n as f32;
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        out.push([a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0], a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1]]);
    }
}

/// Contornos do texto posicionado.
pub fn text_outline(font_bytes: &[u8], text: &str, opt: &TextOptions) -> Result<TextOutline, String> {
    let font = FontRef::try_from_slice(font_bytes).map_err(|e| format!("fonte inválida: {e}"))?;
    let scale = PxScale::from(opt.size);
    let sf = font.as_scaled(scale);
    let upem = font.units_per_em().unwrap_or(1000.0);
    let k = opt.size / upem;
    let mut contours: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut pen = 0.0f32;
    let chars: Vec<char> = text.chars().collect();
    for ch in &chars {
        let id = font.glyph_id(*ch);
        let adv = if opt.vertical { opt.size } else { sf.h_advance(id) };
        // origem do glifo: horizontal = linha de base em y=0; vertical = centrado em x, caixa de "size" de altura
        let (gx, gy) = if opt.vertical { (-sf.h_advance(id) / 2.0, pen + opt.size * 0.88) } else { (pen, 0.0) };
        if let Some(outline) = font.outline(id) {
            let mut cur: Vec<[f32; 2]> = Vec::new();
            let mut last: Option<[f32; 2]> = None;
            for c in &outline.curves {
                let tp = |p: ab_glyph::Point| [gx + p.x * k, gy - p.y * k];
                let (start, end) = match c {
                    OutlineCurve::Line(a, b) => (tp(*a), tp(*b)),
                    OutlineCurve::Quad(a, _, c2) => (tp(*a), tp(*c2)),
                    OutlineCurve::Cubic(a, _, _, d) => (tp(*a), tp(*d)),
                };
                let continuous = last.is_some_and(|l| (l[0] - start[0]).abs() < 1e-3 && (l[1] - start[1]).abs() < 1e-3);
                if !continuous {
                    if cur.len() >= 3 {
                        contours.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(start);
                }
                match c {
                    OutlineCurve::Line(_, _) => cur.push(end),
                    OutlineCurve::Quad(a, b, c2) => flatten_quad(&mut cur, tp(*a), tp(*b), tp(*c2), 8),
                    OutlineCurve::Cubic(a, b, c2, d) => flatten_cubic(&mut cur, tp(*a), tp(*b), tp(*c2), tp(*d), 10),
                }
                last = Some(end);
            }
            if cur.len() >= 3 {
                contours.push(cur);
            }
        }
        pen += adv + opt.letter_spacing;
    }
    if contours.is_empty() {
        return Err("texto sem glifos desenháveis nesta fonte".into());
    }
    // centraliza o bloco e aplica escala, cisalhamento e rotação em torno do centro
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for c in &contours {
        for p in c {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
    }
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (sn, cs) = opt.rotation.to_radians().sin_cos();
    let (mut bx0, mut by0, mut bx1, mut by1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for c in &mut contours {
        for p in c.iter_mut() {
            let (mut x, y) = ((p[0] - cx) * opt.scale[0], (p[1] - cy) * opt.scale[1]);
            x += -y * opt.skew;
            let (rx, ry) = (x * cs - y * sn, x * sn + y * cs);
            *p = [opt.center[0] + rx, opt.center[1] + ry];
            bx0 = bx0.min(p[0]);
            by0 = by0.min(p[1]);
            bx1 = bx1.max(p[0]);
            by1 = by1.max(p[1]);
        }
    }
    Ok(TextOutline { contours, bounds: [bx0, by0, bx1, by1] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_advance_the_pen() {
        let font = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../aiai-studio/assets/fonts/NotoSansJP-Regular.otf")).unwrap();
        let opt = TextOptions { size: 40.0, letter_spacing: 0.0, vertical: false, center: [200.0, 50.0], rotation: 0.0, skew: 0.0, scale: [1.0, 1.0] };
        let a = text_outline(&font, "ab", &opt).unwrap();
        let b = text_outline(&font, "a b", &opt).unwrap();
        let wa = a.bounds[2] - a.bounds[0];
        let wb = b.bounds[2] - b.bounds[0];
        assert!(wb > wa + 6.0, "espaço avança a caneta: {wa} vs {wb}");
    }
}
