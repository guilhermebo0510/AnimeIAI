//! Corrigir linha (S25, à moda do Clip Studio): as ferramentas que mexem em linhas vetoriais já desenhadas e a remoção de
//! poeira. Ponto de controle é o Editar pontos ([`crate::points`]); aqui ficam as que agem por arrasto ou pincel:
//!
//! - **Puxar linha** ([`pinch`]): arrastar a linha a puxa na direção do cursor; o efeito cai ao longo do traço até o
//!   `range` (px de curva) com o formato dado por `level` e, com `fix_ends`, as pontas não saem do lugar.
//! - **Simplificar** ([`simplify_area`]): o pincel passa e os pontos debaixo dele que não seguram a forma saem (a
//!   linha não se afasta mais que a tolerância da de antes; mesma medida do Simplificar do Editar pontos).
//! - **Conectar** ([`connect`]): o pincel passa por duas pontas próximas (do mesmo traço ou de traços diferentes) e
//!   elas viram um traço só (o do traço mais comprido dá cor, ponta e largura base; as larguras em px de cada ponto
//!   continuam as mesmas). `smooth` funde as pontas num ponto suave; sem ele a emenda é um canto.
//! - **Ajustar largura** ([`adjust_width`]): engrossar, afinar, escalar ou largura fixa debaixo do pincel, com queda
//!   suave na borda do pincel (ou a linha inteira, com `whole_line`).
//! - **Redesenhar linha** ([`redraw`]): um caminho novo desenhado de um lugar da linha a outro troca aquele trecho.
//!   **Redesenhar largura** ([`redraw_width`]): passar ao longo da linha põe nela a largura da pressão do traço novo.
//! - **Remover poeira** ([`find_dust_raster`], [`find_dust_vector`]): pontinhos (componentes conexos) e furos
//!   transparentes menores que `size` saem; em camada vetorial, traços menores que `size`. Ou só selecionar a poeira.
//!
//! Para não perder resolução onde o pincel mexe, os trechos tocados ganham pontos novos em cima da curva antes
//! ([`densify`]); os pontos que já existiam mantêm o id estável (`StrokePoint.id`). Tudo em px do cel; a requisição da
//! bridge (`LINE_FIX`, [`line_fix_plan`]) recebe px do canvas e desfaz o movimento da camada.

use crate::context::*;
use crate::points::{cel_placement, insert_on_curve, keep_only, simplify_plan_masked};
use aiai_core::*;
use aiai_paint::{Affine, Image};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// --------------------------------------------------------------------------------------------- curva por comprimento

/// A curva desenhada pelo motor parametrizada pelo comprimento (px do cel).
#[derive(Debug, Clone)]
pub struct CurveParam {
    /// Curva achatada (x, y, pressão), ~1 px entre pontos.
    pub flat: Vec<(f32, f32, f32)>,
    /// Comprimento acumulado até cada ponto de `flat`.
    pub cum: Vec<f32>,
    /// Comprimento até cada ponto de controle.
    pub ctrl: Vec<f32>,
    pub total: f32,
}

