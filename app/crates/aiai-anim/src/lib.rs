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
//! 4. Traços sem par se apagam (os de A, do fim para o começo) ou se desenham (os de B) ao longo das intercalações,
//!    andando junto com o movimento do desenho, em vez de pularem na metade do caminho.

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

/// Os dois traços podem casar: mesmo grupo de intercalação, ou um deles sem grupo.
fn same_group(x: &Stroke, y: &Stroke) -> bool {
    x.group.is_empty() || y.group.is_empty() || x.group == y.group
}

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
    let same_structure = a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.points.len() == y.points.len() && x.closed == y.closed && x.group == y.group);
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
                if used_a[i] || used_b[j] || a[i].closed != b[j].closed || !same_group(&a[i], &b[j]) {
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

/// O movimento global de A para B (o mesmo das intercalações em `MotionMode::Global`): rotação, escala e trajetória do
/// centro pelos traços casados. Serve para levar outras coisas junto com o desenho, como as sementes do balde.
#[derive(Debug, Clone, Copy)]
pub struct GlobalMotion {
    rigid: Option<(f32, f32, [f32; 2], [f32; 2])>,
    bend: f32,
}

impl GlobalMotion {
    pub fn of(a: &[Stroke], b: &[Stroke], opts: &InbetweenOptions) -> GlobalMotion {
        let (mut all_a, mut all_b) = (Vec::new(), Vec::new());
        for (ia, ib) in correspond(a, b, opts) {
            if let (Some(i), Some(j)) = (ia, ib) {
                let n = a[i].points.len().max(b[j].points.len()).max(8) * 2;
                let pa = resample(&a[i], n);
                let pb = align(&pa, &resample(&b[j], n), a[i].closed);
                all_a.extend(pa);
                all_b.extend(pb);
            }
        }
        GlobalMotion { rigid: (!all_a.is_empty()).then(|| procrustes(&all_a, &all_b)), bend: opts.bend }
    }

    /// Ponto `pa` (em A) cujo par em B é `pb`, no tempo `t`: o movimento rígido mais o resto que falta, como os pontos
    /// dos traços. Sem traços casados, reta de `pa` a `pb`.
    pub fn at(&self, pa: [f32; 2], pb: [f32; 2], t: f32) -> [f32; 2] {
        let q = interpolate_points(&[P { x: pa[0], y: pa[1], p: 1.0 }], &[P { x: pb[0], y: pb[1], p: 1.0 }], t, self.rigid, self.bend);
        [q[0].x, q[0].y]
    }
}

/// Primeira fração `f` (0..1) do comprimento da polilinha `pts`.
fn trim(pts: &[P], f: f32) -> Vec<P> {
    if pts.len() < 2 || f >= 1.0 {
        return pts.to_vec();
    }
    let total: f32 = pts.windows(2).map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt()).sum();
    let target = total * f.max(0.0);
    let mut out = vec![pts[0]];
    let mut acc = 0.0f32;
    for w in pts.windows(2) {
        let d = ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt();
        if acc + d >= target {
            let k = if d > 1e-6 { (target - acc) / d } else { 0.0 };
            out.push(P { x: w[0].x + (w[1].x - w[0].x) * k, y: w[0].y + (w[1].y - w[0].y) * k, p: w[0].p + (w[1].p - w[0].p) * k });
            return out;
        }
        acc += d;
        out.push(w[1]);
    }
    out
}

