//! Réguas editáveis (S24 2.1–2.2): perspectiva de 1, 2 e 3 pontos, paralelas, radial (linhas de concentração), simetria
//! (espelho com N eixos) e grade em perspectiva sobre um plano (chão, parede) por homografia. Como no Clip Studio Paint,
//! a régua pertence a uma camada de régua: fica em `params.rulers` de uma camada de guia (`LayerType::Guide`), várias por
//! camada, uma ativa. O app desenha as linhas por cima do canvas (só visão: nunca entram no render nem na exportação),
//! arrasta as alças (pontos de fuga, horizonte, centro, ângulo, cantos da grade) e encaixa os traços da caneta e do pincel;
//! "Converter em traços" assa as linhas no cel da camada de guia (como `aiai_perspective`).
//!
//! Toda a geometria (linhas recortadas, alças, arrasto, direção de encaixe, cópias da simetria) mora aqui, sem egui: a UI,
//! a bridge (`RULER`) e os testes usam a mesma conta.

use crate::context::*;
use crate::guide::guide_stroke;
use aiai_core::*;
use aiai_paint::warp::Homography;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::f32::consts::{PI, TAU};

/// Chave em `LayerData.params`.
pub const PARAMS_KEY: &str = "rulers";
/// Versão do formato gravado (sobe quando um campo muda de sentido; campo novo com padrão não precisa).
pub const VERSION: u32 = 1;

pub type P = [f32; 2];

fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul(a: P, k: f32) -> P {
    [a[0] * k, a[1] * k]
}
fn dot(a: P, b: P) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: P, b: P) -> f32 {
    a[0] * b[1] - a[1] * b[0]
}
fn len(a: P) -> f32 {
    a[0].hypot(a[1])
}
fn unit(a: P) -> Option<P> {
    let l = len(a);
    (l > 1e-6 && l.is_finite()).then(|| [a[0] / l, a[1] / l])
}
fn dir_of(deg: f32) -> P {
    let (s, c) = deg.to_radians().sin_cos();
    [c, s]
}

// ------------------------------------------------------------------------------------------------ dados

/// Tipo da régua.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum RulerKind {
    /// Perspectiva de 1, 2 ou 3 pontos (o número de `vps`).
    #[default]
    Perspective,
    /// Linhas paralelas (ângulo e espaçamento).
    Parallel,
    /// Linhas de concentração saindo de um centro.
    Radial,
    /// Espelho com N eixos (ou só rotação): o traço é repetido nas cópias.
    Symmetry,
    /// Grade em perspectiva sobre um plano: os 4 cantos de um quadrilátero (chão, parede) por homografia.
    Grid,
    /// Régua linear (S25): um segmento entre as duas pontas `pts`; o traço que começa perto vai PARA o segmento.
    Linear,
    /// Régua curva (S25, também a da caneta de régua): curva suave que passa pelos pontos `pts` (`closed` fecha).
    Curve,
    /// Régua de figura (S25): contorno de retângulo, elipse ou polígono regular (`shape`, `center`, `size`, `angle`).
    Figure,
    /// Círculos concêntricos (régua especial, S25): o traço segue a elipse da partida (mesmo centro, proporção e giro).
    Concentric,
    /// Guia (S25): reta horizontal (`angle` 0) ou vertical (90) por `center`, de borda a borda.
    Guide,
}

impl RulerKind {
    /// Régua "objeto" (linear, curva, figura, guia): encaixa só o traço que começa perto dela (`SNAP_PX`), em cima dela.
    pub fn is_object(self) -> bool {
        matches!(self, RulerKind::Linear | RulerKind::Curve | RulerKind::Figure | RulerKind::Guide)
    }
}

/// Figura da régua de figura.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum FigureShape {
    Rect,
    #[default]
    Ellipse,
    /// Polígono regular de `count` lados (o primeiro vértice aponta para cima com `angle` 0).
    Polygon,
}

/// Distância (px da TELA) até a régua objeto em que o traço encaixa nela, medida no Clip Studio (S25: a 40 px encaixa, a
/// 43 px não, com zoom 100%).
pub const SNAP_PX: f32 = 40.0;

/// Uma régua. Campos que o tipo não usa ficam com o padrão e são ignorados (trocar o tipo não perde os outros).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Ruler {
    /// Identificador estável dentro da camada ("r1", "r2"...).
    pub id: String,
    pub name: String,
    pub kind: RulerKind,
    /// Linhas à mostra no canvas (a régua oculta também não encaixa).
    pub visible: bool,
    /// Os traços da caneta e do pincel encaixam nesta régua (com o encaixe geral ligado).
    pub snap: bool,
    /// Travada: as alças não mexem.
    pub locked: bool,
    /// Cor das linhas, "#rrggbbaa".
    pub color: String,
    /// Perspectiva: pontos de fuga (1 a 3) em px do canvas; podem ficar muito fora do canvas. Com 2 e 3 pontos o horizonte
    /// (nível do olho) passa pelos dois primeiros; o terceiro é o vertical.
    pub vps: Vec<P>,
    /// Perspectiva de 1 ponto: inclinação do horizonte em graus (0 = na horizontal).
    pub horizon_angle: f32,
    /// Perspectiva: raios por ponto de fuga.
    pub density: u32,
    /// Paralelas (origem), radial e simetria (centro), px do canvas.
    pub center: P,
    /// Paralelas: direção das linhas; simetria: direção do primeiro eixo; radial: ângulo do primeiro raio (graus).
    pub angle: f32,
    /// Paralelas: distância entre as linhas (px).
    pub spacing: f32,
    /// Radial: número de raios; simetria: número de eixos (com espelho) ou de cópias por rotação.
    pub count: u32,
    /// Simetria: espelho nos eixos (desligado = só rotação).
    pub mirror: bool,
    /// Grade: os 4 cantos do plano (sup-esq, sup-dir, inf-dir, inf-esq) = a imagem das `cells` dentro do quadrilátero.
    pub quad: [P; 4],
    /// Grade: células em u (lado sup-esq → sup-dir) e v (sup-esq → inf-esq) dentro do quadrilátero.
    pub cells: [u32; 2],
    /// Grade: células a mais em cada lado, além do quadrilátero (o plano continua rumo ao horizonte).
    pub extend: u32,
    /// Linear: as duas pontas; curva: os pontos por onde a curva passa (px do canvas).
    pub pts: Vec<P>,
    /// Curva: fechada (o último ponto volta ao primeiro).
    pub closed: bool,
    /// Figura: retângulo, elipse ou polígono regular (`count` lados).
    pub shape: FigureShape,
    /// Figura e círculos concêntricos: semi-eixos (metade da largura e da altura antes do giro `angle`), px do canvas.
    pub size: [f32; 2],
}

impl Default for Ruler {
    fn default() -> Self {
        Ruler::preset(RulerKind::Perspective, 2, (1920, 1080), None, "r1", "")
    }
}

/// As réguas de uma camada de guia (`params.rulers`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RulerSet {
    pub version: u32,
    /// Id da régua ativa (alças, encaixe, propriedades).
    pub active: Option<String>,
    pub items: Vec<Ruler>,
}

impl Default for RulerSet {
    fn default() -> Self {
        RulerSet { version: VERSION, active: None, items: Vec::new() }
    }
}

/// Nome padrão (português; o app passa o nome traduzido).
pub fn default_name(kind: RulerKind, points: usize) -> String {
    match kind {
        RulerKind::Perspective => format!("Perspectiva {points} {}", if points == 1 { "ponto" } else { "pontos" }),
        RulerKind::Parallel => "Paralelas".into(),
        RulerKind::Radial => "Radial".into(),
        RulerKind::Symmetry => "Simetria".into(),
        RulerKind::Grid => "Grade em perspectiva".into(),
        RulerKind::Linear => "Régua linear".into(),
        RulerKind::Curve => "Régua curva".into(),
        RulerKind::Figure => "Régua de figura".into(),
        RulerKind::Concentric => "Círculos concêntricos".into(),
        RulerKind::Guide => "Guia".into(),
    }
}

impl Ruler {
    /// Régua nova com os valores de partida do tipo, posta em volta de `at` (ou do centro do canvas).
    pub fn preset(kind: RulerKind, points: usize, canvas: (u32, u32), at: Option<P>, id: &str, name: &str) -> Ruler {
        let (w, h) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
        let at = at.unwrap_or([w / 2.0, h * 0.45]);
        let points = points.clamp(1, 3);
        let vps = match points {
            1 => vec![at],
            2 => vec![[at[0] - 1.2 * w, at[1]], [at[0] + 1.2 * w, at[1]]],
            _ => vec![[at[0] - 1.2 * w, at[1]], [at[0] + 1.2 * w, at[1]], [at[0], at[1] + 2.5 * h]],
        };
        let color = match kind {
            RulerKind::Perspective => "#3aa0ffb4",
            RulerKind::Parallel => "#3cc878b4",
            RulerKind::Radial => "#f0a03cb4",
            RulerKind::Symmetry => "#e05ad2b4",
            RulerKind::Grid => "#38c8d8b4",
            RulerKind::Linear | RulerKind::Curve | RulerKind::Figure => "#9b6bffc8",
            RulerKind::Concentric => "#ff7a5ab4",
            RulerKind::Guide => "#22b8e6c8",
        };
        let pts = match kind {
            RulerKind::Linear => vec![[at[0] - 0.15 * w, at[1]], [at[0] + 0.15 * w, at[1]]],
            RulerKind::Curve => vec![[at[0] - 0.2 * w, at[1]], [at[0] - 0.07 * w, at[1] - 0.08 * h], [at[0] + 0.07 * w, at[1] + 0.08 * h], [at[0] + 0.2 * w, at[1]]],
            _ => Vec::new(),
        };
        let size = match kind {
            RulerKind::Concentric => [0.1 * w, 0.06 * w],
            _ => [0.12 * w, 0.12 * h.min(w)],
        };
        let (gw, gh) = (w * 0.18, h * 0.12);
        Ruler {
            id: id.into(),
            name: if name.is_empty() { default_name(kind, points) } else { name.into() },
            kind,
            visible: true,
            snap: true,
            locked: false,
            color: color.into(),
            vps,
            horizon_angle: 0.0,
            density: 24,
            center: at,
            angle: if kind == RulerKind::Symmetry { 90.0 } else { 0.0 },
            spacing: (h / 12.0).max(4.0),
            count: match kind {
                RulerKind::Radial => 48,
                RulerKind::Figure => 5,
                _ => 1,
            },
            mirror: true,
            quad: [[at[0] - gw, at[1] - gh * 0.6], [at[0] + gw, at[1] - gh * 0.6], [at[0] + gw * 1.7, at[1] + gh], [at[0] - gw * 1.7, at[1] + gh]],
            cells: [8, 8],
            extend: 4,
            pts,
            closed: false,
            shape: FigureShape::Ellipse,
            size,
        }
    }

    /// Número de pontos de fuga (perspectiva).
    pub fn points(&self) -> usize {
        self.vps.len()
    }

    pub fn rgba(&self) -> Rgba {
        Rgba::from_hex(&self.color).unwrap_or(Rgba([0.23, 0.63, 1.0, 0.7]))
    }

