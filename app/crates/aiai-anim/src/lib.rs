//! Intercalação vetorial automática (動画 / inbetween), no espírito do CACANi (comportamento estudado, implementação
//! própria):
//!
//! 1. **Correspondência** de traços entre as chaves A e B: por ordem quando B é uma cópia editada de A (mesma
//!    quantidade de traços e de pontos), senão por custo de forma/posição (guloso com limiar), com orientação e
//!    deslocamento cíclico escolhidos para traços fechados. Pares manuais sobrescrevem.
//! 2. **Movimento**: decomposição de Procrustes (rotação + escala + translação) global da pose A→B, mais o resíduo de
//!    cada ponto. Interpolar ângulo e escala em vez de coordenadas evita o "encolhimento" da interpolação linear em
//!    rotações (cabeça virando, braço girando). `bend` curva a trajetória do centro em arco.
//! 3. **Spacing**: os tempos t de cada intercalação vêm de uma tabela (gráfico de espaçamento do genga) ou de easing.
//! 4. Traços sem par aparecem/somem na metade do caminho.

pub mod particles;

use aiai_core::{ColorRef, Ease, Id, Stroke, StrokeGeometry, StrokePoint};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum MotionMode {
    /// Uma rotação/escala para o desenho todo + resíduo por ponto (padrão; mantém o desenho coeso).
    #[default]
    Global,
    /// Rotação/escala por traço (partes que giram independentes).
    PerStroke,
    /// Interpolação linear simples dos pontos.
    Linear,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InbetweenOptions {
    #[serde(default)]
    pub motion: MotionMode,
    /// Curvatura da trajetória do centro (fração da distância percorrida; + = para a esquerda do movimento).
    #[serde(default)]
    pub bend: f32,
    /// Pares manuais (índice em A, índice em B) que sobrescrevem a correspondência automática.
    #[serde(default)]
    pub pairs: Vec<(usize, usize)>,
    /// Custo máximo (px médios) para casar automaticamente dois traços.
    #[serde(default = "default_max_cost")]
    pub max_cost: f32,
}

fn default_max_cost() -> f32 {
    200.0
}

impl Default for InbetweenOptions {
    fn default() -> Self {
        InbetweenOptions { motion: MotionMode::Global, bend: 0.0, pairs: Vec::new(), max_cost: default_max_cost() }
    }
}

/// Par de correspondência (A, B); `None` = traço sem par.
pub type Pair = (Option<usize>, Option<usize>);

#[derive(Clone, Copy, Debug)]
struct P {
    x: f32,
    y: f32,
    p: f32,
}

/// Reamostra os pontos de controle do traço em `n` pontos igualmente espaçados pelo comprimento da polilinha.
fn resample(s: &Stroke, n: usize) -> Vec<P> {
    let pts: Vec<P> = s.points.iter().map(|q| P { x: q.x, y: q.y, p: q.pressure }).collect();
    let n = n.max(2);
    if pts.len() == 1 {
        return vec![pts[0]; n];
    }
    let mut cum = vec![0.0f32];
    let closed_extra = if s.closed { 1 } else { 0 };
    let seq: Vec<P> = pts.iter().copied().chain(if s.closed { Some(pts[0]) } else { None }).collect();
    for w in seq.windows(2) {
        let d = ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt();
        cum.push(cum.last().expect("não vazio") + d);
    }
    let total = *cum.last().expect("não vazio");
    let _ = closed_extra;
    let count = if s.closed { n } else { n };
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let target = if s.closed { total * k as f32 / count as f32 } else { total * k as f32 / (count - 1) as f32 };
        let i = cum.partition_point(|&c| c < target).clamp(1, seq.len() - 1);
        let (a, b) = (seq[i - 1], seq[i]);
        let seg = (cum[i] - cum[i - 1]).max(1e-6);
        let f = ((target - cum[i - 1]) / seg).clamp(0.0, 1.0);
        out.push(P { x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f, p: a.p + (b.p - a.p) * f });
    }
    out
}

fn mean_dist(a: &[P], b: &[P]) -> f32 {
    a.iter().zip(b).map(|(p, q)| ((p.x - q.x).powi(2) + (p.y - q.y).powi(2)).sqrt()).sum::<f32>() / a.len().max(1) as f32
}