impl CurveParam {
    pub fn of(s: &Stroke) -> Option<CurveParam> {
        let flat = aiai_paint::flatten_points(s);
        if flat.len() < 2 || s.points.len() < 2 {
            return None;
        }
        let mut cum = Vec::with_capacity(flat.len());
        let mut acc = 0.0f32;
        cum.push(0.0);
        for w in flat.windows(2) {
            acc += (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
            cum.push(acc);
        }
        // onde cada ponto de controle cai na curva (ela passa por eles, em ordem)
        let mut ctrl = Vec::with_capacity(s.points.len());
        let mut from = 0usize;
        for q in &s.points {
            let mut best = (f32::MAX, from);
            for (k, f) in flat.iter().enumerate().skip(from) {
                let d = (f.0 - q.x).hypot(f.1 - q.y);
                if d < best.0 {
                    best = (d, k);
                }
                if d < 1e-3 {
                    break;
                }
            }
            ctrl.push(cum[best.1]);
            from = best.1;
        }
        Some(CurveParam { flat, cum, ctrl, total: acc })
    }

    /// Lugar da curva mais perto de (x, y): (distância, comprimento, x, y).
    pub fn project(&self, x: f32, y: f32) -> (f32, f32, f32, f32) {
        let mut best = (f32::MAX, 0.0, x, y);
        for k in 0..self.flat.len() - 1 {
            let (a, b) = (self.flat[k], self.flat[k + 1]);
            let (d, t) = seg_dist(x, y, (a.0, a.1), (b.0, b.1));
            if d < best.0 {
                best = (d, self.cum[k] + (self.cum[k + 1] - self.cum[k]) * t, a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            }
        }
        best
    }

    /// Ponto (x, y, pressão) da curva no comprimento `s`.
    pub fn at(&self, s: f32) -> (f32, f32, f32) {
        let s = s.clamp(0.0, self.total);
        let k = self.cum.partition_point(|&c| c < s).clamp(1, self.flat.len() - 1);
        let (c0, c1) = (self.cum[k - 1], self.cum[k]);
        let t = if c1 > c0 { (s - c0) / (c1 - c0) } else { 0.0 };
        let (a, b) = (self.flat[k - 1], self.flat[k]);
        (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
    }

    /// Valor por ponto de controle (`f(i)`) interpolado no comprimento `s`.
    pub fn ctrl_lerp(&self, s: f32, f: impl Fn(usize) -> f32) -> f32 {
        let n = self.ctrl.len();
        if s <= self.ctrl[0] {
            return f(0);
        }
        for i in 0..n - 1 {
            let (a, b) = (self.ctrl[i], self.ctrl[i + 1]);
            if s <= b {
                let t = if b > a { (s - a) / (b - a) } else { 0.0 };
                return f(i) + (f(i + 1) - f(i)) * t;
            }
        }
        f(n - 1)
    }

    /// Trechos (comprimento inicial, final) da curva que passam a até `radius` do caminho.
    pub fn runs_near(&self, path: &[[f32; 2]], radius: f32) -> Vec<(f32, f32)> {
        let mut runs: Vec<(f32, f32)> = Vec::new();
        let mut open: Option<f32> = None;
        for (k, q) in self.flat.iter().enumerate() {
            let near = path_dist(q.0, q.1, path) <= radius;
            match (near, open) {
                (true, None) => open = Some(self.cum[k]),
                (false, Some(a)) => {
                    runs.push((a, self.cum[k]));
                    open = None;
                }
                _ => {}
            }
        }
        if let Some(a) = open {
            runs.push((a, self.total));
        }
        runs
    }
}

fn seg_dist(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((px - a.0) * dx + (py - a.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    ((px - (a.0 + dx * t)).hypot(py - (a.1 + dy * t)), t)
}

/// Distância do ponto até o caminho do pincel (polilinha; um ponto só vale como clique).
pub fn path_dist(x: f32, y: f32, path: &[[f32; 2]]) -> f32 {
    match path.len() {
        0 => f32::MAX,
        1 => (path[0][0] - x).hypot(path[0][1] - y),
        _ => path.windows(2).map(|w| seg_dist(x, y, (w[0][0], w[0][1]), (w[1][0], w[1][1])).0).fold(f32::MAX, f32::min),
    }
}

/// Peso do pincel no ponto: 1 no meio do caminho, caindo suave até 0 na borda do pincel.
pub fn brush_weight(x: f32, y: f32, path: &[[f32; 2]], radius: f32) -> f32 {
    let t = (1.0 - path_dist(x, y, path) / radius.max(1e-3)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bézier explícita (3n+1 controles) vira mão livre pelos pontos da curva (~3 px), para dar para puxar, inserir e
/// cortar; as outras geometrias voltam iguais.
pub fn editable(s: &Stroke) -> Stroke {
    if s.geometry != StrokeGeometry::Bezier {
        return s.clone();
    }
    let flat = aiai_paint::flatten_points(s);
    let anchors: Vec<f32> = s.points.iter().step_by(3).map(|q| q.width).collect();
    let n = flat.len();
    let mut pts: Vec<StrokePoint> = Vec::new();
    let mut last: Option<(f32, f32)> = None;
    for (k, q) in flat.iter().enumerate() {
        if k + 1 < n && last.is_some_and(|l| (q.0 - l.0).hypot(q.1 - l.1) < 3.0) {
            continue;
        }
        // largura: a das âncoras, pela fração do caminho
        let f = k as f32 / (n - 1).max(1) as f32 * (anchors.len() - 1).max(0) as f32;
        let (i0, i1) = (f.floor() as usize, (f.ceil() as usize).min(anchors.len() - 1));
        let w = anchors[i0] + (anchors[i1] - anchors[i0]) * (f - f.floor());
        pts.push(StrokePoint { x: q.0, y: q.1, pressure: q.2.clamp(0.0, 1.0), tilt: None, t: None, corner: false, width: w, id: 0, handle: None });
        last = Some((q.0, q.1));
    }
    let mut ns = s.clone();
    ns.points = pts;
    ns.geometry = StrokeGeometry::Freehand;
    ns.materialize_point_ids();
    ns
}

/// Põe pontos em cima da curva nos trechos `ranges` (comprimentos; no traço fechado podem passar de 0 ou do total e dão
/// a volta) até que os pontos de controle fiquem a no máximo `step` px de curva um do outro ali.
pub fn densify(s: &mut Stroke, ranges: &[(f32, f32)], step: f32) {
    let step = step.max(1.0);
    let Some(cp) = CurveParam::of(s) else { return };
    let total = cp.total;
    let mut norm: Vec<(f32, f32)> = Vec::new();
    for &(a, b) in ranges {
        if s.closed && total > 0.0 {
            if a < 0.0 {
                norm.push((a + total, total));
            }
            if b > total {
                norm.push((0.0, b - total));
            }
        }
        norm.push((a.max(0.0), b.min(total)));
    }
    let inside = |x: f32| norm.iter().any(|&(a, b)| x >= a - step && x <= b + step);
    let n = s.points.len();
    let segs = if s.closed { n } else { n - 1 };
    let mut want: Vec<(f32, f32)> = Vec::new();
    for i in 0..segs {
        let a = cp.ctrl[i];
        let b = if i + 1 < n { cp.ctrl[i + 1] } else { total };
        let len = b - a;
        if len <= step {
            continue;
        }
        let k = (len / step).ceil() as usize;
        for j in 1..k {
            let at = a + len * j as f32 / k as f32;
            if inside(at) {
                let (x, y, _) = cp.at(at);
                want.push((x, y));
            }
        }
    }
    for (x, y) in want {
        insert_on_curve(s, x, y, step.max(2.0));
    }
}

// ------------------------------------------------------------------------------------------------- puxar linha

/// Opções do Puxar linha.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PinchParams {
    /// Até onde (px de curva, para cada lado do ponto agarrado) a linha acompanha o puxão.
    pub range: f32,
    /// Formato da queda, 0..1: 0 = só perto do ponto agarrado (bico), 1 = arco largo e redondo.
    pub level: f32,
    /// As pontas do traço não saem do lugar.
    pub fix_ends: bool,
}

impl Default for PinchParams {
    fn default() -> Self {
        PinchParams { range: 80.0, level: 0.5, fix_ends: false }
    }
}

/// Queda do puxão na distância relativa `t` (0 = no ponto agarrado, 1 = fim do alcance).
pub fn pinch_falloff(t: f32, level: f32) -> f32 {
    if t >= 1.0 {
        return 0.0;
    }
    let base = 0.5 + 0.5 * (std::f32::consts::PI * t.max(0.0)).cos();
    base.powf(0.5 + 2.5 * (1.0 - level.clamp(0.0, 1.0)))
}

/// Puxa a linha: o lugar da curva mais perto de `grab` anda até `to` e os pontos em volta acompanham pela queda.
pub fn pinch(s: &Stroke, grab: [f32; 2], to: [f32; 2], p: &PinchParams) -> Result<Stroke, String> {
    if !(grab.iter().chain(&to).all(|v| v.is_finite()) && p.range.is_finite() && p.level.is_finite()) {
        return Err("puxar: coordenadas e opções devem ser números finitos".into());
    }
    let mut ns = editable(s);
    let r = p.range.max(2.0);
    let cp = CurveParam::of(&ns).ok_or("puxar: traço sem curva (menos de 2 pontos)")?;
    let (_, s0, gx, gy) = cp.project(grab[0], grab[1]);
    // o lugar agarrado vira ponto de controle (é ele que vai até o cursor)
    if !cp.ctrl.iter().any(|c| (c - s0).abs() < 0.5) {
        insert_on_curve(&mut ns, gx, gy, 2.0);
    }
    densify(&mut ns, &[(s0 - r, s0 + r)], (r / 5.0).max(1.5));
    let cp = CurveParam::of(&ns).ok_or("puxar: traço sem curva")?;
    let (_, s0, _, _) = cp.project(gx, gy);
    let (dx, dy) = (to[0] - grab[0], to[1] - grab[1]);
    let n = ns.points.len();
    for i in 0..n {
        let mut d = (cp.ctrl[i] - s0).abs();
        if ns.closed {
            d = d.min(cp.total - d);
        }
        let mut w = pinch_falloff(d / r, p.level);
        if p.fix_ends && !ns.closed {
            // pontas presas: o peso vai a zero chegando nelas (sem dobra na ponta)
            let end = cp.ctrl[i].min(cp.total - cp.ctrl[i]);
            let t = (end / (r * 0.5).max(1.0)).clamp(0.0, 1.0);
            w *= t * t * (3.0 - 2.0 * t);
        }
        ns.points[i].x += dx * w;
        ns.points[i].y += dy * w;
    }
    Ok(ns)
}

// --------------------------------------------------------------------------------------------------- simplificar

/// Tolerância (px) do Simplificar pela força 0..100 do painel: 0,2 px (quase nada) a 8,2 px.
pub fn simplify_tolerance(strength: f32) -> f32 {
    0.2 + strength.clamp(0.0, 100.0) * 0.08
}

/// Tira os pontos debaixo do pincel que não seguram a forma (a linha não sai mais que `tol` px da de antes). `None` =
/// o pincel não tirou nada deste traço.
pub fn simplify_area(s: &Stroke, path: &[[f32; 2]], radius: f32, tol: f32) -> Result<Option<(Stroke, Vec<u32>)>, String> {
    let ns = editable(s);
    let eligible: Vec<bool> = ns.points.iter().map(|q| path_dist(q.x, q.y, path) <= radius).collect();
    if !eligible.iter().any(|e| *e) {
        return Ok(None);
    }
    let (keep, _) = simplify_plan_masked(&ns, tol.max(0.0), Some(&eligible))?;
    if keep.iter().all(|k| *k) {
        return Ok(None);
    }
    let removed = (0..ns.points.len()).filter(|&i| !keep[i]).map(|i| ns.point_id(i)).collect();
    Ok(Some((keep_only(&ns, &keep), removed)))
}

// ------------------------------------------------------------------------------------------------------ conectar

/// Opções do Conectar.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectParams {
    /// Distância máxima (px) entre as duas pontas que se ligam.
    pub distance: f32,
    /// Emenda suave (as pontas viram um ponto só, sem quina); sem ela, a emenda é um canto.
    pub smooth: bool,
}

impl Default for ConnectParams {
    fn default() -> Self {
        ConnectParams { distance: 24.0, smooth: true }
    }
}

/// O que o Conectar fez: traços que mudaram (ids mantidos) e os que sumiram (entraram em outro).
#[derive(Debug, Clone, Default)]
pub struct ConnectResult {
    pub changed: Vec<Stroke>,
    pub removed: Vec<Id>,
    pub joins: usize,
}

/// Inverte o sentido do traço (pontos, alças e perfil).
pub fn reversed(s: &Stroke) -> Stroke {
    let mut ns = s.clone();
    ns.points.reverse();
    for q in &mut ns.points {
        q.handle = q.handle.map(|h| [-h[0], -h[1]]);
    }
    ns.profile.0 = ns.profile.0.iter().rev().map(|[t, m]| [1.0 - t, *m]).collect();
    ns
}

fn profile_at(profile: &[[f32; 2]], t: f32) -> f32 {
    match profile {
        [] => 1.0,
        [first, ..] if t <= first[0] => first[1],
        _ => {
            for w in profile.windows(2) {
                let ([t0, m0], [t1, m1]) = (w[0], w[1]);
                if t <= t1 {
                    let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
                    return m0 + (m1 - m0) * f;
                }
            }
            profile[profile.len() - 1][1]
        }
    }
}

/// Passa o perfil de largura para os multiplicadores dos pontos (o perfil some): dois traços com perfis diferentes
/// podem virar um só sem mudar de largura.
pub fn bake_profile(s: &mut Stroke) {
    if s.profile.0.is_empty() {
        return;
    }
    if let Some(cp) = CurveParam::of(s) {
        let total = cp.total.max(1e-6);
        for (i, q) in s.points.iter_mut().enumerate() {
            q.width = (q.width * profile_at(&s.profile.0, cp.ctrl[i] / total)).clamp(0.0, 20.0);
        }
    }
    s.profile.0.clear();
}

fn arc_len(s: &Stroke) -> f32 {
    CurveParam::of(s).map(|c| c.total).unwrap_or(0.0)
}

/// Junta `a` (pela ponta `a_start`: true = o começo) com `b` (pela `b_start`). O traço mais comprido dá o id, a cor, a
/// ponta e a largura base; cada ponto continua com a largura em px que tinha.
pub fn join(a: &Stroke, a_start: bool, b: &Stroke, b_start: bool, smooth: bool) -> Stroke {
    let (mut a, mut b) = (editable(a), editable(b));
    bake_profile(&mut a);
    bake_profile(&mut b);
    if a_start {
        a = reversed(&a);
    }
    if !b_start {
        b = reversed(&b);
    }
    // a termina na emenda; b começa nela
    let a_base = arc_len(&a) >= arc_len(&b);
    let px_a: Vec<f32> = (0..a.points.len()).map(|i| aiai_paint::width_at_point(&a, i)).collect();
    let px_b: Vec<f32> = (0..b.points.len()).map(|i| aiai_paint::width_at_point(&b, i)).collect();
    let mut base = if a_base { a.clone() } else { b.clone() };
    base.materialize_point_ids();
    let mut other = if a_base { b.clone() } else { a.clone() };
    let mut next = base.next_point_id();
    for q in &mut other.points {
        q.id = next;
        next += 1;
    }
    let (pa, pb) = if a_base { (base.points.clone(), other.points.clone()) } else { (other.points.clone(), base.points.clone()) };
    let both_poly = a.geometry == StrokeGeometry::Polyline && b.geometry == StrokeGeometry::Polyline;
    let geometry = if both_poly {
        StrokeGeometry::Polyline
    } else if a.geometry == StrokeGeometry::Freehand && b.geometry == StrokeGeometry::Freehand {
        StrokeGeometry::Freehand
    } else {
        StrokeGeometry::Spline
    };
    // polilinha que entra numa curva: os pontos dela viram cantos (os lados continuam retos)
    let mark = |pts: Vec<StrokePoint>, poly: bool| -> Vec<StrokePoint> { pts.into_iter().map(|mut q| { q.corner |= poly && geometry != StrokeGeometry::Polyline; q }).collect() };
    let mut pa = mark(pa, a.geometry == StrokeGeometry::Polyline);
    let mut pb = mark(pb, b.geometry == StrokeGeometry::Polyline);
    let mut px: Vec<f32> = px_a.iter().chain(&px_b).copied().collect();
    let (ea, eb) = (*pa.last().expect("traço com pontos"), pb[0]);
    let gap = (ea.x - eb.x).hypot(ea.y - eb.y);
    if smooth {
        let la = pa.len() - 1;
        pa[la].corner = false;
        pa[la].handle = None;
        pb[0].corner = false;
        pb[0].handle = None;
        let w_min = px_a[la].min(px_b[0]);
        if gap <= (w_min * 0.75).max(1.5) {
            // pontas quase no mesmo lugar: um ponto só, no meio
            pa[la].x = (ea.x + eb.x) * 0.5;
            pa[la].y = (ea.y + eb.y) * 0.5;
            pa[la].pressure = (ea.pressure + eb.pressure) * 0.5;
            px[la] = (px_a[la] + px_b[0]) * 0.5;
            pb.remove(0);
            px.remove(la + 1);
        }
    } else {
        let la = pa.len() - 1;
        pa[la].corner = true;
        pb[0].corner = true;
    }
    let mut out = base;
    out.points = pa.into_iter().chain(pb).collect();
    out.geometry = geometry;
    out.closed = false;
    for (i, w) in px.iter().enumerate() {
        out.points[i].width = aiai_paint::point_width_for(&out, i, *w);
    }
    out
}

/// Fecha o traço ligando a ponta final à inicial.
pub fn close(s: &Stroke, smooth: bool) -> Stroke {
    let mut ns = editable(s);
    ns.materialize_point_ids();
    let n = ns.points.len();
    let (a, b) = (ns.points[0], ns.points[n - 1]);
    let w = aiai_paint::width_at_point(&ns, 0).min(aiai_paint::width_at_point(&ns, n - 1));
    if n >= 3 && (a.x - b.x).hypot(a.y - b.y) <= (w * 0.75).max(1.5) {
        ns.points.pop();
    }
    ns.closed = true;
    if !smooth {
        ns.points[0].corner = true;
        let l = ns.points.len() - 1;
        ns.points[l].corner = true;
    }
    ns
}

/// Liga as pontas debaixo do pincel (caminho `path`, raio `radius`) que estão a até `distance` uma da outra; a mais
/// perto primeiro, até não sobrar par.
pub fn connect(strokes: &[Stroke], path: &[[f32; 2]], radius: f32, p: &ConnectParams) -> ConnectResult {
    let mut cur: Vec<Stroke> = strokes.to_vec();
    let mut changed: Vec<Id> = Vec::new();
    let mut res = ConnectResult::default();
    loop {
        // pontas de traços abertos debaixo do pincel: (traço, é o começo, x, y)
        let mut ends: Vec<(usize, bool, f32, f32)> = Vec::new();
        for (k, s) in cur.iter().enumerate() {
            if s.closed || s.points.len() < 2 {
                continue;
            }
            let (f, l) = (s.points[0], s.points[s.points.len() - 1]);
            for (start, q) in [(true, f), (false, l)] {
                if path_dist(q.x, q.y, path) <= radius {
                    ends.push((k, start, q.x, q.y));
                }
            }
        }
        let mut best: Option<(f32, usize, usize)> = None;
        for i in 0..ends.len() {
            for j in i + 1..ends.len() {
                let (a, b) = (ends[i], ends[j]);
                if a.0 == b.0 && cur[a.0].points.len() < 3 {
                    continue;
                }
                let d = (a.2 - b.2).hypot(a.3 - b.3);
                if d <= p.distance && best.is_none_or(|x| d < x.0) {
                    best = Some((d, i, j));
                }
            }
        }
        let Some((_, i, j)) = best else { break };
        let (a, b) = (ends[i], ends[j]);
        if a.0 == b.0 {
            let id = cur[a.0].id.clone();
            cur[a.0] = close(&cur[a.0], p.smooth);
            if !changed.contains(&id) {
                changed.push(id);
            }
        } else {
            let merged = join(&cur[a.0], a.1, &cur[b.0], b.1, p.smooth);
            let gone = if merged.id == cur[a.0].id { b.0 } else { a.0 };
            let keep = if gone == a.0 { b.0 } else { a.0 };
            let gone_id = cur[gone].id.clone();
            changed.retain(|x| x != &gone_id);
            res.removed.push(gone_id);
            if !changed.contains(&merged.id) {
                changed.push(merged.id.clone());
            }
            cur[keep] = merged;
            cur.remove(gone);
        }
        res.joins += 1;
    }
    res.changed = cur.into_iter().filter(|s| changed.contains(&s.id)).collect();
    res
}

// -------------------------------------------------------------------------------------------------- largura

/// Como o Ajustar largura muda a largura debaixo do pincel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum WidthMode {
    /// Soma `amount` px.
    #[default]
    Widen,
    /// Tira `amount` px.
    Narrow,
    /// Multiplica por `amount`.
    Scale,
    /// Leva a `amount` px.
    Fixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WidthParams {
    pub mode: WidthMode,
    pub amount: f32,
    /// A linha inteira de cada traço tocado (sem queda pelo pincel).
    pub whole_line: bool,
}

/// Engrossa/afina/escala/fixa a largura debaixo do pincel. `None` = o pincel não pegou o traço.
pub fn adjust_width(s: &Stroke, path: &[[f32; 2]], radius: f32, p: &WidthParams) -> Option<Stroke> {
    if !p.amount.is_finite() {
        return None;
    }
    let mut ns = editable(s);
    let cp = CurveParam::of(&ns)?;
    let runs = cp.runs_near(path, radius);
    if runs.is_empty() {
        return None;
    }
    if !p.whole_line {
        let pad = radius;
        densify(&mut ns, &runs.iter().map(|&(a, b)| (a - pad, b + pad)).collect::<Vec<_>>(), (radius * 0.5).max(1.5));
    }
    let mut any = false;
    for i in 0..ns.points.len() {
        let q = ns.points[i];
        let w = if p.whole_line { 1.0 } else { brush_weight(q.x, q.y, path, radius) };
        if w <= 0.0 {
            continue;
        }
        let px0 = aiai_paint::width_at_point(&ns, i);
        let px1 = match p.mode {
            WidthMode::Widen => px0 + p.amount * w,
            WidthMode::Narrow => px0 - p.amount * w,
            WidthMode::Scale => px0 * p.amount.max(0.0).powf(w),
            WidthMode::Fixed => px0 + (p.amount - px0) * w,
        }
        .max(0.05);
        ns.points[i].width = aiai_paint::point_width_for(&ns, i, px1);
        any = true;
    }
    any.then_some(ns)
}

// ------------------------------------------------------------------------------------------------- redesenhar

/// Largura (fração da largura base) que a pressão dá no Redesenhar largura: a curva da G-pen, para qualquer ponta.
pub fn pressure_width(p: f32) -> f32 {
    0.12 + 0.88 * p.clamp(0.0, 1.0).powf(0.8)
}

/// Douglas-Peucker num caminho desenhado (mantém pontas).
fn rdp_path(p: &[[f32; 3]], tol: f32) -> Vec<[f32; 3]> {
    fn go(p: &[[f32; 3]], a: usize, b: usize, tol: f32, keep: &mut [bool]) {
        if b <= a + 1 {
            return;
        }
        let (mut best, mut idx) = (0.0f32, a);
        for i in a + 1..b {
            let d = seg_dist(p[i][0], p[i][1], (p[a][0], p[a][1]), (p[b][0], p[b][1])).0;
            if d > best {
                (best, idx) = (d, i);
            }
        }
        if best > tol {
            keep[idx] = true;
            go(p, a, idx, tol, keep);
            go(p, idx, b, tol, keep);
        }
    }
    if p.len() < 3 {
        return p.to_vec();
    }
    let mut keep = vec![false; p.len()];
    keep[0] = true;
    keep[p.len() - 1] = true;
    go(p, 0, p.len() - 1, tol, &mut keep);
    p.iter().zip(&keep).filter(|(_, k)| **k).map(|(q, _)| *q).collect()
}

/// O traço de `strokes` em que o caminho começa E termina (as duas pontas a até `tol` da curva): o de menor soma.
pub fn redraw_pick(strokes: &[Stroke], path: &[[f32; 3]], tol: f32) -> Option<usize> {
    let (a, b) = (path.first()?, path.last()?);
    strokes
        .iter()
        .enumerate()
        .filter_map(|(k, s)| {
            let cp = CurveParam::of(&editable(s))?;
            let (da, sa, _, _) = cp.project(a[0], a[1]);
            let (db, sb, _, _) = cp.project(b[0], b[1]);
            (da <= tol && db <= tol && (sb - sa).abs() >= 2.0).then_some((k, da + db))
        })
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map(|x| x.0)
}

/// Troca o trecho da linha entre onde o caminho começa e onde termina pelo caminho (x, y, pressão), emendado na curva
/// nas duas pontas. Pressão do caminho toda igual a 1 (mouse) = a largura de antes, interpolada entre as emendas.
pub fn redraw(s: &Stroke, path: &[[f32; 3]], tol: f32) -> Result<Stroke, String> {
    if path.len() < 2 || path.iter().any(|q| !(q[0].is_finite() && q[1].is_finite() && q[2].is_finite())) {
        return Err("redesenhar: o caminho precisa de 2 pontos ou mais (números finitos)".into());
    }
    let mut ns = editable(s);
    let cp = CurveParam::of(&ns).ok_or("redesenhar: traço sem curva")?;
    let mut path: Vec<[f32; 3]> = path.to_vec();
    let (da, mut sa, _, _) = cp.project(path[0][0], path[0][1]);
    let last = path[path.len() - 1];
    let (db, mut sb, _, _) = cp.project(last[0], last[1]);
    if da > tol || db > tol {
        return Err(format!("redesenhar: o caminho deve começar e terminar em cima da linha (a até {tol:.0} px; começa a {da:.1}, termina a {db:.1})"));
    }
    if sa > sb {
        path.reverse();
        std::mem::swap(&mut sa, &mut sb);
    }
    if sb - sa < 2.0 {
        return Err("redesenhar: desenhe ao longo de um trecho da linha (de um lugar dela a outro)".into());
    }
    let mouse = path.iter().all(|q| (q[2] - 1.0).abs() < 1e-3);
    let path = rdp_path(&path, 0.6);
    // emendas: os lugares da curva em sa e sb
    let (ax, ay, apr) = cp.at(sa);
    let (bx, by, bpr) = cp.at(sb);
    let wa = cp.ctrl_lerp(sa, |i| ns.points[i].width);
    let wb = cp.ctrl_lerp(sb, |i| ns.points[i].width);
    let plen: Vec<f32> = {
        let mut acc = 0.0;
        let mut v = vec![0.0];
        for w in path.windows(2) {
            acc += (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            v.push(acc);
        }
        v
    };
    let ptotal = plen.last().copied().unwrap_or(0.0).max(1e-6);
    ns.materialize_point_ids();
    let poly = ns.geometry == StrokeGeometry::Polyline;
    let mut next = ns.next_point_id();
    let mut mid: Vec<StrokePoint> = Vec::new();
    let n = path.len();
    for (k, q) in path.iter().enumerate() {
        let f = plen[k] / ptotal;
        let (x, y) = match k {
            0 => (ax, ay),
            _ if k + 1 == n => (bx, by),
            _ => (q[0], q[1]),
        };
        let pressure = if mouse { apr + (bpr - apr) * f } else { q[2] };
        mid.push(StrokePoint { x, y, pressure: pressure.clamp(0.0, 1.0), tilt: None, t: None, corner: false, width: wa + (wb - wa) * f, id: next, handle: None });
        next += 1;
    }
    let eps = 0.5;
    let mut pts: Vec<StrokePoint> = Vec::new();
    for (i, q) in ns.points.iter().enumerate() {
        if cp.ctrl[i] < sa - eps {
            pts.push(StrokePoint { corner: q.corner || poly, ..*q });
        }
    }
    pts.extend(mid);
    for (i, q) in ns.points.iter().enumerate() {
        // no traço fechado o ponto 0 também fica no fim (comprimento 0 = total)
        if cp.ctrl[i] > sb + eps && !(i == 0 && ns.closed) {
            pts.push(StrokePoint { corner: q.corner || poly, ..*q });
        }
    }
    ns.points = pts;
    if poly {
        ns.geometry = StrokeGeometry::Spline;
    }
    ns.validate()?;
    Ok(ns)
}

/// Põe na linha, debaixo do caminho, a largura da pressão do traço novo (`pressure_width`), com queda suave na borda
/// do pincel. `None` = o caminho não passou perto do traço.
pub fn redraw_width(s: &Stroke, path: &[[f32; 3]], radius: f32) -> Option<Stroke> {
    let xy: Vec<[f32; 2]> = path.iter().map(|q| [q[0], q[1]]).collect();
    let mut ns = editable(s);
    let cp = CurveParam::of(&ns)?;
    let runs = cp.runs_near(&xy, radius);
    if runs.is_empty() {
        return None;
    }
    densify(&mut ns, &runs.iter().map(|&(a, b)| (a - radius, b + radius)).collect::<Vec<_>>(), (radius * 0.5).max(2.0));
    let mut any = false;
    for i in 0..ns.points.len() {
        let q = ns.points[i];
        let w = brush_weight(q.x, q.y, &xy, radius);
        if w <= 0.0 {
            continue;
        }
        // pressão do ponto do caminho mais perto
        let pr = path.iter().min_by(|a, b| (a[0] - q.x).hypot(a[1] - q.y).total_cmp(&(b[0] - q.x).hypot(b[1] - q.y))).map(|a| a[2]).unwrap_or(1.0);
        let px0 = aiai_paint::width_at_point(&ns, i);
        let target = ns.width * pressure_width(pr);
        ns.points[i].width = aiai_paint::point_width_for(&ns, i, px0 + (target - px0) * w);
        any = true;
    }
    any.then_some(ns)
}

// ---------------------------------------------------------------------------------------------- remover poeira

/// O que a poeira é.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum DustMode {
    /// Pontinhos de tinta soltos no transparente.
    #[default]
    Opaque,
    /// Furos transparentes dentro da tinta (são tapados com a cor em volta).
    Holes,
    /// Os dois.
    Both,
}

/// Um grão de poeira raster: os pixels, se é furo (e a cor que o tapa) e a caixa [x0, y0, x1, y1).
#[derive(Debug, Clone)]
pub struct Speck {
    pub pixels: Vec<u32>,
    pub hole: bool,
    pub color: [f32; 4],
    pub bbox: [i32; 4],
}

const INK: f32 = 1.0 / 255.0;

/// Componentes conexos (8 vizinhos) dos pixels com `is(i)`; devolve (pixels, caixa, encosta na borda da imagem).
fn components(w: u32, h: u32, is: impl Fn(usize) -> bool) -> Vec<(Vec<u32>, [i32; 4], bool)> {
    let (w, h) = (w as i32, h as i32);
    let mut seen = vec![false; (w * h) as usize];
    let mut out = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    for start in 0..(w * h) as usize {
        if seen[start] || !is(start) {
            continue;
        }
        seen[start] = true;
        stack.push(start as u32);
        let mut px = Vec::new();
        let mut bb = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        let mut border = false;
        while let Some(i) = stack.pop() {
            px.push(i);
            let (x, y) = (i as i32 % w, i as i32 / w);
            bb = [bb[0].min(x), bb[1].min(y), bb[2].max(x + 1), bb[3].max(y + 1)];
            border |= x == 0 || y == 0 || x == w - 1 || y == h - 1;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if (dx, dy) == (0, 0) || nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    let j = (ny * w + nx) as usize;
                    if !seen[j] && is(j) {
                        seen[j] = true;
                        stack.push(j as u32);
                    }
                }
            }
        }
        out.push((px, bb, border));
    }
    out
}

/// Poeira de uma imagem raster: componentes de tinta (ou furos transparentes fechados) cuja caixa não passa de `size`
/// px. `within` (cobertura do tamanho da imagem) limita aos grãos com algum pixel dentro.
pub fn find_dust_raster(img: &Image, size: f32, mode: DustMode, within: Option<&[f32]>) -> Vec<Speck> {
    let (w, h) = (img.width, img.height);
    let small = |bb: &[i32; 4]| ((bb[2] - bb[0]).max(bb[3] - bb[1]) as f32) <= size;
    let inside = |px: &[u32]| within.is_none_or(|m| px.iter().any(|&i| m.get(i as usize).copied().unwrap_or(0.0) > 0.5));
    let mut out = Vec::new();
    if matches!(mode, DustMode::Opaque | DustMode::Both) {
        for (px, bb, _) in components(w, h, |i| img.data[i][3] > INK) {
            if small(&bb) && inside(&px) {
                out.push(Speck { pixels: px, hole: false, color: [0.0; 4], bbox: bb });
            }
        }
    }
    if matches!(mode, DustMode::Holes | DustMode::Both) {
        for (px, bb, border) in components(w, h, |i| img.data[i][3] < 0.5) {
            if border || !small(&bb) || !inside(&px) {
                continue;
            }
            // cor de tapar: a média (sem pré-multiplicar) da tinta em volta
            let mut acc = [0.0f32; 4];
            let set: std::collections::HashSet<u32> = px.iter().copied().collect();
            for &i in &px {
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let j = (ny as u32 * w + nx as u32) as usize;
                    let c = img.data[j];
                    if !set.contains(&(j as u32)) && c[3] >= 0.5 {
                        acc = [acc[0] + c[0] / c[3], acc[1] + c[1] / c[3], acc[2] + c[2] / c[3], acc[3] + 1.0];
                    }
                }
            }
            if acc[3] > 0.0 {
                out.push(Speck { pixels: px, hole: true, color: [acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3], 1.0], bbox: bb });
            }
        }
    }
    out
}

/// Tira a poeira achada da imagem (pontinhos ficam transparentes; furos, tapados).
pub fn apply_dust_raster(img: &mut Image, specks: &[Speck]) {
    for s in specks {
        for &i in &s.pixels {
            img.data[i as usize] = if s.hole { s.color } else { [0.0; 4] };
        }
    }
}

/// Caixa [x0, y0, x1, y1] do traço desenhado (com a largura).
pub fn stroke_bounds(s: &Stroke) -> Option<[f32; 4]> {
    let smp = aiai_paint::sample_stroke(s);
    if smp.is_empty() {
        return None;
    }
    Some(smp.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, q| [b[0].min(q.x - q.r), b[1].min(q.y - q.r), b[2].max(q.x + q.r), b[3].max(q.y + q.r)]))
}