/// Traço sem par no tempo `t`: os pontos (no espaço de A) levados pelo movimento rígido e cortados na fração `f`.
fn unmatched(s: &Stroke, to_a: impl Fn(P) -> P, t: f32, rigid: Option<(f32, f32, [f32; 2], [f32; 2])>, bend: f32, f: f32) -> Option<Stroke> {
    let pa: Vec<P> = s.points.iter().map(|q| to_a(P { x: q.x, y: q.y, p: q.pressure })).collect();
    // sem resíduo: o traço anda com o corpo (o alvo do resíduo é o próprio movimento rígido)
    let moved = match rigid {
        Some((ang, sc, ca, cb)) => {
            let (s1, c1) = ang.sin_cos();
            let pb: Vec<P> = pa.iter().map(|a| {
                let (ax, ay) = (a.x - ca[0], a.y - ca[1]);
                P { x: sc * (c1 * ax - s1 * ay) + cb[0], y: sc * (s1 * ax + c1 * ay) + cb[1], p: a.p }
            }).collect();
            interpolate_points(&pa, &pb, t, rigid, bend)
        }
        None => pa,
    };
    let part = trim(&moved, f);
    let len: f32 = part.windows(2).map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt()).sum();
    if len < 0.5 {
        return None;
    }
    let mut out = s.clone();
    out.id = Id(String::new());
    out.closed = false;
    out.points = part.iter().map(|q| StrokePoint { x: q.x, y: q.y, pressure: q.p.clamp(0.0, 1.0), tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect();
    if out.geometry == StrokeGeometry::Bezier {
        out.geometry = StrokeGeometry::Polyline;
    }
    Some(out)
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
    // 1 para N (auditoria 1.5a): traço sem par num grupo que tem um par casado nasce do traço de A desse par (ou, o de A
    // sem par, vai para o traço de B do par), em vez de aparecer/sumir aos poucos
    let partner_in = |g: &str| -> Option<(usize, usize)> {
        if g.is_empty() {
            return None;
        }
        pairs.iter().find_map(|&(i, j)| match (i, j) {
            (Some(i), Some(j)) if a[i].group == g && b[j].group == g => Some((i, j)),
            _ => None,
        })
    };
    let morph = |from: &Stroke, to: &Stroke, keep: &Stroke| -> Stroke {
        let n = from.points.len().max(to.points.len()).max(8) * 2;
        let pa = resample(from, n);
        let pb = align(&pa, &resample(to, n), from.closed && to.closed);
        let rigid = match opts.motion {
            MotionMode::Global => global,
            MotionMode::PerStroke => Some(procrustes(&pa, &pb)),
            MotionMode::Linear => None,
        };
        let pts = interpolate_points(&pa, &pb, t, rigid, opts.bend);
        let mut s = keep.clone();
        s.id = Id(String::new());
        s.closed = from.closed && to.closed;
        s.geometry = StrokeGeometry::Spline;
        s.points = pts.iter().map(|q| StrokePoint { x: q.x, y: q.y, pressure: q.p.clamp(0.0, 1.0), tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect();
        s.width = from.width + (to.width - from.width) * t;
        s
    };
    let mut out = Vec::new();
    for (k, &(ia, ib)) in pairs.iter().enumerate() {
        match (&preps[k], ia, ib) {
            (None, Some(i), None) if t > 0.0 && partner_in(&a[i].group).is_some() => {
                let (_, pj) = partner_in(&a[i].group).expect("par");
                if t < 1.0 {
                    out.push(morph(&a[i], &b[pj], &a[i]));
                }
            }
            (None, None, Some(j)) if t < 1.0 && partner_in(&b[j].group).is_some() => {
                let (pi, _) = partner_in(&b[j].group).expect("par");
                if t > 0.0 {
                    out.push(morph(&a[pi], &b[j], &b[j]));
                }
            }
            (Some(pr), _, _) => {
                let rigid = match opts.motion {
                    MotionMode::Global => global,
                    MotionMode::PerStroke => Some(procrustes(&pr.pa, &pr.pb)),
                    MotionMode::Linear => None,
                };
                let pts = interpolate_points(&pr.pa, &pr.pb, t, rigid, opts.bend);
                let mut s = pr.sa.clone();
                s.id = Id(String::new());
                s.points = pts.iter().map(|q| StrokePoint { x: q.x, y: q.y, pressure: q.p.clamp(0.0, 1.0), tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect();
                if !pr.direct {
                    s.geometry = StrokeGeometry::Spline;
                }
                s.width = pr.sa.width + (pr.sb.width - pr.sa.width) * t;
                s.opacity = pr.sa.opacity + (pr.sb.opacity - pr.sa.opacity) * t;
                s.color = lerp_color(&pr.sa.color, &pr.sb.color, t);
                out.push(s);
            }
            // sem par: o de A se apaga do fim para o começo; o de B se desenha (começa na pose de A, chega na de B)
            (None, Some(i), None) => {
                if t <= 0.0 {
                    let mut s = a[i].clone();
                    s.id = Id(String::new());
                    out.push(s);
                } else if let Some(s) = unmatched(&a[i], |p| p, t, global, opts.bend, 1.0 - t) {
                    out.push(s);
                }
            }
            (None, None, Some(j)) => {
                if t >= 1.0 {
                    let mut s = b[j].clone();
                    s.id = Id(String::new());
                    out.push(s);
                } else {
                    let inv = global.map(|(ang, sc, ca, cb)| {
                        let (s1, c1) = (-ang).sin_cos();
                        move |q: P| {
                            let (bx, by) = ((q.x - cb[0]) / sc.max(1e-6), (q.y - cb[1]) / sc.max(1e-6));
                            P { x: c1 * bx - s1 * by + ca[0], y: s1 * bx + c1 * by + ca[1], p: q.p }
                        }
                    });
                    let st = match inv {
                        Some(f) => unmatched(&b[j], f, t, global, opts.bend, t),
                        None => unmatched(&b[j], |p| p, t, None, opts.bend, t),
                    };
                    if let Some(s) = st {
                        out.push(s);
                    }
                }
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
            points: pts.iter().map(|&(x, y)| StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
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
            group: String::new(),
        }
    }

    fn len(s: &Stroke) -> f32 {
        s.points.windows(2).map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt()).sum()
    }

    fn grouped(pts: &[(f32, f32)], g: &str) -> Stroke {
        let mut s = st(pts, false);
        s.group = g.into();
        s
    }

    #[test]
    fn groups_only_match_inside_the_group() {
        // olho e boca trocam de lugar entre as chaves; sem grupo o casamento iria pelo mais perto
        let a = vec![grouped(&[(0.0, 0.0), (40.0, 0.0)], "olho"), grouped(&[(0.0, 100.0), (40.0, 100.0)], "boca")];
        let b = vec![grouped(&[(0.0, 95.0), (20.0, 95.0), (40.0, 95.0)], "olho"), grouped(&[(0.0, 5.0), (20.0, 5.0), (40.0, 5.0)], "boca")];
        let pairs = correspond(&a, &b, &InbetweenOptions { max_cost: 500.0, ..Default::default() });
        assert!(pairs.contains(&(Some(0), Some(0))) && pairs.contains(&(Some(1), Some(1))), "{pairs:?}");
        // sem grupo, o mais perto ganha
        let mut a2 = a.clone();
        let mut b2 = b.clone();
        a2.iter_mut().chain(b2.iter_mut()).for_each(|s| s.group.clear());
        let pairs = correspond(&a2, &b2, &InbetweenOptions { max_cost: 500.0, ..Default::default() });
        assert!(pairs.contains(&(Some(0), Some(1))), "{pairs:?}");
    }

    #[test]
    fn one_stroke_splits_into_two_inside_a_group() {
        // 1 para N: a franja vira duas mechas; a mecha a mais nasce da de A, não do nada
        let a = vec![grouped(&[(0.0, 0.0), (100.0, 0.0)], "franja")];
        let b = vec![grouped(&[(0.0, 0.0), (50.0, 20.0)], "franja"), grouped(&[(50.0, 20.0), (100.0, 0.0)], "franja")];
        let mid = inbetween(&a, &b, 0.5, &InbetweenOptions::default());
        assert_eq!(mid.len(), 2, "as duas mechas no meio");
        for s in &mid {
            assert!(len(s) > 40.0, "a mecha a mais já tem corpo no meio: {}", len(s));
        }
        assert_eq!(inbetween(&a, &b, 0.0, &InbetweenOptions::default()).len(), 1, "na chave A só a de A");
        assert_eq!(inbetween(&a, &b, 1.0, &InbetweenOptions::default()).len(), 2);
        // o contrário: duas viram uma (a de A sem par vai para a de B do grupo)
        let mid = inbetween(&b, &a, 0.5, &InbetweenOptions::default());
        assert_eq!(mid.len(), 2);
        assert_eq!(inbetween(&b, &a, 1.0, &InbetweenOptions::default()).len(), 1);
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
        // o traço novo (10 px) se desenha aos poucos: 1/4 em t = 0,25, 3/4 em t = 0,75
        let q = inbetween(&a, &b, 0.25, &InbetweenOptions::default());
        let h = inbetween(&a, &b, 0.75, &InbetweenOptions::default());
        assert_eq!((q.len(), h.len()), (3, 3));
        let new_len = |v: &[Stroke]| v.iter().map(len).fold(f32::MAX, f32::min);
        assert!((new_len(&q) - 2.5).abs() < 0.6 && (new_len(&h) - 7.5).abs() < 0.6, "{} {}", new_len(&q), new_len(&h));
        assert_eq!(inbetween(&a, &b, 0.0, &InbetweenOptions::default()).len(), 2, "em A o traço novo ainda não existe");
    }

    #[test]
    fn global_motion_carries_points_around_the_rotation() {
        // braço girando 90° em volta de (0, 0): a ponta (100, 0) que vai para (0, 100) passa por fora da reta (girando),
        // como os pontos do traço intercalado
        let a = vec![st(&[(0.0, 0.0), (100.0, 0.0)], false)];
        let b = vec![st(&[(0.0, 0.0), (0.0, 100.0)], false)];
        let m = GlobalMotion::of(&a, &b, &InbetweenOptions::default());
        let h = m.at([100.0, 0.0], [0.0, 100.0], 0.5);
        let mid = inbetween(&a, &b, 0.5, &InbetweenOptions::default());
        let tip = mid[0].points.last().unwrap();
        assert!((h[0] - tip.x).abs() < 0.5 && (h[1] - tip.y).abs() < 0.5, "a semente vai com a ponta do traço: {h:?} {tip:?}");
        assert!((h[0] * h[0] + h[1] * h[1]).sqrt() > 80.0, "fora da reta (70,7 do pivô): {h:?}");
        let e = m.at([100.0, 0.0], [0.0, 100.0], 1.0);
        assert!((e[0]).abs() < 0.01 && (e[1] - 100.0).abs() < 0.01, "chega no ponto de B: {e:?}");
    }

    #[test]
    fn spacing_tables() {
        assert_eq!(halves(3, true), vec![0.5, 0.75, 0.875]);
        assert_eq!(halves(2, false), vec![0.25, 0.5]);
        let s = spacing(3, Ease::Linear);
        assert!((s[1] - 0.5).abs() < 1e-6);
    }
}