/// Alinha `b` a `a` (mesmo tamanho): inverte e/ou gira ciclicamente (fechados) para minimizar a distância.
fn align(a: &[P], b: &[P], closed: bool) -> Vec<P> {
    let mut best = b.to_vec();
    let mut best_d = mean_dist(a, b);
    let rev: Vec<P> = b.iter().rev().copied().collect();
    let candidates: Vec<Vec<P>> = if closed {
        let n = b.len();
        let mut c = Vec::new();
        for base in [b.to_vec(), rev] {
            for sh in 0..n {
                c.push((0..n).map(|i| base[(i + sh) % n]).collect());
            }
        }
        c
    } else {
        vec![rev]
    };
    for c in candidates {
        let d = mean_dist(a, &c);
        if d < best_d {
            best_d = d;
            best = c;
        }
    }
    best
}

const SHAPE_N: usize = 24;

/// Correspondência automática entre os traços de A e B.
pub fn correspond(a: &[Stroke], b: &[Stroke], opts: &InbetweenOptions) -> Vec<Pair> {
    let mut used_a = vec![false; a.len()];
    let mut used_b = vec![false; b.len()];
    let mut pairs: Vec<Pair> = Vec::new();
    for &(i, j) in &opts.pairs {
        if i < a.len() && j < b.len() && !used_a[i] && !used_b[j] {
            used_a[i] = true;
            used_b[j] = true;
            pairs.push((Some(i), Some(j)));
        }
    }
    let same_structure = a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.points.len() == y.points.len() && x.closed == y.closed);
    if same_structure {
        for i in 0..a.len() {
            if !used_a[i] && !used_b[i] {
                used_a[i] = true;
                used_b[i] = true;
                pairs.push((Some(i), Some(i)));
            }
        }
    } else {
        let ra: Vec<Vec<P>> = a.iter().map(|s| resample(s, SHAPE_N)).collect();
        let rb: Vec<Vec<P>> = b.iter().map(|s| resample(s, SHAPE_N)).collect();
        let mut costs = Vec::new();
        for i in 0..a.len() {
            for j in 0..b.len() {
                if used_a[i] || used_b[j] || a[i].closed != b[j].closed {
                    continue;
                }
                let aligned = align(&ra[i], &rb[j], a[i].closed);
                costs.push((mean_dist(&ra[i], &aligned), i, j));
            }
        }
        costs.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
        for (c, i, j) in costs {
            if c > opts.max_cost {
                break;
            }
            if !used_a[i] && !used_b[j] {
                used_a[i] = true;
                used_b[j] = true;
                pairs.push((Some(i), Some(j)));
            }
        }
    }
    for (i, u) in used_a.iter().enumerate() {
        if !u {
            pairs.push((Some(i), None));
        }
    }
    for (j, u) in used_b.iter().enumerate() {
        if !u {
            pairs.push((None, Some(j)));
        }
    }
    // ordem de desenho: segue A, depois os novos de B
    pairs.sort_by_key(|p| (p.0.unwrap_or(usize::MAX), p.1.unwrap_or(usize::MAX)));
    pairs
}

/// Procrustes 2D: ângulo, escala e centros que melhor levam `p` a `q`.
fn procrustes(p: &[P], q: &[P]) -> (f32, f32, [f32; 2], [f32; 2]) {
    let n = p.len().max(1) as f32;
    let cp = [p.iter().map(|v| v.x).sum::<f32>() / n, p.iter().map(|v| v.y).sum::<f32>() / n];
    let cq = [q.iter().map(|v| v.x).sum::<f32>() / n, q.iter().map(|v| v.y).sum::<f32>() / n];
    let (mut sxx, mut sxy, mut np, mut nq) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for (a, b) in p.iter().zip(q) {
        let (ax, ay, bx, by) = (a.x - cp[0], a.y - cp[1], b.x - cq[0], b.y - cq[1]);
        sxx += ax * bx + ay * by;
        sxy += ax * by - ay * bx;
        np += ax * ax + ay * ay;
        nq += bx * bx + by * by;
    }
    let angle = sxy.atan2(sxx);
    let scale = if np > 1e-6 { (nq / np).sqrt() } else { 1.0 };
    (angle, scale, cp, cq)
}

