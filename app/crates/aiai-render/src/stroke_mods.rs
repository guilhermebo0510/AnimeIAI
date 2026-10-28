//! Modificadores de traço da camada vetorial (auditoria 4.7h), aplicados só no render, quadro a quadro, sobre uma cópia
//! dos traços (o desenho guardado não muda):
//!
//! - `boil` (line boil): a linha treme um pouco a cada `every` quadros, como desenhos refeitos à mão;
//! - `wind` (vento): o traço balança, mais na ponta que na raiz (cabelo, capim, fita);
//! - `mirror` (espelho): uma cópia espelhada dos traços em volta de um eixo.
//!
//! Tudo determinístico (mesmo quadro = mesmo resultado), lido de `params.stroke_mods` da camada.

use aiai_core::{Id, Stroke};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StrokeMods {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boil: Option<Boil>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind: Option<Wind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<Mirror>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Boil {
    /// Deslocamento máximo (px do desenho).
    pub amount: f32,
    /// Quadros entre uma tremida e a próxima (2 = "nos dois", como a animação limitada).
    #[serde(default = "two")]
    pub every: u32,
    /// Distância (px) entre as ondulações ao longo da linha.
    #[serde(default = "forty")]
    pub scale: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wind {
    /// Quanto a ponta do traço anda (px).
    pub amount: f32,
    /// Balanços por segundo.
    #[serde(default = "one")]
    pub speed: f32,
    /// Direção do empurrão em graus (0 = direita, 90 = baixo).
    #[serde(default)]
    pub angle_deg: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mirror {
    /// false = espelha da esquerda para a direita (eixo vertical em x = `center`); true = de cima para baixo.
    #[serde(default)]
    pub vertical: bool,
    pub center: f32,
}

fn two() -> u32 {
    2
}
fn forty() -> f32 {
    40.0
}
fn one() -> f32 {
    1.0
}

impl StrokeMods {
    pub fn from_params(params: &serde_json::Value) -> Option<StrokeMods> {
        let m: StrokeMods = serde_json::from_value(params.get("stroke_mods")?.clone()).ok()?;
        (m.boil.is_some() || m.wind.is_some() || m.mirror.is_some()).then_some(m)
    }
}

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn rnd(seed: u64, i: i64) -> f32 {
    (mix(seed ^ (i as u64).wrapping_mul(0x2545_F491_4F6C_DD1D)) >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
}

/// Ruído 1D suave em [-1, 1] (valores sorteados nos inteiros, smoothstep entre eles).
fn noise(seed: u64, x: f32) -> f32 {
    let i = x.floor();
    let f = x - i;
    let u = f * f * (3.0 - 2.0 * f);
    let (a, b) = (rnd(seed, i as i64), rnd(seed, i as i64 + 1));
    a + (b - a) * u
}

fn id_seed(id: &Id, k: usize) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in id.0.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    mix(h ^ k as u64)
}

/// Comprimento acumulado até cada ponto e o total.
fn arc(s: &Stroke) -> (Vec<f32>, f32) {
    let mut acc = 0.0;
    let mut v = Vec::with_capacity(s.points.len());
    for (i, p) in s.points.iter().enumerate() {
        if i > 0 {
            let q = &s.points[i - 1];
            acc += (p.x - q.x).hypot(p.y - q.y);
        }
        v.push(acc);
    }
    (v, acc)
}

/// Os traços do quadro `frame` com os modificadores aplicados.
pub fn apply(strokes: &[Stroke], m: &StrokeMods, frame: u32, fps: f64) -> Vec<Stroke> {
    let mut out: Vec<Stroke> = strokes.to_vec();
    if let Some(b) = &m.boil {
        let key = (frame / b.every.max(1)) as u64;
        let scale = b.scale.max(1.0);
        for (k, s) in out.iter_mut().enumerate() {
            let seed = id_seed(&s.id, k) ^ mix(key);
            let (cum, _) = arc(s);
            for (p, d) in s.points.iter_mut().zip(cum) {
                p.x += b.amount * noise(seed, d / scale);
                p.y += b.amount * noise(seed ^ 0x5555, d / scale + 17.3);
            }
        }
    }
    if let Some(w) = &m.wind {
        let t = frame as f64 / fps.max(1.0);
        let (dx, dy) = (w.angle_deg.to_radians().cos(), w.angle_deg.to_radians().sin());
        for (k, s) in out.iter_mut().enumerate() {
            let (cum, total) = arc(s);
            if total <= 0.0 {
                continue;
            }
            let phase = (id_seed(&s.id, k) >> 40) as f64 / (1u64 << 24) as f64 * std::f64::consts::TAU;
            for (p, d) in s.points.iter_mut().zip(cum) {
                let u = d / total;
                // a onda sobe do pé para a ponta: a ponta atrasa um pouco em relação à raiz
                let a = (std::f64::consts::TAU * (w.speed as f64 * t) + phase - u as f64 * 1.2).sin() as f32;
                let push = w.amount * u.powf(1.5) * (0.6 + 0.4 * a);
                p.x += dx * push;
                p.y += dy * push;
            }
        }
    }
    if let Some(mi) = &m.mirror {
        let copies: Vec<Stroke> = out
            .iter()
            .map(|s| {
                let mut c = s.clone();
                for p in c.points.iter_mut() {
                    if mi.vertical {
                        p.y = 2.0 * mi.center - p.y;
                    } else {
                        p.x = 2.0 * mi.center - p.x;
                    }
                }
                c
            })
            .collect();
        out.extend(copies);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_core::*;

    fn line() -> Stroke {
        Stroke {
            id: Id::from("s"),
            points: (0..=20).map(|i| StrokePoint { x: i as f32 * 10.0, y: 50.0, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
            geometry: StrokeGeometry::Polyline,
            tension: 0.0,
            tip: VectorTip::Marker,
            width: 3.0,
            profile: WidthProfile::default(),
            color: ColorRef::default(),
            opacity: 1.0,
            closed: false,
            aliased: false,
            color_trace: false,
            group: String::new(),
        }
    }

    #[test]
    fn boil_changes_every_n_frames_and_is_deterministic() {
        let m = StrokeMods { boil: Some(Boil { amount: 2.0, every: 2, scale: 40.0 }), ..Default::default() };
        let s = [line()];
        let (f0, f1, f2) = (apply(&s, &m, 0, 24.0), apply(&s, &m, 1, 24.0), apply(&s, &m, 2, 24.0));
        assert_eq!(f0, f1, "nos dois: o quadro 1 repete o 0");
        assert_ne!(f0, f2);
        assert_eq!(f0, apply(&s, &m, 0, 24.0), "determinístico");
        assert!(f2[0].points.iter().all(|p| (p.y - 50.0).abs() <= 2.0 + 1e-4));
    }

    #[test]
    fn wind_moves_the_tip_more_than_the_root_and_mirror_adds_copies() {
        let m = StrokeMods { wind: Some(Wind { amount: 20.0, speed: 1.0, angle_deg: 90.0 }), ..Default::default() };
        let r = apply(&[line()], &m, 6, 24.0);
        let p = &r[0].points;
        assert!((p[0].y - 50.0).abs() < 1e-4, "a raiz não anda");
        assert!((p[20].y - 50.0).abs() > 5.0, "a ponta anda: {}", p[20].y);
        let m = StrokeMods { mirror: Some(Mirror { vertical: false, center: 0.0 }), ..Default::default() };
        let r = apply(&[line()], &m, 0, 24.0);
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].points[20].x, -200.0);
    }
}