/// Traços que são poeira: a caixa desenhada não passa de `size` px. `within(x, y)` = o meio do traço conta.
pub fn find_dust_vector(strokes: &[Stroke], size: f32, within: Option<&dyn Fn(f32, f32) -> bool>) -> Vec<Id> {
    strokes
        .iter()
        .filter_map(|s| {
            let b = stroke_bounds(s)?;
            let small = (b[2] - b[0]).max(b[3] - b[1]) <= size;
            let mid = ((b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5);
            (small && within.is_none_or(|f| f(mid.0, mid.1))).then(|| s.id.clone())
        })
        .collect()
}

/// O que a varredura de poeira achou no cel exposto de uma camada (raster ou vetorial).
#[derive(Debug, Clone)]
pub struct DustScan {
    pub cel: Id,
    /// Traços-poeira (camada vetorial).
    pub vector: Vec<Id>,
    vector_boxes: Vec<[i32; 4]>,
    /// Grãos (px do cel) e a imagem do cel em que foram achados (camada raster).
    pub specks: Vec<Speck>,
    pub img: Option<Image>,
    /// Cobertura (px do cel, tamanho do canvas) dos traços-poeira, para selecionar.
    vector_cov: Option<Vec<f32>>,
    /// cel → canvas (`None` = identidade).
    pub fwd: Option<Affine>,
    pub size: (u32, u32),
}

impl DustScan {
    pub fn count(&self) -> usize {
        self.vector.len() + self.specks.len()
    }

    /// Caixas [x0, y0, x1, y1) da poeira no cel.
    pub fn boxes(&self) -> Vec<[i32; 4]> {
        self.vector_boxes.iter().copied().chain(self.specks.iter().map(|s| s.bbox)).collect()
    }

    /// Cobertura da poeira no CANVAS (`size`), 0..1 por pixel: o que o "Selecionar poeira" seleciona.
    pub fn canvas_coverage(&self) -> Vec<f32> {
        let (w, h) = self.size;
        let mut cel = self.vector_cov.clone().unwrap_or_else(|| vec![0.0; (w * h) as usize]);
        for s in &self.specks {
            for &i in &s.pixels {
                cel[i as usize] = 1.0;
            }
        }
        match self.fwd.and_then(|f| f.inverse()) {
            None => cel,
            Some(inv) => (0..w * h)
                .map(|i| {
                    let (x, y) = inv.apply((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
                    let (cx, cy) = (x.floor() as i64, y.floor() as i64);
                    if cx < 0 || cy < 0 || cx >= w as i64 || cy >= h as i64 { 0.0 } else { cel[(cy as u32 * w + cx as u32) as usize] }
                })
                .collect(),
        }
    }
}

/// Acha a poeira do cel exposto da camada: grãos com até `size` px do cel. `within` = cobertura do CANVAS (tamanho do
/// canvas) que limita a busca (seleção ou caixa); `None` = a camada inteira.
#[allow(clippy::too_many_arguments)]
pub fn dust_scan(p: &Project, blobs: &BlobStore, cut_id: &Id, layer_id: &Id, frame: u32, size: f32, mode: DustMode, within: Option<&[f32]>) -> ToolResult<DustScan> {
    let cut = cut_data(p, cut_id)?;
    let layer = layer_data(p, layer_id)?;
    if layer.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    let node = exposed_cel(p, cut, layer_id, frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {frame}")))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    let (w, h) = canvas_size(p, cut);
    if within.is_some_and(|m| m.len() != (w * h) as usize) {
        return Err(ToolError::Invalid(format!("a área da poeira não tem o tamanho do canvas ({w}x{h})")));
    }
    let place = cel_placement(p, cut_id, layer_id, frame)?;
    let fwd = (!place.is_identity()).then_some(place);
    // cobertura do canvas lida num ponto do cel
    let at = |m: &[f32], x: f32, y: f32| -> f32 {
        let (cx, cy) = fwd.map(|f| f.apply(x, y)).unwrap_or((x, y));
        if cx < 0.0 || cy < 0.0 || cx >= w as f32 || cy >= h as f32 { 0.0 } else { m[(cy as u32 * w + cx as u32) as usize] }
    };
    let mut scan = DustScan { cel: node.id.clone(), vector: Vec::new(), vector_boxes: Vec::new(), specks: Vec::new(), img: None, vector_cov: None, fwd, size: (w, h) };
    match layer.kind {
        LayerType::Vector => {
            let inside = |x: f32, y: f32| within.is_none_or(|m| at(m, x, y) > 0.5);
            scan.vector = find_dust_vector(&cel.strokes, size, Some(&inside));
            let mut cov = vec![0.0f32; (w * h) as usize];
            for s in cel.strokes.iter().filter(|s| scan.vector.contains(&s.id)) {
                if let Some(b) = stroke_bounds(s) {
                    scan.vector_boxes.push([b[0].floor() as i32, b[1].floor() as i32, b[2].ceil() as i32, b[3].ceil() as i32]);
                }
                if let Some(c) = aiai_paint::rasterize_stroke(&aiai_paint::sample_stroke(s), s.aliased, w, h) {
                    for yy in 0..c.height {
                        for xx in 0..c.width {
                            let (x, y) = (c.x0 + xx as i32, c.y0 + yy as i32);
                            if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                                let i = (y as u32 * w + x as u32) as usize;
                                cov[i] = cov[i].max(c.cov[(yy * c.width + xx) as usize]);
                            }
                        }
                    }
                }
            }
            scan.vector_cov = Some(cov);
        }
        LayerType::Raster => {
            let img = crate::paint::cel_raster(blobs, cel, w, h)?;
            let mask: Option<Vec<f32>> = within.map(|m| (0..w * h).map(|i| at(m, (i % w) as f32 + 0.5, (i / w) as f32 + 0.5)).collect());
            scan.specks = find_dust_raster(&img, size, mode, mask.as_deref());
            scan.img = Some(img);
        }
        other => return Err(ToolError::Invalid(format!("remover poeira vale em camada raster ou vetorial (a camada é {other:?})"))),
    }
    Ok(scan)
}

/// Comando que tira a poeira achada (um passo de desfazer); `None` = não havia poeira.
pub fn dust_command(blobs: &mut BlobStore, scan: DustScan) -> Option<Command> {
    if !scan.vector.is_empty() {
        return Some(Command::RemoveStrokes { cel: scan.cel, ids: scan.vector });
    }
    let mut img = scan.img?;
    if scan.specks.is_empty() {
        return None;
    }
    let bounds = scan.specks.iter().fold([i32::MAX, i32::MAX, i32::MIN, i32::MIN], |b, s| [b[0].min(s.bbox[0]), b[1].min(s.bbox[1]), b[2].max(s.bbox[2]), b[3].max(s.bbox[3])]);
    apply_dust_raster(&mut img, &scan.specks);
    Some(Command::SetTiles { cel: scan.cel, tiles: crate::paint::tiles_for_region(blobs, &img, bounds) })
}

// --------------------------------------------------------------------------------------------------- requisição

/// Ferramenta do Corrigir linha pedida pela bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineFixOp {
    /// Puxar linha: `stroke` (ou o mais perto de `from`), `from` → `to`, `range`, `level`, `fix_ends`.
    Pinch,
    /// Simplificar debaixo do pincel: `path`, `radius`, `strength` (0..100) ou `tolerance` (px).
    Simplify,
    /// Conectar pontas debaixo do pincel: `path`, `radius`, `distance`, `smooth`.
    Connect,
    /// Ajustar largura: `path`, `radius`, `mode` (widen/narrow/scale/fixed), `amount`, `whole_line`.
    Width,
    /// Redesenhar linha: `path` (+ `pressure`), `tolerance` (as pontas do caminho em cima da linha).
    Redraw,
    /// Redesenhar largura: `path` + `pressure`, `radius`.
    RedrawWidth,
    /// Remover poeira (ou só achar, com `dry_run`): `size`, `dust_mode`, `rect` opcional.
    Dust,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LineFixRequest {
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    pub op: LineFixOp,
    /// pinch: traço a puxar (sem ele, o mais perto de `from`).
    #[serde(default)]
    pub stroke: Option<Id>,
    /// pinch: onde agarrar e para onde levar (px do canvas).
    #[serde(default)]
    pub from: Option<[f32; 2]>,
    #[serde(default)]
    pub to: Option<[f32; 2]>,
    /// pinch: alcance em px de curva (padrão 80).
    #[serde(default)]
    pub range: Option<f32>,
    /// pinch: formato da queda 0..1 (padrão 0,5).
    #[serde(default)]
    pub level: Option<f32>,
    #[serde(default)]
    pub fix_ends: bool,
    /// Caminho do pincel / do traço novo (px do canvas).
    #[serde(default)]
    pub path: Vec<[f32; 2]>,
    /// redraw/redraw_width: pressão de cada ponto de `path` (0..1; sem ela, 1).
    #[serde(default)]
    pub pressure: Vec<f32>,
    /// Raio do pincel (px do canvas; padrão 15).
    #[serde(default)]
    pub radius: Option<f32>,
    /// simplify: força 0..100 (padrão 25).
    #[serde(default)]
    pub strength: Option<f32>,
    /// simplify: tolerância em px (no lugar da força); redraw: distância máxima das pontas do caminho até a linha (padrão 12).
    #[serde(default)]
    pub tolerance: Option<f32>,
    /// connect: distância máxima entre as pontas (padrão 24).
    #[serde(default)]
    pub distance: Option<f32>,
    /// connect: emenda suave (padrão true).
    #[serde(default)]
    pub smooth: Option<bool>,
    /// width: widen (padrão), narrow, scale, fixed.
    #[serde(default)]
    pub mode: Option<WidthMode>,
    /// width: px (widen/narrow/fixed) ou fator (scale). Padrão 1 px / 1,25×.
    #[serde(default)]
    pub amount: Option<f32>,
    #[serde(default)]
    pub whole_line: bool,
    /// dust: tamanho máximo do grão em px (padrão 6).
    #[serde(default)]
    pub size: Option<f32>,
    #[serde(default)]
    pub dust_mode: Option<DustMode>,
    /// dust: só dentro desta caixa [x0, y0, x1, y1] do canvas.
    #[serde(default)]
    pub rect: Option<[f32; 4]>,
    /// Ensaio: devolve o que mudaria sem mudar o documento.
    #[serde(default)]
    pub dry_run: bool,
}

/// Resultado de um pedido: o comando (nenhum quando nada mudou ou no ensaio da poeira) e o relatório.
#[derive(Debug, Clone)]
pub struct LineFixPlan {
    pub command: Option<Command>,
    pub report: LineFixReport,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LineFixReport {
    /// Traços que mudaram (ids).
    pub changed: Vec<Id>,
    /// Traços que saíram (conectar, poeira vetorial).
    pub removed: Vec<Id>,
    /// Pontos tirados (simplificar), por traço.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_points: Vec<(Id, Vec<u32>)>,
    /// Emendas feitas (conectar).
    pub joins: usize,
    /// Grãos de poeira (dust): quantos e as caixas [x0, y0, x1, y1] no cel.
    pub dust: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dust_boxes: Vec<[i32; 4]>,
}

/// Camada (vetorial ou raster), cel exposto e transformação canvas → cel.
fn target<'a>(p: &'a Project, req: &LineFixRequest) -> ToolResult<(&'a LayerData, &'a Node, &'a CelData, Affine)> {
    let cut = cut_data(p, &req.cut)?;
    let layer = layer_data(p, &req.layer)?;
    if layer.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    let node = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {}", req.frame)))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    let inv = cel_placement(p, &req.cut, &req.layer, req.frame)?.inverse().ok_or_else(|| ToolError::Invalid("o movimento da camada neste quadro não tem inversa (escala 0)".into()))?;
    Ok((layer, node, cel, inv))
}

/// Px do cel por px do canvas.
fn scale_of(inv: &Affine) -> f32 {
    let (a, b) = (inv.apply(0.0, 0.0), inv.apply(1.0, 0.0));
    (b.0 - a.0).hypot(b.1 - a.1).max(1e-6)
}

fn need_path(req: &LineFixRequest) -> ToolResult<()> {
    if req.path.is_empty() || req.path.iter().flatten().any(|v| !v.is_finite()) {
        return Err(ToolError::Invalid(format!("{:?}: 'path' vazio ou com número não finito", req.op).to_lowercase()));
    }
    Ok(())
}

fn positive(v: Option<f32>, default: f32, name: &str) -> ToolResult<f32> {
    let v = v.unwrap_or(default);
    if !(v.is_finite() && v > 0.0) {
        return Err(ToolError::Invalid(format!("'{name}' deve ser um número > 0")));
    }
    Ok(v)
}

/// Monta o comando de um pedido `LINE_FIX` (coordenadas em px do canvas).
pub fn line_fix_plan(p: &Project, blobs: &mut BlobStore, req: &LineFixRequest) -> ToolResult<LineFixPlan> {
    let (layer, node, cel, inv) = target(p, req)?;
    let k = scale_of(&inv);
    let to_cel = |q: [f32; 2]| {
        let (x, y) = inv.apply(q[0], q[1]);
        [x, y]
    };
    let path: Vec<[f32; 2]> = req.path.iter().map(|q| to_cel(*q)).collect();
    let mut rep = LineFixReport::default();
    if req.op == LineFixOp::Dust {
        let size = positive(req.size, 6.0, "size")? * k;
        let mode = req.dust_mode.unwrap_or_default();
        let (w, h) = canvas_size(p, cut_data(p, &req.cut)?);
        // a caixa é do canvas: vira cobertura do canvas (a varredura leva para o cel)
        let within: Option<Vec<f32>> = req.rect.map(|r| {
            let (x0, y0, x1, y1) = (r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3]));
            (0..w * h).map(|i| { let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5); if x >= x0 && y >= y0 && x < x1 && y < y1 { 1.0 } else { 0.0 } }).collect()
        });
        let scan = dust_scan(p, blobs, &req.cut, &req.layer, req.frame, size, mode, within.as_deref())?;
        rep.dust = scan.count();
        rep.dust_boxes = scan.boxes();
        rep.removed = scan.vector.clone();
        let command = if req.dry_run { None } else { dust_command(blobs, scan) };
        return Ok(LineFixPlan { command, report: rep });
    }
    if layer.kind != LayerType::Vector {
        return Err(ToolError::Invalid(format!("{:?} só vale em camada vetorial (a camada é {:?})", req.op, layer.kind).to_lowercase()));
    }
    let radius = positive(req.radius, 15.0, "radius")? * k;
    let pressure = |i: usize| req.pressure.get(i).copied().unwrap_or(1.0);
    let path3: Vec<[f32; 3]> = path.iter().enumerate().map(|(i, q)| [q[0], q[1], pressure(i)]).collect();
    let mut changed: Vec<Stroke> = Vec::new();
    match req.op {
        LineFixOp::Pinch => {
            let (from, to) = (req.from.ok_or_else(|| ToolError::Invalid("pinch: falta 'from'".into()))?, req.to.ok_or_else(|| ToolError::Invalid("pinch: falta 'to'".into()))?);
            let (f, t) = (to_cel(from), to_cel(to));
            let s = match &req.stroke {
                Some(id) => cel.strokes.iter().find(|s| &s.id == id).ok_or_else(|| ToolError::NotFound(format!("traço {id} no cel exposto")))?,
                None => {
                    let tol = positive(req.tolerance, 12.0, "tolerance")? * k;
                    let best = cel.strokes.iter().filter_map(|s| Some((s, CurveParam::of(&editable(s))?.project(f[0], f[1]).0))).min_by(|a, b| a.1.total_cmp(&b.1));
                    match best {
                        Some((s, d)) if d <= tol => s,
                        _ => return Err(ToolError::NotFound(format!("pinch: nenhum traço a até {} px de {from:?}", tol / k))),
                    }
                }
            };
            let params = PinchParams { range: positive(req.range, 80.0, "range")? * k, level: req.level.unwrap_or(0.5).clamp(0.0, 1.0), fix_ends: req.fix_ends };
            changed.push(pinch(s, f, t, &params).map_err(ToolError::Invalid)?);
        }
        LineFixOp::Simplify => {
            need_path(req)?;
            let tol = match req.tolerance {
                Some(t) => positive(Some(t), 1.0, "tolerance")? * k,
                None => simplify_tolerance(req.strength.unwrap_or(25.0)) * k,
            };
            for s in &cel.strokes {
                if let Some((ns, gone)) = simplify_area(s, &path, radius, tol).map_err(ToolError::Invalid)? {
                    rep.removed_points.push((ns.id.clone(), gone));
                    changed.push(ns);
                }
            }
        }
        LineFixOp::Connect => {
            need_path(req)?;
            let params = ConnectParams { distance: positive(req.distance, 24.0, "distance")? * k, smooth: req.smooth.unwrap_or(true) };
            let r = connect(&cel.strokes, &path, radius, &params);
            rep.joins = r.joins;
            rep.removed = r.removed;
            changed = r.changed;
        }
        LineFixOp::Width => {
            need_path(req)?;
            let mode = req.mode.unwrap_or_default();
            let amount = match mode {
                WidthMode::Scale => positive(req.amount, 1.25, "amount")?,
                _ => positive(req.amount, 1.0, "amount")? * k,
            };
            let params = WidthParams { mode, amount, whole_line: req.whole_line };
            changed = cel.strokes.iter().filter_map(|s| adjust_width(s, &path, radius, &params)).collect();
        }
        LineFixOp::Redraw => {
            need_path(req)?;
            let tol = positive(req.tolerance, 12.0, "tolerance")? * k;
            let i = redraw_pick(&cel.strokes, &path3, tol).ok_or_else(|| ToolError::NotFound(format!("redraw: o caminho não começa e termina em cima de um mesmo traço (a até {} px)", tol / k)))?;
            changed.push(redraw(&cel.strokes[i], &path3, tol).map_err(ToolError::Invalid)?);
        }
        LineFixOp::RedrawWidth => {
            need_path(req)?;
            changed = cel.strokes.iter().filter_map(|s| redraw_width(s, &path3, radius)).collect();
        }
        LineFixOp::Dust => unreachable!(),
    }
    for s in &changed {
        s.validate().map_err(|e| ToolError::Invalid(format!("o traço {} ficaria inválido: {e}", s.id)))?;
    }
    rep.changed = changed.iter().map(|s| s.id.clone()).collect();
    if changed.is_empty() || req.dry_run {
        return Ok(LineFixPlan { command: None, report: rep });
    }
    Ok(LineFixPlan { command: Some(fix_command(&node.id, changed, rep.removed.clone())), report: rep })
}