/// Pontos interpolados de pares já alinhados, com o movimento escolhido.
fn interpolate_points(pa: &[P], pb: &[P], t: f32, rigid: Option<(f32, f32, [f32; 2], [f32; 2])>, bend: f32) -> Vec<P> {
    match rigid {
        None => pa.iter().zip(pb).map(|(a, b)| P { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, p: a.p + (b.p - a.p) * t }).collect(),
        Some((ang, sc, ca, cb)) => {
            let (s1, c1) = ang.sin_cos();
            let (st, ct) = (ang * t).sin_cos();
            let sct = sc.powf(t);
            // trajetória do centro: reta + arco
            let (dx, dy) = (cb[0] - ca[0], cb[1] - ca[1]);
            let off = bend * (std::f32::consts::PI * t).sin();
            let center = [ca[0] + dx * t - dy * off, ca[1] + dy * t + dx * off];
            pa.iter()
                .zip(pb)
                .map(|(a, b)| {
                    let (ax, ay) = (a.x - ca[0], a.y - ca[1]);
                    // onde o movimento rígido completo levaria a
                    let rx1 = sc * (c1 * ax - s1 * ay) + cb[0];
                    let ry1 = sc * (s1 * ax + c1 * ay) + cb[1];
                    let (resx, resy) = (b.x - rx1, b.y - ry1);
                    let rx = sct * (ct * ax - st * ay) + center[0] + resx * t;
                    let ry = sct * (st * ax + ct * ay) + center[1] + resy * t;
                    P { x: rx, y: ry, p: a.p + (b.p - a.p) * t }
                })
                .collect()
        }
    }
}

fn lerp_color(a: &ColorRef, b: &ColorRef, t: f32) -> ColorRef {
    match (a, b) {
        (ColorRef::Direct { rgba: x }, ColorRef::Direct { rgba: y }) => {
            let mut c = x.0;
            for k in 0..4 {
                c[k] += (y.0[k] - c[k]) * t;
            }
            ColorRef::Direct { rgba: aiai_core::Rgba(c) }
        }
        _ => if t < 0.5 { a.clone() } else { b.clone() },
    }
}

/// Desenho intercalado no tempo `t` (0 = A, 1 = B).
pub fn inbetween(a: &[Stroke], b: &[Stroke], t: f32, opts: &InbetweenOptions) -> Vec<Stroke> {
    let pairs = correspond(a, b, opts);
    // pontos reamostrados e alinhados de cada par
    struct Prep<'s> {
        sa: &'s Stroke,
        sb: &'s Stroke,
        pa: Vec<P>,
        pb: Vec<P>,
        direct: bool,
    }
    let mut preps: Vec<Option<Prep>> = Vec::new();
    for &(ia, ib) in &pairs {
        match (ia, ib) {
            (Some(i), Some(j)) => {
                let (sa, sb) = (&a[i], &b[j]);
                let direct = sa.points.len() == sb.points.len() && sa.geometry == sb.geometry && sa.closed == sb.closed;
                let (pa, pb) = if direct {
                    let pa: Vec<P> = sa.points.iter().map(|q| P { x: q.x, y: q.y, p: q.pressure }).collect();
                    let pb: Vec<P> = sb.points.iter().map(|q| P { x: q.x, y: q.y, p: q.pressure }).collect();
                    (pa, pb)
                } else {
                    let n = sa.points.len().max(sb.points.len()).max(8) * 2;
                    let pa = resample(sa, n);
                    let pb = align(&pa, &resample(sb, n), sa.closed);
                    (pa, pb)
                };
                preps.push(Some(Prep { sa, sb, pa, pb, direct }));
            }
            _ => preps.push(None),
        }
    }
    let global = if opts.motion == MotionMode::Global {
        let all_a: Vec<P> = preps.iter().flatten().flat_map(|p| p.pa.clone()).collect();
        let all_b: Vec<P> = preps.iter().flatten().flat_map(|p| p.pb.clone()).collect();
        (!all_a.is_empty()).then(|| procrustes(&all_a, &all_b))
    } else {
        None
    };
    let mut out = Vec::new();
    for (k, &(ia, ib)) in pairs.iter().enumerate() {
        match (&preps[k], ia, ib) {
            (Some(pr), _, _) => {
                let rigid = match opts.motion {
                    MotionMode::Global => global,
                    MotionMode::PerStroke => Some(procrustes(&pr.pa, &pr.pb)),
                    MotionMode::Linear => None,
                };
                let pts = interpolate_points(&pr.pa, &pr.pb, t, rigid, opts.bend);
                let mut s = pr.sa.clone();
                s.id = Id(String::new());
                s.points = pts.iter().map(|q| StrokePoint { x: q.x, y: q.y, pressure: q.p.clamp(0.0, 1.0), tilt: None, t: None }).collect();
                if !pr.direct {
                    s.geometry = StrokeGeometry::Spline;
                }
                s.width = pr.sa.width + (pr.sb.width - pr.sa.width) * t;
                s.opacity = pr.sa.opacity + (pr.sb.opacity - pr.sa.opacity) * t;
                s.color = lerp_color(&pr.sa.color, &pr.sb.color, t);
                out.push(s);
            }
            (None, Some(i), None) if t < 0.5 => {
                let mut s = a[i].clone();
                s.id = Id(String::new());
                out.push(s);
            }
            (None, None, Some(j)) if t >= 0.5 => {
                let mut s = b[j].clone();
                s.id = Id(String::new());
                out.push(s);
            }
            _ => {}
        }
    }
    out
}

