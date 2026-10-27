//! Do traço (pontos + parâmetros) para uma cadeia de amostras (x, y, raio) densa, pronta para rasterizar.
//!
//! - Spline/Freehand: Catmull-Rom **centrípeta** (Yuksel et al. 2011: sem laços nem cúspides), convertida em Bézier
//!   cúbicas por segmento; `tension` encolhe as tangentes.
//! - Polyline: segmentos retos. Bezier: cúbicas explícitas.
//! - Largura: `width/2 × ponta(pressão) × perfil(t) × afinamento(entrada/saída)` — a ponta G-pen afina nas pontas
//!   (入り/抜き) como a linha principal de anime.

use aiai_core::{Stroke, StrokeGeometry, VectorTip};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeSample {
    pub x: f32,
    pub y: f32,
    pub r: f32,
}

#[derive(Clone, Copy)]
struct P {
    x: f32,
    y: f32,
    p: f32,
    /// Multiplicador da largura (do ponto do traço, interpolado).
    w: f32,
}

impl P {
    fn sub(self, o: P) -> (f32, f32) {
        (self.x - o.x, self.y - o.y)
    }
    fn dist(self, o: P) -> f32 {
        let (dx, dy) = self.sub(o);
        (dx * dx + dy * dy).sqrt()
    }
}

/// Distância máxima entre amostras consecutivas (px).
const STEP: f32 = 1.0;

/// Pontos da curva (x, y, pressão) achatados a ~1 px, como o motor desenha (para cortar traços: borracha vetorial).
pub fn flatten_points(stroke: &Stroke) -> Vec<(f32, f32, f32)> {
    flatten(stroke).into_iter().map(|q| (q.x, q.y, q.p)).collect()
}

/// Pontos da curva (x, y, pressão) já achatados, antes da largura.
fn flatten(stroke: &Stroke) -> Vec<P> {
    let pts: Vec<P> = stroke.points.iter().map(|q| P { x: q.x, y: q.y, p: q.pressure, w: q.width }).collect();
    let corners: Vec<bool> = stroke.points.iter().map(|q| q.corner).collect();
    let handles: Vec<Option<[f32; 2]>> = stroke.points.iter().map(|q| q.handle.filter(|_| !q.corner)).collect();
    if pts.len() == 1 {
        return pts;
    }
    match stroke.geometry {
        StrokeGeometry::Polyline => {
            let mut out = Vec::new();
            let n = pts.len();
            let segs = if stroke.closed { n } else { n - 1 };
            for i in 0..segs {
                let a = pts[i];
                let b = pts[(i + 1) % n];
                let steps = (a.dist(b) / STEP).ceil().max(1.0) as usize;
                for k in 0..steps {
                    let t = k as f32 / steps as f32;
                    out.push(lerp(a, b, t));
                }
            }
            if !stroke.closed {
                out.push(pts[n - 1]);
            } else {
                out.push(pts[0]);
            }
            out
        }
        StrokeGeometry::Bezier => {
            let mut out = Vec::new();
            let mut i = 0;
            while i + 3 < pts.len() {
                cubic(&mut out, pts[i], pts[i + 1], pts[i + 2], pts[i + 3]);
                i += 3;
            }
            out.push(*pts.last().expect("não vazio"));
            out
        }
        StrokeGeometry::Spline | StrokeGeometry::Freehand => catmull_rom(&pts, &corners, &handles, stroke.closed, stroke.tension),
    }
}

fn lerp(a: P, b: P, t: f32) -> P {
    P { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, p: a.p + (b.p - a.p) * t, w: a.w + (b.w - a.w) * t }
}

/// Achata uma cúbica (sem o ponto final) com passo ≤ STEP.
fn cubic(out: &mut Vec<P>, b0: P, b1: P, b2: P, b3: P) {
    let est = b0.dist(b1) + b1.dist(b2) + b2.dist(b3);
    let steps = (est / STEP).ceil().clamp(1.0, 20000.0) as usize;
    for k in 0..steps {
        let t = k as f32 / steps as f32;
        let u = 1.0 - t;
        let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        out.push(P {
            x: w0 * b0.x + w1 * b1.x + w2 * b2.x + w3 * b3.x,
            y: w0 * b0.y + w1 * b1.y + w2 * b2.y + w3 * b3.y,
            // pressão segue o parâmetro entre os pontos de passagem (b0 e b3)
            p: b0.p + (b3.p - b0.p) * t,
            w: b0.w + (b3.w - b0.w) * t,
        });
    }
}