    /// Valores dentro do que o desenho aguenta (pontos finitos, contagens com limite, quadrilátero que vira homografia).
    pub fn validate(&self) -> Result<(), String> {
        let finite = |p: &P| p[0].is_finite() && p[1].is_finite() && p[0].abs() < 1e7 && p[1].abs() < 1e7;
        if self.id.trim().is_empty() {
            return Err("régua sem id".into());
        }
        if Rgba::from_hex(&self.color).is_none() {
            return Err(format!("cor inválida '{}' (use #rrggbb ou #rrggbbaa)", self.color));
        }
        if !finite(&self.center) || !self.angle.is_finite() || !self.horizon_angle.is_finite() {
            return Err("centro/ângulo fora do alcance".into());
        }
        match self.kind {
            RulerKind::Perspective => {
                if !(1..=3).contains(&self.vps.len()) {
                    return Err(format!("perspectiva precisa de 1 a 3 pontos de fuga (veio {})", self.vps.len()));
                }
                if !self.vps.iter().all(finite) {
                    return Err("ponto de fuga fora do alcance (|x|, |y| < 10 000 000)".into());
                }
                if self.vps.len() >= 2 && len(sub(self.vps[1], self.vps[0])) < 1.0 {
                    return Err("os dois primeiros pontos de fuga coincidem (o horizonte passa por eles)".into());
                }
                if !(2..=256).contains(&self.density) {
                    return Err(format!("density deve ficar em 2..256 (veio {})", self.density));
                }
            }
            RulerKind::Parallel => {
                if !(self.spacing.is_finite() && self.spacing >= 1.0) {
                    return Err(format!("spacing deve ser ≥ 1 px (veio {})", self.spacing));
                }
            }
            RulerKind::Radial => {
                if !(2..=720).contains(&self.count) {
                    return Err(format!("count deve ficar em 2..720 raios (veio {})", self.count));
                }
            }
            RulerKind::Symmetry => {
                if !(1..=64).contains(&self.count) {
                    return Err(format!("count deve ficar em 1..64 eixos (veio {})", self.count));
                }
                if self.count == 1 && !self.mirror {
                    return Err("simetria de 1 eixo sem espelho não faz cópia nenhuma".into());
                }
            }
            RulerKind::Grid => {
                if !self.quad.iter().all(finite) {
                    return Err("canto da grade fora do alcance".into());
                }
                if !(1..=200).contains(&self.cells[0]) || !(1..=200).contains(&self.cells[1]) {
                    return Err(format!("cells deve ficar em 1..200 (veio {:?})", self.cells));
                }
                if self.extend > 100 {
                    return Err(format!("extend deve ficar em 0..100 (veio {})", self.extend));
                }
                if grid_h(self).is_none() {
                    return Err("os cantos da grade não formam um quadrilátero (três alinhados ou repetidos)".into());
                }
            }
            RulerKind::Linear => {
                if self.pts.len() != 2 || !self.pts.iter().all(finite) {
                    return Err(format!("régua linear precisa de 2 pontas finitas em pts (veio {})", self.pts.len()));
                }
                if len(sub(self.pts[1], self.pts[0])) < 1.0 {
                    return Err("as duas pontas da régua linear coincidem".into());
                }
            }
            RulerKind::Curve => {
                if !(2..=1024).contains(&self.pts.len()) || !self.pts.iter().all(finite) {
                    return Err(format!("régua curva precisa de 2 a 1024 pontos finitos em pts (veio {})", self.pts.len()));
                }
                if self.closed && self.pts.len() < 3 {
                    return Err("curva fechada precisa de 3 pontos ou mais".into());
                }
                if Track::of(self).is_none_or(|t| t.length() < 1.0) {
                    return Err("os pontos da régua curva coincidem".into());
                }
            }
            RulerKind::Figure | RulerKind::Concentric => {
                if !(self.size.iter().all(|s| s.is_finite() && *s >= 0.5 && *s < 1e7)) {
                    return Err(format!("size (semi-eixos) deve ser ≥ 0,5 px (veio {:?})", self.size));
                }
                if self.kind == RulerKind::Figure && self.shape == FigureShape::Polygon && !(3..=64).contains(&self.count) {
                    return Err(format!("polígono: count deve ficar em 3..64 lados (veio {})", self.count));
                }
                if self.kind == RulerKind::Concentric && !(self.spacing.is_finite() && self.spacing >= 1.0) {
                    return Err(format!("spacing (distância entre os círculos mostrados) deve ser ≥ 1 px (veio {})", self.spacing));
                }
            }
            RulerKind::Guide => {}
        }
        Ok(())
    }
}

impl RulerSet {
    pub fn get(&self, id: &str) -> Option<&Ruler> {
        self.items.iter().find(|r| r.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Ruler> {
        self.items.iter_mut().find(|r| r.id == id)
    }

    /// A régua ativa (cai na primeira quando o id gravado não existe mais).
    pub fn active_ruler(&self) -> Option<&Ruler> {
        self.active.as_deref().and_then(|id| self.get(id)).or(self.items.first())
    }

    /// Próximo id livre ("r1", "r2"...).
    pub fn next_id(&self) -> String {
        let n = self.items.iter().filter_map(|r| r.id.strip_prefix('r').and_then(|s| s.parse::<u32>().ok())).max().unwrap_or(0);
        format!("r{}", n + 1)
    }

    /// Acrescenta e ativa; devolve o id.
    pub fn add(&mut self, mut r: Ruler) -> String {
        if r.id.is_empty() || self.get(&r.id).is_some() {
            r.id = self.next_id();
        }
        let id = r.id.clone();
        self.items.push(r);
        self.active = Some(id.clone());
        id
    }

    /// Tira a régua; a ativa passa para a vizinha.
    pub fn remove(&mut self, id: &str) -> bool {
        let Some(i) = self.items.iter().position(|r| r.id == id) else { return false };
        self.items.remove(i);
        if self.active.as_deref() == Some(id) {
            self.active = self.items.get(i.min(self.items.len().saturating_sub(1))).map(|r| r.id.clone());
        }
        true
    }

    /// Ativa a régua seguinte (Shift+U no app).
    pub fn cycle(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let cur = self.active_ruler().map(|r| r.id.clone());
        let i = cur.and_then(|id| self.items.iter().position(|r| r.id == id)).unwrap_or(0);
        self.active = Some(self.items[(i + 1) % self.items.len()].id.clone());
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version > VERSION {
            return Err(format!("réguas gravadas por uma versão mais nova do formato ({} > {VERSION})", self.version));
        }
        let mut seen = std::collections::HashSet::new();
        for r in &self.items {
            if !seen.insert(r.id.as_str()) {
                return Err(format!("id de régua repetido: {}", r.id));
            }
            r.validate().map_err(|e| format!("régua {}: {e}", r.id))?;
        }
        Ok(())
    }
}

/// Réguas da camada (vazio se não há nenhuma). Formato quebrado é erro (não some em silêncio).
pub fn read_params(params: &Value) -> Result<RulerSet, String> {
    match params.get(PARAMS_KEY) {
        None | Some(Value::Null) => Ok(RulerSet::default()),
        Some(v) => {
            // campos que faltam numa régua vêm do padrão do TIPO dela (não do padrão genérico da struct)
            let mut v = v.clone();
            if let Some(items) = v.get_mut("items").and_then(Value::as_array_mut) {
                for it in items.iter_mut().filter(|it| it.is_object()) {
                    let kind = it.get("kind").and_then(|k| serde_json::from_value::<RulerKind>(k.clone()).ok()).unwrap_or_default();
                    let points = it.get("vps").and_then(Value::as_array).map(|a| a.len()).unwrap_or(2);
                    let mut base = serde_json::to_value(Ruler::preset(kind, points, (1920, 1080), None, "", "")).map_err(|e| e.to_string())?;
                    aiai_core::patch::merge(&mut base, it);
                    *it = base;
                }
            }
            let set: RulerSet = serde_json::from_value(v).map_err(|e| format!("params.rulers ilegível: {e}"))?;
            set.validate()?;
            Ok(set)
        }
    }
}

pub fn read_layer(l: &LayerData) -> Result<RulerSet, String> {
    read_params(&l.params)
}

/// Comando que grava o conjunto inteiro na camada (um passo de undo).
pub fn set_command(layer: &Id, set: &RulerSet, label: &str) -> Command {
    let mut s = set.clone();
    s.version = VERSION;
    Command::Batch {
        commands: vec![Command::SetProps { id: layer.clone(), name: None, data: json!({ "params": { PARAMS_KEY: s } }) }],
        label: Some(label.into()),
    }
}

// ------------------------------------------------------------------------------------------------ geometria

/// Linha da régua já recortada (px do canvas); `strong` = horizonte, eixo principal (desenhada mais grossa).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RulerLine {
    pub a: P,
    pub b: P,
    pub strong: bool,
}

/// Recorta `p + t·d`, t em [t0, t1], ao retângulo `r` = [x0, y0, x1, y1] (Liang–Barsky).
pub fn clip_param(p: P, d: P, t0: f32, t1: f32, r: [f32; 4]) -> Option<(P, P)> {
    let (mut lo, mut hi) = (t0, t1);
    for (q, dd, a, b) in [(p[0], d[0], r[0], r[2]), (p[1], d[1], r[1], r[3])] {
        if dd.abs() < 1e-12 {
            if q < a || q > b {
                return None;
            }
            continue;
        }
        let (ta, tb) = ((a - q) / dd, (b - q) / dd);
        let (ta, tb) = if ta < tb { (ta, tb) } else { (tb, ta) };
        lo = lo.max(ta);
        hi = hi.min(tb);
        if lo > hi {
            return None;
        }
    }
    (lo.is_finite() && hi.is_finite()).then(|| (add(p, mul(d, lo)), add(p, mul(d, hi))))
}

/// Arco de ângulos (radianos, a0 ≤ a1) sob o qual o retângulo `rect` é visto do ponto `vp`: todo o círculo com o ponto
/// dentro; senão o menor arco que contém os 4 cantos. Os raios de um ponto de fuga se espalham por ele.
pub fn ray_arc(vp: P, rect: [f32; 4]) -> (f32, f32) {
    let inside = vp[0] >= rect[0] && vp[1] >= rect[1] && vp[0] <= rect[2] && vp[1] <= rect[3];
    if inside {
        return (0.0, TAU);
    }
    let corners = [[rect[0], rect[1]], [rect[2], rect[1]], [rect[2], rect[3]], [rect[0], rect[3]]];
    let angs: Vec<f32> = corners.iter().map(|c| (c[1] - vp[1]).atan2(c[0] - vp[0])).collect();
    let mut best = (0.0f32, TAU);
    for &s in &angs {
        let span = angs.iter().map(|&a| (a - s).rem_euclid(TAU)).fold(0.0f32, f32::max);
        if span < best.1 - best.0 {
            best = (s, s + span);
        }
    }
    best
}

/// Horizonte da perspectiva: um ponto e a direção (unitária). 2 e 3 pontos: pelos dois primeiros; 1 ponto: pelo ponto, com
/// a inclinação `horizon_angle`.
pub fn horizon(r: &Ruler) -> Option<(P, P)> {
    if r.kind != RulerKind::Perspective {
        return None;
    }
    match r.vps.len() {
        0 => None,
        1 => Some((r.vps[0], dir_of(r.horizon_angle))),
        _ => {
            let d = unit(sub(r.vps[1], r.vps[0]))?;
            // sempre da esquerda para a direita (a normal "para baixo" fica estável)
            Some((r.vps[0], if d[0] < 0.0 { mul(d, -1.0) } else { d }))
        }
    }
}

/// Pontos de fuga que estão no horizonte (os que o arrasto do horizonte leva junto).
fn on_horizon(r: &Ruler) -> usize {
    r.vps.len().min(2)
}

/// Homografia da grade: (u, v) em células → px do canvas; (0, 0) = canto sup-esq, (cells.u, cells.v) = inf-dir.
pub fn grid_h(r: &Ruler) -> Option<Homography> {
    let (nu, nv) = (r.cells[0].max(1) as f32, r.cells[1].max(1) as f32);
    let h = Homography::from_quads([[0.0, 0.0], [nu, 0.0], [nu, nv], [0.0, nv]], r.quad)?;
    h.0.iter().all(|v| v.is_finite()).then_some(h)
}

/// Denominador projetivo da homografia em (u, v) (> 0 = do lado de cá do horizonte do plano; 1 em (0, 0)).
fn h_w(h: &Homography, u: f32, v: f32) -> f32 {
    (h.0[6] * u as f64 + h.0[7] * v as f64 + h.0[8]) as f32
}

/// Trecho da reta da grade (u fixo variando v, ou o contrário) que fica na frente do horizonte do plano.
fn grid_segment(h: &Homography, fixed: f32, along_v: bool, lo: f32, hi: f32) -> Option<(P, P)> {
    const WMIN: f32 = 0.02;
    // w é afim no parâmetro: w(t) = w0 + k·t
    let w_at = |t: f32| if along_v { h_w(h, fixed, t) } else { h_w(h, t, fixed) };
    let (w0, k) = (w_at(0.0), w_at(1.0) - w_at(0.0));
    let (mut a, mut b) = (lo, hi);
    if k.abs() < 1e-9 {
        if w0 <= WMIN {
            return None;
        }
    } else {
        let t = (WMIN - w0) / k;
        if k > 0.0 {
            a = a.max(t);
        } else {
            b = b.min(t);
        }
    }
    if a >= b {
        return None;
    }
    let pt = |t: f32| {
        let (x, y) = if along_v { h.apply(fixed, t) } else { h.apply(t, fixed) };
        [x, y]
    };
    Some((pt(a), pt(b)))
}

// ------------------------------------------------------------------------------------------------ réguas objeto (S25)

/// Gira `p` em `deg` graus (em volta da origem).
fn rot(p: P, deg: f32) -> P {
    let (s, c) = deg.to_radians().sin_cos();
    [c * p[0] - s * p[1], s * p[0] + c * p[1]]
}

/// Ponto da Catmull-Rom centrípeta (Barry–Goldman) entre `p1` e `p2`, `u` em [0, 1]: passa pelos pontos sem laçadas.
fn cr_point(p0: P, p1: P, p2: P, p3: P, u: f32) -> P {
    let knot = |a: P, b: P| len(sub(b, a)).sqrt().max(1e-3);
    let (t0, t1) = (0.0f32, knot(p0, p1));
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let t = t1 + (t2 - t1) * u;
    let lerp = |a: P, b: P, ta: f32, tb: f32| add(mul(a, (tb - t) / (tb - ta)), mul(b, (t - ta) / (tb - ta)));
    let a1 = lerp(p0, p1, t0, t1);
    let a2 = lerp(p1, p2, t1, t2);
    let a3 = lerp(p2, p3, t2, t3);
    let b1 = lerp(a1, a2, t0, t2);
    let b2 = lerp(a2, a3, t1, t3);
    lerp(b1, b2, t1, t2)
}

/// Curva suave que passa por `pts` (Catmull-Rom centrípeta), `per` amostras por trecho; fechada liga o último ao
/// primeiro. Nas pontas da aberta, o vizinho que falta é o reflexo do ponto seguinte (a curva sai reta).
pub fn spline(pts: &[P], closed: bool, per: usize) -> Vec<P> {
    // pontos repetidos em seguida não fazem trecho
    let mut p: Vec<P> = Vec::with_capacity(pts.len());
    for q in pts {
        if p.last().is_none_or(|l| len(sub(*q, *l)) > 1e-3) {
            p.push(*q);
        }
    }
    if closed && p.len() > 2 && len(sub(p[0], p[p.len() - 1])) < 1e-3 {
        p.pop();
    }
    let n = p.len();
    if n < 2 {
        return p;
    }
    let per = per.max(1);
    let get = |i: isize| -> P {
        if closed {
            p[i.rem_euclid(n as isize) as usize]
        } else if i < 0 {
            sub(mul(p[0], 2.0), p[1])
        } else if i as usize >= n {
            sub(mul(p[n - 1], 2.0), p[n - 2])
        } else {
            p[i as usize]
        }
    };
    let segs = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(segs * per + 1);
    for i in 0..segs as isize {
        for k in 0..per {
            out.push(cr_point(get(i - 1), get(i), get(i + 1), get(i + 2), k as f32 / per as f32));
        }
    }
    out.push(if closed { p[0] } else { p[n - 1] });
    out
}

/// Vértices do contorno da figura (sem repetir o primeiro no fim), px do canvas.
fn figure_pts(r: &Ruler) -> Vec<P> {
    let [rx, ry] = r.size;
    let local: Vec<P> = match r.shape {
        FigureShape::Rect => vec![[-rx, -ry], [rx, -ry], [rx, ry], [-rx, ry]],
        FigureShape::Ellipse => {
            // ~3 px de corda (o encaixe e o desenho ficam a menos de 0,1 px da elipse de verdade)
            let per = PI * (3.0 * (rx + ry) - ((3.0 * rx + ry) * (rx + 3.0 * ry)).max(0.0).sqrt());
            let n = ((per / 3.0).ceil() as usize).clamp(48, 4096);
            (0..n).map(|k| {
                let a = TAU * k as f32 / n as f32;
                [rx * a.cos(), ry * a.sin()]
            }).collect()
        }
        FigureShape::Polygon => {
            let n = r.count.clamp(3, 64);
            (0..n).map(|k| {
                let a = -PI / 2.0 + TAU * k as f32 / n as f32;
                [rx * a.cos(), ry * a.sin()]
            }).collect()
        }
    };
    local.into_iter().map(|q| add(r.center, rot(q, r.angle))).collect()
}

/// O caminho da régua objeto (linear, curva, figura, guia) e se é fechado; `None` para os outros tipos. A guia é uma
/// reta "infinita" (±1 000 000 px).
pub fn outline(r: &Ruler) -> Option<(Vec<P>, bool)> {
    match r.kind {
        RulerKind::Linear if r.pts.len() >= 2 => Some((r.pts[..2].to_vec(), false)),
        RulerKind::Curve if r.pts.len() >= 2 => Some((spline(&r.pts, r.closed && r.pts.len() >= 3, 32), r.closed && r.pts.len() >= 3)),
        RulerKind::Figure => Some((figure_pts(r), true)),
        RulerKind::Guide => {
            let d = dir_of(r.angle);
            Some((vec![add(r.center, mul(d, -1e6)), add(r.center, mul(d, 1e6))], false))
        }
        _ => None,
    }
}

/// Polilinha medida (comprimento acumulado) onde o traço encaixa: o ponto mais perto e a continuidade pelo parâmetro
/// (comprimento de arco desde o começo), para o traço não pular de um trecho da curva para outro que passa perto.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// Pontos (o fechado repete o primeiro no fim).
    pub pts: Vec<P>,
    cum: Vec<f32>,
    pub closed: bool,
}

