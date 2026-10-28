//! Assistência pontual de traço, determinística (auditoria 4.8e; sem rede neural e sem IA generativa de imagem):
//!
//! - **Linha fantasma** (`continuation`): pelos últimos ~40 px do traço em andamento, a direção e a curvatura (com a
//!   variação dela, como uma clotoide, a espiral de Euler que dá os arcos limpos) seguem por mais `len` px. Se a
//!   continuação passa perto de outra linha (ponta ou meio de um traço do cel), a fantasma termina exatamente nela
//!   (fechamento de junção, sem buraco para o balde).
//! - **Embelezar** (`beautify`): depois de soltar, uma versão do traço sem o tremido da mão (média gaussiana ao longo
//!   do arco, pontas no lugar, e os pontos de controle reduzidos), para trocar pela original com uma tecla.
//!
//! As duas devolvem pontos de curva; quem chama decide se vira traço (com a pressão e a largura do traço original).

/// Sugestão de continuação: pontos (px do cel) a partir da ponta da caneta, e onde ela encontra outra linha.
#[derive(Debug, Clone, PartialEq)]
pub struct Ghost {
    pub points: Vec<[f32; 2]>,
    /// O ponto de outra linha em que a fantasma termina (fechamento de junção).
    pub joins: Option<[f32; 2]>,
}

/// Reamostra o caminho a cada `step` px pelo comprimento do arco (do começo ao fim).
pub fn resample(path: &[[f32; 2]], step: f32) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    let Some(&first) = path.first() else { return out };
    out.push(first);
    let mut carry = 0.0f32;
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg = (b[0] - a[0]).hypot(b[1] - a[1]);
        if seg < 1e-6 {
            continue;
        }
        let mut t = step - carry;
        while t <= seg {
            let f = t / seg;
            out.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]);
            t += step;
        }
        carry = seg - (t - step);
    }
    // o último ponto sempre entra (a ponta do traço fica onde está)
    if let (Some(&l), Some(&o)) = (path.last(), out.last()) {
        if (l[0] - o[0]).hypot(l[1] - o[1]) > 0.01 {
            out.push(l);
        }
    }
    out
}

