//! Estado do editor (o que o usuário está fazendo, fora do documento): corte/camada/quadro ativos, cor atual, opções das
//! ferramentas, visão do canvas, seleção, reprodução e medidas de latência. O documento em si é a `CoreSession`; aqui só
//! vive o que não é desfazível. As contas puras (visão, estabilizador, acerto da borracha, seleção) ficam neste arquivo
//! sem depender de UI, com testes de unidade.

use super::commands::{self as cmds, NodeId};
use super::render_service::{RenderService, Rendered};
use crate::workspace::ToolId;
use aiai_core::*;
use egui::{Pos2, Rect, Vec2};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

// ------------------------------------------------------------------------------------------------ visão do canvas

pub(crate) const MIN_ZOOM: f32 = 0.1;
pub(crate) const MAX_ZOOM: f32 = 16.0;

/// Visão do canvas. `zoom` é em pixels FÍSICOS da tela por pixel do desenho (1.0 = 100%, independe do DPI/zoom da
/// interface); `center` é o ponto do desenho que fica no centro da área visível; `rot` em radianos (horário na tela).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct View {
    pub zoom: f32,
    pub center: Vec2,
    pub rot: f32,
    pub flip: bool,
}

impl Default for View {
    fn default() -> Self {
        View { zoom: 1.0, center: Vec2::ZERO, rot: 0.0, flip: false }
    }
}

/// Transformação desenho → tela (pontos lógicos do egui): `tela = origin + M · desenho`, com M = R(rot) · diag(±s, s).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Xf {
    pub origin: Pos2,
    /// [a, b, c, d]: x' = a·x + b·y; y' = c·x + d·y.
    pub m: [f32; 4],
    inv: [f32; 4],
    /// Pontos lógicos por pixel do desenho.
    pub scale: f32,
}

impl Xf {
    pub(crate) fn to_screen(&self, p: Vec2) -> Pos2 {
        Pos2::new(self.origin.x + self.m[0] * p.x + self.m[1] * p.y, self.origin.y + self.m[2] * p.x + self.m[3] * p.y)
    }

    pub(crate) fn to_canvas(&self, q: Pos2) -> Vec2 {
        let d = q - self.origin;
        Vec2::new(self.inv[0] * d.x + self.inv[1] * d.y, self.inv[2] * d.x + self.inv[3] * d.y)
    }

    /// Só a parte linear (deslocamentos), de tela para desenho.
    pub(crate) fn delta_to_canvas(&self, d: Vec2) -> Vec2 {
        Vec2::new(self.inv[0] * d.x + self.inv[1] * d.y, self.inv[2] * d.x + self.inv[3] * d.y)
    }
}

impl View {
    pub(crate) fn xf(&self, rect: Rect, ppp: f32) -> Xf {
        let s = self.zoom / ppp;
        let sx = if self.flip { -s } else { s };
        let (sn, cs) = self.rot.sin_cos();
        let m = [cs * sx, -sn * s, sn * sx, cs * s];
        let c = rect.center();
        let mut origin = Pos2::new(c.x - (m[0] * self.center.x + m[1] * self.center.y), c.y - (m[2] * self.center.x + m[3] * self.center.y));
        // sem rotação (ou múltiplos de 90°) a borda do desenho cai exatamente num pixel físico: nada de imagem borrada a 100%
        if (sn * cs).abs() < 1e-4 {
            origin.x = (origin.x * ppp).round() / ppp;
            origin.y = (origin.y * ppp).round() / ppp;
        }
        let det = m[0] * m[3] - m[1] * m[2];
        let det = if det.abs() < 1e-12 { 1e-12 } else { det };
        Xf { origin, m, inv: [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det], scale: s }
    }

    /// Enquadra o canvas inteiro (com margem em pontos) respeitando a rotação.
    pub(crate) fn fit(&mut self, rect: Rect, ppp: f32, canvas: (u32, u32), margin: f32) {
        let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
        let (sn, cs) = (self.rot.sin().abs(), self.rot.cos().abs());
        let (bw, bh) = (cw * cs + ch * sn, cw * sn + ch * cs);
        let aw = ((rect.width() - 2.0 * margin) * ppp).max(16.0);
        let ah = ((rect.height() - 2.0 * margin) * ppp).max(16.0);
        self.zoom = (aw / bw).min(ah / bh).clamp(MIN_ZOOM, MAX_ZOOM);
        self.center = Vec2::new(cw / 2.0, ch / 2.0);
    }

    pub(crate) fn set_zoom_at(&mut self, rect: Rect, ppp: f32, anchor: Pos2, zoom: f32) {
        let before = self.xf(rect, ppp).to_canvas(anchor);
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        // mantém o ponto do desenho sob o cursor: center' = p − M'⁻¹ · (âncora − centro da área)
        let xf = self.xf(rect, ppp);
        self.center = before - xf.delta_to_canvas(anchor - rect.center());
    }

    pub(crate) fn zoom_at(&mut self, rect: Rect, ppp: f32, anchor: Pos2, factor: f32) {
        self.set_zoom_at(rect, ppp, anchor, self.zoom * factor);
    }

    pub(crate) fn pan_by_screen(&mut self, rect: Rect, ppp: f32, delta: Vec2) {
        let xf = self.xf(rect, ppp);
        self.center -= xf.delta_to_canvas(delta);
    }

    /// Gira a visão em torno do centro da área (o ponto do desenho no centro não muda).
    pub(crate) fn rotate_by(&mut self, delta: f32) {
        self.rot = (self.rot + delta).rem_euclid(std::f32::consts::TAU);
    }

    pub(crate) fn rotation_deg(&self) -> f32 {
        let d = self.rot.to_degrees();
        if d > 180.0 { d - 360.0 } else { d }
    }
}

// ------------------------------------------------------------------------------------------------ estabilizador

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum StabMode {
    Off,
    /// Média móvel das últimas N amostras.
    #[default]
    Average,
    /// "Lazy brush": a ponta só anda quando o cursor passa do raio (corda).
    Lazy,
}

