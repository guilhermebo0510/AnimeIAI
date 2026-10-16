//! Ajustes de cor (camadas de ajuste e operações diretas): níveis, curvas, matiz/saturação/luminosidade, equilíbrio
//! de cor, brilho/contraste, mapa de degradê, inverter, limiar, posterizar. Operam em sRGB não pré-multiplicado.

use crate::image::Image;
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Adjustment {
    /// Níveis: entrada preto/branco, gama, saída preto/branco (0..1).
    Levels { in_black: f32, in_white: f32, gamma: f32, out_black: f32, out_white: f32 },
    /// Curvas: pontos (x, y) 0..1 para RGB (todos) e opcionais por canal.
    Curves {
        rgb: Vec<[f32; 2]>,
        #[serde(default)]
        r: Vec<[f32; 2]>,
        #[serde(default)]
        g: Vec<[f32; 2]>,
        #[serde(default)]
        b: Vec<[f32; 2]>,
    },
    /// Matiz (graus), saturação e luminosidade (-1..1).
    HueSaturation { hue: f32, saturation: f32, lightness: f32 },
    /// Equilíbrio de cor: deslocamento RGB em sombras, meios-tons e realces (-1..1).
    ColorBalance { shadows: [f32; 3], midtones: [f32; 3], highlights: [f32; 3] },
    BrightnessContrast { brightness: f32, contrast: f32 },
    /// Mapa de degradê: luminância → cor (paradas t, "#rrggbb").
    GradientMap { stops: Vec<(f32, [f32; 3])> },
    Invert,
    Threshold { level: f32 },
    Posterize { levels: u32 },
}

fn curve_eval(points: &[[f32; 2]], x: f32) -> f32 {
    if points.len() < 2 {
        return x;
    }
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    if x <= pts[0][0] {
        return pts[0][1];
    }
    if x >= pts[pts.len() - 1][0] {
        return pts[pts.len() - 1][1];
    }
    // spline monotônica de Fritsch–Carlson (sem overshoot, como as curvas do Photoshop)
    let n = pts.len();
    let mut d = vec![0.0f32; n - 1];
    for i in 0..n - 1 {
        d[i] = (pts[i + 1][1] - pts[i][1]) / (pts[i + 1][0] - pts[i][0]).max(1e-6);
    }
    let mut m = vec![0.0f32; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        m[i] = if d[i - 1] * d[i] <= 0.0 { 0.0 } else { (d[i - 1] + d[i]) / 2.0 };
    }
    for i in 0..n - 1 {
        if d[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
        } else {
            let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                m[i] = t * a * d[i];
                m[i + 1] = t * b * d[i];
            }
        }
    }
    let i = (0..n - 1).find(|&i| x <= pts[i + 1][0]).unwrap_or(n - 2);
    let h = pts[i + 1][0] - pts[i][0];
    let t = (x - pts[i][0]) / h;
    let (t2, t3) = (t * t, t * t * t);
    let (h00, h10, h01, h11) = (2.0 * t3 - 3.0 * t2 + 1.0, t3 - 2.0 * t2 + t, -2.0 * t3 + 3.0 * t2, t3 - t2);
    (h00 * pts[i][1] + h10 * h * m[i] + h01 * pts[i + 1][1] + h11 * h * m[i + 1]).clamp(0.0, 1.0)
}

