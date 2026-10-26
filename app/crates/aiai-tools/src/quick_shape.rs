//! Forma rápida (QuickArc, auditoria 4.8d): o traço à mão que termina com a caneta parada um instante vira a forma
//! limpa que ele parece — reta, arco de círculo ou elipse fechada. Se nenhuma encaixa bem, o traço fica como foi.
//! Tudo em px do cel; `px` é o tamanho de 1 px da tela no cel (as tolerâncias mínimas são da tela).

use aiai_core::StrokeGeometry;

/// O que o traço virou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickKind {
    Line,
    Arc,
    Ellipse,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuickShape {
    pub kind: QuickKind,
    pub points: Vec<[f32; 2]>,
    pub geometry: StrokeGeometry,
    pub closed: bool,
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn seg_dist(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    dist(p, [a[0] + dx * t, a[1] + dy * t])
}

/// Círculo pelos três pontos (centro, raio); `None` se estão alinhados.
fn circle3(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> Option<([f32; 2], f32)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-6 {
        return None;
    }
    let (a2, b2, c2) = (a[0] * a[0] + a[1] * a[1], b[0] * b[0] + b[1] * b[1], c[0] * c[0] + c[1] * c[1]);
    let ux = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
    let uy = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
    let o = [ux, uy];
    Some((o, dist(o, a)))
}

/// Ponto na fração `f` (0..1) do comprimento do caminho.
fn at_fraction(pts: &[[f32; 2]], cum: &[f32], f: f32) -> [f32; 2] {
    let target = cum.last().copied().unwrap_or(0.0) * f;
    let i = cum.partition_point(|&c| c < target).clamp(1, pts.len() - 1);
    let (c0, c1) = (cum[i - 1], cum[i]);
    let t = if c1 > c0 { (target - c0) / (c1 - c0) } else { 0.0 };
    [pts[i - 1][0] + (pts[i][0] - pts[i - 1][0]) * t, pts[i - 1][1] + (pts[i][1] - pts[i - 1][1]) * t]
}

/// Tenta encaixar o caminho numa reta, num arco ou numa elipse fechada (nessa ordem de preferência).
pub fn quick_shape(pts: &[[f32; 2]], px: f32) -> Option<QuickShape> {
    if pts.len() < 3 {
        return None;
    }
    let mut cum = vec![0.0f32];
    for w in pts.windows(2) {
        cum.push(cum.last().unwrap() + dist(w[0], w[1]));
    }
    let len = *cum.last().unwrap();
    if len < 20.0 * px {
        return None;
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let chord = dist(a, b);
    // elipse: volta perto do começo depois de dar a volta
    if chord < (0.2 * len / std::f32::consts::PI).max(12.0 * px) && len > 60.0 * px {
        return ellipse(pts);
    }
    // reta
    let dev_line = pts.iter().map(|&p| seg_dist(p, a, b)).fold(0.0f32, f32::max);
    if dev_line <= (0.035 * chord).max(3.0 * px) {
        return Some(QuickShape { kind: QuickKind::Line, points: vec![a, b], geometry: StrokeGeometry::Polyline, closed: false });
    }
    // arco: círculo pelo começo, meio e fim; todos os pontos perto dele, e o meio do lado certo
    let m = at_fraction(pts, &cum, 0.5);
    let (o, r) = circle3(a, m, b)?;
    let dev = pts.iter().map(|&p| (dist(p, o) - r).abs()).fold(0.0f32, f32::max);
    if dev > (0.07 * r).max(3.0 * px) || r > 20.0 * len {
        return None;
    }
    let ang = |p: [f32; 2]| (p[1] - o[1]).atan2(p[0] - o[0]);
    let (a0, am, a1) = (ang(a), ang(m), ang(b));
    // varredura de a0 até a1 passando por am
    let tau = std::f32::consts::TAU;
    let norm = |x: f32| ((x % tau) + tau) % tau;
    let (to_m, to_end) = (norm(am - a0), norm(a1 - a0));
    let sweep = if to_m <= to_end { to_end } else { to_end - tau };
    let n = ((sweep.abs() / (std::f32::consts::PI / 8.0)).ceil() as usize).max(2);
    let points = (0..=n)
        .map(|k| {
            let t = a0 + sweep * k as f32 / n as f32;
            [o[0] + r * t.cos(), o[1] + r * t.sin()]
        })
        .collect();
    Some(QuickShape { kind: QuickKind::Arc, points, geometry: StrokeGeometry::Spline, closed: false })
}

/// Elipse pelos momentos do caminho (centro e eixos da covariância); aceita se os pontos ficam perto dela.
fn ellipse(pts: &[[f32; 2]]) -> Option<QuickShape> {
    let n = pts.len() as f32;
    let (cx, cy) = (pts.iter().map(|p| p[0]).sum::<f32>() / n, pts.iter().map(|p| p[1]).sum::<f32>() / n);
    let (mut sxx, mut syy, mut sxy) = (0.0f32, 0.0f32, 0.0f32);
    for p in pts {
        let (dx, dy) = (p[0] - cx, p[1] - cy);
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    let (sxx, syy, sxy) = (sxx / n, syy / n, sxy / n);
    let th = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (s, c) = th.sin_cos();
    // raio de cada ponto nos eixos girados: o raio do eixo é o maior alcance (robusto a amostras desiguais)
    let (mut ra, mut rb) = (0.0f32, 0.0f32);
    for p in pts {
        let (dx, dy) = (p[0] - cx, p[1] - cy);
        ra = ra.max((dx * c + dy * s).abs());
        rb = rb.max((-dx * s + dy * c).abs());
    }
    if ra < 2.0 || rb < 2.0 {
        return None;
    }
    let dev = pts
        .iter()
        .map(|p| {
            let (dx, dy) = (p[0] - cx, p[1] - cy);
            let (u, v) = ((dx * c + dy * s) / ra, (-dx * s + dy * c) / rb);
            ((u * u + v * v).sqrt() - 1.0).abs()
        })
        .sum::<f32>()
        / n;
    if dev > 0.09 {
        return None;
    }
    let points = (0..16)
        .map(|k| {
            let t = k as f32 / 16.0 * std::f32::consts::TAU;
            let (u, v) = (ra * t.cos(), rb * t.sin());
            [cx + u * c - v * s, cy + u * s + v * c]
        })
        .collect();
    Some(QuickShape { kind: QuickKind::Ellipse, points, geometry: StrokeGeometry::Spline, closed: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wobble(k: usize) -> f32 {
        ((k * 7919) % 13) as f32 / 13.0 - 0.5
    }

    #[test]
    fn line_arc_ellipse_and_none() {
        let line: Vec<[f32; 2]> = (0..60).map(|k| [10.0 + k as f32 * 3.0, 50.0 + wobble(k) * 2.0 + k as f32 * 0.4]).collect();
        let q = quick_shape(&line, 1.0).unwrap();
        assert_eq!(q.kind, QuickKind::Line);
        assert_eq!(q.points.len(), 2);
        // arco de 120° num círculo de raio 80
        let arc: Vec<[f32; 2]> = (0..=60).map(|k| {
            let t = (200.0f32 + k as f32 * 2.0).to_radians();
            [300.0 + (80.0 + wobble(k) * 2.0) * t.cos(), 300.0 + 80.0 * t.sin()]
        }).collect();
        let q = quick_shape(&arc, 1.0).unwrap();
        assert_eq!(q.kind, QuickKind::Arc);
        for p in &q.points {
            assert!((dist(*p, [300.0, 300.0]) - 80.0).abs() < 2.5, "no círculo: {p:?}");
        }
        assert!(dist(q.points[0], arc[0]) < 2.5 && dist(*q.points.last().unwrap(), arc[60]) < 2.5, "mesmas pontas");
        // elipse girada que fecha
        let el: Vec<[f32; 2]> = (0..=90).map(|k| {
            let t = k as f32 / 90.0 * std::f32::consts::TAU * 0.98;
            let (u, v) = (120.0 * t.cos(), 60.0 * t.sin());
            let (s, c) = 0.5f32.sin_cos();
            [400.0 + u * c - v * s + wobble(k), 300.0 + u * s + v * c]
        }).collect();
        let q = quick_shape(&el, 1.0).unwrap();
        assert!(q.kind == QuickKind::Ellipse && q.closed, "{q:?}");
        // zigue-zague: nada
        let zig: Vec<[f32; 2]> = (0..60).map(|k| [k as f32 * 4.0, if k % 10 < 5 { 0.0 } else { 30.0 }]).collect();
        assert!(quick_shape(&zig, 1.0).is_none());
    }
}