/// Estabiliza as amostras de um traço enquanto chegam. As unidades são do desenho (pixels do canvas).
#[derive(Debug, Clone)]
pub(crate) struct Stabilizer {
    mode: StabMode,
    window: usize,
    radius: f32,
    ring: VecDeque<Vec2>,
    pos: Option<Vec2>,
    last_raw: Option<Vec2>,
}

impl Stabilizer {
    pub(crate) fn new(mode: StabMode, window: usize, radius: f32) -> Self {
        Stabilizer { mode, window: window.clamp(1, 64), radius: radius.max(0.0), ring: VecDeque::new(), pos: None, last_raw: None }
    }

    /// Alimenta uma amostra bruta; devolve a posição estabilizada (ou nada, se a ponta não andou).
    pub(crate) fn feed(&mut self, p: Vec2) -> Option<Vec2> {
        self.last_raw = Some(p);
        match self.mode {
            StabMode::Off => Some(p),
            StabMode::Average => {
                self.ring.push_back(p);
                while self.ring.len() > self.window {
                    self.ring.pop_front();
                }
                let n = self.ring.len() as f32;
                let sum = self.ring.iter().fold(Vec2::ZERO, |a, b| a + *b);
                Some(sum / n)
            }
            StabMode::Lazy => match self.pos {
                None => {
                    self.pos = Some(p);
                    Some(p)
                }
                Some(cur) => {
                    let d = p - cur;
                    let len = d.length();
                    if len > self.radius {
                        let next = cur + d / len * (len - self.radius);
                        self.pos = Some(next);
                        Some(next)
                    } else {
                        None
                    }
                }
            },
        }
    }

    /// Fim do traço: alcança o cursor suavemente (a média converge repetindo o último ponto; a corda se recolhe).
    pub(crate) fn finish(&mut self) -> Vec<Vec2> {
        let Some(last) = self.last_raw else { return Vec::new() };
        let mut out = Vec::new();
        match self.mode {
            StabMode::Off => {}
            StabMode::Average => {
                for _ in 0..self.window {
                    if let Some(p) = self.feed(last) {
                        out.push(p);
                    }
                }
            }
            StabMode::Lazy => {
                if let Some(cur) = self.pos {
                    let d = last - cur;
                    let steps = (d.length() / 2.0).ceil().clamp(1.0, 40.0) as usize;
                    for i in 1..=steps {
                        out.push(cur + d * (i as f32 / steps as f32));
                    }
                }
            }
        }
        out
    }
}

// ------------------------------------------------------------------------------------------------ geometria

pub(crate) fn dist_point_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_sq();
    if l2 < 1e-12 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// O caminho do cursor (com raio) toca o traço? Usa a linha de centro já achatada do motor (`sample_stroke`), com a
/// largura variável de cada amostra: acerta quando a distância ≤ raio da borracha + raio local do traço.
pub(crate) fn stroke_hit(stroke: &Stroke, path: &[Vec2], radius: f32) -> bool {
    let samples = aiai_paint::sample_stroke(stroke);
    if samples.is_empty() || path.is_empty() {
        return false;
    }
    // densifica o caminho para não pular traços finos entre amostras distantes
    let step = radius.max(1.0);
    let mut pts: Vec<Vec2> = Vec::with_capacity(path.len());
    for (i, p) in path.iter().enumerate() {
        if i > 0 {
            let a = path[i - 1];
            let n = ((*p - a).length() / step).floor() as usize;
            for k in 1..=n {
                pts.push(a + (*p - a) * (k as f32 / (n + 1) as f32));
            }
        }
        pts.push(*p);
    }
    let seg = |i: usize| -> (Vec2, Vec2, f32) {
        let a = &samples[i];
        let b = &samples[(i + 1).min(samples.len() - 1)];
        (Vec2::new(a.x, a.y), Vec2::new(b.x, b.y), a.r.max(b.r))
    };
    pts.iter().any(|p| (0..samples.len()).any(|i| {
        let (a, b, r) = seg(i);
        dist_point_segment(*p, a, b) <= radius + r
    }))
}

/// O que o clique faz na ferramenta Editar pontos (itens do painel Ferramenta).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PointsMode {
    /// Escolhe e arrasta pontos (Ctrl+arrastar muda a largura; Shift+clique no traço insere; botão direito apaga).
    #[default]
    Move,
    /// Clique no traço põe um ponto ali (em cima da curva).
    Add,
    /// Clique num ponto o apaga.
    Delete,
    /// Clique num ponto alterna canto (quebrado) e suave (arredondado).
    Corner,
    /// Arrastar um ponto para cima/baixo engrossa/afina o traço naquele ponto.
    Width,
}

// ------------------------------------------------------------------------------------------------ seleção

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SelMode {
    #[default]
    Rect,
    Lasso,
    Ellipse,
    /// Varinha mágica: região contígua de cor parecida (pixels da camada ativa).
    Wand,
}

/// Forma da caneta: à mão livre, ou formas de régua (linha, curva por cliques, retângulo, elipse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PenShape {
    #[default]
    Freehand,
    Line,
    Curve,
    Rect,
    Ellipse,
}

/// Máscara da varinha (calculada pelo motor no clique) e a caixa dela.
#[derive(Debug)]
pub(crate) struct WandMask {
    pub w: u32,
    pub h: u32,
    pub cov: Vec<f32>,
    pub bbox: [f32; 4],
    pub tolerance: f32,
}

/// Seleção atual (em pixels do desenho) e o deslocamento ainda não aplicado (Enter aplica, Esc cancela).
#[derive(Debug, Clone)]
pub(crate) struct Selection {
    pub mode: SelMode,
    /// Rect/Elipse: dois cantos. Laço: o caminho.
    pub pts: Vec<Vec2>,
    pub offset: Vec2,
    /// Sem seleção de verdade: o Mover leva o conteúdo inteiro da camada.
    pub whole: bool,
    /// Alças do Mover: escala (x, y; negativa = espelhada) e rotação (graus, horário) em volta do centro original,
    /// ainda não aplicadas.
    pub scale: Vec2,
    pub rotate: f32,
    /// Varinha: máscara da região.
    pub mask: Option<std::sync::Arc<WandMask>>,
    /// Seleção de objetos (camada vetorial): os traços escolhidos com o Mover; `pts` = caixa deles. O Mover leva só
    /// esses traços (inteiros), não o que estiver dentro da caixa.
    pub strokes: Vec<aiai_core::Id>,
    /// Camada dos traços escolhidos (trocar de camada desfaz a escolha).
    pub strokes_layer: Option<aiai_core::Id>,
}