fn angle_diff(a: f32, b: f32) -> f32 {
    let mut d = b - a;
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

/// Continua o traço `path` por `len` px (ver o topo do módulo). `targets`: pontos das outras linhas do cel (pontas e
/// amostras dos traços); a fantasma fecha no alvo a até `snap` px do caminho previsto. `None` com menos de ~12 px de
/// traço (não dá para saber a direção).
pub fn continuation(path: &[[f32; 2]], len: f32, targets: &[[f32; 2]], snap: f32) -> Option<Ghost> {
    const STEP: f32 = 3.0;
    let r = resample(path, STEP);
    if r.len() < 5 || len <= 0.0 {
        return None;
    }
    // os últimos ~42 px (até 15 segmentos)
    let tail = &r[r.len().saturating_sub(15)..];
    let ang: Vec<f32> = tail.windows(2).map(|w| (w[1][1] - w[0][1]).atan2(w[1][0] - w[0][0])).collect();
    // curvatura de cada par de segmentos (rad/px), na posição s (px) a partir do começo da cauda
    let k: Vec<(f32, f32)> = ang.windows(2).enumerate().map(|(i, w)| ((i as f32 + 1.0) * STEP, angle_diff(w[0], w[1]) / STEP)).collect();
    if k.is_empty() {
        return None;
    }
    // reta pelos mínimos quadrados: k(s) = k0 + k1 (s - s_fim), pesando mais o fim
    let s_end = k.last().map(|q| q.0).unwrap_or(0.0);
    let (mut sw, mut sx, mut sy, mut sxx, mut sxy) = (0.0f32, 0.0, 0.0, 0.0, 0.0);
    for (i, (s, kk)) in k.iter().enumerate() {
        let wgt = 1.0 + i as f32;
        let x = s - s_end;
        sw += wgt;
        sx += wgt * x;
        sy += wgt * kk;
        sxx += wgt * x * x;
        sxy += wgt * x * kk;
    }
    let det = sw * sxx - sx * sx;
    let (k0, k1) = if det.abs() > 1e-6 { ((sy * sxx - sx * sxy) / det, (sw * sxy - sx * sy) / det) } else { (sy / sw, 0.0) };
    // limites: raio mínimo de 6 px e variação suave (a mão tremendo não vira espiral)
    let k0 = k0.clamp(-1.0 / 6.0, 1.0 / 6.0);
    let k1 = k1.clamp(-0.004, 0.004);
    let mut theta = *ang.last().unwrap_or(&0.0);
    let tip = *r.last().expect("há pontos");
    let mut pts = vec![tip];
    let mut p = tip;
    let ds = 1.5f32;
    let mut s = 0.0f32;
    while s < len {
        let kk = (k0 + k1 * s).clamp(-1.0 / 6.0, 1.0 / 6.0);
        theta += kk * ds;
        p = [p[0] + theta.cos() * ds, p[1] + theta.sin() * ds];
        pts.push(p);
        s += ds;
    }
    // fechamento de junção: o alvo mais perto do caminho previsto (à frente da ponta, não no próprio traço)
    let own_end = path.last().copied().unwrap_or(tip);
    let mut best: Option<(f32, usize, [f32; 2])> = None;
    for t in targets {
        if (t[0] - own_end[0]).hypot(t[1] - own_end[1]) < 2.0 {
            continue;
        }
        for (i, q) in pts.iter().enumerate().skip(2) {
            let d = (q[0] - t[0]).hypot(q[1] - t[1]);
            if d <= snap && best.is_none_or(|(bd, bi, _)| (i as f32 * ds + 4.0 * d) < (bi as f32 * ds + 4.0 * bd)) {
                best = Some((d, i, *t));
            }
        }
    }
    if let Some((_, i, t)) = best {
        // do bico até o alvo: Bézier quadrática com a tangente da ponta, terminando exatamente no alvo
        let d = (t[0] - tip[0]).hypot(t[1] - tip[1]);
        let t0 = *ang.last().unwrap_or(&0.0);
        let c = [tip[0] + t0.cos() * d * 0.5, tip[1] + t0.sin() * d * 0.5];
        let n = (i.max(4)) as f32;
        let curve: Vec<[f32; 2]> = (0..=i.max(4))
            .map(|j| {
                let u = j as f32 / n;
                let v = 1.0 - u;
                [v * v * tip[0] + 2.0 * v * u * c[0] + u * u * t[0], v * v * tip[1] + 2.0 * v * u * c[1] + u * u * t[1]]
            })
            .collect();
        return Some(Ghost { points: curve, joins: Some(t) });
    }
    Some(Ghost { points: pts, joins: None })
}

/// Versão sem o tremido: reamostra a cada 2 px, média gaussiana (`strength` px de desvio) com as pontas fixas, e
/// reduz os pontos (Douglas-Peucker a 0,4 px). Traço fechado (`closed`) é suavizado em volta.
pub fn beautify(path: &[[f32; 2]], strength: f32, closed: bool) -> Vec<[f32; 2]> {
    let r = resample(path, 2.0);
    let n = r.len();
    if n < 4 || strength <= 0.0 {
        return path.to_vec();
    }
    let sigma = (strength / 2.0).max(0.5);
    let rad = (sigma * 3.0).ceil() as isize;
    let kern: Vec<f32> = (-rad..=rad).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let mut out = Vec::with_capacity(n);
    for i in 0..n as isize {
        if !closed && (i == 0 || i == n as isize - 1) {
            out.push(r[i as usize]);
            continue;
        }
        let (mut x, mut y, mut wsum) = (0.0f32, 0.0f32, 0.0f32);
        let m = n as isize - 1;
        for (k, w) in kern.iter().enumerate() {
            let j = i + k as isize - rad;
            // fechado: dá a volta; aberto: reflexo ímpar nas pontas (2·ponta − espelho), que mantém a ponta no lugar
            // e a direção da linha, sem encolher a linha nem deixar o tremido perto das pontas
            let q = if closed {
                r[j.rem_euclid(n as isize) as usize]
            } else if j < 0 {
                let e = r[(-j).min(m) as usize];
                [2.0 * r[0][0] - e[0], 2.0 * r[0][1] - e[1]]
            } else if j > m {
                let e = r[(2 * m - j).max(0) as usize];
                [2.0 * r[m as usize][0] - e[0], 2.0 * r[m as usize][1] - e[1]]
            } else {
                r[j as usize]
            };
            x += q[0] * w;
            y += q[1] * w;
            wsum += w;
        }
        out.push([x / wsum, y / wsum]);
    }
    rdp(&out, 0.4)
}

/// Douglas-Peucker: os pontos que mudam a forma em mais de `tol` px.
pub fn rdp(pts: &[[f32; 2]], tol: f32) -> Vec<[f32; 2]> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (pts[a], pts[b]);
        let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
        let l = dx.hypot(dy).max(1e-6);
        let (mut far, mut fi) = (0.0f32, a);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = ((p[0] - pa[0]) * dy - (p[1] - pa[1]) * dx).abs() / l;
            if d > far {
                far = d;
                fi = i;
            }
        }
        if far > tol {
            keep[fi] = true;
            stack.push((a, fi));
            stack.push((fi, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Alvos de fechamento de um conjunto de traços: as pontas e uma amostra a cada ~4 px de cada traço.
pub fn targets_of(strokes: &[aiai_core::Stroke]) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    for s in strokes {
        let sm = aiai_paint::sample_stroke(s);
        let pts: Vec<[f32; 2]> = sm.iter().map(|q| [q.x, q.y]).collect();
        out.extend(resample(&pts, 4.0));
        if let Some(l) = pts.last() {
            out.push(*l);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arc(cx: f32, cy: f32, r: f32, a0: f32, a1: f32, n: usize) -> Vec<[f32; 2]> {
        (0..=n).map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            [cx + r * a.cos(), cy + r * a.sin()]
        }).collect()
    }

    #[test]
    fn a_straight_line_continues_straight() {
        let path: Vec<[f32; 2]> = (0..=30).map(|i| [10.0 + i as f32 * 2.0, 50.0]).collect();
        let g = continuation(&path, 60.0, &[], 6.0).unwrap();
        let end = *g.points.last().unwrap();
        assert!((end[0] - 130.0).abs() < 2.0 && (end[1] - 50.0).abs() < 0.5, "{end:?}");
        assert!(g.joins.is_none());
    }

    #[test]
    fn an_arc_keeps_its_curvature() {
        // um quarto de círculo de raio 80: a continuação fica no mesmo círculo
        let path = arc(100.0, 100.0, 80.0, 0.0, std::f32::consts::FRAC_PI_2, 60);
        let g = continuation(&path, 50.0, &[], 6.0).unwrap();
        for q in &g.points {
            let r = (q[0] - 100.0).hypot(q[1] - 100.0);
            assert!((r - 80.0).abs() < 2.5, "saiu do arco: raio {r}");
        }
    }

    #[test]
    fn the_ghost_closes_exactly_on_a_nearby_line() {
        // traço horizontal indo para a direita; uma linha vertical em x = 120 (y de 30 a 70), 4 px abaixo do caminho
        let path: Vec<[f32; 2]> = (0..=30).map(|i| [20.0 + i as f32 * 2.0, 50.0]).collect();
        let targets: Vec<[f32; 2]> = (0..=10).map(|i| [120.0, 30.0 + i as f32 * 4.0]).collect();
        let g = continuation(&path, 80.0, &targets, 6.0).unwrap();
        let j = g.joins.expect("fecha na linha vertical");
        assert_eq!(j[0], 120.0);
        assert_eq!(*g.points.last().unwrap(), j);
        assert!((g.points[0][0] - 80.0).abs() < 0.5);
        // longe demais (alvos a 20 px do caminho): segue reto, sem fechar
        let far: Vec<[f32; 2]> = (0..=10).map(|i| [120.0, 75.0 + i as f32 * 4.0]).collect();
        assert!(continuation(&path, 80.0, &far, 6.0).unwrap().joins.is_none());
    }

    #[test]
    fn beautify_removes_the_shake_and_keeps_the_ends() {
        // reta de 200 px com tremido de ±1,5 px
        let shaky: Vec<[f32; 2]> = (0..=100).map(|i| [i as f32 * 2.0, 40.0 + 1.5 * ((i as f32) * 1.7).sin()]).collect();
        let b = beautify(&shaky, 6.0, false);
        assert_eq!(b[0], shaky[0]);
        assert_eq!(*b.last().unwrap(), *shaky.last().unwrap());
        let worst = b.iter().skip(3).take(b.len().saturating_sub(6)).map(|p| (p[1] - 40.0).abs()).fold(0.0f32, f32::max);
        assert!(worst < 0.5, "tremido que sobrou: {worst}");
        assert!(b.len() < shaky.len() / 3, "{} pontos", b.len());
    }
}