/// `corners[i]`: a curva quebra no ponto i (cada lado é feito como se o traço terminasse ali). `handles[i]`: tangente
/// explícita (sai por +h, chega por -h) no lugar da automática.
fn catmull_rom(pts: &[P], corners: &[bool], handles: &[Option<[f32; 2]>], closed: bool, tension: f32) -> Vec<P> {
    let n = pts.len();
    let segs = if closed { n } else { n - 1 };
    let mut out = Vec::new();
    for i in 0..segs {
        let (p1, b1, b2, p2) = segment_controls(pts, corners, handles, closed, tension, i);
        cubic(&mut out, p1, b1, b2, p2);
    }
    out.push(if closed { pts[0] } else { pts[n - 1] });
    out
}

/// Controles da cúbica do segmento `i` (de `pts[i]` a `pts[i+1]`).
fn segment_controls(pts: &[P], corners: &[bool], handles: &[Option<[f32; 2]>], closed: bool, tension: f32, i: usize) -> (P, P, P, P) {
    let i = i as isize;
    let n = pts.len();
    let get = |i: isize| -> P {
        if closed {
            pts[i.rem_euclid(n as isize) as usize]
        } else if i < 0 {
            // reflexão do primeiro segmento
            P { x: 2.0 * pts[0].x - pts[1].x, y: 2.0 * pts[0].y - pts[1].y, ..pts[0] }
        } else if i as usize >= n {
            let a = pts[n - 1];
            let b = pts[n - 2];
            P { x: 2.0 * a.x - b.x, y: 2.0 * a.y - b.y, ..a }
        } else {
            pts[i as usize]
        }
    };
    let k = (1.0 - tension).clamp(0.0, 2.0);
    {
        let (p1, p2) = (get(i), get(i + 1));
        let corner = |k: isize| corners.get(k.rem_euclid(n as isize) as usize).copied().unwrap_or(false);
        // num canto, o vizinho do outro lado é trocado pelo reflexo: a tangente segue o próprio segmento
        let p0 = if corner(i) { P { x: 2.0 * p1.x - p2.x, y: 2.0 * p1.y - p2.y, ..p1 } } else { get(i - 1) };
        let p3 = if corner(i + 1) { P { x: 2.0 * p2.x - p1.x, y: 2.0 * p2.y - p1.y, ..p2 } } else { get(i + 2) };
        let knot = |a: P, b: P| a.dist(b).sqrt().max(1e-4); // alfa = 0,5 (centrípeta)
        let (d01, d12, d23) = (knot(p0, p1), knot(p1, p2), knot(p2, p3));
        let (t0, t1, t2, t3) = (0.0, d01, d01 + d12, d01 + d12 + d23);
        let tangent = |a: P, b: P, c: P, ta: f32, tb: f32, tc: f32| -> (f32, f32) {
            // derivada em b (com tempos ta < tb < tc), multiplicada por (t2 - t1) depois
            let (abx, aby) = b.sub(a);
            let (acx, acy) = c.sub(a);
            let (bcx, bcy) = c.sub(b);
            (
                abx / (tb - ta) - acx / (tc - ta) + bcx / (tc - tb),
                aby / (tb - ta) - acy / (tc - ta) + bcy / (tc - tb),
            )
        };
        let m1 = tangent(p0, p1, p2, t0, t1, t2);
        let m2 = tangent(p1, p2, p3, t1, t2, t3);
        let s = (t2 - t1) / 3.0 * k;
        let mut b1 = P { x: p1.x + m1.0 * s, y: p1.y + m1.1 * s, ..p1 };
        let mut b2 = P { x: p2.x - m2.0 * s, y: p2.y - m2.1 * s, ..p2 };
        let h = |k: isize| handles.get(k.rem_euclid(n as isize) as usize).copied().flatten();
        if let Some([hx, hy]) = h(i) {
            b1 = P { x: p1.x + hx, y: p1.y + hy, ..p1 };
        }
        if let Some([hx, hy]) = h(i + 1) {
            b2 = P { x: p2.x - hx, y: p2.y - hy, ..p2 };
        }
        (p1, b1, b2, p2)
    }
}