impl Track {
    pub fn new(pts: Vec<P>, closed: bool) -> Option<Track> {
        let mut p: Vec<P> = Vec::with_capacity(pts.len() + 1);
        for q in pts {
            if !(q[0].is_finite() && q[1].is_finite()) {
                return None;
            }
            if p.last().is_none_or(|l| len(sub(q, *l)) > 1e-4) {
                p.push(q);
            }
        }
        if p.len() < 2 {
            return None;
        }
        let closed = closed && p.len() >= 3;
        if closed && len(sub(p[0], p[p.len() - 1])) > 1e-4 {
            p.push(p[0]);
        }
        let mut cum = vec![0.0f32; p.len()];
        for i in 1..p.len() {
            cum[i] = cum[i - 1] + len(sub(p[i], p[i - 1]));
        }
        Some(Track { pts: p, cum, closed })
    }

    pub fn of(r: &Ruler) -> Option<Track> {
        let (p, c) = outline(r)?;
        Track::new(p, c)
    }

    pub fn length(&self) -> f32 {
        *self.cum.last().unwrap_or(&0.0)
    }

    /// Ponto no parâmetro `s` (o fechado dá a volta; o aberto para nas pontas).
    pub fn at(&self, s: f32) -> P {
        let l = self.length();
        let s = if self.closed && l > 0.0 { s.rem_euclid(l) } else { s.clamp(0.0, l) };
        let i = self.cum.partition_point(|c| *c <= s).clamp(1, self.pts.len() - 1);
        let (a, b) = (self.pts[i - 1], self.pts[i]);
        let seg = self.cum[i] - self.cum[i - 1];
        let t = if seg > 0.0 { (s - self.cum[i - 1]) / seg } else { 0.0 };
        add(a, mul(sub(b, a), t))
    }