/// Comando de um passo: troca os traços mudados e tira os que sumiram.
pub fn fix_command(cel: &Id, changed: Vec<Stroke>, removed: Vec<Id>) -> Command {
    let mut v = Vec::new();
    if !changed.is_empty() {
        v.push(Command::ReplaceStrokes { cel: cel.clone(), strokes: changed });
    }
    if !removed.is_empty() {
        v.push(Command::RemoveStrokes { cel: cel.clone(), ids: removed });
    }
    if v.len() == 1 {
        return v.remove(0);
    }
    Command::Batch { commands: v, label: Some("Corrigir linha".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> StrokePoint {
        StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }
    }

    fn stroke(id: &str, pts: &[(f32, f32)], geometry: StrokeGeometry) -> Stroke {
        let mut s: Stroke = serde_json::from_value(serde_json::json!({"id": id, "points": [], "width": 4.0, "tip": "marker", "color": {"type": "direct", "rgba": [0, 0, 0, 1]}})).unwrap();
        s.points = pts.iter().map(|&(x, y)| pt(x, y)).collect();
        s.geometry = geometry;
        s
    }

    fn y_at(s: &Stroke, x: f32) -> f32 {
        aiai_paint::flatten_points(s).iter().min_by(|a, b| (a.0 - x).abs().total_cmp(&(b.0 - x).abs())).unwrap().1
    }

    #[test]
    fn curve_param_maps_control_points_and_projects() {
        let s = stroke("a", &[(0.0, 0.0), (100.0, 0.0), (200.0, 0.0)], StrokeGeometry::Polyline);
        let cp = CurveParam::of(&s).unwrap();
        assert!((cp.total - 200.0).abs() < 0.5);
        assert!((cp.ctrl[1] - 100.0).abs() < 0.6, "{:?}", cp.ctrl);
        let (d, at, x, _) = cp.project(150.0, 7.0);
        assert!((d - 7.0).abs() < 0.1 && (at - 150.0).abs() < 0.6 && (x - 150.0).abs() < 0.6);
        assert!((cp.at(50.0).0 - 50.0).abs() < 0.6);
    }

    #[test]
    fn densify_adds_points_only_in_the_range_and_keeps_ids_and_shape() {
        let mut s = stroke("a", &[(0.0, 0.0), (200.0, 0.0)], StrokeGeometry::Spline);
        densify(&mut s, &[(80.0, 120.0)], 10.0);
        assert!(s.points.len() > 4, "{}", s.points.len());
        assert!(s.points.iter().all(|q| q.y.abs() < 1e-3));
        assert!(s.points.iter().skip(1).take(s.points.len() - 2).all(|q| q.x > 60.0 && q.x < 140.0), "só perto do trecho");
        assert_eq!((s.point_id(0), s.point_id(s.points.len() - 1)), (1, 2), "as pontas mantêm os ids");
    }

    #[test]
    fn pinch_pulls_with_falloff_and_can_fix_the_ends() {
        let s = stroke("a", &[(0.0, 0.0), (100.0, 0.0), (200.0, 0.0), (300.0, 0.0)], StrokeGeometry::Spline);
        let p = PinchParams { range: 80.0, level: 0.5, fix_ends: false };
        let ns = pinch(&s, [150.0, 0.0], [150.0, 30.0], &p).unwrap();
        assert!((y_at(&ns, 150.0) - 30.0).abs() < 1.5, "o lugar agarrado vai até o cursor: {}", y_at(&ns, 150.0));
        assert!(y_at(&ns, 110.0) > 1.0 && y_at(&ns, 110.0) < 29.0, "queda no meio do alcance");
        assert!(y_at(&ns, 20.0).abs() < 0.5 && y_at(&ns, 280.0).abs() < 0.5, "fora do alcance não muda");
        // agarrado perto da ponta: sem prender, a ponta anda; presa, não
        let free = pinch(&s, [290.0, 0.0], [290.0, 40.0], &p).unwrap();
        assert!(free.points.last().unwrap().y > 20.0);
        let fixed = pinch(&s, [290.0, 0.0], [290.0, 40.0], &PinchParams { fix_ends: true, ..p }).unwrap();
        assert!(fixed.points.last().unwrap().y.abs() < 1e-3, "ponta presa");
        // nível baixo = bico; alto = arco largo
        let sharp = pinch(&s, [150.0, 0.0], [150.0, 30.0], &PinchParams { level: 0.0, ..p }).unwrap();
        let round = pinch(&s, [150.0, 0.0], [150.0, 30.0], &PinchParams { level: 1.0, ..p }).unwrap();
        assert!(y_at(&round, 105.0) > y_at(&sharp, 105.0) + 3.0, "{} {}", y_at(&round, 105.0), y_at(&sharp, 105.0));
    }

    #[test]
    fn simplify_area_only_touches_points_under_the_brush() {
        let s = stroke("a", &(0..=40).map(|i| (i as f32 * 10.0, 0.0)).collect::<Vec<_>>(), StrokeGeometry::Freehand);
        let (ns, gone) = simplify_area(&s, &[[100.0, 0.0], [200.0, 0.0]], 15.0, 0.5).unwrap().unwrap();
        assert!(!gone.is_empty());
        // os pontos fora do pincel ficam (ids 1..=9 e 22..=41)
        for id in (1..=9).chain(23..=41) {
            assert!(ns.point_index(id).is_some(), "id {id} fora do pincel saiu");
        }
        assert!(gone.iter().all(|&id| (10..=22).contains(&id)), "{gone:?}");
        assert!(simplify_area(&s, &[[100.0, 300.0]], 15.0, 0.5).unwrap().is_none(), "longe do traço: nada");
    }

    #[test]
    fn connect_joins_nearby_ends_keeping_pixel_widths() {
        let mut a = stroke("a", &[(0.0, 0.0), (100.0, 0.0)], StrokeGeometry::Spline);
        let mut b = stroke("b", &[(240.0, 0.0), (110.0, 2.0)], StrokeGeometry::Spline);
        b.width = 8.0;
        a.points[1].width = 1.5; // 6 px na ponta de a
        let r = connect(&[a.clone(), b.clone()], &[[105.0, 0.0]], 12.0, &ConnectParams { distance: 20.0, smooth: true });
        assert_eq!(r.joins, 1);
        assert_eq!(r.removed, vec![Id::from("a")], "o mais comprido (b) fica");
        let m = &r.changed[0];
        assert_eq!(m.id, Id::from("b"));
        assert_eq!(m.points.len(), 4);
        assert!(!m.closed);
        assert_eq!((m.points[0].x, m.points[3].x), (0.0, 240.0), "a e depois b invertido (a emenda no meio)");
        let px: Vec<f32> = (0..4).map(|i| aiai_paint::width_at_point(m, i)).collect();
        assert!((px[0] - 4.0).abs() < 1e-3 && (px[1] - 6.0).abs() < 1e-3 && (px[2] - 8.0).abs() < 1e-3 && (px[3] - 8.0).abs() < 1e-3, "{px:?}");
        // longe demais: nada
        assert_eq!(connect(&[a.clone(), b.clone()], &[[105.0, 0.0]], 12.0, &ConnectParams { distance: 5.0, smooth: true }).joins, 0);
        // pontas que se tocam fundem num ponto só
        let c = stroke("c", &[(100.5, 0.0), (180.0, 0.0)], StrokeGeometry::Spline);
        let r = connect(&[a.clone(), c], &[[100.0, 0.0]], 5.0, &ConnectParams::default());
        assert_eq!(r.changed[0].points.len(), 3);
        // sem suave, a emenda é canto
        let r = connect(&[a, b], &[[105.0, 0.0]], 12.0, &ConnectParams { distance: 20.0, smooth: false });
        assert!(r.changed[0].points[1].corner && r.changed[0].points[2].corner);
    }

    #[test]
    fn connect_closes_a_stroke_whose_two_ends_meet() {
        let s = stroke("o", &[(0.0, 0.0), (100.0, 0.0), (50.0, 80.0), (2.0, 1.0)], StrokeGeometry::Spline);
        let r = connect(&[s], &[[1.0, 1.0]], 10.0, &ConnectParams::default());
        assert_eq!(r.joins, 1);
        assert!(r.changed[0].closed);
        assert_eq!(r.changed[0].points.len(), 3, "a ponta que caía em cima da primeira sai");
    }

    #[test]
    fn adjust_width_modes_and_edge_falloff() {
        let s = stroke("a", &(0..=20).map(|i| (i as f32 * 10.0, 0.0)).collect::<Vec<_>>(), StrokeGeometry::Freehand);
        let path = [[100.0, 0.0]];
        let wid = |s: &Stroke, x: f32| {
            let i = (0..s.points.len()).min_by(|&a, &b| (s.points[a].x - x).abs().total_cmp(&(s.points[b].x - x).abs())).unwrap();
            aiai_paint::width_at_point(s, i)
        };
        let w = adjust_width(&s, &path, 20.0, &WidthParams { mode: WidthMode::Widen, amount: 4.0, whole_line: false }).unwrap();
        assert!((wid(&w, 100.0) - 8.0).abs() < 1e-3, "{}", wid(&w, 100.0));
        assert!(wid(&w, 110.0) > 4.0 && wid(&w, 110.0) < 8.0, "queda na borda: {}", wid(&w, 110.0));
        assert!((wid(&w, 160.0) - 4.0).abs() < 1e-3, "fora do pincel");
        let n = adjust_width(&s, &path, 20.0, &WidthParams { mode: WidthMode::Narrow, amount: 3.0, whole_line: false }).unwrap();
        assert!((wid(&n, 100.0) - 1.0).abs() < 1e-3);
        let sc = adjust_width(&s, &path, 20.0, &WidthParams { mode: WidthMode::Scale, amount: 2.0, whole_line: false }).unwrap();
        assert!((wid(&sc, 100.0) - 8.0).abs() < 1e-3);
        let f = adjust_width(&s, &path, 20.0, &WidthParams { mode: WidthMode::Fixed, amount: 2.0, whole_line: true }).unwrap();
        assert!((wid(&f, 0.0) - 2.0).abs() < 1e-3 && (wid(&f, 200.0) - 2.0).abs() < 1e-3, "linha inteira");
        assert!(adjust_width(&s, &[[100.0, 100.0]], 20.0, &WidthParams { mode: WidthMode::Widen, amount: 1.0, whole_line: false }).is_none());
    }

    #[test]
    fn redraw_replaces_the_stretch_between_the_ends_of_the_path() {
        let s = stroke("a", &(0..=30).map(|i| (i as f32 * 10.0, 0.0)).collect::<Vec<_>>(), StrokeGeometry::Freehand);
        // caminho de x=100 a x=200 por cima (arco a y=-40), desenhado de trás para frente
        let path: Vec<[f32; 3]> = (0..=20).map(|i| { let x = 200.0 - i as f32 * 5.0; [x, -40.0 * ((x - 100.0) / 100.0 * std::f32::consts::PI).sin(), 1.0] }).collect();
        assert_eq!(redraw_pick(&[s.clone()], &path, 12.0), Some(0));
        let ns = redraw(&s, &path, 12.0).unwrap();
        assert!(y_at(&ns, 150.0) < -35.0, "o trecho é o novo: {}", y_at(&ns, 150.0));
        assert!(y_at(&ns, 50.0).abs() < 0.5 && y_at(&ns, 250.0).abs() < 0.5, "o resto fica");
        assert!(ns.point_index(1).is_some() && ns.point_index(31).is_some(), "pontas mantêm ids");
        let far: Vec<[f32; 3]> = vec![[100.0, 50.0, 1.0], [200.0, 0.0, 1.0]];
        assert!(redraw(&s, &far, 12.0).unwrap_err().contains("em cima da linha"));
    }

    #[test]
    fn redraw_width_follows_the_pressure() {
        let s = stroke("a", &[(0.0, 0.0), (200.0, 0.0)], StrokeGeometry::Spline);
        let path: Vec<[f32; 3]> = (0..=10).map(|i| [50.0 + i as f32 * 10.0, 0.0, 0.2]).collect();
        let ns = redraw_width(&s, &path, 10.0).unwrap();
        let i = (0..ns.points.len()).min_by(|&a, &b| (ns.points[a].x - 100.0).abs().total_cmp(&(ns.points[b].x - 100.0).abs())).unwrap();
        assert!((aiai_paint::width_at_point(&ns, i) - 4.0 * pressure_width(0.2)).abs() < 1e-3);
        assert!((aiai_paint::width_at_point(&ns, 0) - 4.0).abs() < 1e-3, "fora do caminho fica");
    }

    #[test]
    fn dust_finds_small_specks_and_holes_only() {
        let mut img = Image::new(40, 40);
        let ink = [0.0, 0.0, 0.0, 1.0];
        // linha grande (20 px), pontinho 2x2, furo 2x2 dentro de um bloco 10x10
        for x in 5..25 {
            img.data[(2 * 40 + x) as usize] = ink;
        }
        for (x, y) in [(30, 30), (31, 30), (30, 31), (31, 31)] {
            img.data[y * 40 + x] = ink;
        }
        for y in 10..20 {
            for x in 10..20 {
                img.data[y * 40 + x] = [0.9, 0.1, 0.1, 1.0];
            }
        }
        for (x, y) in [(14, 14), (15, 14), (14, 15), (15, 15)] {
            img.data[y * 40 + x] = [0.0; 4];
        }
        let o = find_dust_raster(&img, 4.0, DustMode::Opaque, None);
        assert_eq!(o.len(), 1, "só o pontinho");
        assert_eq!(o[0].bbox, [30, 30, 32, 32]);
        let h = find_dust_raster(&img, 4.0, DustMode::Holes, None);
        assert_eq!(h.len(), 1, "só o furo fechado (o fundo encosta na borda)");
        assert!((h[0].color[0] - 0.9).abs() < 1e-4);
        let mut fixed = img.clone();
        apply_dust_raster(&mut fixed, &find_dust_raster(&img, 4.0, DustMode::Both, None));
        assert_eq!(fixed.data[30 * 40 + 30], [0.0; 4]);
        assert_eq!(fixed.data[14 * 40 + 14][3], 1.0);
        assert_eq!(fixed.data[2 * 40 + 10], ink, "a linha fica");
        // dentro de uma área: o pontinho fora dela fica
        let mut m = vec![0.0f32; 1600];
        m[0] = 1.0;
        assert!(find_dust_raster(&img, 4.0, DustMode::Opaque, Some(&m)).is_empty());
        // vetorial
        let tiny = stroke("t", &[(5.0, 5.0), (6.0, 6.0)], StrokeGeometry::Polyline);
        let long = stroke("l", &[(0.0, 0.0), (100.0, 0.0)], StrokeGeometry::Polyline);
        assert_eq!(find_dust_vector(&[tiny, long], 8.0, None), vec![Id::from("t")]);
    }

    #[test]
    fn bezier_becomes_editable_freehand_with_the_same_shape() {
        let mut b = stroke("b", &[(0.0, 0.0), (30.0, -60.0), (70.0, -60.0), (100.0, 0.0)], StrokeGeometry::Bezier);
        b.points[3].width = 2.0;
        let e = editable(&b);
        assert_eq!(e.geometry, StrokeGeometry::Freehand);
        assert!(e.points.len() > 10);
        assert!(crate::points::curve_deviation(&b, &e) < 1.0);
        assert!((e.points.last().unwrap().width - 2.0).abs() < 1e-3);
    }
}