impl Selection {
    pub(crate) fn new(mode: SelMode, pts: Vec<Vec2>, whole: bool) -> Self {
        Selection { mode, pts, offset: Vec2::ZERO, whole, scale: Vec2::splat(1.0), rotate: 0.0, mask: None, strokes: Vec::new(), strokes_layer: None }
    }

    /// Centro da caixa da seleção (pivô da escala e da rotação), antes do deslocamento.
    pub(crate) fn center(&self) -> Vec2 {
        self.rect_of().map(|r| Vec2::new(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)).unwrap_or(Vec2::ZERO)
    }

    /// Transformação pendente (escala e rotação em volta do centro, depois o deslocamento).
    pub(crate) fn affine(&self) -> aiai_paint::Affine {
        let c = self.center();
        // M = T(c + offset) · R · S · T(-c), a mesma ordem do Deform::Affine do commit
        let (s, co) = self.rotate.to_radians().sin_cos();
        let (a, b, d, e) = (co * self.scale.x, -s * self.scale.y, s * self.scale.x, co * self.scale.y);
        aiai_paint::Affine([a, b, c.x + self.offset.x - (a * c.x + b * c.y), d, e, c.y + self.offset.y - (d * c.x + e * c.y)])
    }

    pub(crate) fn map(&self, p: Vec2) -> Vec2 {
        let (x, y) = self.affine().apply(p.x, p.y);
        Vec2::new(x, y)
    }