    /// Ponto do trecho `i` mais perto de `q`: (ponto, parâmetro, distância).
    fn on_seg(&self, i: usize, q: P) -> (P, f32, f32) {
        let (a, b) = (self.pts[i], self.pts[i + 1]);
        let d = sub(b, a);
        let l2 = dot(d, d);
        let t = if l2 > 0.0 { (dot(sub(q, a), d) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let p = add(a, mul(d, t));
        (p, self.cum[i] + t * (self.cum[i + 1] - self.cum[i]), len(sub(q, p)))
    }

    /// O ponto do caminho mais perto de `q`: (ponto, parâmetro, distância).
    pub fn nearest(&self, q: P) -> (P, f32, f32) {
        (0..self.pts.len() - 1).map(|i| self.on_seg(i, q)).min_by(|a, b| a.2.total_cmp(&b.2)).expect("Track tem ≥ 2 pontos")
    }

    /// O ponto mais perto de `q` entre os trechos a até `win` (em comprimento de arco) do parâmetro `s0`: o traço anda
    /// pelo caminho sem saltar para outra parte dele que passa perto (o "8", as duas pernas de um U estreito).
    pub fn nearest_near(&self, q: P, s0: f32, win: f32) -> (P, f32) {
        let l = self.length();
        let gap = |lo: f32, hi: f32| -> f32 {
            let one = |s: f32| if s < lo { lo - s } else if s > hi { s - hi } else { 0.0 };
            if self.closed && l > 0.0 { one(s0).min(one(s0 - l)).min(one(s0 + l)) } else { one(s0) }
        };
        let best = (0..self.pts.len() - 1).filter(|&i| gap(self.cum[i], self.cum[i + 1]) <= win).map(|i| self.on_seg(i, q)).min_by(|a, b| a.2.total_cmp(&b.2));
        match best {
            Some((p, s, _)) => (p, s),
            None => (self.at(s0), s0),
        }
    }
}

/// Círculos concêntricos: `q` levado à elipse que passa por `start` (mesmo centro, proporção `size` e giro `angle`).
pub fn concentric_project(r: &Ruler, start: P, q: P) -> P {
    let [rx, ry] = [r.size[0].max(1e-3), r.size[1].max(1e-3)];
    let local = |p: P| {
        let l = rot(sub(p, r.center), -r.angle);
        [l[0] / rx, l[1] / ry]
    };
    let k = len(local(start));
    let v = local(q);
    let Some(u) = unit(v).or_else(|| unit(local(start))) else { return r.center };
    let w = mul(u, k);
    add(r.center, rot([w[0] * rx, w[1] * ry], r.angle))
}

/// A elipse dos círculos concêntricos que passa por `start` (para mostrar onde o traço vai).
pub fn concentric_ring(r: &Ruler, start: P) -> Vec<P> {
    let [rx, ry] = [r.size[0].max(1e-3), r.size[1].max(1e-3)];
    let l = rot(sub(start, r.center), -r.angle);
    let k = len([l[0] / rx, l[1] / ry]);
    let mut x = r.clone();
    x.size = [rx * k, ry * k];
    x.shape = FigureShape::Ellipse;
    let mut p = figure_pts(&x);
    if let Some(f) = p.first().copied() {
        p.push(f);
    }
    p
}

/// A régua objeto (encaixe ligado) mais perto de `s` a até `tol` px do canvas, como no Clip Studio (a mais perto
/// ganha): (índice em `rulers`, caminho, parâmetro do ponto mais perto).
pub fn pick_object(rulers: &[Ruler], s: P, tol: f32) -> Option<(usize, Track, f32)> {
    rulers
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind.is_object() && r.visible && r.snap)
        .filter_map(|(i, r)| {
            let t = Track::of(r)?;
            let (_, ps, d) = t.nearest(s);
            (d <= tol).then_some((i, t, ps, d))
        })
        .min_by(|a, b| a.3.total_cmp(&b.3))
        .map(|(i, t, ps, _)| (i, t, ps))
}

/// Caneta de régua: o caminho à mão simplificado (Ramer–Douglas–Peucker com tolerância `tol` px) nos pontos por onde a
/// régua curva passa. `None` se o caminho não anda.
pub fn simplify(path: &[P], tol: f32) -> Option<Vec<P>> {
    let mut p: Vec<P> = Vec::with_capacity(path.len());
    for q in path {
        if p.last().is_none_or(|l| len(sub(*q, *l)) > 0.5) {
            p.push(*q);
        }
    }
    if p.len() < 2 {
        return None;
    }
    let mut keep = vec![false; p.len()];
    keep[0] = true;
    keep[p.len() - 1] = true;
    let mut stack = vec![(0usize, p.len() - 1)];
    while let Some((i, j)) = stack.pop() {
        if j <= i + 1 {
            continue;
        }
        let (a, b) = (p[i], p[j]);
        let d = sub(b, a);
        let l = len(d);
        let dist = |q: P| if l < 1e-6 { len(sub(q, a)) } else { cross(d, sub(q, a)).abs() / l };
        let (k, dm) = (i + 1..j).map(|k| (k, dist(p[k]))).max_by(|x, y| x.1.total_cmp(&y.1)).expect("j > i + 1");
        if dm > tol {
            keep[k] = true;
            stack.push((i, k));
            stack.push((k, j));
        }
    }
    let out: Vec<P> = p.iter().zip(&keep).filter(|(_, k)| **k).map(|(q, _)| *q).collect();
    (out.len() >= 2 && len(sub(out[out.len() - 1], out[0])) + out.windows(2).map(|w| len(sub(w[1], w[0]))).sum::<f32>() > 1.0).then_some(out)
}

/// Polilinhas da régua objeto e dos círculos concêntricos (com a força de cada uma) dentro de `clip`. Concêntricos: a
/// elipse da régua (forte), os anéis a cada `spacing` px (no eixo maior) até cobrir o recorte e o eixo pelo centro.
fn polylines(r: &Ruler, clip: [f32; 4]) -> Vec<(Vec<P>, bool)> {
    match r.kind {
        RulerKind::Concentric => {
            let mut out = Vec::new();
            let [rx, ry] = [r.size[0].max(1e-3), r.size[1].max(1e-3)];
            let corners = [[clip[0], clip[1]], [clip[2], clip[1]], [clip[2], clip[3]], [clip[0], clip[3]]];
            let kmax = corners.iter().map(|c| {
                let l = rot(sub(*c, r.center), -r.angle);
                len([l[0] / rx, l[1] / ry])
            }).fold(0.0f32, f32::max);
            let dk = r.spacing.max(1.0) / rx.max(ry);
            // no máximo ~40 anéis na tela (zoom muito longe: pula de m em m)
            let m = ((kmax / dk / 40.0).ceil() as i64).max(1);
            let mut j = m;
            while (j as f32) * dk <= kmax {
                let k = j as f32 * dk;
                if (k - 1.0).abs() > dk * 0.25 {
                    let mut x = r.clone();
                    x.size = [rx * k, ry * k];
                    x.shape = FigureShape::Ellipse;
                    let mut p = figure_pts(&x);
                    p.push(p[0]);
                    out.push((p, false));
                }
                j += m;
            }
            let mut base = figure_pts(&{
                let mut x = r.clone();
                x.shape = FigureShape::Ellipse;
                x
            });
            base.push(base[0]);
            out.push((base, true));
            if let Some((a, b)) = clip_param(r.center, dir_of(r.angle), -1e8, 1e8, clip) {
                out.push((vec![a, b], false));
            }
            out
        }
        _ => match outline(r) {
            Some((mut p, closed)) => {
                if closed {
                    p.push(p[0]);
                }
                vec![(p, true)]
            }
            None => Vec::new(),
        },
    }
}

/// As linhas da régua recortadas a `clip` ([x0, y0, x1, y1], px do canvas). Os raios dos pontos de fuga se distribuem
/// pelo arco sob o qual o CANVAS é visto (o mesmo em qualquer zoom e pan) e se estendem até a borda de `clip`.
pub fn lines(r: &Ruler, canvas: (u32, u32), clip: [f32; 4]) -> Vec<RulerLine> {
    let (w, h) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
    let big = 1e8f32;
    let mut out = Vec::new();
    let mut push = |seg: Option<(P, P)>, strong: bool| {
        if let Some((a, b)) = seg {
            out.push(RulerLine { a, b, strong });
        }
    };
    match r.kind {
        RulerKind::Perspective => {
            let n = r.density.clamp(2, 256);
            for vp in &r.vps {
                let (a0, a1) = ray_arc(*vp, [0.0, 0.0, w, h]);
                for k in 0..n {
                    let a = a0 + (a1 - a0) * (k as f32 + 0.5) / n as f32;
                    push(clip_param(*vp, [a.cos(), a.sin()], 0.0, big, clip), false);
                }
            }
            if let Some((p, d)) = horizon(r) {
                push(clip_param(p, d, -big, big, clip), true);
            }
        }
        RulerKind::Parallel => {
            let d = dir_of(r.angle);
            let nrm = [-d[1], d[0]];
            let sp = r.spacing.max(1.0);
            let corners = [[clip[0], clip[1]], [clip[2], clip[1]], [clip[2], clip[3]], [clip[0], clip[3]]];
            let offs: Vec<f32> = corners.iter().map(|c| dot(sub(*c, r.center), nrm)).collect();
            let (smin, smax) = offs.iter().fold((f32::MAX, f32::MIN), |(a, b), &o| (a.min(o), b.max(o)));
            // muitas linhas na tela (zoom muito longe): pula de k em k para não passar de ~600
            let step = (((smax - smin) / sp / 600.0).ceil() as i64).max(1);
            let (k0, k1) = ((smin / sp).ceil() as i64, (smax / sp).floor() as i64);
            let mut k = k0.div_euclid(step) * step;
            while k <= k1 {
                push(clip_param(add(r.center, mul(nrm, k as f32 * sp)), d, -big, big, clip), k == 0);
                k += step;
            }
        }
        RulerKind::Radial => {
            let n = r.count.clamp(2, 720);
            for k in 0..n {
                let a = r.angle.to_radians() + TAU * k as f32 / n as f32;
                push(clip_param(r.center, [a.cos(), a.sin()], 0.0, big, clip), false);
            }
        }
        RulerKind::Symmetry => {
            let n = r.count.clamp(1, 64);
            for k in 0..n {
                if r.mirror {
                    let a = r.angle.to_radians() + PI * k as f32 / n as f32;
                    push(clip_param(r.center, [a.cos(), a.sin()], -big, big, clip), k == 0);
                } else {
                    let a = r.angle.to_radians() + TAU * k as f32 / n as f32;
                    push(clip_param(r.center, [a.cos(), a.sin()], 0.0, big, clip), k == 0);
                }
            }
        }
        RulerKind::Grid => {
            let Some(hm) = grid_h(r) else { return out };
            let (nu, nv) = (r.cells[0].clamp(1, 200) as i64, r.cells[1].clamp(1, 200) as i64);
            let e = r.extend.min(100) as i64;
            let (vlo, vhi) = (-e as f32, (nv + e) as f32);
            let (ulo, uhi) = (-e as f32, (nu + e) as f32);
            for i in -e..=nu + e {
                let strong = i == 0 || i == nu;
                push(grid_segment(&hm, i as f32, true, vlo, vhi).and_then(|(a, b)| clip_param(a, sub(b, a), 0.0, 1.0, clip)), strong);
            }
            for j in -e..=nv + e {
                let strong = j == 0 || j == nv;
                push(grid_segment(&hm, j as f32, false, ulo, uhi).and_then(|(a, b)| clip_param(a, sub(b, a), 0.0, 1.0, clip)), strong);
            }
        }
        RulerKind::Linear | RulerKind::Curve | RulerKind::Figure | RulerKind::Concentric | RulerKind::Guide => {
            for (p, strong) in polylines(r, clip) {
                for w in p.windows(2) {
                    push(clip_param(w[0], sub(w[1], w[0]), 0.0, 1.0, clip), strong);
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------ encaixe

/// Direções (unitárias) em que um traço que começa em `s` pode encaixar. Perspectiva: rumo a cada ponto de fuga (1 ponto
/// soma a horizontal do horizonte e a vertical a ele; 2 pontos soma a vertical); paralelas: a direção; radial: o raio
/// do centro que passa por `s`; grade: as duas linhas da grade que passam por `s`. Simetria não encaixa (repete o traço).
pub fn snap_dirs(r: &Ruler, s: P) -> Vec<P> {
    let mut out = Vec::new();
    match r.kind {
        RulerKind::Perspective => {
            for vp in &r.vps {
                if let Some(d) = unit(sub(*vp, s)) {
                    out.push(d);
                }
            }
            if let Some((_, hd)) = horizon(r) {
                if r.vps.len() == 1 {
                    out.push(hd);
                }
                if r.vps.len() <= 2 {
                    out.push([-hd[1], hd[0]]);
                }
            }
        }
        RulerKind::Parallel => out.push(dir_of(r.angle)),
        RulerKind::Radial => out.extend(unit(sub(s, r.center))),
        RulerKind::Symmetry => {}
        RulerKind::Grid => {
            if let Some((hm, inv)) = grid_h(r).and_then(|h| h.inverse().map(|i| (h, i))) {
                let (u, v) = inv.apply(s[0], s[1]);
                let at = |u: f32, v: f32| {
                    let (x, y) = hm.apply(u, v);
                    [x, y]
                };
                let p0 = at(u, v);
                out.extend(unit(sub(at(u + 0.01, v), p0)));
                out.extend(unit(sub(at(u, v + 0.01), p0)));
            }
        }
        // objetos e concêntricos encaixam ponto a ponto (`Track`, `concentric_project`), não por direção
        RulerKind::Linear | RulerKind::Curve | RulerKind::Figure | RulerKind::Concentric | RulerKind::Guide => {}
    }
    out
}

/// A direção de encaixe mais próxima do primeiro trecho `d` do traço (o menor ângulo entre as retas), como no CSP.
pub fn pick_dir(r: &Ruler, s: P, d: P) -> Option<P> {
    let d = unit(d)?;
    snap_dirs(r, s).into_iter().max_by(|a, b| dot(*a, d).abs().total_cmp(&dot(*b, d).abs()))
}

/// `q` projetado na reta que passa por `s` com direção `dir` (unitária).
pub fn project(s: P, dir: P, q: P) -> P {
    add(s, mul(dir, dot(sub(q, s), dir)))
}

/// Afim 2×3 [a, b, tx, c, d, ty]: (x, y) → (a·x + b·y + tx, c·x + d·y + ty).
pub type Map2 = [f32; 6];

pub fn apply_map(m: &Map2, p: P) -> P {
    [m[0] * p[0] + m[1] * p[1] + m[2], m[3] * p[0] + m[4] * p[1] + m[5]]
}

/// Cópias da simetria (sem a identidade): N rotações em volta do centro e, com espelho, cada uma composta com o reflexo
/// no primeiro eixo (os N eixos ficam a 180°/N um do outro).
pub fn symmetry_maps(r: &Ruler) -> Vec<Map2> {
    if r.kind != RulerKind::Symmetry {
        return Vec::new();
    }
    let n = r.count.clamp(1, 64);
    let c = r.center;
    // em volta da origem e depois devolve ao centro
    let around = |a: f32, b: f32, cc: f32, d: f32| -> Map2 { [a, b, c[0] - a * c[0] - b * c[1], cc, d, c[1] - cc * c[0] - d * c[1]] };
    let t = 2.0 * r.angle.to_radians();
    let refl = (t.cos(), t.sin());
    let mut out = Vec::new();
    for k in 0..n {
        let (s, co) = (TAU * k as f32 / n as f32).sin_cos();
        if k > 0 {
            out.push(around(co, -s, s, co));
        }
        if r.mirror {
            // R_k · Ref, Ref = [[cos 2θ, sin 2θ], [sin 2θ, −cos 2θ]]
            let (rc, rs) = refl;
            out.push(around(co * rc - s * rs, co * rs + s * rc, s * rc + co * rs, s * rs - co * rc));
        }
    }
    out
}

/// Quadrilátero de uma grade de chão coerente com uma perspectiva de 2 ou 3 pontos: canto perto do pé do canvas, lados
/// rumo aos dois pontos de fuga do horizonte (as linhas da grade estendida convergem neles).
pub fn floor_quad(r: &Ruler, canvas: (u32, u32)) -> Option<[P; 4]> {
    if r.kind != RulerKind::Perspective || r.vps.len() < 2 {
        return None;
    }
    let (w, h) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
    let (hp, hd) = horizon(r)?;
    let down = [-hd[1], hd[0]];
    let c = [w / 2.0, h / 2.0];
    let foot = project(hp, hd, c);
    let dist = dot(sub(c, foot), down).max(0.0) + h * 0.3;
    let o = add(foot, mul(down, dist));
    let (v0, v1) = (r.vps[0], r.vps[1]);
    let f = 0.18;
    let a = add(o, mul(sub(v0, o), f));
    let b = add(o, mul(sub(v1, o), f));
    // D = (A → V1) ∩ (B → V0)
    let (d1, d2) = (sub(v1, a), sub(v0, b));
    let den = cross(d1, d2);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = cross(sub(b, a), d2) / den;
    let d = add(a, mul(d1, t));
    // ordem dos cantos como se vê (losango): esquerda, fundo (em cima), direita, frente (embaixo)
    Some([a, d, b, o])
}

// ------------------------------------------------------------------------------------------------ alças

/// Alça de uma régua (ferramenta Régua).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Handle {
    /// Ponto de fuga `i`.
    Vp(usize),
    /// Horizonte (nível do olho): arrastar leva os pontos do horizonte junto, na perpendicular.
    Horizon,
    /// Centro (radial, simetria) ou origem (paralelas).
    Center,
    /// Ângulo (paralelas, simetria, radial).
    Angle,
    /// Canto `i` da grade.
    Corner(usize),
    /// Meio da grade: move o quadrilátero inteiro; da linear e da curva: move todos os pontos.
    Body,
    /// Ponto `i` da régua linear (ponta) ou curva.
    Point(usize),
    /// Canto da caixa da figura / dos concêntricos (semi-eixos).
    Size,
}

impl Handle {
    /// Nome estável (bridge/notas de UI): "vp0", "horizon", "center", "angle", "corner2", "body".
    pub fn key(self) -> String {
        match self {
            Handle::Vp(i) => format!("vp{i}"),
            Handle::Horizon => "horizon".into(),
            Handle::Center => "center".into(),
            Handle::Angle => "angle".into(),
            Handle::Corner(i) => format!("corner{i}"),
            Handle::Body => "body".into(),
            Handle::Point(i) => format!("point{i}"),
            Handle::Size => "size".into(),
        }
    }
}

/// Alças da régua e onde ficam (px do canvas). `arm` = comprimento da alça de ângulo em px do canvas (o app passa um
/// tamanho fixo de tela dividido pela escala).
pub fn handles(r: &Ruler, canvas: (u32, u32), arm: f32) -> Vec<(Handle, P)> {
    let c = [canvas.0 as f32 / 2.0, canvas.1 as f32 / 2.0];
    let mut out = Vec::new();
    match r.kind {
        RulerKind::Perspective => {
            for (i, p) in r.vps.iter().enumerate() {
                out.push((Handle::Vp(i), *p));
            }
            if let Some((p, d)) = horizon(r) {
                out.push((Handle::Horizon, project(p, d, c)));
            }
        }
        RulerKind::Parallel | RulerKind::Symmetry | RulerKind::Radial => {
            out.push((Handle::Center, r.center));
            out.push((Handle::Angle, add(r.center, mul(dir_of(r.angle), arm))));
        }
        RulerKind::Grid => {
            for (i, p) in r.quad.iter().enumerate() {
                out.push((Handle::Corner(i), *p));
            }
            if let Some(hm) = grid_h(r) {
                let (x, y) = hm.apply(r.cells[0] as f32 / 2.0, r.cells[1] as f32 / 2.0);
                out.push((Handle::Body, [x, y]));
            }
        }
        RulerKind::Linear | RulerKind::Curve => {
            for (i, p) in r.pts.iter().enumerate() {
                out.push((Handle::Point(i), *p));
            }
            if !r.pts.is_empty() {
                let (lo, hi) = bbox(&r.pts);
                let mid = mul(add(lo, hi), 0.5);
                // meio da linear: no segmento; da curva: o meio da caixa, a não ser que caia em cima de um ponto
                let body = if r.pts.iter().any(|p| len(sub(*p, mid)) < arm * 0.25) { add(mid, [0.0, arm * 0.35]) } else { mid };
                out.push((Handle::Body, body));
            }
        }
        RulerKind::Figure | RulerKind::Concentric => {
            out.push((Handle::Center, r.center));
            out.push((Handle::Size, add(r.center, rot(r.size, r.angle))));
            out.push((Handle::Angle, add(r.center, rot([r.size[0] + arm * 0.6, 0.0], r.angle))));
        }
        RulerKind::Guide => out.push((Handle::Center, project(r.center, dir_of(r.angle), c))),
    }
    out
}

/// Caixa (mínimo, máximo) dos pontos.
fn bbox(pts: &[P]) -> (P, P) {
    pts.iter().fold(([f32::MAX, f32::MAX], [f32::MIN, f32::MIN]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]))
}

/// Alça mais perto de `p` a até `tol` (px do canvas).
pub fn hit_handle(r: &Ruler, canvas: (u32, u32), arm: f32, p: P, tol: f32) -> Option<Handle> {
    handles(r, canvas, arm).into_iter().map(|(h, q)| (h, len(sub(q, p)))).filter(|(_, d)| *d <= tol).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(h, _)| h)
}

/// A régua com a alça `h` arrastada de `start` até `cur` (px do canvas), a partir do estado `orig` do começo do arrasto.
/// Ponto de fuga do horizonte: anda livre e os outros do horizonte acompanham na perpendicular (o horizonte não gira);
/// com `alt`, só o ponto anda (o horizonte gira junto). Horizonte: leva os pontos do horizonte na perpendicular.
pub fn drag(orig: &Ruler, h: Handle, start: P, cur: P, alt: bool) -> Ruler {
    let mut r = orig.clone();
    let delta = sub(cur, start);
    match h {
        Handle::Vp(i) if i < r.vps.len() => {
            let free = alt || i >= on_horizon(orig) || orig.vps.len() == 1;
            r.vps[i] = add(orig.vps[i], delta);
            if !free {
                if let Some((_, hd)) = horizon(orig) {
                    let nrm = [-hd[1], hd[0]];
                    let perp = mul(nrm, dot(delta, nrm));
                    for j in 0..on_horizon(orig) {
                        if j != i {
                            r.vps[j] = add(orig.vps[j], perp);
                        }
                    }
                }
            }
        }
        Handle::Horizon => {
            if let Some((_, hd)) = horizon(orig) {
                let nrm = [-hd[1], hd[0]];
                let perp = mul(nrm, dot(delta, nrm));
                for j in 0..on_horizon(orig) {
                    r.vps[j] = add(orig.vps[j], perp);
                }
            }
        }
        Handle::Center => r.center = add(orig.center, delta),
        Handle::Angle => {
            let d = sub(cur, orig.center);
            if len(d) > 1e-3 {
                let mut a = d[1].atan2(d[0]).to_degrees();
                if orig.kind == RulerKind::Parallel || orig.kind == RulerKind::Symmetry && orig.mirror {
                    // reta: 0..180
                    a = a.rem_euclid(180.0);
                }
                r.angle = a;
            }
        }
        Handle::Corner(i) if i < 4 => r.quad[i] = add(orig.quad[i], delta),
        Handle::Body if matches!(orig.kind, RulerKind::Linear | RulerKind::Curve) => {
            for (k, p) in r.pts.iter_mut().enumerate() {
                *p = add(orig.pts[k], delta);
            }
        }
        Handle::Body => {
            for k in 0..4 {
                r.quad[k] = add(orig.quad[k], delta);
            }
        }
        Handle::Point(i) if i < r.pts.len() => r.pts[i] = add(orig.pts[i], delta),
        Handle::Size => {
            // o canto segue o cursor no referencial girado da figura
            let l = rot(sub(cur, orig.center), -orig.angle);
            r.size = [l[0].abs().max(0.5), l[1].abs().max(0.5)];
        }
        _ => {}
    }
    // arrasto que levaria a régua a um estado inválido (pontos coincidentes, quadrilátero degenerado): fica como estava
    if r.validate().is_err() && orig.validate().is_ok() { orig.clone() } else { r }
}

/// Horizonte no meio do canvas (o "nível do olho" do painel): y do ponto do horizonte em x = largura/2.
pub fn eye_level(r: &Ruler, canvas: (u32, u32)) -> Option<f32> {
    let (p, d) = horizon(r)?;
    if d[0].abs() < 1e-6 {
        return None;
    }
    let x = canvas.0 as f32 / 2.0;
    Some(p[1] + d[1] * (x - p[0]) / d[0])
}

/// Muda o nível do olho para `y` (no meio do canvas): os pontos do horizonte sobem/descem juntos.
pub fn set_eye_level(r: &mut Ruler, canvas: (u32, u32), y: f32) {
    if let Some(cur) = eye_level(r, canvas) {
        let dy = y - cur;
        for j in 0..on_horizon(r) {
            r.vps[j][1] += dy;
        }
    }
}

/// Muda o número de pontos de fuga (1 a 3) mantendo o horizonte: entra/sai o segundo (do outro lado) e o vertical.
pub fn set_points(r: &mut Ruler, canvas: (u32, u32), n: usize) {
    let n = n.clamp(1, 3);
    let (w, h) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
    let hz = horizon(r);
    while r.vps.len() > n {
        r.vps.pop();
    }
    while r.vps.len() < n {
        let next = match (r.vps.len(), hz) {
            (0, _) => [w / 2.0, h * 0.45],
            (1, Some((p, d))) => {
                // o segundo no mesmo horizonte, espelhado em volta do meio do canvas; o primeiro perto do meio (1 ponto
                // central) ganha o segundo a 1,2 largura para a direita, para os dois não caírem juntos
                let c = [w / 2.0, h / 2.0];
                let foot = project(p, d, c);
                let off = dot(sub(p, foot), d);
                if off.abs() >= 0.1 * w {
                    add(foot, mul(d, -off))
                } else {
                    add(p, mul(d, 1.2 * w))
                }
            }
            _ => {
                // o vertical: abaixo do meio do canvas, longe (vista de cima)
                let (p, d) = horizon(r).unwrap_or(([w / 2.0, h * 0.45], [1.0, 0.0]));
                add(project(p, d, [w / 2.0, h / 2.0]), mul([-d[1], d[0]], 2.5 * h))
            }
        };
        r.vps.push(next);
    }
}

/// Ponto em volta do qual a régua está (onde a régua de outro tipo nasce numa conversão): o ponto de fuga do 1 ponto, o
/// pé do meio do canvas no horizonte (2 e 3 pontos), o centro do quadrilátero da grade, o centro/origem dos outros.
pub fn anchor(r: &Ruler, canvas: (u32, u32)) -> P {
    let c = [canvas.0.max(1) as f32 / 2.0, canvas.1.max(1) as f32 / 2.0];
    match r.kind {
        RulerKind::Perspective if r.vps.len() == 1 => r.vps[0],
        RulerKind::Perspective => horizon(r).map(|(p, d)| project(p, d, c)).unwrap_or(r.center),
        RulerKind::Grid => mul(r.quad.iter().fold([0.0, 0.0], |a, q| add(a, *q)), 0.25),
        RulerKind::Linear | RulerKind::Curve if !r.pts.is_empty() => {
            let (lo, hi) = bbox(&r.pts);
            mul(add(lo, hi), 0.5)
        }
        _ => r.center,
    }
}

/// A régua `r` convertida para o tipo `kind` (`points` = pontos de fuga da perspectiva) no mesmo lugar. Entre
/// perspectivas o horizonte fica: 1 → 2 põe o segundo ponto espelhado em volta do meio do canvas, 2 → 3 soma o vertical,
/// 3 → 2 tira o vertical, 2/3 → 1 deixa um ponto só no horizonte, no pé do meio do canvas (com a inclinação do
/// horizonte). Perspectiva de 2/3 pontos → grade usa o chão coerente com os pontos de fuga. Os outros recomeçam com os
/// padrões do tipo em volta de `anchor`. Ficam id, nome, olho, encaixe, cadeado e a cor que o usuário escolheu (a cor
/// padrão do tipo antigo vira a padrão do novo).
pub fn convert(r: &Ruler, kind: RulerKind, points: usize, canvas: (u32, u32)) -> Ruler {
    let points = points.clamp(1, 3);
    if kind == RulerKind::Perspective && r.kind == RulerKind::Perspective {
        let mut x = r.clone();
        if points == 1 && x.vps.len() > 1 {
            let c = [canvas.0.max(1) as f32 / 2.0, canvas.1.max(1) as f32 / 2.0];
            match horizon(&x) {
                Some((p, d)) => {
                    x.vps = vec![project(p, d, c)];
                    x.horizon_angle = d[1].atan2(d[0]).to_degrees();
                }
                None => x.vps.truncate(1),
            }
        } else {
            set_points(&mut x, canvas, points);
        }
        return x;
    }
    if kind == r.kind {
        return r.clone();
    }
    let mut x = Ruler::preset(kind, points, canvas, Some(anchor(r, canvas)), &r.id, &r.name);
    if kind == RulerKind::Grid {
        if let Some(q) = floor_quad(r, canvas) {
            x.quad = q;
        }
    }
    // entre réguas objeto parecidas a forma fica: linear ⇄ curva pelos pontos, figura ⇄ concêntricos pela caixa
    match (r.kind, kind) {
        (RulerKind::Linear, RulerKind::Curve) if r.pts.len() == 2 => x.pts = vec![r.pts[0], mul(add(r.pts[0], r.pts[1]), 0.5), r.pts[1]],
        (RulerKind::Curve, RulerKind::Linear) if r.pts.len() >= 2 && len(sub(r.pts[r.pts.len() - 1], r.pts[0])) >= 1.0 => x.pts = vec![r.pts[0], r.pts[r.pts.len() - 1]],
        (RulerKind::Figure, RulerKind::Concentric) | (RulerKind::Concentric, RulerKind::Figure) => (x.size, x.angle) = (r.size, r.angle),
        _ => {}
    }
    let old_default = Ruler::preset(r.kind, 1, canvas, None, "", "x").color;
    if !r.color.eq_ignore_ascii_case(&old_default) {
        x.color = r.color.clone();
    }
    x.visible = r.visible;
    x.snap = r.snap;
    x.locked = r.locked;
    x
}

// ------------------------------------------------------------------------------------------------ assar e bridge

/// Traços de guia com as linhas da régua dentro do canvas (o mesmo traço de `aiai_perspective`).
pub fn bake_strokes(r: &Ruler, canvas: (u32, u32)) -> Vec<Stroke> {
    let (w, h) = (canvas.0 as f32, canvas.1 as f32);
    let color = r.rgba();
    let clip = [0.0, 0.0, w, h];
    let width = |strong: bool| if strong { 2.0 } else { 1.2 };
    match r.kind {
        // curvas e contornos: um traço por polilinha (não um por trechinho); a guia e o eixo recortados ao canvas
        RulerKind::Linear | RulerKind::Curve | RulerKind::Figure | RulerKind::Concentric | RulerKind::Guide => polylines(r, clip)
            .into_iter()
            .filter_map(|(p, strong)| {
                let p = if p.len() == 2 { clip_param(p[0], sub(p[1], p[0]), 0.0, 1.0, clip).map(|(a, b)| vec![a, b])? } else { p };
                Some(guide_stroke(p, width(strong), color))
            })
            .collect(),
        _ => lines(r, canvas, clip).into_iter().map(|l| guide_stroke(vec![l.a, l.b], width(l.strong), color)).collect(),
    }
}

/// Corte que contém a camada (sobe pelas pastas).
pub fn cut_of_layer(p: &Project, layer: &Id) -> Option<Id> {
    let mut cur = p.node(layer)?.parent.clone();
    for _ in 0..64 {
        let id = cur?;
        let n = p.node(&id)?;
        if matches!(n.data, NodeKind::Cut(_)) {
            return Some(id);
        }
        cur = n.parent.clone();
    }
    None
}

/// Acrescenta as linhas da régua ao cel exposto pela camada de guia no quadro (um passo de undo).
pub fn bake_command(p: &Project, cut: &Id, frame: u32, layer: &Id, r: &Ruler) -> ToolResult<Command> {
    let c = cut_data(p, cut)?;
    let node = exposed_cel(p, c, layer, frame).ok_or_else(|| ToolError::Invalid(format!("a camada {layer} não expõe cel no quadro {frame}")))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    let strokes = bake_strokes(r, canvas_size(p, c));
    if strokes.is_empty() {
        return Err(ToolError::Invalid("a régua não tem linha nenhuma dentro do canvas".into()));
    }
    let base = cel.strokes.len();
    let items = strokes.into_iter().enumerate().map(|(i, stroke)| aiai_core::command::IndexedStroke { index: base + i, stroke }).collect();
    Ok(Command::Batch { commands: vec![Command::InsertStrokes { cel: node.id.clone(), items }], label: Some(format!("Converter régua em traços: {}", r.name)) })
}

/// O que `RULER` faz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RulerOp {
    /// Lista as réguas da camada (não muda nada).
    List,
    /// Cria uma régua (`ruler.kind` obrigatório; o resto tem padrão) e a ativa.
    Create,
    /// Muda os campos dados em `ruler` da régua `id`.
    Edit,
    /// Apaga a régua `id`.
    Delete,
    /// Ativa a régua `id`.
    Activate,
    /// Assa as linhas da régua `id` (ou da ativa) como traços no cel da camada no quadro `frame`.
    Bake,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RulerRequest {
    /// Camada de guia (tipo `guide`) que guarda as réguas.
    pub layer: Id,
    pub op: RulerOp,
    /// Régua alvo (edit, delete, activate; bake usa a ativa sem id).
    #[serde(default)]
    pub id: Option<String>,
    /// create: campos da régua nova (`kind` obrigatório; sem `vps` a perspectiva nasce com `points` pontos); edit: só
    /// os campos a mudar. Chave desconhecida é erro.
    #[serde(default)]
    #[schemars(with = "Option<Ruler>")]
    pub ruler: Option<Value>,
    /// create, perspectiva sem `vps`: 1, 2 ou 3 pontos de fuga (padrão 2); edit: muda o número de pontos mantendo o horizonte.
    #[serde(default)]
    pub points: Option<u32>,
    /// edit, perspectiva: nível do olho (y do horizonte no meio do canvas); os pontos do horizonte acompanham.
    #[serde(default)]
    pub eye_level: Option<f32>,
    /// bake: corte (padrão: o corte da camada).
    #[serde(default)]
    pub cut: Option<Id>,
    /// bake: quadro 0-based (padrão 0).
    #[serde(default)]
    pub frame: Option<u32>,
}

/// Resultado de `RULER`: o comando (nada em `list`) e as réguas como ficam.
pub struct RulerPlan {
    pub command: Option<Command>,
    pub set: RulerSet,
    pub id: Option<String>,
}

fn merge_ruler(base: &Ruler, patch: &Value) -> ToolResult<Ruler> {
    if !patch.is_object() {
        return Err(ToolError::Invalid("ruler deve ser um objeto".into()));
    }
    let mut v = serde_json::to_value(base).map_err(|e| ToolError::Invalid(e.to_string()))?;
    aiai_core::patch::merge(&mut v, patch);
    serde_json::from_value::<Ruler>(v).map_err(|e| ToolError::Invalid(format!("ruler: {e}")))
}

pub fn ruler_plan(p: &Project, req: &RulerRequest) -> ToolResult<RulerPlan> {
    let l = layer_data(p, &req.layer)?;
    if l.kind != LayerType::Guide {
        return Err(ToolError::Invalid(format!("{} não é camada de guia (as réguas ficam numa camada do tipo guide)", req.layer)));
    }
    let mut set = read_layer(l).map_err(ToolError::Invalid)?;
    let cut = match &req.cut {
        Some(c) => c.clone(),
        None => cut_of_layer(p, &req.layer).ok_or_else(|| ToolError::Invalid(format!("a camada {} não está em um corte", req.layer)))?,
    };
    let canvas = canvas_size(p, cut_data(p, &cut)?);
    let need_id = |set: &RulerSet| -> ToolResult<String> {
        let id = req.id.clone().ok_or_else(|| ToolError::Invalid("falta 'id' da régua".into()))?;
        if set.get(&id).is_none() {
            let ids: Vec<&str> = set.items.iter().map(|r| r.id.as_str()).collect();
            return Err(ToolError::NotFound(format!("régua '{id}' (há: {ids:?})")));
        }
        Ok(id)
    };
    let label;
    let mut id = None;
    match req.op {
        RulerOp::List => return Ok(RulerPlan { command: None, id: set.active_ruler().map(|r| r.id.clone()), set }),
        RulerOp::Create => {
            let patch = req.ruler.clone().unwrap_or(json!({}));
            let kind: RulerKind = match patch.get("kind") {
                Some(k) => serde_json::from_value(k.clone()).map_err(|e| ToolError::Invalid(format!("ruler.kind: {e}")))?,
                None => return Err(ToolError::Invalid("create: falta ruler.kind (perspective, parallel, radial, symmetry, grid, linear, curve, figure, concentric, guide)".into())),
            };
            let pts = req.points.unwrap_or(2);
            if !(1..=3).contains(&pts) {
                return Err(ToolError::Invalid(format!("points deve ser 1, 2 ou 3 (veio {pts})")));
            }
            let nid = set.next_id();
            let base = Ruler::preset(kind, pts as usize, canvas, None, &nid, "");
            let mut r = merge_ruler(&base, &patch)?;
            if r.id.is_empty() || set.get(&r.id).is_some() {
                r.id = nid;
            }
            r.validate().map_err(ToolError::Invalid)?;
            id = Some(set.add(r));
            label = "Nova régua";
        }
        RulerOp::Edit => {
            let rid = need_id(&set)?;
            let mut cur = set.get(&rid).cloned().expect("existe");
            // tipo novo: converte no lugar primeiro (como o app), depois aplica os campos dados
            if let Some(k) = req.ruler.as_ref().and_then(|p| p.get("kind")) {
                let kind: RulerKind = serde_json::from_value(k.clone()).map_err(|e| ToolError::Invalid(format!("ruler.kind: {e}")))?;
                if kind != cur.kind {
                    cur = convert(&cur, kind, req.points.unwrap_or(2) as usize, canvas);
                }
            }
            let mut r = match &req.ruler {
                Some(patch) => merge_ruler(&cur, patch)?,
                None => cur,
            };
            r.id = rid.clone();
            if let Some(n) = req.points {
                if !(1..=3).contains(&n) || r.kind != RulerKind::Perspective {
                    return Err(ToolError::Invalid("points (1..3) só vale para régua de perspectiva".into()));
                }
                set_points(&mut r, canvas, n as usize);
            }
            if let Some(y) = req.eye_level {
                if eye_level(&r, canvas).is_none() {
                    return Err(ToolError::Invalid("eye_level só vale para perspectiva com horizonte que não é vertical".into()));
                }
                set_eye_level(&mut r, canvas, y);
            }
            r.validate().map_err(ToolError::Invalid)?;
            *set.get_mut(&rid).expect("existe") = r;
            id = Some(rid);
            label = "Editar régua";
        }
        RulerOp::Delete => {
            let rid = need_id(&set)?;
            set.remove(&rid);
            label = "Apagar régua";
        }
        RulerOp::Activate => {
            let rid = need_id(&set)?;
            set.active = Some(rid.clone());
            id = Some(rid);
            label = "Ativar régua";
        }
        RulerOp::Bake => {
            let r = match &req.id {
                Some(_) => set.get(&need_id(&set)?).cloned(),
                None => set.active_ruler().cloned(),
            }
            .ok_or_else(|| ToolError::Invalid("a camada não tem régua para converter".into()))?;
            let cmd = bake_command(p, &cut, req.frame.unwrap_or(0), &req.layer, &r)?;
            return Ok(RulerPlan { command: Some(cmd), id: Some(r.id), set });
        }
    }
    let command = set_command(&req.layer, &set, label);
    Ok(RulerPlan { command: Some(command), set, id })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: P, b: P, tol: f32) -> bool {
        len(sub(a, b)) <= tol
    }

    fn two_point() -> Ruler {
        let mut r = Ruler::preset(RulerKind::Perspective, 2, (1920, 1080), Some([960.0, 500.0]), "r1", "");
        r.vps = vec![[-1000.0, 500.0], [3000.0, 500.0]];
        r
    }

    /// 2 pontos: o primeiro trecho escolhe o ponto de fuga certo e o traço projetado fica na reta que vai até ele.
    #[test]
    fn two_point_strokes_snap_to_the_right_vanishing_direction() {
        let r = two_point();
        let s = [1000.0, 800.0];
        // rumo à esquerda e um pouco para cima: VP0 em (-1000, 500)
        let d = pick_dir(&r, s, [-30.0, -6.0]).unwrap();
        let to_vp0 = unit(sub(r.vps[0], s)).unwrap();
        assert!(dot(d, to_vp0).abs() > 0.9999, "{d:?}");
        // um ponto qualquer do traço vai para a reta s → VP0
        let q = project(s, d, [700.0, 760.0]);
        assert!(cross(sub(q, s), sub(r.vps[0], s)).abs() / len(sub(r.vps[0], s)) < 1e-2, "fora da reta: {q:?}");
        // rumo à direita: VP1
        let d1 = pick_dir(&r, s, [40.0, -4.0]).unwrap();
        assert!(dot(d1, unit(sub(r.vps[1], s)).unwrap()).abs() > 0.9999);
        // quase vertical: a vertical do horizonte
        let dv = pick_dir(&r, s, [3.0, 50.0]).unwrap();
        assert!(dv[0].abs() < 1e-4 && dv[1].abs() > 0.9999, "{dv:?}");
        // 3 pontos: a vertical vira o terceiro ponto
        let mut r3 = r.clone();
        r3.vps.push([960.0, 4000.0]);
        let d3 = pick_dir(&r3, s, [3.0, 50.0]).unwrap();
        assert!(dot(d3, unit(sub(r3.vps[2], s)).unwrap()) > 0.9999);
    }

    #[test]
    fn grid_homography_maps_the_corners_and_extends_toward_the_horizon() {
        let mut g = Ruler::preset(RulerKind::Grid, 2, (1920, 1080), None, "r1", "");
        g.quad = [[700.0, 500.0], [1200.0, 500.0], [1500.0, 800.0], [400.0, 800.0]];
        g.cells = [10, 6];
        let h = grid_h(&g).unwrap();
        for (uv, q) in [[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]].iter().zip(g.quad) {
            let (x, y) = h.apply(uv[0], uv[1]);
            assert!(close([x, y], q, 1e-2), "{uv:?} → ({x}, {y}) ≠ {q:?}");
        }
        // as linhas laterais (u = 0 e u = 10) se encontram no horizonte do plano: estender v para trás aproxima as duas
        let (a0, a1) = (h.apply(0.0, -40.0), h.apply(10.0, -40.0));
        assert!((a1.0 - a0.0).abs() < 500.0 * 0.5, "convergem: {a0:?} {a1:?}");
        // grade com extensão: nenhuma linha atravessa o horizonte do plano (todas ficam abaixo dele)
        g.extend = 30;
        let ls = lines(&g, (1920, 1080), [-5000.0, -5000.0, 7000.0, 7000.0]);
        // as 71 linhas de u fixo aparecem (só encurtam no horizonte); das de v fixo, as que ficariam atrás do horizonte somem
        assert!(ls.len() > 71 + 7 && ls.len() < 71 + 67, "{}", ls.len());
        // horizonte do plano: as laterais se cruzam em y = 500 - 300·(500/600)... só confere que nada passa acima dele
        let horizon_y = {
            // interseção das laterais (u=0 e u=10)
            let (p, q) = ([700.0f32, 500.0f32], [400.0f32, 800.0f32]);
            let (r1, r2) = ([1200.0f32, 500.0f32], [1500.0f32, 800.0f32]);
            let (d1, d2) = (sub(q, p), sub(r2, r1));
            let t = cross(sub(r1, p), d2) / cross(d1, d2);
            add(p, mul(d1, t))[1]
        };
        for l in &ls {
            assert!(l.a[1] >= horizon_y - 1.0 && l.b[1] >= horizon_y - 1.0, "linha passa do horizonte {horizon_y}: {l:?}");
        }
        // encaixe na grade: as duas direções são as das linhas da grade que passam pelo ponto
        let ds = snap_dirs(&g, [950.0, 650.0]);
        assert_eq!(ds.len(), 2);
    }

    #[test]
    fn a_floor_grid_from_two_points_converges_on_the_vanishing_points() {
        let r = two_point();
        let q = floor_quad(&r, (1920, 1080)).unwrap();
        let mut g = Ruler::preset(RulerKind::Grid, 2, (1920, 1080), None, "r2", "");
        g.quad = q;
        let h = grid_h(&g).unwrap();
        // a linha u = 0 (O → B) e a v = 0 (O → A) apontam para VP1 e VP0
        let at = |u: f32, v: f32| {
            let (x, y) = h.apply(u, v);
            [x, y]
        };
        let (o, ua, vb) = (at(0.0, 0.0), at(8.0, 0.0), at(0.0, 8.0));
        let col = |a: P, b: P, c: P| cross(sub(b, a), sub(c, a)).abs() / len(sub(c, a)).max(1.0);
        assert!(col(o, ua, r.vps[1]) < 0.5, "v=0 não vai a VP1");
        assert!(col(o, vb, r.vps[0]) < 0.5, "u=0 não vai a VP0");
        // e as linhas interiores também (u = 4 converge em VP0)
        assert!(col(at(4.0, 0.0), at(4.0, 8.0), r.vps[0]) < 1.0);
        // o canto de fundo (sup-dir) fica mais perto do horizonte que o da frente (inf-esq)
        assert!(q[1][1] < q[3][1], "{q:?}");
    }

    #[test]
    fn symmetry_copies_mirror_and_rotate_around_the_center() {
        let mut s = Ruler::preset(RulerKind::Symmetry, 2, (1000, 1000), Some([500.0, 500.0]), "r1", "");
        s.angle = 90.0;
        s.count = 1;
        let m = symmetry_maps(&s);
        assert_eq!(m.len(), 1);
        assert!(close(apply_map(&m[0], [400.0, 300.0]), [600.0, 300.0], 1e-3), "espelho no eixo vertical");
        s.count = 2;
        let m = symmetry_maps(&s);
        assert_eq!(m.len(), 3, "2 eixos = 4 cópias com o original");
        let pts: Vec<P> = m.iter().map(|mm| apply_map(mm, [400.0, 300.0])).collect();
        for want in [[600.0, 300.0], [600.0, 700.0], [400.0, 700.0]] {
            assert!(pts.iter().any(|p| close(*p, want, 1e-2)), "falta {want:?} em {pts:?}");
        }
        s.mirror = false;
        s.count = 4;
        assert_eq!(symmetry_maps(&s).len(), 3, "só rotação: 3 cópias");
    }

    #[test]
    fn dragging_a_vanishing_point_keeps_the_horizon_level_unless_alt() {
        let r = two_point();
        let moved = drag(&r, Handle::Vp(0), r.vps[0], [-800.0, 450.0], false);
        assert!(close(moved.vps[0], [-800.0, 450.0], 1e-3));
        assert!(close(moved.vps[1], [3000.0, 450.0], 1e-3), "o outro ponto acompanha na vertical: {:?}", moved.vps[1]);
        let alt = drag(&r, Handle::Vp(0), r.vps[0], [-800.0, 450.0], true);
        assert!(close(alt.vps[1], r.vps[1], 1e-3), "com Alt só o ponto anda");
        let hz = drag(&r, Handle::Horizon, [960.0, 500.0], [970.0, 420.0], false);
        assert!(close(hz.vps[0], [-1000.0, 420.0], 1e-3) && close(hz.vps[1], [3000.0, 420.0], 1e-3));
        assert_eq!(eye_level(&hz, (1920, 1080)), Some(420.0));
        // arrastar um ponto para cima do outro não vale (horizonte indefinido)
        let bad = drag(&r, Handle::Vp(0), r.vps[0], r.vps[1], true);
        assert_eq!(bad, r);
        // alça mais perto do clique
        assert_eq!(hit_handle(&r, (1920, 1080), 80.0, [-995.0, 503.0], 12.0), Some(Handle::Vp(0)));
        assert_eq!(hit_handle(&r, (1920, 1080), 80.0, [960.0, 505.0], 12.0), Some(Handle::Horizon));
    }

    #[test]
    fn lines_stay_inside_the_clip_and_rays_are_stable_with_the_view() {
        let r = two_point();
        let clip = [0.0, 0.0, 1920.0, 1080.0];
        let ls = lines(&r, (1920, 1080), clip);
        assert!(ls.iter().filter(|l| l.strong).count() == 1, "um horizonte");
        for l in &ls {
            for p in [l.a, l.b] {
                assert!(p[0] >= -0.01 && p[0] <= 1920.01 && p[1] >= -0.01 && p[1] <= 1080.01, "{p:?}");
            }
        }
        // ver mais longe (clip maior) não muda os ângulos dos raios: só estende (cada raio do recorte pequeno continua
        // na reta de um raio do recorte grande)
        let wide = lines(&r, (1920, 1080), [-4000.0, -4000.0, 6000.0, 6000.0]);
        assert_eq!(wide.iter().filter(|l| !l.strong).count(), 48, "24 raios por ponto");
        for l in ls.iter().filter(|l| !l.strong) {
            let on_wide = wide.iter().filter(|w| !w.strong).any(|w| {
                let d = unit(sub(w.b, w.a)).unwrap();
                [l.a, l.b].iter().all(|p| cross(sub(*p, w.a), d).abs() < 0.5)
            });
            assert!(on_wide, "o raio {l:?} mudou de ângulo com o recorte");
        }
        // paralelas: espaçamento e a linha do meio é forte
        let mut pa = Ruler::preset(RulerKind::Parallel, 2, (1920, 1080), Some([960.0, 540.0]), "r2", "");
        pa.spacing = 100.0;
        let pl = lines(&pa, (1920, 1080), clip);
        assert_eq!(pl.len(), 11, "y = 40, 140, ..., 1040");
        assert_eq!(pl.iter().filter(|l| l.strong).count(), 1);
    }

    #[test]
    fn serde_round_trip_defaults_versions_and_unknown_fields() {
        let mut set = RulerSet::default();
        let id = set.add(two_point());
        set.add(Ruler::preset(RulerKind::Radial, 2, (1920, 1080), None, "", ""));
        assert_eq!(set.items[1].id, "r2");
        let v = json!({ PARAMS_KEY: set });
        let back = read_params(&v).unwrap();
        assert_eq!(back, set);
        assert_eq!(back.active_ruler().unwrap().id, "r2");
        // só o tipo: o resto vem do padrão
        let min = read_params(&json!({ "rulers": { "items": [ { "id": "a", "kind": "radial" } ] } })).unwrap();
        assert_eq!(min.version, VERSION);
        assert_eq!(min.items[0].kind, RulerKind::Radial);
        // chave desconhecida, versão futura e régua quebrada são erro (não somem)
        assert!(read_params(&json!({ "rulers": { "items": [ { "id": "a", "vanish": [] } ] } })).is_err());
        assert!(read_params(&json!({ "rulers": { "version": 99 } })).is_err());
        assert!(read_params(&json!({ "rulers": { "items": [ { "id": "a", "kind": "perspective", "vps": [] } ] } })).is_err());
        assert!(read_params(&json!({})).unwrap().items.is_empty());
        let mut s2 = set.clone();
        assert!(s2.remove(&id));
        s2.cycle();
        assert_eq!(s2.active.as_deref(), Some("r2"));
    }

    /// Linear: o ponto vai para o segmento (perpendicular) e para nas pontas, como no Clip Studio (o traço não passa da
    /// régua); a régua mais perto da partida ganha, e só dentro da tolerância.
    #[test]
    fn linear_ruler_projects_onto_the_segment_and_the_nearest_object_wins() {
        let mut lin = Ruler::preset(RulerKind::Linear, 2, (1920, 1080), None, "r1", "");
        lin.pts = vec![[100.0, 100.0], [600.0, 350.0]];
        lin.validate().unwrap();
        let t = Track::of(&lin).unwrap();
        let (p, s, d) = t.nearest([300.0, 250.0]);
        // na reta: y = 100 + 0,5·(x − 100)
        assert!((p[1] - (100.0 + 0.5 * (p[0] - 100.0))).abs() < 1e-3, "{p:?}");
        assert!((d - len(sub([300.0, 250.0], p))).abs() < 1e-4 && s > 0.0);
        // além da ponta: fica na ponta
        assert!(close(t.nearest([900.0, 400.0]).0, [600.0, 350.0], 1e-3));
        assert!(close(t.nearest([0.0, 0.0]).0, [100.0, 100.0], 1e-3));
        // escolha da régua: a 30 px encaixa, a 60 px não; com duas, a mais perto
        let mut lin2 = lin.clone();
        lin2.id = "r2".into();
        lin2.pts = vec![[100.0, 160.0], [600.0, 410.0]];
        let both = vec![lin.clone(), lin2];
        let n = [-0.5f32 / 1.25f32.sqrt(), 1.0 / 1.25f32.sqrt()]; // normal da régua, para baixo
        let at = |k: f32| add([350.0, 225.0], mul(n, k));
        assert_eq!(pick_object(&both[..1], at(30.0), SNAP_PX).map(|x| x.0), Some(0));
        assert!(pick_object(&both[..1], at(60.0), SNAP_PX).is_none());
        assert_eq!(pick_object(&both, at(35.0), SNAP_PX).map(|x| x.0), Some(1), "a segunda está a ~19 px");
        // sem encaixe, oculta ou de outro tipo: não conta
        let mut off = lin.clone();
        off.snap = false;
        assert!(pick_object(&[off], at(5.0), SNAP_PX).is_none());
        assert!(pick_object(&[two_point()], [960.0, 500.0], SNAP_PX).is_none());
    }

    /// Curva: passa pelos pontos clicados; o encaixe com continuidade não salta para a outra perna de um U estreito.
    #[test]
    fn curve_ruler_passes_through_its_points_and_snapping_keeps_continuity() {
        let mut c = Ruler::preset(RulerKind::Curve, 2, (1920, 1080), None, "r1", "");
        c.pts = vec![[100.0, 500.0], [300.0, 300.0], [500.0, 500.0], [700.0, 300.0]];
        c.validate().unwrap();
        let t = Track::of(&c).unwrap();
        for p in &c.pts {
            assert!(t.nearest(*p).2 < 0.05, "a curva não passa por {p:?}");
        }
        // U estreito: duas pernas verticais a 30 px uma da outra, unidas embaixo
        let mut u = c.clone();
        u.pts = vec![[100.0, 100.0], [100.0, 400.0], [115.0, 430.0], [130.0, 400.0], [130.0, 100.0]];
        let tu = Track::of(&u).unwrap();
        // começa na perna esquerda e a mão desvia para a direita no alto: sem continuidade iria para a outra perna
        let (_, mut s, _) = tu.nearest([100.0, 150.0]);
        let mut last = [100.0, 150.0];
        for q in [[104.0, 160.0], [110.0, 175.0], [117.0, 190.0], [119.0, 205.0]] {
            let (p, ns) = tu.nearest_near(q, s, len(sub(q, last)) * 2.0 + 8.0);
            assert!(p[0] < 105.0, "pulou de perna: {q:?} → {p:?}");
            s = ns;
            last = q;
        }
        assert!(tu.nearest([119.0, 205.0]).0[0] > 125.0, "sem continuidade iria para a direita");
        // fechada: dá a volta (o parâmetro passa do fim para o começo)
        let mut cl = c.clone();
        cl.closed = true;
        let tc = Track::of(&cl).unwrap();
        assert!(tc.closed && close(tc.at(tc.length() + 1.0), tc.at(1.0), 1e-2));
        let (_, s_end, _) = tc.nearest(cl.pts[0]);
        let (p, _) = tc.nearest_near(add(cl.pts[0], [-3.0, 3.0]), s_end, 10.0);
        assert!(len(sub(p, cl.pts[0])) < 6.0);
    }

    #[test]
    fn figure_outlines_project_onto_rect_ellipse_and_polygon() {
        let mut f = Ruler::preset(RulerKind::Figure, 2, (1920, 1080), Some([500.0, 400.0]), "r1", "");
        f.size = [200.0, 100.0];
        // retângulo: um ponto de dentro vai para o lado mais perto
        f.shape = FigureShape::Rect;
        let t = Track::of(&f).unwrap();
        assert!(t.closed);
        assert!(close(t.nearest([520.0, 480.0]).0, [520.0, 500.0], 1e-3));
        assert!(close(t.nearest([690.0, 390.0]).0, [700.0, 390.0], 1e-3));
        // girado 90°: a caixa deita de pé
        f.angle = 90.0;
        assert!(close(Track::of(&f).unwrap().nearest([500.0, 650.0]).0, [500.0, 600.0], 1e-2));
        // elipse: o ponto encaixado satisfaz a equação
        f.angle = 0.0;
        f.shape = FigureShape::Ellipse;
        let te = Track::of(&f).unwrap();
        for q in [[650.0, 420.0], [480.0, 290.0], [300.0, 400.0], [900.0, 700.0]] {
            let p = te.nearest(q).0;
            let e = ((p[0] - 500.0) / 200.0).powi(2) + ((p[1] - 400.0) / 100.0).powi(2);
            assert!((e - 1.0).abs() < 2e-3, "{q:?} → {p:?} ({e})");
        }
        // polígono de 5 lados: o primeiro vértice em cima; o ponto encaixado está num dos lados
        f.shape = FigureShape::Polygon;
        f.size = [100.0, 100.0];
        f.count = 5;
        f.validate().unwrap();
        let v = figure_pts(&f);
        assert_eq!(v.len(), 5);
        assert!(close(v[0], [500.0, 300.0], 1e-3));
        let tp = Track::of(&f).unwrap();
        assert!(tp.nearest([500.0, 290.0]).2 < 10.5);
        f.count = 2;
        assert!(f.validate().is_err());
    }

    #[test]
    fn concentric_keeps_the_starting_ring_and_guides_snap_along_their_line() {
        let mut c = Ruler::preset(RulerKind::Concentric, 2, (1920, 1080), Some([500.0, 500.0]), "r1", "");
        c.size = [200.0, 100.0];
        c.angle = 30.0;
        c.validate().unwrap();
        let start = [650.0, 560.0];
        let k = |p: P| {
            let l = rot(sub(p, c.center), -c.angle);
            ((l[0] / 200.0).powi(2) + (l[1] / 100.0).powi(2)).sqrt()
        };
        let k0 = k(start);
        for q in [[400.0, 300.0], [900.0, 900.0], [500.0, 510.0], [10.0, 500.0]] {
            let p = concentric_project(&c, start, q);
            assert!((k(p) - k0).abs() < 1e-3, "{q:?} → {p:?}: anel {} ≠ {k0}", k(p));
            // e na direção (no referencial da elipse) do ponto da mão
            let (lp, lq) = (rot(sub(p, c.center), -c.angle), rot(sub(q, c.center), -c.angle));
            assert!(cross([lp[0] / 200.0, lp[1] / 100.0], [lq[0] / 200.0, lq[1] / 100.0]).abs() < 1e-3);
        }
        // círculo: raio da partida
        c.size = [100.0, 100.0];
        let p = concentric_project(&c, [530.0, 540.0], [900.0, 500.0]);
        assert!(close(p, [550.0, 500.0], 1e-3), "{p:?}");
        assert!(concentric_ring(&c, [530.0, 540.0]).iter().all(|q| (len(sub(*q, c.center)) - 50.0).abs() < 1e-2));
        // guia horizontal em y = 300: o traço encaixa na linha de borda a borda
        let mut g = Ruler::preset(RulerKind::Guide, 2, (1920, 1080), Some([700.0, 300.0]), "r2", "");
        g.validate().unwrap();
        let tg = Track::of(&g).unwrap();
        assert!(close(tg.nearest([5000.0, 320.0]).0, [5000.0, 300.0], 1e-2));
        assert!(pick_object(&[g.clone()], [-200.0, 330.0], SNAP_PX).is_some(), "a guia vale em toda a largura");
        assert!(pick_object(&[g.clone()], [-200.0, 350.0], SNAP_PX).is_none());
        g.angle = 90.0;
        assert!(close(Track::of(&g).unwrap().nearest([710.0, 9.0]).0, [700.0, 9.0], 1e-2));
        // as linhas aparecem no recorte
        let ls = lines(&g, (1920, 1080), [0.0, 0.0, 1920.0, 1080.0]);
        assert_eq!(ls.len(), 1);
        assert!(close(ls[0].a, [700.0, 0.0], 1e-2) || close(ls[0].b, [700.0, 0.0], 1e-2));
        assert!(lines(&c, (1920, 1080), [0.0, 0.0, 1920.0, 1080.0]).len() > 100, "anéis + eixo");
    }

    #[test]
    fn ruler_pen_path_simplifies_to_a_few_points() {
        // meia senoide amostrada a cada px, com tremor de ±0,4 px
        let path: Vec<P> = (0..=600).map(|i| {
            let x = i as f32;
            [x, 300.0 + 100.0 * (x / 600.0 * PI).sin() + if i % 2 == 0 { 0.4 } else { -0.4 }]
        }).collect();
        let pts = simplify(&path, 2.0).unwrap();
        assert!(pts.len() >= 4 && pts.len() <= 20, "{}", pts.len());
        assert_eq!(pts[0], path[0]);
        assert_eq!(*pts.last().unwrap(), path[600]);
        let mut r = Ruler::preset(RulerKind::Curve, 2, (1920, 1080), None, "r1", "");
        r.pts = pts;
        r.validate().unwrap();
        // a curva fica perto do caminho da mão
        let t = Track::of(&r).unwrap();
        for q in path.iter().step_by(25) {
            assert!(t.nearest(*q).2 < 3.0, "{q:?} longe da curva");
        }
        assert!(simplify(&[[1.0, 1.0], [1.2, 1.1]], 2.0).is_none(), "caminho que não anda");
    }

    #[test]
    fn new_kinds_have_handles_that_drag_and_survive_serde() {
        let canvas = (1920, 1080);
        let mut lin = Ruler::preset(RulerKind::Linear, 2, canvas, None, "r1", "");
        let p1 = lin.pts[1];
        let moved = drag(&lin, Handle::Point(1), p1, add(p1, [10.0, 40.0]), false);
        assert!(close(moved.pts[1], add(p1, [10.0, 40.0]), 1e-3) && moved.pts[0] == lin.pts[0]);
        let body = drag(&lin, Handle::Body, [0.0, 0.0], [5.0, 5.0], false);
        assert!(close(body.pts[0], add(lin.pts[0], [5.0, 5.0]), 1e-3));
        // ponta em cima da outra: inválido, fica como estava
        assert_eq!(drag(&lin, Handle::Point(1), p1, lin.pts[0], false), lin);
        let mut fig = Ruler::preset(RulerKind::Figure, 2, canvas, Some([500.0, 500.0]), "r2", "");
        fig.angle = 90.0;
        let sz = drag(&fig, Handle::Size, [0.0, 0.0], [530.0, 700.0], false);
        assert!((sz.size[0] - 200.0).abs() < 1e-2 && (sz.size[1] - 30.0).abs() < 1e-2, "{:?}", sz.size);
        assert!(hit_handle(&fig, canvas, 80.0, add(fig.center, rot(fig.size, 90.0)), 4.0) == Some(Handle::Size));
        // serde: conjunto com os tipos novos volta igual; JSON antigo (sem os campos novos) carrega
        let mut set = RulerSet::default();
        lin.closed = false;
        set.add(lin);
        set.add(fig);
        let mut cur = Ruler::preset(RulerKind::Curve, 2, canvas, None, "", "");
        cur.closed = true;
        set.add(cur);
        set.add(Ruler::preset(RulerKind::Concentric, 2, canvas, None, "", ""));
        set.add(Ruler::preset(RulerKind::Guide, 2, canvas, None, "", ""));
        let back = read_params(&json!({ PARAMS_KEY: set })).unwrap();
        assert_eq!(back, set);
        let old = json!({ "rulers": { "version": 1, "active": "r1", "items": [ { "id": "r1", "name": "P", "kind": "perspective", "visible": true, "snap": true, "locked": false, "color": "#3aa0ffb4", "vps": [[-100.0, 400.0], [2000.0, 400.0]], "horizon_angle": 0.0, "density": 24, "center": [960.0, 486.0], "angle": 0.0, "spacing": 90.0, "count": 1, "mirror": true, "quad": [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], "cells": [8, 8], "extend": 4 } ] } });
        let o = read_params(&old).unwrap();
        assert_eq!((o.items[0].kind, o.items[0].vps.len()), (RulerKind::Perspective, 2));
        // só o tipo: a linear nasce com as duas pontas
        let min = read_params(&json!({ "rulers": { "items": [ { "id": "a", "kind": "linear" } ] } })).unwrap();
        assert_eq!(min.items[0].pts.len(), 2);
        assert!(read_params(&json!({ "rulers": { "items": [ { "id": "a", "kind": "linear", "pts": [[1.0, 1.0]] } ] } })).is_err());
        assert!(read_params(&json!({ "rulers": { "items": [ { "id": "a", "kind": "figure", "shape": "star" } ] } })).is_err());
        // conversões: linear → curva mantém as pontas; figura → concêntricos mantém a caixa
        let lc = convert(&set.items[0], RulerKind::Curve, 2, canvas);
        assert_eq!((lc.pts[0], lc.pts[2]), (set.items[0].pts[0], set.items[0].pts[1]));
        let fc = convert(&set.items[1], RulerKind::Concentric, 2, canvas);
        assert_eq!((fc.center, fc.size), (set.items[1].center, set.items[1].size));
        // assar: um traço por polilinha (a figura vira um traço só)
        assert_eq!(bake_strokes(&set.items[1], canvas).len(), 1);
    }

    #[test]
    fn changing_the_number_of_points_keeps_the_horizon() {
        let mut r = two_point();
        set_points(&mut r, (1920, 1080), 3);
        assert_eq!(r.vps.len(), 3);
        assert!(r.vps[2][1] > 2000.0, "o vertical fica embaixo: {:?}", r.vps[2]);
        set_points(&mut r, (1920, 1080), 1);
        assert_eq!(r.vps, vec![[-1000.0, 500.0]]);
        set_points(&mut r, (1920, 1080), 2);
        assert_eq!(r.vps[1][1], 500.0);
        assert!(r.vps[1][0] > 960.0, "o segundo nasce do outro lado: {:?}", r.vps[1]);
        r.validate().unwrap();
    }

    #[test]
    fn converting_the_type_keeps_the_place_horizon_and_colour() {
        let canvas = (1920, 1080);
        // 1 → 2: o segundo ponto espelhado em volta do meio, no mesmo horizonte
        let mut one = Ruler::preset(RulerKind::Perspective, 1, canvas, Some([400.0, 450.0]), "r1", "Minha");
        one.color = "#ff0000ff".into();
        let two = convert(&one, RulerKind::Perspective, 2, canvas);
        assert_eq!(two.vps.len(), 2);
        assert_eq!(two.vps[0], [400.0, 450.0]);
        assert!((two.vps[1][0] - 1520.0).abs() < 1e-3 && (two.vps[1][1] - 450.0).abs() < 1e-3, "espelho: {:?}", two.vps[1]);
        assert_eq!((two.id.as_str(), two.name.as_str(), two.color.as_str()), ("r1", "Minha", "#ff0000ff"));
        // 1 ponto no meio: o segundo não cai em cima do primeiro
        let mid = Ruler::preset(RulerKind::Perspective, 1, canvas, Some([960.0, 450.0]), "r1", "");
        let m2 = convert(&mid, RulerKind::Perspective, 2, canvas);
        assert!((m2.vps[1][0] - m2.vps[0][0]).abs() > 1000.0 && m2.vps[1][1] == 450.0);
        // 2 → 3 soma o vertical; 3 → 2 tira
        let three = convert(&two, RulerKind::Perspective, 3, canvas);
        assert_eq!(&three.vps[..2], &two.vps[..]);
        assert!(three.vps[2][1] > 2000.0);
        assert_eq!(convert(&three, RulerKind::Perspective, 2, canvas).vps, two.vps);
        // 2 → 1: um ponto no horizonte, no pé do meio do canvas
        let back = convert(&two_point(), RulerKind::Perspective, 1, canvas);
        assert_eq!(back.vps, vec![[960.0, 500.0]]);
        assert_eq!(back.horizon_angle, 0.0);
        // perspectiva → paralelas: padrões do tipo em volta do horizonte; cor padrão vira a do tipo novo
        let par = convert(&two_point(), RulerKind::Parallel, 2, canvas);
        assert_eq!((par.kind, par.center), (RulerKind::Parallel, [960.0, 500.0]));
        assert_eq!(par.color, Ruler::preset(RulerKind::Parallel, 2, canvas, None, "", "x").color);
        // perspectiva → grade: o chão coerente com os pontos de fuga
        let grid = convert(&two_point(), RulerKind::Grid, 2, canvas);
        assert_eq!(Some(grid.quad), floor_quad(&two_point(), canvas));
        // radial → simetria no mesmo centro, cor escolhida fica
        let mut rad = Ruler::preset(RulerKind::Radial, 2, canvas, Some([300.0, 200.0]), "r4", "");
        rad.color = "#112233ff".into();
        rad.locked = true;
        let sym = convert(&rad, RulerKind::Symmetry, 2, canvas);
        assert_eq!((sym.center, sym.color.as_str(), sym.locked), ([300.0, 200.0], "#112233ff", true));
        for r in [two, three, back, par, grid, sym] {
            r.validate().unwrap();
        }
    }
}