fn rgb_to_hsl(c: [f32; 3]) -> [f32; 3] {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    let l = (mx + mn) / 2.0;
    if mx == mn {
        return [0.0, 0.0, l];
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == c[0] {
        ((c[1] - c[2]) / d + if c[1] < c[2] { 6.0 } else { 0.0 }) / 6.0
    } else if mx == c[1] {
        ((c[2] - c[0]) / d + 2.0) / 6.0
    } else {
        ((c[0] - c[1]) / d + 4.0) / 6.0
    };
    [h, s, l]
}

fn hsl_to_rgb(c: [f32; 3]) -> [f32; 3] {
    let (h, s, l) = (c[0].rem_euclid(1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0));
    if s == 0.0 {
        return [l, l, l];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}

impl Adjustment {
    pub fn apply_rgb(&self, c: [f32; 3]) -> [f32; 3] {
        match self {
            Adjustment::Levels { in_black, in_white, gamma, out_black, out_white } => c.map(|v| {
                let t = ((v - in_black) / (in_white - in_black).max(1e-6)).clamp(0.0, 1.0).powf(1.0 / gamma.max(0.01));
                out_black + (out_white - out_black) * t
            }),
            Adjustment::Curves { rgb, r, g, b } => {
                let base = [curve_eval(rgb, c[0]), curve_eval(rgb, c[1]), curve_eval(rgb, c[2])];
                [curve_eval(r, base[0]), curve_eval(g, base[1]), curve_eval(b, base[2])]
            }
            Adjustment::HueSaturation { hue, saturation, lightness } => {
                let mut hsl = rgb_to_hsl(c);
                hsl[0] += hue / 360.0;
                hsl[1] = (hsl[1] * (1.0 + saturation)).clamp(0.0, 1.0);
                hsl[2] = if *lightness >= 0.0 { hsl[2] + (1.0 - hsl[2]) * lightness } else { hsl[2] * (1.0 + lightness) };
                hsl_to_rgb(hsl)
            }
            Adjustment::ColorBalance { shadows, midtones, highlights } => {
                let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
                let ws = (1.0 - l * 2.0).clamp(0.0, 1.0);
                let wh = (l * 2.0 - 1.0).clamp(0.0, 1.0);
                let wm = 1.0 - ws - wh;
                let mut o = c;
                for k in 0..3 {
                    o[k] = (c[k] + 0.3 * (shadows[k] * ws + midtones[k] * wm + highlights[k] * wh)).clamp(0.0, 1.0);
                }
                o
            }
            Adjustment::BrightnessContrast { brightness, contrast } => {
                let k = (1.0 + contrast).max(0.0);
                c.map(|v| ((v - 0.5) * k + 0.5 + brightness * 0.5).clamp(0.0, 1.0))
            }
            Adjustment::GradientMap { stops } => {
                let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
                if stops.is_empty() {
                    return c;
                }
                if l <= stops[0].0 {
                    return stops[0].1;
                }
                for w in stops.windows(2) {
                    if l <= w[1].0 {
                        let f = (l - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
                        return [0, 1, 2].map(|k| w[0].1[k] + (w[1].1[k] - w[0].1[k]) * f);
                    }
                }
                stops[stops.len() - 1].1
            }
            Adjustment::Invert => c.map(|v| 1.0 - v),
            Adjustment::Threshold { level } => {
                let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
                if l >= *level { [1.0; 3] } else { [0.0; 3] }
            }
            Adjustment::Posterize { levels } => {
                let n = (*levels).max(2) as f32 - 1.0;
                c.map(|v| (v * n).round() / n)
            }
        }
    }

    /// Aplica em toda a imagem (pré-multiplicada), com intensidade `amount` e máscara opcional por pixel.
    pub fn apply_image(&self, img: &mut Image, amount: f32, mask: Option<&[f32]>) {
        img.data.par_iter_mut().enumerate().for_each(|(i, p)| {
            if p[3] <= 0.0 {
                return;
            }
            let k = amount * mask.map_or(1.0, |m| m[i]);
            if k <= 0.0 {
                return;
            }
            let un = [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
            let adj = self.apply_rgb(un);
            for c in 0..3 {
                p[c] = (un[c] + (adj[c] - un[c]) * k) * p[3];
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_like_settings_do_nothing_and_curves_are_monotone() {
        let c = [0.2, 0.5, 0.9];
        let lv = Adjustment::Levels { in_black: 0.0, in_white: 1.0, gamma: 1.0, out_black: 0.0, out_white: 1.0 };
        assert!(lv.apply_rgb(c).iter().zip(c).all(|(a, b)| (a - b).abs() < 1e-6));
        let hs = Adjustment::HueSaturation { hue: 0.0, saturation: 0.0, lightness: 0.0 };
        assert!(hs.apply_rgb(c).iter().zip(c).all(|(a, b)| (a - b).abs() < 1e-5));
        let pts = vec![[0.0, 0.0], [0.3, 0.5], [0.7, 0.6], [1.0, 1.0]];
        let mut prev = -1.0;
        for i in 0..=100 {
            let v = curve_eval(&pts, i as f32 / 100.0);
            assert!(v >= prev - 1e-6, "curva monotônica");
            prev = v;
        }
        assert_eq!(Adjustment::Invert.apply_rgb([0.25, 0.5, 1.0]), [0.75, 0.5, 0.0]);
    }
}