/// Alça (tangente) do ponto `i` como o motor usa: a explícita, ou a automática da spline (o controle de saída menos o
/// ponto; no último ponto de um traço aberto, o ponto menos o controle de chegada). `None` fora de Spline/Freehand.
pub fn point_handle(stroke: &Stroke, i: usize) -> Option<[f32; 2]> {
    if !matches!(stroke.geometry, StrokeGeometry::Spline | StrokeGeometry::Freehand) || i >= stroke.points.len() || stroke.points.len() < 2 {
        return None;
    }
    if let Some(h) = stroke.points[i].handle {
        return Some(h);
    }
    let pts: Vec<P> = stroke.points.iter().map(|q| P { x: q.x, y: q.y, p: q.pressure, w: q.width }).collect();
    let corners: Vec<bool> = stroke.points.iter().map(|q| q.corner).collect();
    let handles: Vec<Option<[f32; 2]>> = stroke.points.iter().map(|q| q.handle.filter(|_| !q.corner)).collect();
    let n = pts.len();
    let segs = if stroke.closed { n } else { n - 1 };
    if i < segs {
        let (p1, b1, _, _) = segment_controls(&pts, &corners, &handles, stroke.closed, stroke.tension, i);
        Some([b1.x - p1.x, b1.y - p1.y])
    } else {
        let (_, _, b2, p2) = segment_controls(&pts, &corners, &handles, stroke.closed, stroke.tension, i - 1);
        Some([p2.x - b2.x, p2.y - b2.y])
    }
}

/// Largura (px) do traço no ponto `i`: largura × ponta(pressão) × multiplicador do ponto (sem o afinamento das
/// pontas nem o perfil). É o número que o painel Editar pontos mostra e edita.
pub fn width_at_point(stroke: &Stroke, i: usize) -> f32 {
    stroke.points.get(i).map(|q| stroke.width * tip_factor(stroke.tip, q.pressure) * q.width).unwrap_or(stroke.width)
}

/// Multiplicador do ponto `i` que dá a largura `px` naquele ponto (inverso de [`width_at_point`]).
pub fn point_width_for(stroke: &Stroke, i: usize, px: f32) -> f32 {
    let q = stroke.points.get(i).map(|q| q.pressure).unwrap_or(1.0);
    (px / (stroke.width * tip_factor(stroke.tip, q)).max(1e-4)).clamp(0.0, 20.0)
}

/// Multiplicador da ponta pela pressão.
fn tip_factor(tip: VectorTip, p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    match tip {
        VectorTip::GPen => 0.12 + 0.88 * p.powf(0.8),
        VectorTip::MaruPen => 0.55 + 0.45 * p,
        VectorTip::Marker => 1.0,
        VectorTip::Pencil | VectorTip::Genga => 0.4 + 0.6 * p,
        VectorTip::School => 0.75 + 0.25 * p,
        VectorTip::Fude => 0.04 + 0.96 * p.powf(1.3),
        VectorTip::Square => 1.0,
    }
}

/// Afinamento de entrada/saída (0..1) para pontas que afinam.
/// `blunt` = [começo, fim] sem afinar (ponta reta: ponto da ponta marcado como canto, como a ponta soldada).
fn taper(tip: VectorTip, s: f32, total: f32, width: f32, blunt: [bool; 2]) -> f32 {
    let len = match tip {
        VectorTip::GPen => (total * 0.18).min(width * 5.0),
        VectorTip::MaruPen => (total * 0.08).min(width * 2.0),
        VectorTip::School => (total * 0.06).min(width * 1.5),
        VectorTip::Fude => (total * 0.25).min(width * 7.0),
        VectorTip::Marker | VectorTip::Pencil | VectorTip::Genga | VectorTip::Square => 0.0,
    };
    if len <= 0.0 {
        return 1.0;
    }
    let ramp = |d: f32| {
        let t = (d / len).clamp(0.0, 1.0);
        0.15 + 0.85 * (t * t * (3.0 - 2.0 * t))
    };
    let a = if blunt[0] { 1.0 } else { ramp(s) };
    let b = if blunt[1] { 1.0 } else { ramp(total - s) };
    a.min(b)
}

