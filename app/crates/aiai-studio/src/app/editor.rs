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

// ------------------------------------------------------------------------------------------------ seleção

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SelMode {
    #[default]
    Rect,
    Lasso,
    Ellipse,
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
}

impl Selection {
    pub(crate) fn rect_of(&self) -> Option<[f32; 4]> {
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
        }
    }

    /// Aplica o deslocamento às coordenadas (depois de um Enter).
    pub(crate) fn bake_offset(&mut self) {
        for p in &mut self.pts {
            *p += self.offset;
        }
        self.offset = Vec2::ZERO;
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

/// Centro da caixa do traço (a regra de `transform_command` para decidir se o traço está dentro da seleção).
pub(crate) fn stroke_center(s: &Stroke) -> Vec2 {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in &s.points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0)
}

// ------------------------------------------------------------------------------------------------ opções, gestos, reprodução

pub(crate) struct ToolOptions {
    pub pen_width: f32,
    pub pen_tip: VectorTip,
    pub pen_aliased: bool,
    /// Terminar perto do começo fecha o traço (sem pontas afinadas abrindo vão para o balde).
    pub pen_autoclose: bool,
    pub stab_mode: StabMode,
    pub stab_window: u32,
    /// Raio da corda do lazy brush em PONTOS da tela (vira pixels do desenho dividindo pela escala da visão).
    pub stab_radius: f32,
    pub brush_preset: usize,
    pub brush_size: f32,
    pub brush_opacity: f32,
    pub eraser_size: f32,
    pub fill_gap: f32,
    pub fill_expand: u32,
    pub select_mode: SelMode,
}

impl Default for ToolOptions {
    fn default() -> Self {
        ToolOptions {
            pen_width: 3.0,
            pen_tip: VectorTip::GPen,
            pen_aliased: false,
            pen_autoclose: true,
            stab_mode: StabMode::Average,
            stab_window: 4,
            stab_radius: 10.0,
            brush_preset: 1,
            brush_size: 24.0,
            brush_opacity: 1.0,
            eraser_size: 20.0,
            fill_gap: 3.0,
            fill_expand: 2,
            select_mode: SelMode::Rect,
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
}

pub(crate) enum Gesture {
    None,
    Pan { last: Pos2, button: egui::PointerButton },
    Rotate { last_angle: f32 },
    Stroke(Box<StrokeGesture>),
    Select { start: Vec2, pts: Vec<Vec2>, mode: SelMode },
    MoveSel { last: Vec2 },
    /// Conta-gotas: amostra ao apertar e enquanto arrasta.
    Eyedrop,
    /// Balde: o clique age ao soltar.
    FillClick,
}

/// Formas "fantasma": o que o gesto acabou de fazer, mostrado no overlay até o render novo chegar.
pub(crate) struct Ghost {
    pub pts: Vec<Vec2>,
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
    pub cut: Option<NodeId>,
    pub layer: Option<NodeId>,
    pub frame: u32,
    /// Cel escolhido no painel de camadas (o timesheet expõe este número ao clicar numa célula).
    pub cel_number: Option<u32>,
    /// Nó selecionado na árvore do projeto (para criar cena/corte dentro dele).
    pub sel_node: Option<NodeId>,
    pub color: ColorRef,
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
    /// Workspace Personagens: personagem selecionado, render da folha de modelo, formulários da folha e da consistência.
    pub chars: super::panels::characters::CharState,
    /// Pedido de troca de workspace feito por um painel (o app atende no fim do quadro).
    pub want_workspace: Option<crate::workspace::WorkspaceId>,
}

impl Default for Editor {
    fn default() -> Self {
        Editor {
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
            toast: None,
            pending_latency: None,
            latency: None,
            latency_log: Vec::new(),
            form_duration: 48,
            form_custom_size: false,
            form_size: [1920, 1080],
            rename: None,
            color_edit: None,
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
            points: pts.iter().map(|(x, y)| StrokePoint { x: *x, y: *y, pressure: 1.0, tilt: None, t: None }).collect(),
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
        let mut sel = Selection { mode: SelMode::Rect, pts: vec![Vec2::new(10.0, 10.0), Vec2::new(50.0, 40.0)], offset: Vec2::ZERO, whole: false };
        assert!(sel.contains(Vec2::new(30.0, 20.0)) && !sel.contains(Vec2::new(60.0, 20.0)));
        assert!(matches!(sel.shape(), Some(aiai_paint::select::SelectShape::Rect { .. })));
        sel.offset = Vec2::new(5.0, 0.0);
        sel.bake_offset();
        assert!(sel.contains(Vec2::new(54.0, 20.0)) && sel.offset == Vec2::ZERO);
        let tri = Selection { mode: SelMode::Lasso, pts: vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(50.0, 100.0)], offset: Vec2::ZERO, whole: false };
        assert!(tri.contains(Vec2::new(50.0, 30.0)) && !tri.contains(Vec2::new(5.0, 90.0)));
        let el = Selection { mode: SelMode::Ellipse, pts: vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 50.0)], offset: Vec2::ZERO, whole: false };
        assert!(el.contains(Vec2::new(50.0, 25.0)) && !el.contains(Vec2::new(2.0, 2.0)));
        let degenerate = Selection { mode: SelMode::Lasso, pts: vec![Vec2::ZERO, Vec2::new(1.0, 1.0)], offset: Vec2::ZERO, whole: false };
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