    pub(crate) fn is_identity(&self) -> bool {
        self.offset == Vec2::ZERO && self.plain_scale() && self.rotate.abs() < 1e-3
    }
    pub(crate) fn rect_of(&self) -> Option<[f32; 4]> {
        if let Some(m) = &self.mask {
            return (m.bbox[2] > 0.0 && m.bbox[3] > 0.0).then_some(m.bbox);
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in &self.pts {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
    }

    pub(crate) fn shape(&self) -> Option<aiai_paint::select::SelectShape> {
        use aiai_paint::select::SelectShape;
        match self.mode {
            SelMode::Rect => self.rect_of().map(|rect| SelectShape::Rect { rect }),
            SelMode::Ellipse => self.rect_of().map(|rect| SelectShape::Ellipse { rect }),
            SelMode::Lasso => (self.pts.len() >= 3).then(|| SelectShape::Lasso { points: self.pts.iter().map(|p| [p.x, p.y]).collect() }),
            SelMode::Wand => self.pts.first().zip(self.mask.as_ref()).map(|(p, m)| SelectShape::Wand { seed: [p.x, p.y], tolerance: m.tolerance, gap: 0.0 }),
        }
    }

    /// `p` (px do canvas, já com a transformação pendente) cai dentro da escolha?
    pub(crate) fn contains_mapped(&self, p: Vec2) -> bool {
        match self.affine().inverse() {
            Some(inv) => {
                let (x, y) = inv.apply(p.x, p.y);
                self.contains(Vec2::new(x, y))
            }
            None => false,
        }
    }

    pub(crate) fn contains(&self, p: Vec2) -> bool {
        match self.mode {
            SelMode::Rect => self.rect_of().is_some_and(|r| p.x >= r[0] && p.x <= r[0] + r[2] && p.y >= r[1] && p.y <= r[1] + r[3]),
            SelMode::Ellipse => self.rect_of().is_some_and(|r| {
                let (rx, ry) = (r[2] / 2.0, r[3] / 2.0);
                let (dx, dy) = ((p.x - (r[0] + rx)) / rx, (p.y - (r[1] + ry)) / ry);
                dx * dx + dy * dy <= 1.0
            }),
            SelMode::Lasso => point_in_polygon(p, &self.pts),
            SelMode::Wand => self.mask.as_ref().is_some_and(|m| {
                let (x, y) = (p.x.floor() as i64, p.y.floor() as i64);
                x >= 0 && y >= 0 && (x as u32) < m.w && (y as u32) < m.h && m.cov[(y as u32 * m.w + x as u32) as usize] >= 0.5
            }),
        }
    }

    /// Aplica a transformação pendente às coordenadas (depois de um Enter). Com escala/rotação, retângulo, elipse e
    /// varinha viram laço (o contorno transformado).
    pub(crate) fn bake_offset(&mut self) {
        if self.plain_scale() && self.rotate.abs() < 1e-3 && self.mask.is_none() {
            for p in &mut self.pts {
                *p += self.offset;
            }
        } else {
            let outline: Vec<Vec2> = match (self.mode, self.rect_of()) {
                (SelMode::Lasso, _) => self.pts.clone(),
                (SelMode::Ellipse, Some(r)) => (0..48)
                    .map(|i| {
                        let a = i as f32 / 48.0 * std::f32::consts::TAU;
                        Vec2::new(r[0] + r[2] / 2.0 * (1.0 + a.cos()), r[1] + r[3] / 2.0 * (1.0 + a.sin()))
                    })
                    .collect(),
                (_, Some(r)) => vec![Vec2::new(r[0], r[1]), Vec2::new(r[0] + r[2], r[1]), Vec2::new(r[0] + r[2], r[1] + r[3]), Vec2::new(r[0], r[1] + r[3])],
                _ => self.pts.clone(),
            };
            self.pts = outline.into_iter().map(|p| self.map(p)).collect();
            self.mode = SelMode::Lasso;
            self.mask = None;
        }
        self.offset = Vec2::ZERO;
        self.scale = Vec2::splat(1.0);
        self.rotate = 0.0;
    }

    /// Sem escala pendente.
    pub(crate) fn plain_scale(&self) -> bool {
        (self.scale - Vec2::splat(1.0)).length() < 1e-4
    }
}

pub(crate) fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

// ------------------------------------------------------------------------------------------------ opções, gestos, reprodução

pub(crate) struct ToolOptions {
    pub pen_width: f32,
    pub pen_tip: VectorTip,
    pub pen_aliased: bool,
    /// Terminar perto do começo fecha o traço (sem pontas afinadas abrindo vão para o balde).
    pub pen_autoclose: bool,
    pub pen_shape: PenShape,
    /// Mover em camada vetorial edita os pontos dos traços em vez de mover a seleção.
    pub stab_mode: StabMode,
    pub stab_window: u32,
    /// Raio da corda do lazy brush em PONTOS da tela (vira pixels do desenho dividindo pela escala da visão).
    pub stab_radius: f32,
    pub brush_preset: usize,
    pub brush_size: f32,
    pub brush_opacity: f32,
    /// Ajustes finos do pincel (cópia editada do preset escolhido); escolher outro preset descarta.
    pub brush_custom: Option<aiai_paint::brush::BrushPreset>,
    pub eraser_size: f32,
    /// Dureza da borracha raster (dura 0,9; macia 0).
    pub eraser_hardness: f32,
    /// Propriedades do pincel: todas (em vez de só as que o pincel usa).
    pub brush_show_all: bool,
    /// Borracha em camada vetorial: traço inteiro, trecho coberto ou até a interseção.
    pub eraser_mode: aiai_tools::erase::EraseMode,
    pub fill_gap: f32,
    pub fill_expand: u32,
    /// Balde fecha as frestas em volta (regiões sem tinta de até `LEFTOVER_PX`) com a cor que as cerca.
    pub fill_close_leftovers: bool,
    pub select_mode: SelMode,
    pub points_mode: PointsMode,
    /// Mover em camada vetorial: clicar num traço escolhe só ele (Shift soma); sem nada escolhido, leva a camada.
    pub move_pick: bool,
    /// Transformação: cantos mantêm a proporção (Shift libera), como o Photoshop atual; desligado = livre (Shift trava).
    pub keep_aspect: bool,
    /// Tolerância de cor da varinha (0..1).
    pub wand_tolerance: f32,
}

impl Default for ToolOptions {
    fn default() -> Self {
        ToolOptions {
            pen_width: 3.0,
            pen_tip: VectorTip::GPen,
            pen_aliased: false,
            pen_autoclose: true,
            pen_shape: PenShape::Freehand,
            stab_mode: StabMode::Average,
            stab_window: 4,
            stab_radius: 10.0,
            brush_preset: 1,
            brush_size: 24.0,
            brush_opacity: 1.0,
            brush_custom: None,
            eraser_size: 20.0,
            eraser_hardness: 0.9,
            brush_show_all: false,
            eraser_mode: aiai_tools::erase::EraseMode::Segment,
            fill_gap: 3.0,
            fill_expand: 2,
            fill_close_leftovers: false,
            select_mode: SelMode::Rect,
            points_mode: PointsMode::Move,
            move_pick: true,
            keep_aspect: false,
            wand_tolerance: 0.12,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Sample {
    pub p: Vec2,
    pub pressure: f32,
    pub t: f32,
}

pub(crate) struct StrokeGesture {
    pub tool: ToolId,
    pub layer: NodeId,
    pub samples: Vec<Sample>,
    pub stab: Stabilizer,
    pub t0: Instant,
    /// Borracha vetorial: traços que já seriam apagados (para destacar).
    pub hits: Vec<NodeId>,
    /// Caminho bruto do cursor (borracha).
    pub path: Vec<Vec2>,
    pub screen_scale: f32,
    /// Camada que se move no quadro (movimento de camada / transformação do cel): `fwd` leva do cel ao canvas e
    /// `inv` do canvas ao cel. As amostras e o caminho ficam no espaço do cel (o que é gravado); a prévia volta ao canvas.
    pub fwd: Option<aiai_paint::Affine>,
    pub inv: Option<aiai_paint::Affine>,
    /// Régua com encaixe ligado: o traço segue a reta escolhida pelo primeiro trecho (ou é repetido na simetria).
    pub snap: Option<super::rulers::Snap>,
}

impl StrokeGesture {
    /// Ponto do cel → canvas (para desenhar a prévia onde o render vai mostrar).
    pub(crate) fn to_canvas(&self, p: Vec2) -> Vec2 {
        match &self.fwd {
            Some(a) => {
                let (x, y) = a.apply(p.x, p.y);
                Vec2::new(x, y)
            }
            None => p,
        }
    }
}

pub(crate) enum Gesture {
    None,
    Pan { last: Pos2, button: egui::PointerButton },
    Rotate { last_angle: f32 },
    Stroke(Box<StrokeGesture>),
    Select { start: Vec2, pts: Vec<Vec2>, mode: SelMode },
    MoveSel { last: Vec2 },
    /// Mover em camada vetorial: caixa de seleção de traços (arrasto fora de qualquer traço); `add` = Shift.
    BoxPick { start: Vec2, cur: Vec2, add: bool },
    /// Editar pontos: arrasto de um ponto de controle (posições no espaço do cel; `width` = muda a largura no ponto).
    DragNode { idx: usize, start: Vec2, cur: Vec2, width: bool, t0: std::time::Instant },
    /// Editar pontos: arrasto de uma ponta da alça do ponto `idx` (`side` = +1 a de saída, -1 a de chegada).
    DragHandle { idx: usize, side: f32, cur: Vec2, t0: std::time::Instant },
    /// Caneta em forma de régua (linha, retângulo, elipse): do ponto de partida até o cursor.
    Shape { start: Vec2, cur: Vec2 },
    /// Caneta em curva por cliques: cada clique põe um ponto; Enter, duplo clique ou botão direito termina.
    PolyCurve { pts: Vec<Vec2>, last_click: std::time::Instant },
    /// Alça de canto do Mover: escala em volta do centro.
    ScaleSel { handle: usize, start_scale: Vec2, start_offset: Vec2, start_rotate: f32 },
    /// Alça de rotação do Mover.
    RotateSel { start_angle: f32, start_rot: f32 },
    /// Conta-gotas: amostra ao apertar e enquanto arrasta.
    Eyedrop,
    /// Balde: o clique age ao soltar.
    FillClick,
    /// Régua: arrasto de uma alça da régua ativa (`orig` = como estava no começo; a prévia fica em `rulers.preview`).
    RulerDrag { layer: NodeId, orig: Box<aiai_tools::ruler::Ruler>, handle: aiai_tools::ruler::Handle, start: Vec2 },
    /// Régua: arrasto no vazio que cria uma régua do tipo escolhido.
    RulerCreate { start: Vec2, cur: Vec2 },
}

/// Formas "fantasma": o que o gesto acabou de fazer, mostrado no overlay até o render novo chegar.
pub(crate) struct Ghost {
    pub pts: Vec<Vec2>,
    /// Raio de cada ponto (unidades do canvas) quando o fantasma é a geometria real do traço; vazio = largura fixa.
    pub radii: Vec<f32>,
    pub width: f32,
    pub color: egui::Color32,
    pub until_counter: u64,
    /// Se o render novo não chegar (erro), o fantasma some sozinho.
    pub born: Instant,
}

pub(crate) struct Playback {
    pub playing: bool,
    pub started: Instant,
    pub start_pos: u32,
    pub shown_times: VecDeque<Instant>,
    pub fps_real: f32,
    pub looping: bool,
    pub loop_in: u32,
    /// Último quadro (inclusive) do laço; 0 = até o fim do corte.
    pub loop_out: u32,
    pub last_shown: Option<u32>,
}

impl Default for Playback {
    fn default() -> Self {
        Playback { playing: false, started: Instant::now(), start_pos: 0, shown_times: VecDeque::new(), fps_real: 0.0, looping: true, loop_in: 0, loop_out: 0, last_shown: None }
    }
}

impl Playback {
    pub(crate) fn range(&self, duration: u32) -> (u32, u32) {
        let last = duration.saturating_sub(1);
        let hi = if self.loop_out == 0 { last } else { self.loop_out.min(last) };
        (self.loop_in.min(hi), hi)
    }

    pub(crate) fn note_shown(&mut self) {
        let now = Instant::now();
        self.shown_times.push_back(now);
        while self.shown_times.front().is_some_and(|t| now.duration_since(*t).as_secs_f32() > 1.0) {
            self.shown_times.pop_front();
        }
        self.fps_real = self.shown_times.len() as f32;
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Latency {
    pub label: String,
    pub tool_ms: f32,
    pub render_ms: f32,
    pub total_ms: f32,
}

pub(crate) struct PendingLatency {
    pub release_at: Instant,
    pub tool_ms: f32,
    pub counter: u64,
    pub label: String,
}

/// Imagem que o canvas está mostrando agora.
pub(crate) struct Shown {
    pub tex: egui::TextureHandle,
    pub rendered: Arc<Rendered>,
    pub nearest: bool,
}

pub(crate) struct Editor {
    /// Shift seguro neste quadro (formas da caneta: ângulo de 15°, quadrado, círculo).
    pub shift: bool,
    /// Alt apertado (escala a partir do centro na transformação).
    pub alt: bool,
    /// Sub View (imagens de referência do disco, fora do projeto).
    pub subview: super::panels::subview::SubViewState,
    /// Traço escolhido para editar pontos (Mover em "Editar pontos").
    pub node_stroke: Option<aiai_core::Id>,
    /// Camadas escolhidas além da ativa (Ctrl/Shift+clique no painel Camadas): apagar, agrupar e arrastar valem para todas.
    pub layer_sel: Vec<aiai_core::Id>,
    /// Arrasto de camadas no painel: as que vão (em ordem de cima para baixo).
    pub layer_drag: Option<Vec<aiai_core::Id>>,
    /// Ponto escolhido no traço (índice; o painel mostra e edita posição, largura e canto dele).
    pub node_sel: Option<usize>,
    /// Campo numérico do Editar pontos em edição (chave, valor): grava ao soltar/sair do campo (um passo de desfazer).
    pub node_field: Option<(&'static str, f32)>,
    /// Miniatura de traço de cada preset de pincel (feita pelo próprio motor, uma vez).
    /// Biblioteca de pincéis (fábrica + usuário) e estado do painel Pincéis.
    pub brushes: super::brushes::BrushLibrary,
    pub brush_panel: super::panels::brushes::BrushPanelState,
    /// Biblioteca de recursos (painéis Biblioteca e Prévia do recurso).
    pub library: super::library::LibraryState,
    /// Painel a trazer para a frente no próximo quadro (aba do layout).
    pub want_panel: Option<crate::layout::PanelKind>,
    /// Som ao vivo (reprodução e scrub da mixagem do corte).
    pub live: super::audio_out::AudioOut,
    pub cut: Option<NodeId>,
    pub layer: Option<NodeId>,
    pub frame: u32,
    /// Cel escolhido no painel de camadas (o timesheet expõe este número ao clicar numa célula).
    pub cel_number: Option<u32>,
    /// Nó selecionado na árvore do projeto (para criar cena/corte dentro dele).
    pub sel_node: Option<NodeId>,
    pub color: ColorRef,
    /// Prévia ao vivo do pincel/borracha (o resultado real do motor durante o arrasto).
    pub live_stroke: super::live::Live,
    /// Mover ao vivo: a camada separada do quadro, pronta para o arrasto.
    pub move_live: super::move_live::MoveLive,
    /// Ferramenta Texto: estilo do proximo texto, sessao de digitacao, arrasto e estado do painel Texto.
    pub text: super::text_tool::TextState,
    /// Último valor real da cor do pincel (para não perder a cor quando a paleta some).
    pub color_seen: Rgba,
    pub palette: Option<NodeId>,
    pub opts: ToolOptions,
    pub view: View,
    pub fit_pending: bool,
    /// A visão acompanha o tamanho da área (mantém o canvas inteiro visível) até o usuário dar zoom, mover ou girar.
    pub auto_fit: bool,
    pub last_canvas_size: Vec2,
    pub show_guides: bool,
    pub show_reference: bool,
    pub show_grid: bool,
    pub show_camera: bool,
    pub render: Option<RenderService>,
    pub shown: Option<Shown>,
    pub checker: Option<egui::TextureHandle>,
    pub checker_dark: bool,
    pub gesture: Gesture,
    pub selection: Option<Selection>,
    pub ghosts: Vec<Ghost>,
    pub playback: Playback,
    /// Retângulo da área do canvas no último quadro (para a bridge e para as contas de tela).
    pub canvas_rect: Option<Rect>,
    pub cursor: Option<Pos2>,
    pub pressure: f32,
    /// Curva de pressão das preferências (mínimo, expoente), aplicada a toda pressão de caneta lida.
    pub pressure_curve: (f32, f32),
    /// Já chegou pressão de caneta nesta sessão (mostrado nas Preferências).
    pub pen_seen: bool,
    pub toast: Option<String>,
    pub pending_latency: Option<PendingLatency>,
    pub latency: Option<Latency>,
    pub latency_log: Vec<Latency>,
    /// Atalhos em uso (cópia das preferências; o app atualiza quando o usuário muda).
    pub keys: crate::keymap::Keymap,
    /// Formulários do painel de projeto.
    pub form_duration: u32,
    pub form_custom_size: bool,
    pub form_size: [u32; 2],
    pub rename: Option<(NodeId, String)>,
    /// Edição de cor da paleta: (paleta, cor, nome, rgba) ainda não aplicada.
    pub color_edit: Option<(NodeId, NodeId, String, egui::Color32)>,
    /// Seletor de cor: matiz/saturação/valor guardados para a cor mostrada (no cinza a matiz não se perde).
    pub wheel_hsv: Option<(egui::Color32, [f32; 3])>,
    /// Rolagem e zoom do timeline.
    pub tl_scroll: Vec2,
    pub tl_cell_w: f32,
    pub tl_ctx_cell: Option<(NodeId, u32)>,
    pub lint_report: Option<String>,
    /// Nós da árvore do projeto recolhidos (o padrão é aberto).
    pub tree_closed: std::collections::HashSet<NodeId>,
    // ---- pré-produção (Roteiro e Storyboard): o documento aberto e a seleção (corrigidos por `actions::sync_*`)
    pub sel_script: Option<NodeId>,
    pub sel_scene: Option<Id>,
    pub sel_element: Option<Id>,
    pub sel_storyboard: Option<NodeId>,
    pub sel_panel: Option<Id>,
    /// Roteiro: mostrar todas as cenas em vez de só a selecionada.
    pub script_all: bool,
    /// Personagem escolhido para o próximo "Nova fala".
    pub script_speaker: Option<NodeId>,
    /// Elemento recém-criado que deve receber o cursor no próximo quadro.
    pub script_focus: Option<Id>,
    pub docs: super::actions::DocCache,
    pub thumbs: super::panels::board::ThumbCache,
    /// Workspace Produção: filtro, caches da tabela de cortes, lint, comparação de retake e formulários.
    pub prod: super::panels::production::ProdState,
    /// Workspace Edição: documento aberto, cabeçote, seleção, prévia em segundo plano, zoom da linha do tempo.
    pub edit: super::panels::edit::EditState,
    /// Workspace Entrega: preset, pedido e fila de exportação (uma por vez, em segundo plano).
    pub deliver: super::panels::deliver::DeliverState,
    /// Workspace Áudio: clipe selecionado, monitor do corte, durações dos WAV, última medição de loudness.
    pub audio: super::panels::audio::AudioState,
    /// Workspace Áudio (S24): forma de onda, rack, medidores, multipista, voz sintética e a reprodução deles.
    pub audio_ws: super::panels::audio_ws::AudioWsState,
    /// Workspace Personagens: personagem selecionado, render da folha de modelo, formulários da folha e da consistência.
    pub chars: super::panels::characters::CharState,
    /// Pedido de troca de workspace feito por um painel (o app atende no fim do quadro).
    pub want_workspace: Option<crate::workspace::WorkspaceId>,
    /// Corte de desenho aberto a partir de um painel do storyboard: o canvas mostra "Voltar ao storyboard".
    pub back_to_board: Option<NodeId>,
    /// Animação: mesa de luz, densidade da linha do tempo, cels-chave destravados no douga, Cena e Referências.
    pub anim: super::anim::AnimState,
    /// Réguas: mostrar, encaixe geral, tipo da próxima, camada de guia das réguas e a prévia do arrasto.
    pub rulers: super::rulers::RulerState,
    /// Camada 3D: modo do gizmo, arrasto em curso e prévia reduzida.
    pub m3d: super::model3d::M3dState,
    /// Composição (S24 3.1–3.5): viewer, Linha do tempo da composição, Efeitos e predefinições.
    pub comp: super::comp::CompState,
}

impl Default for Editor {
    fn default() -> Self {
        Editor {
            back_to_board: None,
            anim: Default::default(),
            rulers: Default::default(),
            m3d: Default::default(),
            comp: Default::default(),
            live_stroke: Default::default(),
            move_live: Default::default(),
            text: Default::default(),
            color_seen: Rgba::BLACK,
            subview: Default::default(),
            node_stroke: None,
            layer_sel: Vec::new(),
            layer_drag: None,
            node_sel: None,
            node_field: None,
            shift: false,
            alt: false,
            brushes: Default::default(),
            brush_panel: Default::default(),
            library: Default::default(),
            want_panel: None,
            live: super::audio_out::AudioOut::new(false),
            keys: crate::keymap::Keymap::default(),
            cut: None,
            layer: None,
            frame: 0,
            cel_number: None,
            sel_node: None,
            color: ColorRef::Direct { rgba: Rgba::BLACK },
            palette: None,
            opts: ToolOptions::default(),
            view: View::default(),
            fit_pending: true,
            auto_fit: true,
            last_canvas_size: Vec2::ZERO,
            show_guides: true,
            show_reference: true,
            show_grid: false,
            show_camera: true,
            render: None,
            shown: None,
            checker: None,
            checker_dark: true,
            gesture: Gesture::None,
            selection: None,
            ghosts: Vec::new(),
            playback: Playback::default(),
            canvas_rect: None,
            cursor: None,
            pressure: 1.0,
            pressure_curve: (0.0, 1.0),
            pen_seen: false,
            toast: None,
            pending_latency: None,
            latency: None,
            latency_log: Vec::new(),
            form_duration: 48,
            form_custom_size: false,
            form_size: [1920, 1080],
            rename: None,
            color_edit: None,
            wheel_hsv: None,
            tl_scroll: Vec2::ZERO,
            tl_cell_w: 14.0,
            tl_ctx_cell: None,
            lint_report: None,
            tree_closed: Default::default(),
            sel_script: None,
            sel_scene: None,
            sel_element: None,
            sel_storyboard: None,
            sel_panel: None,
            script_all: false,
            script_speaker: None,
            script_focus: None,
            docs: Default::default(),
            thumbs: Default::default(),
            prod: Default::default(),
            edit: Default::default(),
            deliver: Default::default(),
            audio: Default::default(),
            audio_ws: Default::default(),
            chars: Default::default(),
            want_workspace: None,
        }
    }
}

impl Editor {
    pub(crate) fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some(msg.into());
    }

    /// Corte ativo válido (cai para o primeiro corte do documento) e quadro dentro da duração.
    pub(crate) fn active_cut(&mut self, core: &CoreSession) -> Option<NodeId> {
        let p = &core.project;
        if self.cut.as_ref().is_none_or(|c| !cmds::is_cut(p, c)) {
            let new = cmds::first_cut(p);
            if new != self.cut {
                self.fit_pending = true;
                self.layer = None;
                self.shown = None;
                self.selection = None;
                self.gesture = Gesture::None;
                self.ghosts.clear();
                self.playback.playing = false;
            }
            self.cut = new;
        }
        let cut = self.cut.clone()?;
        if let Some(cd) = cmds::cut_data(p, &cut) {
            self.frame = self.frame.min(cd.duration.saturating_sub(1));
        }
        Some(cut)
    }

    pub(crate) fn set_cut(&mut self, id: NodeId) {
        if self.cut.as_ref() != Some(&id) {
            self.cut = Some(id);
            self.layer = None;
            self.frame = 0;
            self.fit_pending = true;
            self.shown = None;
            self.selection = None;
            self.gesture = Gesture::None;
            self.playback.playing = false;
            self.cel_number = None;
        }
    }

    /// Camada ativa válida (do corte ativo); sem escolha, a de cima.
    pub(crate) fn active_layer(&mut self, core: &CoreSession) -> Option<NodeId> {
        let cut = self.active_cut(core)?;
        let p = &core.project;
        let ok = self.layer.as_ref().is_some_and(|l| cmds::layer_data(p, l).is_some() && p.cut_of(l).as_ref() == Some(&cut));
        if !ok {
            self.layer = cmds::layer_rows(p, &cut).first().map(|r| r.id.clone());
        }
        self.layer.clone()
    }

    /// Aplica um comando do usuário; erro vira aviso. Marca o início da medida de latência.
    pub(crate) fn apply(&mut self, core: &mut CoreSession, cmd: Command) -> bool {
        match cmds::run(core, cmd) {
            Ok(o) => {
                self.toast = Some(o.description);
                true
            }
            Err(e) => {
                self.toast = Some(e);
                false
            }
        }
    }

    pub(crate) fn begin_latency(&mut self, release_at: Instant, label: &str, core: &CoreSession) {
        self.pending_latency = Some(PendingLatency { release_at, tool_ms: release_at.elapsed().as_secs_f32() * 1000.0, counter: core.change_counter, label: label.to_owned() });
    }

    pub(crate) fn set_color_direct(&mut self, rgba: Rgba) {
        self.color = ColorRef::Direct { rgba };
    }

    /// A cor do pincel pode apontar para uma paleta que não existe neste projeto (outro projeto aberto, sessão
    /// antiga, paleta apagada): vira cor direta com o último valor visto, senão o traço é gravado em magenta
    /// ("cor de paleta ausente") enquanto a prévia mostra preto. Roda a cada quadro (uma busca de nó).
    pub(crate) fn sanitize_color(&mut self, p: &Project) {
        let live = |pal: &NodeId| match p.node(pal).map(|n| &n.data) {
            Some(NodeKind::Palette(pd)) => Some(pd),
            _ => None,
        };
        match &self.color {
            ColorRef::Direct { rgba } => self.color_seen = *rgba,
            ColorRef::Palette { palette, color } => match live(palette).and_then(|pd| pd.resolve(color)) {
                Some(c) => self.color_seen = c,
                None => self.color = ColorRef::Direct { rgba: self.color_seen },
            },
        }
        if self.palette.as_ref().is_some_and(|pl| live(pl).is_none()) {
            self.palette = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(800.0, 500.0))
    }

    #[test]
    fn screen_canvas_round_trip_at_any_dpi_rotation_and_flip() {
        for ppp in [1.0, 1.25, 1.5, 2.0] {
            for rot in [0.0, 0.3, std::f32::consts::FRAC_PI_2, 3.0] {
                for flip in [false, true] {
                    let v = View { zoom: 1.7, center: Vec2::new(300.0, 200.0), rot, flip };
                    let xf = v.xf(rect(), ppp);
                    for p in [Vec2::new(0.0, 0.0), Vec2::new(1919.0, 1079.0), Vec2::new(333.5, 77.25)] {
                        let back = xf.to_canvas(xf.to_screen(p));
                        assert!((back - p).length() < 1e-2, "ppp {ppp} rot {rot} flip {flip}: {p:?} -> {back:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn one_hundred_percent_maps_one_canvas_pixel_to_one_physical_pixel() {
        for ppp in [1.0f32, 1.25, 1.5] {
            let v = View { zoom: 1.0, center: Vec2::new(960.0, 540.0), rot: 0.0, flip: false };
            let xf = v.xf(rect(), ppp);
            let a = xf.to_screen(Vec2::new(10.0, 10.0));
            let b = xf.to_screen(Vec2::new(11.0, 10.0));
            assert!(((b.x - a.x) * ppp - 1.0).abs() < 1e-4, "1 px do desenho = 1 px físico");
            // a origem da imagem cai na grade de pixels físicos
            let o = xf.origin;
            assert!(((o.x * ppp) - (o.x * ppp).round()).abs() < 1e-3 && ((o.y * ppp) - (o.y * ppp).round()).abs() < 1e-3);
        }
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor_fixed() {
        let ppp = 1.25;
        let mut v = View { zoom: 0.5, center: Vec2::new(960.0, 540.0), rot: 0.4, flip: true };
        let anchor = Pos2::new(333.0, 222.0);
        let before = v.xf(rect(), ppp).to_canvas(anchor);
        for f in [1.5, 1.5, 0.3, 4.0, 40.0] {
            v.zoom_at(rect(), ppp, anchor, f);
            let after = v.xf(rect(), ppp).to_canvas(anchor);
            // o ponto muda só pela rotação do snap de pixel: tolerância de meio pixel físico no zoom atual
            assert!((after - before).length() < 0.6 / v.zoom.max(0.1) + 0.05, "{before:?} {after:?} zoom {}", v.zoom);
        }
        assert!(v.zoom <= MAX_ZOOM && v.zoom >= MIN_ZOOM);
        v.set_zoom_at(rect(), ppp, anchor, 0.001);
        assert_eq!(v.zoom, MIN_ZOOM);
    }

    #[test]
    fn fit_shows_the_whole_canvas() {
        let mut v = View::default();
        v.fit(rect(), 1.0, (1920, 1080), 16.0);
        let xf = v.xf(rect(), 1.0);
        let (a, b) = (xf.to_screen(Vec2::ZERO), xf.to_screen(Vec2::new(1920.0, 1080.0)));
        assert!(a.x >= rect().min.x && a.y >= rect().min.y && b.x <= rect().max.x && b.y <= rect().max.y, "{a:?} {b:?}");
        v.rot = 0.5;
        v.fit(rect(), 1.0, (1920, 1080), 16.0);
        let xf = v.xf(rect(), 1.0);
        for c in [Vec2::ZERO, Vec2::new(1920.0, 0.0), Vec2::new(1920.0, 1080.0), Vec2::new(0.0, 1080.0)] {
            assert!(rect().expand(1.0).contains(xf.to_screen(c)), "{c:?} fora com a rotação");
        }
    }

    #[test]
    fn stabilizers_smooth_and_reach_the_end() {
        let zigzag: Vec<Vec2> = (0..40).map(|i| Vec2::new(i as f32 * 3.0, if i % 2 == 0 { 0.0 } else { 6.0 })).collect();
        for mode in [StabMode::Average, StabMode::Lazy] {
            let mut st = Stabilizer::new(mode, 6, 8.0);
            let mut out: Vec<Vec2> = zigzag.iter().filter_map(|p| st.feed(*p)).collect();
            out.extend(st.finish());
            let amp = |v: &[Vec2]| v.windows(2).map(|w| (w[1].y - w[0].y).abs()).sum::<f32>() / v.len() as f32;
            assert!(amp(&out) < amp(&zigzag) * 0.6, "{mode:?} não suavizou ({} vs {})", amp(&out), amp(&zigzag));
            let end = *out.last().unwrap();
            assert!((end - *zigzag.last().unwrap()).length() < 4.0, "{mode:?} terminou longe do cursor: {end:?}");
        }
        let mut off = Stabilizer::new(StabMode::Off, 1, 0.0);
        assert_eq!(off.feed(Vec2::new(5.0, 5.0)), Some(Vec2::new(5.0, 5.0)));
        assert!(off.finish().is_empty());
    }

    fn stroke(pts: &[(f32, f32)], width: f32) -> Stroke {
        Stroke {
            id: Id::from("s"),
            points: pts.iter().map(|(x, y)| StrokePoint { x: *x, y: *y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
            geometry: StrokeGeometry::Polyline,
            tension: 0.0,
            tip: VectorTip::Marker,
            width,
            profile: WidthProfile::default(),
            color: ColorRef::default(),
            opacity: 1.0,
            closed: false,
            aliased: false,
            color_trace: false,
        }
    }

    #[test]
    fn eraser_hits_use_radius_plus_half_width() {
        let s = stroke(&[(0.0, 50.0), (100.0, 50.0)], 6.0);
        // marcador: raio local = 3; borracha raio 5 → acerta até 8 px da linha
        assert!(stroke_hit(&s, &[Vec2::new(50.0, 43.0)], 5.0));
        assert!(!stroke_hit(&s, &[Vec2::new(50.0, 41.0)], 5.0), "9 px > 5 + 3");
        assert!(!stroke_hit(&s, &[Vec2::new(50.0, 30.0)], 5.0));
        // caminho que atravessa o traço sem amostra em cima dele
        assert!(stroke_hit(&s, &[Vec2::new(50.0, 0.0), Vec2::new(50.0, 100.0)], 2.0), "segmento longo atravessando");
        assert!(!stroke_hit(&s, &[], 5.0));
    }

    #[test]
    fn selection_contains_and_shapes() {
        let mut sel = Selection::new(SelMode::Rect, vec![Vec2::new(10.0, 10.0), Vec2::new(50.0, 40.0)], false);
        assert!(sel.contains(Vec2::new(30.0, 20.0)) && !sel.contains(Vec2::new(60.0, 20.0)));
        assert!(matches!(sel.shape(), Some(aiai_paint::select::SelectShape::Rect { .. })));
        sel.offset = Vec2::new(5.0, 0.0);
        sel.bake_offset();
        assert!(sel.contains(Vec2::new(54.0, 20.0)) && sel.offset == Vec2::ZERO);
        let tri = Selection::new(SelMode::Lasso, vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(50.0, 100.0)], false);
        assert!(tri.contains(Vec2::new(50.0, 30.0)) && !tri.contains(Vec2::new(5.0, 90.0)));
        let el = Selection::new(SelMode::Ellipse, vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 50.0)], false);
        assert!(el.contains(Vec2::new(50.0, 25.0)) && !el.contains(Vec2::new(2.0, 2.0)));
        let degenerate = Selection::new(SelMode::Lasso, vec![Vec2::ZERO, Vec2::new(1.0, 1.0)], false);
        assert!(degenerate.shape().is_none());
    }

    #[test]
    fn playback_range_clamps_to_the_cut() {
        let mut pb = Playback::default();
        assert_eq!(pb.range(24), (0, 23));
        pb.loop_in = 4;
        pb.loop_out = 10;
        assert_eq!(pb.range(24), (4, 10));
        pb.loop_out = 99;
        assert_eq!(pb.range(24), (4, 23));
        pb.loop_in = 50;
        assert_eq!(pb.range(24), (23, 23));
    }
}