fn profile_at(profile: &[[f32; 2]], t: f32) -> f32 {
    if profile.is_empty() {
        return 1.0;
    }
    if t <= profile[0][0] {
        return profile[0][1];
    }
    for w in profile.windows(2) {
        let ([t0, m0], [t1, m1]) = (w[0], w[1]);
        if t <= t1 {
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
            return m0 + (m1 - m0) * f;
        }
    }
    profile[profile.len() - 1][1]
}

/// Amostras finais (x, y, raio) do traço.
pub fn sample_stroke(stroke: &Stroke) -> Vec<StrokeSample> {
    let flat = flatten(stroke);
    if flat.is_empty() {
        return Vec::new();
    }
    let mut cum = Vec::with_capacity(flat.len());
    let mut acc = 0.0f32;
    cum.push(0.0);
    for w in flat.windows(2) {
        acc += w[0].dist(w[1]);
        cum.push(acc);
    }
    let total = acc.max(1e-6);
    let min_r = if stroke.aliased { 0.5 } else { 0.25 };
    let blunt = [stroke.points.first().is_some_and(|q| q.corner), stroke.points.last().is_some_and(|q| q.corner)];
    flat.iter()
        .zip(&cum)
        .map(|(q, &s)| {
            let t = s / total;
            // perfil explícito e afinamento automático da ponta: vale o mais fino dos dois. Multiplicar os dois
            // (0,12 × 0,15) derrubava a ponta para o raio mínimo e as linhas não se encontravam nos cantos.
            let mut k = profile_at(&stroke.profile.0, t);
            if !stroke.closed {
                k = k.min(taper(stroke.tip, s, total, stroke.width, blunt));
            }
            let r = 0.5 * stroke.width * tip_factor(stroke.tip, q.p) * q.w.max(0.0) * k;
            StrokeSample { x: q.x, y: q.y, r: r.max(min_r) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_core::{ColorRef, Id, StrokePoint, WidthProfile};

    pub(crate) fn mk(points: &[(f32, f32, f32)], geometry: StrokeGeometry, tip: VectorTip, width: f32) -> Stroke {
        Stroke {
            id: Id::from("s"),
            points: points.iter().map(|&(x, y, p)| StrokePoint { x, y, pressure: p, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
            geometry,
            tension: 0.0,
            tip,
            width,
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
    fn spline_passes_through_points_and_is_dense() {
        let s = mk(&[(0.0, 0.0, 1.0), (100.0, 50.0, 1.0), (200.0, 0.0, 1.0)], StrokeGeometry::Spline, VectorTip::Marker, 4.0);
        let smp = sample_stroke(&s);
        assert!(smp.iter().any(|q| (q.x - 100.0).abs() < 1e-3 && (q.y - 50.0).abs() < 1e-3), "passa pelo ponto do meio");
        for w in smp.windows(2) {
            let d = ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt();
            assert!(d <= STEP * 1.6, "amostras densas: {d}");
        }
        assert!(smp.iter().all(|q| (q.r - 2.0).abs() < 1e-5), "marcador tem largura constante");
    }

    #[test]
    fn gpen_tapers_at_both_ends() {
        let s = mk(&[(0.0, 0.0, 1.0), (300.0, 0.0, 1.0)], StrokeGeometry::Polyline, VectorTip::GPen, 10.0);
        let smp = sample_stroke(&s);
        let mid = smp[smp.len() / 2].r;
        assert!(smp[0].r < mid * 0.3 && smp[smp.len() - 1].r < mid * 0.3, "{} {} {}", smp[0].r, mid, smp.last().unwrap().r);
        assert!((mid - 5.0).abs() < 0.01);
    }

    #[test]
    fn corner_on_an_end_makes_it_blunt() {
        // ponta soldada: o último ponto é canto e não afina; o começo continua afinando
        let mut s = mk(&[(0.0, 0.0, 1.0), (300.0, 0.0, 1.0)], StrokeGeometry::Polyline, VectorTip::GPen, 10.0);
        s.points[1].corner = true;
        let smp = sample_stroke(&s);
        let mid = smp[smp.len() / 2].r;
        assert!(smp[0].r < mid * 0.3, "o começo afina");
        assert!((smp[smp.len() - 1].r - mid).abs() < 0.01, "o fim fica reto: {}", smp.last().unwrap().r);
    }
}