/// Tempos das intercalações entre duas chaves: `n` desenhos com o easing dado (ex.: ease_out = acumula perto de B,
/// como um "tsume" para a chave final).
pub fn spacing(n: usize, ease: Ease) -> Vec<f32> {
    (1..=n).map(|k| ease.apply(k as f32 / (n + 1) as f32)).collect()
}

/// Gráfico de "metades" do genga: cada intercalação no meio do intervalo restante em direção a uma chave
/// (`toward_b` = favorece B). Ex.: n=3 → [0.5, 0.75, 0.875].
pub fn halves(n: usize, toward_b: bool) -> Vec<f32> {
    let mut v = Vec::new();
    let mut t = 0.0f32;
    for _ in 0..n {
        t += (1.0 - t) / 2.0;
        v.push(if toward_b { t } else { 1.0 - t });
    }
    if !toward_b {
        v.reverse();
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_core::{VectorTip, WidthProfile};

    fn st(pts: &[(f32, f32)], closed: bool) -> Stroke {
        Stroke {
            id: Id::from("x"),
            points: pts.iter().map(|&(x, y)| StrokePoint { x, y, pressure: 1.0, tilt: None, t: None }).collect(),
            geometry: StrokeGeometry::Polyline,
            tension: 0.0,
            tip: VectorTip::Marker,
            width: 4.0,
            profile: WidthProfile::default(),
            color: ColorRef::default(),
            opacity: 1.0,
            closed,
            aliased: false,
            color_trace: false,
        }
    }

    fn len(s: &Stroke) -> f32 {
        s.points.windows(2).map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt()).sum()
    }

    #[test]
    fn rotation_keeps_length_in_global_mode_but_shrinks_linear() {
        // ponteiro girando 90° em torno de (0,0): a meio caminho deve ter o mesmo comprimento (100) a 45°
        let a = vec![st(&[(0.0, 0.0), (100.0, 0.0)], false), st(&[(0.0, 0.0), (0.0, -10.0)], false)];
        let b = vec![st(&[(0.0, 0.0), (0.0, 100.0)], false), st(&[(0.0, 0.0), (10.0, 0.0)], false)];
        let g = inbetween(&a, &b, 0.5, &InbetweenOptions::default());
        let lin = inbetween(&a, &b, 0.5, &InbetweenOptions { motion: MotionMode::Linear, ..Default::default() });
        assert!((len(&g[0]) - 100.0).abs() < 2.0, "global preserva: {}", len(&g[0]));
        assert!(len(&lin[0]) < 75.0, "linear encolhe: {}", len(&lin[0]));
        // extremos exatos
        let g0 = inbetween(&a, &b, 0.0, &InbetweenOptions::default());
        let g1 = inbetween(&a, &b, 1.0, &InbetweenOptions::default());
        assert!((g0[0].points[1].x - 100.0).abs() < 1e-3 && (g1[0].points[1].y - 100.0).abs() < 1e-3);
    }

    #[test]
    fn correspondence_by_shape_when_counts_differ() {
        let a = vec![st(&[(0.0, 0.0), (50.0, 0.0), (100.0, 0.0)], false), st(&[(0.0, 100.0), (0.0, 200.0)], false)];
        let b = vec![st(&[(5.0, 105.0), (5.0, 150.0), (5.0, 205.0)], false), st(&[(100.0, 5.0), (0.0, 5.0)], false), st(&[(300.0, 300.0), (310.0, 300.0)], false)];
        let pairs = correspond(&a, &b, &InbetweenOptions::default());
        assert!(pairs.contains(&(Some(0), Some(1))), "linha horizontal (invertida) casa: {pairs:?}");
        assert!(pairs.contains(&(Some(1), Some(0))), "{pairs:?}");
        assert!(pairs.contains(&(None, Some(2))), "traço novo aparece: {pairs:?}");
        assert_eq!(inbetween(&a, &b, 0.25, &InbetweenOptions::default()).len(), 2);
        assert_eq!(inbetween(&a, &b, 0.75, &InbetweenOptions::default()).len(), 3);
    }

    #[test]
    fn spacing_tables() {
        assert_eq!(halves(3, true), vec![0.5, 0.75, 0.875]);
        assert_eq!(halves(2, false), vec![0.25, 0.5]);
        let s = spacing(3, Ease::Linear);
        assert!((s[1] - 0.5).abs() < 1e-6);
    }
}
